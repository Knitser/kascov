---
name: cluster-134
description: "Skill for the Cluster_134 area of KasDev. 10 symbols across 2 files."
---

# Cluster_134

10 symbols | 2 files | Cohesion: 72%

## When to Use

- Working with code in `crates/`
- Understanding how simulate work
- Modifying cluster_134-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov-sim/src/lib.rs` | failing_rule, blake2b32, xonly, p2pk_spk, spec (+4) |
| `crates/kascov/src/main.rs` | simulate_handler |

## Entry Points

Start here when exploring this area:

- **`simulate`** (Function) — `crates/kascov-sim/src/lib.rs:173`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `simulate` | Function | `crates/kascov-sim/src/lib.rs` | 173 |
| `failing_rule` | Function | `crates/kascov-sim/src/lib.rs` | 115 |
| `blake2b32` | Function | `crates/kascov-sim/src/lib.rs` | 131 |
| `xonly` | Function | `crates/kascov-sim/src/lib.rs` | 140 |
| `p2pk_spk` | Function | `crates/kascov-sim/src/lib.rs` | 144 |
| `spec` | Function | `crates/kascov-sim/src/lib.rs` | 151 |
| `splice_field` | Function | `crates/kascov-sim/src/lib.rs` | 163 |
| `unknown_template_is_not_runnable` | Function | `crates/kascov-sim/src/lib.rs` | 673 |
| `concrete_trace_is_captured` | Function | `crates/kascov-sim/src/lib.rs` | 690 |
| `simulate_handler` | Function | `crates/kascov/src/main.rs` | 7355 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Simulate_handler → Trim` | cross_community | 4 |
| `Simulate_handler → Empty` | cross_community | 4 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Cluster_140 | 2 calls |
| Js | 1 calls |
| Scripts | 1 calls |
| Cluster_179 | 1 calls |
| Cluster_136 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "simulate"})` — see callers and callees
2. `gitnexus_query({query: "cluster_134"})` — find related execution flows
3. Read key files listed above for implementation details
