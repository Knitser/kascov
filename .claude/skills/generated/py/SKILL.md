---
name: py
description: "Skill for the Py area of KasDev. 48 symbols across 2 files."
---

# Py

48 symbols | 2 files | Cohesion: 82%

## When to Use

- Working with code in `clients/`
- Understanding how tx, address, digest work
- Modifying py-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `clients/py/kascov.py` | _get, tx, address, digest, galaxy (+28) |
| `clients/py/test_kascov.py` | test_token_market_and_vesting_endpoints_hit_registered_routes, _sha, _pair, _tree, test_canonical_json_pins_the_exact_wire_bytes (+10) |

## Entry Points

Start here when exploring this area:

- **`tx`** (Function) — `clients/py/kascov.py:92`
- **`address`** (Function) — `clients/py/kascov.py:96`
- **`digest`** (Function) — `clients/py/kascov.py:100`
- **`galaxy`** (Function) — `clients/py/kascov.py:104`
- **`reorgs`** (Function) — `clients/py/kascov.py:108`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `tx` | Function | `clients/py/kascov.py` | 92 |
| `address` | Function | `clients/py/kascov.py` | 96 |
| `digest` | Function | `clients/py/kascov.py` | 100 |
| `galaxy` | Function | `clients/py/kascov.py` | 104 |
| `reorgs` | Function | `clients/py/kascov.py` | 108 |
| `templates` | Function | `clients/py/kascov.py` | 112 |
| `market` | Function | `clients/py/kascov.py` | 165 |
| `token_market` | Function | `clients/py/kascov.py` | 168 |
| `pool` | Function | `clients/py/kascov.py` | 176 |
| `vesting_detail` | Function | `clients/py/kascov.py` | 184 |
| `vesting_claims` | Function | `clients/py/kascov.py` | 187 |
| `index` | Function | `clients/py/kascov.py` | 190 |
| `openapi` | Function | `clients/py/kascov.py` | 193 |
| `activity` | Function | `clients/py/kascov.py` | 196 |
| `test_token_market_and_vesting_endpoints_hit_registered_routes` | Function | `clients/py/test_kascov.py` | 137 |
| `tokens` | Function | `clients/py/kascov.py` | 116 |
| `token` | Function | `clients/py/kascov.py` | 126 |
| `token_holders` | Function | `clients/py/kascov.py` | 135 |
| `token_events` | Function | `clients/py/kascov.py` | 140 |
| `token_trades` | Function | `clients/py/kascov.py` | 145 |

## How to Explore

1. `gitnexus_context({name: "tx"})` — see callers and callees
2. `gitnexus_query({query: "py"})` — find related execution flows
3. Read key files listed above for implementation details
