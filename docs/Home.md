# kascov — Kaspa Covenant Explorer

> First-mover tooling for covenants on Kaspa L1, born days after the [[Toccata Protocol Notes|Toccata hardfork]] activated (June 30, 2026). Live at **[kascov-explorer.web.app](https://kascov-explorer.web.app)**.

**What it is:** a Rust CLI + indexer + always-on web explorer that finds covenant UTXOs, traces their lineage by covenant ID, and decodes their state scripts — including the programs revealed at spend time. Kaspa nodes *validate* covenants but expose no way to *query* them — no "get UTXOs by covenant id" RPC exists, and no other explorer decodes covenant lineage. kascov fills that gap.

## The vault

- [[Architecture]] — workspace layout, crate boundaries, design rules, deployment topology
- [[Toccata Protocol Notes]] — KIP-20 covenant mechanics, networks, activation facts
- [[Sync Engine]] — acceptance-driven indexing, reorg handling, KIP-20 genesis validation, tip anchoring
- [[Storage Schema]] — the SQLite index
- [[CLI Reference]] — every command with examples
- [[Decoding]] — disassembler, spend-time reveals, template decoders
- [[Covenant Lab]] — creating real covenants on testnet-10
- [[Roadmap]] — what's next

## Why indexing matters (the product moat)

Kaspa nodes prune block data after ~3 days. A covenant's history older than the pruning point is **unrecoverable** from a regular node — unless someone indexed it while it happened. **This stopped being theoretical on July 2, 2026**: mainnet's first covenants appeared (`c7948684ae…`, 195 events in its first hour) and kascov indexed them live; TN10 produced a covenant storm the same day (1,100 → 5,800+ covenants within hours). Whoever runs the index from day one owns the complete record. See [[Sync Engine#Pruning and truncated lineage]].

## Status (July 2, 2026 — evening)

| Milestone | Status |
|---|---|
| M1 scan (live network dump) | ✅ verified on mainnet |
| M2 index + list/show | ✅ verified on testnet-10 |
| M3 trace + reorg correctness | ✅ replay-tested |
| M4 watch (live feed) | ✅ |
| M5 decode (disassembler) | ✅ + in-browser port (`web/disasm.js`, byte-identical on all indexed scripts) |
| M6 own covenant end-to-end | ✅ [[Covenant Lab]] on TN10 |
| M7 export + web dashboard | ✅ [kascov-explorer.web.app](https://kascov-explorer.web.app) |
| M8 always-on serving | ✅ Cloud Run worker, GCS-backed continuity, live JSON API |
| M9 spend-time decoding | ✅ sig capture, verified P2SH reveals, payload Δ in `trace` |
| M10 KIP-20 genesis validation | ✅ consensus-hash-verified classification |
| M11 template decoders | ✅ p2pk/p2sh labeled live; SilverScript bodies pending regeneration ([[Decoding#SilverScript templates]]) |
