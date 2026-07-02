# kascov — Kaspa Covenant Explorer

> First-mover tooling for covenants on Kaspa L1, born days after the [[Toccata Protocol Notes|Toccata hardfork]] activated (June 30, 2026).

**What it is:** a Rust CLI + indexer that finds covenant UTXOs, traces their lineage by covenant ID, and decodes their state scripts. Kaspa nodes *validate* covenants but expose no way to *query* them — no "get UTXOs by covenant id" RPC exists, and no block explorer decodes covenant data. kascov fills that gap.

## The vault

- [[Architecture]] — workspace layout, crate boundaries, design rules
- [[Toccata Protocol Notes]] — KIP-20 covenant mechanics, networks, activation facts
- [[Sync Engine]] — acceptance-driven indexing, reorg handling, cursor recovery
- [[Storage Schema]] — the SQLite index
- [[CLI Reference]] — every command with examples
- [[Decoding]] — the script disassembler and decoder registry
- [[Covenant Lab]] — creating real covenants on testnet-10
- [[Roadmap]] — what's next

## Why indexing matters (the product moat)

Kaspa nodes prune block data after ~3 days. A covenant's history older than the pruning point is **unrecoverable** from a regular node — unless someone indexed it while it happened. Mainnet covenant traffic was near zero days after activation; whoever runs `kascov sync --follow` from day one owns the complete lineage record. See [[Sync Engine#Pruning and truncated lineage]].

## Status (July 2, 2026)

| Milestone | Status |
|---|---|
| M1 scan (live network dump) | ✅ verified on mainnet |
| M2 index + list/show | ✅ verified on testnet-10 |
| M3 trace + reorg correctness | ✅ replay-tested |
| M4 watch (live feed) | ✅ |
| M5 decode (disassembler) | ✅ verified on live TN10 covenant |
| M6 own covenant end-to-end | 🔄 [[Covenant Lab]] |
