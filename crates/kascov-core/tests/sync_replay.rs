//! Replay a synthetic chain (genesis → transitions → burn, plus a reorg)
//! through the real sync engine and store, and assert the index is correct.

use std::collections::HashMap;
use std::sync::Mutex;

use kascov_core::model::*;
use kascov_core::node::{compute_covenant_id, ChainSource};
use kascov_core::store::{EventKind, Store};
use kascov_core::sync::{sync_once, SyncUpdate};
use kascov_core::{Error, Result};

fn h(n: u8) -> BlockHash {
    BlockHash([n; 32])
}
fn tx_id(n: u8) -> TxId {
    TxId([n; 32])
}
fn cov(n: u8) -> CovenantId {
    CovenantId([n; 32])
}

/// The covenant output shape `covenant_tx` produces, hashed the KIP-20 way:
/// a valid genesis id for a tx spending `outpoint` into that single output.
fn valid_genesis_id(outpoint: Outpoint) -> CovenantId {
    compute_covenant_id(&outpoint, &[(0, 100_000_000, 0, &[0xaa, 0xbb])])
}

fn covenant_tx(txid: TxId, spends: Vec<Outpoint>, covenant: Option<CovenantId>) -> Transaction {
    Transaction {
        txid,
        version: 1,
        inputs: spends
            .into_iter()
            .map(|previous_outpoint| Input {
                previous_outpoint,
                signature_script: vec![0x01, 0x99], // a one-push unlocking script
                compute_budget: 7,
            })
            .collect(),
        outputs: match covenant {
            Some(covenant_id) => vec![Output {
                value: 100_000_000,
                spk_version: 0,
                spk_script: vec![0xaa, 0xbb],
                covenant: Some(CovenantBinding { covenant_id, authorizing_input: 0 }),
            }],
            None => vec![Output { value: 100_000_000, spk_version: 0, spk_script: vec![0xcc], covenant: None }],
        },
        payload: vec![0xde, 0xad], // v1 payload, captured on covenant events
    }
}

/// In-memory chain: a scripted sequence of ChainSteps handed out one per
/// virtual_chain_from call, plus block bodies by hash.
struct FakeChain {
    blocks: HashMap<BlockHash, Block>,
    steps: Mutex<Vec<ChainStep>>,
    sink: BlockHash,
}

