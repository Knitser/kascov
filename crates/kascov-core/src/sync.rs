//! Acceptance-driven sync: follow the virtual selected parent chain, classify
//! covenant activity in accepted transactions, and keep the store's cursor
//! moving in lockstep.

use std::collections::{HashMap, HashSet};

use futures::stream::{FuturesUnordered, StreamExt};

use crate::model::*;
use crate::node::ChainSource;
use crate::store::{BlockEvents, EventKind, NewEvent, NewUtxo, Store};
use crate::Result;

#[derive(Clone, Copy, Debug, Default)]
pub struct SyncStats {
    pub chain_blocks: u64,
    pub events: u64,
    pub reorged_out: u64,
}

/// Live updates emitted while syncing.
#[derive(Clone, Debug)]
pub enum SyncUpdate {
    Progress(SyncStats),
    Reorg { rolled_back: u64 },
    Event { covenant_id: CovenantId, kind: EventKind, txid: TxId, accepting_daa: u64 },
}

/// How often the cursor advances through event-less chain blocks.
const CHECKPOINT_EVERY: u64 = 500;

/// Accepting blocks fetched ahead while earlier ones are processed. Keeps
/// catch-up throughput above the chain's block rate (fetches are WAN-bound;
/// TN10 alone produces ~10 blocks/s).
const FETCH_AHEAD: usize = 16;

/// Process all virtual chain changes since the stored cursor (or `from`, or the
/// current sink for a fresh index). Returns once caught up.
pub async fn sync_once(
    node: &impl ChainSource,
    store: &mut Store,
    from: Option<BlockHash>,
    mut updates: impl FnMut(SyncUpdate),
) -> Result<SyncStats> {
    let mut stats = SyncStats::default();

    // Note the chain tip (virtual DAA ↔ wall clock) so exports can date events
    // exactly. Advisory — a failed lookup only matters for a fresh index,
    // which needs the sink below.
    let dag = node.dag_info().await;
    if let Ok(dag) = &dag {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        store.set_tip(dag.virtual_daa_score, now_ms)?;
    }

    let cursor = match store.cursor()? {
        Some(cursor) => cursor,
        None => {
            // Fresh index: start from `from`, or the current sink.
            let start = match from {
                Some(from) => from,
                None => dag?.sink,
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
        updates(SyncUpdate::Reorg { rolled_back: stats.reorged_out });
    }

    let mut since_checkpoint = 0u64;
    let mut last_seen: Option<BlockHash> = None;
    let mut last_daa = 0u64;

    /* Prefetch accepting blocks concurrently (ordered) while the store work
       below stays strictly sequential per chain block. Items are moved into
       the stream so the fetch closure stays lifetime-free. */
    let mut prefetched = futures::stream::iter(step.added)
        .map(|accepted| async move {
            let block = node.block_with_txs(accepted.accepting_block).await;
            (accepted, block)
        })
        .buffered(FETCH_AHEAD);

    while let Some((accepted, block)) = prefetched.next().await {
        stats.chain_blocks += 1;
        since_checkpoint += 1;
        last_seen = Some(accepted.accepting_block);

        let accepting = block?;
        last_daa = accepting.daa_score;
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
        if bodies.len() < wanted.len() {
            // One sequential retry for transient RPC failures, then fail the
            // pass: advancing the cursor past unresolved bodies would drop
            // covenant events silently and permanently.
            for &hash in &accepting.mergeset {
                if bodies.len() == wanted.len() {
                    break;
                }
                if let Ok(block) = node.block_with_txs(hash).await {
                    for tx in block.transactions {
                        if wanted.contains(&tx.txid) {
                            bodies.insert(tx.txid, tx);
                        }
                    }
                }
            }
            if bodies.len() < wanted.len() {
                return Err(crate::Error::Rpc(format!(
                    "unresolved accepted tx bodies in chain block {} ({} of {}) — failing the pass to retry",
                    accepted.accepting_block,
                    bodies.len(),
                    wanted.len()
                )));
            }
        }

        let block_events = classify(
            store,
            &accepted,
            accepting.daa_score,
            accepted.accepted_tx_ids.iter().filter_map(|id| bodies.get(id)),
        )?;

        if !block_events.events.is_empty() {
            stats.events += block_events.events.len() as u64;
            for event in &block_events.events {
                updates(SyncUpdate::Event {
                    covenant_id: event.covenant_id,
                    kind: event.kind,
                    txid: event.txid,
                    accepting_daa: block_events.accepting_daa,
                });
            }
            store.apply(&block_events, accepted.accepting_block)?;
            since_checkpoint = 0;
        } else if since_checkpoint >= CHECKPOINT_EVERY {
            store.apply(&block_events, accepted.accepting_block)?;
            since_checkpoint = 0;
        }

        if stats.chain_blocks % 100 == 0 {
            updates(SyncUpdate::Progress(stats));
        }
    }

    // Final checkpoint so the next run resumes at the tip. It carries the
    // last walked block's real DAA so processed_daa (the indexer's honest
    // progress mark) advances every completed pass, even on stretches with
    // no covenant events — steady-state passes walk ~20 blocks and never
    // hit the mid-stream checkpoint above.
    if since_checkpoint > 0 {
        if let Some(cursor) = last_seen {
            let mut checkpoint = BlockEvents::empty(cursor);
            checkpoint.accepting_daa = last_daa;
            store.apply(&checkpoint, cursor)?;
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
                // The signature script is the spend-time reveal: for P2SH
                // states its final push is the program the covenant ran.
                block_events.spent_utxos.push((
                    input.previous_outpoint,
                    tx.txid,
                    input.signature_script.clone(),
                    input.compute_budget,
                ));
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
            } else if is_valid_genesis(tx, &covenant_id) {
                EventKind::Genesis
            } else {
                // First sighting that doesn't prove genesis (KIP-20 hash
                // mismatch): a covenant born before we started watching.
                // Recording it as a transition leaves lineage_complete false.
                EventKind::Transition
            };
            known_overlay.insert(covenant_id);
            block_events.events.push(NewEvent {
                covenant_id,
                kind,
                txid: tx.txid,
                payload: (!tx.payload.is_empty()).then(|| tx.payload.clone()),
                lane_namespace: crate::store::lane_namespace(&tx.payload),
            });
        }
    }
    Ok(block_events)
}

/// KIP-20 genesis proof: the id must recompute from the authorizing input's
/// previous outpoint plus this transaction's outputs bound to the id.
fn is_valid_genesis(tx: &Transaction, id: &CovenantId) -> bool {
    let bound: Vec<(u32, &crate::model::Output)> = tx
        .outputs
        .iter()
        .enumerate()
        .filter(|(_, o)| o.covenant.is_some_and(|b| b.covenant_id == *id))
        .map(|(i, o)| (i as u32, o))
        .collect();
    let Some(&(_, first)) = bound.first() else { return false };
    let auth = first.covenant.expect("filtered on Some").authorizing_input;
    if bound.iter().any(|(_, o)| o.covenant.expect("filtered on Some").authorizing_input != auth) {
        return false;
    }
    let Some(input) = tx.inputs.get(auth as usize) else { return false };
    let fields: Vec<(u32, u64, u16, &[u8])> = bound
        .iter()
        .map(|&(i, o)| (i, o.value, o.spk_version, o.spk_script.as_slice()))
        .collect();
    crate::node::compute_covenant_id(&input.previous_outpoint, &fields) == *id
}
