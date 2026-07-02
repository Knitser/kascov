//! Covenant detection: turn observed transactions into covenant sightings.

use serde::Serialize;

use crate::model::*;

/// A covenant-bound output observed in an accepted or scanned transaction.
#[derive(Clone, Debug, Serialize)]
pub struct CovenantSighting {
    pub covenant_id: CovenantId,
    pub outpoint: Outpoint,
    pub authorizing_input: u16,
    /// Outpoint spent by the authorizing input — the covenant's previous state
    /// UTXO for continuations, or the funding outpoint for a genesis.
    pub authorizing_outpoint: Option<Outpoint>,
    pub value: u64,
    pub spk_version: u16,
    #[serde(serialize_with = "hex_ser")]
    pub spk_script: Vec<u8>,
    pub tx_version: u16,
    pub block_hash: BlockHash,
    pub daa_score: u64,
}

fn hex_ser<S: serde::Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&hex::encode(bytes))
}

/// Extract every covenant-bound output from a block's transactions.
pub fn covenant_sightings(block: &Block) -> Vec<CovenantSighting> {
    let mut sightings = Vec::new();
    for tx in &block.transactions {
        for (index, output) in tx.outputs.iter().enumerate() {
            let Some(binding) = output.covenant else { continue };
            sightings.push(CovenantSighting {
                covenant_id: binding.covenant_id,
                outpoint: Outpoint { txid: tx.txid, index: index as u32 },
                authorizing_input: binding.authorizing_input,
                authorizing_outpoint: tx
                    .inputs
                    .get(binding.authorizing_input as usize)
                    .map(|input| input.previous_outpoint),
                value: output.value,
                spk_version: output.spk_version,
                spk_script: output.spk_script.clone(),
                tx_version: tx.version,
                block_hash: block.hash,
                daa_score: block.daa_score,
            });
        }
    }
    sightings
}