impl FakeChain {
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

impl ChainSource for FakeChain {
    async fn dag_info(&self) -> Result<DagInfo> {
        Ok(DagInfo {
            network: "testnet-10".into(),
            sink: self.sink,
            virtual_daa_score: 0,
            pruning_point: self.sink,
        })
    }
    async fn block_with_txs(&self, hash: BlockHash) -> Result<Block> {
        self.blocks.get(&hash).cloned().ok_or(Error::Rpc(format!("no block {hash}")))
    }
    async fn virtual_chain_from(&self, _cursor: BlockHash) -> Result<ChainStep> {
        let mut steps = self.steps.lock().unwrap();
        if steps.is_empty() {
            Ok(ChainStep { removed: vec![], added: vec![] })
        } else {
            Ok(steps.remove(0))
        }
    }
    async fn mempool_txs(&self) -> Result<Vec<Transaction>> {
        Ok(vec![])
    }
}

fn accepted(block: BlockHash, txs: &[TxId]) -> AcceptedBlock {
    AcceptedBlock { accepting_block: block, accepted_tx_ids: txs.to_vec() }
}

#[tokio::test]
async fn genesis_transitions_burn_and_reorg() {
    let dir = std::env::temp_dir().join(format!("kascov-test-{}", std::process::id()));
    let db = dir.join("replay.db");
    let _ = std::fs::remove_file(&db);
    let mut store = Store::open(&db, Network::Testnet(10)).unwrap();

    let mut chain = FakeChain { blocks: HashMap::new(), steps: Mutex::new(vec![]), sink: h(0) };

    // Chain block 1: tx A creates covenant X (genesis) — its id must be the
    // KIP-20 hash of the spent outpoint + the authorized output, or the
    // classifier will (rightly) refuse to call it a genesis.
    let genesis_spend = Outpoint { txid: tx_id(0x01), index: 0 };
    let cov_x = valid_genesis_id(genesis_spend);
    let genesis_tx = covenant_tx(tx_id(0xA0), vec![genesis_spend], Some(cov_x));
    // Chain block 2: tx B spends A:0, continues covenant X (transition).
    let transition_tx = covenant_tx(tx_id(0xB0), vec![Outpoint { txid: tx_id(0xA0), index: 0 }], Some(cov_x));
    // Chain block 3 (later reorged out): tx C spends B:0 with no successor (burn).
    let burn_tx = covenant_tx(tx_id(0xD0), vec![Outpoint { txid: tx_id(0xB0), index: 0 }], None);

    chain.block(h(1), 100, vec![genesis_tx]);
    chain.block(h(2), 200, vec![transition_tx]);
    chain.block(h(3), 300, vec![burn_tx.clone()]);

    chain.steps.lock().unwrap().extend([
        ChainStep { removed: vec![], added: vec![accepted(h(1), &[tx_id(0xA0)]), accepted(h(2), &[tx_id(0xB0)])] },
        ChainStep { removed: vec![], added: vec![accepted(h(3), &[tx_id(0xD0)])] },
    ]);

    // Pass 1: genesis + transition.
    let mut events = vec![];
    let stats = sync_once(&chain, &mut store, Some(h(0)), |u| {
        if let SyncUpdate::Event { kind, covenant_id, .. } = u {
            events.push((kind, covenant_id));
        }
    })
    .await
    .unwrap();
    assert_eq!(stats.events, 2);
    assert_eq!(events, vec![(EventKind::Genesis, cov_x), (EventKind::Transition, cov_x)]);

    let tip = store.tip().unwrap().expect("tip recorded on every pass");
    assert_eq!(tip.0, 0, "FakeChain reports virtual daa 0");
    assert!(tip.1 > 0, "tip wall-clock must be recorded");

    let summary = store.summary(&cov_x).unwrap().unwrap();
    assert_eq!(summary.event_count, 2);
    assert_eq!(summary.live_utxos, 1, "transition output should be the only live state UTXO");
    assert!(summary.lineage_complete);
    assert_eq!(summary.genesis_txid, Some(tx_id(0xA0)));

    // Pass 2: the burn is accepted.
    let stats = sync_once(&chain, &mut store, None, |_| {}).await.unwrap();
    assert_eq!(stats.events, 1);
    let summary = store.summary(&cov_x).unwrap().unwrap();
    assert_eq!(summary.event_count, 3);
    assert_eq!(summary.live_utxos, 0, "covenant should be burned");

    // The burn's unlocking script, budget, and the tx payload were captured.
    let spent = store
        .utxos(&cov_x, false)
        .unwrap()
        .into_iter()
        .find(|u| u.spent_txid == Some(tx_id(0xD0)))
        .expect("burned state UTXO recorded");
    assert_eq!(spent.spent_sig.as_deref(), Some([0x01, 0x99].as_slice()));
    assert_eq!(spent.spent_budget, Some(7));
    let burn_event = store.events(&cov_x).unwrap().into_iter().last().unwrap();
    assert_eq!(burn_event.payload.as_deref(), Some([0xde, 0xad].as_slice()));

    // Pass 3: chain block 3 is reorged out, replaced by an empty block 4.
    chain.block(h(4), 301, vec![]);
    chain.steps.lock().unwrap().push(ChainStep {
        removed: vec![h(3)],
        added: vec![accepted(h(4), &[])],
    });
    let stats = sync_once(&chain, &mut store, None, |_| {}).await.unwrap();
    assert_eq!(stats.reorged_out, 1);
    let summary = store.summary(&cov_x).unwrap().unwrap();
    assert_eq!(summary.event_count, 2, "burn event must be rolled back");
    assert_eq!(summary.live_utxos, 1, "state UTXO must be live again after rollback");
    let unspent = store
        .utxos(&cov_x, false)
        .unwrap()
        .into_iter()
        .find(|u| u.outpoint.txid == tx_id(0xB0))
        .expect("transition UTXO present");
    assert_eq!(unspent.spent_sig, None, "rollback must clear the captured sig");

    // Pass 4: the burn is re-accepted in chain block 5 — index converges.
    chain.block(h(5), 302, vec![burn_tx]);
    chain.steps.lock().unwrap().push(ChainStep { removed: vec![], added: vec![accepted(h(5), &[tx_id(0xD0)])] });
    sync_once(&chain, &mut store, None, |_| {}).await.unwrap();
    let summary = store.summary(&cov_x).unwrap().unwrap();
    assert_eq!(summary.event_count, 3);
    assert_eq!(summary.live_utxos, 0);

    // Lineage trace is complete and ordered.
    let trace = store.events(&cov_x).unwrap();
    let kinds: Vec<&str> = trace.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, ["genesis", "transition", "burn"]);
}

