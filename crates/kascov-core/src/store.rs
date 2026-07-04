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

/// An event joined with its covenant, for cross-covenant feeds.
#[derive(Clone, Debug, Serialize)]
pub struct GlobalEventRow {
    pub covenant_id: CovenantId,
    pub seq: u64,
    pub kind: String,
    pub txid: TxId,
    pub accepting_daa: u64,
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

        let store = Self { conn };
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
            tx.execute(
                "INSERT OR REPLACE INTO covenant_utxos
                 (txid, output_index, covenant_id, value, spk_version, spk_script,
                  created_block, created_daa, spent_block, spent_txid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL)",
                params![
                    utxo.outpoint.txid.0.as_slice(),
                    utxo.outpoint.index,
                    utxo.covenant_id.0.as_slice(),
                    utxo.value,
                    utxo.spk_version,
                    utxo.spk_script,
                    block.accepting_block.0.as_slice(),
                    block.accepting_daa
                ],
            )
            .map_err(db_err)?;
        }
        for (outpoint, spending_txid, sig, budget) in &block.spent_utxos {
            tx.execute(
                "UPDATE covenant_utxos SET spent_block = ?1, spent_txid = ?2, spent_sig = ?3, spent_budget = ?4
                 WHERE txid = ?5 AND output_index = ?6",
                params![
                    block.accepting_block.0.as_slice(),
                    spending_txid.0.as_slice(),
                    sig,
                    budget,
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
        tx.commit().map_err(db_err)
    }

    /// Undo everything attributed to the given (reorged-out) chain blocks.
    pub fn rollback(&mut self, removed: &[BlockHash]) -> Result<()> {
        let tx = self.conn.transaction().map_err(db_err)?;
        for hash in removed {
            let hash = hash.0.as_slice();
            tx.execute(
                "UPDATE covenant_utxos SET spent_block = NULL, spent_txid = NULL, spent_sig = NULL, spent_budget = NULL WHERE spent_block = ?1",
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
}
