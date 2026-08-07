---
name: js
description: "Skill for the Js area of KasDev. 40 symbols across 8 files."
---

# Js

40 symbols | 8 files | Cohesion: 71%

## When to Use

- Working with code in `clients/`
- Understanding how isId64, normalizeId, bytesToHex work
- Modifying js-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `clients/js/kascov-encode.mjs` | isId64, normalizeId, bytesToHex, taggedToValue, taggedToBare (+8) |
| `clients/js/kascov.mjs` | canonicalJson, rotr, sha256Hex, hexToBytes, verifyBadge (+2) |
| `crates/kascov/src/main.rs` | parse_coin_ids, parse_token_directory_query, empty_query_preserves_the_unbounded_legacy_directory, capped_drain, covenant_filter (+1) |
| `clients/js/kascov.test.mjs` | nodeSha, nodePair, buildTree, getReader, impl (+1) |
| `web/app.js` | renderDev, wireApiSidebar, wireApiTools, apply |
| `crates/kascov-sim/src/lib.rs` | parse_trace, parse_hex_array |
| `web/core/pending.js` | trim |
| `clients/py/test_kascov.py` | read |

## Entry Points

Start here when exploring this area:

- **`isId64`** (Function) — `clients/js/kascov-encode.mjs:55`
- **`normalizeId`** (Function) — `clients/js/kascov-encode.mjs:64`
- **`bytesToHex`** (Function) — `clients/js/kascov-encode.mjs:86`
- **`taggedToValue`** (Function) — `clients/js/kascov-encode.mjs:134`
- **`taggedToBare`** (Function) — `clients/js/kascov-encode.mjs:174`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `isId64` | Function | `clients/js/kascov-encode.mjs` | 55 |
| `normalizeId` | Function | `clients/js/kascov-encode.mjs` | 64 |
| `bytesToHex` | Function | `clients/js/kascov-encode.mjs` | 86 |
| `taggedToValue` | Function | `clients/js/kascov-encode.mjs` | 134 |
| `taggedToBare` | Function | `clients/js/kascov-encode.mjs` | 174 |
| `canonicalJson` | Function | `clients/js/kascov.mjs` | 220 |
| `sha256Hex` | Function | `clients/js/kascov.mjs` | 243 |
| `verifyBadge` | Function | `clients/js/kascov.mjs` | 285 |
| `hexToBytes` | Function | `clients/js/kascov-encode.mjs` | 73 |
| `exprInt` | Function | `clients/js/kascov-encode.mjs` | 96 |
| `exprBytes` | Function | `clients/js/kascov-encode.mjs` | 108 |
| `exprBool` | Function | `clients/js/kascov-encode.mjs` | 117 |
| `exprString` | Function | `clients/js/kascov-encode.mjs` | 122 |
| `valueToTagged` | Function | `clients/js/kascov-encode.mjs` | 192 |
| `bareToTagged` | Function | `clients/js/kascov-encode.mjs` | 248 |
| `read` | Function | `clients/py/test_kascov.py` | 34 |
| `streamUrl` | Method | `clients/js/kascov.mjs` | 170 |
| `stream` | Method | `clients/js/kascov.mjs` | 179 |
| `renderDev` | Function | `web/app.js` | 3753 |
| `wireApiSidebar` | Function | `web/app.js` | 4259 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Lane_mint_handler → Trim` | cross_community | 4 |
| `Publish_handler → Trim` | cross_community | 4 |
| `Main → Trim` | cross_community | 4 |
| `Prove_holding_handler → Trim` | cross_community | 4 |
| `Og_card_handler → Trim` | cross_community | 4 |
| `Zk_verify_handler → Trim` | cross_community | 4 |
| `Preflight_handler → Trim` | cross_community | 4 |
| `Share_handler → Trim` | cross_community | 4 |
| `Compile_handler → Trim` | cross_community | 4 |
| `Simulate_handler → Trim` | cross_community | 4 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Web | 3 calls |

## How to Explore

1. `gitnexus_context({name: "isId64"})` — see callers and callees
2. `gitnexus_query({query: "js"})` — find related execution flows
3. Read key files listed above for implementation details
