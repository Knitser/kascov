# Architecture

Four crates + a no-build web app, strict boundaries:

```
crates/
├── kascov-core/     the library everything agrees on
│   ├── node/        wRPC client wrapper + ChainSource trait + consensus-hash boundary
│   ├── model.rs     kascov's own stable types
│   ├── sync.rs      the [[Sync Engine]]
│   ├── store.rs     the [[Storage Schema]]
│   └── detect.rs    tx → covenant sightings
├── kascov-decode/   [[Decoding]] — disassembler, P2SH reveals, template registry
├── kascov/          the CLI + the `serve` worker ([[CLI Reference]])
└── kascov-lab/      [[Covenant Lab]] — makes real covenants to index
web/                 vanilla-JS explorer (no build step) + disasm.js (verified
                     JS port of kascov-decode's disassembler)
```

## Design rules

**Rule 1 — quarantine upstream types.** `kaspa-*` types never leave `kascov-core/src/node/`. Everything downstream uses `model.rs` types (`CovenantId`, `BlockHash`, `Transaction`, …). When rusty-kaspa's API churns, exactly one module absorbs the breakage. The same boundary exposes `node::compute_covenant_id` — a thin wrapper over the consensus KIP-20 hash so classification can never drift from the chain. Exception: [[Covenant Lab]] deliberately uses kaspa crates directly — it *builds* transactions, which is exactly the upstream surface.

**Rule 2 — one pin to rule them all.** Kaspa crates on crates.io are frozen pre-Toccata (0.15.0, Sept 2024). All kaspa deps are git dependencies pinned to a single rev (`98a4ccd`, master post-Toccata) declared once in the workspace `Cargo.toml`. Borsh wRPC encoding is version-sensitive: bump the pin together with the node you connect to.

**Rule 3 — the engine is testable without a node.** `sync_once` is generic over the `ChainSource` trait; integration tests drive it with an in-memory `FakeChain` replaying scripted chain steps (genesis → transitions → burn → reorg → re-acceptance), now constructing real KIP-20 ids so genesis validation is exercised too. See `crates/kascov-core/tests/sync_replay.rs`.

**Rule 4 — decoding never blocks shipping.** Lineage tracing is format-agnostic; the [[Decoding]] fallback (full disassembly) is always correct. Template-specific decoders are additive.

## Deployment topology (live since July 2)

```
Firebase Hosting  ──  static web/ (SPA, no build step; HTML no-cache, js/css 5 min)
      │
      └── /data/** rewrite ──► Cloud Run: kascov-worker (`kascov serve`)
                                 ├─ follows mainnet + TN10 (concurrent prefetch)
                                 ├─ /data/<net>.json        full snapshot (15/30s cache, br/gzip)
                                 ├─ /data/<net>-live.json   stats+tip+150 events (5/10s cache)
                                 └─ SQLite DBs ⟷ gs://kascov-explorer-index (5-min backups,
                                    restore on boot — history survives redeploys)
```

Redeploy: `gcloud run deploy kascov-worker --source .` (or `scripts/deploy-worker.sh`). Hosting: `firebase deploy --only hosting` after `web/` changes. The old laptop loop (`scripts/kascov-live.sh`) is obsolete for production. Health probe: `/data/mainnet-live.json` (`/healthz` is swallowed by Google's frontend on the deterministic URL).

## Networks

Defaults to **mainnet** (public resolver, zero setup). `--network testnet-10` for the covenant test traffic. Testnet 12 is legacy — see [[Toccata Protocol Notes#Networks]].
