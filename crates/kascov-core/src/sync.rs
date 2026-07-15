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
    Event {
        covenant_id: CovenantId,
        kind: EventKind,
        txid: TxId,
        accepting_daa: u64,
        /// 0-based index in the accepting block's accepted-tx list.
        tx_index: u32,
    },
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
                let block = match block {
                    Ok(b) => b,
                    Err(e) => {
                        // Correctness is covered by the sequential retry below
                        // (which hard-fails the pass) — but count what we
                        // swallow here so a persistently flaky node is visible.
                        static MERGESET_FETCH_ERRORS: std::sync::atomic::AtomicU64 =
                            std::sync::atomic::AtomicU64::new(0);
                        let n = MERGESET_FETCH_ERRORS
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                            + 1;
                        tracing::warn!("mergeset block fetch failed ({n} total): {e}");
                        continue;
                    }
                };
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

        // Enumerate BEFORE the body filter so each index is the tx's position
        // in the node's accepted-tx list (acceptance = UTXO application
        // order); unresolved bodies hard-fail above, so none are skipped.
        let block_events = classify(
            store,
            &accepted,
            &accepting,
            accepted
                .accepted_tx_ids
                .iter()
                .enumerate()
                .filter_map(|(i, id)| bodies.get(id).map(|tx| (i as u32, tx))),
        )?;

        if !block_events.events.is_empty() {
            stats.events += block_events.events.len() as u64;
            for event in &block_events.events {
                updates(SyncUpdate::Event {
                    covenant_id: event.covenant_id,
                    kind: event.kind,
                    txid: event.txid,
                    accepting_daa: block_events.accepting_daa,
                    tx_index: event.tx_index,
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

/// Newest candidate anchors probed one by one — a shallow strand is the
/// common case.
const RE_ANCHOR_DENSE_PROBES: u64 = 8;
/// Candidate anchors sampled evenly through the WHOLE indexed DAA range.
/// Depth must come from DAA spacing, not row counts: on the production TN10
/// index the newest 400 distinct accepting blocks span only ~12 minutes of
/// chain. Total probes stay bounded (~24); each response is server-capped
/// (~2,480 blocks), so probing is cheap.
const RE_ANCHOR_SPREAD_PROBES: u64 = 16;

/// Outcome of a [`re_anchor`] attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReAnchor {
    /// The cursor walk works (and does something when we lag) — whatever is
    /// failing, it isn't the cursor. Nothing was touched.
    NotWedged,
    /// Re-anchored: rolled back everything above the anchor and repointed
    /// the cursor there.
    Anchored(BlockHash),
    /// The cursor is unwalkable and so is every sampled block of our own
    /// history — node-side data for our indexed span is gone. Nothing was
    /// touched; the caller owns the last resort.
    NothingWalkable,
}

/// Recovery for a STRANDED cursor: the block still exists on the node
/// (headers outlive walkability), but `virtual_chain_from` refuses it —
/// typically a branch abandoned by a deep testnet reorg, or a block past the
/// node's walk retention. Existence checks can't see this, so we probe
/// walkability directly: candidate anchors come from our own indexed history
/// (the newest few, then samples spread through the whole indexed range),
/// and the first one the node can walk from becomes the new cursor.
/// Everything indexed above it lived on the abandoned side and goes through
/// the same [`Store::rollback`] a witnessed reorg would get, including its
/// reorg_log entry.
///
/// Walkability is judged lag-aware: while the store is far behind the tip, a
/// truthful walk must return blocks, so an EMPTY "success" (how some nodes
/// answer a stranded cursor) counts as unwalkable rather than as health.
pub async fn re_anchor(node: &impl ChainSource, store: &mut Store) -> Result<ReAnchor> {
    let Some(cursor) = store.cursor()? else { return Ok(ReAnchor::NotWedged) };
    let lagging = match (store.processed_daa()?, store.tip()?) {
        (Some(processed), Some((tip, _))) => tip.saturating_sub(processed) > WEDGE_LAG_DAA,
        _ => false,
    };
    let walkable = |step: &ChainStep| {
        !(lagging && step.removed.is_empty() && step.added.is_empty())
    };
    if node.virtual_chain_from(cursor).await.is_ok_and(|step| walkable(&step)) {
        return Ok(ReAnchor::NotWedged);
    }
    let mut candidates = store.recent_accepting_blocks(RE_ANCHOR_DENSE_PROBES)?;
    candidates.extend(store.spread_accepting_blocks(RE_ANCHOR_SPREAD_PROBES)?);
    let mut probed = HashSet::new();
    for (anchor, anchor_daa) in candidates {
        if anchor == cursor || !probed.insert(anchor) {
            continue; // the cursor is already proven unwalkable above
        }
        if !node.virtual_chain_from(anchor).await.is_ok_and(|step| walkable(&step)) {
            continue;
        }
        let above = store.accepting_blocks_above(anchor_daa)?;
        if !above.is_empty() {
            tracing::info!(
                "re-anchor: rolling back {} accepting blocks above DAA {anchor_daa}",
                above.len()
            );
            store.rollback(&above)?;
        }
        // Cursor repoint carrying the anchor's own DAA (unlike reset_cursor's
        // bare repoint), so processed_daa is honest immediately instead of
        // overstating progress until the next completed pass.
        let mut checkpoint = BlockEvents::empty(anchor);
        checkpoint.accepting_daa = anchor_daa;
        store.apply(&checkpoint, anchor)?;
        return Ok(ReAnchor::Anchored(anchor));
    }
    Ok(ReAnchor::NothingWalkable)
}

/// Lag (virtual tip DAA ahead of processed DAA) beyond which a cursor that
/// stops advancing counts as wedged — ~30 minutes at TN10's 10 blocks/s.
pub const WEDGE_LAG_DAA: u64 = 18_000;
/// Consecutive no-progress passes (while lagging beyond [`WEDGE_LAG_DAA`])
/// before [`ProgressWatch`] demands recovery.
pub const WEDGE_PASSES: u32 = 10;

/// Success-that-does-nothing detector: some nodes answer a stranded cursor
/// with an EMPTY successful walk, so sync passes "succeed", error counters
/// stay zero, and the follower sleeps forever while the chain runs away.
/// Feed it (processed, tip) after each successful pass; it demands recovery
/// once the cursor has sat still for [`WEDGE_PASSES`] passes while more than
/// [`WEDGE_LAG_DAA`] behind the tip. Pure state machine — the caller owns
/// the clock and the recovery.
#[derive(Debug, Default)]
pub struct ProgressWatch {
    last_processed: Option<u64>,
    stuck_passes: u32,
}

/// What one successful sync pass told the [`ProgressWatch`].
#[derive(Clone, Copy, Debug, Default)]
pub struct PassVerdict {
    /// processed_daa moved — in either direction: a recovery rollback is the
    /// index acting, not a stall.
    pub advanced: bool,
    /// The wedge signature held for [`WEDGE_PASSES`]: attempt recovery now.
    pub demand_recovery: bool,
}

impl ProgressWatch {
    /// Record one successful pass's (processed_daa, tip_daa).
    pub fn observe(&mut self, processed: Option<u64>, tip: Option<u64>) -> PassVerdict {
        let advanced = processed.is_some() && processed != self.last_processed;
        if advanced {
            self.last_processed = processed;
        }
        let lagging = matches!((processed, tip), (Some(p), Some(t)) if t.saturating_sub(p) > WEDGE_LAG_DAA);
        if advanced || !lagging {
            self.stuck_passes = 0;
            return PassVerdict { advanced, demand_recovery: false };
        }
        self.stuck_passes += 1;
        if self.stuck_passes >= WEDGE_PASSES {
            // Re-arm: a recovery attempt that changes nothing earns another
            // full window before the next demand.
            self.stuck_passes = 0;
            return PassVerdict { advanced, demand_recovery: true };
        }
        PassVerdict { advanced, demand_recovery: false }
    }
}

/// One-shot backfill of `tx_index` onto event rows written before capture,
/// bounded by node retention: walk the selected chain from the pruning point
/// (the oldest block with acceptance data) to the store's sync cursor,
/// stamping each accepted tx's list position onto matching rows. UPDATE-only —
/// no block bodies are fetched. Rows older than the pruning point stay NULL
/// forever (their acceptance data is pruned); that is expected, not an error.
/// Progress persists in meta, so interrupted runs resume and completed runs
/// return in O(1). Returns how many rows were stamped this run.
pub async fn backfill_tx_index(node: &impl ChainSource, store: &mut Store) -> Result<u64> {
    if store.tx_index_backfill_done()? {
        return Ok(0);
    }
    let Some(stop) = store.cursor()? else {
        // Fresh index: every future row is written with capture.
        store.set_tx_index_backfill_done()?;
        return Ok(0);
    };
    let resume = store.tx_index_backfill_resume()?;
    let mut cursor = match resume {
        Some(hash) => hash,
        None => node.dag_info().await?.pruning_point,
    };
    let mut stamped = 0u64;
    loop {
        let step = match node.virtual_chain_from(cursor).await {
            Ok(step) => step,
            Err(e) => {
                // A stale resume point (pruned since the interrupted run)
                // restarts from the current pruning point instead of wedging;
                // re-walked blocks are cheap NULL-probe no-ops.
                let pruning_point = node.dag_info().await?.pruning_point;
                if cursor == pruning_point {
                    return Err(e);
                }
                cursor = pruning_point;
                node.virtual_chain_from(cursor).await?
            }
        };
        if step.added.is_empty() {
            break; // reached the chain tip — everything reachable is stamped
        }
        // The node caps each response (mergeset_size_limit * 10 merged
        // blocks); one write transaction per response.
        let mut batch = Vec::with_capacity(step.added.len());
        let mut reached_stop = false;
        for accepted in step.added {
            let indices: Vec<(TxId, u32)> = accepted
                .accepted_tx_ids
                .iter()
                .enumerate()
                .map(|(i, &id)| (id, i as u32))
                .collect();
            let block = accepted.accepting_block;
            batch.push((block, indices));
            if block == stop {
                reached_stop = true;
                break;
            }
        }
        stamped += store.stamp_tx_indices(&batch)?;
        cursor = batch.last().expect("non-empty added").0;
        store.set_tx_index_backfill_progress(cursor)?;
        if reached_stop {
            break; // rows past the sync cursor are written with capture
        }
    }
    store.set_tx_index_backfill_done()?;
    Ok(stamped)
}

/// Classify the covenant activity of one accepting chain block's accepted
/// txs, given as `(index in the accepted-tx list, body)`.
fn classify<'a>(
    store: &Store,
    accepted: &AcceptedBlock,
    accepting: &Block,
    txs: impl Iterator<Item = (u32, &'a Transaction)>,
) -> Result<BlockEvents> {
    let mut block_events = BlockEvents {
        accepting_block: accepted.accepting_block,
        accepting_daa: accepting.daa_score,
        accepting_time_ms: accepting.timestamp_ms,
        accepting_blue_score: accepting.blue_score,
        events: vec![],
        created_utxos: vec![],
        spent_utxos: vec![],
    };
    // Overlay for intra-block chains: a tx spending a covenant UTXO created by
    // an earlier tx in the same accepting block.
    let mut created_overlay: HashMap<Outpoint, CovenantId> = HashMap::new();
    let mut known_overlay: HashSet<CovenantId> = HashSet::new();

    for (tx_index, tx) in txs {
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
                tx_index,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn h(n: u8) -> BlockHash {
        BlockHash([n; 32])
    }
    fn tx_id(n: u8) -> TxId {
        TxId([n; 32])
    }

    /// Acceptance data only — the backfill must never ask for block bodies.
    struct FakeAcceptance {
        pruning_point: BlockHash,
        /// cursor -> the step returned for it
        steps: HashMap<BlockHash, ChainStep>,
        rpc_calls: AtomicU64,
    }

    impl ChainSource for FakeAcceptance {
        async fn dag_info(&self) -> crate::Result<DagInfo> {
            self.rpc_calls.fetch_add(1, Ordering::Relaxed);
            Ok(DagInfo {
                network: "testnet-10".into(),
                sink: self.pruning_point,
                virtual_daa_score: 0,
                pruning_point: self.pruning_point,
            })
        }
        async fn block_with_txs(&self, hash: BlockHash) -> crate::Result<Block> {
            panic!("backfill must be UPDATE-only, but fetched block {hash}");
        }
        async fn virtual_chain_from(&self, cursor: BlockHash) -> crate::Result<ChainStep> {
            self.rpc_calls.fetch_add(1, Ordering::Relaxed);
            self.steps
                .get(&cursor)
                .cloned()
                .ok_or(crate::Error::Rpc(format!("unknown cursor {cursor}")))
        }
    }

    fn event(cov: u8, txid: TxId, tx_index: u32) -> crate::store::NewEvent {
        NewEvent {
            covenant_id: CovenantId([cov; 32]),
            kind: EventKind::Genesis,
            txid,
            tx_index,
            payload: None,
            lane_namespace: None,
        }
    }

    fn block_events(hash: BlockHash, daa: u64, events: Vec<NewEvent>) -> BlockEvents {
        let mut block = BlockEvents::empty(hash);
        block.accepting_daa = daa;
        block.events = events;
        block
    }

    /// The retention-window walk stamps NULL rows with the node's acceptance
    /// order, resumes across responses, marks itself done, and skips in O(1)
    /// (zero RPC) once complete.
    #[tokio::test]
    async fn backfill_stamps_pre_capture_rows_and_completes() {
        let db = std::env::temp_dir()
            .join(format!("kascov-sync-backfill-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let mut store = Store::open(&db, Network::Testnet(10)).unwrap();

        // Two accepting blocks; accepted lists carry a coinbase (index 0) and
        // a plain payment that never produced covenant rows.
        store
            .apply(
                &block_events(
                    h(1),
                    100,
                    vec![event(0xA1, tx_id(0xA0), 1), event(0xB2, tx_id(0xB0), 2)],
                ),
                h(1),
            )
            .unwrap();
        store
            .apply(&block_events(h(2), 200, vec![event(0xA1, tx_id(0xC0), 1)]), h(2))
            .unwrap();
        store.wipe_tx_indices_for_test().unwrap();
        assert_eq!(store.events(&CovenantId([0xA1; 32])).unwrap()[0].tx_index, None);

        // The node answers in two capped responses: pruning point -> h1, h1 -> h2.
        let node = FakeAcceptance {
            pruning_point: h(0),
            steps: HashMap::from([
                (
                    h(0),
                    ChainStep {
                        removed: vec![],
                        added: vec![AcceptedBlock {
                            accepting_block: h(1),
                            accepted_tx_ids: vec![tx_id(0xEE), tx_id(0xA0), tx_id(0xB0)],
                        }],
                    },
                ),
                (
                    h(1),
                    ChainStep {
                        removed: vec![],
                        added: vec![AcceptedBlock {
                            accepting_block: h(2),
                            accepted_tx_ids: vec![tx_id(0xEF), tx_id(0xC0)],
                        }],
                    },
                ),
            ]),
            rpc_calls: AtomicU64::new(0),
        };

        let stamped = backfill_tx_index(&node, &mut store).await.unwrap();
        assert_eq!(stamped, 3);
        let a1 = store.events(&CovenantId([0xA1; 32])).unwrap();
        assert_eq!(a1[0].tx_index, Some(1));
        assert_eq!(a1[1].tx_index, Some(1)); // h2's list: coinbase, then 0xC0
        assert_eq!(store.events(&CovenantId([0xB2; 32])).unwrap()[0].tx_index, Some(2));
        assert!(store.tx_index_backfill_done().unwrap());

        // Completed runs are O(1): no RPC at all.
        node.rpc_calls.store(0, Ordering::Relaxed);
        assert_eq!(backfill_tx_index(&node, &mut store).await.unwrap(), 0);
        assert_eq!(node.rpc_calls.load(Ordering::Relaxed), 0);
    }

    fn utxo(cov: u8, txid: TxId, index: u32) -> NewUtxo {
        NewUtxo {
            outpoint: Outpoint { txid, index },
            covenant_id: CovenantId([cov; 32]),
            value: 1_000,
            spk_version: 0,
            spk_script: vec![0x51],
        }
    }

    /// An empty successful step — what a node answers for a walkable cursor
    /// already at the tip (and how some nodes answer a STRANDED one).
    fn walkable() -> ChainStep {
        ChainStep { removed: vec![], added: vec![] }
    }

    /// A successful walk that actually returns chain blocks — what a
    /// truthful node answers for a walkable block below the tip.
    fn walkable_with_blocks() -> ChainStep {
        ChainStep {
            removed: vec![],
            added: vec![AcceptedBlock { accepting_block: h(9), accepted_tx_ids: vec![] }],
        }
    }

    /// Four indexed accepting blocks (daa 100..400); h2 creates a covenant
    /// UTXO that h3 spends; cursor at h4.
    fn stranded_store(name: &str) -> Store {
        let db =
            std::env::temp_dir().join(format!("kascov-sync-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let mut store = Store::open(&db, Network::Testnet(10)).unwrap();
        store.apply(&block_events(h(1), 100, vec![event(0xA1, tx_id(0xA0), 1)]), h(1)).unwrap();
        let mut b2 = block_events(h(2), 200, vec![event(0xB2, tx_id(0xB0), 1)]);
        b2.created_utxos.push(utxo(0xB2, tx_id(0xB0), 0));
        store.apply(&b2, h(2)).unwrap();
        let mut b3 = block_events(h(3), 300, vec![event(0xC3, tx_id(0xC0), 1)]);
        b3.created_utxos.push(utxo(0xC3, tx_id(0xC0), 0));
        b3.spent_utxos.push((Outpoint { txid: tx_id(0xB0), index: 0 }, tx_id(0xC0), vec![0xAA], 7));
        store.apply(&b3, h(3)).unwrap();
        let mut b4 = block_events(h(4), 400, vec![event(0xA1, tx_id(0xD0), 1)]);
        b4.created_utxos.push(utxo(0xA1, tx_id(0xD0), 0));
        store.apply(&b4, h(4)).unwrap();
        assert_eq!(store.cursor().unwrap(), Some(h(4)));
        store
    }

    /// The stranded cursor (h4) fails the walk, so does h3; h2 is the newest
    /// walkable candidate. Everything above it rolls back — h3's spend of
    /// h2's UTXO un-spends, h3/h4's rows disappear — and the cursor lands on
    /// the anchor with an honest processed_daa.
    #[tokio::test]
    async fn re_anchor_picks_newest_walkable_and_rolls_back_above() {
        let mut store = stranded_store("re-anchor");
        let node = FakeAcceptance {
            pruning_point: h(0),
            steps: HashMap::from([(h(2), walkable())]),
            rpc_calls: AtomicU64::new(0),
        };

        let outcome = re_anchor(&node, &mut store).await.unwrap();
        assert_eq!(outcome, ReAnchor::Anchored(h(2)));
        assert_eq!(store.cursor().unwrap(), Some(h(2)));
        assert_eq!(store.processed_daa().unwrap(), Some(200));
        // Cursor probe + h3 + h2 (h4 = the cursor is skipped, h1 never needed).
        assert_eq!(node.rpc_calls.load(Ordering::Relaxed), 3);

        // Rolled back above DAA 200: A1 keeps only its daa-100 event, its h4
        // UTXO is gone; C3 (born in h3) disappears entirely.
        let a1 = store.events(&CovenantId([0xA1; 32])).unwrap();
        assert_eq!(a1.len(), 1);
        assert!(store.utxos(&CovenantId([0xA1; 32]), false).unwrap().is_empty());
        assert!(store.events(&CovenantId([0xC3; 32])).unwrap().is_empty());
        assert!(store.utxos(&CovenantId([0xC3; 32]), false).unwrap().is_empty());
        // h2's UTXO survives and its rolled-back spend is undone.
        let b2 = store.utxos(&CovenantId([0xB2; 32]), false).unwrap();
        assert_eq!(b2.len(), 1);
        assert!(b2[0].live);
        assert_eq!(b2[0].spent_txid, None);
    }

    /// The re-anchor rollback lands in reorg_log exactly like a witnessed
    /// reorg: the DAA we had reached, and how many chain blocks were undone.
    #[tokio::test]
    async fn re_anchor_rollback_records_reorg_log() {
        let mut store = stranded_store("re-anchor-log");
        assert!(store.reorg_log(10).unwrap().is_empty());
        let node = FakeAcceptance {
            pruning_point: h(0),
            steps: HashMap::from([(h(2), walkable())]),
            rpc_calls: AtomicU64::new(0),
        };

        re_anchor(&node, &mut store).await.unwrap();
        let log = store.reorg_log(10).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].daa, 400);
        assert_eq!(log[0].rolled_back, 2); // h3 and h4
    }

    /// Nothing we indexed is walkable: report so (the caller owns the last
    /// resort) and leave the store byte-for-byte alone.
    #[tokio::test]
    async fn re_anchor_without_walkable_candidate_leaves_store_untouched() {
        let mut store = stranded_store("re-anchor-none");
        let node = FakeAcceptance {
            pruning_point: h(0),
            steps: HashMap::new(),
            rpc_calls: AtomicU64::new(0),
        };

        assert_eq!(re_anchor(&node, &mut store).await.unwrap(), ReAnchor::NothingWalkable);
        assert_eq!(store.cursor().unwrap(), Some(h(4)));
        assert_eq!(store.processed_daa().unwrap(), Some(400));
        assert_eq!(store.events(&CovenantId([0xA1; 32])).unwrap().len(), 2);
        assert_eq!(store.events(&CovenantId([0xC3; 32])).unwrap().len(), 1);
        assert!(store.reorg_log(10).unwrap().is_empty());
        // Cursor probe + h3, h2, h1 (h4 = the cursor is skipped).
        assert_eq!(node.rpc_calls.load(Ordering::Relaxed), 4);
    }

    /// A walkable cursor means the failures are something else: hands off,
    /// zero candidate probes.
    #[tokio::test]
    async fn re_anchor_healthy_cursor_short_circuits() {
        let mut store = stranded_store("re-anchor-healthy");
        let node = FakeAcceptance {
            pruning_point: h(0),
            steps: HashMap::from([(h(4), walkable())]),
            rpc_calls: AtomicU64::new(0),
        };

        assert_eq!(re_anchor(&node, &mut store).await.unwrap(), ReAnchor::NotWedged);
        assert_eq!(store.cursor().unwrap(), Some(h(4)));
        assert_eq!(node.rpc_calls.load(Ordering::Relaxed), 1);
    }

    /// The empty-walk lie: a node answers the stranded cursor with an EMPTY
    /// "success" while the index lags far behind the tip. A truthful walk
    /// would return blocks, so re_anchor treats it as unwalkable and still
    /// re-anchors onto a candidate whose walk actually returns chain blocks.
    #[tokio::test]
    async fn re_anchor_sees_through_empty_success_walks_when_lagging() {
        let mut store = stranded_store("re-anchor-emptylie");
        store.set_tip(400 + WEDGE_LAG_DAA + 1, 1).unwrap();
        let node = FakeAcceptance {
            pruning_point: h(0),
            steps: HashMap::from([
                (h(4), walkable()), // the lie
                (h(2), walkable_with_blocks()),
            ]),
            rpc_calls: AtomicU64::new(0),
        };

        assert_eq!(re_anchor(&node, &mut store).await.unwrap(), ReAnchor::Anchored(h(2)));
        assert_eq!(store.cursor().unwrap(), Some(h(2)));
        assert_eq!(store.processed_daa().unwrap(), Some(200));
        // Cursor probe + h3 + h2 (h4 = the cursor is skipped as a candidate).
        assert_eq!(node.rpc_calls.load(Ordering::Relaxed), 3);
    }

    /// The success-that-does-nothing wedge: passes succeed, the cursor sits
    /// still far behind the tip. The watch demands recovery on exactly the
    /// WEDGE_PASSES-th such pass, then re-arms for another full window.
    #[test]
    fn progress_watch_demands_recovery_after_stuck_passes() {
        let mut watch = ProgressWatch::default();
        let tip = 100 + WEDGE_LAG_DAA + 1;
        assert!(watch.observe(Some(100), Some(tip)).advanced); // first sighting
        for pass in 1..WEDGE_PASSES {
            let verdict = watch.observe(Some(100), Some(tip));
            assert!(!verdict.advanced);
            assert!(!verdict.demand_recovery, "demanded too early at pass {pass}");
        }
        assert!(watch.observe(Some(100), Some(tip)).demand_recovery);
        // Re-armed: the next demand needs a full window again.
        assert!(!watch.observe(Some(100), Some(tip)).demand_recovery);
    }

    /// Any cursor movement — forward progress or a recovery rollback — and
    /// any pass within the lag threshold reset the wedge counter.
    #[test]
    fn progress_watch_resets_on_movement_or_small_lag() {
        let mut watch = ProgressWatch::default();
        let tip = 100 + WEDGE_LAG_DAA + 1;
        for _ in 0..WEDGE_PASSES - 1 {
            watch.observe(Some(100), Some(tip));
        }
        // Forward movement resets the window.
        assert!(watch.observe(Some(101), Some(tip)).advanced);
        for _ in 0..WEDGE_PASSES - 1 {
            assert!(!watch.observe(Some(101), Some(tip)).demand_recovery);
        }
        // A rollback (processed moves DOWN) is the index acting, not a stall.
        assert!(watch.observe(Some(50), Some(tip)).advanced);
        // Lag at (not beyond) the threshold never counts as wedged.
        for _ in 0..WEDGE_PASSES * 2 {
            let verdict = watch.observe(Some(50), Some(50 + WEDGE_LAG_DAA));
            assert!(!verdict.demand_recovery);
        }
        // A fresh index (no processed mark yet) never demands recovery.
        let mut fresh = ProgressWatch::default();
        for _ in 0..WEDGE_PASSES * 2 {
            assert!(!fresh.observe(None, Some(tip)).demand_recovery);
        }
    }

    /// Rows older than the pruning point are unreachable — the walk stamps
    /// what it can, leaves the rest NULL, and still completes (graceful, not
    /// an error).
    #[tokio::test]
    async fn backfill_leaves_pruned_history_null() {
        let db = std::env::temp_dir()
            .join(format!("kascov-sync-backfill-pruned-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let mut store = Store::open(&db, Network::Testnet(10)).unwrap();

        // h1 predates the pruning point (h2); only h3 is walkable.
        store
            .apply(&block_events(h(1), 100, vec![event(0xA1, tx_id(0xA0), 1)]), h(1))
            .unwrap();
        store
            .apply(&block_events(h(3), 300, vec![event(0xB2, tx_id(0xB0), 1)]), h(3))
            .unwrap();
        store.wipe_tx_indices_for_test().unwrap();

        let node = FakeAcceptance {
            pruning_point: h(2),
            steps: HashMap::from([(
                h(2),
                ChainStep {
                    removed: vec![],
                    added: vec![AcceptedBlock {
                        accepting_block: h(3),
                        accepted_tx_ids: vec![tx_id(0xEE), tx_id(0xB0)],
                    }],
                },
            )]),
            rpc_calls: AtomicU64::new(0),
        };

        let stamped = backfill_tx_index(&node, &mut store).await.unwrap();
        assert_eq!(stamped, 1);
        assert_eq!(store.events(&CovenantId([0xA1; 32])).unwrap()[0].tx_index, None);
        assert_eq!(store.events(&CovenantId([0xB2; 32])).unwrap()[0].tx_index, Some(1));
        assert!(store.tx_index_backfill_done().unwrap());
    }
}
