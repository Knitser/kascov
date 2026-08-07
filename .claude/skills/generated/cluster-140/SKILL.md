---
name: cluster-140
description: "Skill for the Cluster_140 area of KasDev. 44 symbols across 4 files."
---

# Cluster_140

44 symbols | 4 files | Cohesion: 88%

## When to Use

- Working with code in `crates/`
- Understanding how simulate_input, blake2b32, xonly work
- Modifying cluster_140-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov-labkit/src/lib.rs` | simulate_input, blake2b32, xonly, p2sh_spk, entrypoint_spec (+28) |
| `crates/kascov-decode/src/lib.rs` | encode_push, emit, snum, silverscript_skeletons, all_silverscript_skeletons_derive (+3) |
| `crates/kascov-lab/src/main.rs` | main, examples |
| `crates/kascov-decode/src/kcc1.rs` | signature_script |

## Entry Points

Start here when exploring this area:

- **`simulate_input`** (Function) — `crates/kascov-labkit/src/lib.rs:46`
- **`blake2b32`** (Function) — `crates/kascov-labkit/src/lib.rs:72`
- **`xonly`** (Function) — `crates/kascov-labkit/src/lib.rs:82`
- **`p2sh_spk`** (Function) — `crates/kascov-labkit/src/lib.rs:87`
- **`entrypoint_spec`** (Function) — `crates/kascov-labkit/src/lib.rs:133`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `simulate_input` | Function | `crates/kascov-labkit/src/lib.rs` | 46 |
| `blake2b32` | Function | `crates/kascov-labkit/src/lib.rs` | 72 |
| `xonly` | Function | `crates/kascov-labkit/src/lib.rs` | 82 |
| `p2sh_spk` | Function | `crates/kascov-labkit/src/lib.rs` | 87 |
| `entrypoint_spec` | Function | `crates/kascov-labkit/src/lib.rs` | 133 |
| `load_or_create_key` | Function | `crates/kascov-labkit/src/lib.rs` | 181 |
| `address_of` | Function | `crates/kascov-labkit/src/lib.rs` | 205 |
| `keygen` | Function | `crates/kascov-labkit/src/lib.rs` | 211 |
| `balance` | Function | `crates/kascov-labkit/src/lib.rs` | 245 |
| `submit` | Function | `crates/kascov-labkit/src/lib.rs` | 278 |
| `demo` | Function | `crates/kascov-labkit/src/lib.rs` | 385 |
| `deploy` | Function | `crates/kascov-labkit/src/lib.rs` | 472 |
| `settle_escrow` | Function | `crates/kascov-labkit/src/lib.rs` | 570 |
| `escrow_demo` | Function | `crates/kascov-labkit/src/lib.rs` | 743 |
| `build_constrained_spend` | Function | `crates/kascov-labkit/src/lib.rs` | 811 |
| `spend` | Function | `crates/kascov-labkit/src/lib.rs` | 1106 |
| `contract_demo` | Function | `crates/kascov-labkit/src/lib.rs` | 1286 |
| `encode_push` | Function | `crates/kascov-decode/src/lib.rs` | 192 |
| `emit` | Function | `crates/kascov-decode/src/lib.rs` | 391 |
| `snum` | Function | `crates/kascov-decode/src/lib.rs` | 759 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Main → Empty` | cross_community | 5 |
| `Lastwill_refresh_shape_and_engine_pass → Is_push` | cross_community | 5 |
| `Lastwill_refresh_shape_and_engine_pass → Push_value` | cross_community | 5 |
| `Lastwill_refresh_shape_and_engine_pass → Skeleton` | cross_community | 5 |
| `Mecenas_receive_continuation_shape_and_engine_pass → Is_push` | cross_community | 5 |
| `Mecenas_receive_continuation_shape_and_engine_pass → Push_value` | cross_community | 5 |
| `Mecenas_receive_continuation_shape_and_engine_pass → Skeleton` | cross_community | 5 |
| `Main → Trim` | cross_community | 4 |
| `Anchor_passport → Empty` | cross_community | 4 |
| `Lastwill_refresh_shape_and_engine_pass → Snum` | intra_community | 4 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Cluster_179 | 9 calls |
| Js | 2 calls |
| Scripts | 2 calls |
| Node | 2 calls |
| Cluster_154 | 2 calls |
| Cluster_231 | 1 calls |
| Cluster_141 | 1 calls |
| Cluster_151 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "simulate_input"})` — see callers and callees
2. `gitnexus_query({query: "cluster_140"})` — find related execution flows
3. Read key files listed above for implementation details
