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
    -- KIP-21 lane namespace: the 4-byte app tag (hex) of a payload shaped as
    -- <4-byte namespace><16 zero bytes>… — NULL when the payload isn't a lane.
    lane_namespace TEXT,
    -- Precomputed payload classification (write-time stamps, backfilled on
    -- open): payload_tag is 'json' / 'jsonhex' / 'tag:<8 hex>' ('' when the
    -- payload is shorter than 4 bytes); inscription_kind is the decoded
    -- inscription label ('' when the payload isn't a parseable inscription).
    -- Both are NULL only when payload is NULL or the row predates the stamp.
    payload_tag TEXT,
    inscription_kind TEXT,
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
-- community-verified source: a compiled program proven byte-identical to
-- submitted SilverScript source (verify-and-publish).
CREATE TABLE IF NOT EXISTS verified_sources (
    program_hash TEXT PRIMARY KEY,
    program_hex TEXT NOT NULL,
    source TEXT NOT NULL,
    args TEXT NOT NULL,
    template TEXT,
    verified_at INTEGER NOT NULL
);
-- covenant event alerting: POST a webhook when a matching event fires.
CREATE TABLE IF NOT EXISTS webhook_subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    covenant_id BLOB,
    kind TEXT,
    url TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS webhook_by_covenant ON webhook_subscriptions(covenant_id);
-- an append-only ledger of virtual-chain reorgs the indexer has applied. Each
-- row is one rollback: the DAA we had reached, when it happened (ms), and how
-- many chain blocks were undone.
CREATE TABLE IF NOT EXISTS reorg_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    daa INTEGER NOT NULL,
    at_ms INTEGER NOT NULL,
    rolled_back INTEGER NOT NULL
);
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
    /// Sum of state outputs created at the genesis DAA — same definition as
    /// `born_value()`/`born_values()` (folded into the row query so grid
    /// builders don't need a separate full-table pass).
    pub born_value: u64,
    /// Recognized template, `covenant_templates()` pick rule: the most
    /// specific (non-p2pk/p2sh) revealed or state template wins, else any.
    pub template: Option<String>,
}

/// Shared SELECT for `CovenantSummary` rows (`list`/`list_page`/`summary`).
/// Every correlated subselect probes `utxo_by_covenant`, so cost stays
/// O(states-of-covenant) per row. The born-value subselect mirrors
/// `born_value()` exactly (outputs created at the genesis DAA; NULL
/// genesis_daa matches nothing → 0). The template COALESCE mirrors
/// `covenant_templates()` exactly: prefer a non-p2* revealed_template, then a
/// non-p2* state template, else any template at all — over the same
/// has-any-template row filter.
const SUMMARY_SELECT: &str = "SELECT c.covenant_id, c.genesis_txid, c.genesis_daa, c.lineage_complete,
        c.event_count, c.last_activity_daa,
        (SELECT COUNT(*) FROM covenant_utxos u WHERE u.covenant_id = c.covenant_id AND u.spent_block IS NULL),
        (SELECT COALESCE(SUM(value), 0) FROM covenant_utxos u WHERE u.covenant_id = c.covenant_id AND u.spent_block IS NULL),
        (SELECT COALESCE(SUM(u.value), 0) FROM covenant_utxos u WHERE u.covenant_id = c.covenant_id AND u.created_daa = c.genesis_daa),
        COALESCE(
          (SELECT MAX(CASE WHEN u.revealed_template IS NOT NULL AND u.revealed_template <> '' AND u.revealed_template NOT LIKE 'p2%' THEN u.revealed_template
                           WHEN u.template NOT LIKE 'p2%' THEN u.template END)
             FROM covenant_utxos u
             WHERE u.covenant_id = c.covenant_id
               AND ((u.template IS NOT NULL AND u.template <> '') OR (u.revealed_template IS NOT NULL AND u.revealed_template <> ''))),
          (SELECT MAX(COALESCE(NULLIF(u.revealed_template, ''), u.template))
             FROM covenant_utxos u
             WHERE u.covenant_id = c.covenant_id
               AND ((u.template IS NOT NULL AND u.template <> '') OR (u.revealed_template IS NOT NULL AND u.revealed_template <> ''))))
 FROM covenants c";

