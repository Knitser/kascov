# Architecture

Six crates + a no-build web app, strict boundaries:

```
crates/
├── kascov-core/     the library everything agrees on
│   ├── node/        wRPC client wrapper + ChainSource trait + consensus-hash boundary
│   ├── model.rs     kascov's own stable types
│   ├── sync.rs      the [[Sync Engine]]
│   ├── store.rs     the [[Storage Schema]]
│   └── detect.rs    tx → covenant sightings
├── kascov-decode/   [[Decoding]] — disassembler, P2SH reveals, template registry
├── kascov-sim/      off-chain script-engine harness — powers /simulate, /debug
│                    (real-witness replay) and /zk-verify
├── kascov/          the CLI + the `serve` worker ([[CLI Reference]])
│   └── src/og.rs    share cards: SVG → resvg → 1200×630 PNG, fonts embedded
├── kascov-labkit/   transaction-building library shared by the lab CLI and the
│                    worker's custodial one-click /deploy
└── kascov-lab/      [[Covenant Lab]] — CLI over labkit; makes real covenants to index
web/                 vanilla-JS explorer (no build step) + disasm.js (verified
                     JS port of kascov-decode's disassembler)
```

The web directory is itself split by responsibility: `app.js` owns views and DOM orchestration, while `web/core/` owns state, loading policy, request sharing, refresh gating, routing, formatting, pending reconciliation, and price data. See [[Web Explorer]] for the browser-side architecture.

## Design rules

**Rule 1 — quarantine upstream types.** `kaspa-*` types never leave `kascov-core/src/node/`. Everything downstream uses `model.rs` types (`CovenantId`, `BlockHash`, `Transaction`, …). When rusty-kaspa's API churns, exactly one module absorbs the breakage. The same boundary exposes `node::compute_covenant_id` — a thin wrapper over the consensus KIP-20 hash so classification can never drift from the chain. Exception: [[Covenant Lab]] deliberately uses kaspa crates directly — it *builds* transactions, which is exactly the upstream surface.

**Rule 2 — one pin to rule them all.** Kaspa crates on crates.io are frozen pre-Toccata (0.15.0, Sept 2024). All kaspa deps are git dependencies pinned to a single rev (`98a4ccd`, master post-Toccata) declared once in the workspace `Cargo.toml`. Borsh wRPC encoding is version-sensitive: bump the pin together with the node you connect to.

**Rule 3 — the engine is testable without a node.** `sync_once` is generic over the `ChainSource` trait; integration tests drive it with an in-memory `FakeChain` replaying scripted chain steps (genesis → transitions → burn → reorg → re-acceptance), now constructing real KIP-20 ids so genesis validation is exercised too. See `crates/kascov-core/tests/sync_replay.rs`.

**Rule 4 — decoding never blocks shipping.** Lineage tracing is format-agnostic; the [[Decoding]] fallback (full disassembly) is always correct. Template-specific decoders are additive.

**Rule 5 — live data must not own the reader.** Chain activity may invalidate caches, but it may not erase typed input, collapse an open story, replay a page transition, move focus, or multiply identical network requests. Browser refresh work is keyed and coalesced; only real navigation performs navigation effects. See [[Web Explorer#Live refresh model]].

**Rule 6 — one application shell.** Product documentation that behaves like a page belongs inside the shared shell, with the normal header, search, routing, accessibility, and responsive system. The builder guide is static semantic HTML inside `index.html`, not a second hand-maintained site. Compatibility redirects preserve already-shared standalone URLs.

## Deployment topology (live since July 22)

```
Dedicated Windows Server
├─ Caddy  ──  static C:\kascov\web (SPA, no build step)
│    ├─ /data/**, /share/**, /og/**, /badge/**, /img/**,
│    │  /sitemap.xml, /feed.xml, /health*
│    │                    ──► 127.0.0.1:8080
│    └─ /data/*/stream    ──► 127.0.0.1:8080 (flush immediately; no buffering)
├─ WSL2 Ubuntu: kascov-worker systemd service (`kascov serve`)
│                                ├─ follows mainnet + TN10 (concurrent prefetch)
                                 ├─ /data/<net>.json        grid snapshot — 20k-row first page,
                                 │                          next_after_daa/next_after_id cursors
                                 ├─ /data/<net>-live.json   stats+tip+150 events (5/10s cache)
                                 ├─ /data/<net>/pending     authoritative mempool snapshot +
                                 │                          explicit feed health/revision
                                 ├─ /data/<net>/…           coin/tx/addr detail, analytics
                                 │                          (galaxy, lanes, reorgs, lifespans,
                                 │                          inscriptions), search, debug, SSE,
                                 │                          simulate/compile/publish/verified,
                                 │                          subscribe/unsubscribe, deploy (gated)
                                 ├─ /share/<net>/<id> · /og/<net>/<id>.png · /sitemap.xml
                                 │                          crawler-visible coin pages + PNG
                                 │                          OG cards (src/og.rs)
                                 ├─ per-network webhook-delivery task — mpsc queue off the
                                 │    follower callback → sequential SSRF-guarded POSTs
                                 │    (3 attempts, 5s timeout, no redirects, auto-retire)
                                 ├─ galaxy keep-warm task — rebuilds the frontend's two
                                 │    payload variants every ~240s so no request pays a
                                 │    cold multi-second build
                                 └─ local SQLite DBs → verified local + GCS offsite backups
└─ archival kaspad Windows services
     ├─ mainnet wRPC ──► WSL worker over the private host bridge
     └─ testnet-10 wRPC ──► WSL worker over the private host bridge
```

Redeploy: fast-forward `/home/kascov/kascov` to the tested `main` revision, build `kascov --release` as the service user, restart `kascov-worker`, and mirror that same revision's `web/` into `C:\kascov\web`. Caddy does not need a reload for ordinary web or worker releases. Verify `/health`, both pending snapshots, the live feed, and the SPA after every release. The old laptop, Firebase, and Cloud Run deploy paths are obsolete.

The daily digest and first-party traffic report are operational companions rather than worker subsystems. Their failure and privacy semantics are documented in [[Operations]] and [[Traffic Analytics]].

## Networks

Defaults to **mainnet** (public resolver, zero setup). `--network testnet-10` for the covenant test traffic. Testnet 12 is legacy — see [[Toccata Protocol Notes#Networks]].
