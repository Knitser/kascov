# Architecture

Four crates, strict boundaries:

```
crates/
├── kascov-core/     the library everything agrees on
│   ├── node/        wRPC client wrapper + ChainSource trait
│   ├── model.rs     kascov's own stable types
│   ├── sync.rs      the [[Sync Engine]]
│   ├── store.rs     the [[Storage Schema]]
│   └── detect.rs    tx → covenant sightings
├── kascov-decode/   [[Decoding]] — disassembler + decoder registry
├── kascov/          the CLI ([[CLI Reference]])
└── kascov-lab/      [[Covenant Lab]] — makes real covenants to index
```

## Design rules

**Rule 1 — quarantine upstream types.** `kaspa-*` types never leave `kascov-core/src/node/wrpc.rs`. Everything downstream uses `model.rs` types (`CovenantId`, `BlockHash`, `Transaction`, …). When rusty-kaspa's API churns (it will — the crates are pre-1.0 and the fork is weeks old), exactly one file absorbs the breakage. Exception: [[Covenant Lab]] deliberately uses kaspa crates directly — it *builds* transactions, which is exactly the upstream surface.

**Rule 2 — one pin to rule them all.** Kaspa crates on crates.io are frozen pre-Toccata (0.15.0, Sept 2024). All kaspa deps are git dependencies pinned to a single rev (`98a4ccd`, master post-Toccata) declared once in the workspace `Cargo.toml`. Borsh wRPC encoding is version-sensitive: bump the pin together with the node you connect to.

**Rule 3 — the engine is testable without a node.** `sync_once` is generic over the `ChainSource` trait; integration tests drive it with an in-memory `FakeChain` replaying scripted chain steps (genesis → transitions → burn → reorg → re-acceptance). See `crates/kascov-core/tests/sync_replay.rs`.

**Rule 4 — decoding never blocks shipping.** Lineage tracing is format-agnostic; the [[Decoding]] fallback (full disassembly) is always correct. Template-specific decoders are additive.

## Networks

Defaults to **mainnet** (public resolver, zero setup). `--network testnet-10` for the covenant test traffic. Testnet 12 is legacy — see [[Toccata Protocol Notes#Networks]].
