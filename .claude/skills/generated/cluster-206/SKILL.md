---
name: cluster-206
description: "Skill for the Cluster_206 area of KasDev. 29 symbols across 2 files."
---

# Cluster_206

29 symbols | 2 files | Cohesion: 92%

## When to Use

- Working with code in `crates/`
- Understanding how match_kron_curve, match_kron_curve_v2, match_kron_curve_v3 work
- Modifying cluster_206-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov-core/src/market.rs` | push_units, le_i64, match_kron_curve, match_kron_curve_v2, match_kron_curve_v3 (+23) |
| `crates/kascov-core/src/bench.rs` | replay_into |

## Entry Points

Start here when exploring this area:

- **`match_kron_curve`** (Function) — `crates/kascov-core/src/market.rs:246`
- **`match_kron_curve_v2`** (Function) — `crates/kascov-core/src/market.rs:301`
- **`match_kron_curve_v3`** (Function) — `crates/kascov-core/src/market.rs:364`
- **`unmatched_tag`** (Function) — `crates/kascov-core/src/market.rs:479`
- **`match_kron_pool`** (Function) — `crates/kascov-core/src/market.rs:522`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `match_kron_curve` | Function | `crates/kascov-core/src/market.rs` | 246 |
| `match_kron_curve_v2` | Function | `crates/kascov-core/src/market.rs` | 301 |
| `match_kron_curve_v3` | Function | `crates/kascov-core/src/market.rs` | 364 |
| `unmatched_tag` | Function | `crates/kascov-core/src/market.rs` | 479 |
| `match_kron_pool` | Function | `crates/kascov-core/src/market.rs` | 522 |
| `match_kron_pool_v2` | Function | `crates/kascov-core/src/market.rs` | 527 |
| `match_kron_pool_v3` | Function | `crates/kascov-core/src/market.rs` | 532 |
| `match_kron_pool_tn_a` | Function | `crates/kascov-core/src/market.rs` | 537 |
| `bracket_holds` | Function | `crates/kascov-core/src/market.rs` | 610 |
| `invariant_holds` | Function | `crates/kascov-core/src/market.rs` | 663 |
| `derive_market_program` | Function | `crates/kascov-core/src/market.rs` | 709 |
| `push_units` | Function | `crates/kascov-core/src/market.rs` | 177 |
| `le_i64` | Function | `crates/kascov-core/src/market.rs` | 233 |
| `match_pool_build` | Function | `crates/kascov-core/src/market.rs` | 541 |
| `the_fixture_matches_itself_and_reads_its_own_constants` | Function | `crates/kascov-core/src/market.rs` | 1422 |
| `one_flipped_byte_outside_the_slots_is_a_different_program` | Function | `crates/kascov-core/src/market.rs` | 1438 |
| `the_pool_build_matches_and_reads_its_state` | Function | `crates/kascov-core/src/market.rs` | 1457 |
| `the_v2_fixture_matches_itself_and_reads_its_own_constants` | Function | `crates/kascov-core/src/market.rs` | 1488 |
| `v2_one_flipped_byte_outside_the_slots_is_a_different_program` | Function | `crates/kascov-core/src/market.rs` | 1509 |
| `v2_a_program_lying_about_its_own_length_is_rejected` | Function | `crates/kascov-core/src/market.rs` | 1522 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Cluster_153 | 1 calls |
| Examples | 1 calls |
| Cluster_174 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "match_kron_curve"})` — see callers and callees
2. `gitnexus_query({query: "cluster_206"})` — find related execution flows
3. Read key files listed above for implementation details
