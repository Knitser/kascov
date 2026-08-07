---
name: examples
description: "Skill for the Examples area of KasDev. 48 symbols across 16 files."
---

# Examples

48 symbols | 16 files | Cohesion: 72%

## When to Use

- Working with code in `crates/`
- Understanding how prove_genesis_lock, p2sh_hash, revealed_template work
- Modifying examples-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `crates/kascov-decode/src/kcc20.rs` | revealed_template, decode_token_state, decode_state_block, owner, identifier_type (+14) |
| `crates/kascov/src/main.rs` | price_cache, parse_kraken_price, parse_coingecko_price, fetch_price, price_handler (+3) |
| `crates/kascov-decode/src/disasm.rs` | disassemble, opcode_info, disassembles_covenant_style_script, flags_truncated_push |
| `crates/kascov-core/src/tokens.rs` | arg_pushes, prove_recovered |
| `crates/kascov-core/src/store.rs` | restamp_kcc20_if_stale, derivation_runs |
| `crates/kascov/src/og.rs` | civil_date, iso_date |
| `crates/kascov-core/examples/audit_genesis.rs` | is_valid_genesis, main |
| `crates/kascov-decode/src/vesting.rs` | prove_genesis_lock |
| `crates/kascov-decode/src/lib.rs` | p2sh_hash |
| `crates/kascov-decode/examples/reg_probe.rs` | main |

## Entry Points

Start here when exploring this area:

- **`prove_genesis_lock`** (Function) — `crates/kascov-decode/src/vesting.rs:198`
- **`p2sh_hash`** (Function) — `crates/kascov-decode/src/lib.rs:822`
- **`revealed_template`** (Function) — `crates/kascov-decode/src/kcc20.rs:78`
- **`decode_token_state`** (Function) — `crates/kascov-decode/src/kcc20.rs:93`
- **`decode_state_block`** (Function) — `crates/kascov-decode/src/kcc20.rs:132`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `prove_genesis_lock` | Function | `crates/kascov-decode/src/vesting.rs` | 198 |
| `p2sh_hash` | Function | `crates/kascov-decode/src/lib.rs` | 822 |
| `revealed_template` | Function | `crates/kascov-decode/src/kcc20.rs` | 78 |
| `decode_token_state` | Function | `crates/kascov-decode/src/kcc20.rs` | 93 |
| `decode_state_block` | Function | `crates/kascov-decode/src/kcc20.rs` | 132 |
| `owner` | Function | `crates/kascov-decode/src/kcc20.rs` | 160 |
| `identifier_type` | Function | `crates/kascov-decode/src/kcc20.rs` | 164 |
| `amount` | Function | `crates/kascov-decode/src/kcc20.rs` | 168 |
| `end` | Function | `crates/kascov-decode/src/kcc20.rs` | 176 |
| `locate_state_block` | Function | `crates/kascov-decode/src/kcc20.rs` | 190 |
| `has_state_block` | Function | `crates/kascov-decode/src/kcc20.rs` | 221 |
| `kcc1_template_hash` | Function | `crates/kascov-decode/src/kcc20.rs` | 231 |
| `splice_token_state` | Function | `crates/kascov-decode/src/kcc20.rs` | 242 |
| `blake2b_256` | Function | `crates/kascov-decode/src/kcc20.rs` | 260 |
| `prove_output_state` | Function | `crates/kascov-decode/src/kcc20.rs` | 275 |
| `restamp_kcc20_if_stale` | Function | `crates/kascov-core/src/store.rs` | 4210 |
| `derivation_runs` | Function | `crates/kascov-core/src/store.rs` | 4697 |
| `iso_date` | Function | `crates/kascov/src/og.rs` | 259 |
| `disassemble` | Function | `crates/kascov-decode/src/disasm.rs` | 46 |
| `opcode_info` | Function | `crates/kascov-decode/src/disasm.rs` | 105 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Og_card_handler → Civil_date` | cross_community | 4 |
| `Vesting_handler → From_str` | cross_community | 4 |
| `Share_handler → Civil_date` | cross_community | 4 |
| `Feed_handler → Civil_date` | cross_community | 4 |
| `Follow_forever → P2sh_hash` | cross_community | 4 |
| `Follow_forever → StateBlock` | cross_community | 4 |
| `Real_mainnet_launch_recovers_the_creator_cell → StateBlock` | cross_community | 4 |
| `Price_handler → From_str` | intra_community | 4 |
| `Price_handler → As_str` | cross_community | 4 |
| `Registry_handler → From_str` | cross_community | 3 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Cluster_154 | 3 calls |
| Tests | 3 calls |
| Scripts | 2 calls |
| Cluster_221 | 2 calls |
| Cluster_145 | 1 calls |
| Js | 1 calls |
| Cluster_192 | 1 calls |
| Cluster_153 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "prove_genesis_lock"})` — see callers and callees
2. `gitnexus_query({query: "examples"})` — find related execution flows
3. Read key files listed above for implementation details
