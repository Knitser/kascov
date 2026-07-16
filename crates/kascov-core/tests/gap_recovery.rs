//! End-to-end gap recovery: reproduce the production incident in miniature —
//! a healthy pre-gap index, a deep-reorg wedge answered by a sink reset (so
//! the follower resumes at the tip and a whole DAA window is skipped), then
//! `recover_gap` walking the canonical chain from the node's pruning point —
//! and assert the merged index equals what an unwedged indexer would hold.
//!
//! The synthetic incident exercises every enumerated UTXO case:
//!   * X — born pre-gap, moved IN the gap, moved again post-gap: its pre-gap
//!     cell was spent in-gap (created-before/spent-in), its gap cell was
//!     spent post-gap by a tx production DID record a (transition) event for
//!     (created-in/spent-after with partial production knowledge), and its
//!     post-gap seq must be re-sequenced behind the merged gap event;
//!   * Y — born AND burned entirely inside the gap (created+spent in gap;
//!     genesis in gap): production knows nothing of it;
//!   * Z — born in the gap, PURE-burned post-gap: production saw neither, so
//!     the reconcile pass must both repair the spend and insert the burn;
//!   * W — born in the gap with TWO cells: one spent post-gap by a recorded
//!     transition (dedup path), one pure-burned post-gap (insert path), and
//!     production's partial row (transition-first, lineage incomplete) must
//!     be healed into a complete KIP-20-proven lineage.

use std::collections::HashMap;

use kascov_core::model::*;
use kascov_core::node::{compute_covenant_id, ChainSource};
use kascov_core::store::Store;
use kascov_core::sync::{recover_gap, sync_once, GapRecoveryOptions};
use kascov_core::{Error, Result};

fn h(n: u8) -> BlockHash {
    BlockHash([n; 32])
}
fn tx_id(n: u8) -> TxId {
    TxId([n; 32])
}

const VALUE: u64 = 100_000_000;
const SPK: [u8; 2] = [0xaa, 0xbb];

/// A KIP-20-valid covenant id for a tx spending `outpoint` into `n_outputs`
/// bound outputs of the shape `covenant_tx` produces.
fn valid_genesis_id(outpoint: Outpoint, n_outputs: u32) -> CovenantId {
    let fields: Vec<(u32, u64, u16, &[u8])> =
        (0..n_outputs).map(|i| (i, VALUE, 0, SPK.as_slice())).collect();
    compute_covenant_id(&outpoint, &fields)
}

fn covenant_tx(
    txid: TxId,
    spends: Vec<Outpoint>,
    covenant: Option<CovenantId>,
    n_outputs: u32,
) -> Transaction {
    Transaction {
        txid,
        version: 1,
        inputs: spends
            .into_iter()
            .map(|previous_outpoint| Input {
                previous_outpoint,
                signature_script: vec![0x01, 0x99],
                compute_budget: 7,
            })
            .collect(),
        outputs: match covenant {
            Some(covenant_id) => (0..n_outputs)
                .map(|_| Output {
                    value: VALUE,
                    spk_version: 0,
                    spk_script: SPK.to_vec(),
                    covenant: Some(CovenantBinding { covenant_id, authorizing_input: 0 }),
                })
                .collect(),
            None => {
                vec![Output { value: VALUE, spk_version: 0, spk_script: vec![0xcc], covenant: None }]
            }
        },
        payload: vec![0xde, 0xad],
    }
}

/// In-memory chain keyed by walk cursor — unlike the scripted-sequence fake
/// in sync_replay.rs, `virtual_chain_from` answers by cursor, which is what
/// recover_gap's local-cursor walk (and the production node) actually does.
/// A cursor with no scripted step is the tip: an empty successful walk.
struct FakeGapChain {
    blocks: HashMap<BlockHash, Block>,
    steps: HashMap<BlockHash, ChainStep>,
    sink: BlockHash,
    pruning_point: BlockHash,
}

