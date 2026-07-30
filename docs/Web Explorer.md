# Web Explorer

The public explorer is a no-build, progressively enhanced single-page app in `web/`. It is deliberately plain HTML, CSS, and JavaScript: production serves the checked-in files directly, so the code that passed review is the code users receive.

This note describes the web architecture as of **July 25, 2026**, including the live-refresh hardening and the builder guide's move into the application shell.

## File map

| file | responsibility |
|---|---|
| `web/index.html` | semantic shell, static page/view markup, API reference, builder guide |
| `web/style.css` | shared visual system and responsive behavior |
| `web/app.js` | routing, rendering, live-feed orchestration, interactions |
| `web/core/state.js` | shared client state and cache containers |
| `web/core/data.js` | fetch, cache, pagination, and request sharing; no DOM |
| `web/core/routing.js` | network-aware route rewriting |
| `web/core/loading.js` | policy for routes that actually need the large snapshot |
| `web/core/refresh.js` | keyed leading/trailing refresh gate |
| `web/core/pending.js` | reconciliation of pending snapshots and SSE events |
| `web/galaxy.js` | lazily loaded galaxy renderer |
| `web/*.test.mjs` | DOM-contract, routing, loading, refresh, and responsive regression tests |

The browser-side disassembler remains `web/disasm.js`; see [[Decoding]].

## Route model

Routes use the hash so the static shell can be hosted without a server-side router:

- `#/explore`, `#/{network}/explore`
- `#/{network}/c/{covenant-id}`
- `#/{network}/tx/{transaction-id}`
- `#/{network}/addr/{address}`
- `#/{network}/tokens` and `#/{network}/token/{id}`
- `#/{network}/decode`, `build`, `preflight`, and `dev`
- `#/guide`
- `#/changelog`

Network-scoped links are rewritten from the active network. Switching networks preserves the useful current destination instead of dropping the user on the home page. The guide and changelog are intentionally network-neutral and remain on their current page when the network switch changes.

Clean paths served by Caddy fall back to `index.html`, and the boot code converts them into their hash-route equivalent. This keeps pasted URLs such as `/explore` and `/testnet-10/c/<id>` useful without duplicating the route implementation.

## Data loading

### Small first paint, large data later

Landing and Explore are the only views that require the paginated network snapshot. The browser starts that request immediately but can paint from `<network>-live.json` first. Detail, address, lane, token, transaction, playground, API, guide, and changelog views use their own small endpoints or static markup and do not wait for the grid.

The grid request is retried once after a transient failure. A successful page receives a derived in-memory index for name lookup, filtering, sorting, and watchlist rendering. “Load more” appends cursor pages and advances the `(after_daa, after_id)` cursor without rebuilding everything from scratch.

Coin detail is loaded on demand from `/data/{network}/c/{id}.json` and merged over any summary row already held by the grid. This keeps direct links independent of the currently loaded grid window.

### Request sharing

Live rendering can ask for the same resource more than once before the first transfer completes. `web/core/data.js` therefore shares promises:

- one cold network snapshot per network;
- one in-flight request per small feed key, including live, pending, lanes, inscriptions, lifespans, analytics, and galaxy tiers;
- one confirmation/detail transfer when multiple live events ask for the same fact.

The promise is removed only if it is still the current map entry. A completed request cannot accidentally delete a newer one.

### Cache compatibility

Optional endpoints are feature-detected. A `404` from an older worker hides that feature and is cached for a bounded interval instead of causing a retry storm. Responses use `cache: "no-cache"` because the application owns freshness and the worker already provides endpoint-specific server caches.

Browser module URLs in `index.html` carry an explicit revision query. When request-layer modules change, bump that revision so an old `app.js` cannot import stale core modules from the browser cache.

## Live refresh model

Kaspa can produce new state continuously, so “refresh” is an orchestration problem rather than a timer.

### Keyed refresh gate

`createRefreshGate(intervalMs)` owns one slot per key:

