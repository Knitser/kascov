//! kascov's own stable data model. Kaspa RPC types are mapped into these at the
//! `node::wrpc` boundary and never appear elsewhere.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

macro_rules! hash32 {
    ($name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(#[serde(with = "hex_bytes")] pub [u8; 32]);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&hex::encode(self.0))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self)
            }
        }

        impl FromStr for $name {
            type Err = crate::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let mut bytes = [0u8; 32];
                hex::decode_to_slice(s, &mut bytes).map_err(|_| crate::Error::Invalid {
                    what: stringify!($name),
                    value: s.to_string(),
                })?;
                Ok(Self(bytes))
            }
        }
    };
}

hash32!(BlockHash);
hash32!(TxId);
hash32!(CovenantId);

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(&s, &mut bytes).map_err(serde::de::Error::custom)?;
        Ok(bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Outpoint {
    pub txid: TxId,
    pub index: u32,
}

impl fmt::Display for Outpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.txid, self.index)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    Mainnet,
    Testnet(u32),
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Network::Mainnet => f.write_str("mainnet"),
            Network::Testnet(suffix) => write!(f, "testnet-{suffix}"),
        }
    }
}

impl FromStr for Network {
    type Err = crate::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "mainnet" => Ok(Network::Mainnet),
            _ => s
                .strip_prefix("testnet-")
                .and_then(|n| n.parse().ok())
                .map(Network::Testnet)
                .ok_or_else(|| crate::Error::Invalid { what: "network", value: s.to_string() }),
        }
    }
}

/// Covenant binding on a transaction output (KIP-20).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CovenantBinding {
    pub covenant_id: CovenantId,
    pub authorizing_input: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Output {
    pub value: u64,
    pub spk_version: u16,
    #[serde(with = "serde_bytes_hex")]
    pub spk_script: Vec<u8>,
    pub covenant: Option<CovenantBinding>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Input {
    pub previous_outpoint: Outpoint,
    /// The unlocking script — for P2SH spends its final push reveals the
    /// actual program a covenant ran (spend-time decoding).
    #[serde(with = "serde_bytes_hex", default)]
    pub signature_script: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub txid: TxId,
    pub version: u16,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Block {
    pub hash: BlockHash,
    pub daa_score: u64,
    pub timestamp_ms: u64,
    pub parents: Vec<BlockHash>,
    /// Blocks merged by this block (blues + reds), when the node provided
    /// verbose data. Needed to resolve accepted transactions' bodies.
    pub mergeset: Vec<BlockHash>,
    pub transactions: Vec<Transaction>,
}

/// One step of the virtual selected chain, as reported by the node.
#[derive(Clone, Debug)]
pub struct ChainStep {
    pub removed: Vec<BlockHash>,
    pub added: Vec<AcceptedBlock>,
}

/// A chain block together with the transactions it accepted.
#[derive(Clone, Debug)]
pub struct AcceptedBlock {
    pub accepting_block: BlockHash,
    pub accepted_tx_ids: Vec<TxId>,
}

#[derive(Clone, Debug)]
pub struct DagInfo {
    pub network: String,
    pub sink: BlockHash,
    pub virtual_daa_score: u64,
    pub pruning_point: BlockHash,
}

#[derive(Clone, Debug)]
pub struct ServerInfo {
    pub version: String,
    pub network: String,
    pub is_synced: bool,
    pub has_utxo_index: bool,
}

mod serde_bytes_hex {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}
