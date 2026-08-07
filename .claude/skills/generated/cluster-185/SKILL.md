---
name: cluster-185
description: "Skill for the Cluster_185 area of KasDev. 13 symbols across 2 files."
---

# Cluster_185

13 symbols | 2 files | Cohesion: 76%

## When to Use

- Working with code in `crates/`
- Understanding how reorg_log work
- Modifying cluster_185-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov-core/src/sync.rs` | h, tx_id, utxo, walkable, walkable_with_blocks (+7) |
| `crates/kascov-core/src/store.rs` | reorg_log |

## Entry Points

Start here when exploring this area:

- **`reorg_log`** (Function) — `crates/kascov-core/src/store.rs:2524`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `reorg_log` | Function | `crates/kascov-core/src/store.rs` | 2524 |
| `h` | Function | `crates/kascov-core/src/sync.rs` | 1091 |
| `tx_id` | Function | `crates/kascov-core/src/sync.rs` | 1094 |
| `utxo` | Function | `crates/kascov-core/src/sync.rs` | 1228 |
| `walkable` | Function | `crates/kascov-core/src/sync.rs` | 1240 |
| `walkable_with_blocks` | Function | `crates/kascov-core/src/sync.rs` | 1249 |
| `stranded_store` | Function | `crates/kascov-core/src/sync.rs` | 1261 |
| `re_anchor_picks_newest_walkable_and_rolls_back_above` | Function | `crates/kascov-core/src/sync.rs` | 1299 |
| `re_anchor_rollback_records_reorg_log` | Function | `crates/kascov-core/src/sync.rs` | 1337 |
| `re_anchor_without_walkable_candidate_leaves_store_untouched` | Function | `crates/kascov-core/src/sync.rs` | 1356 |
| `re_anchor_healthy_cursor_short_circuits` | Function | `crates/kascov-core/src/sync.rs` | 1380 |
| `re_anchor_sees_through_empty_success_walks_when_lagging` | Function | `crates/kascov-core/src/sync.rs` | 1401 |
| `classify_pending_covers_genesis_transition_burn_and_noop` | Function | `crates/kascov-core/src/sync.rs` | 1586 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Classify_pending_covers_genesis_transition_burn_and_noop → Meta` | cross_community | 4 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Tests | 3 calls |
| Cluster_187 | 2 calls |
| Cluster_181 | 2 calls |
| Cluster_195 | 1 calls |
| Cluster_194 | 1 calls |
| Examples | 1 calls |

## How to Explore

1. `gitnexus_context({name: "reorg_log"})` — see callers and callees
2. `gitnexus_query({query: "cluster_185"})` — find related execution flows
3. Read key files listed above for implementation details