1. the first caller starts the task;
2. callers during that task share its promise;
3. callers inside the quiet interval collapse into one pending promise;
4. if more work arrived while the task ran, exactly one trailing task is scheduled;
5. the latest supplied task wins for that trailing run.

Snapshot refreshes use a 45-second per-network gate. Live-feed pokes use a separate five-second gate. Bursty SSE, polling, and visibility events therefore cannot multiply the same multi-megabyte download.

### Refresh boundaries

Background refreshes:

- do nothing in a hidden tab;
- stop if the selected network changed;
- re-request at least the number of grid rows already loaded, so a paginated view does not shrink;
- skip rendering when `generated_at_ms` did not change;
- preserve the detail currently being read;
- do not rebuild input-owning views such as build, decode, preflight, address, lane, token, or transaction pages.

That last boundary prevents a live chain update from erasing typed form state or the success link returned by a one-click deploy.

### View transitions and focus

Only an actual route change may replay the entrance fade. A data/SSE rerender removes any stray entering class and leaves the visible page visible.

On a real route transition, focus moves to the new view's first heading so assistive technology announces the page. The temporary `.route-focus-target` class suppresses the global visual focus ring for that programmatic move; ordinary keyboard focus still receives the visible ring.

## Pending covenant activity

The pending lane combines:

- a short-TTL authoritative snapshot from `/data/{network}/pending`;
- `pending` SSE frames;
- `pending_resolved` frames when a transaction confirms or leaves the mempool.

Rows are keyed and generation-aware, so an older snapshot cannot resurrect a transaction already resolved by the stream. Repeated frames merge covenant events into one stable transaction row. Feed health and revision are explicit; an empty healthy mempool is distinguishable from a broken feed.

See [[Architecture#Deployment topology (live since July 22)]] for the worker side.

## Explore section navigation

The Explore shortcuts follow document order:

1. live
2. galaxy
3. analytics
4. discover
5. coins

At normal widths the shortcuts form a sticky horizontal rail. At `1360px` and wider they move into the unused left gutter as a fixed vertical rail, leaving the 1080px content column untouched. On phones the rail becomes a horizontally scrollable row with 44px touch targets.

The controls use the same compact rectangular geometry as the rest of the application. Hover, keyboard focus, and press each have a distinct border/background state. Section targets define scroll margins so headings clear the sticky site header and shortcut rail.

## Builder guide inside the shell

The 15-minute deploy → spend → replay guide used to be `web/guide.html`, a standalone page with a copied palette. It is now the static `view-guide` section inside `index.html`.

The move gives the guide:

- the normal header, global search, network tabs, and footer;
- a sticky section rail with scroll tracking;
- copy buttons that omit sample output/error lines;
- in-app links to the compiler, preflight tool, API reference, and real transactions;
- `#/guide?at=<section>` deep links;
- a crawlable `/guide` entry in the generated sitemap.

Existing `/guide.html#trap` and `/guide#trap` links are rewritten to `/#/guide?at=trap`. The prose stays in HTML rather than being generated by JavaScript, so crawlers and non-script readers still receive the actual guide.

The practical command walkthrough is also summarized in [[Covenant Lab#The web guide]].

## Regression contracts

The web tests are intentionally small and structural. Important contracts include:

- cold and feed requests share one transfer;
- refresh bursts produce one in-flight and one trailing run;
- only landing and Explore require the snapshot;
- pending snapshot/SSE reconciliation cannot resurrect resolved transactions;
- live rerenders cannot restart the view fade;
- programmatic route focus does not paint a page-sized ring;
- Explore shortcut order and rectangular interaction states stay fixed;
- guide routes, redirects, section links, copy controls, and network switching remain valid;
- phone navigation and touch targets stay reachable without page overflow.

Run all browser-independent checks with:

```bash
node --test web/*.test.mjs
```

## Related notes

- [[Architecture]]
- [[Operations]]
- [[Traffic Analytics]]
- [[Covenant Lab]]
- [[Decoding]]
