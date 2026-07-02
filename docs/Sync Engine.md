# Sync Engine

`kascov-core/src/sync.rs` — the correctness core. Design: **acceptance-driven**. Kaspa is a blockDAG; individual blocks can be red/unaccepted. A covenant state transition only "happens" when its transaction is accepted by the **virtual selected parent chain** (VCC). So the engine follows chain blocks, not raw blocks.

## Flow

1. **Tip note** — each pass starts by recording the node's virtual DAA score + wall clock in [[Storage Schema|meta]] (`tip_daa`/`tip_at_ms`). Exports anchor on it, which is what makes every timestamp on the site exact rather than estimated.
2. **Cursor** = last processed chain block (persisted in meta). Fresh index starts at the current sink (or `--from <hash>`), persisting immediately.
3. `get_virtual_chain_from_block(cursor, include_accepted_tx_ids=true)` returns removed chain blocks (reorg) + added chain blocks with their accepted tx ids.
4. **Removed** → `store.rollback()`: un-spend UTXOs spent by those blocks (clearing captured sig scripts), delete UTXOs created there, delete events, drop covenants whose genesis rolled back.
5. **Added** → accepting blocks are **prefetched concurrently** (`FETCH_AHEAD = 16`, ordered) while store work stays sequential. This is load-bearing: sequential WAN fetches (~10 blocks/s) could not outrun TN10's 10 bps during the July 2 covenant storm — the index fell ~85 min behind; with prefetch it converges at several times chain speed. Accepted tx bodies resolve from the accepting block first, then its mergeset (fetched concurrently). Classify with `classify()`.
6. **Apply atomically**: events + UTXO changes (including each covenant spend's **signature script**, for [[Decoding#Spend-time decoding (shipped)]]) + cursor advance in one SQLite transaction per event-carrying chain block. Event-less blocks advance the cursor every 500 blocks (checkpoint) — replay after a crash is safe because skipped blocks by definition carried no events.

## Classification

Per accepted transaction, per covenant id:

| spends covenant UTXO | creates output with id | event |
|---|---|---|
| yes | yes | `transition` |
| yes | no | `burn` |
| no | yes (id known) | `transition` (anomaly-tolerant) |
| no | yes (id unknown, **KIP-20 hash validates**) | `genesis` |
| no | yes (id unknown, hash does **not** validate) | `transition` + `lineage_complete = false` |

**KIP-20 genesis validation** (`is_valid_genesis`): the claimed id must recompute from the authorizing input's previous outpoint + this tx's outputs bound to the id — via `node::compute_covenant_id`, a boundary wrapper around the consensus implementation at the pinned rev ([[Architecture#Design rules]] Rule 1). Consensus already rejects invalid ids on-chain, so a mismatch can only mean one thing: a covenant born before we started watching. It is recorded honestly as a truncated lineage instead of a fake genesis.

An **intra-block overlay** handles chains within one accepting block (tx B spending a covenant UTXO created by tx A accepted in the same chain block) — store lookups alone would miss those.

## Pruning and truncated lineage

Nodes prune after ~3 days. Covenants first seen mid-life get `lineage_complete = false` and `trace` prints `[history truncated — covenant first seen mid-life]` instead of silently lying. Continuous indexing from day one is what makes the record complete — see [[Home#Why indexing matters (the product moat)]].

## Testing

`tests/sync_replay.rs` drives the real engine + store with a scripted `FakeChain` (via the `ChainSource` trait): genesis (with a **real KIP-20 id**, so validation is exercised) → transition → burn, sig-script capture, reorg rollback of the burn (sig cleared), re-acceptance convergence, tip recording, and mid-life truncation (unprovable genesis → transition + incomplete lineage).
