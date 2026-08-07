---
name: cluster-189
description: "Skill for the Cluster_189 area of KasDev. 20 symbols across 1 files."
---

# Cluster_189

20 symbols | 1 files | Cohesion: 54%

## When to Use

- Working with code in `crates/`
- Understanding how empty, reset_cursor, apply work
- Modifying cluster_189-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov-core/src/store.rs` | empty, reset_cursor, apply, template_stats, lane_recent (+15) |

## Entry Points

Start here when exploring this area:

- **`empty`** (Function) — `crates/kascov-core/src/store.rs:761`
- **`reset_cursor`** (Function) — `crates/kascov-core/src/store.rs:1314`
- **`apply`** (Function) — `crates/kascov-core/src/store.rs:1587`
- **`template_stats`** (Function) — `crates/kascov-core/src/store.rs:2879`
- **`lane_recent`** (Function) — `crates/kascov-core/src/store.rs:3375`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `empty` | Function | `crates/kascov-core/src/store.rs` | 761 |
| `reset_cursor` | Function | `crates/kascov-core/src/store.rs` | 1314 |
| `apply` | Function | `crates/kascov-core/src/store.rs` | 1587 |
| `template_stats` | Function | `crates/kascov-core/src/store.rs` | 2879 |
| `lane_recent` | Function | `crates/kascov-core/src/store.rs` | 3375 |
| `spent_by_txid` | Function | `crates/kascov-core/src/store.rs` | 3427 |
| `covenants_by_id_range` | Function | `crates/kascov-core/src/store.rs` | 3883 |
| `covenants_by_pubkey` | Function | `crates/kascov-core/src/store.rs` | 3927 |
| `recent_events` | Function | `crates/kascov-core/src/store.rs` | 4061 |
| `events_after` | Function | `crates/kascov-core/src/store.rs` | 4102 |
| `id_range_scan_maps_hex_prefixes` | Function | `crates/kascov-core/src/store.rs` | 5904 |
| `processed_daa_tracks_applies_and_skips_empty` | Function | `crates/kascov-core/src/store.rs` | 6259 |
| `recent_events_orders_newest_first_and_limits` | Function | `crates/kascov-core/src/store.rs` | 6280 |
| `covenants_by_pubkey_matches_exact_p2pk_states` | Function | `crates/kascov-core/src/store.rs` | 6425 |
| `template_stats_recognize_and_bucket` | Function | `crates/kascov-core/src/store.rs` | 6511 |
| `template_stats_aggregate_by_resolved_covenant_name` | Function | `crates/kascov-core/src/store.rs` | 6638 |
| `lane_dashboard_buckets_and_recent` | Function | `crates/kascov-core/src/store.rs` | 6921 |
| `spent_by_txid_returns_witness` | Function | `crates/kascov-core/src/store.rs` | 6970 |
| `ordering_key_daa_then_tx_index_sorts_interleaving` | Function | `crates/kascov-core/src/store.rs` | 7303 |
| `events_feed_cursor_walks_interleavings` | Function | `crates/kascov-core/src/store.rs` | 7384 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Main → Empty` | cross_community | 5 |
| `Rollback_reapply_equals_from_scratch → Empty` | cross_community | 4 |
| `Anchor_passport → Empty` | cross_community | 4 |
| `Mecenas_receive_continuation_shape_and_engine_pass → Empty` | cross_community | 4 |
| `Simulate_handler → Empty` | cross_community | 4 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Cluster_194 | 12 calls |
| Cluster_153 | 3 calls |
| Cluster_172 | 2 calls |
| Cluster_191 | 1 calls |
| Cluster_171 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "empty"})` — see callers and callees
2. `gitnexus_query({query: "cluster_189"})` — find related execution flows
3. Read key files listed above for implementation details
