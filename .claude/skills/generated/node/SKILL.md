---
name: node
description: "Skill for the Node area of KasDev. 16 symbols across 5 files."
---

# Node

16 symbols | 5 files | Cohesion: 67%

## When to Use

- Working with code in `crates/`
- Understanding how cursor, dag_info, block_with_txs work
- Modifying node-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov-core/src/node/wrpc.rs` | dag_info, block_with_txs, virtual_chain_from, from_hash, to_hash (+5) |
| `crates/kascov/src/main.rs` | recover_wedged_cursor, scan, abbrev |
| `crates/kascov-core/src/store.rs` | cursor |
| `crates/kascov-lab/src/main.rs` | recover_gap |
| `crates/kascov-core/src/detect.rs` | covenant_sightings |

## Entry Points

Start here when exploring this area:

- **`cursor`** (Function) — `crates/kascov-core/src/store.rs:1281`
- **`dag_info`** (Function) — `crates/kascov-core/src/node/wrpc.rs:71`
- **`block_with_txs`** (Function) — `crates/kascov-core/src/node/wrpc.rs:81`
- **`virtual_chain_from`** (Function) — `crates/kascov-core/src/node/wrpc.rs:91`
- **`covenant_sightings`** (Function) — `crates/kascov-core/src/detect.rs:29`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `cursor` | Function | `crates/kascov-core/src/store.rs` | 1281 |
| `dag_info` | Function | `crates/kascov-core/src/node/wrpc.rs` | 71 |
| `block_with_txs` | Function | `crates/kascov-core/src/node/wrpc.rs` | 81 |
| `virtual_chain_from` | Function | `crates/kascov-core/src/node/wrpc.rs` | 91 |
| `covenant_sightings` | Function | `crates/kascov-core/src/detect.rs` | 29 |
| `connect` | Function | `crates/kascov-core/src/node/wrpc.rs` | 24 |
| `server_info` | Function | `crates/kascov-core/src/node/wrpc.rs` | 61 |
| `mempool_txs` | Function | `crates/kascov-core/src/node/wrpc.rs` | 124 |
| `recover_wedged_cursor` | Function | `crates/kascov/src/main.rs` | 2000 |
| `from_hash` | Function | `crates/kascov-core/src/node/wrpc.rs` | 142 |
| `to_hash` | Function | `crates/kascov-core/src/node/wrpc.rs` | 146 |
| `map_block` | Function | `crates/kascov-core/src/node/wrpc.rs` | 150 |
| `recover_gap` | Function | `crates/kascov-lab/src/main.rs` | 311 |
| `scan` | Function | `crates/kascov/src/main.rs` | 1323 |
| `abbrev` | Function | `crates/kascov/src/main.rs` | 1417 |
| `map_tx` | Function | `crates/kascov-core/src/node/wrpc.rs` | 179 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Main → ServerInfo` | cross_community | 5 |
| `Main → CovenantSighting` | cross_community | 4 |
| `Main → Outpoint` | cross_community | 4 |
| `Main → Abbrev` | cross_community | 3 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Tests | 2 calls |
| Cluster_192 | 1 calls |
| Cluster_181 | 1 calls |
| Cluster_189 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "cursor"})` — see callers and callees
2. `gitnexus_query({query: "node"})` — find related execution flows
3. Read key files listed above for implementation details
