# Toccata research & on-chain classification audit — July 5, 2026

## A. The audit: is born/moved/retired true to the chain?

Method: independently re-derive every event's kind from **raw chain data** (the community REST APIs `api-tn10.kaspa.org` / `api.kaspa.org`, which expose `covenant_id` + `covenant_authorizing_input` per output and `compute_budget` per input) and diff against kascov's index. Rule applied, straight from KIP-20 semantics:

- spends no bound state, creates bound outputs → **genesis**
- spends bound state, creates bound outputs (same id) → **transition**
- spends bound state, creates none → **burn**

Sample: 18 coins across both networks (biased toward multi-event lives), 47 events.

**Result: 46/47 exact matches**, including every genesis (with acceptance confirmed), every split-birth ("in 2 pieces"), every staged burn, and the user-flagged `stubborn-pearl-magpie` (chain confirms: genesis created two 284.65 TKAS bound states in one tx; two later txs each destroyed one — our two burn events are both real).

### The one mismatch — an open investigation

`901be291ef…` (mainnet, part of the ZK covenant pair): our index calls tx `2dd9f945…` its **genesis** (KIP-20 hash validated via the node's own `kaspa_consensus_core::covenant_id` function, `lineage_complete=true`). The REST API instead shows that tx **spending an already-bound `901be…` state** (`6c0d3e92…#1`) — i.e. a *transition*, with the true genesis earlier.

Notable: `2dd9f945…` is a **multi-covenant transaction** — it moves `09ef275e…` (quiet-pearl-zebra) *and* `901be…` in a single tx (input 0 → output 0 continues zebra; input 2 → output 1 continues/creates 901be). This is the first "complex stateful multi-contract flow" observed on mainnet, and exactly the shape where a genesis-vs-continuation edge case could hide — in ours or in the community API's back-fill (both cannot be right: an id cannot both pre-exist and hash-validate as fresh genesis, so one indexer is mislabeling).

**Action items:**
1. `kascov inspect-tx <txid>` debug command — fetch a tx via our own node connection and print inputs/outputs with bindings; removes third-party APIs from the truth loop.
2. Re-validate `901be…` from node data; if our classifier prefers genesis-hash over known-continuation in multi-covenant txs, fix the precedence (continuation must win when the authorizing input demonstrably spends the same id).
3. Add the derived rule to `sync_replay` tests with a synthetic multi-covenant tx.

## B. Toccata features we don't cover yet (research findings)

Sources: docs.kaspa.org/toccata, rusty-kaspa `docs/toccata-guide.md`.

| # | Feature | What it is | kascov opportunity |
|---|---------|------------|--------------------|
| 1 | **KIP-21 lanes / seqcommits** | v1 payloads carry app lanes (namespace + gas); nodes serve `GetSeqCommitLaneProof` | Decode lane payloads on events; **"based-app activity" analytics** (events per namespace); surface gas. Nobody exposes this yet. |
| 2 | **Multi-covenant transactions** | one tx moves several covenants (observed live on mainnet) | **"moved together with <coin>"** timeline links + app-graph view (covenant families). We already store the shared txid — pure product work. |
| 3 | **Block color / reward info** | `GetBlockRewardInfo` returns BLUE/RED per block | confidence badge per event ("blue-accepted") |
| 4 | **Fee rule** | fee = 100 sompi × max(compute grams, 2× tx bytes) | show real cost per covenant spend next to compute_budget — fee planning data for contract authors |
| 5 | **storageMass** | replaces `mass` in v1 APIs | include in nerd mode per spend |
| 6 | **Three hash contexts** | txid / tx.hash / sighash split | decoder note only; low value |

Priority: 2 (unique + data already indexed) → 1 (unique, medium effort) → 4 (easy, devs love it) → 3/5 (cosmetic).

## C. "The overview doesn't say much" — storytelling upgrades

Shipped tonight: hover glossary site-wide; multi-piece truth ("born holding X **in 2 pieces**", "**lost a piece** (one state destroyed)"); range-selectable activity chart; honest lag badge.

Next wave (data already available client-side unless noted):
1. Transition rows: value delta and **split/merge shape** ("moved, splitting 1 → 2 pieces").
2. Burn rows: destroyed amount per burn (balance delta at that DAA).
3. **Cross-covenant mentions**: when another coin shares the event's txid → "in the same transaction as <coin>" (needs a small `/tx/` extension to return all covenants for a txid, not just one).
4. Grid cards: template chip (Mecenas/Escrow/…) — template is now persisted per UTXO server-side; expose a per-covenant `template` in the grid feed.
5. Genesis rows: funder address ("funded by kaspa:qz3e…", needs storing the authorizing input's previous address at index time — additive column).
