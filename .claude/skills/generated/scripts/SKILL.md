---
name: scripts
description: "Skill for the Scripts area of KasDev. 188 symbols across 19 files."
---

# Scripts

188 symbols | 19 files | Cohesion: 71%

## When to Use

- Working with code in `scripts/`
- Understanding how isoWeekOf, mondayOf, alreadyPosted work
- Modifying scripts-related functionality

## Key Files

| File | Symbols |
|------|---------|
| `scripts/discord-holder-bot.mjs` | voteEligibility, roundIsOpen, castBallot, tallyCounts, buildTally (+64) |
| `scripts/traffic_report.py` | header, visitor_key, is_browser, is_bot, is_asset (+18) |
| `scripts/discord-bench-wire.mjs` | fmt, isoWeekOf, mondayOf, alreadyPosted, extractSummary (+8) |
| `scripts/dev-serve.mjs` | json, hex64, pick, between, broadcast (+7) |
| `scripts/discord-changelog-bot.mjs` | stampOf, pendingEntries, deliveryPlan, buildEntryEmbed, sleep (+6) |
| `scripts/traffic_snapshot.py` | parse_iso, refresh_long_windows, build_snapshot, load_existing, write_atomic (+4) |
| `scripts/test_traffic_report.py` | test_endpoint_normalization_bounds_public_identifiers, row, test_counts_visitors_sessions_and_api_sources, test_series_buckets_requests_without_exporting_visitor_keys, test_spa_fallback_does_not_turn_scanner_paths_into_page_views (+3) |
| `scripts/discord-price-bot.mjs` | fmtKas, fmtInt, priceKas, fmtChange, buildEmbed (+3) |
| `web/core/data.js` | loadAddress, loadLanePage, loadAllTrades, loadVerification, loadOlderTokenEvents (+2) |
| `scripts/discord-update-server.mjs` | call, bare, main, cat, chan (+1) |

## Entry Points

Start here when exploring this area:

- **`isoWeekOf`** (Function) — `scripts/discord-bench-wire.mjs:49`
- **`mondayOf`** (Function) — `scripts/discord-bench-wire.mjs:61`
- **`alreadyPosted`** (Function) — `scripts/discord-bench-wire.mjs:70`
- **`extractSummary`** (Function) — `scripts/discord-bench-wire.mjs:82`
- **`pickSpecimen`** (Function) — `scripts/discord-bench-wire.mjs:118`

## Key Symbols

| Symbol | Type | File | Line |
|--------|------|------|------|
| `TrafficReport` | Class | `scripts/traffic_report.py` | 238 |
| `isoWeekOf` | Function | `scripts/discord-bench-wire.mjs` | 49 |
| `mondayOf` | Function | `scripts/discord-bench-wire.mjs` | 61 |
| `alreadyPosted` | Function | `scripts/discord-bench-wire.mjs` | 70 |
| `extractSummary` | Function | `scripts/discord-bench-wire.mjs` | 82 |
| `pickSpecimen` | Function | `scripts/discord-bench-wire.mjs` | 118 |
| `formatWire` | Function | `scripts/discord-bench-wire.mjs` | 135 |
| `postWire` | Function | `scripts/discord-bench-wire.mjs` | 208 |
| `voteEligibility` | Function | `scripts/discord-holder-bot.mjs` | 174 |
| `roundIsOpen` | Function | `scripts/discord-holder-bot.mjs` | 189 |
| `castBallot` | Function | `scripts/discord-holder-bot.mjs` | 197 |
| `tallyCounts` | Function | `scripts/discord-holder-bot.mjs` | 211 |
| `buildTally` | Function | `scripts/discord-holder-bot.mjs` | 222 |
| `isOperator` | Function | `scripts/discord-holder-bot.mjs` | 238 |
| `parseSlate` | Function | `scripts/discord-holder-bot.mjs` | 245 |
| `mintParticipation` | Function | `scripts/discord-holder-bot.mjs` | 264 |
| `leftTheServer` | Function | `scripts/discord-holder-bot.mjs` | 150 |
| `balanceBucket` | Function | `scripts/discord-holder-bot.mjs` | 298 |
| `alertsEnabled` | Function | `scripts/discord-holder-bot.mjs` | 307 |
| `cursorDiff` | Function | `scripts/discord-holder-bot.mjs` | 314 |

## Execution Flows

| Flow | Type | Steps |
|------|------|-------|
| `OnVoteOpen → TallyCounts` | intra_community | 5 |
| `OnVoteOpen → RoundIsOpen` | intra_community | 5 |
| `OnVote → TallyCounts` | intra_community | 5 |
| `OnVote → RoundIsOpen` | intra_community | 5 |
| `Registry_handler → Header` | cross_community | 4 |
| `Registry_handler → Send` | cross_community | 4 |
| `Registry_handler → Status` | cross_community | 4 |
| `OnVoteOpen → MintParticipation` | intra_community | 4 |
| `OnVoteOpen → CurrentRound` | intra_community | 4 |
| `OnVoteOpen → Json` | cross_community | 4 |

## Connected Areas

| Area | Connections |
|------|-------------|
| Js | 6 calls |
| Web | 4 calls |
| Tests | 1 calls |
| Cluster_207 | 1 calls |
| Cluster_221 | 1 calls |

## How to Explore

1. `gitnexus_context({name: "isoWeekOf"})` — see callers and callees
2. `gitnexus_query({query: "scripts"})` — find related execution flows
3. Read key files listed above for implementation details
