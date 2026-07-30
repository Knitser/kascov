# Storage Schema

SQLite (`rusqlite`, bundled, WAL mode, 10s busy timeout — concurrent readers like backups and `serve` snapshots must wait out write bursts instead of failing with `SQLITE_BUSY`; that silent failure mode actually bit the worker's backups during the July 2 storm). One file per network — default `~/.kascov/<network>.db`. Disposable by design: rebuildable from the node's pruning point, `kascov reset --yes` drops it. The `meta.network` guard refuses to mix networks in one file. The hosted worker keeps its DBs continuous across restarts via GCS backup/restore ([[Architecture#Deployment topology (live since July 2)]]).

```sql
meta(key, value)              -- network, cursor (last chain block),
                              -- tip_daa + tip_at_ms (chain tip anchor, written
                              -- every sync pass — exports date events with it)

covenants(
  covenant_id BLOB PK,
  genesis_txid, genesis_daa,  -- NULL when first seen mid-life
  lineage_complete,           -- see [[Sync Engine#Classification]]
  event_count, last_activity_daa
)

covenant_events(              -- the lineage log
  covenant_id, seq,           -- PK (covenant_id, seq), seq = per-covenant counter
  kind,                       -- genesis | transition | burn
  txid, accepting_block, accepting_daa
)                             -- indexes: by accepting_block (rollback),
                              --          by accepting_daa (global recent feed)

covenant_utxos(               -- every covenant-bound output ever seen
  txid, output_index,         -- PK
  covenant_id, value, spk_version, spk_script,
  created_block, created_daa,
  spent_block, spent_txid,    -- NULL while live
  spent_sig                   -- the spend's signature script (spend-time
)                             -- decoding; additive migration on open)
```

## Why these shapes

- **Events are attributed to their accepting chain block**, not their containing block — that's what reorg rollback keys on (`DELETE ... WHERE accepting_block IN (removed)`).
- **UTXOs carry both `created_block` and `spent_block`** so rollback is two UPDATE/DELETEs, no undo log: un-spend what the removed block spent (also clearing `spent_sig`), delete what it created.
- **`spent_sig` lives on the UTXO row**, not the event: the reveal belongs to the specific state that was consumed, and it rolls back with the spend.
- A covenant's **status is derived**, not stored: active = any UTXO with `spent_block IS NULL`; burned = events exist but no live UTXO.
- A covenant may have **multiple live UTXOs** (KIP-20 allows several outputs sharing one id in a tx) — hence a UTXO table rather than a single "tip" column.
- **Schema migrations are additive**: `execute_batch(SCHEMA)` uses `IF NOT EXISTS`, and new columns are `ALTER TABLE … ADD COLUMN` attempts whose duplicate-column error means "already done" — old DBs (including the worker's GCS restores) upgrade on open.

Cursor advance and event writes share one SQLite transaction — crash-consistent resume is free ([[Sync Engine#Flow]]).
