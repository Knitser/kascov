# Roadmap

Rough priority order, July 2026.

## Near

- **Spend-time decoding** — capture covenant-spending signature scripts during [[Sync Engine|sync]]; disassemble the revealed P2SH preimage; show `payload Δ` between transitions in `trace`. See [[Decoding#Spend-time decoding (next)]].
- **KIP-20 genesis hash validation** in classification — recompute the covenant id from the genesis outpoint + authorized outputs (the [[Covenant Lab]] already does this construction) to distinguish true genesis from truncated-lineage first sightings, instead of the current observational rule.
- **Notification-based follow** — subscribe to `virtual-chain-changed` instead of the 2s poll in `sync --follow`.

## Mid

- **SilverScript template decoders** — recognize compiled contracts from `silverscript-lang/tests/examples/` (Mecenas, escrow, last-will) and name their state fields in the [[Decoding]] registry.
- **kascov-api** — axum crate: read-only JSON + SSE endpoints mirroring the CLI over the same [[Storage Schema|store]] (`/covenants`, `/covenants/:id/lineage`, `/events` stream).
- **Web UI** — static single-page covenant browser on top of kascov-api.

## Publishing

- Repo has no remote yet ([[Architecture]] assumes `github.com/exoticxp/kascov`).
- The pitch writes itself: *mainnet covenant traffic is still near zero — the complete history starts with whoever indexes from day one.* Ship the "first covenant explorer for Kaspa L1" post while that's still true.

## Operational notes

- Bump the rusty-kaspa git pin **together with** the node version (borsh compatibility — [[Architecture#Design rules]] Rule 2).
- Testnet resets: `kascov reset --yes`, re-sync from scratch, done.
- An archival node would extend lineage beyond the ~3-day pruning window for covenants indexed late.
