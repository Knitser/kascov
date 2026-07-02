//! The only module allowed to import kaspa-* types. Everything is mapped to
//! `crate::model` at this boundary so upstream churn stays contained here.

use std::time::Duration;

use kaspa_consensus_core::network::{NetworkId, NetworkType};
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_rpc_core::{RpcBlock, RpcHash, RpcTransaction};
use kaspa_wrpc_client::{
    client::{ConnectOptions, ConnectStrategy},
    KaspaRpcClient, Resolver, WrpcEncoding,
};

use crate::model::*;
use crate::{Error, Result};

pub struct NodeHandle {
    client: KaspaRpcClient,
    network: Network,
}

impl NodeHandle {
    /// Connect to a node. With `url = None` the public resolver is used to
    /// discover a node for the given network.
    pub async fn connect(network: Network, url: Option<&str>) -> Result<Self> {
        let network_id = match network {
            Network::Mainnet => NetworkId::new(NetworkType::Mainnet),
            Network::Testnet(suffix) => NetworkId::with_suffix(NetworkType::Testnet, suffix),
        };
        let resolver = url.is_none().then(Resolver::default);

        let client = KaspaRpcClient::new(WrpcEncoding::Borsh, url, resolver, Some(network_id), None)
            .map_err(|e| Error::Connect(e.to_string()))?;

        let options = ConnectOptions {
            block_async_connect: true,
            connect_timeout: Some(Duration::from_millis(10_000)),
            strategy: ConnectStrategy::Fallback,
            ..Default::default()
        };
        client.connect(Some(options)).await.map_err(|e| Error::Connect(e.to_string()))?;

        let handle = Self { client, network };
        let info = handle.server_info().await?;
        if info.network != network.to_string() {
            return Err(Error::NodeMismatch(format!(
                "node is on {} but kascov was asked for {network}",
                info.network
            )));
        }
        Ok(handle)
    }

    pub fn network(&self) -> Network {
        self.network
    }

    pub async fn server_info(&self) -> Result<ServerInfo> {
        let info = self.client.get_server_info().await.map_err(rpc_err)?;
        Ok(ServerInfo {
            version: info.server_version,
            network: info.network_id.to_string(),
            is_synced: info.is_synced,
            has_utxo_index: info.has_utxo_index,
        })
    }

    pub async fn dag_info(&self) -> Result<DagInfo> {
        let info = self.client.get_block_dag_info().await.map_err(rpc_err)?;
        Ok(DagInfo {
            network: info.network.to_string(),
            sink: from_hash(info.sink),
            virtual_daa_score: info.virtual_daa_score,
            pruning_point: from_hash(info.pruning_point_hash),
        })
    }

    pub async fn block_with_txs(&self, hash: BlockHash) -> Result<Block> {
        let block = self.client.get_block(to_hash(hash), true).await.map_err(rpc_err)?;
        Ok(map_block(block))
    }

    /// Virtual selected chain changes since `cursor`, with accepted tx ids.
    pub async fn virtual_chain_from(&self, cursor: BlockHash) -> Result<ChainStep> {
        let response = self
            .client
            .get_virtual_chain_from_block(to_hash(cursor), true, None)
            .await
            .map_err(rpc_err)?;
        Ok(ChainStep {
            removed: response.removed_chain_block_hashes.into_iter().map(from_hash).collect(),
            added: response
                .accepted_transaction_ids
                .into_iter()
                .map(|accepted| AcceptedBlock {
                    accepting_block: from_hash(accepted.accepting_block_hash),
                    accepted_tx_ids: accepted
                        .accepted_transaction_ids
                        .into_iter()
                        .map(|id| TxId(id.as_bytes()))
                        .collect(),
                })
                .collect(),
        })
    }
}

fn rpc_err(e: kaspa_rpc_core::RpcError) -> Error {
    Error::Rpc(e.to_string())
}

fn from_hash(hash: RpcHash) -> BlockHash {
    BlockHash(hash.as_bytes())
}

fn to_hash(hash: BlockHash) -> RpcHash {
    RpcHash::from_bytes(hash.0)
}

fn map_block(block: RpcBlock) -> Block {
    let mergeset = block
        .verbose_data
        .as_ref()
        .map(|verbose| {
            verbose
                .merge_set_blues_hashes
                .iter()
                .chain(verbose.merge_set_reds_hashes.iter())
                .map(|h| from_hash(*h))
                .collect()
        })
        .unwrap_or_default();
    Block {
        hash: from_hash(block.header.hash),
        daa_score: block.header.daa_score,
        timestamp_ms: block.header.timestamp,
        parents: block.header.direct_parents().iter().map(|h| from_hash(*h)).collect(),
        mergeset,
        transactions: block.transactions.into_iter().map(map_tx).collect(),
    }
}

fn map_tx(tx: RpcTransaction) -> Transaction {
    let txid = tx
        .verbose_data
        .as_ref()
        .map(|v| TxId(v.transaction_id.as_bytes()))
        .unwrap_or(TxId([0; 32]));
    Transaction {
        txid,
        version: tx.version,
        inputs: tx
            .inputs
            .into_iter()
            .map(|input| Input {
                previous_outpoint: Outpoint {
                    txid: TxId(input.previous_outpoint.transaction_id.as_bytes()),
                    index: input.previous_outpoint.index,
                },
                signature_script: input.signature_script,
                compute_budget: input.compute_budget,
            })
            .collect(),
        outputs: tx
            .outputs
            .into_iter()
            .map(|output| Output {
                value: output.value,
                spk_version: output.script_public_key.version(),
                spk_script: output.script_public_key.script().to_vec(),
                covenant: output.covenant.map(|binding| CovenantBinding {
                    covenant_id: CovenantId(binding.0.covenant_id.as_bytes()),
                    authorizing_input: binding.0.authorizing_input,
                }),
            })
            .collect(),
        payload: tx.payload,
    }
}

/// Recompute a KIP-20 covenant id from its genesis outpoint and authorized
/// outputs `(global index, value, spk version, spk script)` — the binding
/// itself is excluded by construction. Calls the consensus implementation
/// from the pinned rusty-kaspa rev, so it can never drift from the chain.
pub fn compute_covenant_id(
    genesis_outpoint: &Outpoint,
    auth_outputs: &[(u32, u64, u16, &[u8])],
) -> CovenantId {
    use kaspa_consensus_core::hashing::covenant_id::covenant_id;
    use kaspa_consensus_core::tx::{ScriptPublicKey, TransactionOutpoint, TransactionOutput};

    let outpoint =
        TransactionOutpoint::new(RpcHash::from_bytes(genesis_outpoint.txid.0), genesis_outpoint.index);
    let outputs: Vec<(u32, TransactionOutput)> = auth_outputs
        .iter()
        .map(|&(index, value, version, script)| {
            (index, TransactionOutput::new(value, ScriptPublicKey::from_vec(version, script.to_vec())))
        })
        .collect();
    CovenantId(covenant_id(outpoint, outputs.iter().map(|(i, o)| (*i, o))).as_bytes())
}