impl FakeGapChain {
    fn block(&mut self, hash: BlockHash, daa: u64, txs: Vec<Transaction>) {
        self.blocks.insert(
            hash,
            Block {
                hash,
                daa_score: daa,
                blue_score: daa,
                timestamp_ms: daa * 1000,
                parents: vec![],
                mergeset: vec![],
                transactions: txs,
            },
        );
    }
}

impl ChainSource for FakeGapChain {
    async fn dag_info(&self) -> Result<DagInfo> {
        Ok(DagInfo {
            network: "testnet-10".into(),
            sink: self.sink,
            virtual_daa_score: 0,
            pruning_point: self.pruning_point,
        })
    }
    async fn block_with_txs(&self, hash: BlockHash) -> Result<Block> {
        self.blocks.get(&hash).cloned().ok_or(Error::Rpc(format!("no block {hash}")))
    }
    async fn virtual_chain_from(&self, cursor: BlockHash) -> Result<ChainStep> {
        Ok(self.steps.get(&cursor).cloned().unwrap_or(ChainStep { removed: vec![], added: vec![] }))
    }
}

fn accepted(block: BlockHash, txs: &[TxId]) -> AcceptedBlock {
    AcceptedBlock { accepting_block: block, accepted_tx_ids: txs.to_vec() }
}

