---
name: cluster-194
description: "Skill for the Cluster_194 area of KasDev. 29 symbols across 2 files."
---

# Cluster_194

29 symbols | 2 files | Cohesion: 60%

## When to Use

- Working with code in `crates/`
- Understanding how set_tip, stamp_tx_indices, list work
- Modifying cluster_194-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov-core/src/store.rs` | set_tip, stamp_tx_indices, list, list_page, activity (+20) |
| `crates/kascov/src/main.rs` | build_snapshot, build_sitemap_xml, sitemap_carries_lastmod_from_last_activity, sitemap_without_a_store_still_lists_the_root |

## Entry Points

Start here when exploring this area:

- **`set_tip`** (Function) — `crates/kascov-core/src/store.rs:1287`
- **`stamp_tx_indices`** (Function) — `crates/kascov-core/src/store.rs:1978`
- **`list`** (Function) — `crates/kascov-core/src/store.rs:2544`
- **`list_page`** (Function) — `crates/kascov-core/src/store.rs:2561`
- **`activity`** (Function) — `crates/kascov-core/src/store.rs:2730`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `set_tip` | Function | `crates/kascov-core/src/store.rs` | 1287 |
| `stamp_tx_indices` | Function | `crates/kascov-core/src/store.rs` | 1978 |
| `list` | Function | `crates/kascov-core/src/store.rs` | 2544 |
| `list_page` | Function | `crates/kascov-core/src/store.rs` | 2561 |
| `activity` | Function | `crates/kascov-core/src/store.rs` | 2730 |
| `born_values` | Function | `crates/kascov-core/src/store.rs` | 2832 |
| `lane_activity` | Function | `crates/kascov-core/src/store.rs` | 3405 |
| `active_flags` | Function | `crates/kascov-core/src/store.rs` | 3804 |
| `token_balances_page` | Function | `crates/kascov-core/src/store.rs` | 5035 |
| `global_token_trades_page` | Function | `crates/kascov-core/src/store.rs` | 5185 |
| `test_store` | Function | `crates/kascov-core/src/store.rs` | 5638 |
| `block_with_events` | Function | `crates/kascov-core/src/store.rs` | 5642 |
| `claimed_image_keeps_only_conforming_schemes` | Function | `crates/kascov-core/src/store.rs` | 5674 |
| `lane_detail_includes_payload_tag_lanes` | Function | `crates/kascov-core/src/store.rs` | 5719 |
| `find_daa_gap_spots_the_reset_discontinuity` | Function | `crates/kascov-core/src/store.rs` | 5776 |
| `finalize_gap_recovery_resequences_and_rederives_citing_tokens` | Function | `crates/kascov-core/src/store.rs` | 5800 |
| `tip_roundtrip_and_overwrite` | Function | `crates/kascov-core/src/store.rs` | 6249 |
| `digest_windows_and_headliners` | Function | `crates/kascov-core/src/store.rs` | 6318 |
| `activity_buckets_and_bounds` | Function | `crates/kascov-core/src/store.rs` | 6372 |
| `cov_by_activity_index_serves_list_page` | Function | `crates/kascov-core/src/store.rs` | 6724 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Folded_born_value_and_template_match_map_queries → Meta` | cross_community | 5 |
| `Galaxy_core_tier_positions_match_full_tier → Active_flags` | cross_community | 4 |
| `Galaxy_fmt2_columnar_is_index_aligned_with_legacy → Active_flags` | cross_community | 4 |
| `Sitemap_carries_lastmod_from_last_activity → Meta` | cross_community | 4 |
| `Folded_born_value_and_template_match_map_queries → Db_err` | cross_community | 4 |
| `Folded_born_value_and_template_match_map_queries → Set_meta` | cross_community | 4 |
| `Galaxy_members_form_an_organic_disc_not_a_single_ring → Active_flags` | cross_community | 4 |
| `Sitemap_handler → List_page` | cross_community | 3 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Tests | 9 calls |
| Cluster_189 | 9 calls |
| Cluster_197 | 1 calls |
| Cluster_179 | 1 calls |
| Cluster_172 | 1 calls |
| Cluster_201 | 1 calls |
| Cluster_187 | 1 calls |
| Cluster_154 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "set_tip"})` — see callers and callees
2. `gitnexus_query({query: "cluster_194"})` — find related execution flows
3. Read key files listed above for implementation details
