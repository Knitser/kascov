# Sync Engine

`kascov-core/src/sync.rs` — the correctness core. Design: **acceptance-driven**. Kaspa is a blockDAG; individual blocks can be red/unaccepted. A covenant state transition only "happens" when its transaction is accepted by the **virtual selected parent chain** (VCC). So the engine follows chain blocks, not raw blocks.

## Flow

1. **Cursor** = last processed chain block (persisted in [[Storage Schema|meta]]). Fresh index starts at the current sink (or `--from <hash>`), persisting immediately.
2. `get_virtual_chain_from_block(cursor, include_accepted_tx_ids=true)` returns removed chain blocks (reorg) + added chain blocks with their accepted tx ids.
3. **Removed** → `store.rollback()`: un-spend UTXOs spent by those blocks, delete UTXOs created there, delete events, drop covenants whose genesis rolled back.
4. **Added** → resolve accepted tx bodies: the accepting block's own txs first, then its mergeset blocks (`merge_set_blues_hashes` + reds, fetched concurrently). Classify with `classify()`.
5. **Apply atomically**: events + UTXO changes + cursor advance in one SQLite transaction per event-carrying chain block. Event-less blocks advance the cursor every 500 blocks (checkpoint) — replay after a crash is safe because skipped blocks by definition carried no events.

## Classification

Per accepted transaction, per covenant id:

| spends covenant UTXO | creates output with id | event |
|---|---|---|
| yes | yes | `transition` |
| yes | no | `burn` |
| no | yes (id unknown) | `genesis` |
| no | yes (id known) | `transition` (anomaly-tolerant) |

An **intra-block overlay** handles chains within one accepting block (tx B spending a covenant UTXO created by tx A accepted in the same chain block) — store lookups alone would miss those.

## Pruning and truncated lineage

Nodes prune after ~3 days. Covenants first seen mid-life get `lineage_complete = false` and `trace` prints `[history truncated — covenant first seen mid-life]` instead of silently lying. Continuous indexing from day one is what makes the record complete — see [[Home#Why indexing matters (the product moat)]].

## Testing

`tests/sync_replay.rs` drives the real engine + store with a scripted `FakeChain` (via the `ChainSource` trait, see [[Architecture#Design rules]]): genesis → transition → burn, reorg rollback of the burn, re-acceptance convergence, and mid-life truncation.