fn map_summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CovenantSummary> {
    Ok(CovenantSummary {
        covenant_id: CovenantId(row.get(0)?),
        genesis_txid: row.get::<_, Option<[u8; 32]>>(1)?.map(TxId),
        genesis_daa: row.get(2)?,
        lineage_complete: row.get(3)?,
        event_count: row.get(4)?,
        last_activity_daa: row.get(5)?,
        live_utxos: row.get(6)?,
        live_value: row.get(7)?,
        born_value: row.get(8)?,
        template: row.get(9)?,
    })
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

/// A pubkey that has owned a p2pk-shaped state UTXO of one covenant — the
/// inverse of `covenants_by_pubkey`, scoped to a single coin's holders.
#[derive(Clone, Debug, Serialize)]
pub struct HolderRow {
    /// Owner pubkey (32-byte x-only or 33-byte ECDSA), hex-encoded.
    pub pubkey: String,
    /// The key currently owns at least one live state UTXO of this covenant.
    pub controls_now: bool,
    /// How many state UTXOs (live + spent) have carried this key.
    pub states_seen: u64,
    pub first_seen_daa: u64,
    pub last_seen_daa: u64,
}

/// One applied virtual-chain reorg: the DAA the indexer had reached, the
/// wall-clock instant it was undone (ms since epoch), and how many chain
/// blocks were rolled back.
#[derive(Clone, Debug, Serialize)]
pub struct ReorgRow {
    pub daa: u64,
    pub at_ms: u64,
    pub rolled_back: u64,
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

/// A state UTXO some transaction spent, with the captured witness — what the
/// real-spend debugger replays through the script engine.
#[derive(Clone, Debug, Serialize)]
pub struct SpentStateRow {
    pub covenant_id: CovenantId,
    pub outpoint: Outpoint,
    pub value: u64,
    pub spk_version: u16,
    #[serde(serialize_with = "crate::detect::hex_ser")]
    pub spk_script: Vec<u8>,
    /// The spend's unlocking script, when captured.
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
    /// The KIP-21 lane namespace (4-byte app tag, hex) when the payload has the
    /// lane shape; NULL otherwise. Derive with [`lane_namespace`].
    pub lane_namespace: Option<String>,
}

/// Sniff a KIP-21 user-lane namespace out of a v1 tx payload. The lane shape is
/// a leading 4-byte app namespace followed by 16 zero bytes (mirrors the same
/// probe the `inspect tx` tool prints). Returns the namespace as lowercase hex,
/// or `None` when the payload is too short or isn't lane-shaped.
pub fn lane_namespace(payload: &[u8]) -> Option<String> {
    if payload.len() >= 20 && payload[4..20].iter().all(|&b| b == 0) {
        Some(hex::encode(&payload[..4]))
    } else {
        None
    }
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

/// Milliseconds since the Unix epoch (wall clock). Used to timestamp reorg-log
/// rows; a backwards clock only yields a smaller number, never a panic.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Parse an inscription payload's first JSON value — raw `{"…`, or ASCII-hex-
/// encoded — tolerating trailing binary after the object.
fn extract_inscription_json(payload: &[u8]) -> Option<serde_json::Value> {
    let first = |bytes: &[u8]| {
        serde_json::Deserializer::from_slice(bytes)
            .into_iter::<serde_json::Value>()
            .next()
            .and_then(|r| r.ok())
    };
    if payload.starts_with(b"{\"") {
        return first(payload);
    }
    if payload.starts_with(b"7b22") {
        let run: Vec<u8> = payload.iter().copied().take_while(|b| b.is_ascii_hexdigit()).collect();
        let n = run.len() & !1;
        if let Ok(dec) = hex::decode(&run[..n]) {
            return first(&dec);
        }
    }
    None
}

/// A short human label for what an inscription is: KRC-20-style protocol/op/
/// tick when present, else the `t`/tick/top-level type.
fn inscription_kind(v: &serde_json::Value) -> String {
    let obj = v.as_object();
    let get = |k: &str| obj.and_then(|o| o.get(k)).and_then(|x| x.as_str());
    let clip = |s: &str| s.chars().take(24).collect::<String>();
    let label = if let Some(p) = get("p") {
        let mut s = clip(p);
        if let Some(op) = get("op") {
            s.push_str(" · ");
            s.push_str(&clip(op));
        }
        if let Some(tick) = get("tick") {
            s.push_str(" · ");
            s.push_str(&clip(tick));
        }
        s
    } else if let Some(t) = get("t") {
        clip(t)
    } else if let Some(tick) = get("tick") {
        format!("token · {}", clip(tick))
    } else if let Some((k, _)) = obj.and_then(|o| o.iter().next()) {
        clip(k)
    } else {
        "JSON".into()
    };
    // keep it printable
    label.chars().filter(|c| !c.is_control()).collect()
}

/// Classify a payload for the based-app tag buckets — the exact Rust port of
/// the CASE the legacy `based_app_namespaces` scan computed per row:
/// `json` for raw `{"…`, `jsonhex` for ASCII-hex `7b22…`, else `tag:<hex>` of
/// the leading 4 bytes. Payloads shorter than 4 bytes stamp `''` (the legacy
/// query's `length(payload) >= 4` filter excluded them).
fn payload_tag(payload: &[u8]) -> String {
    if payload.len() < 4 {
        return String::new();
    }
    if payload.starts_with(b"{\"") {
        "json".into()
    } else if payload.starts_with(b"7b22") {
        "jsonhex".into()
    } else {
        format!("tag:{}", hex::encode(&payload[..4]))
    }
}

/// Decode a payload's inscription label for the precomputed stamp — the same
/// first-512-bytes window + parse the legacy `inscription_breakdown` scan
/// used per row. `''` when the payload isn't a parseable inscription.
fn inscription_kind_of(payload: &[u8]) -> String {
    let head = &payload[..payload.len().min(512)];
    extract_inscription_json(head).map(|v| inscription_kind(&v)).unwrap_or_default()
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
        // Only ignore SQLITE_ERROR (1) with "duplicate column" — re-raise
        // genuine failures like disk-full, I/O errors, or database corruption.
        let migrations = [
            "ALTER TABLE covenant_utxos ADD COLUMN spent_sig BLOB",
            "ALTER TABLE covenant_utxos ADD COLUMN spent_budget INTEGER",
            "ALTER TABLE covenant_events ADD COLUMN payload BLOB",
            "ALTER TABLE covenant_events ADD COLUMN lane_namespace TEXT",
            "ALTER TABLE covenant_utxos ADD COLUMN template TEXT",
            "ALTER TABLE covenant_utxos ADD COLUMN revealed_template TEXT",
            "ALTER TABLE covenant_events ADD COLUMN payload_tag TEXT",
            "ALTER TABLE covenant_events ADD COLUMN inscription_kind TEXT",
        ];
        for sql in &migrations {
            if let Err(e) = conn.execute(sql, []) {
                match &e {
                    rusqlite::Error::SqliteFailure(err, _)
                        if err.code == rusqlite::ErrorCode::Unknown =>
                    {
                        // SQLITE_ERROR — likely "duplicate column name"; skip.
                    }
                    _ => return Err(db_err(e)),
                }
            }
        }
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
        // Payload-tag backfill todo (payload_tag and inscription_kind are
        // always stamped together — insert path and backfill both set the
        // pair — so one probe covers both columns).
        conn.execute(
            "CREATE INDEX IF NOT EXISTS ev_payload_tag_todo ON covenant_events(payload_tag) WHERE payload IS NOT NULL AND payload_tag IS NULL",
            [],
        )
        .map_err(db_err)?;
        // Covering partial indexes so the lanes/inscriptions analytics are
        // pure index-order GROUP BYs instead of full event-table scans. Their
        // predicates must match the queries in based_app_namespaces /
        // inscription_breakdown verbatim.
        conn.execute(
            "CREATE INDEX IF NOT EXISTS ev_tag_stats ON covenant_events(payload_tag, covenant_id) WHERE lane_namespace IS NULL AND payload_tag IS NOT NULL AND payload_tag <> ''",
            [],
        )
        .map_err(db_err)?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS ev_inscription_stats ON covenant_events(inscription_kind, covenant_id) WHERE inscription_kind IS NOT NULL AND inscription_kind <> ''",
            [],
        )
        .map_err(db_err)?;
        // The grid orders by recency: without this index every page (and every
        // 20s snapshot rebuild) full-scans + temp-sorts the covenants table —
        // measured at ~6s per request at 168k covenants on the live worker.
        // The compound key also serves list_page's (daa, id) cursor seek.
        conn.execute(
            "CREATE INDEX IF NOT EXISTS cov_by_activity ON covenants(last_activity_daa DESC, covenant_id DESC)",
            [],
        )
        .map_err(db_err)?;
        // Per-lane dashboards: recent events + activity buckets for one
        // namespace are index-order walks instead of event-table scans. The
        // partial predicate keeps it tiny (lanes are rare next to events).
        conn.execute(
            "CREATE INDEX IF NOT EXISTS ev_by_lane ON covenant_events(lane_namespace, accepting_daa) WHERE lane_namespace IS NOT NULL",
            [],
        )
        .map_err(db_err)?;
        // The real-spend debugger looks up state UTXOs by the txid that spent
        // them — without this, every /debug/<txid> is a full utxo-table scan.
        conn.execute(
            "CREATE INDEX IF NOT EXISTS utxo_by_spent_txid ON covenant_utxos(spent_txid) WHERE spent_txid IS NOT NULL",
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
        store.backfill_payload_tags()?;
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

    /// Stamp payload_tag + inscription_kind onto event rows that predate the
    /// columns: one-shot after a migration, an O(1) probe against the empty
    /// ev_payload_tag_todo partial index on every open after that. Both
    /// columns are stamped together (see the todo index comment). Only the
    /// first 512 payload bytes are fetched — the tag needs 4 and the
    /// inscription decode always used the same 512-byte window.
    fn backfill_payload_tags(&mut self) -> Result<()> {
        const BATCH: i64 = 5000;
        let mut stamped = 0u64;
        loop {
            let rows: Vec<(i64, Vec<u8>)> = {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT rowid, substr(payload, 1, 512) FROM covenant_events
                         WHERE payload IS NOT NULL AND payload_tag IS NULL LIMIT ?1",
                    )
                    .map_err(db_err)?;
                let collected = stmt
                    .query_map([BATCH], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(db_err)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(db_err)?;
                collected
            };
            if rows.is_empty() {
                break;
            }
            let tx = self.conn.transaction().map_err(db_err)?;
            for (rowid, head) in &rows {
                tx.execute(
                    "UPDATE covenant_events SET payload_tag = ?1, inscription_kind = ?2 WHERE rowid = ?3",
                    params![payload_tag(head), inscription_kind_of(head), rowid],
                )
                .map_err(db_err)?;
            }
            tx.commit().map_err(db_err)?;
            stamped += rows.len() as u64;
            if stamped % 50_000 == 0 {
                tracing::info!("payload-tag backfill: {stamped} events stamped…");
            }
        }
        if stamped > 0 {
            tracing::info!("payload-tag backfill: {stamped} events stamped");
        }
        Ok(())
    }

    /// True while any payload-carrying event row still lacks its payload_tag /
    /// inscription_kind stamp (an old binary wrote after this one's backfill,
    /// or a backfill is racing on another connection). O(1) via the
    /// ev_payload_tag_todo partial index.
    fn payload_tags_pending(&self) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM covenant_events WHERE payload IS NOT NULL AND payload_tag IS NULL)",
                [],
                |r| r.get(0),
            )
            .map_err(db_err)
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
            // Payload classification is stamped at write time (like the UTXO
            // templates above) so the lanes/inscriptions analytics stay pure
            // GROUP BYs at read time. NULL payload → NULL stamps.
            let (tag, kind) = match &event.payload {
                Some(p) => (Some(payload_tag(p)), Some(inscription_kind_of(p))),
                None => (None, None),
            };
            tx.execute(
                "INSERT INTO covenant_events (covenant_id, seq, kind, txid, accepting_block, accepting_daa, payload, lane_namespace, payload_tag, inscription_kind)
                 VALUES (?1,
                   (SELECT COALESCE(MAX(seq), -1) + 1 FROM covenant_events WHERE covenant_id = ?1),
                   ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    event.covenant_id.0.as_slice(),
                    event.kind.as_str(),
                    event.txid.0.as_slice(),
                    block.accepting_block.0.as_slice(),
                    block.accepting_daa,
                    event.payload,
                    event.lane_namespace,
                    tag,
                    kind
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
        }
        // Covenants whose genesis was rolled back disappear entirely.
        tx.execute("DELETE FROM covenants WHERE event_count <= 0", []).map_err(db_err)?;
        // Record the reorg for the public feed. The best-available DAA is the
        // indexer's own progress mark (the tip we had reached) — the removed
        // blocks are being deleted, so their DAAs aren't reliably queryable
        // here, and not every reorged block carried covenant activity anyway.
        if !removed.is_empty() {
            let daa: u64 = tx
                .query_row("SELECT value FROM meta WHERE key = 'processed_daa'", [], |row| {
                    row.get::<_, String>(0)
                })
                .optional()
                .map_err(db_err)?
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            tx.execute(
                "INSERT INTO reorg_log (daa, at_ms, rolled_back) VALUES (?1, ?2, ?3)",
                params![daa, now_ms(), removed.len() as u64],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)
    }

    /// The most recent applied reorgs, newest first. Backs the public reorg
    /// feed; caps at `limit` rows.
    pub fn reorg_log(&self, limit: u64) -> Result<Vec<ReorgRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT daa, at_ms, rolled_back FROM reorg_log ORDER BY id DESC LIMIT ?1")
            .map_err(db_err)?;
        let limit = limit.min(i64::MAX as u64) as i64;
        let rows = stmt
            .query_map([limit], |row| {
                Ok(ReorgRow { daa: row.get(0)?, at_ms: row.get(1)?, rolled_back: row.get(2)? })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    pub fn list(&self, limit: u64) -> Result<Vec<CovenantSummary>> {
        let sql = format!("{SUMMARY_SELECT} ORDER BY c.last_activity_daa DESC LIMIT ?1");
        let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
        let limit = limit.min(i64::MAX as u64) as i64;
        let rows = stmt
            .query_map([limit], map_summary_row)
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// A single page of the covenant list, newest activity first. `after` is an
    /// exclusive compound cursor `(last_activity_daa, covenant_id)`: pass the
    /// previous page's `(next_after_daa, next_after_id)` to walk backwards.
    /// `None` starts from the tip. The compound key means covenants sharing a
    /// boundary DAA page deterministically instead of being skipped.
    pub fn list_page(&self, after: Option<(u64, [u8; 32])>, limit: u64) -> Result<Vec<CovenantSummary>> {
        let order = "ORDER BY c.last_activity_daa DESC, c.covenant_id DESC";
        let limit = limit.min(i64::MAX as u64) as i64;
        let rows = match after {
            Some((daa, id)) => {
                let sql = format!(
                    "{SUMMARY_SELECT} WHERE c.last_activity_daa < ?1 \
                       OR (c.last_activity_daa = ?1 AND c.covenant_id < ?2) {order} LIMIT ?3"
                );
                let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
                let daa = daa.min(i64::MAX as u64) as i64;
                let out = stmt
                    .query_map(params![daa, id.as_slice(), limit], map_summary_row)
                    .map_err(db_err)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(db_err)?;
                out
            }
            None => {
                let sql = format!("{SUMMARY_SELECT} {order} LIMIT ?1");
                let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
                let out = stmt
                    .query_map([limit], map_summary_row)
                    .map_err(db_err)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(db_err)?;
                out
            }
        };
        Ok(rows)
    }

    pub fn summary(&self, id: &CovenantId) -> Result<Option<CovenantSummary>> {
        let sql = format!("{SUMMARY_SELECT} WHERE c.covenant_id = ?1");
        let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
        let row = stmt
            .query_map([id.0.as_slice()], map_summary_row)
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
    /// Lifespan distribution of retired coins: for every covenant with a
    /// genesis and a burn, life = burn_daa − genesis_daa. Returns a fixed
    /// time-bucket histogram (10 DAA ≈ 1 s), the median lifespan in DAA, and
    /// the total sampled. The "how long do smart coins live?" analytic.
    pub fn lifespan_stats(&self) -> Result<(Vec<(&'static str, u64)>, u64, u64)> {
        let cte = "WITH lifespans AS (
            SELECT (bb.b - gg.g) AS life FROM
              (SELECT covenant_id, MIN(accepting_daa) g FROM covenant_events WHERE kind='genesis' GROUP BY covenant_id) gg
              JOIN (SELECT covenant_id, accepting_daa b FROM covenant_events WHERE kind='burn') bb ON gg.covenant_id = bb.covenant_id
            WHERE (bb.b - gg.g) >= 0)";
        let labels = ["< 10 s", "10 s – 1 min", "1 – 10 min", "10 min – 1 h", "1 – 6 h", "6 h +"];
        let hist_sql = format!(
            "{cte} SELECT CASE
               WHEN life < 100 THEN 0 WHEN life < 600 THEN 1 WHEN life < 6000 THEN 2
               WHEN life < 36000 THEN 3 WHEN life < 216000 THEN 4 ELSE 5 END AS b, COUNT(*)
             FROM lifespans GROUP BY b"
        );
        let mut counts = [0u64; 6];
        {
            let mut stmt = self.conn.prepare(&hist_sql).map_err(db_err)?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)? as usize, r.get::<_, i64>(1)? as u64)))
                .map_err(db_err)?;
            for row in rows {
                let (b, c) = row.map_err(db_err)?;
                if b < 6 {
                    counts[b] = c;
                }
            }
        }
        let total: u64 = counts.iter().sum();
        let median = if total > 0 {
            let med_sql = format!("{cte} SELECT life FROM lifespans ORDER BY life LIMIT 1 OFFSET ?");
            self.conn
                .query_row(&med_sql, [(total / 2) as i64], |r| r.get::<_, i64>(0))
                .map(|v| v as u64)
                .unwrap_or(0)
        } else {
            0
        };
        let buckets = labels.iter().zip(counts.iter()).map(|(l, c)| (*l, *c)).collect();
        Ok((buckets, median, total))
    }

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

    /// Based-app activity, classified: covenant events that carried a v1 tx
    /// payload, grouped by what the payload actually IS — JSON inscriptions
    /// (raw `{"…` and hex-encoded) folded together, everything else keyed by
    /// its leading 4-byte tag. Returns (key, event_count, distinct_covenants);
    /// key is `json` / `jsonhex` / `tag:<hex>`. The worker turns these into
    /// human labels. Busiest first.
    ///
    /// Reads the precomputed `payload_tag` stamp (covering ev_tag_stats
    /// index); while any row is still unstamped it falls back to the legacy
    /// per-row scan so results never go partial mid-backfill.
    pub fn based_app_namespaces(&self) -> Result<Vec<(String, u64, u64)>> {
        if self.payload_tags_pending()? {
            return self.based_app_namespaces_scan();
        }
        let mut stmt = self
            .conn
            .prepare(
                // The WHERE terms must stay verbatim-identical to the
                // ev_tag_stats partial-index predicate. payload_tag <> ''
                // encodes the legacy `payload IS NOT NULL AND
                // length(payload) >= 4` filter; lane_namespace IS NULL keeps
                // the strict complement with lane_namespaces().
                "SELECT payload_tag,
                        COUNT(*) AS events,
                        COUNT(DISTINCT covenant_id) AS coins
                 FROM covenant_events
                 WHERE lane_namespace IS NULL AND payload_tag IS NOT NULL AND payload_tag <> ''
                 GROUP BY payload_tag
                 ORDER BY events DESC, payload_tag
                 LIMIT 200",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64)))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Legacy per-row scan of [`based_app_namespaces`] — classifies payloads
    /// with substr/hex on every call. Kept as the mid-backfill fallback (and
    /// as the ground truth the tests compare the stamped path against).
    fn based_app_namespaces_scan(&self) -> Result<Vec<(String, u64, u64)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT CASE
                          WHEN substr(payload, 1, 2) = x'7b22' THEN 'json'
                          WHEN substr(payload, 1, 4) = x'37623232' THEN 'jsonhex'
                          ELSE 'tag:' || lower(hex(substr(payload, 1, 4)))
                        END AS k,
                        COUNT(*) AS events,
                        COUNT(DISTINCT covenant_id) AS coins
                 FROM covenant_events
                 WHERE payload IS NOT NULL AND length(payload) >= 4
                   AND lane_namespace IS NULL
                 GROUP BY k
                 ORDER BY events DESC
                 LIMIT 200",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64)))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// KIP-21 user-lane activity, grouped by the stored `lane_namespace` (the
    /// 4-byte app tag, hex). Only events whose payload had the lane shape at
    /// write time are counted — the strict complement of the generic tag
    /// buckets in [`based_app_namespaces`], so a lane never double-counts.
    /// Returns (namespace_hex, event_count, distinct_covenants), busiest first.
    pub fn lane_namespaces(&self) -> Result<Vec<(String, u64, u64)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT lane_namespace,
                        COUNT(*) AS events,
                        COUNT(DISTINCT covenant_id) AS coins
                 FROM covenant_events
                 WHERE lane_namespace IS NOT NULL
                 GROUP BY lane_namespace
                 ORDER BY events DESC
                 LIMIT 200",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64)))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Decoded inscription activity: parse the JSON payloads (raw `{"…` and
    /// ASCII-hex-encoded) and group by what they actually are — protocol/op/
    /// tick for KRC-20-style tokens, or the `t`/top-level type for others.
    /// Returns (kind_label, event_count, distinct_covenants), busiest first.
    ///
    /// Reads the precomputed `inscription_kind` stamp (covering
    /// ev_inscription_stats index); while any row is still unstamped it falls
    /// back to the legacy parse-every-payload scan so results never go
    /// partial mid-backfill.
    pub fn inscription_breakdown(&self) -> Result<Vec<(String, u64, u64)>> {
        if self.payload_tags_pending()? {
            return self.inscription_breakdown_scan();
        }
        let mut stmt = self
            .conn
            .prepare(
                // WHERE terms verbatim-identical to the ev_inscription_stats
                // partial-index predicate; '' marks non-inscription payloads.
                "SELECT inscription_kind,
                        COUNT(*) AS events,
                        COUNT(DISTINCT covenant_id) AS coins
                 FROM covenant_events
                 WHERE inscription_kind IS NOT NULL AND inscription_kind <> ''
                 GROUP BY inscription_kind
                 ORDER BY events DESC, inscription_kind
                 LIMIT 60",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64)))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Legacy scan of [`inscription_breakdown`] — JSON-parses every candidate
    /// payload on each call. Kept as the mid-backfill fallback (and as the
    /// ground truth the tests compare the stamped path against).
    fn inscription_breakdown_scan(&self) -> Result<Vec<(String, u64, u64)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT substr(payload, 1, 512), lower(hex(covenant_id))
                 FROM covenant_events
                 WHERE payload IS NOT NULL
                   AND (substr(payload, 1, 2) = x'7b22' OR substr(payload, 1, 4) = x'37623232')",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_err)?;
        // kind -> (event count, distinct covenant ids)
        let mut agg: std::collections::HashMap<String, (u64, std::collections::HashSet<String>)> =
            std::collections::HashMap::new();
        for row in rows {
            let (payload, cid) = row.map_err(db_err)?;
            let Some(v) = extract_inscription_json(&payload) else { continue };
            let kind = inscription_kind(&v);
            let e = agg.entry(kind).or_default();
            e.0 += 1;
            e.1.insert(cid);
        }
        let mut out: Vec<(String, u64, u64)> =
            agg.into_iter().map(|(k, (c, set))| (k, c, set.len() as u64)).collect();
        out.sort_by(|a, b| b.1.cmp(&a.1));
        out.truncate(60);
        Ok(out)
    }

    /// Record a community-verified source (proven byte-identical to a compiled
    /// program). Keyed by the program's blake2b hash.
    pub fn put_verified_source(&self, hash: &str, hex: &str, source: &str, args: &str, template: Option<&str>, now_ms: u64) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO verified_sources (program_hash, program_hex, source, args, template, verified_at) VALUES (?1,?2,?3,?4,?5,?6)",
                params![hash, hex, source, args, template, now_ms as i64],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Fetch a published source by program hash → (source, args, template, verified_at).
    pub fn get_verified_source(&self, hash: &str) -> Result<Option<(String, String, Option<String>, u64)>> {
        self.conn
            .query_row(
                "SELECT source, args, template, verified_at FROM verified_sources WHERE program_hash = ?1",
                params![hash],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)? as u64)),
            )
            .optional()
            .map_err(db_err)
    }

    /// Add a webhook subscription (covenant_id / kind NULL = wildcard). Returns its id.
    pub fn add_subscription(&self, covenant_id: Option<&[u8]>, kind: Option<&str>, url: &str, now_ms: u64) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO webhook_subscriptions (covenant_id, kind, url, created_at) VALUES (?1,?2,?3,?4)",
                params![covenant_id, kind, url, now_ms as i64],
            )
            .map_err(db_err)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Remove a subscription by id. Returns whether one was deleted.
    pub fn delete_subscription(&self, id: i64) -> Result<bool> {
        let n = self.conn.execute("DELETE FROM webhook_subscriptions WHERE id = ?1", params![id]).map_err(db_err)?;
        Ok(n > 0)
    }

    /// Webhook URLs matching an event (covenant_id + kind; NULL columns are wildcards).
    pub fn subscriptions_for(&self, covenant_id: &[u8], kind: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT url FROM webhook_subscriptions WHERE (covenant_id IS NULL OR covenant_id = ?1) AND (kind IS NULL OR kind = ?2)")
            .map_err(db_err)?;
        let rows = stmt.query_map(params![covenant_id, kind], |r| r.get::<_, String>(0)).map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Total active subscriptions (for the fire loop to skip work when zero).
    pub fn subscription_count(&self) -> Result<u64> {
        self.conn.query_row("SELECT COUNT(*) FROM webhook_subscriptions", [], |r| r.get::<_, i64>(0)).map(|n| n as u64).map_err(db_err)
    }

    /// Like [`subscriptions_for`] but returns `(id, url)` — the delivery loop
    /// needs the id to retire a subscription after repeated failures.
    pub fn subscriptions_matching(&self, covenant_id: &[u8], kind: &str) -> Result<Vec<(i64, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, url FROM webhook_subscriptions WHERE (covenant_id IS NULL OR covenant_id = ?1) AND (kind IS NULL OR kind = ?2)")
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![covenant_id, kind], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(db_err)
    }

    /// One lane's headline numbers: (event count, distinct covenants).
    /// Walks the ev_by_lane partial index.
    pub fn lane_stats(&self, namespace: &str) -> Result<(u64, u64)> {
        self.conn
            .query_row(
                "SELECT COUNT(*), COUNT(DISTINCT covenant_id)
                 FROM covenant_events WHERE lane_namespace = ?1",
                params![namespace],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)),
            )
            .map_err(db_err)
    }

    /// The newest events inside one lane namespace, newest first.
    pub fn lane_recent(&self, namespace: &str, limit: u64) -> Result<Vec<GlobalEventRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT covenant_id, seq, kind, txid, accepting_daa
                 FROM covenant_events WHERE lane_namespace = ?1
                 ORDER BY accepting_daa DESC, rowid DESC LIMIT ?2",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![namespace, limit.min(i64::MAX as u64) as i64], |row| {
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

    /// One lane's event counts per fixed-width DAA bucket, oldest first.
    /// Returns `(bucket_start_daa, count)`; empty buckets are omitted.
    pub fn lane_activity(&self, namespace: &str, bucket_daa: u64) -> Result<Vec<(u64, u64)>> {
        let width = bucket_daa.max(1);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT accepting_daa / ?2 AS bucket, COUNT(*)
                 FROM covenant_events WHERE lane_namespace = ?1
                 GROUP BY bucket ORDER BY bucket",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![namespace, width as i64], |row| {
                Ok((row.get::<_, u64>(0)? * width, row.get::<_, i64>(1)? as u64))
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// The state UTXOs a transaction spent (with the captured unlocking
    /// scripts) — the raw material of the real-spend debugger. Walks the
    /// utxo_by_spent_txid partial index.
    pub fn spent_by_txid(&self, txid: &TxId) -> Result<Vec<SpentStateRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT covenant_id, txid, output_index, value, spk_version, spk_script, spent_sig, spent_budget
                 FROM covenant_utxos WHERE spent_txid = ?1 ORDER BY txid, output_index",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([txid.0.as_slice()], |row| {
                Ok(SpentStateRow {
                    covenant_id: CovenantId(row.get(0)?),
                    outpoint: Outpoint { txid: TxId(row.get(1)?), index: row.get(2)? },
                    value: row.get(3)?,
                    spk_version: row.get(4)?,
                    spk_script: row.get(5)?,
                    spent_sig: row.get(6)?,
                    spent_budget: row.get(7)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Transactions that touched more than one covenant, with the covenants
    /// they moved together — the raw edges of multi-contract "apps".
    /// (A single tx moving several covenants is a Toccata multi-contract flow.)
    pub fn multi_covenant_txs(&self) -> Result<Vec<(TxId, Vec<CovenantId>)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT txid, covenant_id FROM covenant_events
                 WHERE txid IN (
                   SELECT txid FROM covenant_events
                   GROUP BY txid HAVING COUNT(DISTINCT covenant_id) > 1
                 )
                 ORDER BY txid",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((TxId(r.get(0)?), CovenantId(r.get(1)?))))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        // group consecutive rows by txid (query is ordered)
        let mut out: Vec<(TxId, Vec<CovenantId>)> = Vec::new();
        for (txid, cov) in rows {
            match out.last_mut() {
                Some((t, covs)) if *t == txid => {
                    if !covs.contains(&cov) {
                        covs.push(cov);
                    }
                }
                _ => out.push((txid, vec![cov])),
            }
        }
        Ok(out)
    }

    /// Alive/burned per covenant in ONE grouped pass over covenant_utxos
    /// (walks utxo_by_covenant). Replaces deriving the flag from
    /// `list(u64::MAX)`, whose two correlated subqueries per row cost ~2N
    /// index probes at N covenants. Covenants with no UTXO rows are absent —
    /// callers treat missing as inactive.
    pub fn active_flags(&self) -> Result<std::collections::HashMap<CovenantId, bool>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT covenant_id, MAX(spent_block IS NULL) FROM covenant_utxos GROUP BY covenant_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((CovenantId(r.get(0)?), r.get::<_, i64>(1)? != 0)))
            .map_err(db_err)?
            .collect::<std::result::Result<std::collections::HashMap<_, _>, _>>()
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

    /// Every covenant id, nothing else — one cheap primary-key scan with none
    /// of the per-row summary subselects. Feeds the worker's in-memory search
    /// index (friendly names derive from the id alone).
    pub fn covenant_ids(&self) -> Result<Vec<[u8; 32]>> {
        let mut stmt = self
            .conn
            .prepare("SELECT covenant_id FROM covenants")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, [u8; 32]>(0))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// COUNT(*) over covenants — the cheap staleness probe for caches built
    /// from `covenant_ids()` (ids are append-only, so a stable count means a
    /// stable id set).
    pub fn covenant_count(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM covenants", [], |r| r.get(0))
            .map_err(db_err)
    }

    /// Covenants whose 32-byte id lies in the inclusive `[lo, hi]` byte range,
    /// id order. This is how a hex prefix search maps onto the BLOB primary
    /// key: prefix bytes padded with 0x00 form `lo`, padded with 0xff form
    /// `hi`, and BLOB comparison (memcmp) turns the BETWEEN into a bounded
    /// index range scan.
    pub fn covenants_by_id_range(
        &self,
        lo: &[u8; 32],
        hi: &[u8; 32],
        limit: u64,
    ) -> Result<Vec<CovenantSummary>> {
        let sql = format!(
            "{SUMMARY_SELECT} WHERE c.covenant_id BETWEEN ?1 AND ?2 ORDER BY c.covenant_id LIMIT ?3"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
        let limit = limit.min(i64::MAX as u64) as i64;
        let rows = stmt
            .query_map(params![lo.as_slice(), hi.as_slice(), limit], map_summary_row)
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
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

    /// The p2pk-state owners of ONE covenant — the inverse of
    /// `covenants_by_pubkey`. Groups this covenant's state UTXOs by their
    /// exact spk (a p2pk script is unique per owner key), then keeps the rows
    /// whose shape is the p2pk template `[len-2 push][key][OpCheckSig]` and
    /// lifts the owner pubkey out of the script. Single indexed-by-covenant
    /// query, bounded by a SQL `LIMIT` (a multiple of `limit`, since the
    /// p2pk-shape filter runs after the fetch) so a covenant with many distinct
    /// scripts can't materialize unbounded groups on every detail load; the
    /// Rust-side cap then keeps `limit` most-recent p2pk owners (pass e.g. 100).
    pub fn holders_of_covenant(&self, id: &CovenantId, limit: u64) -> Result<Vec<HolderRow>> {
        // scan bound: enough headroom to survive the shape filter, still bounded
        let scan = limit.saturating_mul(10).clamp(64, i64::MAX as u64) as i64;
        let mut stmt = self
            .conn
            .prepare(
                "SELECT spk_script,
                        MAX(spent_block IS NULL) AS controls_now,
                        COUNT(*) AS states_seen,
                        MIN(created_daa) AS first_seen_daa,
                        MAX(created_daa) AS last_seen_daa
                 FROM covenant_utxos
                 WHERE covenant_id = ?1
                 GROUP BY spk_script
                 ORDER BY last_seen_daa DESC
                 LIMIT ?2",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![id.0.as_slice(), scan], |row| {
                let spk: Vec<u8> = row.get(0)?;
                Ok((
                    spk,
                    row.get::<_, bool>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, u64>(4)?,
                ))
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        let mut holders = Vec::new();
        for (spk, controls_now, states_seen, first_seen_daa, last_seen_daa) in rows {
            // p2pk shape: [len-2 push opcode][key][OpCheckSig], key 32 or 33 bytes.
            let key = match spk.first().copied() {
                Some(len @ (32 | 33)) if spk.len() == len as usize + 2 && spk.last() == Some(&0xac) => {
                    &spk[1..1 + len as usize]
                }
                _ => continue,
            };
            holders.push(HolderRow {
                pubkey: hex::encode(key),
                controls_now,
                states_seen,
                first_seen_daa,
                last_seen_daa,
            });
            if holders.len() as usize >= limit as usize {
                break;
            }
        }
        Ok(holders)
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

    fn test_store_path(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir()
            .join(format!("kascov-store-test-{}-{name}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn test_store(name: &str) -> Store {
        Store::open(&test_store_path(name), Network::Testnet(10)).unwrap()
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
                    lane_namespace: None,
                })
                .collect(),
            created_utxos: vec![],
            spent_utxos: vec![],
        }
    }

    #[test]
    fn id_range_scan_maps_hex_prefixes() {
        let mut store = test_store("id-range");
        // ids 0xA0.., 0xA1.., 0xA1(0xA1 everywhere), 0xB0..
        let mut id_a1_zero = [0u8; 32];
        id_a1_zero[0] = 0xa1;
        let block = BlockEvents {
            accepting_block: BlockHash([9; 32]),
            accepting_daa: 100,
            events: vec![
                NewEvent { covenant_id: CovenantId([0xa0; 32]), kind: EventKind::Genesis, txid: TxId([1; 32]), payload: None, lane_namespace: None },
                NewEvent { covenant_id: CovenantId(id_a1_zero), kind: EventKind::Genesis, txid: TxId([2; 32]), payload: None, lane_namespace: None },
                NewEvent { covenant_id: CovenantId([0xa1; 32]), kind: EventKind::Genesis, txid: TxId([3; 32]), payload: None, lane_namespace: None },
                NewEvent { covenant_id: CovenantId([0xb0; 32]), kind: EventKind::Genesis, txid: TxId([4; 32]), payload: None, lane_namespace: None },
            ],
            created_utxos: vec![],
            spent_utxos: vec![],
        };
        store.apply(&block, BlockHash([9; 32])).unwrap();

        assert_eq!(store.covenant_count().unwrap(), 4);
        assert_eq!(store.covenant_ids().unwrap().len(), 4);

        // prefix "a1" → [a1 00…00, a1 ff…ff]: both a1-led ids, in id order.
        let mut lo = [0u8; 32];
        lo[0] = 0xa1;
        let mut hi = [0xffu8; 32];
        hi[0] = 0xa1;
        let rows = store.covenants_by_id_range(&lo, &hi, 20).unwrap();
        let ids: Vec<[u8; 32]> = rows.iter().map(|r| r.covenant_id.0).collect();
        assert_eq!(ids, vec![id_a1_zero, [0xa1; 32]]);

        // limit is honored
        assert_eq!(store.covenants_by_id_range(&lo, &hi, 1).unwrap().len(), 1);

        // a range with no members is empty, not an error
        let mut lo2 = [0u8; 32];
        lo2[0] = 0xc0;
        let mut hi2 = [0xffu8; 32];
        hi2[0] = 0xc0;
        assert!(store.covenants_by_id_range(&lo2, &hi2, 20).unwrap().is_empty());
    }

    #[test]
    fn lane_namespace_sniff() {
        // Lane shape: 4-byte namespace + 16 zero bytes → namespace hex.
        let mut lane = vec![0xde, 0xad, 0xbe, 0xef];
        lane.extend_from_slice(&[0u8; 16]);
        assert_eq!(lane_namespace(&lane).as_deref(), Some("deadbeef"));
        // Trailing bytes after the 16 zeros are allowed (payload body).
        let mut lane_body = lane.clone();
        lane_body.extend_from_slice(b"hello");
        assert_eq!(lane_namespace(&lane_body).as_deref(), Some("deadbeef"));
        // Too short (< 20 bytes) is never a lane.
        assert_eq!(lane_namespace(&lane[..19]), None);
        // Non-zero in the 16-byte gap disqualifies it (e.g. a JSON payload).
        let mut not_lane = vec![0xde, 0xad, 0xbe, 0xef];
        not_lane.extend_from_slice(&[0u8; 16]);
        not_lane[10] = 1;
        assert_eq!(lane_namespace(&not_lane), None);
    }

    #[test]
    fn lane_namespaces_group_and_exclude_tags() {
        let store = test_store("lanes");
        let lane_ns = "01020304".to_string();
        let mut lane_payload = hex::decode(&lane_ns).unwrap();
        lane_payload.extend_from_slice(&[0u8; 16]);
        let block = BlockEvents {
            accepting_block: BlockHash([9; 32]),
            accepting_daa: 100,
            events: vec![
                NewEvent {
                    covenant_id: CovenantId([1; 32]),
                    kind: EventKind::Genesis,
                    txid: TxId([1; 32]),
                    payload: Some(lane_payload.clone()),
                    lane_namespace: Some(lane_ns.clone()),
                },
                // A generic (non-lane) tagged payload — must stay in the tag
                // buckets and never appear as a lane.
                NewEvent {
                    covenant_id: CovenantId([2; 32]),
                    kind: EventKind::Genesis,
                    txid: TxId([2; 32]),
                    payload: Some(vec![0xaa, 0xbb, 0xcc, 0xdd, 0x01]),
                    lane_namespace: None,
                },
            ],
            created_utxos: vec![],
            spent_utxos: vec![],
        };
        let mut store = store;
        store.apply(&block, BlockHash([9; 32])).unwrap();
        let lanes = store.lane_namespaces().unwrap();
        assert_eq!(lanes, vec![(lane_ns, 1, 1)]);
        // The tag view excludes the lane row (no double count) but keeps the
        // generic tagged payload.
        let tags = store.based_app_namespaces().unwrap();
        assert_eq!(tags, vec![("tag:aabbccdd".to_string(), 1, 1)]);
        // Everything was stamped at write time, so this went through the
        // payload_tag fast path — and it must agree with the legacy scan.
        assert!(!store.payload_tags_pending().unwrap());
        assert_eq!(tags, store.based_app_namespaces_scan().unwrap());
    }

    /// The full stamp lifecycle: write-time stamping, the legacy-scan
    /// fallback while stamps are missing, and the on-open backfill — the
    /// grouped fast-path results must match the legacy scans at every step,
    /// and the lane-vs-tag complement must survive the round trip.
    #[test]
    fn payload_tag_backfill_matches_scan() {
        let path = test_store_path("tag-backfill");
        let lane_ns = "01020304".to_string();
        let mut lane_payload = hex::decode(&lane_ns).unwrap();
        lane_payload.extend_from_slice(&[0u8; 16]);
        let json = br#"{"p":"krc-20","op":"mint","tick":"KAS"}"#.to_vec();
        let jsonhex = hex::encode(br#"{"t":"note"}"#).into_bytes();
        let ev = |cov: u8, tx: u8, payload: Option<Vec<u8>>, lane: Option<String>| NewEvent {
            covenant_id: CovenantId([cov; 32]),
            kind: EventKind::Genesis,
            txid: TxId([tx; 32]),
            payload,
            lane_namespace: lane,
        };
        let block = BlockEvents {
            accepting_block: BlockHash([9; 32]),
            accepting_daa: 100,
            events: vec![
                ev(1, 1, Some(lane_payload), Some(lane_ns.clone())),
                ev(2, 2, Some(vec![0xaa, 0xbb, 0xcc, 0xdd, 0x01]), None),
                ev(3, 3, Some(json.clone()), None),
                ev(4, 4, Some(json), None),  // same kind, second covenant
                ev(5, 5, Some(jsonhex), None),
                ev(6, 6, Some(vec![0x01]), None), // < 4 bytes: excluded everywhere
                ev(7, 7, None, None),
            ],
            created_utxos: vec![],
            spent_utxos: vec![],
        };
        let mut store = Store::open(&path, Network::Testnet(10)).unwrap();
        store.apply(&block, BlockHash([9; 32])).unwrap();

        // The legacy scans leave the order of equal-count groups to SQLite's
        // sorter / HashMap; the fast path breaks ties by key. Normalize scan
        // output to the fast path's deterministic (events DESC, key) order.
        let norm = |mut v: Vec<(String, u64, u64)>| {
            v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            v
        };

        // Write-time stamps: fast path active and agreeing with the scans.
        assert!(!store.payload_tags_pending().unwrap());
        let tags = store.based_app_namespaces().unwrap();
        let kinds = store.inscription_breakdown().unwrap();
        assert_eq!(tags, norm(store.based_app_namespaces_scan().unwrap()));
        assert_eq!(kinds, norm(store.inscription_breakdown_scan().unwrap()));
        assert_eq!(
            tags,
            vec![
                ("json".to_string(), 2, 2),
                ("jsonhex".to_string(), 1, 1),
                ("tag:aabbccdd".to_string(), 1, 1),
            ]
        );
        assert_eq!(
            kinds,
            vec![("krc-20 · mint · KAS".to_string(), 2, 2), ("note".to_string(), 1, 1)]
        );
        // Complement: the lane row lives in lane_namespaces, never in tags.
        assert_eq!(store.lane_namespaces().unwrap(), vec![(lane_ns, 1, 1)]);

        // Wipe the stamps (rows as an old binary would have written them):
        // both public fns must notice and fall back to the legacy scans.
        store
            .conn
            .execute("UPDATE covenant_events SET payload_tag = NULL, inscription_kind = NULL", [])
            .unwrap();
        assert!(store.payload_tags_pending().unwrap());
        assert_eq!(norm(store.based_app_namespaces().unwrap()), tags);
        assert_eq!(norm(store.inscription_breakdown().unwrap()), kinds);

        // Reopen: the backfill stamps everything and the fast path returns.
        drop(store);
        let store = Store::open(&path, Network::Testnet(10)).unwrap();
        assert!(!store.payload_tags_pending().unwrap());
        assert_eq!(store.based_app_namespaces().unwrap(), tags);
        assert_eq!(store.inscription_breakdown().unwrap(), kinds);
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

    #[test]
    fn cov_by_activity_index_serves_list_page() {
        let store = test_store("activity-index");
        // the ordered list query must use the compound index, not a temp B-tree
        for sql in [
            "SELECT covenant_id FROM covenants ORDER BY last_activity_daa DESC, covenant_id DESC LIMIT 10",
            "SELECT covenant_id FROM covenants WHERE last_activity_daa < 100 \
               OR (last_activity_daa = 100 AND covenant_id < x'ff') \
             ORDER BY last_activity_daa DESC, covenant_id DESC LIMIT 10",
        ] {
            let plan: Vec<String> = store
                .conn
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map([], |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap();
            let joined = plan.join(" | ");
            assert!(joined.contains("cov_by_activity"), "plan missing index: {joined}");
            assert!(!joined.contains("TEMP B-TREE"), "plan still sorts: {joined}");
        }
    }

    #[test]
    fn active_flags_matches_list_derivation() {
        let mut store = test_store("active-flags");
        // A1: one live utxo (active) · B2: utxo created then spent (burned)
        let mut b1 = block_with_events(1, 100, vec![(0xA1, EventKind::Genesis, 0x01), (0xB2, EventKind::Genesis, 0x02)]);
        b1.created_utxos = vec![
            NewUtxo { outpoint: Outpoint { txid: TxId([0x01; 32]), index: 0 }, covenant_id: CovenantId([0xA1; 32]), value: 5, spk_version: 0, spk_script: vec![] },
            NewUtxo { outpoint: Outpoint { txid: TxId([0x02; 32]), index: 0 }, covenant_id: CovenantId([0xB2; 32]), value: 7, spk_version: 0, spk_script: vec![] },
        ];
        store.apply(&b1, BlockHash([1; 32])).unwrap();
        let mut b2 = block_with_events(2, 200, vec![(0xB2, EventKind::Burn, 0x03)]);
        b2.spent_utxos = vec![(Outpoint { txid: TxId([0x02; 32]), index: 0 }, TxId([0x03; 32]), vec![], 0)];
        store.apply(&b2, BlockHash([2; 32])).unwrap();

        let flags = store.active_flags().unwrap();
        for c in store.list(u64::MAX).unwrap() {
            assert_eq!(
                flags.get(&c.covenant_id).copied().unwrap_or(false),
                c.live_utxos > 0,
                "flag mismatch for {:?}", c.covenant_id
            );
        }
        assert_eq!(flags.get(&CovenantId([0xA1; 32])), Some(&true));
        assert_eq!(flags.get(&CovenantId([0xB2; 32])), Some(&false));
    }

    /// The born_value/template columns folded into the summary row queries
    /// must agree, row for row, with the standalone map builders they mirror
    /// (`born_values()` / `covenant_templates()`) and the point query
    /// `born_value()` — across `list()`, `list_page()` and `summary()`.
    #[test]
    fn folded_born_value_and_template_match_map_queries() {
        let mut store = test_store("folded-summary");
        let junk = vec![0x51, 0x51]; // OpTrue OpTrue — recognizes as '' (no template)
        let utxo = |tx: u8, cov: u8, value: u64| NewUtxo {
            outpoint: Outpoint { txid: TxId([tx; 32]), index: 0 },
            covenant_id: CovenantId([cov; 32]),
            value,
            spk_version: 1,
            spk_script: junk.clone(),
        };
        // genesis block: A1 born with two outputs (5+7), B2 with one (9), C3 bare
        let mut b1 = block_with_events(
            1,
            100,
            vec![
                (0xA1, EventKind::Genesis, 0x01),
                (0xB2, EventKind::Genesis, 0x02),
                (0xC3, EventKind::Genesis, 0x07),
            ],
        );
        b1.created_utxos = vec![utxo(0x01, 0xA1, 5), utxo(0x08, 0xA1, 7), utxo(0x02, 0xB2, 9)];
        store.apply(&b1, BlockHash([1; 32])).unwrap();
        // later block: A1 gains a post-genesis state (NOT born value), B2 is swept
        let mut b2 = block_with_events(
            2,
            200,
            vec![(0xA1, EventKind::Transition, 0x03), (0xB2, EventKind::Burn, 0x04)],
        );
        b2.created_utxos = vec![utxo(0x03, 0xA1, 11)];
        b2.spent_utxos =
            vec![(Outpoint { txid: TxId([0x02; 32]), index: 0 }, TxId([0x04; 32]), vec![], 0)];
        store.apply(&b2, BlockHash([2; 32])).unwrap();

        // Stamp templates directly to exercise every pick-rule branch:
        // A1: a generic p2 state row plus a non-p2 reveal → the reveal wins;
        // B2: p2-only → the any-template fallback picks it; C3: no rows → None.
        // (A1's third row keeps the write-time '' stamp: excluded by the filter.)
        for (tx, sql) in [
            (0x01u8, "UPDATE covenant_utxos SET template = 'p2pk state' WHERE txid = ?1"),
            (0x08, "UPDATE covenant_utxos SET template = 'p2sh commitment', revealed_template = 'mecenas' WHERE txid = ?1"),
            (0x02, "UPDATE covenant_utxos SET template = 'p2sh commitment' WHERE txid = ?1"),
        ] {
            store.conn.execute(sql, [[tx; 32].as_slice()]).unwrap();
        }

        let born = store.born_values().unwrap();
        let templates = store.covenant_templates().unwrap();
        let listed = store.list(u64::MAX).unwrap();
        assert_eq!(listed.len(), 3);
        let paged = store.list_page(None, 10).unwrap();
        assert_eq!(paged.len(), 3);
        for c in listed.iter().chain(paged.iter()) {
            assert_eq!(
                c.born_value,
                born.get(&c.covenant_id).copied().unwrap_or(0),
                "born_value mismatch for {:?}", c.covenant_id
            );
            assert_eq!(
                c.born_value,
                store.born_value(&c.covenant_id).unwrap(),
                "point born_value mismatch for {:?}", c.covenant_id
            );
            assert_eq!(
                c.template.as_ref(),
                templates.get(&c.covenant_id),
                "template mismatch for {:?}", c.covenant_id
            );
            let s = store.summary(&c.covenant_id).unwrap().unwrap();
            assert_eq!((s.born_value, &s.template), (c.born_value, &c.template));
        }
        // pinned expectations, so the folded columns and the maps can't both
        // drift in the same direction unnoticed
        let a1 = store.summary(&CovenantId([0xA1; 32])).unwrap().unwrap();
        assert_eq!((a1.born_value, a1.template.as_deref()), (12, Some("mecenas")));
        let b2 = store.summary(&CovenantId([0xB2; 32])).unwrap().unwrap();
        assert_eq!((b2.born_value, b2.template.as_deref()), (9, Some("p2sh commitment")));
        let c3 = store.summary(&CovenantId([0xC3; 32])).unwrap().unwrap();
        assert_eq!((c3.born_value, c3.template), (0, None));
    }

    #[test]
    fn lane_dashboard_buckets_and_recent() {
        let mut store = test_store("lane-dashboard");
        let ns = "deadbeef".to_string();
        let mut lane_payload = hex::decode(&ns).unwrap();
        lane_payload.extend_from_slice(&[0u8; 16]);
        let ev = |cov: u8, tx: u8, lane: Option<&str>| NewEvent {
            covenant_id: CovenantId([cov; 32]),
            kind: EventKind::Transition,
            txid: TxId([tx; 32]),
            payload: Some(lane_payload.clone()),
            lane_namespace: lane.map(str::to_string),
        };
        // daa 100: two lane events (two covenants) + one foreign-lane event.
        let mut b1 = BlockEvents::empty(BlockHash([1; 32]));
        b1.accepting_daa = 100;
        b1.events = vec![ev(1, 1, Some(&ns)), ev(2, 2, Some(&ns)), ev(3, 3, Some("cafebabe"))];
        store.apply(&b1, BlockHash([1; 32])).unwrap();
        // daa 150: same bucket (width 100) as 100.
        let mut b2 = BlockEvents::empty(BlockHash([2; 32]));
        b2.accepting_daa = 150;
        b2.events = vec![ev(1, 4, Some(&ns))];
        store.apply(&b2, BlockHash([2; 32])).unwrap();
        // daa 250: next bucket. Also a non-lane event that must not count.
        let mut b3 = BlockEvents::empty(BlockHash([3; 32]));
        b3.accepting_daa = 250;
        b3.events = vec![ev(2, 5, Some(&ns)), ev(9, 6, None)];
        store.apply(&b3, BlockHash([3; 32])).unwrap();

        assert_eq!(store.lane_stats(&ns).unwrap(), (4, 2));
        assert_eq!(store.lane_activity(&ns, 100).unwrap(), vec![(100, 3), (200, 1)]);
        let recent = store.lane_recent(&ns, 2).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].txid, TxId([5; 32])); // newest first
        assert_eq!(recent[0].accepting_daa, 250);
        // unknown lane: empty, not an error
        assert_eq!(store.lane_stats("00000000").unwrap(), (0, 0));
        assert!(store.lane_activity("00000000", 100).unwrap().is_empty());
        assert!(store.lane_recent("00000000", 10).unwrap().is_empty());
    }

    #[test]
    fn spent_by_txid_returns_witness() {
        let mut store = test_store("spent-by-txid");
        let outpoint = Outpoint { txid: TxId([0x10; 32]), index: 0 };
        let mut b1 = BlockEvents::empty(BlockHash([1; 32]));
        b1.accepting_daa = 100;
        b1.created_utxos = vec![NewUtxo {
            outpoint,
            covenant_id: CovenantId([0xA1; 32]),
            value: 5_000,
            spk_version: 1,
            spk_script: vec![0xaa, 0x20],
        }];
        store.apply(&b1, BlockHash([1; 32])).unwrap();

        let spender = TxId([0x20; 32]);
        assert!(store.spent_by_txid(&spender).unwrap().is_empty());

        let mut b2 = BlockEvents::empty(BlockHash([2; 32]));
        b2.accepting_daa = 200;
        b2.spent_utxos = vec![(outpoint, spender, vec![0x01, 0x51], 60)];
        store.apply(&b2, BlockHash([2; 32])).unwrap();

        let rows = store.spent_by_txid(&spender).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].covenant_id, CovenantId([0xA1; 32]));
        assert_eq!(rows[0].outpoint, outpoint);
        assert_eq!(rows[0].value, 5_000);
        assert_eq!(rows[0].spk_script, vec![0xaa, 0x20]);
        assert_eq!(rows[0].spent_sig.as_deref(), Some([0x01, 0x51].as_slice()));
        assert_eq!(rows[0].spent_budget, Some(60));
    }
}
