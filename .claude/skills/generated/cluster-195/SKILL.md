---
name: cluster-195
description: "Skill for the Cluster_195 area of KasDev. 16 symbols across 2 files."
---

# Cluster_195

16 symbols | 2 files | Cohesion: 62%

## When to Use

- Working with code in `crates/`
- Understanding how backup_to, accepting_block_of, utxo_covenant work
- Modifying cluster_195-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov/src/main.rs` | main, export, live_path, db_path, open_store (+6) |
| `crates/kascov-core/src/store.rs` | backup_to, accepting_block_of, utxo_covenant, recover_program, utxos |

## Entry Points

Start here when exploring this area:

- **`backup_to`** (Function) — `crates/kascov-core/src/store.rs:1319`
- **`accepting_block_of`** (Function) — `crates/kascov-core/src/store.rs:2941`
- **`utxo_covenant`** (Function) — `crates/kascov-core/src/store.rs:2955`
- **`recover_program`** (Function) — `crates/kascov-core/src/store.rs:4290`
- **`utxos`** (Function) — `crates/kascov-core/src/store.rs:5591`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `backup_to` | Function | `crates/kascov-core/src/store.rs` | 1319 |
| `accepting_block_of` | Function | `crates/kascov-core/src/store.rs` | 2941 |
| `utxo_covenant` | Function | `crates/kascov-core/src/store.rs` | 2955 |
| `recover_program` | Function | `crates/kascov-core/src/store.rs` | 4290 |
| `utxos` | Function | `crates/kascov-core/src/store.rs` | 5591 |
| `main` | Function | `crates/kascov/src/main.rs` | 154 |
| `export` | Function | `crates/kascov/src/main.rs` | 276 |
| `live_path` | Function | `crates/kascov/src/main.rs` | 302 |
| `db_path` | Function | `crates/kascov/src/main.rs` | 752 |
| `open_store` | Function | `crates/kascov/src/main.rs` | 761 |
| `inspect_tx` | Function | `crates/kascov/src/main.rs` | 917 |
| `sync` | Function | `crates/kascov/src/main.rs` | 997 |
| `sync_session` | Function | `crates/kascov/src/main.rs` | 1023 |
| `list` | Function | `crates/kascov/src/main.rs` | 1094 |
| `show` | Function | `crates/kascov/src/main.rs` | 1138 |
| `trace` | Function | `crates/kascov/src/main.rs` | 1238 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Main → ServerInfo` | cross_community | 5 |
| `Main → CovenantSighting` | cross_community | 4 |
| `Main → Outpoint` | cross_community | 4 |
| `Main → Db_path` | intra_community | 4 |
| `Main → UtxoRow` | intra_community | 4 |
| `Main → Abbrev` | cross_community | 3 |
| `Main → Summary` | cross_community | 3 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Tests | 6 calls |
| Node | 5 calls |
| Cluster_171 | 2 calls |
| Cluster_153 | 2 calls |
| Cluster_142 | 1 calls |
| Cluster_203 | 1 calls |
| Examples | 1 calls |
| Cluster_194 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "backup_to"})` — see callers and callees
2. `gitnexus_query({query: "cluster_195"})` — find related execution flows
3. Read key files listed above for implementation details
