---
name: tests
description: "Skill for the Tests area of KasDev. 187 symbols across 12 files."
---

# Tests

187 symbols | 12 files | Cohesion: 70%

## When to Use

- Working with code in `crates/`
- Understanding how verify_zk_script, owner_display, db_err work
- Modifying tests-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov/src/main.rs` | now_ms, build_live_snapshot, build_digest, build_activity_snapshot, build_grid_snapshot (+88) |
| `crates/kascov-core/src/store.rs` | db_err, open, tip, processed_daa, backfill_payload_tags (+37) |
| `crates/kascov/src/api.rs` | bad_request, parse_limit, parse_id, holder_json, trim_complete_event_page (+21) |
| `crates/kascov-core/tests/sync_replay.rs` | h, tx_id, cov, valid_genesis_id, covenant_tx (+4) |
| `crates/kascov-core/tests/gap_recovery.rs` | h, tx_id, covenant_tx, block, gap_recovery_merges_canonical_history_and_is_idempotent |
| `crates/kascov-decode/tests/template_hash_conformance.rs` | unhex, the_hash_is_stable_for_the_same_parts, an_external_implementation_agrees |
| `crates/kascov-core/tests/kcc20_worked_example.rs` | h32, worked_example_replays_and_verifies |
| `crates/kascov-core/src/tokens.rs` | owner_display, kinds |
| `crates/kascov/src/og.rs` | friendly_name, fmt_date |
| `crates/kascov-sim/src/lib.rs` | verify_zk_script |

## Entry Points

Start here when exploring this area:

- **`verify_zk_script`** (Function) — `crates/kascov-sim/src/lib.rs:498`
- **`owner_display`** (Function) — `crates/kascov-core/src/tokens.rs:200`
- **`db_err`** (Function) — `crates/kascov-core/src/store.rs:857`
- **`open`** (Function) — `crates/kascov-core/src/store.rs:988`
- **`tip`** (Function) — `crates/kascov-core/src/store.rs:1299`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `verify_zk_script` | Function | `crates/kascov-sim/src/lib.rs` | 498 |
| `owner_display` | Function | `crates/kascov-core/src/tokens.rs` | 200 |
| `db_err` | Function | `crates/kascov-core/src/store.rs` | 857 |
| `open` | Function | `crates/kascov-core/src/store.rs` | 988 |
| `tip` | Function | `crates/kascov-core/src/store.rs` | 1299 |
| `processed_daa` | Function | `crates/kascov-core/src/store.rs` | 1307 |
| `tx_index_backfill_done` | Function | `crates/kascov-core/src/store.rs` | 2016 |
| `summary` | Function | `crates/kascov-core/src/store.rs` | 2597 |
| `digest` | Function | `crates/kascov-core/src/store.rs` | 2642 |
| `event_daa_bounds` | Function | `crates/kascov-core/src/store.rs` | 2760 |
| `lifespan_stats` | Function | `crates/kascov-core/src/store.rs` | 2778 |
| `revealed_template_counts` | Function | `crates/kascov-core/src/store.rs` | 2923 |
| `put_verified_source` | Function | `crates/kascov-core/src/store.rs` | 3182 |
| `get_verified_source` | Function | `crates/kascov-core/src/store.rs` | 3201 |
| `lane_stats` | Function | `crates/kascov-core/src/store.rs` | 3361 |
| `claimed_token_meta` | Function | `crates/kascov-core/src/store.rs` | 3551 |
| `token_image` | Function | `crates/kascov-core/src/store.rs` | 3633 |
| `put_token_image` | Function | `crates/kascov-core/src/store.rs` | 3650 |
| `covenant_kcc1_hashes` | Function | `crates/kascov-core/src/store.rs` | 3671 |
| `covenants_by_kcc1_hash` | Function | `crates/kascov-core/src/store.rs` | 3690 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Lane_mint_handler → Val` | cross_community | 6 |
| `Publish_handler → Val` | cross_community | 6 |
| `Prove_holding_handler → Val` | cross_community | 6 |
| `Zk_verify_handler → Val` | cross_community | 6 |
| `Preflight_handler → Val` | cross_community | 6 |
| `Compile_handler → Val` | cross_community | 6 |
| `Lane_mint_handler → Ct_eq` | cross_community | 5 |
| `Lane_mint_handler → Lane_mac` | cross_community | 5 |
| `Publish_handler → Ct_eq` | cross_community | 5 |
| `Publish_handler → Lane_mac` | cross_community | 5 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Js | 12 calls |
| Cluster_189 | 10 calls |
| Examples | 8 calls |
| Cluster_194 | 7 calls |
| Cluster_192 | 7 calls |
| Cluster_195 | 4 calls |
| Scripts | 4 calls |
| Cluster_221 | 4 calls |

## How to Explore

1. `gitnexus_context({name: "verify_zk_script"})` — see callers and callees
2. `gitnexus_query({query: "tests"})` — find related execution flows
3. Read key files listed above for implementation details
