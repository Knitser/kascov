# kascov — Kaspa Covenant Explorer

> First-mover tooling for covenants on Kaspa L1, born days after the [[Toccata Protocol Notes|Toccata hardfork]] activated (June 30, 2026). Live at **[kascov.io](https://kascov.io)**.

**What it is:** a Rust CLI + indexer + always-on web explorer that finds covenant UTXOs, traces their lineage by covenant ID, and decodes their state scripts — including the programs revealed at spend time. Kaspa nodes *validate* covenants but expose no way to *query* them — no "get UTXOs by covenant id" RPC exists, and no other explorer decodes covenant lineage. kascov fills that gap.

## The vault

- [[Architecture]] — workspace layout, crate boundaries, design rules, deployment topology
- [[System Map]] — end-to-end flows, ownership, truth hierarchy, failure containment
- [[Toccata Protocol Notes]] — KIP-20 covenant mechanics, networks, activation facts
- [[Sync Engine]] — acceptance-driven indexing, reorg handling, KIP-20 genesis validation, tip anchoring
- [[Storage Schema]] — the SQLite index
- [[CLI Reference]] — every command with examples
- [[Decoding]] — disassembler, spend-time reveals, template decoders
- [[Covenant Lab]] — creating real covenants on testnet-10
- [[Web Explorer]] — browser architecture, routing, request sharing, live refresh, guide
- [[Operations]] — release, digest automation, changelog/feed, verification
- [[Testing Strategy]] — test layers, invariants, commands, regression policy
- [[Traffic Analytics]] — private Caddy-log measurement and report semantics
- [[Roadmap]] and [[Feature-Roadmap]] — historical plans and remaining work

## Why indexing matters (the product moat)

Kaspa nodes retain at least ~30 hours of prunable consensus data at 10 bps (per IzioDev, Jul 17 2026 — the old "~3 days" figure was pre-Crescendo). A covenant's history older than the pruning point is **unrecoverable** from a regular node — unless someone indexed it while it happened. **This stopped being theoretical on July 2, 2026**: mainnet's first covenants appeared (`c7948684ae…`, 195 events in its first hour) and kascov indexed them live; TN10 produced a covenant storm the same day (1,100 → 5,800+ covenants within hours). Whoever runs the index from day one owns the complete record. See [[Sync Engine#Pruning and truncated lineage]].

## Status (July 25, 2026)

| Milestone | Status |
|---|---|
| M1 scan (live network dump) | ✅ verified on mainnet |
| M2 index + list/show | ✅ verified on testnet-10 |
| M3 trace + reorg correctness | ✅ replay-tested |
| M4 watch (live feed) | ✅ |
| M5 decode (disassembler) | ✅ + in-browser port (`web/disasm.js`, byte-identical on all indexed scripts) |
| M6 own covenant end-to-end | ✅ [[Covenant Lab]] on TN10 |
| M7 export + web dashboard | ✅ [kascov.io](https://kascov.io) |
| M8 always-on serving | ✅ Cloud Run worker, GCS-backed continuity, live JSON API |
| M9 spend-time decoding | ✅ sig capture, verified P2SH reveals, payload Δ in `trace` |
| M10 KIP-20 genesis validation | ✅ consensus-hash-verified classification |
| M11 template decoders | ✅ p2pk/p2sh, SilverScript, Genesis0, PURE, KCC-1 identities, and KCC20 recognized from chain data ([[Decoding]]) |

The product has moved well beyond the original milestones: address and transaction pages, SSE, authoritative pending activity, contract analytics, the galaxy, KCC20 accounting, verified token art, share cards, webhooks, preflight, simulation/debugging, a no-code builder, one-click TN10 deploy, a public API reference, and the integrated 15-minute builder guide are all live. Production now runs beside kascov's own archival mainnet and TN10 nodes. See [[Web Explorer]], [[Architecture]], and the in-app changelog for the current surface.

### Latest hardening wave

The July 25 work made the continuously updating site calm under load:

- duplicate snapshot, feed, and confirmation requests now share one transfer;
- refresh bursts collapse into one active and one trailing run;
- background updates preserve forms, deploy results, open details, scroll, and view visibility;
- route focus remains accessible without painting a large heading ring;
- Explore shortcuts follow page order and use the shared rectangular interaction language;
- the builder guide now lives inside the application shell while old links redirect safely;
- traffic reports group parameterized endpoints and exclude scanner probes;
- the intentionally secret-less daily digest now acts as a green canary for the public mainnet digest endpoint.

The exact audited change ledger is in [[Operations#July 25, 2026 documentation catch-up]].
