---
name: cluster-201
description: "Skill for the Cluster_201 area of KasDev. 12 symbols across 2 files."
---

# Cluster_201

12 symbols | 2 files | Cohesion: 77%

## When to Use

- Working with code in `crates/`
- Understanding how multi_covenant_txs, covenant_templates work
- Modifying cluster_201-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov/src/main.rs` | build_families, build_galaxy, build_galaxy_fmt, blake2b32, ev (+5) |
| `crates/kascov-core/src/store.rs` | multi_covenant_txs, covenant_templates |

## Entry Points

Start here when exploring this area:

- **`multi_covenant_txs`** (Function) — `crates/kascov-core/src/store.rs:3767`
- **`covenant_templates`** (Function) — `crates/kascov-core/src/store.rs:3824`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `multi_covenant_txs` | Function | `crates/kascov-core/src/store.rs` | 3767 |
| `covenant_templates` | Function | `crates/kascov-core/src/store.rs` | 3824 |
| `build_families` | Function | `crates/kascov/src/main.rs` | 5939 |
| `build_galaxy` | Function | `crates/kascov/src/main.rs` | 6057 |
| `build_galaxy_fmt` | Function | `crates/kascov/src/main.rs` | 6061 |
| `blake2b32` | Function | `crates/kascov/src/main.rs` | 6560 |
| `ev` | Function | `crates/kascov/src/main.rs` | 10140 |
| `galaxy_store` | Function | `crates/kascov/src/main.rs` | 10154 |
| `galaxy_clusters_nodes_and_edges` | Function | `crates/kascov/src/main.rs` | 10191 |
| `galaxy_fmt2_columnar_is_index_aligned_with_legacy` | Function | `crates/kascov/src/main.rs` | 10227 |
| `galaxy_members_form_an_organic_disc_not_a_single_ring` | Function | `crates/kascov/src/main.rs` | 10273 |
| `galaxy_core_tier_positions_match_full_tier` | Function | `crates/kascov/src/main.rs` | 10319 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Galaxy_members_form_an_organic_disc_not_a_single_ring → Meta` | cross_community | 5 |
| `Galaxy_core_tier_positions_match_full_tier → Db_err` | cross_community | 4 |
| `Galaxy_core_tier_positions_match_full_tier → Meta` | cross_community | 4 |
| `Galaxy_core_tier_positions_match_full_tier → Set_meta` | cross_community | 4 |
| `Galaxy_core_tier_positions_match_full_tier → Multi_covenant_txs` | intra_community | 4 |
| `Galaxy_core_tier_positions_match_full_tier → Covenant_templates` | intra_community | 4 |
| `Galaxy_core_tier_positions_match_full_tier → Active_flags` | cross_community | 4 |
| `Galaxy_fmt2_columnar_is_index_aligned_with_legacy → Db_err` | cross_community | 4 |
| `Galaxy_fmt2_columnar_is_index_aligned_with_legacy → Meta` | cross_community | 4 |
| `Galaxy_fmt2_columnar_is_index_aligned_with_legacy → Set_meta` | cross_community | 4 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Tests | 3 calls |
| Cluster_221 | 2 calls |
| Cluster_194 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "multi_covenant_txs"})` — see callers and callees
2. `gitnexus_query({query: "cluster_201"})` — find related execution flows
3. Read key files listed above for implementation details
