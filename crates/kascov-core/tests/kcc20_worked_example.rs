//! The recon's hand-verified worked example, replayed byte-for-byte from
//! real TN10 rows: KCC20 token 02035650… (issuance controller 138d5802…).
//!
//! Ground truth (every state hash-proven on chain):
//!   1. 40dcce57… GENESIS: minter branch (owner = controller id, type 0x02,
//!      amount 0, isMinter). Supply 0.
//!   2-5. four MINTs of exactly 199,469,400 each.
//!   6. e386469c… MINT (frontier): both outputs LIVE — the minter branch at
//!      0 and the holder at 997,347,000 = 5 × 199,469,400, recoverable only
//!      via witness splice-and-hash.
//! Audit: genesis 0 + minted 997,347,000 − burned 0 = Σ live frontier.
//!
//! The fixture (tests/fixtures/kcc20_worked_example.json) is the verbatim
//! covenant_utxos + covenant_events extract for the token covenant from the
//! prod TN10 index (2026-07-12 backup).

use kascov_core::model::{BlockHash, CovenantId, Network, Outpoint, TxId};
use kascov_core::store::{BlockEvents, EventKind, NewEvent, NewUtxo, Store};
use kascov_core::tokens::STATUS_VERIFIED;

#[derive(serde::Deserialize)]
struct Fixture {
    covenant_id: String,
    events: Vec<Ev>,
    utxos: Vec<Utxo>,
}

#[derive(serde::Deserialize)]
struct Ev {
    seq: u64,
    kind: String,
    txid: String,
    accepting_daa: u64,
    tx_index: Option<u32>,
}

#[derive(serde::Deserialize)]
struct Utxo {
    txid: String,
    output_index: u32,
    value: u64,
    spk_version: u16,
    spk_script: String,
    spent_txid: Option<String>,
    spent_sig: Option<String>,
}

fn h32(s: &str) -> [u8; 32] {
    let mut b = [0u8; 32];
    hex::decode_to_slice(s, &mut b).unwrap();
    b
}

#[test]
fn worked_example_replays_and_verifies() {
    let fixture: Fixture = serde_json::from_str(include_str!("fixtures/kcc20_worked_example.json")).unwrap();
    let cov = CovenantId(h32(&fixture.covenant_id));

    let db = std::env::temp_dir()
        .join(format!("kascov-worked-example-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let mut store = Store::open(&db, Network::Testnet(10)).unwrap();

    // Replay: one accepting block per event, in seq order — created outputs
    // of the event's tx plus the spends that tx performed, exactly as the
    // live sync would have applied them.
    for ev in &fixture.events {
        let mut block = BlockEvents::empty(BlockHash([ev.seq as u8 + 1; 32]));
        block.accepting_daa = ev.accepting_daa;
        block.accepting_time_ms = ev.accepting_daa * 100;
        block.accepting_blue_score = ev.accepting_daa;
        for u in fixture.utxos.iter().filter(|u| u.txid == ev.txid) {
            block.created_utxos.push(NewUtxo {
                outpoint: Outpoint { txid: TxId(h32(&u.txid)), index: u.output_index },
                covenant_id: cov,
                value: u.value,
                spk_version: u.spk_version,
                spk_script: hex::decode(&u.spk_script).unwrap(),
            });
        }
        for u in fixture.utxos.iter().filter(|u| u.spent_txid.as_deref() == Some(ev.txid.as_str())) {
            block.spent_utxos.push((
                Outpoint { txid: TxId(h32(&u.txid)), index: u.output_index },
                TxId(h32(&ev.txid)),
                hex::decode(u.spent_sig.as_deref().expect("fixture spends carry sigs")).unwrap(),
                0,
            ));
        }
        block.events.push(NewEvent {
            covenant_id: cov,
            kind: match ev.kind.as_str() {
                "genesis" => EventKind::Genesis,
                "burn" => EventKind::Burn,
                _ => EventKind::Transition,
            },
            txid: TxId(h32(&ev.txid)),
            tx_index: ev.tx_index.unwrap_or(0),
            payload: None,
            lane_namespace: None,
        });
        let hash = block.accepting_block;
        store.apply(&block, hash).unwrap();
    }

    // The apply hook derived incrementally; the arithmetic must match the
    // hand audit exactly.
    let t = store.token_row(&cov).unwrap().expect("token derived");
    assert_eq!(t.validation, STATUS_VERIFIED, "reason: {:?}", t.invalid_reason);
    assert_eq!(t.supply, Some(997_347_000));
    assert_eq!(t.minted, Some(997_347_000), "5 × 199,469,400");
    assert_eq!(t.burned, Some(0));
    assert_eq!(t.unresolved_cells, 0, "frontier fully recovered via splice-and-hash");
    assert_eq!(t.holders, 2, "minter branch + the single holder");
    assert_eq!(t.template.as_deref(), Some("KCC20 token"));

    // Frontier balances: minter branch (controller-owned, type 0x02) at 0,
    // holder pubkey (type 0x00) at the full supply.
    let balances = store.token_balances(&cov, 10).unwrap();
    assert_eq!(balances.len(), 2);
    assert_eq!(balances[0].balance, 997_347_000);
    assert!(balances[0].owner.starts_with("00"), "top holder is a pubkey owner");
    assert_eq!(balances[1].balance, 0);
    assert!(balances[1].owner.starts_with("02"), "minter branch is covenant-owned");

    // Classification: genesis, then five mints of the constant quantum.
    let events = store.token_events_page(&cov, None, 100).unwrap();
    let mut kinds: Vec<(u64, String)> = events.iter().map(|e| (e.seq, e.kind.clone())).collect();
    kinds.dedup();
    assert_eq!(
        kinds,
        vec![
            (0, "genesis".into()),
            (1, "mint".into()),
            (2, "mint".into()),
            (3, "mint".into()),
            (4, "mint".into()),
            (5, "mint".into()),
        ]
    );
    // every mint's holder post-state is exactly one more constant quantum
    for seq in 1..=5u64 {
        let holder_amount: i64 = events
            .iter()
            .filter(|e| e.seq == seq && e.owner_to.as_deref().is_some_and(|o| o.starts_with("00")))
            .map(|e| e.amount.unwrap())
            .sum();
        assert_eq!(holder_amount, 199_469_400 * seq as i64, "constant mint quantum × {seq}");
    }

    // The from-scratch boot pass must agree with the incremental hook.
    let hook_row = serde_json::to_value(store.token_row(&cov).unwrap().unwrap()).unwrap();
    assert_eq!(store.derive_tokens_if_stale().unwrap(), 1);
    let boot_row = serde_json::to_value(store.token_row(&cov).unwrap().unwrap()).unwrap();
    assert_eq!(hook_row, boot_row);

    let _ = std::fs::remove_file(&db);
}