#[tokio::test]
async fn mid_life_covenant_is_marked_truncated() {
    let dir = std::env::temp_dir().join(format!("kascov-test-{}", std::process::id()));
    let db = dir.join("truncated.db");
    let _ = std::fs::remove_file(&db);
    let mut store = Store::open(&db, Network::Testnet(10)).unwrap();

    let mut chain = FakeChain { blocks: HashMap::new(), steps: Mutex::new(vec![]), sink: h(0) };
    // A continuation output for a covenant whose earlier history we never saw:
    // it asserts an id that does NOT recompute from this tx's outpoint and
    // outputs (KIP-20), so it can't be a genesis — it's a covenant that was
    // born before we started watching. The classifier records a transition
    // and the lineage stays honestly incomplete.
    let tx = covenant_tx(tx_id(0xE0), vec![Outpoint { txid: tx_id(0x99), index: 7 }], Some(cov(0xC2)));
    chain.block(h(1), 100, vec![tx]);
    chain.steps.lock().unwrap().push(ChainStep { removed: vec![], added: vec![accepted(h(1), &[tx_id(0xE0)])] });

    sync_once(&chain, &mut store, Some(h(0)), |_| {}).await.unwrap();
    let summary = store.summary(&cov(0xC2)).unwrap().unwrap();
    assert_eq!(summary.event_count, 1);
    assert_eq!(summary.live_utxos, 1);
    assert!(!summary.lineage_complete, "unprovable genesis must mark lineage truncated");
    assert_eq!(summary.genesis_txid, None);
    let kinds: Vec<String> = store.events(&cov(0xC2)).unwrap().iter().map(|e| e.kind.clone()).collect();
    assert_eq!(kinds, ["transition"], "first sighting without genesis proof is a transition");
}

#[tokio::test]
async fn intra_block_create_and_spend_is_marked_spent() {
    let dir = std::env::temp_dir().join(format!("kascov-test-{}", std::process::id()));
    let db = dir.join("intra-block.db");
    let _ = std::fs::remove_file(&db);
    let mut store = Store::open(&db, Network::Testnet(10)).unwrap();

    let mut chain = FakeChain { blocks: HashMap::new(), steps: Mutex::new(vec![]), sink: h(0) };

    // tx A births covenant X, tx B immediately spends A:0 (continuation) —
    // and BOTH are accepted by the same chain block, as happens routinely on
    // a 10 bps network where one accepting block sweeps a whole mergeset.
    let funding = Outpoint { txid: tx_id(0x01), index: 0 };
    let id = valid_genesis_id(funding);
    let tx_a = covenant_tx(tx_id(0xA0), vec![funding], Some(id));
    let tx_b = covenant_tx(tx_id(0xB0), vec![Outpoint { txid: tx_id(0xA0), index: 0 }], Some(id));
    chain.block(h(1), 100, vec![tx_a, tx_b]);
    chain
        .steps
        .lock()
        .unwrap()
        .push(ChainStep { removed: vec![], added: vec![accepted(h(1), &[tx_id(0xA0), tx_id(0xB0)])] });

    sync_once(&chain, &mut store, Some(h(0)), |_| {}).await.unwrap();

    let summary = store.summary(&id).unwrap().unwrap();
    assert_eq!(summary.event_count, 2, "genesis + transition");
    assert_eq!(summary.live_utxos, 1, "A:0 was spent within the block; only B:0 is live");

    let utxos = store.utxos(&id, false).unwrap();
    let a0 = utxos.iter().find(|u| u.outpoint.txid == tx_id(0xA0)).unwrap();
    assert!(!a0.live, "intra-block-spent UTXO must not stay live");
    assert_eq!(a0.spent_txid, Some(tx_id(0xB0)));
    assert!(a0.spent_sig.is_some(), "the spend's signature script must be captured");

    // Write-time tx_index capture: each event carries its tx's 0-based
    // position in the accepting block's accepted-tx list.
    let events = store.events(&id).unwrap();
    assert_eq!(events[0].txid, tx_id(0xA0));
    assert_eq!(events[0].tx_index, Some(0));
    assert_eq!(events[1].txid, tx_id(0xB0));
    assert_eq!(events[1].tx_index, Some(1));
}
