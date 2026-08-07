---
name: cluster-179
description: "Skill for the Cluster_179 area of KasDev. 41 symbols across 2 files."
---

# Cluster_179

41 symbols | 2 files | Cohesion: 92%

## When to Use

- Working with code in `crates/`
- Understanding how rollback, raw_conn, token_trades_page work
- Modifying cluster_179-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov-core/src/tokens.rs` | program, spk, sig, sig_no_args, amt (+33) |
| `crates/kascov-core/src/store.rs` | rollback, raw_conn, token_trades_page |

## Entry Points

Start here when exploring this area:

- **`rollback`** (Function) — `crates/kascov-core/src/store.rs:1779`
- **`raw_conn`** (Function) — `crates/kascov-core/src/store.rs:2474`
- **`token_trades_page`** (Function) — `crates/kascov-core/src/store.rs:5106`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `rollback` | Function | `crates/kascov-core/src/store.rs` | 1779 |
| `raw_conn` | Function | `crates/kascov-core/src/store.rs` | 2474 |
| `token_trades_page` | Function | `crates/kascov-core/src/store.rs` | 5106 |
| `program` | Function | `crates/kascov-core/src/tokens.rs` | 1863 |
| `spk` | Function | `crates/kascov-core/src/tokens.rs` | 1867 |
| `sig` | Function | `crates/kascov-core/src/tokens.rs` | 1891 |
| `sig_no_args` | Function | `crates/kascov-core/src/tokens.rs` | 1915 |
| `amt` | Function | `crates/kascov-core/src/tokens.rs` | 1919 |
| `owner` | Function | `crates/kascov-core/src/tokens.rs` | 1923 |
| `test_store` | Function | `crates/kascov-core/src/tokens.rs` | 1927 |
| `new` | Function | `crates/kascov-core/src/tokens.rs` | 1941 |
| `event` | Function | `crates/kascov-core/src/tokens.rs` | 1948 |
| `out` | Function | `crates/kascov-core/src/tokens.rs` | 1960 |
| `out_v` | Function | `crates/kascov-core/src/tokens.rs` | 1975 |
| `out_spk` | Function | `crates/kascov-core/src/tokens.rs` | 1990 |
| `spend` | Function | `crates/kascov-core/src/tokens.rs` | 2003 |
| `apply` | Function | `crates/kascov-core/src/tokens.rs` | 2022 |
| `row` | Function | `crates/kascov-core/src/tokens.rs` | 2028 |
| `minter_state` | Function | `crates/kascov-core/src/tokens.rs` | 2048 |
| `holder` | Function | `crates/kascov-core/src/tokens.rs` | 2051 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Main → Empty` | cross_community | 5 |
| `Real_mainnet_launch_recovers_the_creator_cell → Meta` | cross_community | 5 |
| `Trades_extract_from_proven_deltas_and_opposite_kas → Meta` | cross_community | 5 |
| `Supply_splits_by_owner_type → Meta` | cross_community | 5 |
| `Genesis_creator_allocation_recovers_without_a_reveal → Meta` | cross_community | 5 |
| `A_graduation_is_not_a_trade → Meta` | cross_community | 5 |
| `Reveal_rollback_regresses_verdict → Meta` | cross_community | 5 |
| `Minter_escalation_is_invalid → Meta` | cross_community | 5 |
| `Amount_bounds_are_conservative → Meta` | cross_community | 5 |
| `Opaque_frontier_is_unvalidated → Meta` | cross_community | 5 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Tests | 6 calls |
| Examples | 3 calls |
| Cluster_189 | 2 calls |
| Cluster_172 | 2 calls |
| Cluster_171 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "rollback"})` — see callers and callees
2. `gitnexus_query({query: "cluster_179"})` — find related execution flows
3. Read key files listed above for implementation details
