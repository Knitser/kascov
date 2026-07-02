# Storage Schema

SQLite (`rusqlite`, bundled, WAL mode). One file per network — default `~/.kascov/<network>.db`. Disposable by design: rebuildable from the node's pruning point, `kascov reset --yes` drops it (testnet resets happen). The `meta.network` guard refuses to mix networks in one file.

```sql
meta(key, value)              -- schema_version, network, cursor (last chain block)

covenants(
  covenant_id BLOB PK,
  genesis_txid, genesis_daa,  -- NULL when first seen mid-life
  lineage_complete,           -- see [[Sync Engine#Pruning and truncated lineage]]
  event_count, last_activity_daa
)

covenant_events(              -- the lineage log
  covenant_id, seq,           -- PK (covenant_id, seq), seq = per-covenant counter
  kind,                       -- genesis | transition | burn
  txid, accepting_block, accepting_daa
)

covenant_utxos(               -- every covenant-bound output ever seen
  txid, output_index,         -- PK
  covenant_id, value, spk_version, spk_script,
  created_block, created_daa,
  spent_block, spent_txid     -- NULL while live
)
```

## Why these shapes

- **Events are attributed to their accepting chain block**, not their containing block — that's what reorg rollback keys on (`DELETE ... WHERE accepting_block IN (removed)`).
- **UTXOs carry both `created_block` and `spent_block`** so rollback is two UPDATE/DELETEs, no undo log: un-spend what the removed block spent, delete what it created.
- A covenant's **status is derived**, not stored: active = any UTXO with `spent_block IS NULL`; burned = events exist but no live UTXO.
- A covenant may have **multiple live UTXOs** (KIP-20 allows several outputs sharing one id in a tx) — hence a UTXO table rather than a single "tip" column.

Cursor advance and event writes share one SQLite transaction — crash-consistent resume is free ([[Sync Engine#Flow]]).
