---
name: cluster-171
description: "Skill for the Cluster_171 area of KasDev. 13 symbols across 3 files."
---

# Cluster_171

13 symbols | 3 files | Cohesion: 60%

## When to Use

- Working with code in `crates/`
- Understanding how token_derivation_stamp, derive_minter, rederive_affected work
- Modifying cluster_171-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov-core/src/store.rs` | now_ms, force_reverify, token_status_snapshot, begin_derivation_run, finish_derivation_run (+2) |
| `crates/kascov-core/src/tokens.rs` | token_derivation_stamp, load_cells, derive_minter, rederive_affected |
| `crates/kascov-core/src/market.rs` | market_stamp, rederive_market_programs |

## Entry Points

Start here when exploring this area:

- **`token_derivation_stamp`** (Function) — `crates/kascov-core/src/tokens.rs:72`
- **`derive_minter`** (Function) — `crates/kascov-core/src/tokens.rs:1457`
- **`rederive_affected`** (Function) — `crates/kascov-core/src/tokens.rs:1534`
- **`force_reverify`** (Function) — `crates/kascov-core/src/store.rs:4294`
- **`derive_tokens_if_stale`** (Function) — `crates/kascov-core/src/store.rs:4833`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `token_derivation_stamp` | Function | `crates/kascov-core/src/tokens.rs` | 72 |
| `derive_minter` | Function | `crates/kascov-core/src/tokens.rs` | 1457 |
| `rederive_affected` | Function | `crates/kascov-core/src/tokens.rs` | 1534 |
| `force_reverify` | Function | `crates/kascov-core/src/store.rs` | 4294 |
| `derive_tokens_if_stale` | Function | `crates/kascov-core/src/store.rs` | 4833 |
| `market_stamp` | Function | `crates/kascov-core/src/market.rs` | 499 |
| `rederive_market_programs` | Function | `crates/kascov-core/src/market.rs` | 989 |
| `load_cells` | Function | `crates/kascov-core/src/tokens.rs` | 239 |
| `now_ms` | Function | `crates/kascov-core/src/store.rs` | 863 |
| `token_status_snapshot` | Function | `crates/kascov-core/src/store.rs` | 4501 |
| `begin_derivation_run` | Function | `crates/kascov-core/src/store.rs` | 4525 |
| `finish_derivation_run` | Function | `crates/kascov-core/src/store.rs` | 4561 |
| `fail_derivation_run` | Function | `crates/kascov-core/src/store.rs` | 4687 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Cluster_153 | 2 calls |
| Cluster_155 | 2 calls |
| Cluster_154 | 1 calls |
| Tests | 1 calls |
| Cluster_192 | 1 calls |
| Cluster_190 | 1 calls |
| Cluster_183 | 1 calls |
| Cluster_206 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "token_derivation_stamp"})` — see callers and callees
2. `gitnexus_query({query: "cluster_171"})` — find related execution flows
3. Read key files listed above for implementation details
