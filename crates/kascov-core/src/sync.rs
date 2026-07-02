//! Acceptance-driven sync: follow the virtual selected parent chain, classify
//! covenant activity in accepted transactions, and keep the store's cursor
//! moving in lockstep.

use std::collections::{HashMap, HashSet};

use futures::stream::{FuturesUnordered, StreamExt};

use crate::model::*;
use crate::node::NodeHandle;
use crate::store::{BlockEvents, EventKind, NewEvent, NewUtxo, Store};
use crate::Result;

#[derive(Clone, Copy, Debug, Default)]
pub struct SyncStats {
    pub chain_blocks: u64,
    pub events: u64,
    pub reorged_out: u64,
}

/// How often the cursor advances through event-less chain blocks.
const CHECKPOINT_EVERY: u64 = 500;

/// Process all virtual chain changes since the stored cursor (or `from`, or the
/// current sink for a fresh index). Returns once caught up.
pub async fn sync_once(
    node: &NodeHandle,
    store: &mut Store,
    from: Option<BlockHash>,
    mut progress: impl FnMut(&SyncStats),
) -> Result<SyncStats> {
    let mut stats = SyncStats::default();

    let cursor = match store.cursor()? {
        Some(cursor) => cursor,
        None => {
            // Fresh index: start from `from`, or the current sink.
            let start = match from {
                Some(from) => from,
                None => node.dag_info().await?.sink,
            };
            tracing::info!("fresh index, starting at {start}");
            store.apply(&BlockEvents::empty(start), start)?;
            start
        }
    };

    let step = node.virtual_chain_from(cursor).await?;
    if !step.removed.is_empty() {
        stats.reorged_out = step.removed.len() as u64;
        tracing::info!("reorg: rolling back {} chain blocks", step.removed.len());
        store.rollback(&step.removed)?;
    }

    let mut since_checkpoint = 0u64;
    let mut last_seen: Option<BlockHash> = None;

    for accepted in &step.added {
        stats.chain_blocks += 1;
        since_checkpoint += 1;
        last_seen = Some(accepted.accepting_block);

        let accepting = node.block_with_txs(accepted.accepting_block).await?;
        let wanted: HashSet<TxId> = accepted.accepted_tx_ids.iter().copied().collect();

        // Resolve accepted transaction bodies: the accepting block's own txs
        // first, then its mergeset blocks for the rest.
        let mut bodies: HashMap<TxId, Transaction> = HashMap::new();
        for tx in &accepting.transactions {
            if wanted.contains(&tx.txid) {
                bodies.insert(tx.txid, tx.clone());
            }
        }
        if bodies.len() < wanted.len() {
            let mut fetches: FuturesUnordered<_> = accepting
                .mergeset
                .iter()
                .map(|&hash| async move { node.block_with_txs(hash).await })
                .collect();
            while let Some(block) = fetches.next().await {
                let Ok(block) = block else { continue };
                for tx in block.transactions {
                    if wanted.contains(&tx.txid) {
                        bodies.insert(tx.txid, tx);
                    }
                }
            }
        }

        let block_events = classify(
            store,
            accepted,
            accepting.daa_score,
            accepted.accepted_tx_ids.iter().filter_map(|id| bodies.get(id)),
        )?;

        if !block_events.events.is_empty() {
            stats.events += block_events.events.len() as u64;
            for event in &block_events.events {
                tracing::info!(
                    "{:?} covenant {} in tx {}",
                    event.kind,
                    event.covenant_id,
                    event.txid
                );
            }
            store.apply(&block_events, accepted.accepting_block)?;
            since_checkpoint = 0;
        } else if since_checkpoint >= CHECKPOINT_EVERY {
            store.apply(&block_events, accepted.accepting_block)?;
            since_checkpoint = 0;
        }

        if stats.chain_blocks % 100 == 0 {
            progress(&stats);
        }
    }

    // Final checkpoint so the next run resumes at the tip.
    if since_checkpoint > 0 {
        if let Some(cursor) = last_seen {
            store.apply(&BlockEvents::empty(cursor), cursor)?;
        }
    }
    Ok(stats)
}

/// Classify the covenant activity of one accepting chain block's accepted txs.
fn classify<'a>(
    store: &Store,
    accepted: &AcceptedBlock,
    accepting_daa: u64,
    txs: impl Iterator<Item = &'a Transaction>,
) -> Result<BlockEvents> {
    let mut block_events = BlockEvents {
        accepting_block: accepted.accepting_block,
        accepting_daa,
        events: vec![],
        created_utxos: vec![],
        spent_utxos: vec![],
    };
    // Overlay for intra-block chains: a tx spending a covenant UTXO created by
    // an earlier tx in the same accepting block.
    let mut created_overlay: HashMap<Outpoint, CovenantId> = HashMap::new();
    let mut known_overlay: HashSet<CovenantId> = HashSet::new();

    for tx in txs {
        // covenant_id -> (spent utxos, created outputs)
        let mut touched: HashMap<CovenantId, (u32, u32)> = HashMap::new();

        for input in &tx.inputs {
            let id = match created_overlay.get(&input.previous_outpoint) {
                Some(&id) => Some(id),
                None => store.live_covenant_utxo(&input.previous_outpoint)?,
            };
            if let Some(id) = id {
                touched.entry(id).or_default().0 += 1;
                block_events.spent_utxos.push((input.previous_outpoint, tx.txid));
            }
        }
        for (index, output) in tx.outputs.iter().enumerate() {
            let Some(binding) = output.covenant else { continue };
            let outpoint = Outpoint { txid: tx.txid, index: index as u32 };
            touched.entry(binding.covenant_id).or_default().1 += 1;
            created_overlay.insert(outpoint, binding.covenant_id);
            block_events.created_utxos.push(NewUtxo {
                outpoint,
                covenant_id: binding.covenant_id,
                value: output.value,
                spk_version: output.spk_version,
                spk_script: output.spk_script.clone(),
            });
        }

        for (covenant_id, (spent, created)) in touched {
            let kind = if spent > 0 && created > 0 {
                EventKind::Transition
            } else if spent > 0 {
                EventKind::Burn
            } else if known_overlay.contains(&covenant_id) || store.known_covenant(&covenant_id)? {
                // An output claims an existing id without spending its UTXO
                // here — record as a transition rather than a second genesis.
                EventKind::Transition
            } else {
                EventKind::Genesis
            };
            known_overlay.insert(covenant_id);
            block_events.events.push(NewEvent { covenant_id, kind, txid: tx.txid });
        }
    }
    Ok(block_events)
}
