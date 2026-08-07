---
name: cluster-209
description: "Skill for the Cluster_209 area of KasDev. 11 symbols across 2 files."
---

# Cluster_209

11 symbols | 2 files | Cohesion: 67%

## When to Use

- Working with code in `crates/`
- Understanding how ensure_witness_schema, media_db_path, open_media_db work
- Modifying cluster_209-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov/src/witness.rs` | ensure_witness_schema, media_db_path, open_media_db, load_row, save_effect (+2) |
| `crates/kascov/src/main.rs` | registry_client, witness_client, witness_forever, listed_img_handler |

## Entry Points

Start here when exploring this area:

- **`ensure_witness_schema`** (Function) — `crates/kascov/src/witness.rs:296`
- **`media_db_path`** (Function) — `crates/kascov/src/witness.rs:327`
- **`open_media_db`** (Function) — `crates/kascov/src/witness.rs:331`
- **`load_row`** (Function) — `crates/kascov/src/witness.rs:346`
- **`save_effect`** (Function) — `crates/kascov/src/witness.rs:380`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `ensure_witness_schema` | Function | `crates/kascov/src/witness.rs` | 296 |
| `media_db_path` | Function | `crates/kascov/src/witness.rs` | 327 |
| `open_media_db` | Function | `crates/kascov/src/witness.rs` | 331 |
| `load_row` | Function | `crates/kascov/src/witness.rs` | 346 |
| `save_effect` | Function | `crates/kascov/src/witness.rs` | 380 |
| `serve_lookup` | Function | `crates/kascov/src/witness.rs` | 447 |
| `storage_round_trips_and_serves_witnessed_only` | Function | `crates/kascov/src/witness.rs` | 664 |
| `registry_client` | Function | `crates/kascov/src/main.rs` | 4422 |
| `witness_client` | Function | `crates/kascov/src/main.rs` | 4685 |
| `witness_forever` | Function | `crates/kascov/src/main.rs` | 4774 |
| `listed_img_handler` | Function | `crates/kascov/src/main.rs` | 4888 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Vesting_handler → Registry_client` | cross_community | 4 |
| `Listed_img_handler → WitnessRow` | intra_community | 4 |
| `Registry_handler → Registry_client` | cross_community | 3 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Cluster_208 | 4 calls |
| Tests | 2 calls |
| Scripts | 1 calls |
| Web | 1 calls |
| Cluster_207 | 1 calls |
| Js | 1 calls |

## How to Explore

1. `gitnexus_context({name: "ensure_witness_schema"})` — see callers and callees
2. `gitnexus_query({query: "cluster_209"})` — find related execution flows
3. Read key files listed above for implementation details
