---
name: traffic
description: "Skill for the Traffic area of KasDev. 16 symbols across 1 files."
---

# Traffic

16 symbols | 1 files | Cohesion: 86%

## When to Use

- Working with code in `web/`
- Understanding how $, svgNode, formatNumber work
- Modifying traffic-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `web/ops/traffic/app.js` | $, svgNode, formatNumber, formatTime, linePath (+11) |

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `$` | Function | `web/ops/traffic/app.js` | 6 |
| `svgNode` | Function | `web/ops/traffic/app.js` | 9 |
| `formatNumber` | Function | `web/ops/traffic/app.js` | 16 |
| `formatTime` | Function | `web/ops/traffic/app.js` | 38 |
| `linePath` | Function | `web/ops/traffic/app.js` | 77 |
| `renderRequestChart` | Function | `web/ops/traffic/app.js` | 86 |
| `renderApiSource` | Function | `web/ops/traffic/app.js` | 140 |
| `renderStatuses` | Function | `web/ops/traffic/app.js` | 183 |
| `renderTable` | Function | `web/ops/traffic/app.js` | 213 |
| `setLiveState` | Function | `web/ops/traffic/app.js` | 239 |
| `render` | Function | `web/ops/traffic/app.js` | 254 |
| `loadSnapshot` | Function | `web/ops/traffic/app.js` | 273 |
| `formatBytes` | Function | `web/ops/traffic/app.js` | 21 |
| `formatLatency` | Function | `web/ops/traffic/app.js` | 32 |
| `metricCard` | Function | `web/ops/traffic/app.js` | 44 |
| `renderMetrics` | Function | `web/ops/traffic/app.js` | 60 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Scripts | 1 calls |

## How to Explore

1. `gitnexus_context({name: "$"})` — see callers and callees
2. `gitnexus_query({query: "traffic"})` — find related execution flows
3. Read key files listed above for implementation details
