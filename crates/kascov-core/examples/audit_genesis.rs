//! One-off audit: re-fetch stored "genesis" transactions from a live node and
//! check whether the KIP-20 covenant id actually recomputes (the post-2d93eed
//! rule) for rows recorded under the old first-sighting-is-genesis rule.
//!
//! Usage: audit_genesis <network> <entries.json>
//! entries.json: [{"covenant_id": hex, "txid": hex, "accepting_block": hex}, ...]

use std::collections::HashMap;

use kascov_core::model::{Network, Outpoint, Transaction, TxId};
use kascov_core::node::{compute_covenant_id, NodeHandle};
use kascov_core::{BlockHash, CovenantId};

#[derive(serde::Deserialize)]
struct Entry {
    covenant_id: String,
    txid: String,
    accepting_block: String,
}

fn is_valid_genesis(tx: &Transaction, id: &CovenantId) -> Result<bool, String> {
    let bound: Vec<(u32, &kascov_core::model::Output)> = tx
        .outputs
        .iter()
        .enumerate()
        .filter(|(_, o)| o.covenant.is_some_and(|b| b.covenant_id == *id))
        .map(|(i, o)| (i as u32, o))
        .collect();
    let Some(&(_, first)) = bound.first() else {
        return Err("no bound outputs".into());
    };
    let auth = first.covenant.expect("filtered").authorizing_input;
    if bound.iter().any(|(_, o)| o.covenant.expect("filtered").authorizing_input != auth) {
        return Ok(false);
    }
    let Some(input) = tx.inputs.get(auth as usize) else {
        return Ok(false);
    };
    let fields: Vec<(u32, u64, u16, &[u8])> = bound
        .iter()
        .map(|&(i, o)| (i, o.value, o.spk_version, o.spk_script.as_slice()))
        .collect();
    let _ = Outpoint { txid: TxId([0; 32]), index: 0 }; // keep import
    Ok(compute_covenant_id(&input.previous_outpoint, &fields) == *id)
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let network: Network = args.next().expect("network").parse().expect("bad network");
    let path = args.next().expect("entries.json path");
    let entries: Vec<Entry> =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");

    let node = NodeHandle::connect(network, None).await.expect("connect");
    eprintln!("connected to {network}");

    let mut valid = 0u32;
    let mut invalid = 0u32;
    let mut missing = 0u32;
    for e in &entries {
        let cov: CovenantId = e.covenant_id.parse().expect("covenant_id");
        let txid: TxId = e.txid.parse().expect("txid");
        let block_hash: BlockHash = e.accepting_block.parse().expect("accepting_block");

        let block = match node.block_with_txs(block_hash).await {
            Ok(b) => b,
            Err(err) => {
                println!("MISSING  {} block fetch failed: {err}", &e.covenant_id[..12]);
                missing += 1;
                continue;
            }
        };
        let mut bodies: HashMap<TxId, Transaction> = HashMap::new();
        for tx in &block.transactions {
            bodies.insert(tx.txid, tx.clone());
        }
        if !bodies.contains_key(&txid) {
            for &m in &block.mergeset {
                if let Ok(mb) = node.block_with_txs(m).await {
                    for tx in mb.transactions {
                        bodies.insert(tx.txid, tx);
                    }
                }
                if bodies.contains_key(&txid) {
                    break;
                }
            }
        }
        let Some(tx) = bodies.get(&txid) else {
            println!("MISSING  {} tx {} not found in accepting block/mergeset", &e.covenant_id[..12], &e.txid[..12]);
            missing += 1;
            continue;
        };
        match is_valid_genesis(tx, &cov) {
            Ok(true) => {
                valid += 1;
                println!("VALID    {} genesis tx {}", &e.covenant_id[..12], &e.txid[..12]);
            }
            Ok(false) => {
                invalid += 1;
                println!("INVALID  {} genesis tx {} — id does not recompute (mid-life first sighting?)", &e.covenant_id[..12], &e.txid[..12]);
                if std::env::var("KASCOV_AUDIT_DEBUG").is_ok() {
                    println!("  tx version {} inputs:", tx.version);
                    for (i, input) in tx.inputs.iter().enumerate() {
                        println!("    #{i} prev {}:{}", input.previous_outpoint.txid, input.previous_outpoint.index);
                    }
                    for (i, o) in tx.outputs.iter().enumerate() {
                        match o.covenant {
                            Some(b) => println!(
                                "    out #{i} value {} bound to {} auth_input {}",
                                o.value, b.covenant_id, b.authorizing_input
                            ),
                            None => println!("    out #{i} value {} (unbound)", o.value),
                        }
                    }
                    // recompute over every input as candidate auth outpoint
                    let bound: Vec<(u32, u64, u16, Vec<u8>)> = tx
                        .outputs
                        .iter()
                        .enumerate()
                        .filter(|(_, o)| o.covenant.is_some_and(|b| b.covenant_id == cov))
                        .map(|(i, o)| (i as u32, o.value, o.spk_version, o.spk_script.clone()))
                        .collect();
                    let fields: Vec<(u32, u64, u16, &[u8])> =
                        bound.iter().map(|(i, v, ver, s)| (*i, *v, *ver, s.as_slice())).collect();
                    for (i, input) in tx.inputs.iter().enumerate() {
                        let got = compute_covenant_id(&input.previous_outpoint, &fields);
                        println!("    recompute with input #{i}: {got}");
                    }
                }
            }
            Err(why) => {
                invalid += 1;
                println!("INVALID  {} genesis tx {} — {why}", &e.covenant_id[..12], &e.txid[..12]);
            }
        }
    }
    println!("---\nvalid: {valid}  invalid: {invalid}  missing: {missing}  total: {}", entries.len());
}
