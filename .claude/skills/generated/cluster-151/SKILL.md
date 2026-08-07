---
name: cluster-151
description: "Skill for the Cluster_151 area of KasDev. 13 symbols across 2 files."
---

# Cluster_151

13 symbols | 2 files | Cohesion: 95%

## When to Use

- Working with code in `crates/`
- Understanding how observed_skeletons, observed_repeat_skeletons, derive work
- Modifying cluster_151-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov-decode/src/lib.rs` | push_value, is_push, derive, derive_observed, match_script (+6) |
| `crates/kascov-decode/src/observed.rs` | observed_skeletons, observed_repeat_skeletons |

## Entry Points

Start here when exploring this area:

- **`observed_skeletons`** (Function) — `crates/kascov-decode/src/observed.rs:27`
- **`observed_repeat_skeletons`** (Function) — `crates/kascov-decode/src/observed.rs:128`
- **`derive`** (Function) — `crates/kascov-decode/src/lib.rs:259`
- **`derive_observed`** (Function) — `crates/kascov-decode/src/lib.rs:313`
- **`derive`** (Function) — `crates/kascov-decode/src/lib.rs:497`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `observed_skeletons` | Function | `crates/kascov-decode/src/observed.rs` | 27 |
| `observed_repeat_skeletons` | Function | `crates/kascov-decode/src/observed.rs` | 128 |
| `derive` | Function | `crates/kascov-decode/src/lib.rs` | 259 |
| `derive_observed` | Function | `crates/kascov-decode/src/lib.rs` | 313 |
| `derive` | Function | `crates/kascov-decode/src/lib.rs` | 497 |
| `push_value` | Function | `crates/kascov-decode/src/lib.rs` | 237 |
| `is_push` | Function | `crates/kascov-decode/src/lib.rs` | 249 |
| `match_script` | Function | `crates/kascov-decode/src/lib.rs` | 408 |
| `match_skel_item` | Function | `crates/kascov-decode/src/lib.rs` | 425 |
| `fields_in_order` | Function | `crates/kascov-decode/src/lib.rs` | 448 |
| `same_shape` | Function | `crates/kascov-decode/src/lib.rs` | 463 |
| `match_script` | Function | `crates/kascov-decode/src/lib.rs` | 651 |
| `observed_skeletons_all_derive` | Function | `crates/kascov-decode/src/lib.rs` | 1074 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Lastwill_refresh_shape_and_engine_pass → Is_push` | cross_community | 5 |
| `Lastwill_refresh_shape_and_engine_pass → Push_value` | cross_community | 5 |
| `Lastwill_refresh_shape_and_engine_pass → Skeleton` | cross_community | 5 |
| `Mecenas_receive_continuation_shape_and_engine_pass → Is_push` | cross_community | 5 |
| `Mecenas_receive_continuation_shape_and_engine_pass → Push_value` | cross_community | 5 |
| `Mecenas_receive_continuation_shape_and_engine_pass → Skeleton` | cross_community | 5 |

## How to Explore

1. `gitnexus_context({name: "observed_skeletons"})` — see callers and callees
2. `gitnexus_query({query: "cluster_151"})` — find related execution flows
3. Read key files listed above for implementation details