#[tokio::test]
async fn gap_recovery_merges_canonical_history_and_is_idempotent() {
    let db = std::env::temp_dir().join(format!("kascov-gap-recovery-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let mut store = Store::open(&db, Network::Testnet(10)).unwrap();

    // ---- the cast ----
    // X: born pre-gap.
    let fund_x = Outpoint { txid: tx_id(0x02), index: 0 };
    let cov_x = valid_genesis_id(fund_x, 1);
    let tx_a = covenant_tx(tx_id(0xA0), vec![fund_x], Some(cov_x), 1); // genesis, pre-gap
    let tx_b = covenant_tx(
        tx_id(0xB0),
        vec![Outpoint { txid: tx_id(0xA0), index: 0 }],
        Some(cov_x),
        1,
    ); // in-gap transition
    let tx_c = covenant_tx(
        tx_id(0xC0),
        vec![Outpoint { txid: tx_id(0xB0), index: 0 }],
        Some(cov_x),
        1,
    ); // post-gap transition
    // Y: born and burned inside the gap.
    let fund_y = Outpoint { txid: tx_id(0x03), index: 0 };
    let cov_y = valid_genesis_id(fund_y, 1);
    let tx_d = covenant_tx(tx_id(0xD0), vec![fund_y], Some(cov_y), 1);
    let tx_e =
        covenant_tx(tx_id(0xE0), vec![Outpoint { txid: tx_id(0xD0), index: 0 }], None, 1);
    // Z: born in the gap, pure-burned post-gap.
    let fund_z = Outpoint { txid: tx_id(0x04), index: 0 };
    let cov_z = valid_genesis_id(fund_z, 1);
    let tx_f = covenant_tx(tx_id(0xF0), vec![fund_z], Some(cov_z), 1);
    let tx_g =
        covenant_tx(tx_id(0x9A), vec![Outpoint { txid: tx_id(0xF0), index: 0 }], None, 1);
    // W: born in the gap with TWO cells; one recorded post-gap transition,
    // one invisible post-gap pure burn.
    let fund_w = Outpoint { txid: tx_id(0x05), index: 0 };
    let cov_w = valid_genesis_id(fund_w, 2);
    let tx_h = covenant_tx(tx_id(0x8A), vec![fund_w], Some(cov_w), 2);
    let tx_i = covenant_tx(
        tx_id(0x8B),
        vec![Outpoint { txid: tx_id(0x8A), index: 0 }],
        Some(cov_w),
        1,
    );
    let tx_j =
        covenant_tx(tx_id(0x8C), vec![Outpoint { txid: tx_id(0x8A), index: 1 }], None, 1);
    // A plain payment accepted ahead of tx_b, so merged rows get a real
    // (non-zero) tx_index from the accepted-tx list position.
    let filler = covenant_tx(tx_id(0x77), vec![Outpoint { txid: tx_id(0xEE), index: 0 }], None, 1);

    let mut chain = FakeGapChain {
        blocks: HashMap::new(),
        steps: HashMap::new(),
        sink: h(0x50),
        pruning_point: h(0x10),
    };
    // pre-gap
    chain.block(h(0x01), 100, vec![tx_a]);
    // canonical gap window (younger than node retention, servable)
    chain.block(h(0x11), 50, vec![]); // below the gap: batch-skip fodder
    chain.block(h(0x21), 1_000_000, vec![filler, tx_b]);
    chain.block(h(0x22), 1_000_100, vec![tx_d]);
    chain.block(h(0x23), 1_000_200, vec![tx_e]);
    chain.block(h(0x24), 1_000_300, vec![tx_f]);
    chain.block(h(0x25), 1_000_400, vec![tx_h]);
    // post-gap (walked by production after the sink reset)
    chain.block(h(0x51), 2_000_000, vec![tx_c]);
    chain.block(h(0x52), 2_000_050, vec![tx_i]);
    chain.block(h(0x53), 2_000_100, vec![tx_g]);
    chain.block(h(0x54), 2_000_150, vec![tx_j]);

    // ---- phase 1: healthy pre-gap sync ----
    chain
        .steps
        .insert(h(0x00), ChainStep { removed: vec![], added: vec![accepted(h(0x01), &[tx_id(0xA0)])] });
    sync_once(&chain, &mut store, Some(h(0x00)), |_| {}).await.unwrap();
    assert_eq!(store.events(&cov_x).unwrap().len(), 1);

    // ---- phase 2: the incident — cursor stranded by a deep reorg, the
    // follower's last resort resets to the current sink; production resumes
    // at the tip with NO knowledge of anything created inside the gap. ----
    store.reset_cursor(h(0x50)).unwrap();
    chain.steps.insert(
        h(0x50),
        ChainStep {
            removed: vec![],
            added: vec![
                accepted(h(0x51), &[tx_id(0xC0)]),
                accepted(h(0x52), &[tx_id(0x8B)]),
                accepted(h(0x53), &[tx_id(0x9A)]),
                accepted(h(0x54), &[tx_id(0x8C)]),
            ],
        },
    );
    sync_once(&chain, &mut store, None, |_| {}).await.unwrap();

    // Production's broken-but-real post-reset state: X's gap spend unseen
    // (A:0 still "live"), W known only mid-life, Y and Z entirely absent,
    // the pure burns invisible.
    assert_eq!(
        store.events(&cov_x).unwrap().iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
        ["genesis", "transition"]
    );
    let a0 = store.utxos(&cov_x, false).unwrap();
    assert!(a0.iter().find(|u| u.outpoint.txid == tx_id(0xA0)).unwrap().live);
    let w = store.summary(&cov_w).unwrap().unwrap();
    assert!(!w.lineage_complete);
    assert_eq!(w.genesis_txid, None);
    assert!(store.events(&cov_y).unwrap().is_empty());
    assert!(store.events(&cov_z).unwrap().is_empty());
    assert_eq!(store.cursor().unwrap(), Some(h(0x54)));
    assert_eq!(store.processed_daa().unwrap(), Some(2_000_150));

    // ---- phase 3: the recovery walk (node's pruning point → tip) ----
    chain.steps.insert(
        h(0x10),
        ChainStep { removed: vec![], added: vec![accepted(h(0x11), &[])] },
    );
    chain.steps.insert(
        h(0x11),
        ChainStep {
            removed: vec![],
            added: vec![
                accepted(h(0x21), &[tx_id(0x77), tx_id(0xB0)]),
                accepted(h(0x22), &[tx_id(0xD0)]),
                accepted(h(0x23), &[tx_id(0xE0)]),
                accepted(h(0x24), &[tx_id(0xF0)]),
                accepted(h(0x25), &[tx_id(0x8A)]),
            ],
        },
    );
    chain.steps.insert(
        h(0x25),
        ChainStep {
            removed: vec![],
            added: vec![
                accepted(h(0x51), &[tx_id(0xC0)]),
                accepted(h(0x52), &[tx_id(0x8B)]),
                accepted(h(0x53), &[tx_id(0x9A)]),
                accepted(h(0x54), &[tx_id(0x8C)]),
            ],
        },
    );

    assert_eq!(store.gap_recovery_pending().unwrap(), None);
    let report =
        recover_gap(&chain, &mut store, &GapRecoveryOptions::default(), |_| {}).await.unwrap();
    assert_eq!(store.gap_recovery_pending().unwrap(), None, "in-flight marker must retire");

    assert!(!report.already_recovered);
    assert_eq!((report.gap_lo, report.gap_hi), (100, 2_000_000));
    assert_eq!(report.chain_blocks_walked, 10); // 1 skipped + 5 gap + 4 reconcile
    assert_eq!(report.blocks_captured, 6); // g1..g5 + the inclusive gap_hi boundary
    assert_eq!(report.events_added, 7); // B, D, E, F, H + reconciled burns G, J
    assert_eq!(report.utxos_added, 5); // B:0, D:0, F:0, H:0, H:1
    assert_eq!(report.spends_repaired, 6); // A:0, D:0, B:0, H:0, F:0, H:1
    assert_eq!(report.covenants_refreshed, 4);
    assert_eq!(report.covenants_resequenced, 2); // X and W (Y, Z merged in order)

    // The store's live cursor and progress mark were never touched.
    assert_eq!(store.cursor().unwrap(), Some(h(0x54)));
    assert_eq!(store.processed_daa().unwrap(), Some(2_000_150));

    // X: strictly chronological per-covenant seq after the merge.
    let x = store.events(&cov_x).unwrap();
    let x_view: Vec<(u64, &str, TxId)> =
        x.iter().map(|e| (e.seq, e.kind.as_str(), e.txid)).collect();
    assert_eq!(
        x_view,
        [
            (0, "genesis", tx_id(0xA0)),
            (1, "transition", tx_id(0xB0)),
            (2, "transition", tx_id(0xC0)),
        ]
    );
    // The merged row captured its acceptance-order position (filler was 0).
    assert_eq!(x[1].tx_index, Some(1));
    // UTXO linkage repaired across the whole lineage.
    let x_utxos = store.utxos(&cov_x, false).unwrap();
    let cell = |txid: TxId| x_utxos.iter().find(|u| u.outpoint.txid == txid).unwrap();
    assert!(!cell(tx_id(0xA0)).live);
    assert_eq!(cell(tx_id(0xA0)).spent_txid, Some(tx_id(0xB0)));
    assert!(!cell(tx_id(0xB0)).live);
    assert_eq!(cell(tx_id(0xB0)).spent_txid, Some(tx_id(0xC0)));
    assert!(cell(tx_id(0xC0)).live);
    let x_sum = store.summary(&cov_x).unwrap().unwrap();
    assert_eq!(x_sum.event_count, 3);
    assert_eq!(x_sum.last_activity_daa, 2_000_000);
    assert!(x_sum.lineage_complete);

    // Y: a whole life inside the gap, reconstructed from nothing.
    let y_view: Vec<(u64, String, TxId)> =
        store.events(&cov_y).unwrap().iter().map(|e| (e.seq, e.kind.clone(), e.txid)).collect();
    assert_eq!(y_view, [(0, "genesis".into(), tx_id(0xD0)), (1, "burn".into(), tx_id(0xE0))]);
    let y_sum = store.summary(&cov_y).unwrap().unwrap();
    assert_eq!(y_sum.genesis_txid, Some(tx_id(0xD0)));
    assert_eq!(y_sum.genesis_daa, Some(1_000_100));
    assert!(y_sum.lineage_complete);
    assert_eq!(y_sum.live_utxos, 0);
    let y_cell = &store.utxos(&cov_y, false).unwrap()[0];
    assert_eq!(y_cell.spent_txid, Some(tx_id(0xE0)));
    assert_eq!(y_cell.spent_sig.as_deref(), Some([0x01, 0x99].as_slice()));
    assert_eq!(y_cell.spent_budget, Some(7));

    // Z: gap birth + post-gap pure burn production never saw — the reconcile
    // pass both repairs the cell and inserts the burn event.
    let z_view: Vec<(u64, String, TxId)> =
        store.events(&cov_z).unwrap().iter().map(|e| (e.seq, e.kind.clone(), e.txid)).collect();
    assert_eq!(z_view, [(0, "genesis".into(), tx_id(0xF0)), (1, "burn".into(), tx_id(0x9A))]);
    let z_burn = &store.events(&cov_z).unwrap()[1];
    assert_eq!(z_burn.accepting_daa, 2_000_100);
    assert_eq!(z_burn.tx_index, Some(0));
    assert_eq!(z_burn.payload.as_deref(), Some([0xde, 0xad].as_slice()));
    let z_cell = &store.utxos(&cov_z, false).unwrap()[0];
    assert!(!z_cell.live);
    assert_eq!(z_cell.spent_txid, Some(tx_id(0x9A)));
    assert!(store.summary(&cov_z).unwrap().unwrap().lineage_complete);

    // W: production's mid-life partial record healed — gap genesis slots in
    // FRONT of the recorded post-gap transition (re-sequencing), the missed
    // burn of the second cell slots in behind it, and the dedup kept exactly
    // one event for the transition production already recorded.
    let w_view: Vec<(u64, String, TxId)> =
        store.events(&cov_w).unwrap().iter().map(|e| (e.seq, e.kind.clone(), e.txid)).collect();
    assert_eq!(
        w_view,
        [
            (0, "genesis".into(), tx_id(0x8A)),
            (1, "transition".into(), tx_id(0x8B)),
            (2, "burn".into(), tx_id(0x8C)),
        ]
    );
    let w_sum = store.summary(&cov_w).unwrap().unwrap();
    assert!(w_sum.lineage_complete, "gap genesis must complete the lineage");
    assert_eq!(w_sum.genesis_txid, Some(tx_id(0x8A)));
    assert_eq!(w_sum.genesis_daa, Some(1_000_400));
    assert_eq!(w_sum.event_count, 3);
    assert_eq!(w_sum.last_activity_daa, 2_000_150);
    let w_utxos = store.utxos(&cov_w, false).unwrap();
    let w_cell = |txid: TxId, index: u32| {
        w_utxos.iter().find(|u| u.outpoint.txid == txid && u.outpoint.index == index).unwrap()
    };
    assert_eq!(w_cell(tx_id(0x8A), 0).spent_txid, Some(tx_id(0x8B))); // recorded transition
    assert_eq!(w_cell(tx_id(0x8A), 1).spent_txid, Some(tx_id(0x8C))); // invisible burn
    assert!(w_cell(tx_id(0x8B), 0).live);

    // The recovery is on record (honest history + idempotence marker).
    assert_eq!(store.gap_recoveries().unwrap(), [(100, 2_000_000)]);

    // ---- phase 4: idempotence — the second run is a no-op ----
    let again =
        recover_gap(&chain, &mut store, &GapRecoveryOptions::default(), |_| {}).await.unwrap();
    assert!(again.already_recovered);
    assert_eq!(again.events_added, 0);
    // Explicit bounds inside the recovered window are a no-op too.
    let opts = GapRecoveryOptions { from_daa: Some(150), to_daa: Some(1_500_000), ..Default::default() };
    assert!(recover_gap(&chain, &mut store, &opts, |_| {}).await.unwrap().already_recovered);
    // Byte-identical outcome: same events, same seqs, same spends.
    let x_after: Vec<(u64, String, TxId)> =
        store.events(&cov_x).unwrap().iter().map(|e| (e.seq, e.kind.clone(), e.txid)).collect();
    assert_eq!(
        x_after,
        x_view.iter().map(|&(s, k, t)| (s, k.to_string(), t)).collect::<Vec<_>>()
    );
    assert_eq!(store.events(&cov_w).unwrap().len(), 3);
    assert_eq!(store.events(&cov_y).unwrap().len(), 2);
    assert_eq!(store.events(&cov_z).unwrap().len(), 2);
    assert_eq!(store.gap_recoveries().unwrap().len(), 1);
}
