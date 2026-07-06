//! SQLite index of covenant activity. One file per network, disposable and
//! rebuildable — the value is continuity (nodes prune, we don't).

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;

use crate::model::*;
use crate::{Error, Result};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS covenants (
    covenant_id BLOB PRIMARY KEY,
    genesis_txid BLOB,
    genesis_daa INTEGER,
    lineage_complete INTEGER NOT NULL DEFAULT 1,
    event_count INTEGER NOT NULL DEFAULT 0,
    last_activity_daa INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS covenant_events (
    covenant_id BLOB NOT NULL,
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL, -- genesis | transition | burn
    txid BLOB NOT NULL,
    accepting_block BLOB NOT NULL,
    accepting_daa INTEGER NOT NULL,
    payload BLOB,       -- the tx's v1 payload, when non-empty
    PRIMARY KEY (covenant_id, seq)
);
CREATE INDEX IF NOT EXISTS ev_by_accepting ON covenant_events(accepting_block);
CREATE INDEX IF NOT EXISTS ev_by_daa ON covenant_events(accepting_daa);
CREATE INDEX IF NOT EXISTS ev_by_txid ON covenant_events(txid);
CREATE TABLE IF NOT EXISTS covenant_utxos (
    txid BLOB NOT NULL,
    output_index INTEGER NOT NULL,
    covenant_id BLOB NOT NULL,
    value INTEGER NOT NULL,
    spk_version INTEGER NOT NULL,
    spk_script BLOB NOT NULL,
    created_block BLOB NOT NULL,
    created_daa INTEGER NOT NULL,
    spent_block BLOB,
    spent_txid BLOB,
    spent_sig BLOB,
    -- template columns: NULL = not yet decoded, '' = decoded but no template
    -- matched, else the recognized template name. revealed_template is the
    -- same for the verified P2SH program revealed by this row's spend.
    template TEXT,
    revealed_template TEXT,
    PRIMARY KEY (txid, output_index)
);
CREATE INDEX IF NOT EXISTS utxo_by_covenant ON covenant_utxos(covenant_id);
CREATE INDEX IF NOT EXISTS utxo_by_created ON covenant_utxos(created_block);
CREATE INDEX IF NOT EXISTS utxo_by_spent ON covenant_utxos(spent_block) WHERE spent_block IS NOT NULL;
";

pub struct Store {
    conn: Connection,
}

#[derive(Clone, Debug, Serialize)]
pub struct CovenantSummary {
    pub covenant_id: CovenantId,
    pub genesis_txid: Option<TxId>,
    pub genesis_daa: Option<u64>,
    pub lineage_complete: bool,
    pub event_count: u64,
    pub last_activity_daa: u64,
    pub live_utxos: u64,
    pub live_value: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct EventRow {
    pub seq: u64,
    pub kind: String,
    pub txid: TxId,
    pub accepting_block: BlockHash,
    pub accepting_daa: u64,
    /// The transaction's v1 payload, when it carried one.
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "opt_hex_ser")]
    pub payload: Option<Vec<u8>>,
}

fn opt_hex_ser<S: serde::Serializer>(bytes: &Option<Vec<u8>>, s: S) -> std::result::Result<S::Ok, S::Error> {
    match bytes {
        Some(b) => s.serialize_str(&hex::encode(b)),
        None => s.serialize_none(),
    }
}

/// Whole-index aggregates, computed inside SQLite.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct StoreStats {
    pub covenants: u64,
    pub active: u64,
    pub burned: u64,
    pub total_events: u64,
    pub live_value: u64,
    pub last_activity_daa: u64,
}

/// Activity inside a trailing DAA window, plus current liveness — pure SQL.
#[derive(Clone, Debug)]
pub struct DigestStats {
    pub births: u64,
    pub moves: u64,
    pub burns: u64,
    pub value_born: u64,
    pub active_now: u64,
    /// (covenant, events inside the window)
    pub busiest: Option<(CovenantId, u64)>,
    /// (covenant, value at birth) among covenants born inside the window
    pub biggest_birth: Option<(CovenantId, u64)>,
}

/// An event joined with its covenant, for cross-covenant feeds.
#[derive(Clone, Debug, Serialize)]
pub struct GlobalEventRow {
    pub covenant_id: CovenantId,
    pub seq: u64,
    pub kind: String,
    pub txid: TxId,
    pub accepting_daa: u64,
}

/// One fixed-width DAA bucket of covenant activity: kind counts inside
/// [daa, daa + bucket width). Buckets with no events are never stored.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ActivityBucket {
    pub daa: u64,
    pub births: u64,
    pub moves: u64,
    pub burns: u64,
}

/// A covenant a pubkey has appeared in as a p2pk-state owner, with role hints.
#[derive(Clone, Debug, Serialize)]
pub struct PubkeyCovenantRow {
    pub covenant_id: CovenantId,
    /// The key currently owns at least one live state UTXO of this covenant.
    pub controls_now: bool,
    /// How many state UTXOs (live + spent) have carried this key.
    pub states_seen: u64,
    pub first_seen_daa: u64,
    pub last_seen_daa: u64,
}

/// One recognized script shape's footprint across every state UTXO ever
/// indexed. `template: None` is the unrecognized bucket.
#[derive(Clone, Debug, Serialize)]
pub struct TemplateStat {
    pub template: Option<String>,
    pub live_states: u64,
    pub live_value: u64,
    pub ever_seen: u64,
    pub covenants: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct UtxoRow {
    pub outpoint: Outpoint,
    pub value: u64,
    pub spk_version: u16,
    #[serde(serialize_with = "crate::detect::hex_ser")]
    pub spk_script: Vec<u8>,
    pub created_daa: u64,
    pub live: bool,
    pub spent_txid: Option<TxId>,
    /// Unlocking script of the spend, when captured (spend-time decoding).
    pub spent_sig: Option<Vec<u8>>,
    /// The spending input's v1 compute-budget commitment.
    pub spent_budget: Option<u16>,
}

/// Events produced while processing one accepting chain block, applied atomically.
pub struct BlockEvents {
    pub accepting_block: BlockHash,
    pub accepting_daa: u64,
    pub events: Vec<NewEvent>,
    pub created_utxos: Vec<NewUtxo>,
    /// (outpoint, spending txid, spending input's signature script, budget)
    pub spent_utxos: Vec<(Outpoint, TxId, Vec<u8>, u16)>,
}

impl BlockEvents {
    pub fn empty(accepting_block: BlockHash) -> Self {
        Self {
            accepting_block,
            accepting_daa: 0,
            events: vec![],
            created_utxos: vec![],
            spent_utxos: vec![],
        }
    }
}

pub struct NewEvent {
    pub covenant_id: CovenantId,
    pub kind: EventKind,
    pub txid: TxId,
    /// The tx's v1 payload, stored only when non-empty.
    pub payload: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    Genesis,
    Transition,
    Burn,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Genesis => "genesis",
            EventKind::Transition => "transition",
            EventKind::Burn => "burn",
        }
    }
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub struct NewUtxo {
    pub outpoint: Outpoint,
    pub covenant_id: CovenantId,
    pub value: u64,
    pub spk_version: u16,
    pub spk_script: Vec<u8>,
}

