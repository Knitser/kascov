---
name: web
description: "Skill for the Web area of KasDev. 482 symbols across 22 files."
---

# Web

482 symbols | 22 files | Cohesion: 74%

## When to Use

- Working with code in `web/`
- Understanding how list_name, parse_list work
- Modifying web-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `web/app.js` | routeLoading, nsLabel, hydrateGalaxyTokenInfo, eventShape, eventSentence (+256) |
| `web/galaxy.js` | create, focus, colorForTemplate, hashStr, clamp (+83) |
| `web/core/data.js` | isAlive, balancesByEventDaa, indexEntry, appendToIndex, loadMoreGrid (+14) |
| `web/core/format.js` | idByte, friendlyName, semanticTemplate, avatarSvg, b (+13) |
| `web/disasm.js` | toHex, snumEncode, snumDecode, toAsm, parseHex (+10) |
| `web/dag.js` | yOf, fade, prefill, draw, layout (+10) |
| `web/graph.js` | colorFor, render, tick, draw, onPointerLeave (+4) |
| `web/gen.js` | D, validateField, prefillFor, sompiToTkas, buildDeployScript (+3) |
| `web/core/state.js` | fmtAmount, daaToMs, watchKey, loadWatch, saveWatch (+2) |
| `web/core/pending.js` | eventsFor, mergeEvents, add, createPendingModel, setConnection (+2) |

## Entry Points

Start here when exploring this area:

- **`list_name`** (Function) — `crates/kascov/src/registry.rs:170`
- **`parse_list`** (Function) — `crates/kascov/src/registry.rs:180`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `list_name` | Function | `crates/kascov/src/registry.rs` | 170 |
| `parse_list` | Function | `crates/kascov/src/registry.rs` | 180 |
| `routeLoading` | Function | `web/app.js` | 52 |
| `nsLabel` | Function | `web/app.js` | 62 |
| `hydrateGalaxyTokenInfo` | Function | `web/app.js` | 523 |
| `eventShape` | Function | `web/app.js` | 979 |
| `eventSentence` | Function | `web/app.js` | 990 |
| `cardStory` | Function | `web/app.js` | 1032 |
| `knownKeyLabel` | Function | `web/app.js` | 1480 |
| `generatorLabel` | Function | `web/app.js` | 1488 |
| `matchesFilter` | Function | `web/app.js` | 1511 |
| `coinCardHtml` | Function | `web/app.js` | 1562 |
| `remoteSearchRows` | Function | `web/app.js` | 1700 |
| `scheduleRemoteSearch` | Function | `web/app.js` | 1704 |
| `remoteGridCardsHtml` | Function | `web/app.js` | 1743 |
| `suggestionItems` | Function | `web/app.js` | 1773 |
| `push` | Function | `web/app.js` | 1778 |
| `markMatch` | Function | `web/app.js` | 1796 |
| `renderSuggest` | Function | `web/app.js` | 1817 |
| `miniCard` | Function | `web/app.js` | 1902 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `Vesting_handler → From_str` | cross_community | 4 |
| `Vesting_handler → ListedToken` | cross_community | 4 |
| `Vesting_handler → Clean_text` | cross_community | 4 |
| `Vesting_handler → ListedVesting` | cross_community | 4 |
| `OnPointerMove → Has` | cross_community | 4 |
| `OnPointerDown → Has` | cross_community | 4 |
| `RenderExplore → RelTime` | cross_community | 4 |
| `OnKeyDown → Has` | cross_community | 4 |
| `Registry_handler → From_str` | cross_community | 3 |
| `Registry_handler → ListedToken` | cross_community | 3 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Scripts | 22 calls |
| Cluster_98 | 10 calls |
| Js | 6 calls |
| Cluster_88 | 2 calls |
| Examples | 2 calls |
| Cluster_97 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "list_name"})` — see callers and callees
2. `gitnexus_query({query: "web"})` — find related execution flows
3. Read key files listed above for implementation details
