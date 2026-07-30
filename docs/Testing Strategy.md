# Testing Strategy

kascov tests invariants at the narrowest deterministic layer that can prove them. Consensus-facing correctness uses Rust integration tests with fake chains and temporary SQLite stores. Browser coordination uses dependency-free Node tests. Operational parsing uses Python fixtures. Live-network demos are evidence, not substitutes for repeatable tests.

## Test pyramid

| layer | command | proves |
|---|---|---|
| Rust workspace | `cargo test --workspace` | model, decoder, store, sync, worker/API, labkit, simulator |
| Sync replay | `cargo test -p kascov-core --test sync_replay` | genesis/transition/burn, signatures, rollback, convergence |
| Gap recovery | `cargo test -p kascov-core --test gap_recovery` | merge, resequence, idempotence, residual accounting |
| KCC20 worked example | `cargo test -p kascov-core --test kcc20_worked_example` | deterministic token derivation against a fixture |
| Web contracts | `node --test web/*.test.mjs` | loading, routing, refresh, pending, galaxy, responsive behavior |
| Traffic report | `python3 -m unittest scripts/test_traffic_report.py` | privacy-conscious request classification |

## Core correctness

`ChainSource` makes the sync engine testable without a real node. `sync_replay.rs` supplies scripted virtual-chain steps and real KIP-20 ids to the same `sync_once` used in production.

The critical sequence is:

1. accepted genesis creates the covenant and state cell;
2. transition spends and replaces state;
3. burn consumes the final state;
4. a reorg removes the burn;
5. rollback restores the spent cell and clears its captured signature;
6. re-acceptance converges to the same final store;
7. an unprovable first sight becomes incomplete lineage, not fake genesis.

Tests should assert both returned values and durable rows. An event count alone cannot prove that rollback repaired UTXOs, summaries, and captured reveals.

## Recovery testing

Recovery paths deserve explicit tests because they intentionally operate outside the steady-state cursor:

- stranded-cursor detection distinguishes “caught up” from “successfully returned nothing while far behind”;
- re-anchoring uses only walkable blocks from indexed history;
- gap recovery leaves the live cursor untouched;
- inclusive DAA bounds cannot drop ties at the boundary;
- repeated runs deduplicate and become no-ops;
- post-gap reconciliation repairs spends whose originating cells were previously unknown;
- finalization resequences events and re-derives affected token accounting.

## Decoder testing

Decoder tests should preserve the “facts before labels” rule:

- all byte sequences disassemble, including malformed/truncated pushes;
- P2SH reveal acceptance requires an exact BLAKE2b commitment match;
- skeleton matches require fixed-body equality and repeated-slot agreement;
- KCC-1 encoders/decoders round-trip canonical integer and state forms;
- KCC20 state proof functions reject a hash mismatch;
- observed-program recognition is fixture-backed, not name-based.

Whenever the Rust decoder changes, verify parity with `web/disasm.js`; the browser must not name or group a script differently from the server.

## Browser contract tests

The web suite intentionally avoids a heavyweight browser dependency for logic that can be isolated:

- `data-inflight.test.mjs` — concurrent cold/feed loads share requests;
- `refresh.test.mjs` — bursts produce one active and one trailing refresh;
- `loading.test.mjs` — only snapshot-owning routes fetch the large grid;
- `pending.test.mjs` — snapshot/SSE ordering and generation safety;
- `routing.test.mjs` — network preservation and valid destinations;
- `view-transition.test.mjs` — data rerenders do not replay navigation;
- `responsive.test.mjs` — structural CSS/accessibility contracts;
- `guide.test.mjs` — guide route, legacy redirect, section completeness, copy wiring;
- galaxy/graph tests — tier identity safety, camera behavior, hit targets, preload sharing;
- token directory tests — identity search, status separation, non-mutating sort/filter.

These tests pin the reason behind the CSS/JavaScript, not pixel values. Visual browser verification is still appropriate after layout changes.

## Operational tests

Traffic fixtures use synthetic Caddy rows. They prove that:

- identifiers are normalized before ranking;
- scanners and bots do not inflate visitors;
- health checks are separated;
- SSE lifetime does not corrupt latency;
- time windows and malformed rows behave predictably;
- no visitor identifiers appear in output.

Digest changes should be dry-run first. Fetch retry and delivery retry have intentionally different safety rules; tests or review must preserve that distinction.

## Regression-test rule

Every bug fix should add the smallest test that would have failed before the fix. The test name should state the user-visible invariant, not the internal method.

Good: `a resolution received before the in-flight snapshot cannot resurrect that transaction`.

Weak: `pending merge test 4`.

## Pre-push checklist

```bash
cargo fmt --all -- --check
cargo test --workspace
node --test web/*.test.mjs
python3 -m unittest scripts/test_traffic_report.py
git diff --check
```

Then inspect the staged diff to ensure generated databases, local Obsidian state, screenshots, credentials, and unrelated work are excluded.

## Related notes

- [[System Map]]
- [[Sync Engine]]
- [[Storage Schema]]
- [[Decoding]]
- [[Web Explorer]]
- [[Operations]]