fn db_err(e: rusqlite::Error) -> Error {
    Error::Rpc(format!("store: {e}"))
}

/// Process-wide decode registry for write-time template recognition —
/// construction derives the SilverScript skeletons once, and Registry is
/// Send + Sync (its decoders are `Box<dyn StateDecoder: Send + Sync>`).
fn registry() -> &'static kascov_decode::Registry {
    static REGISTRY: std::sync::OnceLock<kascov_decode::Registry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(kascov_decode::Registry::default)
}

impl Store {
    pub fn open(path: &Path, network: Network) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Invalid { what: "db path", value: e.to_string() })?;
        }
        let conn = Connection::open(path).map_err(db_err)?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(db_err)?;
        // Concurrent readers (backup, serve snapshots) must wait out write
        // bursts instead of failing with SQLITE_BUSY.
        conn.busy_timeout(std::time::Duration::from_secs(10)).map_err(db_err)?;
        conn.execute_batch(SCHEMA).map_err(db_err)?;
        // Additive migrations for pre-existing databases (SQLite has no
        // ADD COLUMN IF NOT EXISTS; a duplicate-column error means done).
        let _ = conn.execute("ALTER TABLE covenant_utxos ADD COLUMN spent_sig BLOB", []);
        let _ = conn.execute("ALTER TABLE covenant_utxos ADD COLUMN spent_budget INTEGER", []);
        let _ = conn.execute("ALTER TABLE covenant_events ADD COLUMN payload BLOB", []);
        let _ = conn.execute("ALTER TABLE covenant_utxos ADD COLUMN template TEXT", []);
        let _ = conn.execute("ALTER TABLE covenant_utxos ADD COLUMN revealed_template TEXT", []);
        // Partial "todo" indexes keep the backfill probe below O(1) once every
        // row is stamped. They reference the columns added above, so they must
        // be created here (after the ALTERs), never inside SCHEMA — and unlike
        // the duplicate-column ALTERs, a failure here is a real error.
        conn.execute(
            "CREATE INDEX IF NOT EXISTS utxo_template_todo ON covenant_utxos(template) WHERE template IS NULL",
            [],
        )
        .map_err(db_err)?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS utxo_reveal_todo ON covenant_utxos(revealed_template) WHERE spent_sig IS NOT NULL AND revealed_template IS NULL",
            [],
        )
        .map_err(db_err)?;

        let mut store = Self { conn };
        match store.meta("network")? {
            None => store.set_meta("network", &network.to_string())?,
            Some(existing) if existing != network.to_string() => {
                return Err(Error::NodeMismatch(format!(
                    "index at {} belongs to {existing}, not {network}",
                    path.display()
                )));
            }
            Some(_) => {}
        }
        // After the ownership check — a wrong-network database is never mutated.
        store.backfill_templates()?;
        Ok(store)
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| row.get(0))
            .optional()
            .map_err(db_err)
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute("INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)", params![key, value])
            .map_err(db_err)?;
        Ok(())
    }

    pub fn cursor(&self) -> Result<Option<BlockHash>> {
        Ok(self.meta("cursor")?.and_then(|s| s.parse().ok()))
    }

    /// Record where the chain tip was (virtual DAA score) and when we saw it,
    /// atomically — exports anchor DAA scores to wall-clock time with this.
    pub fn set_tip(&self, daa: u64, at_ms: u64) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO meta (key, value)
                 VALUES ('tip_daa', ?1), ('tip_at_ms', ?2)",
                params![daa.to_string(), at_ms.to_string()],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// The last recorded chain tip as (virtual DAA, wall-clock ms), if any.
    pub fn tip(&self) -> Result<Option<(u64, u64)>> {
        let daa: Option<u64> = self.meta("tip_daa")?.and_then(|s| s.parse().ok());
        let at_ms: Option<u64> = self.meta("tip_at_ms")?.and_then(|s| s.parse().ok());
        Ok(daa.zip(at_ms))
    }

    /// The DAA score of the last chain block the indexer actually applied —
    /// unlike tip(), this can never run ahead of what the index contains.
    pub fn processed_daa(&self) -> Result<Option<u64>> {
        Ok(self.meta("processed_daa")?.and_then(|s| s.parse().ok()))
    }

    /// Point the cursor at a new chain block without touching indexed data —
    /// recovery for testnet resets, where the stored cursor no longer exists
    /// on the node and sync would otherwise wedge forever.
    pub fn reset_cursor(&mut self, to: BlockHash) -> Result<()> {
        self.apply(&BlockEvents::empty(to), to)
    }

    /// Write a consistent copy of the database (safe while a writer is active).
    pub fn backup_to(&self, out: &Path) -> Result<()> {
        if out.exists() {
            std::fs::remove_file(out)
                .map_err(|e| Error::Invalid { what: "backup path", value: e.to_string() })?;
        }
        let path = out.to_string_lossy();
        self.conn.execute("VACUUM INTO ?1", [path.as_ref()]).map_err(db_err)?;
        Ok(())
    }

    /// Stamp template recognition onto rows that predate the columns (or were
    /// written by an older binary): one-shot after a migration, O(1) probes
    /// against the empty partial "todo" indexes on every open after that.
    /// Batched transactions keep each writer hold short under busy_timeout.
    fn backfill_templates(&mut self) -> Result<()> {
        const BATCH: i64 = 2000;
        let mut states = 0u64;
        loop {
            // Statement scoped so its borrow ends before the write transaction.
            let rows: Vec<(i64, u16, Vec<u8>)> = {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT rowid, spk_version, spk_script FROM covenant_utxos
                         WHERE template IS NULL LIMIT ?1",
                    )
                    .map_err(db_err)?;
                let collected = stmt
                    .query_map([BATCH], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                    .map_err(db_err)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(db_err)?;
                collected
            };
            if rows.is_empty() {
                break;
            }
            let tx = self.conn.transaction().map_err(db_err)?;
            for (rowid, version, script) in &rows {
                let template = registry().decode(*version, script).template.unwrap_or("");
                tx.execute(
                    "UPDATE covenant_utxos SET template = ?1 WHERE rowid = ?2",
                    params![template, rowid],
                )
                .map_err(db_err)?;
            }
            tx.commit().map_err(db_err)?;
            states += rows.len() as u64;
        }
        let mut reveals = 0u64;
        loop {
            let rows: Vec<(i64, u16, Vec<u8>, Vec<u8>)> = {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT rowid, spk_version, spk_script, spent_sig FROM covenant_utxos
                         WHERE spent_sig IS NOT NULL AND revealed_template IS NULL LIMIT ?1",
                    )
                    .map_err(db_err)?;
                let collected = stmt
                    .query_map([BATCH], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                    })
                    .map_err(db_err)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(db_err)?;
                collected
            };
            if rows.is_empty() {
                break;
            }
            let tx = self.conn.transaction().map_err(db_err)?;
            for (rowid, version, spk, sig) in &rows {
                let template = kascov_decode::p2sh_reveal(spk, sig)
                    .and_then(|redeem| registry().decode(*version, &redeem).template)
                    .unwrap_or("");
                tx.execute(
                    "UPDATE covenant_utxos SET revealed_template = ?1 WHERE rowid = ?2",
                    params![template, rowid],
                )
                .map_err(db_err)?;
            }
            tx.commit().map_err(db_err)?;
            reveals += rows.len() as u64;
        }
        if states + reveals > 0 {
            tracing::info!("template backfill: {states} state scripts decoded, {reveals} spend reveals checked");
        }
        Ok(())
    }

    /// Is this outpoint a live covenant UTXO? Returns its covenant id.
    pub fn live_covenant_utxo(&self, outpoint: &Outpoint) -> Result<Option<CovenantId>> {
        self.conn
            .query_row(
                "SELECT covenant_id FROM covenant_utxos
                 WHERE txid = ?1 AND output_index = ?2 AND spent_block IS NULL",
                params![outpoint.txid.0.as_slice(), outpoint.index],
                |row| row.get::<_, [u8; 32]>(0).map(CovenantId),
            )
            .optional()
            .map_err(db_err)
    }

    pub fn known_covenant(&self, id: &CovenantId) -> Result<bool> {
        let count: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM covenants WHERE covenant_id = ?1",
                [id.0.as_slice()],
                |row| row.get(0),
            )
            .map_err(db_err)?;
        Ok(count > 0)
    }

    /// Apply everything observed in one accepting chain block, atomically,
    /// and advance the cursor.
    pub fn apply(&mut self, block: &BlockEvents, new_cursor: BlockHash) -> Result<()> {
        let tx = self.conn.transaction().map_err(db_err)?;
        // Created rows must land BEFORE spends are marked: one accepting chain
        // block can sweep a whole intra-block chain (tx B spending tx A's
        // covenant output), and marking spends first would no-op against the
        // not-yet-inserted row — leaving a zombie "live" UTXO and dropping the
        // captured spend signature.
        for utxo in &block.created_utxos {
            // Recognition is stamped at write time ('' = no template matched)
            // so template analytics stay pure GROUP BYs at read time.
            let template =
                registry().decode(utxo.spk_version, &utxo.spk_script).template.unwrap_or("");
            tx.execute(
                "INSERT OR REPLACE INTO covenant_utxos
                 (txid, output_index, covenant_id, value, spk_version, spk_script,
                  created_block, created_daa, spent_block, spent_txid, template)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, ?9)",
                params![
                    utxo.outpoint.txid.0.as_slice(),
                    utxo.outpoint.index,
                    utxo.covenant_id.0.as_slice(),
                    utxo.value,
                    utxo.spk_version,
                    utxo.spk_script,
                    block.accepting_block.0.as_slice(),
                    block.accepting_daa,
                    template
                ],
            )
            .map_err(db_err)?;
        }
        for (outpoint, spending_txid, sig, budget) in &block.spent_utxos {
            // Spend-time recognition: a verified P2SH reveal names the program
            // that actually ran ('' = spend seen, nothing recognized). Reading
            // the row here is safe because created rows land first (above); a
            // row we never indexed matches neither the SELECT nor the UPDATE
            // and self-heals via the backfill at the next open.
            let revealed: Option<String> = tx
                .query_row(
                    "SELECT spk_version, spk_script FROM covenant_utxos
                     WHERE txid = ?1 AND output_index = ?2",
                    params![outpoint.txid.0.as_slice(), outpoint.index],
                    |r| Ok((r.get::<_, u16>(0)?, r.get::<_, Vec<u8>>(1)?)),
                )
                .optional()
                .map_err(db_err)?
                .map(|(version, spk)| {
                    kascov_decode::p2sh_reveal(&spk, sig)
                        .and_then(|redeem| registry().decode(version, &redeem).template)
                        .unwrap_or("")
                        .to_string()
                });
            tx.execute(
                "UPDATE covenant_utxos SET spent_block = ?1, spent_txid = ?2, spent_sig = ?3, spent_budget = ?4, revealed_template = ?5
                 WHERE txid = ?6 AND output_index = ?7",
                params![
                    block.accepting_block.0.as_slice(),
                    spending_txid.0.as_slice(),
                    sig,
                    budget,
                    revealed,
                    outpoint.txid.0.as_slice(),
                    outpoint.index
                ],
            )
            .map_err(db_err)?;
        }
        for event in &block.events {
            let is_genesis = event.kind == EventKind::Genesis;
            tx.execute(
                "INSERT INTO covenants (covenant_id, genesis_txid, genesis_daa, lineage_complete)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(covenant_id) DO NOTHING",
                params![
                    event.covenant_id.0.as_slice(),
                    is_genesis.then_some(event.txid.0.as_slice()),
                    is_genesis.then_some(block.accepting_daa),
                    is_genesis
                ],
            )
            .map_err(db_err)?;
            tx.execute(
                "INSERT INTO covenant_events (covenant_id, seq, kind, txid, accepting_block, accepting_daa, payload)
                 VALUES (?1,
                   (SELECT COALESCE(MAX(seq), -1) + 1 FROM covenant_events WHERE covenant_id = ?1),
                   ?2, ?3, ?4, ?5, ?6)",
                params![
                    event.covenant_id.0.as_slice(),
                    event.kind.as_str(),
                    event.txid.0.as_slice(),
                    block.accepting_block.0.as_slice(),
                    block.accepting_daa,
                    event.payload
                ],
            )
            .map_err(db_err)?;
            tx.execute(
                "UPDATE covenants SET event_count = event_count + 1, last_activity_daa = ?2
                 WHERE covenant_id = ?1",
                params![event.covenant_id.0.as_slice(), block.accepting_daa],
            )
            .map_err(db_err)?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('cursor', ?1)",
            [new_cursor.to_string()],
        )
        .map_err(db_err)?;
        // The indexer's own progress, distinct from the node tip: during a
        // backlog replay the tip races ahead while this advances block by
        // block. Skipped when the batch carries no DAA (BlockEvents::empty
        // from reset_cursor / the fresh-index bootstrap) — a cursor repoint
        // is not progress and must never stamp 0 over real progress.
        if block.accepting_daa > 0 {
            tx.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('processed_daa', ?1)",
                [block.accepting_daa.to_string()],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)
    }

    /// Undo everything attributed to the given (reorged-out) chain blocks.
    pub fn rollback(&mut self, removed: &[BlockHash]) -> Result<()> {
        let tx = self.conn.transaction().map_err(db_err)?;
        for hash in removed {
            let hash = hash.0.as_slice();
            // Collect covenant IDs affected by this block before deleting,
            // so we can recompute last_activity_daa from remaining events.
            let affected: Vec<Vec<u8>> = {
                let mut stmt = tx
                    .prepare("SELECT DISTINCT covenant_id FROM covenant_events WHERE accepting_block = ?1")
                    .map_err(db_err)?;
                stmt.query_map([hash], |r| r.get::<_, Vec<u8>>(0))
                    .map_err(db_err)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(db_err)?
            };
            // revealed_template goes back to NULL (not ''): with spent_sig
            // NULL the reveal-todo index predicate no longer matches, so the
            // backfill won't re-decode. `template` stays — it derives from the
            // row's own immutable spk_script.
            tx.execute(
                "UPDATE covenant_utxos SET spent_block = NULL, spent_txid = NULL, spent_sig = NULL, spent_budget = NULL, revealed_template = NULL WHERE spent_block = ?1",
                [hash],
            )
            .map_err(db_err)?;
            tx.execute("DELETE FROM covenant_utxos WHERE created_block = ?1", [hash]).map_err(db_err)?;
            tx.execute(
                "UPDATE covenants SET event_count = event_count -
                   (SELECT COUNT(*) FROM covenant_events WHERE accepting_block = ?1 AND covenant_id = covenants.covenant_id)",
                [hash],
            )
            .map_err(db_err)?;
            tx.execute("DELETE FROM covenant_events WHERE accepting_block = ?1", [hash]).map_err(db_err)?;
            // Recompute last_activity_daa for affected covenants — the
            // rolled-back block's DAA may have been the most recent activity.
            for cid in &affected {
                tx.execute(
                    "UPDATE covenants SET last_activity_daa = COALESCE((SELECT MAX(accepting_daa) FROM covenant_events WHERE covenant_id = ?1), 0) WHERE covenant_id = ?1",
                    [cid],
                ).map_err(db_err)?;
            }
        }
        // Covenants whose genesis was rolled back disappear entirely.
        tx.execute("DELETE FROM covenants WHERE event_count <= 0", []).map_err(db_err)?;
        tx.commit().map_err(db_err)
    }

    pub fn list(&self, limit: u64) -> Result<Vec<CovenantSummary>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT c.covenant_id, c.genesis_txid, c.genesis_daa, c.lineage_complete,
                        c.event_count, c.last_activity_daa,
                        (SELECT COUNT(*) FROM covenant_utxos u WHERE u.covenant_id = c.covenant_id AND u.spent_block IS NULL),
                        (SELECT COALESCE(SUM(value), 0) FROM covenant_utxos u WHERE u.covenant_id = c.covenant_id AND u.spent_block IS NULL)
                 FROM covenants c ORDER BY c.last_activity_daa DESC LIMIT ?1",
            )
            .map_err(db_err)?;
        let limit = limit.min(i64::MAX as u64) as i64;
        let rows = stmt
            .query_map([limit], |row| {
                Ok(CovenantSummary {
                    covenant_id: CovenantId(row.get(0)?),
                    genesis_txid: row.get::<_, Option<[u8; 32]>>(1)?.map(TxId),
                    genesis_daa: row.get(2)?,
                    lineage_complete: row.get(3)?,
                    event_count: row.get(4)?,
                    last_activity_daa: row.get(5)?,
                    live_utxos: row.get(6)?,
                    live_value: row.get(7)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    pub fn summary(&self, id: &CovenantId) -> Result<Option<CovenantSummary>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT c.covenant_id, c.genesis_txid, c.genesis_daa, c.lineage_complete,
                        c.event_count, c.last_activity_daa,
                        (SELECT COUNT(*) FROM covenant_utxos u WHERE u.covenant_id = c.covenant_id AND u.spent_block IS NULL),
                        (SELECT COALESCE(SUM(value), 0) FROM covenant_utxos u WHERE u.covenant_id = c.covenant_id AND u.spent_block IS NULL)
                 FROM covenants c WHERE c.covenant_id = ?1",
            )
            .map_err(db_err)?;
        let row = stmt
            .query_map([id.0.as_slice()], |row| {
                Ok(CovenantSummary {
                    covenant_id: CovenantId(row.get(0)?),
                    genesis_txid: row.get::<_, Option<[u8; 32]>>(1)?.map(TxId),
                    genesis_daa: row.get(2)?,
                    lineage_complete: row.get(3)?,
                    event_count: row.get(4)?,
                    last_activity_daa: row.get(5)?,
                    live_utxos: row.get(6)?,
                    live_value: row.get(7)?,
                })
            })
            .map_err(db_err)?
            .next()
            .transpose()
            .map_err(db_err)?;
        Ok(row)
    }

    /// Aggregate stats in pure SQL — never materializes 40k+ summary rows just
    /// to count them (the live feed rebuilds every few seconds).
    pub fn stats(&self) -> Result<StoreStats> {
        let (covenants, total_events, last_activity_daa) = self
            .conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(event_count), 0), COALESCE(MAX(last_activity_daa), 0) FROM covenants",
                [],
                |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?, r.get::<_, u64>(2)?)),
            )
            .map_err(db_err)?;
        let (active, live_value) = self
            .conn
            .query_row(
                "SELECT COUNT(DISTINCT covenant_id), COALESCE(SUM(value), 0)
                 FROM covenant_utxos WHERE spent_block IS NULL",
                [],
                |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?)),
            )
            .map_err(db_err)?;
        Ok(StoreStats {
            covenants,
            active,
            burned: covenants.saturating_sub(active),
            total_events,
            live_value,
            last_activity_daa,
        })
    }

    /// Activity inside the trailing `window_daa` window ("the last 24 hours"),
    /// anchored at the recorded tip — falling back to the newest event for
    /// indexes that predate tip tracking. Pure SQL; ev_by_daa covers the scans.
    pub fn digest(&self, window_daa: u64) -> Result<DigestStats> {
        let tip_daa: Option<u64> = match self.tip()? {
            Some((daa, _)) => Some(daa),
            None => self
                .conn
                .query_row("SELECT MAX(accepting_daa) FROM covenant_events", [], |r| r.get(0))
                .map_err(db_err)?,
        };
        let cutoff = tip_daa.unwrap_or(0).saturating_sub(window_daa);
        let (births, moves, burns) = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(kind = 'genesis'), 0),
                        COALESCE(SUM(kind = 'transition'), 0),
                        COALESCE(SUM(kind = 'burn'), 0)
                 FROM covenant_events WHERE accepting_daa >= ?1",
                params![cutoff],
                |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?, r.get::<_, u64>(2)?)),
            )
            .map_err(db_err)?;
        // same birth definition as born_values(): outputs created at genesis DAA
        let value_born: u64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(u.value), 0)
                 FROM covenant_utxos u
                 JOIN covenants c ON c.covenant_id = u.covenant_id AND u.created_daa = c.genesis_daa
                 WHERE c.genesis_daa >= ?1",
                params![cutoff],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        // ties broken by covenant_id so cached bodies stay deterministic
        let busiest = self
            .conn
            .query_row(
                "SELECT covenant_id, COUNT(*) AS n FROM covenant_events
                 WHERE accepting_daa >= ?1
                 GROUP BY covenant_id ORDER BY n DESC, covenant_id LIMIT 1",
                params![cutoff],
                |r| Ok((CovenantId(r.get(0)?), r.get::<_, u64>(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        let biggest_birth = self
            .conn
            .query_row(
                "SELECT c.covenant_id, COALESCE(SUM(u.value), 0) AS v
                 FROM covenants c
                 JOIN covenant_utxos u ON u.covenant_id = c.covenant_id AND u.created_daa = c.genesis_daa
                 WHERE c.genesis_daa >= ?1
                 GROUP BY c.covenant_id ORDER BY v DESC, c.covenant_id LIMIT 1",
                params![cutoff],
                |r| Ok((CovenantId(r.get(0)?), r.get::<_, u64>(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        // identical semantics to stats().active — the site's "alive right now"
        let active_now: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(DISTINCT covenant_id) FROM covenant_utxos WHERE spent_block IS NULL",
                [],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        Ok(DigestStats { births, moves, burns, value_born, active_now, busiest, biggest_birth })
    }

    /// Kind counts per fixed-width DAA bucket, ascending, for events at or
    /// after `cutoff_daa`. Empty buckets are omitted — callers zero-fill.
    /// ev_by_daa covers the range scan; the boolean-SUM idiom matches digest().
    pub fn activity(&self, bucket_daa: u64, cutoff_daa: u64) -> Result<Vec<ActivityBucket>> {
        let width = bucket_daa.max(1);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT accepting_daa / ?1 AS bucket,
                        COALESCE(SUM(kind = 'genesis'), 0),
                        COALESCE(SUM(kind = 'transition'), 0),
                        COALESCE(SUM(kind = 'burn'), 0)
                 FROM covenant_events
                 WHERE accepting_daa >= ?2
                 GROUP BY bucket ORDER BY bucket",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![width as i64, cutoff_daa as i64], |row| {
                Ok(ActivityBucket {
                    daa: row.get::<_, u64>(0)? * width,
                    births: row.get(1)?,
                    moves: row.get(2)?,
                    burns: row.get(3)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// (MIN, MAX) accepting_daa over every indexed event — None while empty.
    pub fn event_daa_bounds(&self) -> Result<Option<(u64, u64)>> {
        let (min, max): (Option<u64>, Option<u64>) = self
            .conn
            .query_row(
                "SELECT MIN(accepting_daa), MAX(accepting_daa) FROM covenant_events",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db_err)?;
        Ok(min.zip(max))
    }

    /// Per-covenant birth amounts (sum of outputs created at the genesis DAA),
    /// one query for the whole grid.
    pub fn born_values(&self) -> Result<std::collections::HashMap<CovenantId, u64>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT u.covenant_id, COALESCE(SUM(u.value), 0)
                 FROM covenant_utxos u
                 JOIN covenants c ON c.covenant_id = u.covenant_id AND u.created_daa = c.genesis_daa
                 GROUP BY u.covenant_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| Ok((CovenantId(row.get(0)?), row.get::<_, u64>(1)?)))
            .map_err(db_err)?
            .collect::<std::result::Result<std::collections::HashMap<_, _>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// One covenant's birth amount — grid parity for single-covenant endpoints
    /// (born_values() builds the map for the whole grid; this is the point query).
    pub fn born_value(&self, id: &CovenantId) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT COALESCE(SUM(u.value), 0)
                 FROM covenant_utxos u
                 JOIN covenants c ON c.covenant_id = u.covenant_id AND u.created_daa = c.genesis_daa
                 WHERE u.covenant_id = ?1",
                [id.0.as_slice()],
                |r| r.get(0),
            )
            .map_err(db_err)
    }

    /// "What runs on this network": per-template aggregates in one GROUP BY.
    /// Recognition is stamped at write time, so this never decodes a script.
    /// NULL rows (written by an older binary, healed at the next open) fold
    /// into the unrecognized bucket — honest degradation under version skew.
    pub fn template_stats(&self) -> Result<Vec<TemplateStat>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT COALESCE(template, '') AS tpl,
                        COALESCE(SUM(CASE WHEN spent_block IS NULL THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN spent_block IS NULL THEN value ELSE 0 END), 0),
                        COUNT(*),
                        COUNT(DISTINCT covenant_id)
                 FROM covenant_utxos GROUP BY tpl ORDER BY COUNT(*) DESC, tpl",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                let tpl: String = row.get(0)?;
                Ok(TemplateStat {
                    template: (!tpl.is_empty()).then_some(tpl),
                    live_states: row.get(1)?,
                    live_value: row.get(2)?,
                    ever_seen: row.get(3)?,
                    covenants: row.get(4)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// How many spends ran each recognized template — verified P2SH reveals
    /// only. Compiled contracts (Mecenas, Escrow, LastWill) live behind p2sh
    /// commitments and surface exclusively here; a tx sweeping N states
    /// counts N.
    pub fn revealed_template_counts(&self) -> Result<Vec<(String, u64)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT revealed_template, COUNT(*) FROM covenant_utxos
                 WHERE revealed_template IS NOT NULL AND revealed_template != ''
                 GROUP BY revealed_template ORDER BY COUNT(*) DESC, revealed_template",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// The chain block that accepted this transaction, per the index.
    pub fn accepting_block_of(&self, txid: &TxId) -> Result<Option<BlockHash>> {
        let row = self
            .conn
            .query_row(
                "SELECT accepting_block FROM covenant_events WHERE txid = ?1 LIMIT 1",
                [txid.0.as_slice()],
                |r| Ok(BlockHash(r.get(0)?)),
            )
            .optional()
            .map_err(db_err)?;
        Ok(row)
    }

    /// Which covenant owns this state outpoint, if we track it.
    pub fn utxo_covenant(&self, outpoint: &Outpoint) -> Result<Option<CovenantId>> {
        let row = self
            .conn
            .query_row(
                "SELECT covenant_id FROM covenant_utxos WHERE txid = ?1 AND output_index = ?2",
                params![outpoint.txid.0.as_slice(), outpoint.index],
                |r| Ok(CovenantId(r.get(0)?)),
            )
            .optional()
            .map_err(db_err)?;
        Ok(row)
    }

    /// Every covenant this transaction touched — multi-covenant transactions
    /// (one tx moving several coins) are first-class post-Toccata.
    pub fn covenants_by_txid(&self, txid: &TxId) -> Result<Vec<CovenantId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT covenant_id FROM covenant_events WHERE txid = ?1")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([txid.0.as_slice()], |r| Ok(CovenantId(r.get(0)?)))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Recognized template per covenant — the most specific (non-p2pk/p2sh)
    /// name wins so a SilverScript coin is labeled by its contract, not by
    /// the generic shape of its commitment.
    pub fn covenant_templates(&self) -> Result<std::collections::HashMap<CovenantId, String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT covenant_id,
                        MAX(CASE WHEN revealed_template IS NOT NULL AND revealed_template <> '' AND revealed_template NOT LIKE 'p2%' THEN revealed_template
                                 WHEN template NOT LIKE 'p2%' THEN template END),
                        MAX(COALESCE(NULLIF(revealed_template, ''), template))
                 FROM covenant_utxos WHERE (template IS NOT NULL AND template <> '') OR (revealed_template IS NOT NULL AND revealed_template <> '')
                 GROUP BY covenant_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                let named: Option<String> = r.get(1)?;
                let any: Option<String> = r.get(2)?;
                Ok((CovenantId(r.get(0)?), named.or(any)))
            })
            .map_err(db_err)?
            .filter_map(|row| match row {
                Ok((id, Some(t))) => Some(Ok((id, t))),
                Ok((_, None)) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<std::result::Result<std::collections::HashMap<_, _>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Which covenant did this transaction touch? Covers genesis, transitions,
    /// and burns (their txids are all event txids).
    pub fn covenant_by_txid(&self, txid: &TxId) -> Result<Option<CovenantId>> {
        let row = self
            .conn
            .query_row(
                "SELECT covenant_id FROM covenant_events WHERE txid = ?1 LIMIT 1",
                [txid.0.as_slice()],
                |r| Ok(CovenantId(r.get(0)?)),
            )
            .optional()
            .map_err(db_err)?;
        Ok(row)
    }

    /// Covenants whose p2pk state has carried this owner pubkey (32-byte x-only
    /// or 33-byte ECDSA). Matches the state script byte-exactly — the same shape
    /// P2pkStateDecoder recognizes: [len-2 push opcode][key][OpCheckSig].
    /// Full scan of covenant_utxos: spk_script has no index; exact-equality is a
    /// cheap memcmp and fine at current row counts. If it ever measures hot, the
    /// additive lever is CREATE INDEX IF NOT EXISTS utxo_by_spk ON
    /// covenant_utxos(spk_script).
    pub fn covenants_by_pubkey(&self, pubkey: &[u8]) -> Result<Vec<PubkeyCovenantRow>> {
        if !matches!(pubkey.len(), 32 | 33) {
            return Ok(vec![]);
        }
        let mut expected = Vec::with_capacity(pubkey.len() + 2);
        expected.push(pubkey.len() as u8); // 0x20 or 0x21
        expected.extend_from_slice(pubkey);
        expected.push(0xac); //               OpCheckSig
        let mut stmt = self
            .conn
            .prepare(
                "SELECT covenant_id,
                        MAX(spent_block IS NULL) AS controls_now,
                        COUNT(*) AS states_seen,
                        MIN(created_daa) AS first_seen_daa,
                        MAX(created_daa) AS last_seen_daa
                 FROM covenant_utxos
                 WHERE spk_script = ?1
                 GROUP BY covenant_id
                 ORDER BY last_seen_daa DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([expected.as_slice()], |row| {
                Ok(PubkeyCovenantRow {
                    covenant_id: CovenantId(row.get(0)?),
                    controls_now: row.get(1)?,
                    states_seen: row.get(2)?,
                    first_seen_daa: row.get(3)?,
                    last_seen_daa: row.get(4)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    pub fn events(&self, id: &CovenantId) -> Result<Vec<EventRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT seq, kind, txid, accepting_block, accepting_daa, payload
                 FROM covenant_events WHERE covenant_id = ?1 ORDER BY seq",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([id.0.as_slice()], |row| {
                Ok(EventRow {
                    seq: row.get(0)?,
                    kind: row.get(1)?,
                    txid: TxId(row.get(2)?),
                    accepting_block: BlockHash(row.get(3)?),
                    accepting_daa: row.get(4)?,
                    payload: row.get(5)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// The newest events across all covenants, newest first.
    pub fn recent_events(&self, limit: u64) -> Result<Vec<GlobalEventRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT covenant_id, seq, kind, txid, accepting_daa
                 FROM covenant_events ORDER BY accepting_daa DESC, rowid DESC LIMIT ?1",
            )
            .map_err(db_err)?;
        let limit = limit.min(i64::MAX as u64) as i64;
        let rows = stmt
            .query_map([limit], |row| {
                Ok(GlobalEventRow {
                    covenant_id: CovenantId(row.get(0)?),
                    seq: row.get(1)?,
                    kind: row.get(2)?,
                    txid: TxId(row.get(3)?),
                    accepting_daa: row.get(4)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    pub fn utxos(&self, id: &CovenantId, live_only: bool) -> Result<Vec<UtxoRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT txid, output_index, value, spk_version, spk_script, created_daa,
                        spent_block IS NULL, spent_txid, spent_sig, spent_budget
                 FROM covenant_utxos WHERE covenant_id = ?1 AND (?2 = 0 OR spent_block IS NULL)
                 ORDER BY created_daa",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![id.0.as_slice(), live_only as i64], |row| {
                Ok(UtxoRow {
                    outpoint: Outpoint { txid: TxId(row.get(0)?), index: row.get(1)? },
                    value: row.get(2)?,
                    spk_version: row.get(3)?,
                    spk_script: row.get(4)?,
                    created_daa: row.get(5)?,
                    live: row.get(6)?,
                    spent_txid: row.get::<_, Option<[u8; 32]>>(7)?.map(TxId),
                    spent_sig: row.get(8)?,
                    spent_budget: row.get(9)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store(name: &str) -> Store {
        let path = std::env::temp_dir()
            .join(format!("kascov-store-test-{}-{name}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Store::open(&path, Network::Testnet(10)).unwrap()
    }

    fn block_with_events(hash: u8, daa: u64, events: Vec<(u8, EventKind, u8)>) -> BlockEvents {
        BlockEvents {
            accepting_block: BlockHash([hash; 32]),
            accepting_daa: daa,
            events: events
                .into_iter()
                .map(|(cov, kind, tx)| NewEvent {
                    covenant_id: CovenantId([cov; 32]),
                    kind,
                    txid: TxId([tx; 32]),
                    payload: None,
                })
                .collect(),
            created_utxos: vec![],
            spent_utxos: vec![],
        }
    }

    #[test]
    fn tip_roundtrip_and_overwrite() {
        let store = test_store("tip");
        assert_eq!(store.tip().unwrap(), None);
        store.set_tip(123, 456_000).unwrap();
        assert_eq!(store.tip().unwrap(), Some((123, 456_000)));
        store.set_tip(999, 999_000).unwrap();
        assert_eq!(store.tip().unwrap(), Some((999, 999_000)));
    }

    #[test]
    fn processed_daa_tracks_applies_and_skips_empty() {
        let mut store = test_store("processed");
        assert_eq!(store.processed_daa().unwrap(), None);
        store
            .apply(&block_with_events(1, 100, vec![(0xA1, EventKind::Genesis, 0x01)]), BlockHash([1; 32]))
            .unwrap();
        assert_eq!(store.processed_daa().unwrap(), Some(100));
        // reset_cursor-style empty batch (accepting_daa = 0) must not touch it
        store.reset_cursor(BlockHash([9; 32])).unwrap();
        assert_eq!(store.processed_daa().unwrap(), Some(100));
        // an event-less checkpoint carrying a DAA still advances it
        let mut checkpoint = BlockEvents::empty(BlockHash([2; 32]));
        checkpoint.accepting_daa = 250;
        store.apply(&checkpoint, BlockHash([2; 32])).unwrap();
        assert_eq!(store.processed_daa().unwrap(), Some(250));
    }

    #[test]
    fn recent_events_orders_newest_first_and_limits() {
        let mut store = test_store("recent");
        store
            .apply(&block_with_events(1, 100, vec![(0xA1, EventKind::Genesis, 0x01)]), BlockHash([1; 32]))
            .unwrap();
        store
            .apply(
                &block_with_events(
                    2,
                    200,
                    vec![(0xA1, EventKind::Transition, 0x02), (0xB2, EventKind::Genesis, 0x03)],
                ),
                BlockHash([2; 32]),
            )
            .unwrap();

        let recent = store.recent_events(10).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].accepting_daa, 200);
        // same DAA: later insertion (rowid) first
        assert_eq!(recent[0].covenant_id, CovenantId([0xB2; 32]));
        assert_eq!(recent[0].kind, "genesis");
        assert_eq!(recent[1].covenant_id, CovenantId([0xA1; 32]));
        assert_eq!(recent[2].accepting_daa, 100);
        assert_eq!(recent[2].seq, 0);

        let capped = store.recent_events(1).unwrap();
        assert_eq!(capped.len(), 1);
        assert_eq!(capped[0].accepting_daa, 200);
    }

    #[test]
    fn digest_windows_and_headliners() {
        // fresh empty store: tip fallback path — all zeros, no headliners
        let empty = test_store("digest-empty");
        let d0 = empty.digest(864_000).unwrap();
        assert_eq!((d0.births, d0.moves, d0.burns), (0, 0, 0));
        assert_eq!((d0.value_born, d0.active_now), (0, 0));
        assert_eq!(d0.busiest, None);
        assert_eq!(d0.biggest_birth, None);

        let mut store = test_store("digest");
        // old genesis — outside the window once the tip is set
        store
            .apply(&block_with_events(1, 1_000, vec![(0xA1, EventKind::Genesis, 0x01)]), BlockHash([1; 32]))
            .unwrap();
        // inside the window: 0xB2 born holding 50 KAS + two moves, 0xA1 retires
        let mut b2 = block_with_events(
            2,
            999_000,
            vec![
                (0xB2, EventKind::Genesis, 0x03),
                (0xB2, EventKind::Transition, 0x04),
                (0xB2, EventKind::Transition, 0x05),
                (0xA1, EventKind::Burn, 0x06),
            ],
        );
        b2.created_utxos = vec![NewUtxo {
            outpoint: Outpoint { txid: TxId([0x03; 32]), index: 0 },
            covenant_id: CovenantId([0xB2; 32]),
            value: 5_000_000_000,
            spk_version: 1,
            spk_script: vec![0xac],
        }];
        store.apply(&b2, BlockHash([2; 32])).unwrap();
        store.set_tip(1_000_000, 1_751_000_000_000).unwrap();

        // cutoff = 1_000_000 - 864_000 = 136_000: the daa-1000 genesis drops out
        let d = store.digest(864_000).unwrap();
        assert_eq!((d.births, d.moves, d.burns), (1, 2, 1));
        assert_eq!(d.value_born, 5_000_000_000);
        assert_eq!(d.active_now, 1);
        assert_eq!(d.busiest, Some((CovenantId([0xB2; 32]), 3)));
        assert_eq!(d.biggest_birth, Some((CovenantId([0xB2; 32]), 5_000_000_000)));
    }

    #[test]
    fn activity_buckets_and_bounds() {
        // empty store: no bounds, no buckets
        let empty = test_store("activity-empty");
        assert_eq!(empty.event_daa_bounds().unwrap(), None);
        assert!(empty.activity(14_400, 0).unwrap().is_empty());

        let mut store = test_store("activity");
        store
            .apply(&block_with_events(1, 1_000, vec![(0xA1, EventKind::Genesis, 0x01)]), BlockHash([1; 32]))
            .unwrap();
        store
            .apply(
                &block_with_events(
                    2,
                    999_000,
                    vec![
                        (0xB2, EventKind::Genesis, 0x03),
                        (0xB2, EventKind::Transition, 0x04),
                        (0xB2, EventKind::Transition, 0x05),
                        (0xA1, EventKind::Burn, 0x06),
                    ],
                ),
                BlockHash([2; 32]),
            )
            .unwrap();

        assert_eq!(store.event_daa_bounds().unwrap(), Some((1_000, 999_000)));

        // 24h-range width: daa 1_000 → bucket 0, daa 999_000 → bucket 69 (993_600)
        let rows = store.activity(14_400, 0).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].daa, rows[0].births, rows[0].moves, rows[0].burns), (0, 1, 0, 0));
        assert_eq!(
            (rows[1].daa, rows[1].births, rows[1].moves, rows[1].burns),
            (69 * 14_400, 1, 2, 1)
        );

        // a cutoff at the newest bucket edge drops the old genesis
        let tail = store.activity(14_400, 993_600).unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!((tail[0].daa, tail[0].births, tail[0].moves, tail[0].burns), (993_600, 1, 2, 1));
    }

    #[test]
    fn covenants_by_pubkey_matches_exact_p2pk_states() {
        let mut store = test_store("pubkey");
        let key_a = [0xaa_u8; 32];
        let key_b = [0xbb_u8; 33];
        let p2pk = |key: &[u8]| {
            let mut s = vec![key.len() as u8];
            s.extend_from_slice(key);
            s.push(0xac);
            s
        };
        // decoy: keyA embedded at offset 1 but the tail isn't OpCheckSig
        let mut decoy = vec![0x20];
        decoy.extend_from_slice(&key_a);
        decoy.push(0x00);
        let utxo = |tx: u8, cov: u8, script: Vec<u8>| NewUtxo {
            outpoint: Outpoint { txid: TxId([tx; 32]), index: 0 },
            covenant_id: CovenantId([cov; 32]),
            value: 1_000,
            spk_version: 1,
            spk_script: script,
        };

        let mut b1 = BlockEvents::empty(BlockHash([1; 32]));
        b1.accepting_daa = 100;
        b1.created_utxos = vec![
            utxo(0x01, 0xA1, p2pk(&key_a)), // keyA state #1 (spent below)
            utxo(0x02, 0xB2, p2pk(&key_b)), // keyB (33-byte ECDSA) state
            utxo(0x03, 0xC3, decoy),        // keyA bytes under the wrong opcode
            utxo(0x05, 0xD4, p2pk(&key_a)), // keyA's only state here (spent below)
        ];
        store.apply(&b1, BlockHash([1; 32])).unwrap();

        let mut b2 = BlockEvents::empty(BlockHash([2; 32]));
        b2.accepting_daa = 200;
        b2.created_utxos = vec![utxo(0x04, 0xA1, p2pk(&key_a))]; // keyA state #2, live
        b2.spent_utxos = vec![
            (Outpoint { txid: TxId([0x01; 32]), index: 0 }, TxId([0x04; 32]), vec![], 0),
            (Outpoint { txid: TxId([0x05; 32]), index: 0 }, TxId([0x06; 32]), vec![], 0),
        ];
        store.apply(&b2, BlockHash([2; 32])).unwrap();

        // keyA: current owner of 0xA1 (one live, one spent state), past owner of 0xD4
        let rows = store.covenants_by_pubkey(&key_a).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].covenant_id, CovenantId([0xA1; 32]));
        assert!(rows[0].controls_now);
        assert_eq!(rows[0].states_seen, 2);
        assert_eq!(rows[0].first_seen_daa, 100);
        assert_eq!(rows[0].last_seen_daa, 200);
        assert_eq!(rows[1].covenant_id, CovenantId([0xD4; 32]));
        assert!(!rows[1].controls_now);
        assert_eq!(rows[1].states_seen, 1);

        let rows_b = store.covenants_by_pubkey(&key_b).unwrap();
        assert_eq!(rows_b.len(), 1);
        assert_eq!(rows_b[0].covenant_id, CovenantId([0xB2; 32]));
        assert!(rows_b[0].controls_now);
        assert_eq!(rows_b[0].states_seen, 1);

        // unmatched and wrong-length keys answer honestly empty
        assert!(store.covenants_by_pubkey(&[0xcc; 32]).unwrap().is_empty());
        assert!(store.covenants_by_pubkey(&[0xaa; 31]).unwrap().is_empty());
    }

    #[test]
    fn template_stats_recognize_and_bucket() {
        let mut store = test_store("templates");
        let mut p2pk = vec![0x20];
        p2pk.extend([0x7f; 32]);
        p2pk.push(0xac);
        let junk = vec![0x51, 0x51]; // OpTrue OpTrue — matches no template
        // p2sh commitment over a redeem that is itself template-less
        let redeem = vec![0xb9, 0xcf, 0x51]; // OpTxInputIndex OpInputCovenantId OpTrue
        let digest = blake2b_simd::Params::new().hash_length(32).hash(&redeem);
        let mut p2sh = vec![0xaa, 0x20];
        p2sh.extend_from_slice(digest.as_bytes());
        p2sh.push(0x87);
        let utxo = |tx: u8, cov: u8, script: Vec<u8>| NewUtxo {
            outpoint: Outpoint { txid: TxId([tx; 32]), index: 0 },
            covenant_id: CovenantId([cov; 32]),
            value: 1_000,
            spk_version: 1,
            spk_script: script,
        };

        let mut b1 = BlockEvents::empty(BlockHash([1; 32]));
        b1.accepting_daa = 100;
        b1.created_utxos =
            vec![utxo(0x01, 0xA1, p2pk), utxo(0x02, 0xB2, junk), utxo(0x03, 0xC3, p2sh)];
        store.apply(&b1, BlockHash([1; 32])).unwrap();

        let by_name = |stats: &[TemplateStat], name: Option<&str>| {
            stats.iter().find(|s| s.template.as_deref() == name).cloned().unwrap()
        };
        let stats = store.template_stats().unwrap();
        assert_eq!(stats.len(), 3); // p2pk state, p2sh commitment, unrecognized
        let p2pk_row = by_name(&stats, Some("p2pk state"));
        assert_eq!((p2pk_row.live_states, p2pk_row.ever_seen, p2pk_row.covenants), (1, 1, 1));
        assert_eq!(p2pk_row.live_value, 1_000);
        let unrec = by_name(&stats, None); // '' sentinel: decoded, nothing matched
        assert_eq!((unrec.live_states, unrec.ever_seen, unrec.covenants), (1, 1, 1));
        assert!(store.revealed_template_counts().unwrap().is_empty());

        // spend the p2sh state, revealing its (template-less) program
        let mut sig = vec![0x03];
        sig.extend_from_slice(&redeem);
        let mut b2 = BlockEvents::empty(BlockHash([2; 32]));
        b2.accepting_daa = 200;
        b2.spent_utxos =
            vec![(Outpoint { txid: TxId([0x03; 32]), index: 0 }, TxId([0x04; 32]), sig, 0)];
        store.apply(&b2, BlockHash([2; 32])).unwrap();

        let stats = store.template_stats().unwrap();
        let p2sh_row = by_name(&stats, Some("p2sh commitment"));
        assert_eq!((p2sh_row.live_states, p2sh_row.live_value), (0, 0)); // spent…
        assert_eq!((p2sh_row.ever_seen, p2sh_row.covenants), (1, 1)); // …but remembered
        // the reveal ran but matched no template — '' is stored, not counted
        assert!(store.revealed_template_counts().unwrap().is_empty());
        let revealed: Option<String> = store
            .conn
            .query_row(
                "SELECT revealed_template FROM covenant_utxos WHERE txid = ?1",
                [[0x03u8; 32].as_slice()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(revealed.as_deref(), Some(""));

        // a reveal that IS a recognized shape gets named and counted: commit
        // to a p2pk-shaped redeem, then spend it
        let redeem2: Vec<u8> = {
            let mut s = vec![0x20];
            s.extend([0x11; 32]);
            s.push(0xac);
            s
        };
        let digest2 = blake2b_simd::Params::new().hash_length(32).hash(&redeem2);
        let mut p2sh2 = vec![0xaa, 0x20];
        p2sh2.extend_from_slice(digest2.as_bytes());
        p2sh2.push(0x87);
        let mut sig2 = vec![redeem2.len() as u8];
        sig2.extend_from_slice(&redeem2);
        let mut b3 = BlockEvents::empty(BlockHash([3; 32]));
        b3.accepting_daa = 300;
        b3.created_utxos = vec![utxo(0x05, 0xC3, p2sh2)];
        b3.spent_utxos =
            vec![(Outpoint { txid: TxId([0x05; 32]), index: 0 }, TxId([0x06; 32]), sig2, 0)];
        store.apply(&b3, BlockHash([3; 32])).unwrap();
        assert_eq!(
            store.revealed_template_counts().unwrap(),
            vec![("p2pk state".to_string(), 1)]
        );
    }
}
