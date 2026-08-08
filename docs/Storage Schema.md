# Storage Schema

SQLite (`rusqlite`, bundled, WAL mode, 10s busy timeout — concurrent readers like backups and `serve` snapshots must wait out write bursts instead of failing with `SQLITE_BUSY`; that silent failure mode actually bit the worker's backups during the July 2 storm). One file per network — default `~/.kascov/<network>.db`. Disposable by design: rebuildable from the node's pruning point, `kascov reset --yes` drops it. The `meta.network` guard refuses to mix networks in one file. Production keeps continuous local databases with verified offsite backups ([[Architecture#Deployment topology (live since July 22)]]).

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
  txid, accepting_block, accepting_daa,
  payload, lane_namespace, payload_tag, inscription_kind,
  tx_index, accepting_time_ms, accepting_blue_score
)                             -- indexes: by accepting_block (rollback),
                              --          by accepting_daa (global recent feed)

covenant_utxos(               -- every covenant-bound output ever seen
  txid, output_index,         -- PK
  covenant_id, value, spk_version, spk_script,
  created_block, created_daa,
  spent_block, spent_txid,    -- NULL while live
  spent_sig                   -- the spend's signature script (spend-time
  template, revealed_template -- cached deterministic recognition
)

verified_sources(program_hash PK, program_hex, source, args, template, verified_at)
webhook_subscriptions(id PK, covenant_id, kind, url, created_at)
reorg_log(id PK, daa, at_ms, rolled_back)

tokens(token_id PK, status, invalid_reason, supply, minted, burned,
       holders, unresolved_cells, last_activity_daa, fields_json, derived_at_daa)
token_minters(minter_covenant_id, token_id)
token_events(token_id, covenant_id, seq, delta_idx, kind, amount,
             owner_from, owner_to, accepting_daa, tx_index)
token_balances(token_id, owner, balance, cells)
token_trades(token_id, seq, txid, market_covenant_id, side,
             base_amount, quote_sompi, accepting_daa, accepting_time_ms)
market_programs(covenant_id PK, program_hash, skeleton, invariant_ok,
                exercised_trades, token_covenant_id, lp_token_covenant_id)
vesting_schedules(token_id PK, lock_covenant_id UNIQUE, creator_pubkey,
                  total, start_score, duration_score, genesis_txid,
                  genesis_output_index, template_hash, source, proved_at_daa)
```

## Table families

### Canonical chain-derived tables

`covenants`, `covenant_events`, and `covenant_utxos` are the primary record. They are the only source from which lifecycle truth is derived.

`tx_index`, accepting time, and blue score are capture-era additions. `NULL` means the historical node data was unavailable, not zero. Payload classifications are stamped on write so analytics do not repeatedly parse the full event table.

### Deterministic derived tables

The KCC20 tables are rebuildable projections:

- `tokens` stores the validation verdict and aggregate provenance;
- `token_events` records per-event deltas, including multi-output fan-out;
- `token_balances` contains only live hash-proven cells;
- `token_minters` links governing minter/vault covenants to tokens;
- `token_trades` stores exact chain-derived amount pairs, while publication
  joins through a verified token and an allowlisted, invariant-checked
  `market_programs` row;
- `vesting_schedules` accepts an external schedule only as a candidate. The
  row is written only when the full audited template plus candidate state
  reproduces the genesis lock's P2SH commitment. Live continuation states are
  re-proved against their own commitments when served.

`verified` is fail-closed: an unknown transition, ambiguous amount, unresolved live cell, or broken commitment prevents that verdict and records a reason.

### Operator/application tables

`verified_sources` stores community submissions only after byte-identical compile verification. `webhook_subscriptions` stores delivery intent; it does not imply delivery success. `reorg_log` is append-only operational evidence of rollbacks the index actually applied.

## Transaction boundaries

`Store::apply` is the atomic boundary for one accepting chain block. It updates events, cells, covenant summaries, and the cursor together. Rollback uses accepting-block indexes to:

1. unspend cells spent by removed blocks and clear `spent_sig`;
2. delete cells created by removed blocks;
3. delete removed events;
4. remove empty covenants and refresh surviving summaries;
5. record the rollback in `reorg_log`.

Gap recovery uses merge/finalize operations because it inserts history behind already-indexed rows. It must resequence per-covenant `seq` values and rebuild affected projections before the healed copy is safe to restore.

## Query and index rationale

- `ev_by_accepting` makes reorg deletion bounded by removed blocks.
- `ev_by_daa` powers recent/global feeds and gap detection.
- `ev_by_txid` powers transaction pages.
- UTXO covenant/created/spent indexes support status, rollback, and lineage joins.
- token activity and balance indexes serve directory pagination and top holders.
- compound cursor pagination uses activity DAA plus id as a deterministic tie-breaker.

## Null and empty semantics

- `NULL genesis_*` = first seen mid-life.
- `NULL spent_*` = currently live.
- `NULL template` = not decoded yet; empty string = decoded, no match.
- `NULL tx_index/time/blue_score` = predates capture or is beyond retained acceptance data.
- `NULL token amount/supply` = not provable; never coerce to zero.
- empty payload classification = inspected but no classification; `NULL` may mean no payload or pre-backfill.

## Why these shapes

- **Events are attributed to their accepting chain block**, not their containing block — that's what reorg rollback keys on (`DELETE ... WHERE accepting_block IN (removed)`).
- **UTXOs carry both `created_block` and `spent_block`** so rollback is two UPDATE/DELETEs, no undo log: un-spend what the removed block spent (also clearing `spent_sig`), delete what it created.
- **`spent_sig` lives on the UTXO row**, not the event: the reveal belongs to the specific state that was consumed, and it rolls back with the spend.
- A covenant's **status is derived**, not stored: active = any UTXO with `spent_block IS NULL`; burned = events exist but no live UTXO.
- A covenant may have **multiple live UTXOs** (KIP-20 allows several outputs sharing one id in a tx) — hence a UTXO table rather than a single "tip" column.
- **Schema migrations are additive**: `execute_batch(SCHEMA)` uses `IF NOT EXISTS`, and new columns are `ALTER TABLE … ADD COLUMN` attempts whose duplicate-column error means "already done" — old DBs (including the worker's GCS restores) upgrade on open.

Cursor advance and event writes share one SQLite transaction — crash-consistent resume is free ([[Sync Engine#Flow]]).

See [[Testing Strategy]] for store-level replay and recovery verification.
