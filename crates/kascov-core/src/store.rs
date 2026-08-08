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
    -- 0-based index of the tx within its accepting chain block's accepted-tx
    -- list (node acceptance order = UTXO application order). NULL on rows
    -- written before capture / beyond node retention.
    tx_index INTEGER,
    -- Accepting chain-block header fields, captured with tx_index (free — the
    -- header is already fetched). NULL on pre-capture rows: readers fall back
    -- to DAA estimates (time) / accepting_daa (ordering).
    accepting_time_ms INTEGER,
    accepting_blue_score INTEGER,
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
-- KCC20 token registry, derived deterministically from covenant_events +
-- covenant_utxos by TOKEN_DERIVATION_VERSION (see tokens.rs). status is the
-- validator verdict: 'verified' only when every event in the token's history
-- matched a known rule with every state hash-proven; anything unknown or
-- ambiguous is 'unvalidated' with a reason. Never a false 'verified'.
CREATE TABLE IF NOT EXISTS tokens (
    token_id BLOB PRIMARY KEY,            -- = the KCC20 token covenant's covenant_id
    status TEXT NOT NULL DEFAULT 'unvalidated',  -- verified | invalid | unvalidated
    invalid_reason TEXT,                  -- first failing/ambiguous check; NULL when verified
    supply INTEGER,                       -- genesis + mints - burns; NULL = not provable
    minted INTEGER,                       -- cumulative proven mints; NULL = not provable
    burned INTEGER,                       -- cumulative proven burns; NULL = not provable
    holders INTEGER NOT NULL DEFAULT 0,   -- distinct owners across live hash-proven cells
    -- Where the proven supply actually sits, by decoded owner type. For a
    -- bonding-curve token, covenant-held is the curve's unsold inventory (and
    -- the locked pool after graduation); wallet-held is what people hold.
    -- NULL when supply itself is not provable, or when the parts do not sum.
    held_covenant INTEGER,                -- owner type 0x02
    held_wallet INTEGER,                  -- owner types 0x00 and 0x03
    held_script INTEGER,                  -- owner type 0x01
    unresolved_cells INTEGER NOT NULL DEFAULT 0, -- live cells whose state is unproven
    last_activity_daa INTEGER NOT NULL DEFAULT 0,
    fields_json TEXT,                     -- latest proven state fields (label -> hex)
    derived_at_daa INTEGER                -- provenance: processed_daa when last derived
);
CREATE INDEX IF NOT EXISTS tok_by_activity ON tokens(last_activity_daa DESC, token_id DESC);
-- minter/vault covenant -> governed token, from the reveal's pinned ids
CREATE TABLE IF NOT EXISTS token_minters (
    minter_covenant_id BLOB NOT NULL,
    token_id BLOB NOT NULL,
    PRIMARY KEY (minter_covenant_id, token_id)
);
CREATE INDEX IF NOT EXISTS tok_minter_by_token ON token_minters(token_id);
-- per covenant-event token deltas: (covenant_id, seq) is the FK into
-- covenant_events; delta_idx fans out multi-output events. kind is the token
-- classification (genesis|mint|transfer|split|merge|burn|unknown); amount is
-- NULL exactly when the event could not be proven.
CREATE TABLE IF NOT EXISTS token_events (
    token_id BLOB NOT NULL,
    covenant_id BLOB NOT NULL,
    seq INTEGER NOT NULL,
    delta_idx INTEGER NOT NULL,
    kind TEXT NOT NULL,
    amount INTEGER,
    owner_from TEXT,                      -- hex(identifier_type || owner_identifier)
    owner_to TEXT,
    accepting_daa INTEGER NOT NULL,       -- copied from the event row
    tx_index INTEGER,                     -- copied; NULL on pre-capture rows
    PRIMARY KEY (token_id, covenant_id, seq, delta_idx)
);
CREATE INDEX IF NOT EXISTS tev_by_event ON token_events(covenant_id, seq);
CREATE INDEX IF NOT EXISTS tev_by_token_time ON token_events(token_id, accepting_daa, tx_index);
-- One admitted trade: this token's cells moved against the market covenant's
-- KAS in a single transaction, with both sides hash-proven. RAW CHAIN FACTS
-- ONLY — no bracket verdict, no skeleton, no window aggregate lives here, so
-- the table stays a pure function of this token's own proven states plus
-- covenant_utxos values, and the publish/suppress decision can change without
-- rewriting history. base_amount and quote_sompi are the integer price PAIR;
-- nothing downstream is ever allowed to collapse them into a float.
CREATE TABLE IF NOT EXISTS token_trades (
    token_id BLOB NOT NULL,
    seq INTEGER NOT NULL,                 -- covenant_events.seq of this token
    txid BLOB NOT NULL,
    market_covenant_id BLOB NOT NULL,     -- the counterparty covenant on THIS trade
    side TEXT NOT NULL,                   -- 'buy' | 'sell' (taker's view)
    base_amount INTEGER NOT NULL,         -- tokens moved, > 0
    quote_sompi INTEGER NOT NULL,         -- KAS moved, > 0
    kas_before_sompi INTEGER NOT NULL,
    kas_after_sompi INTEGER NOT NULL,
    base_before INTEGER NOT NULL,
    base_after INTEGER NOT NULL,
    co_covenants INTEGER NOT NULL,        -- other covenants moved by this tx
    -- WHO traded: hex(identifier_type || owner_identifier) of the single
    -- non-covenant owner whose token delta opposed the market's. NULL when
    -- that is ambiguous (several key owners moved, e.g. a batched settlement),
    -- because naming the wrong trader is worse than naming none.
    counterparty TEXT,
    accepting_daa INTEGER NOT NULL,
    accepting_blue_score INTEGER,
    tx_index INTEGER,
    accepting_time_ms INTEGER,            -- NULL on pre-capture rows: windows fail closed
    PRIMARY KEY (token_id, seq)
);
CREATE INDEX IF NOT EXISTS tt_by_token_order ON token_trades(token_id, seq DESC);
CREATE INDEX IF NOT EXISTS tt_by_token_time  ON token_trades(token_id, accepting_time_ms);
CREATE INDEX IF NOT EXISTS tt_by_market      ON token_trades(market_covenant_id);
-- What kascov could read out of a market covenant's own hash-committed
-- program bytes: the skeleton it matched, the curve constants, and how far
-- the two-sided invariant replay has checked its trades. A row here is what
-- LICENSES publishing a price at all — no recognised program, no price.
CREATE TABLE IF NOT EXISTS market_programs (
    covenant_id BLOB PRIMARY KEY,
    program_hash BLOB NOT NULL,
    skeleton TEXT NOT NULL,               -- e.g. 'KRON curve v1' | 'unmatched'
    v_kas_units INTEGER NOT NULL DEFAULT 0,
    token_reserve INTEGER,
    token_covenant_id BLOB,
    lp_token_covenant_id BLOB,
    kas_reserve_sompi INTEGER,
    shares INTEGER,
    graduation_kas_sompi INTEGER,
    fee_bps_json TEXT,
    fee_owners_json TEXT,
    pool_template_hash BLOB,
    state_proved_txid BLOB,
    state_proved_index INTEGER,
    invariant_checked_through_seq INTEGER NOT NULL DEFAULT -1,
    invariant_trades INTEGER NOT NULL DEFAULT 0,
    invariant_ok INTEGER NOT NULL DEFAULT 0,
    exercised_trades INTEGER NOT NULL DEFAULT 0,
    wedge_bps INTEGER,
    derived_at_daa INTEGER,
    -- STRUCTURAL FINGERPRINT. A curve program embeds its own token's constants
    -- (covenant id, reserve, creator, vKas) directly in its bytes, so every
    -- deployment is byte-unique and program_hash can never group two of them.
    -- These two numbers are what deployments of one build DO share, so they
    -- cluster a launchpad's family the way an exact hash cannot.
    --
    -- They are a WEAK signal by construction: matching structure means it is
    -- worth looking at these together, never that they are the same audited
    -- build. Only the byte-for-byte matcher may license a price. Appended at
    -- the END because these r.get indices are positional.
    program_len INTEGER,
    program_pushes INTEGER
);
CREATE INDEX IF NOT EXISTS mp_by_token ON market_programs(token_covenant_id);
-- A resting limit order read off a proof-grade reveal of its own program
-- bytes: one offer at one price, filled once and gone. Rows are DECODED
-- FACTS about committed bytes — nothing may publish a price from this table
-- without its own verification gate (market_programs' bar applies here too).
-- A whole new table reaches deployed databases through SCHEMA itself:
-- execute_batch runs on every open and IF NOT EXISTS creates it when absent
-- (only new COLUMNS need the ALTER list below).
CREATE TABLE IF NOT EXISTS resting_orders (
    covenant_id BLOB PRIMARY KEY,
    token_id BLOB NOT NULL,
    -- This build family only encodes an ask — the maker parcels tokens and
    -- names the sompi wanted. A bid would be a different program shape, so
    -- 'sell' is the only value written today; the column exists because the
    -- shape of a book is not the shape of one family.
    side TEXT NOT NULL,
    -- The offer as the exact pair the bytes commit to: total sompi asked
    -- over tokens offered. A ratio, never a quotient — integer division
    -- would collapse distinct price levels and a float would round them.
    price_num INTEGER NOT NULL,
    price_den INTEGER NOT NULL,
    amount INTEGER NOT NULL,
    maker BLOB NOT NULL,
    expiry_daa INTEGER NOT NULL,
    -- 'open' | 'filled' | 'cancelled'. Resolution is the spend that leaves
    -- the covenant with no live cell; the filled/cancelled label applies the
    -- program's own committed expiry to that spend's accepting DAA (kascov
    -- does not decode which branch the spend ran — see derive_resting_order).
    state TEXT NOT NULL,
    created_daa INTEGER NOT NULL,
    resolved_daa INTEGER
);
-- The live book of one token. GLOB-free equality predicate: 'open' rows are
-- the only ones a book enumeration ever wants, and they are the minority
-- once orders start resolving.
CREATE INDEX IF NOT EXISTS ro_open_by_token ON resting_orders(token_id) WHERE state = 'open';
CREATE TABLE IF NOT EXISTS token_balances (
    token_id BLOB NOT NULL,
    owner TEXT NOT NULL,                  -- hex(identifier_type || owner_identifier)
    balance INTEGER NOT NULL,
    cells INTEGER NOT NULL DEFAULT 0,     -- live proven cells backing this balance
    PRIMARY KEY (token_id, owner)
);
CREATE INDEX IF NOT EXISTS bal_top ON token_balances(token_id, balance DESC);

-- The verification log: one row per derivation PASS over this network's
-- database. A pass is either a rebuild of the derived token tables ('full')
-- or a re-verification of every linked market program ('markets'). The
-- per-block incremental path is deliberately NOT logged: it runs on every
-- block that touches a token and would drown the record it exists to keep.
--
-- This table is a RECORD, never an authority. Nothing may read a counter
-- here to decide whether a figure may be published — the publish gate stays
-- market_programs' own committed bytes, re-read every time. A cached verdict
-- is exactly the kind of trust kascov refuses.
CREATE TABLE IF NOT EXISTS derivation_runs (
    -- AUTOINCREMENT, not a bare rowid alias: pruning deletes the oldest rows,
    -- and a log whose ids could be reused would renumber its own history.
    run_id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,                   -- 'full' | 'markets'
    -- NULL while in flight. 'ok' | 'degraded' (finished, market verification
    -- errored) | 'failed' (the pass itself errored) | 'interrupted' (the row
    -- was still open when a later pass started, so the process is gone).
    outcome TEXT,
    started_ms INTEGER NOT NULL,          -- host wall clock, never chain time
    finished_ms INTEGER,
    processed_daa INTEGER,                -- chain anchor at the start
    stamp TEXT NOT NULL,                  -- the composite stamp in force
    tokens_examined INTEGER NOT NULL DEFAULT 0,
    tokens_verified INTEGER NOT NULL DEFAULT 0,
    tokens_unvalidated INTEGER NOT NULL DEFAULT 0,
    tokens_invalid INTEGER NOT NULL DEFAULT 0,
    tokens_added INTEGER NOT NULL DEFAULT 0,
    tokens_removed INTEGER NOT NULL DEFAULT 0,
    verdicts_changed INTEGER NOT NULL DEFAULT 0,
    markets_examined INTEGER NOT NULL DEFAULT 0,
    markets_matched INTEGER NOT NULL DEFAULT 0,
    markets_unmatched INTEGER NOT NULL DEFAULT 0,
    -- No market_programs row at all: the covenant has never been spent with a
    -- proof-grade reveal, so its bytes have never been read. Deliberately NOT
    -- folded into markets_unmatched: did-not-match and was-never-seen are
    -- different states and the log must not collapse them.
    markets_unrevealed INTEGER NOT NULL DEFAULT 0,
    markets_invariant_failed INTEGER NOT NULL DEFAULT 0,
    changes_json TEXT,                    -- capped; counters above stay exact
    error TEXT
);

-- The unknown-build queue reads this. GLOB, never LIKE: with
-- case_sensitive_like ON, SQLite treats like() as non-deterministic and
-- REFUSES TO PARSE a schema containing it in a partial-index WHERE, which
-- fails every query on that connection, not just this table's. GLOB is
-- always binary and therefore always deterministic.
CREATE INDEX IF NOT EXISTS mp_unknown ON market_programs(program_hash, covenant_id)
    WHERE skeleton GLOB 'unmatched*';
";

pub struct Store {
    conn: Connection,
}

/// One decoded token an address holds.
#[derive(Clone, Debug, Serialize)]
pub struct TokenHoldingRow {
    pub token_id: CovenantId,
    /// 'pubkey' or 'presence' — the same key, different authorization.
    pub owner_kind: String,
    pub balance: i64,
    pub cells: i64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supply: Option<i64>,
}

/// One derivation pass, as published. A record of what ran, never an
/// authority on what may be published.
#[derive(Clone, Debug, Serialize)]
pub struct DerivationRunRow {
    pub run_id: i64,
    pub kind: String,
    pub outcome: Option<String>,
    pub started_ms: i64,
    pub finished_ms: Option<i64>,
    pub processed_daa: Option<i64>,
    pub stamp: String,
    pub tokens_examined: i64,
    pub tokens_verified: i64,
    pub tokens_unvalidated: i64,
    pub tokens_invalid: i64,
    pub tokens_added: i64,
    pub tokens_removed: i64,
    pub verdicts_changed: i64,
    pub markets_examined: i64,
    pub markets_matched: i64,
    pub markets_unmatched: i64,
    pub markets_unrevealed: i64,
    pub markets_invariant_failed: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A program kascov could not match, and how much rides on it. A to-audit
/// entry: nothing here has proven anything.
#[derive(Clone, Debug, Serialize)]
pub struct UnknownBuildRow {
    /// The shape these deployments share: byte length and push count. This is
    /// what clusters a launchpad's family, because each deployment's BYTES are
    /// unique (its own constants are baked in) while its SHAPE is not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_len: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_pushes: Option<i64>,
    /// One member's hash, so a reader can go fetch and check actual bytes.
    pub program_hash: String,
    pub covenants: i64,
    pub trades: i64,
    pub volume_sompi: i64,
    pub tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_daa: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_daa: Option<i64>,
    pub sample_covenant: String,
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
    /// 0-based index in the accepting block's accepted-tx list (consensus
    /// acceptance order). None on rows written before capture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_index: Option<u64>,
    /// The transaction's v1 payload, when it carried one.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "opt_hex_ser"
    )]
    pub payload: Option<Vec<u8>>,
    /// Accepting block header timestamp (ms) — real chain time, not a DAA
    /// estimate. None on rows written before capture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepting_time_ms: Option<u64>,
    /// Accepting block blue score: with tx_index it totally orders events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepting_blue_score: Option<u64>,
}

fn opt_hex_ser<S: serde::Serializer>(
    bytes: &Option<Vec<u8>>,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
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

/// Token identity a deployer CLAIMED in the genesis payload (the KCC-0021
/// shape). Every field is an unsigned, non-unique assertion by whoever authored
/// that transaction: the covenant id stays the canonical identity, and callers
/// must render these with that provenance rather than as verified facts.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ClaimedTokenMeta {
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub image: Option<String>,
    pub image_hash: Option<String>,
    /// Display scale only, never applied to the integers kascov verifies.
    /// `None` means the deployer declared none, which KCC-0021 reads as 0
    /// (raw base units) rather than "unknown".
    pub decimals: Option<u8>,
}

/// An event joined with its covenant, for cross-covenant feeds.
#[derive(Clone, Debug, Serialize)]
pub struct GlobalEventRow {
    pub covenant_id: CovenantId,
    pub seq: u64,
    pub kind: String,
    pub txid: TxId,
    pub accepting_daa: u64,
    /// 0-based index in the accepting block's accepted-tx list (consensus
    /// acceptance order). None on rows written before capture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_index: Option<u64>,
}

/// The canonical event object every cross-covenant API surface serves — one
/// shape, so consumers write one parser. `tx_index`/`payload_len` are omitted
/// (not null) when unknown/absent.
#[derive(Clone, Debug, Serialize)]
pub struct FeedEventRow {
    pub covenant_id: CovenantId,
    pub seq: u64,
    pub kind: String,
    pub txid: TxId,
    pub accepting_daa: u64,
    pub accepting_block: BlockHash,
    /// 0-based index in the accepting block's accepted-tx list (consensus
    /// acceptance order). None on rows written before capture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_index: Option<u64>,
    /// Byte length of the tx's v1 payload; None when it carried none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_len: Option<u64>,
}

/// What a caller-facing unsubscribe attempt did (see
/// [`Store::delete_subscription_secured`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsubscribeOutcome {
    Deleted,
    NotFound,
    /// The row carries a secret and the caller's didn't match.
    WrongSecret,
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

/// One covenant event a single transaction fired — the tx-scoped inverse of
/// [`Store::events`], without the payload bytes (the tx endpoint links out;
/// the covenant detail carries the heavy fields).
#[derive(Clone, Debug, Serialize)]
pub struct TxEventRow {
    pub covenant_id: CovenantId,
    pub seq: u64,
    pub kind: String,
    pub accepting_block: BlockHash,
    pub accepting_daa: u64,
    /// 0-based index in the accepting block's accepted-tx list (consensus
    /// acceptance order). None on rows written before capture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_index: Option<u64>,
}

/// A state cell one transaction created. `template: None` covers both
/// not-yet-decoded and decoded-without-a-match ('' in storage) — the tx view
/// only names a shape when one is actually known.
#[derive(Clone, Debug, Serialize)]
pub struct TxCreatedCellRow {
    pub covenant_id: CovenantId,
    pub index: u32,
    pub value: u64,
    pub template: Option<String>,
}

/// A state cell one transaction spent — shape hints only; the full witness
/// bytes stay on [`Store::spent_by_txid`] (the debugger's feed).
#[derive(Clone, Debug, Serialize)]
pub struct TxSpentCellRow {
    pub covenant_id: CovenantId,
    pub txid: TxId,
    pub index: u32,
    pub value: u64,
    /// Verified P2SH program revealed by this spend, when one matched.
    pub revealed_template: Option<String>,
    /// Whether the spend's unlocking script was captured.
    pub has_witness: bool,
    /// The spending input's 0-based index (KCC-1 leader/delegator ordering).
    /// None on rows spent before capture — role stays unknown, never guessed.
    pub input_index: Option<u32>,
    /// KCC-1 §8.3 TemplateHash of the revealed program (proven state range
    /// only). None = no hash or not yet stamped.
    pub kcc1_template_hash: Option<[u8; 32]>,
}

/// One classified token delta a transaction produced.
#[derive(Clone, Debug, Serialize)]
pub struct TxTokenActionRow {
    pub token_id: CovenantId,
    /// genesis | mint | transfer | split | merge | burn | unknown
    pub kind: String,
    /// None exactly when the delta could not be proven.
    pub amount: Option<i64>,
}

/// Events produced while processing one accepting chain block, applied atomically.
pub struct BlockEvents {
    pub accepting_block: BlockHash,
    pub accepting_daa: u64,
    /// Accepting block header timestamp (ms) — real chain time for events.
    pub accepting_time_ms: u64,
    /// Accepting block blue score — the strictly-increasing chain key that
    /// makes (blue_score, tx_index) a total order over transactions.
    pub accepting_blue_score: u64,
    pub events: Vec<NewEvent>,
    pub created_utxos: Vec<NewUtxo>,
    /// (outpoint, spending txid, spending input's signature script, budget,
    /// spending input's 0-based index — the KCC-1 leader/delegator ordering)
    pub spent_utxos: Vec<(Outpoint, TxId, Vec<u8>, u16, u32)>,
}

impl BlockEvents {
    pub fn empty(accepting_block: BlockHash) -> Self {
        Self {
            accepting_block,
            accepting_daa: 0,
            accepting_time_ms: 0,
            accepting_blue_score: 0,
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
    /// 0-based index of the tx in the accepting block's accepted-tx list —
    /// the node's acceptance order, which is the UTXO application order.
    pub tx_index: u32,
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

/// Tallies from [`Store::merge_recovered_block`] — what a gap-recovery merge
/// actually changed (dedup makes these smaller than what was offered).
#[derive(Clone, Copy, Debug, Default)]
pub struct MergeCounts {
    pub events_added: u64,
    pub utxos_added: u64,
    pub spends_repaired: u64,
}

impl MergeCounts {
    pub fn add(&mut self, other: MergeCounts) {
        self.events_added += other.events_added;
        self.utxos_added += other.utxos_added;
        self.spends_repaired += other.spends_repaired;
    }
}

/// What [`Store::finalize_gap_recovery`] touched.
#[derive(Clone, Copy, Debug, Default)]
pub struct FinalizeCounts {
    pub covenants_refreshed: u64,
    pub covenants_resequenced: u64,
    pub tokens_rederived: u64,
}

pub(crate) fn db_err(e: rusqlite::Error) -> Error {
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
        let run: Vec<u8> = payload
            .iter()
            .copied()
            .take_while(|b| b.is_ascii_hexdigit())
            .collect();
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

/// How much of a payload the inscription decoder looks at. Was 512 bytes;
/// real TN10 inscriptions (batched genesis0 mints, KCC20V3Wrapped orders)
/// routinely push JSON past that, truncating the parse to `''`. Every window
/// user (write-time stamp, backfill, legacy scan) must share this constant,
/// and widening it needs a `CLASSIFIER_VERSION` bump so stale `''` stamps on
/// longer payloads get re-derived.
const INSCRIPTION_WINDOW: usize = 2048;

/// Decode a payload's inscription label for the precomputed stamp — the same
/// leading-window parse the legacy `inscription_breakdown` scan used per
/// row. `''` when the payload isn't a parseable inscription.
fn inscription_kind_of(payload: &[u8]) -> String {
    let head = &payload[..payload.len().min(INSCRIPTION_WINDOW)];
    extract_inscription_json(head)
        .map(|v| inscription_kind(&v))
        .unwrap_or_default()
}

/// Process-wide decode registry for write-time template recognition —
/// construction derives the SilverScript skeletons once, and Registry is
/// Send + Sync (its decoders are `Box<dyn StateDecoder: Send + Sync>`).
pub(crate) fn registry() -> &'static kascov_decode::Registry {
    static REGISTRY: std::sync::OnceLock<kascov_decode::Registry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(kascov_decode::Registry::default)
}

/// Derive (or retract) the resting-order row of one covenant from what the
/// chain proves right now. The oldest proof-grade reveal that
/// `market::match_kcm_order` accepts is the posted offer, and its cell's
/// created_daa is the posting time; the order is resolved once the covenant
/// has no live cell left, and the resolving DAA is read from the spend
/// events of its cells (classify() guarantees every spent covenant UTXO
/// produced one). Idempotent — INSERT OR REPLACE recomputes the whole row —
/// so apply() and rollback() both call it after their writes land.
///
/// Fail-closed at every fork: bytes the matcher refuses leave no row, a row
/// whose proving reveal was rolled back is deleted (a decoded fact never
/// outlives the bytes that proved it), and a consumed order whose resolving
/// spend left no accepting DAA on record is deleted rather than labeled by
/// guesswork.
fn derive_resting_order(conn: &Connection, covenant_id: &[u8; 32]) -> Result<()> {
    // Oldest-first: the first cell whose reveal decodes as an order carries
    // the posting DAA. LIMIT bounds the scan; this family's lifecycle is one
    // or two cells (posted, consumed), so 8 is generous.
    let mut stmt = conn
        .prepare_cached(
            "SELECT spk_script, spent_sig, created_daa FROM covenant_utxos
             WHERE covenant_id = ?1 AND spent_sig IS NOT NULL
             ORDER BY created_daa ASC LIMIT 8",
        )
        .map_err(db_err)?;
    let rows: Vec<(Vec<u8>, Vec<u8>, i64)> = stmt
        .query_map([covenant_id.as_slice()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map_err(db_err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_err)?;
    let mut posted: Option<(crate::market::OrderParams, i64)> = None;
    for (spk, sig, created_daa) in &rows {
        if let Some(program) = kascov_decode::p2sh_reveal(spk, sig) {
            if let Some(order) = crate::market::match_kcm_order(&program) {
                posted = Some((order, *created_daa));
                break;
            }
        }
    }
    let Some((order, created_daa)) = posted else {
        conn.execute(
            "DELETE FROM resting_orders WHERE covenant_id = ?1",
            [covenant_id.as_slice()],
        )
        .map_err(db_err)?;
        return Ok(());
    };

    let live: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM covenant_utxos
             WHERE covenant_id = ?1 AND spent_block IS NULL)",
            [covenant_id.as_slice()],
            |r| r.get(0),
        )
        .map_err(db_err)?;
    let resolved_daa: Option<i64> = if live {
        None
    } else {
        // MAX because the spend that consumed the LAST live cell is the one
        // that resolved the order.
        conn.query_row(
            "SELECT MAX(e.accepting_daa) FROM covenant_events e
             WHERE e.covenant_id = ?1
               AND e.txid IN (SELECT spent_txid FROM covenant_utxos
                              WHERE covenant_id = ?1 AND spent_txid IS NOT NULL)",
            [covenant_id.as_slice()],
            |r| r.get(0),
        )
        .map_err(db_err)?
    };
    let state = match resolved_daa {
        None if live => "open",
        // The program's own committed expiry read against the resolving
        // spend's accepting DAA: at or before expiry only the fill branch is
        // live, after it the reclaim is (OrderParams::expiry_daa is "after
        // which anyone may return the parcel"). The spend's branch itself is
        // not decoded — this is the committed schedule against a chain fact.
        Some(daa) if daa <= order.expiry_daa => "filled",
        Some(_) => "cancelled",
        // Consumed, but no spend event carries a DAA to place the
        // resolution: no state can be proven, so no row is served at all.
        None => {
            conn.execute(
                "DELETE FROM resting_orders WHERE covenant_id = ?1",
                [covenant_id.as_slice()],
            )
            .map_err(db_err)?;
            return Ok(());
        }
    };
    conn.execute(
        // price_den duplicates amount today because this family commits a
        // total-for-parcel price; a family with a per-token price would
        // diverge, so both columns stay.
        "INSERT OR REPLACE INTO resting_orders
           (covenant_id, token_id, side, price_num, price_den, amount, maker,
            expiry_daa, state, created_daa, resolved_daa)
         VALUES (?1, ?2, 'sell', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            covenant_id.as_slice(),
            order.token_covenant_id.as_slice(),
            order.price_sompi,
            order.size,
            order.size,
            order.maker.as_slice(),
            order.expiry_daa,
            state,
            created_daa,
            resolved_daa,
        ],
    )
    .map_err(db_err)?;
    Ok(())
}

/// Version of the write-time classification (decode registry + inscription
/// window). Bump whenever either learns something new, so stamps an older
/// binary left as *generic* get cleared back to NULL on open and the
/// backfills re-derive them; rows the old classifier gave a real name keep
/// it. Version 2: observed-family skeletons (genesis0 / PURE / KCC20) and
/// the 512 B → 2 KiB inscription window.
pub(crate) const CLASSIFIER_VERSION: &str = "2";
/// Bump when the state-block locator learns a new build (the restamp pass's
/// version gate). Exported so the token-derivation stamp can compose it: a
/// decoder learning a build MUST invalidate every stored price.
pub(crate) const KCC20_RESTAMP_VERSION: &str = "1-locate-state-block";

/// KCC-1 spec commit the §8.3 TemplateHash derivation is pinned to. Bump on
/// any change to the derivation (spec churn or state-range coverage) — see
/// rehash_kcc1_if_stale.
const KCC1_ABI_VERSION: &str = "55b28d8";

/// Whether opening may create a brand-new database when none exists at the
/// path. SQLite happily makes one, which for the sole archive of a chain's
/// history is amnesia served as health: every count the empty file then
/// publishes is honestly derived from nothing. Boot paths that own real
/// data pass `Refuse`; first-time setups and the test suite pass `Allow`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreshDb {
    Allow,
    Refuse,
}

impl Store {
    pub fn open(path: &Path, network: Network) -> Result<Self> {
        Self::open_with_policy(path, network, FreshDb::Allow)
    }

    /// `FreshDb::Refuse` fails loudly when the file is missing or zero bytes
    /// (SQLite treats both as "make me a fresh one") instead of booting an
    /// empty archive. The refusal names KASCOV_FRESH_OK because the operator
    /// escape hatch lives at the boot path that maps it to `Allow`; this
    /// constructor never reads the environment itself. A stat failure counts
    /// as missing: when the file's existence is in doubt, so is the archive.
    pub fn open_with_policy(path: &Path, network: Network, fresh: FreshDb) -> Result<Self> {
        if fresh == FreshDb::Refuse && std::fs::metadata(path).map_or(true, |m| m.len() == 0) {
            return Err(Error::Invalid {
                what: "db open",
                value: format!(
                    "{} does not exist (or is zero bytes) and this boot refuses to start a \
                     fresh database in its place — an empty archive would serve zeros as \
                     verified history. If a brand-new database is really intended, set \
                     KASCOV_FRESH_OK=1.",
                    path.display()
                ),
            });
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Invalid {
                what: "db path",
                value: e.to_string(),
            })?;
        }
        let conn = Connection::open(path).map_err(db_err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(db_err)?;
        // Concurrent readers (backup, serve snapshots) must wait out write
        // bursts instead of failing with SQLITE_BUSY.
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .map_err(db_err)?;
        conn.execute_batch(SCHEMA).map_err(db_err)?;
        // Additive migrations for pre-existing databases (SQLite has no
        // ADD COLUMN IF NOT EXISTS; a duplicate-column error means done).
        // Only ignore SQLITE_ERROR (1) with "duplicate column" — re-raise
        // genuine failures like disk-full, I/O errors, or database corruption.
        let migrations = [
            // Structural fingerprint of a market program. New columns in
            // SCHEMA only reach FRESH databases: CREATE TABLE IF NOT EXISTS is
            // a no-op on an existing one, so every deployed database needs the
            // ALTER too, and any index over them must come AFTER it.
            "ALTER TABLE token_trades ADD COLUMN counterparty TEXT",
            "CREATE INDEX IF NOT EXISTS tt_by_counterparty ON token_trades(counterparty, seq DESC)",
            "ALTER TABLE market_programs ADD COLUMN program_len INTEGER",
            "ALTER TABLE market_programs ADD COLUMN program_pushes INTEGER",
            "CREATE INDEX IF NOT EXISTS mp_shape ON market_programs(program_len, program_pushes)
                 WHERE skeleton GLOB 'unmatched*'",
            "CREATE INDEX IF NOT EXISTS tt_by_txid ON token_trades(txid)",
            "ALTER TABLE covenant_utxos ADD COLUMN spent_sig BLOB",
            "ALTER TABLE covenant_utxos ADD COLUMN spent_budget INTEGER",
            "ALTER TABLE covenant_events ADD COLUMN payload BLOB",
            "ALTER TABLE covenant_events ADD COLUMN lane_namespace TEXT",
            "ALTER TABLE covenant_utxos ADD COLUMN template TEXT",
            "ALTER TABLE covenant_utxos ADD COLUMN revealed_template TEXT",
            "ALTER TABLE covenant_events ADD COLUMN payload_tag TEXT",
            "ALTER TABLE covenant_events ADD COLUMN inscription_kind TEXT",
            "ALTER TABLE covenant_events ADD COLUMN tx_index INTEGER",
            "ALTER TABLE covenant_events ADD COLUMN accepting_time_ms INTEGER",
            "ALTER TABLE covenant_events ADD COLUMN accepting_blue_score INTEGER",
            "ALTER TABLE webhook_subscriptions ADD COLUMN secret TEXT",
            // Verified token art: bytes fetched from the deployer's claimed
            // image URL and PROVEN against the sha256 committed in the
            // genesis payload. status: verified | mismatch | fetch_failed |
            // too_large | not_image. Only 'verified' rows ever serve.
            "CREATE TABLE IF NOT EXISTS token_image_cache (
                covenant_id BLOB PRIMARY KEY,
                status TEXT NOT NULL,
                content_type TEXT,
                bytes BLOB,
                fetched_ms INTEGER NOT NULL
            )",
            // KCC-1 draft §8.3 TemplateHash of the revealed program, stamped
            // with revealed_template: 32 bytes, x'' = reveal checked / no
            // proven state range, NULL = not yet checked (todo). Derivation
            // pinned via the kcc1_abi_version meta gate below.
            "ALTER TABLE covenant_utxos ADD COLUMN kcc1_template_hash BLOB",
            // 0-based index of the spending input within its tx — the KCC-1
            // leader/delegator ordering. NULL on rows spent before capture
            // (no deep backfill: bodies beyond node retention are gone, the
            // same limitation tx_index documents above).
            "ALTER TABLE covenant_utxos ADD COLUMN spent_input_index INTEGER",
            // Where a proven supply actually sits, split by decoded owner type.
            // Present in the CREATE above for fresh databases; these carry an
            // existing one forward. Values are filled by the next derivation
            // pass, so they read NULL until then, which is the same thing they
            // mean when supply is unprovable: not established.
            "ALTER TABLE tokens ADD COLUMN held_covenant INTEGER",
            "ALTER TABLE tokens ADD COLUMN held_wallet INTEGER",
            "ALTER TABLE tokens ADD COLUMN held_script INTEGER",
            // The trade layer (derivation v7): which covenant is this token's
            // market, how many trades were admitted, how many candidates were
            // rejected for co-moving another token, and how many admitted
            // trades predate timestamp capture (those null every 24h window
            // for the token — a partial window is never published).
            "ALTER TABLE tokens ADD COLUMN market_covenant_id BLOB",
            "ALTER TABLE tokens ADD COLUMN trades INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE tokens ADD COLUMN co_moved_trades INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE tokens ADD COLUMN trades_missing_time INTEGER NOT NULL DEFAULT 0",
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
        conn.execute(
            "CREATE INDEX IF NOT EXISTS utxo_kcc1_todo ON covenant_utxos(kcc1_template_hash) WHERE spent_sig IS NOT NULL AND kcc1_template_hash IS NULL",
            [],
        )
        .map_err(db_err)?;
        // Covering partial index for the live-state probes: SUMMARY_SELECT's
        // COUNT/SUM(value) subqueries filter `spent_block IS NULL` per
        // covenant on every summary row — with (covenant_id, value) covered,
        // both are index-only instead of probe-then-fetch.
        conn.execute(
            "CREATE INDEX IF NOT EXISTS utxo_live ON covenant_utxos(covenant_id, value) WHERE spent_block IS NULL",
            [],
        )
        .map_err(db_err)?;
        // /template/{hash} lookups and per-template aggregates; x'' rows
        // (checked, no hash) are excluded so the index stays hash-only.
        conn.execute(
            "CREATE INDEX IF NOT EXISTS utxo_by_kcc1 ON covenant_utxos(kcc1_template_hash) WHERE kcc1_template_hash IS NOT NULL AND kcc1_template_hash <> x''",
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
        // KCC20 candidate enumeration for the token derivation pass: the
        // predicate must stay verbatim-identical to the WHERE in
        // derive_tokens_if_stale so the partial index covers it. References
        // ALTER-added columns, so it lives here, never in SCHEMA.
        conn.execute(
            "CREATE INDEX IF NOT EXISTS utxo_kcc20 ON covenant_utxos(covenant_id)
             WHERE template IN ('KCC20 token','KCC20 minter')
                OR revealed_template IN ('KCC20 token','KCC20 minter')",
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
        // After the ownership check — a wrong-network database is never
        // mutated. Stale generic stamps are cleared before the backfills so
        // one open re-derives them with the current classifier.
        store.reclassify_if_stale()?;
        store.rehash_kcc1_if_stale()?;
        store.backfill_templates()?;
        store.backfill_payload_tags()?;
        store.backfill_kcc1_hashes()?;
        Ok(store)
    }

    /// On a classifier-version bump, clear the stamps the old classifier
    /// left as *generic* back to NULL — the "not yet decoded" state the
    /// on-open backfills re-derive from — then record the current version.
    /// Real recognized names are never touched. Idempotent (clearing rows
    /// the current classifier also stamps generic just re-derives the same
    /// value) and cheap: the clears are three linear passes gated to run
    /// once per version, and rows that stay NULL-free afterwards cost the
    /// usual O(1) todo-index probes.
    fn reclassify_if_stale(&mut self) -> Result<()> {
        if self.meta("classifier_version")?.as_deref() == Some(CLASSIFIER_VERSION) {
            return Ok(());
        }
        let tx = self.conn.transaction().map_err(db_err)?;
        // State scripts nothing matched ('' = decoded, no template).
        tx.execute(
            "UPDATE covenant_utxos SET template = NULL WHERE template = ''",
            [],
        )
        .map_err(db_err)?;
        // Spends whose committed P2SH program the old registry couldn't name
        // (or could only call a nested commitment). Only canonical P2SH spks
        // can ever reveal, so the '' rows of plain p2pk spends stay put
        // instead of forcing a re-decode of the whole spent set.
        tx.execute(
            "UPDATE covenant_utxos SET revealed_template = NULL
             WHERE spent_sig IS NOT NULL
               AND revealed_template IN ('', 'p2sh commitment')
               AND length(spk_script) = 35 AND substr(spk_script, 1, 1) = x'aa'",
            [],
        )
        .map_err(db_err)?;
        // Payloads whose inscription parse came up empty under the old
        // 512-byte window and that actually extend past it; payload_tag is
        // cleared with it because the pair is always stamped together.
        tx.execute(
            "UPDATE covenant_events SET payload_tag = NULL, inscription_kind = NULL
             WHERE payload IS NOT NULL AND length(payload) > 512 AND inscription_kind = ''",
            [],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('classifier_version', ?1)",
            [CLASSIFIER_VERSION],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(db_err)
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
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
            std::fs::remove_file(out).map_err(|e| Error::Invalid {
                what: "backup path",
                value: e.to_string(),
            })?;
        }
        let path = out.to_string_lossy();
        self.conn
            .execute("VACUUM INTO ?1", [path.as_ref()])
            .map_err(db_err)?;
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
                    .and_then(|redeem| {
                        kascov_decode::kcc20::revealed_template(registry(), *version, &redeem)
                    })
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

    /// The KCC-1 TemplateHash derivation is pinned to a spec commit; bumping
    /// `KCC1_ABI_VERSION` clears every stamp (hashes AND x'' "checked" marks)
    /// so backfill_kcc1_hashes re-derives under the new rules. The spec is a
    /// Draft — this is the cheap recompute path when its §8.3 framing churns.
    fn rehash_kcc1_if_stale(&mut self) -> Result<()> {
        if self.meta("kcc1_abi_version")?.as_deref() == Some(KCC1_ABI_VERSION) {
            return Ok(());
        }
        let tx = self.conn.transaction().map_err(db_err)?;
        tx.execute("UPDATE covenant_utxos SET kcc1_template_hash = NULL WHERE kcc1_template_hash IS NOT NULL", [])
            .map_err(db_err)?;
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('kcc1_abi_version', ?1)",
            [KCC1_ABI_VERSION],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Stamp the KCC-1 §8.3 TemplateHash onto spent rows whose reveal carries
    /// the proven KCC20 state block (the only programs whose state range is
    /// known, not guessed): 32-byte hash, or x'' for "checked, no proven
    /// range". One-shot after the migration, then O(1) probes against the
    /// empty utxo_kcc1_todo partial index — same shape as backfill_templates.
    fn backfill_kcc1_hashes(&mut self) -> Result<()> {
        const BATCH: i64 = 2000;
        let mut checked = 0u64;
        let mut hashed = 0u64;
        loop {
            let rows: Vec<(i64, Vec<u8>, Vec<u8>)> = {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT rowid, spk_script, spent_sig FROM covenant_utxos
                         WHERE spent_sig IS NOT NULL AND kcc1_template_hash IS NULL LIMIT ?1",
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
            for (rowid, spk, sig) in &rows {
                let hash = kascov_decode::p2sh_reveal(spk, sig)
                    .as_deref()
                    .and_then(kascov_decode::kcc20::kcc1_template_hash);
                if hash.is_some() {
                    hashed += 1;
                }
                tx.execute(
                    "UPDATE covenant_utxos SET kcc1_template_hash = ?1 WHERE rowid = ?2",
                    params![hash.as_ref().map(|h| h.as_slice()).unwrap_or(&[]), rowid],
                )
                .map_err(db_err)?;
            }
            tx.commit().map_err(db_err)?;
            checked += rows.len() as u64;
        }
        if checked > 0 {
            let distinct: i64 = self
                .conn
                .query_row(
                    "SELECT COUNT(DISTINCT kcc1_template_hash) FROM covenant_utxos
                     WHERE kcc1_template_hash IS NOT NULL AND kcc1_template_hash <> x''",
                    [],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            tracing::info!(
                "kcc1 backfill: {checked} reveals checked, {hashed} template hashes stamped ({distinct} distinct)"
            );
        }
        Ok(())
    }

    /// Stamp payload_tag + inscription_kind onto event rows that predate the
    /// columns: one-shot after a migration, an O(1) probe against the empty
    /// ev_payload_tag_todo partial index on every open after that. Both
    /// columns are stamped together (see the todo index comment). Only the
    /// leading `INSCRIPTION_WINDOW` payload bytes are fetched — the tag
    /// needs 4 and the inscription decode never reads past that window.
    fn backfill_payload_tags(&mut self) -> Result<()> {
        const BATCH: i64 = 5000;
        let mut stamped = 0u64;
        loop {
            let rows: Vec<(i64, Vec<u8>)> = {
                let mut stmt = self
                    .conn
                    .prepare(&format!(
                        "SELECT rowid, substr(payload, 1, {INSCRIPTION_WINDOW}) FROM covenant_events
                         WHERE payload IS NOT NULL AND payload_tag IS NULL LIMIT ?1",
                    ))
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
        // Fresh KCC20 recognition observed in THIS block, per covenant:
        // (token evidence, minter evidence). Hoisted from the write-time
        // template stamps below so the token hook at the end of the
        // transaction can gate on it with zero extra decodes.
        let mut kcc20_seen: std::collections::HashMap<[u8; 32], (bool, bool)> =
            std::collections::HashMap::new();
        let mut note_kcc20 = |covenant_id: &CovenantId, template: &str| match template {
            "KCC20 token" => kcc20_seen.entry(covenant_id.0).or_default().0 = true,
            "KCC20 minter" => kcc20_seen.entry(covenant_id.0).or_default().1 = true,
            _ => {}
        };
        // Created rows must land BEFORE spends are marked: one accepting chain
        // block can sweep a whole intra-block chain (tx B spending tx A's
        // covenant output), and marking spends first would no-op against the
        // not-yet-inserted row — leaving a zombie "live" UTXO and dropping the
        // captured spend signature.
        for utxo in &block.created_utxos {
            // Recognition is stamped at write time ('' = no template matched)
            // so template analytics stay pure GROUP BYs at read time.
            let template = registry()
                .decode(utxo.spk_version, &utxo.spk_script)
                .template
                .unwrap_or("");
            note_kcc20(&utxo.covenant_id, template);
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
        // Resting-order triggers, collected while the reveals are in hand:
        // covenants whose spend revealed order bytes, and every spent
        // covenant (one with an existing order row may be consumed by a
        // spend that reveals a successor program which is NOT the order).
        let mut order_covenants: std::collections::BTreeSet<[u8; 32]> = Default::default();
        let mut spent_covenants: std::collections::BTreeSet<[u8; 32]> = Default::default();
        for (outpoint, spending_txid, sig, budget, input_index) in &block.spent_utxos {
            // Spend-time recognition: a verified P2SH reveal names the program
            // that actually ran ('' = spend seen, nothing recognized). Reading
            // the row here is safe because created rows land first (above); a
            // row we never indexed matches neither the SELECT nor the UPDATE
            // and self-heals via the backfill at the next open.
            let mut kcc1_hash: Option<[u8; 32]> = None;
            let revealed: Option<String> = tx
                .query_row(
                    "SELECT spk_version, spk_script, covenant_id FROM covenant_utxos
                     WHERE txid = ?1 AND output_index = ?2",
                    params![outpoint.txid.0.as_slice(), outpoint.index],
                    |r| {
                        Ok((
                            r.get::<_, u16>(0)?,
                            r.get::<_, Vec<u8>>(1)?,
                            r.get::<_, [u8; 32]>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(db_err)?
                .map(|(version, spk, covenant_id)| {
                    let redeem = kascov_decode::p2sh_reveal(&spk, sig);
                    kcc1_hash = redeem
                        .as_deref()
                        .and_then(kascov_decode::kcc20::kcc1_template_hash);
                    // The reveal is the only moment an order program's bytes
                    // are proof-grade in hand. Anything the matcher refuses
                    // is NOT an order — no row is ever written on a guess.
                    if redeem
                        .as_deref()
                        .is_some_and(|p| crate::market::match_kcm_order(p).is_some())
                    {
                        order_covenants.insert(covenant_id);
                    }
                    spent_covenants.insert(covenant_id);
                    let template = redeem
                        .and_then(|redeem| {
                            kascov_decode::kcc20::revealed_template(registry(), version, &redeem)
                        })
                        .unwrap_or("");
                    note_kcc20(&CovenantId(covenant_id), template);
                    template.to_string()
                });
            tx.execute(
                "UPDATE covenant_utxos SET spent_block = ?1, spent_txid = ?2, spent_sig = ?3, spent_budget = ?4, revealed_template = ?5, kcc1_template_hash = ?6, spent_input_index = ?7
                 WHERE txid = ?8 AND output_index = ?9",
                params![
                    block.accepting_block.0.as_slice(),
                    spending_txid.0.as_slice(),
                    sig,
                    budget,
                    revealed,
                    kcc1_hash.as_ref().map(|h| h.as_slice()).unwrap_or(&[]),
                    input_index,
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
                "INSERT INTO covenant_events (covenant_id, seq, kind, txid, accepting_block, accepting_daa, payload, lane_namespace, payload_tag, inscription_kind, tx_index, accepting_time_ms, accepting_blue_score)
                 VALUES (?1,
                   (SELECT COALESCE(MAX(seq), -1) + 1 FROM covenant_events WHERE covenant_id = ?1),
                   ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    event.covenant_id.0.as_slice(),
                    event.kind.as_str(),
                    event.txid.0.as_slice(),
                    block.accepting_block.0.as_slice(),
                    block.accepting_daa,
                    event.payload,
                    event.lane_namespace,
                    tag,
                    kind,
                    event.tx_index,
                    block.accepting_time_ms,
                    block.accepting_blue_score
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
        // Resting-order upkeep — after the events land, because the
        // resolved-DAA lookup in derive_resting_order reads THIS block's
        // spend events. Spent covenants without a fresh order reveal only
        // matter when a row already tracks them: one EXISTS probe each, so
        // non-order blocks pay almost nothing.
        for cov in &spent_covenants {
            if order_covenants.contains(cov) {
                continue;
            }
            let tracked: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM resting_orders WHERE covenant_id = ?1)",
                    [cov.as_slice()],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            if tracked {
                order_covenants.insert(*cov);
            }
        }
        for cov in &order_covenants {
            derive_resting_order(&tx, cov)?;
        }
        // Token accounting hook — same transaction, so readers can never see
        // token tables inconsistent with covenant data (and after the
        // processed_daa stamp, so derived_at_daa provenance names THIS
        // block). Touched covenants are the events' covenants (classify()
        // guarantees every created/spent covenant UTXO produces an event)
        // plus fresh KCC20 recognition from the stamps above; the gate per
        // covenant is two point lookups, so non-KCC20 blocks (the
        // overwhelming majority) pay almost nothing.
        {
            let mut touched: std::collections::BTreeSet<[u8; 32]> =
                block.events.iter().map(|e| e.covenant_id.0).collect();
            touched.extend(kcc20_seen.keys().copied());
            let mut tokens_todo: std::collections::BTreeSet<[u8; 32]> = Default::default();
            let mut minters_todo: std::collections::BTreeSet<[u8; 32]> = Default::default();
            for id in &touched {
                let (token_ev, minter_ev) = kcc20_seen.get(id).copied().unwrap_or_default();
                if token_ev || crate::tokens::is_token(&tx, id)? {
                    tokens_todo.insert(*id);
                }
                if minter_ev || crate::tokens::is_minter(&tx, id)? {
                    minters_todo.insert(*id);
                }
            }
            crate::tokens::rederive_affected(&tx, &minters_todo, &tokens_todo)?;
        }
        tx.commit().map_err(db_err)
    }

    /// Undo everything attributed to the given (reorged-out) chain blocks.
    pub fn rollback(&mut self, removed: &[BlockHash]) -> Result<()> {
        let tx = self.conn.transaction().map_err(db_err)?;
        // Token rewind, phase 1: capture the affected covenant set BEFORE the
        // deletes below destroy the rows that carry the mapping. The
        // spent_block term catches the un-reveal case (rolling back a spend
        // deletes the reveal that proved a cell's state).
        let mut affected: std::collections::BTreeSet<[u8; 32]> = Default::default();
        for hash in removed {
            let hash = hash.0.as_slice();
            for sql in [
                "SELECT DISTINCT covenant_id FROM covenant_events WHERE accepting_block = ?1",
                "SELECT DISTINCT covenant_id FROM covenant_utxos WHERE created_block = ?1",
                "SELECT DISTINCT covenant_id FROM covenant_utxos WHERE spent_block = ?1",
            ] {
                let mut stmt = tx.prepare(sql).map_err(db_err)?;
                let ids = stmt
                    .query_map([hash], |r| r.get::<_, [u8; 32]>(0))
                    .map_err(db_err)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(db_err)?;
                affected.extend(ids);
            }
        }
        let mut tokens_todo: std::collections::BTreeSet<[u8; 32]> = Default::default();
        let mut minters_todo: std::collections::BTreeSet<[u8; 32]> = Default::default();
        for id in &affected {
            if crate::tokens::is_token(&tx, id)? {
                tokens_todo.insert(*id);
            }
            if crate::tokens::is_minter(&tx, id)? {
                minters_todo.insert(*id);
            }
            // Belt and braces: any token whose derived events cite this
            // covenant is re-derived too (tev_by_event).
            let mut stmt = tx
                .prepare("SELECT DISTINCT token_id FROM token_events WHERE covenant_id = ?1")
                .map_err(db_err)?;
            let ids = stmt
                .query_map([id.as_slice()], |r| r.get::<_, [u8; 32]>(0))
                .map_err(db_err)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_err)?;
            tokens_todo.extend(ids);
        }
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
            tx.execute(
                "DELETE FROM covenant_utxos WHERE created_block = ?1",
                [hash],
            )
            .map_err(db_err)?;
            tx.execute(
                "UPDATE covenants SET event_count = event_count -
                   (SELECT COUNT(*) FROM covenant_events WHERE accepting_block = ?1 AND covenant_id = covenants.covenant_id)",
                [hash],
            )
            .map_err(db_err)?;
            tx.execute(
                "DELETE FROM covenant_events WHERE accepting_block = ?1",
                [hash],
            )
            .map_err(db_err)?;
        }
        // Covenants whose genesis was rolled back disappear entirely.
        tx.execute("DELETE FROM covenants WHERE event_count <= 0", [])
            .map_err(db_err)?;
        // Resting orders regress with their proofs: a rolled-back resolving
        // spend reopens the order (its cell is live again), and a rolled-back
        // recognizing reveal leaves nothing decodable, so the row is deleted.
        // Gated on an existing row — a covenant that never proved an order
        // has nothing to rewind.
        for id in &affected {
            let tracked: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM resting_orders WHERE covenant_id = ?1)",
                    [id.as_slice()],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            if tracked {
                derive_resting_order(&tx, id)?;
            }
        }
        // Token rewind, phase 2: re-derive every affected token from the
        // POST-rollback source tables — exactly what a from-scratch index at
        // the rolled-back height would contain. Cells whose proving reveal
        // was rolled back regress to unproven, verdicts regress with them,
        // and a token whose entire evidence disappeared loses its rows.
        crate::tokens::rederive_affected(&tx, &minters_todo, &tokens_todo)?;
        // Record the reorg for the public feed. The best-available DAA is the
        // indexer's own progress mark (the tip we had reached) — the removed
        // blocks are being deleted, so their DAAs aren't reliably queryable
        // here, and not every reorged block carried covenant activity anyway.
        if !removed.is_empty() {
            let daa: u64 = tx
                .query_row(
                    "SELECT value FROM meta WHERE key = 'processed_daa'",
                    [],
                    |row| row.get::<_, String>(0),
                )
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

    /// The newest accepting chain blocks in the index as (block, DAA), newest
    /// first — candidate anchors when re-anchoring a wedged cursor. DISTINCT
    /// over the pair is distinct blocks (an accepting block has exactly one
    /// DAA score); ev_by_daa serves the scan backwards, so cost is O(limit).
    pub fn recent_accepting_blocks(&self, limit: u64) -> Result<Vec<(BlockHash, u64)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT accepting_block, accepting_daa FROM covenant_events
                 ORDER BY accepting_daa DESC LIMIT ?1",
            )
            .map_err(db_err)?;
        let limit = limit.min(i64::MAX as u64) as i64;
        let rows = stmt
            .query_map([limit], |row| {
                Ok((BlockHash(row.get::<_, [u8; 32]>(0)?), row.get(1)?))
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Anchor candidates spread evenly through the WHOLE indexed DAA range —
    /// one accepting block at (or just below) each of `samples` evenly spaced
    /// DAA targets, newest first, both endpoints included. Each sample is one
    /// O(log n) probe on ev_by_daa; adjacent samples can resolve to the same
    /// block on sparse history, so callers should dedupe.
    pub fn spread_accepting_blocks(&self, samples: u64) -> Result<Vec<(BlockHash, u64)>> {
        let bounds: Option<(u64, u64)> = self
            .conn
            .query_row(
                "SELECT MIN(accepting_daa), MAX(accepting_daa) FROM covenant_events",
                [],
                |row| {
                    Ok(row
                        .get::<_, Option<u64>>(0)?
                        .zip(row.get::<_, Option<u64>>(1)?))
                },
            )
            .map_err(db_err)?;
        let Some((min_daa, max_daa)) = bounds else {
            return Ok(vec![]);
        };
        let mut stmt = self
            .conn
            .prepare(
                "SELECT accepting_block, accepting_daa FROM covenant_events
                 WHERE accepting_daa <= ?1 ORDER BY accepting_daa DESC LIMIT 1",
            )
            .map_err(db_err)?;
        let span = max_daa - min_daa;
        let mut out = Vec::with_capacity(samples as usize);
        for i in 0..samples {
            let target = max_daa - span * i / samples.max(2).saturating_sub(1);
            let row = stmt
                .query_row([target], |row| {
                    Ok((BlockHash(row.get::<_, [u8; 32]>(0)?), row.get(1)?))
                })
                .optional()
                .map_err(db_err)?;
            if let Some(sample) = row {
                out.push(sample);
            }
        }
        Ok(out)
    }

    /// Every accepting block with indexed activity above the given DAA,
    /// newest first — the rollback set when re-anchoring below them. Blocks
    /// the cursor visited without covenant activity never enter
    /// covenant_events and carry nothing to undo. ev_by_daa covers the range.
    pub fn accepting_blocks_above(&self, daa: u64) -> Result<Vec<BlockHash>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT accepting_block, accepting_daa FROM covenant_events
                 WHERE accepting_daa > ?1 ORDER BY accepting_daa DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([daa], |row| Ok(BlockHash(row.get::<_, [u8; 32]>(0)?)))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Stamp acceptance-order indices onto event rows that predate capture:
    /// `blocks` is one RPC response's worth of `(accepting block, [(txid,
    /// index)])`, applied in a single write transaction (the batch discipline
    /// of `backfill_templates`). Only rows still NULL are touched — several
    /// covenant rows sharing one txid all get the same index (it's a property
    /// of the tx). Returns how many rows were stamped.
    pub fn stamp_tx_indices(&mut self, blocks: &[(BlockHash, Vec<(TxId, u32)>)]) -> Result<u64> {
        let tx = self.conn.transaction().map_err(db_err)?;
        let mut stamped = 0u64;
        {
            // Most chain blocks carry no covenant events: one indexed probe
            // (ev_by_accepting) per block skips them without per-tx UPDATEs.
            let mut probe = tx
                .prepare(
                    "SELECT EXISTS(SELECT 1 FROM covenant_events
                     WHERE accepting_block = ?1 AND tx_index IS NULL)",
                )
                .map_err(db_err)?;
            let mut update = tx
                .prepare(
                    "UPDATE covenant_events SET tx_index = ?1
                     WHERE accepting_block = ?2 AND txid = ?3 AND tx_index IS NULL",
                )
                .map_err(db_err)?;
            for (block, indices) in blocks {
                let any: bool = probe
                    .query_row([block.0.as_slice()], |r| r.get(0))
                    .map_err(db_err)?;
                if !any {
                    continue;
                }
                for (txid, index) in indices {
                    stamped += update
                        .execute(params![index, block.0.as_slice(), txid.0.as_slice()])
                        .map_err(db_err)? as u64;
                }
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(stamped)
    }

    /// Has the one-shot tx_index backfill walked its whole reachable range?
    /// Completed runs make `backfill_tx_index` an O(1) no-op per session.
    pub fn tx_index_backfill_done(&self) -> Result<bool> {
        Ok(self.meta("tx_index_backfilled_to")?.as_deref() == Some("done"))
    }

    /// Where an interrupted tx_index backfill should resume (the last
    /// accepting block already stamped), if it ever recorded progress.
    pub fn tx_index_backfill_resume(&self) -> Result<Option<BlockHash>> {
        Ok(self
            .meta("tx_index_backfilled_to")?
            .and_then(|s| s.parse().ok()))
    }

    pub fn set_tx_index_backfill_progress(&self, at: BlockHash) -> Result<()> {
        self.set_meta("tx_index_backfilled_to", &at.to_string())
    }

    pub fn set_tx_index_backfill_done(&self) -> Result<()> {
        self.set_meta("tx_index_backfilled_to", "done")
    }

    /* ---------- gap recovery (see sync::recover_gap) ----------
    A deep-reorg wedge answered by a sink reset leaves a DAA window with
    zero indexed events between two healthy segments. These methods merge
    the canonical history of that window back in, offline, on a COPY. */

    /// The widest DAA discontinuity between consecutive indexed events, as
    /// `(last DAA before, first DAA after)`, when it spans at least
    /// `min_span`. A sink-reset gap is exactly such a discontinuity: the
    /// resumed index writes events only above the reset point.
    pub fn find_daa_gap(&self, min_span: u64) -> Result<Option<(u64, u64)>> {
        self.conn
            .query_row(
                "SELECT prev_daa, daa FROM (
                     SELECT accepting_daa AS daa,
                            LAG(accepting_daa) OVER (ORDER BY accepting_daa) AS prev_daa
                     FROM (SELECT DISTINCT accepting_daa FROM covenant_events)
                 )
                 WHERE prev_daa IS NOT NULL AND daa - prev_daa >= ?1
                 ORDER BY daa - prev_daa DESC LIMIT 1",
                [min_span],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)
    }

    /// Every recorded gap recovery as `(from_daa, to_daa)`, newest first —
    /// the idempotence marker recover_gap consults before doing anything.
    pub fn gap_recoveries(&self) -> Result<Vec<(u64, u64)>> {
        let Some(raw) = self.meta("gap_recoveries")? else {
            return Ok(vec![]);
        };
        let entries: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
        let mut out: Vec<(u64, u64)> = entries
            .iter()
            .filter_map(|e| Some((e.get("from_daa")?.as_u64()?, e.get("to_daa")?.as_u64()?)))
            .collect();
        out.reverse(); // stored in append order → newest first
        Ok(out)
    }

    /// The gap window an interrupted recovery run was working on, if any —
    /// recorded BEFORE the walk starts, because a partial merge shrinks the
    /// DAA discontinuity and a re-run's auto-detection would otherwise see
    /// only a sub-window of the original gap and skip the rest.
    pub fn gap_recovery_pending(&self) -> Result<Option<(u64, u64)>> {
        let Some(raw) = self.meta("gap_recovery_pending")? else {
            return Ok(None);
        };
        let mut parts = raw.splitn(2, ':').map(|p| p.parse::<u64>().ok());
        Ok(parts.next().flatten().zip(parts.next().flatten()))
    }

    /// Record the window a recovery run is about to walk (cleared by
    /// [`Store::finalize_gap_recovery`] in the same transaction that writes
    /// the completed-recovery marker).
    pub fn set_gap_recovery_pending(&self, gap_lo: u64, gap_hi: u64) -> Result<()> {
        self.set_meta("gap_recovery_pending", &format!("{gap_lo}:{gap_hi}"))
    }

    /// The canonical chain block the recovery walk last advanced past — so a
    /// run interrupted by a node disconnect resumes mid-walk instead of
    /// re-walking from the pruning point. Any block we already walked is newer
    /// than the pruning point and still walkable on a fresh node; merges dedup,
    /// so a slightly-stale cursor only re-does a few cheap blocks.
    pub fn gap_walk_cursor(&self) -> Result<Option<BlockHash>> {
        Ok(self.meta("gap_walk_cursor")?.and_then(|s| s.parse().ok()))
    }

    pub fn set_gap_walk_cursor(&self, hash: &BlockHash) -> Result<()> {
        self.set_meta("gap_walk_cursor", &hash.to_string())
    }

    /// Does this covenant have any indexed event at or below the given DAA?
    /// The gap capture's "would this be the covenant's chronologically-first
    /// event" probe (ev_by_daa + the PK both serve it).
    pub(crate) fn has_event_at_or_below(&self, id: &CovenantId, daa: u64) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM covenant_events
                 WHERE covenant_id = ?1 AND accepting_daa <= ?2)",
                params![id.0.as_slice(), daa],
                |r| r.get(0),
            )
            .map_err(db_err)
    }

    /// Merge one recovered chain block's covenant activity into the index —
    /// [`Store::apply`]'s twin for out-of-order history, with three deliberate
    /// differences: the sync cursor / processed_daa / tip are NEVER touched
    /// (this is not progress, it's the past), every write is dedup-aware (a
    /// re-run, or the inclusive window boundary re-walking an already-indexed
    /// block, must change nothing), and event seqs are provisional appends —
    /// [`Store::finalize_gap_recovery`] re-sequences afterwards, so no token
    /// hook runs here either.
    pub fn merge_recovered_block(&mut self, block: &BlockEvents) -> Result<MergeCounts> {
        let mut counts = MergeCounts::default();
        let tx = self.conn.transaction().map_err(db_err)?;
        // Created cells land BEFORE spends are marked — same intra-block
        // chain discipline as apply(). INSERT OR IGNORE (not REPLACE): an
        // existing row may carry spend capture this recovered view lacks.
        for utxo in &block.created_utxos {
            let template = registry()
                .decode(utxo.spk_version, &utxo.spk_script)
                .template
                .unwrap_or("");
            let inserted = tx
                .execute(
                    "INSERT OR IGNORE INTO covenant_utxos
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
            counts.utxos_added += inserted as u64;
        }
        for (outpoint, spending_txid, sig, budget, input_index) in &block.spent_utxos {
            // Only a still-unspent row is repaired: an existing spend record
            // (production's own, or an earlier recovery run) always wins —
            // one outpoint has exactly one canonical spend.
            let row: Option<(u16, Vec<u8>)> = tx
                .query_row(
                    "SELECT spk_version, spk_script FROM covenant_utxos
                     WHERE txid = ?1 AND output_index = ?2 AND spent_block IS NULL",
                    params![outpoint.txid.0.as_slice(), outpoint.index],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(db_err)?;
            let Some((version, spk)) = row else { continue };
            // Spend-time recognition, same as apply(): the reveal names the
            // program that actually ran ('' = spend seen, nothing matched).
            let redeem = kascov_decode::p2sh_reveal(&spk, sig);
            let kcc1_hash = redeem
                .as_deref()
                .and_then(kascov_decode::kcc20::kcc1_template_hash);
            let revealed = redeem
                .and_then(|redeem| {
                    kascov_decode::kcc20::revealed_template(registry(), version, &redeem)
                })
                .unwrap_or("");
            let updated = tx
                .execute(
                    "UPDATE covenant_utxos
                     SET spent_block = ?1, spent_txid = ?2, spent_sig = ?3, spent_budget = ?4, revealed_template = ?5, kcc1_template_hash = ?6, spent_input_index = ?7
                     WHERE txid = ?8 AND output_index = ?9 AND spent_block IS NULL",
                    params![
                        block.accepting_block.0.as_slice(),
                        spending_txid.0.as_slice(),
                        sig,
                        budget,
                        revealed,
                        kcc1_hash.as_ref().map(|h| h.as_slice()).unwrap_or(&[]),
                        input_index,
                        outpoint.txid.0.as_slice(),
                        outpoint.index
                    ],
                )
                .map_err(db_err)?;
            counts.spends_repaired += updated as u64;
        }
        for event in &block.events {
            // Dedup on (covenant, txid): one covenant fires at most one event
            // per transaction (classify aggregates), and the same tx observed
            // under a different accepting block (canonical vs. the stranded
            // branch the old cursor died on) is still the same historic event.
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM covenant_events
                     WHERE covenant_id = ?1 AND txid = ?2)",
                    params![event.covenant_id.0.as_slice(), event.txid.0.as_slice()],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            if exists {
                continue;
            }
            // Bare covenant row for gap-born ids; every summary column
            // (event_count, last_activity, genesis columns) is recomputed by
            // finalize_gap_recovery from the merged truth.
            tx.execute(
                "INSERT INTO covenants (covenant_id, lineage_complete) VALUES (?1, 0)
                 ON CONFLICT(covenant_id) DO NOTHING",
                [event.covenant_id.0.as_slice()],
            )
            .map_err(db_err)?;
            let (tag, ikind) = match &event.payload {
                Some(p) => (Some(payload_tag(p)), Some(inscription_kind_of(p))),
                None => (None, None),
            };
            // Provisional seq: MAX+1 keeps walk order among merged rows and
            // never collides; the finalize pass renumbers chronologically.
            tx.execute(
                "INSERT INTO covenant_events (covenant_id, seq, kind, txid, accepting_block, accepting_daa, payload, lane_namespace, payload_tag, inscription_kind, tx_index, accepting_time_ms, accepting_blue_score)
                 VALUES (?1,
                   (SELECT COALESCE(MAX(seq), -1) + 1 FROM covenant_events WHERE covenant_id = ?1),
                   ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    event.covenant_id.0.as_slice(),
                    event.kind.as_str(),
                    event.txid.0.as_slice(),
                    block.accepting_block.0.as_slice(),
                    block.accepting_daa,
                    event.payload,
                    event.lane_namespace,
                    tag,
                    ikind,
                    event.tx_index,
                    block.accepting_time_ms,
                    block.accepting_blue_score
                ],
            )
            .map_err(db_err)?;
            counts.events_added += 1;
        }
        tx.commit().map_err(db_err)?;
        Ok(counts)
    }

    /// Close out a gap recovery in ONE transaction: re-sequence, refresh
    /// summaries, re-derive token accounting, record the recovery in meta.
    ///
    /// The working set is every covenant with an event inside the recovered
    /// window — derived from the events table itself, NOT from merge
    /// bookkeeping, so a run that died between merging and finalizing is
    /// fully repaired by the next run (whose merges all dedup to no-ops).
    /// Every step below is idempotent for the same reason.
    pub fn finalize_gap_recovery(
        &mut self,
        gap_lo: u64,
        gap_hi: u64,
        merged: &MergeCounts,
    ) -> Result<FinalizeCounts> {
        let mut counts = FinalizeCounts::default();
        let tx = self.conn.transaction().map_err(db_err)?;
        let refresh: Vec<[u8; 32]> = {
            let mut stmt = tx
                .prepare(
                    "SELECT DISTINCT covenant_id FROM covenant_events
                     WHERE accepting_daa BETWEEN ?1 AND ?2",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map(params![gap_lo, gap_hi], |r| r.get(0))
                .map_err(db_err)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_err)?;
            rows
        };
        for id in &refresh {
            // Chronological order = (accepting_daa, existing seq). seq as the
            // tie-break preserves indexing order within one DAA score — the
            // same contract live sync gives such rows — and merged rows carry
            // later provisional seqs, so on an exact-DAA tie at the window
            // boundary the already-indexed row (the earlier truth) stays first.
            let ordered: Vec<i64> = {
                let mut stmt = tx
                    .prepare(
                        "SELECT seq FROM covenant_events WHERE covenant_id = ?1
                         ORDER BY accepting_daa, seq",
                    )
                    .map_err(db_err)?;
                let rows = stmt
                    .query_map([id.as_slice()], |r| r.get(0))
                    .map_err(db_err)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(db_err)?;
                rows
            };
            if !ordered
                .iter()
                .enumerate()
                .all(|(new_seq, &old_seq)| old_seq == new_seq as i64)
            {
                // The prior-art trick (scripts/repair-lineage.py, 99b5bba):
                // an in-place renumber can transiently collide with its own
                // unmoved rows on the (covenant_id, seq) PK — park every row
                // at a bijective NEGATIVE temp first, then flip back in one
                // sweep: -(new+1) → -(-(new+1))-1 = new.
                for (new_seq, &old_seq) in ordered.iter().enumerate() {
                    tx.execute(
                        "UPDATE covenant_events SET seq = ?1 WHERE covenant_id = ?2 AND seq = ?3",
                        params![-(new_seq as i64) - 1, id.as_slice(), old_seq],
                    )
                    .map_err(db_err)?;
                }
                tx.execute(
                    "UPDATE covenant_events SET seq = -seq - 1 WHERE covenant_id = ?1 AND seq < 0",
                    [id.as_slice()],
                )
                .map_err(db_err)?;
                counts.covenants_resequenced += 1;
            }
            // Summary refresh from the merged truth.
            tx.execute(
                "UPDATE covenants SET
                   event_count = (SELECT COUNT(*) FROM covenant_events WHERE covenant_id = ?1),
                   last_activity_daa = (SELECT COALESCE(MAX(accepting_daa), 0) FROM covenant_events WHERE covenant_id = ?1)
                 WHERE covenant_id = ?1",
                [id.as_slice()],
            )
            .map_err(db_err)?;
            // Genesis columns follow the chronologically-first event: a
            // KIP-20-proven genesis merged from the gap completes a lineage
            // production could only see mid-life; a first event that is
            // still a transition keeps the lineage honestly incomplete.
            let first: Option<(String, [u8; 32], u64)> = tx
                .query_row(
                    "SELECT kind, txid, accepting_daa FROM covenant_events
                     WHERE covenant_id = ?1 AND seq = 0",
                    [id.as_slice()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(db_err)?;
            match first {
                Some((kind, txid, daa)) if kind == "genesis" => {
                    tx.execute(
                        "UPDATE covenants SET genesis_txid = ?2, genesis_daa = ?3, lineage_complete = 1
                         WHERE covenant_id = ?1",
                        params![id.as_slice(), txid.as_slice(), daa],
                    )
                    .map_err(db_err)?;
                }
                _ => {
                    tx.execute(
                        "UPDATE covenants SET genesis_txid = NULL, genesis_daa = NULL, lineage_complete = 0
                         WHERE covenant_id = ?1",
                        [id.as_slice()],
                    )
                    .map_err(db_err)?;
                }
            }
        }
        counts.covenants_refreshed = refresh.len() as u64;

        // Token accounting: the cheapest CORRECT re-derivation is the
        // per-covenant closure — NOT a TOKEN_DERIVATION_VERSION bump.
        // derive_token is deterministic from the source tables per token, so
        // only tokens whose covenant rows changed (or whose derived rows cite
        // changed covenants — their (covenant_id, seq) references just went
        // stale under the renumber) can differ. That is exactly the closure
        // apply()'s hook and rollback() already trust for correctness. A
        // version bump would rebuild EVERY token on EVERY database (mainnet
        // included) at next boot, for a repair scoped to one testnet window.
        {
            let mut tokens_todo: std::collections::BTreeSet<[u8; 32]> = Default::default();
            let mut minters_todo: std::collections::BTreeSet<[u8; 32]> = Default::default();
            for id in &refresh {
                if crate::tokens::is_token(&tx, id)? || crate::tokens::has_token_evidence(&tx, id)?
                {
                    tokens_todo.insert(*id);
                }
                if crate::tokens::is_minter(&tx, id)?
                    || crate::tokens::has_minter_evidence(&tx, id)?
                {
                    minters_todo.insert(*id);
                }
                let mut stmt = tx
                    .prepare("SELECT DISTINCT token_id FROM token_events WHERE covenant_id = ?1")
                    .map_err(db_err)?;
                let citing = stmt
                    .query_map([id.as_slice()], |r| r.get::<_, [u8; 32]>(0))
                    .map_err(db_err)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(db_err)?;
                tokens_todo.extend(citing);
            }
            // Mirrors tokens::rederive_affected, inlined for the tally.
            let mut todo = tokens_todo;
            for minter in &minters_todo {
                todo.extend(crate::tokens::derive_minter(&tx, minter)?);
            }
            for token in &todo {
                crate::tokens::derive_token(&tx, token)?;
            }
            counts.tokens_rederived = todo.len() as u64;
        }

        // Honest history: the recovery is recorded in meta (machine-readable,
        // append-only) rather than reorg_log — nothing was rolled back, and
        // the reorg feed's consumers count rollbacks. This entry is also the
        // marker that makes the next recover_gap run a no-op.
        let existing: Option<String> = tx
            .query_row(
                "SELECT value FROM meta WHERE key = 'gap_recoveries'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let mut entries: Vec<serde_json::Value> = existing
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        entries.push(serde_json::json!({
            "from_daa": gap_lo,
            "to_daa": gap_hi,
            "at_ms": now_ms(),
            "events_added": merged.events_added,
            "utxos_added": merged.utxos_added,
            "spends_repaired": merged.spends_repaired,
            "covenants_refreshed": counts.covenants_refreshed,
            "note": "recover-gap: canonical history merged from a node walk (the window was skipped by a deep-reorg sink reset)",
        }));
        let raw = serde_json::to_string(&entries).map_err(|e| Error::Invalid {
            what: "gap_recoveries meta",
            value: e.to_string(),
        })?;
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('gap_recoveries', ?1)",
            [raw],
        )
        .map_err(db_err)?;
        // The in-flight window marker and the walk-resume cursor retire with
        // the completed marker, in the same transaction: either all survive a
        // crash or none do.
        tx.execute("DELETE FROM meta WHERE key = 'gap_recovery_pending'", [])
            .map_err(db_err)?;
        tx.execute("DELETE FROM meta WHERE key = 'gap_walk_cursor'", [])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(counts)
    }

    /// Raw connection access for crate-internal tests (planting sentinels,
    /// simulating pre-capture rows) — never a production surface.
    #[cfg(test)]
    pub(crate) fn raw_conn(&self) -> &Connection {
        &self.conn
    }

    /// Simulate rows written by a pre-capture binary (tests only).
    #[cfg(test)]
    pub(crate) fn wipe_tx_indices_for_test(&self) -> Result<()> {
        self.conn
            .execute("UPDATE covenant_events SET tx_index = NULL", [])
            .map_err(db_err)?;
        self.conn
            .execute("DELETE FROM meta WHERE key = 'tx_index_backfilled_to'", [])
            .map_err(db_err)?;
        Ok(())
    }

    /// Regress every stamp to the previous classifier's generic verdicts and
    /// drop the version key — what a database written by the last release
    /// looks like (tests only).
    #[cfg(test)]
    pub(crate) fn simulate_old_classifier_for_test(&self) -> Result<()> {
        self.plant_generic_stamps_for_test()?;
        self.conn
            .execute("DELETE FROM meta WHERE key = 'classifier_version'", [])
            .map_err(db_err)?;
        Ok(())
    }

    /// Overwrite recognized stamps with generic verdicts but keep the
    /// version key — for asserting the reclassification is version-gated
    /// (tests only).
    #[cfg(test)]
    pub(crate) fn plant_generic_stamps_for_test(&self) -> Result<()> {
        self.conn
            .execute(
                "UPDATE covenant_utxos SET revealed_template = '' WHERE revealed_template IS NOT NULL",
                [],
            )
            .map_err(db_err)?;
        self.conn
            .execute(
                "UPDATE covenant_events SET inscription_kind = '' WHERE inscription_kind IS NOT NULL",
                [],
            )
            .map_err(db_err)?;
        Ok(())
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
                Ok(ReorgRow {
                    daa: row.get(0)?,
                    at_ms: row.get(1)?,
                    rolled_back: row.get(2)?,
                })
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
    pub fn list_page(
        &self,
        after: Option<(u64, [u8; 32])>,
        limit: u64,
    ) -> Result<Vec<CovenantSummary>> {
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
                .query_row("SELECT MAX(accepting_daa) FROM covenant_events", [], |r| {
                    r.get(0)
                })
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
                |r| {
                    Ok((
                        r.get::<_, u64>(0)?,
                        r.get::<_, u64>(1)?,
                        r.get::<_, u64>(2)?,
                    ))
                },
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
        Ok(DigestStats {
            births,
            moves,
            burns,
            value_born,
            active_now,
            busiest,
            biggest_birth,
        })
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
        let labels = [
            "< 10 s",
            "10 s – 1 min",
            "1 – 10 min",
            "10 min – 1 h",
            "1 – 6 h",
            "6 h +",
        ];
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
                .query_map([], |r| {
                    Ok((r.get::<_, i64>(0)? as usize, r.get::<_, i64>(1)? as u64))
                })
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
            let med_sql =
                format!("{cte} SELECT life FROM lifespans ORDER BY life LIMIT 1 OFFSET ?");
            self.conn
                .query_row(&med_sql, [(total / 2) as i64], |r| r.get::<_, i64>(0))
                .map(|v| v as u64)
                .unwrap_or(0)
        } else {
            0
        };
        let buckets = labels
            .iter()
            .zip(counts.iter())
            .map(|(l, c)| (*l, *c))
            .collect();
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
            .query_map([], |row| {
                Ok((CovenantId(row.get(0)?), row.get::<_, u64>(1)?))
            })
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

    /// "What runs on this network": per-template aggregates in one pass.
    /// Recognition is stamped at write time, so this never decodes a script.
    /// Each state UTXO counts under its covenant's RESOLVED name — the same
    /// COALESCE precedence as `covenant_templates()`/`SUMMARY_SELECT` (a
    /// non-p2* revealed or state template wins, then a non-p2* state
    /// template, else any) — so a P2SH commitment whose program revealed at
    /// spend folds into the coin's effective name, and "p2sh commitment"
    /// keeps only genuinely-unrevealed coins (commitment-time classification
    /// alone left every semantic template at 0 coins forever). Covenants
    /// where no cell ever matched a template (including NULL rows written by
    /// an older binary, healed at the next open) fold into the unrecognized
    /// bucket — honest degradation under version skew.
    pub fn template_stats(&self) -> Result<Vec<TemplateStat>> {
        let mut stmt = self
            .conn
            .prepare(
                "WITH resolved AS (
                    SELECT covenant_id,
                           COALESCE(
                             MAX(CASE WHEN revealed_template IS NOT NULL AND revealed_template <> '' AND revealed_template NOT LIKE 'p2%' THEN revealed_template
                                      WHEN template NOT LIKE 'p2%' THEN template END),
                             MAX(COALESCE(NULLIF(revealed_template, ''), template))) AS tpl
                    FROM covenant_utxos
                    WHERE (template IS NOT NULL AND template <> '') OR (revealed_template IS NOT NULL AND revealed_template <> '')
                    GROUP BY covenant_id)
                 SELECT COALESCE(r.tpl, '') AS tpl,
                        COALESCE(SUM(CASE WHEN u.spent_block IS NULL THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN u.spent_block IS NULL THEN u.value ELSE 0 END), 0),
                        COUNT(*),
                        COUNT(DISTINCT u.covenant_id)
                 FROM covenant_utxos u
                 LEFT JOIN resolved r ON r.covenant_id = u.covenant_id
                 GROUP BY tpl ORDER BY COUNT(*) DESC, tpl",
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
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)? as u64,
                    r.get::<_, i64>(2)? as u64,
                ))
            })
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
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)? as u64,
                    r.get::<_, i64>(2)? as u64,
                ))
            })
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
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)? as u64,
                    r.get::<_, i64>(2)? as u64,
                ))
            })
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
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)? as u64,
                    r.get::<_, i64>(2)? as u64,
                ))
            })
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
            .prepare(&format!(
                "SELECT substr(payload, 1, {INSCRIPTION_WINDOW}), lower(hex(covenant_id))
                 FROM covenant_events
                 WHERE payload IS NOT NULL
                   AND (substr(payload, 1, 2) = x'7b22' OR substr(payload, 1, 4) = x'37623232')",
            ))
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(db_err)?;
        // kind -> (event count, distinct covenant ids)
        let mut agg: std::collections::HashMap<String, (u64, std::collections::HashSet<String>)> =
            std::collections::HashMap::new();
        for row in rows {
            let (payload, cid) = row.map_err(db_err)?;
            let Some(v) = extract_inscription_json(&payload) else {
                continue;
            };
            let kind = inscription_kind(&v);
            let e = agg.entry(kind).or_default();
            e.0 += 1;
            e.1.insert(cid);
        }
        let mut out: Vec<(String, u64, u64)> = agg
            .into_iter()
            .map(|(k, (c, set))| (k, c, set.len() as u64))
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1));
        out.truncate(60);
        Ok(out)
    }

    /// Record a community-verified source (proven byte-identical to a compiled
    /// program). Keyed by the program's blake2b hash.
    pub fn put_verified_source(
        &self,
        hash: &str,
        hex: &str,
        source: &str,
        args: &str,
        template: Option<&str>,
        now_ms: u64,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO verified_sources (program_hash, program_hex, source, args, template, verified_at) VALUES (?1,?2,?3,?4,?5,?6)",
                params![hash, hex, source, args, template, now_ms as i64],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Fetch a published source by program hash → (source, args, template, verified_at).
    pub fn get_verified_source(
        &self,
        hash: &str,
    ) -> Result<Option<(String, String, Option<String>, u64)>> {
        self.conn
            .query_row(
                "SELECT source, args, template, verified_at FROM verified_sources WHERE program_hash = ?1",
                params![hash],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)? as u64)),
            )
            .optional()
            .map_err(db_err)
    }

    /// Add a webhook subscription (covenant_id / kind NULL = wildcard).
    /// `secret` signs deliveries and gates unsubscribe (NULL = legacy row,
    /// unsigned and deletable by id alone). Returns its id.
    pub fn add_subscription(
        &self,
        covenant_id: Option<&[u8]>,
        kind: Option<&str>,
        url: &str,
        secret: Option<&str>,
        now_ms: u64,
    ) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO webhook_subscriptions (covenant_id, kind, url, secret, created_at) VALUES (?1,?2,?3,?4,?5)",
                params![covenant_id, kind, url, secret, now_ms as i64],
            )
            .map_err(db_err)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Remove a subscription by id. Returns whether one was deleted.
    /// Bypasses any secret — for the delivery loop retiring dead endpoints;
    /// caller-facing unsubscribe goes through [`delete_subscription_secured`].
    pub fn delete_subscription(&self, id: i64) -> Result<bool> {
        let n = self
            .conn
            .execute(
                "DELETE FROM webhook_subscriptions WHERE id = ?1",
                params![id],
            )
            .map_err(db_err)?;
        Ok(n > 0)
    }

    /// Caller-facing unsubscribe: a row with a secret is only deleted when
    /// the caller presents it; legacy NULL-secret rows delete by id alone.
    pub fn delete_subscription_secured(
        &self,
        id: i64,
        secret: Option<&str>,
    ) -> Result<UnsubscribeOutcome> {
        let stored: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT secret FROM webhook_subscriptions WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        match stored {
            None => Ok(UnsubscribeOutcome::NotFound),
            Some(Some(stored)) if secret != Some(stored.as_str()) => {
                Ok(UnsubscribeOutcome::WrongSecret)
            }
            Some(_) => {
                self.conn
                    .execute(
                        "DELETE FROM webhook_subscriptions WHERE id = ?1",
                        params![id],
                    )
                    .map_err(db_err)?;
                Ok(UnsubscribeOutcome::Deleted)
            }
        }
    }

    /// Webhook URLs matching an event (covenant_id + kind; NULL columns are wildcards).
    pub fn subscriptions_for(&self, covenant_id: &[u8], kind: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT url FROM webhook_subscriptions WHERE (covenant_id IS NULL OR covenant_id = ?1) AND (kind IS NULL OR kind = ?2)")
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![covenant_id, kind], |r| r.get::<_, String>(0))
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    /// Total active subscriptions (for the fire loop to skip work when zero).
    pub fn subscription_count(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM webhook_subscriptions", [], |r| {
                r.get::<_, i64>(0)
            })
            .map(|n| n as u64)
            .map_err(db_err)
    }

    /// Like [`subscriptions_for`] but returns `(id, url, secret)` — the
    /// delivery loop needs the id to retire a subscription after repeated
    /// failures and the secret to sign the POST body.
    pub fn subscriptions_matching(
        &self,
        covenant_id: &[u8],
        kind: &str,
    ) -> Result<Vec<(i64, String, Option<String>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, url, secret FROM webhook_subscriptions WHERE (covenant_id IS NULL OR covenant_id = ?1) AND (kind IS NULL OR kind = ?2)")
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![covenant_id, kind], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    /// True when `namespace` names a strict KIP-21 lane (rows carrying it in
    /// lane_namespace). lanes.json publishes strict lanes and generic tag lanes
    /// as DISJOINT sets (its tag aggregation is filtered to lane_namespace IS
    /// NULL), so a detail view must resolve to exactly one of them and never
    /// union the two, or a namespace that exists as both double-counts.
    fn lane_is_strict(&self, namespace: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM covenant_events WHERE lane_namespace = ?1)",
                params![namespace],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n != 0)
            .map_err(db_err)
    }

    /// The WHERE fragment selecting one lane's rows, matching whichever set
    /// lanes.json counted it in: strict lane_namespace when that lane exists,
    /// otherwise the generic tag lane stored as payload_tag 'tag:<hex>'.
    fn lane_where(strict: bool) -> &'static str {
        if strict {
            "lane_namespace = ?1"
        } else {
            "lane_namespace IS NULL AND payload_tag = 'tag:' || ?1"
        }
    }

    /// One lane's headline numbers: (event count, distinct covenants).
    /// Resolves strict KIP-21 lanes (ev_by_lane) and generic tag lanes
    /// (ev_tag_stats) the same way lanes.json aggregates them, so a tag lane's
    /// detail page no longer reads 0 while lanes.json advertises thousands.
    pub fn lane_stats(&self, namespace: &str) -> Result<(u64, u64)> {
        let sql = format!(
            "SELECT COUNT(*), COUNT(DISTINCT covenant_id)
             FROM covenant_events WHERE {}",
            Self::lane_where(self.lane_is_strict(namespace)?)
        );
        self.conn
            .query_row(&sql, params![namespace], |r| {
                Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))
            })
            .map_err(db_err)
    }

    /// The newest events inside one lane namespace, newest first.
    pub fn lane_recent(&self, namespace: &str, limit: u64) -> Result<Vec<GlobalEventRow>> {
        let sql = format!(
            "SELECT covenant_id, seq, kind, txid, accepting_daa, tx_index
             FROM covenant_events WHERE {}
             ORDER BY accepting_daa DESC, rowid DESC LIMIT ?2",
            Self::lane_where(self.lane_is_strict(namespace)?)
        );
        let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(
                params![namespace, limit.min(i64::MAX as u64) as i64],
                |row| {
                    Ok(GlobalEventRow {
                        covenant_id: CovenantId(row.get(0)?),
                        seq: row.get(1)?,
                        kind: row.get(2)?,
                        txid: TxId(row.get(3)?),
                        accepting_daa: row.get(4)?,
                        tx_index: row.get(5)?,
                    })
                },
            )
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// One lane's event counts per fixed-width DAA bucket, oldest first.
    /// Returns `(bucket_start_daa, count)`; empty buckets are omitted.
    pub fn lane_activity(&self, namespace: &str, bucket_daa: u64) -> Result<Vec<(u64, u64)>> {
        let width = bucket_daa.max(1);
        let sql = format!(
            "SELECT accepting_daa / ?2 AS bucket, COUNT(*)
             FROM covenant_events WHERE {}
             GROUP BY bucket ORDER BY bucket",
            Self::lane_where(self.lane_is_strict(namespace)?)
        );
        let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
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
                    outpoint: Outpoint {
                        txid: TxId(row.get(1)?),
                        index: row.get(2)?,
                    },
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

    /// Every covenant event this transaction fired (walks ev_by_txid), in
    /// deterministic (covenant, seq) order. All rows of one tx share the same
    /// accepting block — a transaction is accepted exactly once.
    pub fn events_by_txid(&self, txid: &TxId) -> Result<Vec<TxEventRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT covenant_id, seq, kind, accepting_block, accepting_daa, tx_index
                 FROM covenant_events WHERE txid = ?1 ORDER BY covenant_id, seq",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([txid.0.as_slice()], |row| {
                Ok(TxEventRow {
                    covenant_id: CovenantId(row.get(0)?),
                    seq: row.get(1)?,
                    kind: row.get(2)?,
                    accepting_block: BlockHash(row.get(3)?),
                    accepting_daa: row.get(4)?,
                    tx_index: row.get(5)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// State cells this transaction created — a primary-key prefix walk on
    /// (txid, output_index). NULLIF folds the '' decoded-no-match stamp into
    /// None (see [`TxCreatedCellRow`]).
    pub fn cells_created_by_txid(&self, txid: &TxId) -> Result<Vec<TxCreatedCellRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT covenant_id, output_index, value, NULLIF(template, '')
                 FROM covenant_utxos WHERE txid = ?1 ORDER BY output_index",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([txid.0.as_slice()], |row| {
                Ok(TxCreatedCellRow {
                    covenant_id: CovenantId(row.get(0)?),
                    index: row.get(1)?,
                    value: row.get(2)?,
                    template: row.get(3)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// State cells this transaction spent (walks utxo_by_spent_txid) — the
    /// light sibling of [`Store::spent_by_txid`], carrying shape hints
    /// instead of script bytes.
    pub fn cells_spent_by_txid(&self, txid: &TxId) -> Result<Vec<TxSpentCellRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT covenant_id, txid, output_index, value, NULLIF(revealed_template, ''),
                        spent_sig IS NOT NULL, spent_input_index,
                        NULLIF(kcc1_template_hash, x'')
                 FROM covenant_utxos WHERE spent_txid = ?1 ORDER BY txid, output_index",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([txid.0.as_slice()], |row| {
                Ok(TxSpentCellRow {
                    covenant_id: CovenantId(row.get(0)?),
                    txid: TxId(row.get(1)?),
                    index: row.get(2)?,
                    value: row.get(3)?,
                    revealed_template: row.get(4)?,
                    has_witness: row.get(5)?,
                    input_index: row.get(6)?,
                    kcc1_template_hash: row.get::<_, Option<[u8; 32]>>(7)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Deployer-claimed token metadata from the genesis transaction's payload.
    /// Convention (kascov-defined, now written up as KCC-0021): the genesis tx
    /// of a token covenant may carry JSON — directly or hex-encoded, same as
    /// the payload_tag conventions — with `name` (≤48 chars), `ticker`/`symbol`
    /// (≤12), optionally `image` (≤256, surfaced as a link, never hotlinked),
    /// `image_hash` (SHA-256 of the image bytes, the pin rendering verifies)
    /// and `decimals` (display scale only).
    /// These are CLAIMS by whoever authored the genesis, not unique and not
    /// validated — callers must present them with that provenance.
    pub fn claimed_token_meta(&self, id: &CovenantId) -> Result<Option<ClaimedTokenMeta>> {
        let payload: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT payload FROM covenant_events
                 WHERE covenant_id = ?1 AND kind = 'genesis' AND payload IS NOT NULL
                 ORDER BY seq LIMIT 1",
                [id.0.as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_err)?
            .flatten();
        let Some(raw) = payload else { return Ok(None) };
        if raw.is_empty() || raw.len() > 4096 {
            return Ok(None);
        }
        // Direct JSON first, then hex-encoded JSON (both shapes exist in the
        // wild — payload_tag classifies the same two).
        let parsed: Option<serde_json::Value> = serde_json::from_slice(&raw).ok().or_else(|| {
            std::str::from_utf8(&raw)
                .ok()
                .and_then(|s| hex::decode(s.trim()).ok())
                .and_then(|b| serde_json::from_slice(&b).ok())
        });
        let Some(v) = parsed else { return Ok(None) };
        let clean = |key: &[&str], max: usize| -> Option<String> {
            key.iter()
                .find_map(|k| v.get(k))
                .and_then(|x| x.as_str())
                .and_then(|s| {
                    let s = s.trim();
                    (!s.is_empty()
                        && s.chars().count() <= max
                        && s.chars().all(|c| !c.is_control()))
                    .then(|| s.to_string())
                })
        };
        let name = clean(&["name"], 48);
        let ticker = clean(&["ticker", "symbol"], 12);
        // KCC-0021: the scheme MUST be https:// or ipfs://, and anything else
        // "MUST be rejected at parse and the field dropped, so that a
        // non-conforming scheme can never reach the fetch pipeline". Dropping
        // it HERE rather than at render is the whole point: a payload is
        // attacker-written, every consumer of this struct inherits whatever
        // survives, and HTML escaping does not neutralize a scheme. A
        // `javascript:` image would otherwise reach the token page as a
        // clickable href, where target/rel do nothing to stop it.
        let image = clean(&["image"], 256).filter(|u| {
            let lower = u.to_ascii_lowercase();
            lower.starts_with("https://") || lower.starts_with("ipfs://")
        });
        // 64 lowercase hex chars or nothing — a malformed hash is no hash.
        let image_hash = clean(&["image_hash"], 64)
            .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
            .map(|h| h.to_lowercase());
        // KCC-0021 `decimals`: a display scale only, never applied to the
        // on-chain integers kascov verifies. Accepts a JSON integer or a
        // base-10 string (KRC-20 carries its `dec` as a string, and deployers
        // copy that habit). Bounded 0..=255 after ERC-20's uint8; anything
        // else is treated as undeclared rather than silently clamped, so a
        // typo cannot quietly move a token's decimal point.
        let decimals = v
            .get("decimals")
            .and_then(|d| {
                d.as_u64()
                    .or_else(|| d.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
            })
            .and_then(|n| u8::try_from(n).ok());
        if name.is_none() && ticker.is_none() {
            return Ok(None);
        }
        Ok(Some(ClaimedTokenMeta {
            name,
            ticker,
            image,
            image_hash,
            decimals,
        }))
    }

    /// Cached verified-art row: (status, content_type, bytes, fetched_ms).
    pub fn token_image(
        &self,
        id: &CovenantId,
    ) -> Result<Option<(String, Option<String>, Option<Vec<u8>>, u64)>> {
        self.conn
            .query_row(
                "SELECT status, content_type, bytes, fetched_ms FROM token_image_cache WHERE covenant_id = ?1",
                [id.0.as_slice()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(db_err)
    }

    /// Record a fetch outcome. Only 'verified' rows carry bytes; failures are
    /// negative-cached (the serving layer decides retry windows off
    /// fetched_ms) so a dead URL can't turn every page view into a fetch.
    pub fn put_token_image(
        &self,
        id: &CovenantId,
        status: &str,
        content_type: Option<&str>,
        bytes: Option<&[u8]>,
        fetched_ms: u64,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO token_image_cache (covenant_id, status, content_type, bytes, fetched_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id.0.as_slice(), status, content_type, bytes, fetched_ms],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Distinct KCC-1 TemplateHashes proven across this covenant's reveals
    /// (x'' "checked, no proven range" rows excluded). Usually 0 or 1; more
    /// means the covenant ran under multiple builds.
    pub fn covenant_kcc1_hashes(&self, id: &CovenantId) -> Result<Vec<[u8; 32]>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT kcc1_template_hash FROM covenant_utxos
                 WHERE covenant_id = ?1 AND kcc1_template_hash IS NOT NULL AND kcc1_template_hash <> x''
                 ORDER BY kcc1_template_hash",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([id.0.as_slice()], |row| row.get::<_, [u8; 32]>(0))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Covenants that revealed a program with this KCC-1 TemplateHash —
    /// the /template/{hash} lookup (utxo_by_kcc1 partial index).
    pub fn covenants_by_kcc1_hash(&self, hash: &[u8; 32]) -> Result<Vec<CovenantId>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT covenant_id FROM covenant_utxos
                 WHERE kcc1_template_hash = ?1 ORDER BY covenant_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([hash.as_slice()], |row| Ok(CovenantId(row.get(0)?)))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Per revealed-template name: how many distinct KCC-1 TemplateHashes its
    /// reveals carry, and the hash itself when there is exactly one.
    pub fn kcc1_hashes_by_template(&self) -> Result<Vec<(String, u64, Option<[u8; 32]>)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT revealed_template, COUNT(DISTINCT kcc1_template_hash),
                        CASE WHEN COUNT(DISTINCT kcc1_template_hash) = 1
                             THEN MAX(kcc1_template_hash) END
                 FROM covenant_utxos
                 WHERE revealed_template IS NOT NULL AND revealed_template <> ''
                   AND kcc1_template_hash IS NOT NULL AND kcc1_template_hash <> x''
                 GROUP BY revealed_template",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, Option<[u8; 32]>>(2)?,
                ))
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Classified token deltas this transaction produced. token_events has no
    /// txid column, so join through covenant_events on its (covenant_id, seq)
    /// key: ev_by_txid finds the events, tev_by_event probes the deltas —
    /// index-only on both sides, no scan, no new index needed.
    pub fn token_actions_by_txid(&self, txid: &TxId) -> Result<Vec<TxTokenActionRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.token_id, t.kind, t.amount
                 FROM covenant_events e
                 JOIN token_events t ON t.covenant_id = e.covenant_id AND t.seq = e.seq
                 WHERE e.txid = ?1
                 ORDER BY t.token_id, t.seq, t.delta_idx",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([txid.0.as_slice()], |row| {
                Ok(TxTokenActionRow {
                    token_id: CovenantId(row.get(0)?),
                    kind: row.get(1)?,
                    amount: row.get(2)?,
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
            .query_map([], |r| {
                Ok((CovenantId(r.get(0)?), r.get::<_, i64>(1)? != 0))
            })
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
            .query_map(
                params![lo.as_slice(), hi.as_slice(), limit],
                map_summary_row,
            )
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
                Some(len @ (32 | 33))
                    if spk.len() == len as usize + 2 && spk.last() == Some(&0xac) =>
                {
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
                "SELECT seq, kind, txid, accepting_block, accepting_daa, payload, tx_index,
                        accepting_time_ms, accepting_blue_score
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
                    tx_index: row.get(6)?,
                    accepting_time_ms: row.get(7)?,
                    accepting_blue_score: row.get(8)?,
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
                "SELECT covenant_id, seq, kind, txid, accepting_daa, tx_index
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
                    tx_index: row.get(5)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// One page of the chain-wide event feed in the canonical deterministic
    /// order: (accepting_daa, tx_index NULLS LAST, txid), oldest first —
    /// with (covenant_id, seq) as final tiebreakers, because one tx can move
    /// several covenants (several events share a txid). The cursor is
    /// (after_daa, after_seq): resume at DAA group `after_daa`, skipping its
    /// first `after_seq` events. A within-group offset is a stable cursor
    /// because the order inside a group is total and history only ever
    /// appends at higher DAAs (groups are tiny — ≤ a few dozen events share
    /// one DAA).
    ///
    /// Two queries, both on ev_by_daa: the boundary group (equality probe +
    /// a sort of that one group), then the open range, where SQLite walks the
    /// index in DAA order and only temp-sorts within each group before the
    /// LIMIT cuts off — no compound index needed (measured: <10ms a page from
    /// DAA 0 on a 767k-event index).
    pub fn events_after(
        &self,
        after_daa: u64,
        after_seq: u64,
        limit: u64,
    ) -> Result<Vec<FeedEventRow>> {
        const COLS: &str =
            "covenant_id, seq, kind, txid, accepting_daa, accepting_block, tx_index, length(payload)";
        fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FeedEventRow> {
            Ok(FeedEventRow {
                covenant_id: CovenantId(row.get(0)?),
                seq: row.get(1)?,
                kind: row.get(2)?,
                txid: TxId(row.get(3)?),
                accepting_daa: row.get(4)?,
                accepting_block: BlockHash(row.get(5)?),
                tx_index: row.get(6)?,
                payload_len: row.get(7)?,
            })
        }
        let limit = limit.min(i64::MAX as u64) as i64;
        let offset = after_seq.min(i64::MAX as u64) as i64;
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {COLS} FROM covenant_events WHERE accepting_daa = ?1
                 ORDER BY (tx_index IS NULL), tx_index, txid, covenant_id, seq LIMIT ?2 OFFSET ?3",
            ))
            .map_err(db_err)?;
        let mut rows = stmt
            .query_map(params![after_daa, limit, offset], map_row)
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        let remaining = limit - rows.len() as i64;
        if remaining > 0 {
            let mut stmt = self
                .conn
                .prepare(&format!(
                    "SELECT {COLS} FROM covenant_events WHERE accepting_daa > ?1
                     ORDER BY accepting_daa, (tx_index IS NULL), tx_index, txid, covenant_id, seq LIMIT ?2",
                ))
                .map_err(db_err)?;
            let tail = stmt
                .query_map(params![after_daa, remaining], map_row)
                .map_err(db_err)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_err)?;
            rows.extend(tail);
        }
        Ok(rows)
    }

    /// Covenants whose state (or spend-revealed) template is one of
    /// `templates`, newest activity first. Formerly the tokens-directory row
    /// source; that now reads the derived `tokens` tables (see tokens.rs).
    /// A full covenant_utxos scan (no template index), so callers cache.
    pub fn covenants_with_templates(&self, templates: &[&str]) -> Result<Vec<CovenantSummary>> {
        if templates.is_empty() {
            return Ok(vec![]);
        }
        let marks = vec!["?"; templates.len()].join(",");
        let sql = format!(
            "{SUMMARY_SELECT} WHERE c.covenant_id IN (
                SELECT DISTINCT covenant_id FROM covenant_utxos
                WHERE template IN ({marks}) OR revealed_template IN ({marks}))
             ORDER BY c.last_activity_daa DESC, c.covenant_id DESC"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(templates.iter().chain(templates.iter())),
                map_summary_row,
            )
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// One-shot, versioned full derivation of the token tables from history.
    /// Gated O(1) by a meta version probe; a version bump (rule or skeleton
    /// change) wipes and re-derives everything. Deliberately NOT run in
    /// `open()` — the serve path opens a store per request; the follower
    /// session triggers this so derivation never blocks serving. Batched
    /// transactions keep writer holds short; the meta stamp lands LAST, in
    /// its own transaction, so a crash mid-pass redoes the (deterministic)
    /// work instead of trusting partial state. Returns how many tokens were
    /// derived (0 = already current).
    /// Re-stamp `revealed_template` on reveals whose program carries a KCC20
    /// state block that the registry's pinned skeletons never matched.
    ///
    /// Why this exists: token discovery enumerates candidates from the STORED
    /// template column, so a covenant stamped "p2sh commitment" at write time
    /// stays invisible to `derive_tokens_if_stale` no matter how much the
    /// decoder improves. A live mainnet token (1,888 programs, one unguarded
    /// build) sat in that bucket. Improving the decoder alone fixes only what
    /// arrives next; this re-reads what is already stored.
    ///
    /// The claim is PROVEN, not guessed: `p2sh_reveal` returns a program only
    /// when blake2b(redeem) equals the output's P2SH commitment, so the bytes
    /// examined here are the committed on-chain script. Locating a state block
    /// in them is a fact about the chain.
    ///
    /// Sited in the follower startup path, NEVER in `Store::open`: a
    /// full-table rewrite inside open is precisely the shape that wedged
    /// testnet-10 for 49 hours. Chunked, resumable by rowid, one transaction
    /// per batch, so WAL readers keep serving throughout.
    pub fn restamp_kcc20_if_stale(&mut self) -> Result<u64> {
        const META: &str = "kcc20_restamp_version";
        if self.meta(META)?.as_deref() == Some(KCC20_RESTAMP_VERSION) {
            return Ok(0);
        }
        const BATCH: i64 = 1000;
        let mut restamped = 0u64;
        let mut after: i64 = 0;
        loop {
            let rows: Vec<(i64, Vec<u8>, Vec<u8>)> = {
                let mut stmt = self
                    .conn
                    .prepare(
                        // Only ever FILLS the unrecognized bucket. A row that
                        // already carries a positive identification is left
                        // alone, so this pass can add coverage but can never
                        // overwrite an existing verdict with a weaker one.
                        "SELECT rowid, spk_script, spent_sig FROM covenant_utxos
                         WHERE spent_sig IS NOT NULL AND rowid > ?1
                           AND (revealed_template IS NULL
                                OR revealed_template = ''
                                OR revealed_template = 'p2sh commitment')
                         ORDER BY rowid LIMIT ?2",
                    )
                    .map_err(db_err)?;
                let collected = stmt
                    .query_map(params![after, BATCH], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
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
            for (rowid, spk, sig) in &rows {
                after = *rowid;
                let Some(program) = kascov_decode::p2sh_reveal(spk, sig) else {
                    continue;
                };
                if kascov_decode::kcc20::locate_state_block(&program).is_none() {
                    continue;
                }
                tx.execute(
                    "UPDATE covenant_utxos SET revealed_template = 'KCC20 token' WHERE rowid = ?1",
                    params![rowid],
                )
                .map_err(db_err)?;
                restamped += 1;
            }
            tx.commit().map_err(db_err)?;
        }
        self.set_meta(META, KCC20_RESTAMP_VERSION)?;
        Ok(restamped)
    }

    /// Re-verify everything from scratch, ignoring the version gates.
    ///
    /// The gates exist so ordinary boots are cheap: nothing is re-derived
    /// while the rules that produced it are unchanged. This clears them, so
    /// the next pass walks every token's whole event history again and
    /// re-reads every market program's committed bytes. Expensive on purpose.
    ///
    /// It cannot recover the PAST. Runs that happened before this log existed
    /// left no record, and kascov will not manufacture one. What this gives
    /// you is a fresh, complete, honestly-recorded verification of the state
    /// as it is now — which is the thing an auditor can actually check.
    /// Run the audit bench: read-only forensics on every unmatched market
    /// program. Returns the report; writing it somewhere is the caller's job.
    pub fn audit_bench(&self) -> Result<serde_json::Value> {
        crate::bench::run_bench(&self.conn)
    }

    /// Recover one covenant's program from its own spends, the same way the
    /// bench does. Pinning a new build starts here: the bytes come out of the
    /// chain's own reveal, never out of a launchpad's website, so the fixture
    /// a matcher is later built against is itself chain-proven.
    pub fn recover_program(&self, covenant_id: &[u8; 32]) -> Result<Option<Vec<u8>>> {
        crate::bench::recover_program(&self.conn, covenant_id)
    }

    pub fn force_reverify(&mut self) -> Result<u64> {
        use crate::tokens::TOKEN_DERIVATION_META;
        self.conn
            .execute(
                "DELETE FROM meta WHERE key IN (?1, 'market_program_version')",
                params![TOKEN_DERIVATION_META],
            )
            .map_err(db_err)?;
        self.derive_tokens_if_stale()
    }

    /// Every trade this key took the other side of, newest first.
    ///
    /// An address that bought and then sold out holds nothing, so a holdings
    /// lookup finds nothing and the page looked empty for someone with real
    /// history. Trading is a third index, alongside covenant ownership and
    /// token balances, and a key can appear in any combination of them.
    ///
    /// Types 0x00 and 0x03 are the same x-only key, so both prefixes are asked
    /// for; the counterparty is stored in the RAW hex(type || key) form.
    pub fn trades_by_key(
        &self,
        pubkey: &[u8],
        limit: u32,
    ) -> Result<Vec<(CovenantId, crate::tokens::TokenTradeRow)>> {
        let x_only = if pubkey.len() == 33 {
            &pubkey[1..]
        } else {
            pubkey
        };
        if x_only.len() != 32 {
            return Ok(Vec::new());
        }
        let hex_key = hex::encode(x_only);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT token_id, seq, txid, market_covenant_id, side, base_amount, quote_sompi,
                        kas_before_sompi, kas_after_sompi, base_before, base_after,
                        co_covenants, accepting_daa, accepting_time_ms, counterparty
                 FROM token_trades
                 WHERE counterparty IN (?1, ?2)
                 ORDER BY accepting_daa DESC, seq DESC
                 LIMIT ?3",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(
                params![format!("00{hex_key}"), format!("03{hex_key}"), limit],
                |r| {
                    Ok((
                        CovenantId(r.get(0)?),
                        crate::tokens::TokenTradeRow {
                            seq: r.get::<_, i64>(1)? as u64,
                            txid: crate::TxId(r.get(2)?),
                            market_covenant_id: CovenantId(r.get(3)?),
                            side: r.get(4)?,
                            base_amount: r.get(5)?,
                            quote_sompi: r.get(6)?,
                            kas_before_sompi: r.get(7)?,
                            kas_after_sompi: r.get(8)?,
                            base_before: r.get(9)?,
                            base_after: r.get(10)?,
                            co_covenants: r.get(11)?,
                            accepting_daa: r.get::<_, i64>(12)? as u64,
                            accepting_time_ms: r.get(13)?,
                            counterparty: r.get(14)?,
                        },
                    ))
                },
            )
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// How many verified trades this token has, so a UI can offer "all of
    /// them" with a real number instead of the size of whatever page it got.
    pub fn token_trades_count(&self, id: &CovenantId) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM token_trades WHERE token_id = ?1",
                [id.0.as_slice()],
                |r| r.get(0),
            )
            .map_err(db_err)
    }

    /// How kascov reads one transaction as a trade, if it admitted one.
    ///
    /// This is what makes a tx permalink answerable: a reader comparing two
    /// indexers wants to see the reading, not infer it from cell values. The
    /// pool balances before and after are included precisely because they are
    /// what a disagreement is settled with.
    pub fn trade_by_txid(
        &self,
        txid: &[u8; 32],
    ) -> Result<Option<(CovenantId, crate::tokens::TokenTradeRow)>> {
        self.conn
            .query_row(
                "SELECT token_id, seq, txid, market_covenant_id, side, base_amount, quote_sompi,
                        kas_before_sompi, kas_after_sompi, base_before, base_after,
                        co_covenants, accepting_daa, accepting_time_ms, counterparty
                 FROM token_trades WHERE txid = ?1 LIMIT 1",
                [txid.as_slice()],
                |r| {
                    Ok((
                        CovenantId(r.get(0)?),
                        crate::tokens::TokenTradeRow {
                            seq: r.get::<_, i64>(1)? as u64,
                            txid: crate::TxId(r.get(2)?),
                            market_covenant_id: CovenantId(r.get(3)?),
                            side: r.get(4)?,
                            base_amount: r.get(5)?,
                            quote_sompi: r.get(6)?,
                            kas_before_sompi: r.get(7)?,
                            kas_after_sompi: r.get(8)?,
                            base_before: r.get(9)?,
                            base_after: r.get(10)?,
                            co_covenants: r.get(11)?,
                            accepting_daa: r.get::<_, i64>(12)? as u64,
                            accepting_time_ms: r.get(13)?,
                            counterparty: r.get(14)?,
                        },
                    ))
                },
            )
            .optional()
            .map_err(db_err)
    }

    /// Which decoded tokens this key holds, and how much of each.
    ///
    /// `token_balances.owner` is `hex(identifier_type || owner_identifier)`.
    /// Types 0x00 (pubkey) and 0x03 (presence) are the SAME x-only key — the
    /// authorization differs, not the identity — so an address page must ask
    /// for both or it under-reports the holder. Types 0x01 (script) and 0x02
    /// (covenant) are different entities entirely and are deliberately absent.
    pub fn token_holdings_for_pubkey(&self, pubkey: &[u8]) -> Result<Vec<TokenHoldingRow>> {
        // x-only: a 33-byte compressed key carries a parity prefix the state
        // block never stores, so index on the trailing 32 bytes.
        let x_only = if pubkey.len() == 33 {
            &pubkey[1..]
        } else {
            pubkey
        };
        if x_only.len() != 32 {
            return Ok(Vec::new());
        }
        let hex_key = hex::encode(x_only);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT b.token_id, b.owner, b.balance, b.cells, t.status, t.supply
                 FROM token_balances b
                 JOIN tokens t ON t.token_id = b.token_id
                 WHERE b.owner IN (?1, ?2) AND b.balance > 0
                 ORDER BY b.balance DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(
                params![format!("00{hex_key}"), format!("03{hex_key}")],
                |r| {
                    let owner: String = r.get(1)?;
                    Ok(TokenHoldingRow {
                        token_id: crate::CovenantId(r.get(0)?),
                        owner_kind: match owner.get(..2) {
                            Some("00") => "pubkey".into(),
                            Some("03") => "presence".into(),
                            _ => "other".into(),
                        },
                        balance: r.get(2)?,
                        cells: r.get(3)?,
                        status: r.get(4)?,
                        supply: r.get(5)?,
                    })
                },
            )
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Every token's current verdict. Taken before the derived tables are
    /// dropped, so a pass can report what it CHANGED rather than only what it
    /// ended up with.
    fn token_status_snapshot(&self) -> Result<std::collections::BTreeMap<[u8; 32], String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT token_id, status FROM tokens")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, [u8; 32]>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Open a run row. Also stamps any row a previous process left open as
    /// `interrupted` and prunes the log to its retention window.
    ///
    /// "Interrupted" is inferred, not observed, and the inference is only
    /// sound because passes are serialised: a still-open row means whoever
    /// wrote it is not writing any more. `Command::Restamp` opens the same
    /// database file and can run while the follower is mid-pass, so a manual
    /// restamp can mark a live run interrupted. That is a wrong label on a
    /// real event, not an invented event, and the page says so.
    fn begin_derivation_run(&mut self, kind: &str, stamp: &str) -> Result<i64> {
        let now = now_ms() as i64;
        self.conn
            .execute(
                "UPDATE derivation_runs SET outcome = 'interrupted', finished_ms = ?1
                 WHERE outcome IS NULL",
                params![now],
            )
            .map_err(db_err)?;
        self.conn
            .execute(
                "DELETE FROM derivation_runs WHERE run_id <= (
                     SELECT MAX(run_id) - 200 FROM derivation_runs)",
                [],
            )
            .map_err(db_err)?;
        let daa = self.processed_daa()?.map(|d| d as i64);
        self.conn
            .execute(
                "INSERT INTO derivation_runs (kind, started_ms, processed_daa, stamp)
                 VALUES (?1, ?2, ?3, ?4)",
                params![kind, now, daa, stamp],
            )
            .map_err(db_err)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Tally the state of the derived tables and close the run row.
    ///
    /// `markets_matched` and `markets_invariant_failed` read an ALLOWLIST of
    /// audited skeletons. A denylist (`skeleton NOT LIKE 'unmatched%'`) would
    /// count every give-up row as matched the moment the tag changes, and —
    /// because market.rs's 3-column unmatched INSERT OR REPLACE lets
    /// `invariant_ok` fall back to its DEFAULT 0 — would report every one of
    /// testnet's 227 unmatched programs as "its own formula failed on its own
    /// trades". That is not an unproven figure, it is a false one.
    fn finish_derivation_run(
        &mut self,
        run_id: i64,
        kind: &str,
        examined: u64,
        before: &std::collections::BTreeMap<[u8; 32], String>,
        error: Option<&str>,
    ) -> Result<()> {
        let allow: [&str; 6] = crate::market::MATCHED_SKELETONS;
        let (verified, unvalidated, invalid) = self
            .conn
            .query_row(
                "SELECT
                     SUM(CASE WHEN status = 'verified' THEN 1 ELSE 0 END),
                     SUM(CASE WHEN status = 'unvalidated' THEN 1 ELSE 0 END),
                     SUM(CASE WHEN status NOT IN ('verified','unvalidated') THEN 1 ELSE 0 END)
                 FROM tokens",
                [],
                |r| {
                    Ok((
                        r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                        r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                        r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    ))
                },
            )
            .map_err(db_err)?;

        let after: std::collections::BTreeMap<[u8; 32], String> = self
            .conn
            .prepare("SELECT token_id, status FROM tokens")
            .map_err(db_err)?
            .query_map([], |r| {
                Ok((r.get::<_, [u8; 32]>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        let added = after.keys().filter(|k| !before.contains_key(*k)).count() as i64;
        let removed = before.keys().filter(|k| !after.contains_key(*k)).count() as i64;
        let mut changes: Vec<serde_json::Value> = Vec::new();
        for (id, now_status) in &after {
            if let Some(was) = before.get(id) {
                if was != now_status {
                    changes.push(serde_json::json!({
                        "token": hex::encode(id), "from": was, "to": now_status,
                    }));
                }
            }
        }
        let changed = changes.len() as i64;
        changes.truncate(64);

        // Market composition, allowlisted. A token whose market covenant has
        // no market_programs row at all is 'unrevealed', never 'unmatched'.
        let (m_examined, matched, unmatched, unrevealed, inv_failed) = self
            .conn
            .query_row(
                // The IN list arity must equal MATCHED_SKELETONS.len() —
                // the compile-time destructure below fails the build if the
                // allowlist grows without this query following it.
                "SELECT
                    COUNT(*),
                    SUM(CASE WHEN mp.skeleton IN (?1, ?2, ?3, ?4, ?5) THEN 1 ELSE 0 END),
                    SUM(CASE WHEN mp.skeleton IS NOT NULL AND mp.skeleton NOT IN (?1, ?2, ?3, ?4, ?5)
                             THEN 1 ELSE 0 END),
                    SUM(CASE WHEN mp.skeleton IS NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN mp.skeleton IN (?1, ?2, ?3, ?4, ?5) AND mp.invariant_ok = 0
                             THEN 1 ELSE 0 END)
                 FROM (SELECT DISTINCT market_covenant_id AS c FROM tokens
                       WHERE market_covenant_id IS NOT NULL) t
                 LEFT JOIN market_programs mp ON mp.covenant_id = t.c",
                params![allow[0], allow[1], allow[2], allow[3], allow[4]],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                        r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                        r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                        r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    ))
                },
            )
            .map_err(db_err)?;

        let outcome = if error.is_some() { "degraded" } else { "ok" };
        self.conn
            .execute(
                "UPDATE derivation_runs SET outcome = ?1, finished_ms = ?2,
                     tokens_examined = ?3, tokens_verified = ?4, tokens_unvalidated = ?5,
                     tokens_invalid = ?6, tokens_added = ?7, tokens_removed = ?8,
                     verdicts_changed = ?9, markets_examined = ?10, markets_matched = ?11,
                     markets_unmatched = ?12, markets_unrevealed = ?13,
                     markets_invariant_failed = ?14, changes_json = ?15, error = ?16
                 WHERE run_id = ?17",
                params![
                    outcome,
                    now_ms() as i64,
                    examined as i64,
                    verified,
                    unvalidated,
                    invalid,
                    if kind == "full" { added } else { 0 },
                    if kind == "full" { removed } else { 0 },
                    if kind == "full" { changed } else { 0 },
                    m_examined,
                    matched,
                    unmatched,
                    unrevealed,
                    inv_failed,
                    if changes.is_empty() {
                        None
                    } else {
                        Some(serde_json::Value::from(changes).to_string())
                    },
                    error,
                    run_id,
                ],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Close a run row that died on an error. Without this the row stays open
    /// and the NEXT pass mislabels a failure as `interrupted` — "the process
    /// died" when in fact it ran and returned an error that was discarded.
    fn fail_derivation_run(&mut self, run_id: i64, err: &str) {
        let _ = self.conn.execute(
            "UPDATE derivation_runs SET outcome = 'failed', finished_ms = ?1, error = ?2
             WHERE run_id = ?3",
            params![now_ms() as i64, err, run_id],
        );
    }

    /// The newest runs, newest first. A reverse walk of the integer primary
    /// key — no index needed.
    pub fn derivation_runs(&self, limit: u32) -> Result<Vec<DerivationRunRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT run_id, kind, outcome, started_ms, finished_ms, processed_daa, stamp,
                        tokens_examined, tokens_verified, tokens_unvalidated, tokens_invalid,
                        tokens_added, tokens_removed, verdicts_changed,
                        markets_examined, markets_matched, markets_unmatched,
                        markets_unrevealed, markets_invariant_failed, changes_json, error
                 FROM derivation_runs ORDER BY run_id DESC LIMIT ?1",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(DerivationRunRow {
                    run_id: r.get(0)?,
                    kind: r.get(1)?,
                    outcome: r.get(2)?,
                    started_ms: r.get(3)?,
                    finished_ms: r.get(4)?,
                    processed_daa: r.get(5)?,
                    stamp: r.get(6)?,
                    tokens_examined: r.get(7)?,
                    tokens_verified: r.get(8)?,
                    tokens_unvalidated: r.get(9)?,
                    tokens_invalid: r.get(10)?,
                    tokens_added: r.get(11)?,
                    tokens_removed: r.get(12)?,
                    verdicts_changed: r.get(13)?,
                    markets_examined: r.get(14)?,
                    markets_matched: r.get(15)?,
                    markets_unmatched: r.get(16)?,
                    markets_unrevealed: r.get(17)?,
                    markets_invariant_failed: r.get(18)?,
                    changes: r
                        .get::<_, Option<String>>(19)?
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    error: r.get(20)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// How many unknown builds exist in total, so a capped list can say what
    /// it is not showing. A page about what kascov could not verify must not
    /// itself quietly hide part of the answer.
    pub fn unknown_build_totals(&self) -> Result<(i64, i64)> {
        self.conn
            .query_row(
                // families, then covenants — the page reports both
                "SELECT COUNT(*), COALESCE(SUM(n), 0) FROM (
                     SELECT COUNT(*) AS n FROM market_programs
                     WHERE skeleton GLOB 'unmatched*'
                     GROUP BY COALESCE(program_len, -1), COALESCE(program_pushes, -1),
                              CASE WHEN program_len IS NULL
                                   THEN hex(program_hash) ELSE '' END)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db_err)
    }

    /// Market programs kascov could not match, grouped by the exact bytes it
    /// could not match, ranked by how much activity rides on each.
    ///
    /// This is a TO-AUDIT list. A build appearing here has proven nothing; a
    /// high rank means more is at stake if it is never audited, never that it
    /// is more likely to be sound.
    pub fn unknown_builds(&self, limit: u32) -> Result<Vec<UnknownBuildRow>> {
        let mut stmt = self
            .conn
            .prepare(
                // GLOB, not LIKE: see the mp_unknown index comment. Also note
                // covenants.last_activity_daa is NOT NULL DEFAULT 0, so a
                // COALESCE fallback to trade DAA would never fire and could
                // publish a "last seen" older than "first seen"; take the max
                // of both and let 0 mean unknown.
                "SELECT MIN(hex(mp.program_hash)) AS h,
                        COUNT(DISTINCT mp.covenant_id) AS covenants,
                        COALESCE(SUM(tr.trades), 0) AS trades,
                        COALESCE(SUM(tr.volume), 0) AS volume,
                        COALESCE(MAX(tr.tokens), 0) AS tokens,
                        MIN(NULLIF(c.genesis_daa, 0)) AS first_daa,
                        MAX(MAX(COALESCE(c.last_activity_daa, 0), COALESCE(tr.last_daa, 0))) AS last_daa,
                        MIN(hex(mp.covenant_id)) AS sample,
                        mp.program_len, mp.program_pushes
                 FROM market_programs mp
                 LEFT JOIN covenants c ON c.covenant_id = mp.covenant_id
                 LEFT JOIN (
                     SELECT market_covenant_id AS m, COUNT(*) AS trades,
                            SUM(quote_sompi) AS volume, COUNT(DISTINCT token_id) AS tokens,
                            MAX(accepting_daa) AS last_daa
                     FROM token_trades GROUP BY market_covenant_id
                 ) tr ON tr.m = mp.covenant_id
                 WHERE mp.skeleton GLOB 'unmatched*'
                 -- Group by SHAPE, not by hash. Grouping on program_hash
                 -- clusters nothing: a curve program bakes its own token's
                 -- constants into its bytes, so 222 deployments produced 222
                 -- unique hashes and 222 rows of one. Programs whose length
                 -- and push count agree are the same build with different
                 -- constants, which is the family an auditor wants.
                 -- COALESCE so rows written before this column existed fall
                 -- back to per-hash grouping rather than merging into one
                 -- bogus NULL family.
                 GROUP BY COALESCE(mp.program_len, -1),
                          COALESCE(mp.program_pushes, -1),
                          CASE WHEN mp.program_len IS NULL
                               THEN hex(mp.program_hash) ELSE '' END
                 ORDER BY volume DESC, trades DESC, covenants DESC
                 LIMIT ?1",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(UnknownBuildRow {
                    program_hash: r.get::<_, String>(0)?.to_lowercase(),
                    covenants: r.get(1)?,
                    trades: r.get(2)?,
                    volume_sompi: r.get(3)?,
                    tokens: r.get(4)?,
                    first_daa: r.get(5)?,
                    last_daa: r.get::<_, Option<i64>>(6)?.filter(|d| *d > 0),
                    sample_covenant: r.get::<_, String>(7)?.to_lowercase(),
                    program_len: r.get(8)?,
                    program_pushes: r.get(9)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    pub fn derive_tokens_if_stale(&mut self) -> Result<u64> {
        use crate::tokens::{token_derivation_stamp, TOKEN_DERIVATION_META};
        // The stamp is a COMPOSITE of the derivation, classifier and restamp
        // versions: three passes rewrite decoder stamps without touching the
        // derivation constant, and any of them changing what a program decodes
        // as must mechanically invalidate every stored trade and price.
        let stamp = token_derivation_stamp();
        if self.meta(TOKEN_DERIVATION_META)?.as_deref() == Some(stamp.as_str()) {
            // The trade rows are current, but market-program verification has
            // its own one-shot gate: it arrived after v7 stamped these rows,
            // and without this a quiet token's market would stay unverified
            // until its next trade happened to touch it.
            const MARKET_META: &str = "market_program_version";
            // Composite with the matcher version (see market::market_stamp):
            // teaching the matcher a new build has to invalidate this gate, or
            // the stored 'unmatched:N' rows are never retried and the new build
            // stays invisible until each covenant happens to trade again.
            let market_version = crate::market::market_stamp();
            if self.meta(MARKET_META)?.as_deref() != Some(market_version.as_str()) {
                // Every covenant a token points at today, PLUS every row
                // already in market_programs. The union is the point: a
                // graduated token's old curve is nobody's market any more,
                // and iterating only live links left such rows tagged by a
                // retired matcher forever — "retry the stored verdicts" has
                // to mean all of them.
                let markets: std::collections::BTreeSet<[u8; 32]> = self
                    .conn
                    .prepare(
                        "SELECT DISTINCT market_covenant_id FROM tokens
                         WHERE market_covenant_id IS NOT NULL
                         UNION
                         SELECT covenant_id FROM market_programs",
                    )
                    .map_err(db_err)?
                    .query_map([], |r| r.get::<_, [u8; 32]>(0))
                    .map_err(db_err)?
                    .collect::<std::result::Result<_, _>>()
                    .map_err(db_err)?;
                // A markets-only pass gets its own row, but ONLY when it has
                // work: the follower calls this on every restart, and a
                // zero-everything row per restart would push real passes out
                // of the retention window.
                let run_id = self.begin_derivation_run("markets", &stamp)?;
                let before = self.token_status_snapshot()?;
                match crate::market::rederive_market_programs(&self.conn, &markets) {
                    Ok(()) => {
                        self.set_meta(MARKET_META, market_version.as_str())?;
                        self.finish_derivation_run(run_id, "markets", 0, &before, None)?;
                    }
                    Err(err) => {
                        // The gate is NOT advanced: a failed re-verification
                        // must run again next boot rather than being recorded
                        // as done.
                        let msg = err.to_string();
                        self.fail_derivation_run(run_id, &msg);
                        return Err(err);
                    }
                }
            }
            return Ok(0);
        }
        // Snapshot the verdicts BEFORE the DELETE: this is the only moment
        // the previous pass's answers still exist, and the difference between
        // them and the new ones is the narrative this log exists to keep.
        let before = self.token_status_snapshot()?;
        let run_id = self.begin_derivation_run("full", &stamp)?;
        let outcome = (|| -> Result<u64> {
            let tx = self.conn.transaction().map_err(db_err)?;
            for sql in [
                "DELETE FROM token_events",
                "DELETE FROM token_balances",
                "DELETE FROM token_minters",
                "DELETE FROM token_trades",
                "DELETE FROM tokens",
            ] {
                tx.execute(sql, []).map_err(db_err)?;
            }
            tx.commit().map_err(db_err)?;
            // Candidate enumeration — the WHERE must stay verbatim-identical to
            // the utxo_kcc20 partial-index predicate so this is an index walk,
            // not a utxo-table scan.
            let candidates: Vec<([u8; 32], bool, bool)> = {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT covenant_id,
                                MAX(CASE WHEN template = 'KCC20 token' OR revealed_template = 'KCC20 token' THEN 1 ELSE 0 END),
                                MAX(CASE WHEN template = 'KCC20 minter' OR revealed_template = 'KCC20 minter' THEN 1 ELSE 0 END)
                         FROM covenant_utxos
                         WHERE template IN ('KCC20 token','KCC20 minter')
                            OR revealed_template IN ('KCC20 token','KCC20 minter')
                         GROUP BY covenant_id",
                    )
                    .map_err(db_err)?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get(0)?,
                            r.get::<_, i64>(1)? != 0,
                            r.get::<_, i64>(2)? != 0,
                        ))
                    })
                    .map_err(db_err)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(db_err)?;
                rows
            };
            let mut token_set: std::collections::BTreeSet<[u8; 32]> = Default::default();
            // Minters first: their pinned ids join the token set (a pinned id
            // with no KCC20 evidence of its own still gets an honest
            // 'unvalidated' row).
            {
                let tx = self.conn.transaction().map_err(db_err)?;
                for (id, _, minter_ev) in &candidates {
                    if *minter_ev {
                        token_set.extend(crate::tokens::derive_minter(&tx, id)?);
                    }
                }
                tx.commit().map_err(db_err)?;
            }
            token_set.extend(
                candidates
                    .iter()
                    .filter(|(_, token_ev, _)| *token_ev)
                    .map(|(id, _, _)| *id),
            );
            let ids: Vec<[u8; 32]> = token_set.into_iter().collect();
            for chunk in ids.chunks(32) {
                let tx = self.conn.transaction().map_err(db_err)?;
                for id in chunk {
                    crate::tokens::derive_token(&tx, id)?;
                }
                tx.commit().map_err(db_err)?;
            }
            // Verify every market covenant the derivation just linked: read its
            // program constants out of committed bytes and replay its trades.
            // A failure here downgrades that covenant's figures, never the pass.
            {
                // Same union as the gated pass above: stored rows whose
                // covenant no token links any more still deserve the current
                // matcher's verdict.
                let markets: std::collections::BTreeSet<[u8; 32]> = self
                    .conn
                    .prepare(
                        "SELECT DISTINCT market_covenant_id FROM tokens
                         WHERE market_covenant_id IS NOT NULL
                         UNION
                         SELECT covenant_id FROM market_programs",
                    )
                    .map_err(db_err)?
                    .query_map([], |r| r.get::<_, [u8; 32]>(0))
                    .map_err(db_err)?
                    .collect::<std::result::Result<_, _>>()
                    .map_err(db_err)?;
                if let Err(err) = crate::market::rederive_market_programs(&self.conn, &markets) {
                    tracing::warn!("market-program verification failed: {err}");
                }
            }
            self.set_meta(TOKEN_DERIVATION_META, &stamp)?;
            Ok(ids.len() as u64)
        })();
        match outcome {
            Ok(n) => {
                self.finish_derivation_run(run_id, "full", n, &before, None)?;
                Ok(n)
            }
            Err(err) => {
                // Close it as FAILED. Leaving it open would let the next pass
                // stamp it 'interrupted', which claims the process died when in
                // fact it ran and returned an error nobody kept.
                self.fail_derivation_run(run_id, &err.to_string());
                Err(err)
            }
        }
    }

    /// The last completed token-derivation version, if any — serves as the
    /// "derivation pending" signal for the API.
    pub fn token_derivation_version(&self) -> Result<Option<String>> {
        self.meta(crate::tokens::TOKEN_DERIVATION_META)
    }

    /// Every derived token, newest activity first — the tokens.json source.
    pub fn token_directory(&self) -> Result<Vec<crate::tokens::TokenDirRow>> {
        crate::tokens::token_directory(&self.conn)
    }

    /// One derived token's directory row.
    pub fn token_row(&self, id: &CovenantId) -> Result<Option<crate::tokens::TokenDirRow>> {
        crate::tokens::token_row(&self.conn, &id.0)
    }

    /// Top holders of one token by live hash-proven balance.
    pub fn token_balances(
        &self,
        id: &CovenantId,
        limit: u64,
    ) -> Result<Vec<crate::tokens::TokenBalanceRow>> {
        crate::tokens::token_balances(&self.conn, &id.0, limit)
    }

    /// One page of a token's classified event deltas (exclusive `after_seq`
    /// cursor, oldest first).
    pub fn token_events_page(
        &self,
        id: &CovenantId,
        after_seq: Option<u64>,
        limit: u64,
    ) -> Result<Vec<crate::tokens::TokenEventRow>> {
        crate::tokens::token_events_page(&self.conn, &id.0, after_seq, limit)
    }

    /// The gated market summary for one token, computed at serve time. `deep`
    /// widens the trade scan from the directory's 32 to a token page's 1000.
    pub fn token_market_summary(
        &self,
        row: &crate::tokens::TokenDirRow,
        deep: bool,
    ) -> Result<crate::market::MarketSummary> {
        let tip_ms = self.tip()?.map(|(_, ms)| ms as i64);
        crate::market::market_summary(
            &self.conn,
            &row.token_id.0,
            row.market_covenant_id.as_ref().map(|c| &c.0),
            row.held_covenant,
            row.held_wallet,
            row.held_script,
            row.trades_missing_time,
            tip_ms,
            if deep { 1000 } else { 32 },
        )
    }

    /// Newest admitted trades first, as stored by the derivation.
    pub fn token_trades_page(
        &self,
        id: &CovenantId,
        limit: u64,
    ) -> Result<Vec<crate::tokens::TokenTradeRow>> {
        crate::tokens::token_trades_page(&self.conn, &id.0, limit)
    }

    /// One page of a token's event deltas walking BACKWARDS from the tip.
    pub fn token_events_page_before(
        &self,
        id: &CovenantId,
        before_seq: Option<u64>,
        limit: u64,
    ) -> Result<Vec<crate::tokens::TokenEventRow>> {
        crate::tokens::token_events_page_before(&self.conn, &id.0, before_seq, limit)
    }

    /// Every registered minter/vault covenant with the token ids it pins.
    pub fn token_minter_directory(&self) -> Result<Vec<crate::tokens::TokenMinterRow>> {
        crate::tokens::token_minter_directory(&self.conn)
    }

    /// How many classified events the validator walked for one token.
    pub fn token_event_count(&self, id: &CovenantId) -> Result<u64> {
        crate::tokens::token_event_count(&self.conn, &id.0)
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
                    outpoint: Outpoint {
                        txid: TxId(row.get(0)?),
                        index: row.get(1)?,
                    },
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
        let path = std::env::temp_dir().join(format!(
            "kascov-store-test-{}-{name}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn test_store(name: &str) -> Store {
        Store::open(&test_store_path(name), Network::Testnet(10)).unwrap()
    }

    /// The archive-boot policy. Under `Refuse` a missing file is an error
    /// that tells the operator both what refused and how to overrule it —
    /// never a silently created empty database.
    #[test]
    fn refuse_fresh_errs_on_a_missing_db_and_names_the_escape_hatch() {
        let path = test_store_path("fresh-refuse-missing");
        let Err(err) = Store::open_with_policy(&path, Network::Testnet(10), FreshDb::Refuse) else {
            panic!("a missing archive must not be replaced by an empty one");
        };
        let msg = err.to_string();
        assert!(msg.contains("KASCOV_FRESH_OK"), "no escape hatch in: {msg}");
        assert!(
            msg.contains(&path.display().to_string()),
            "no path in: {msg}"
        );
        assert!(
            !path.exists(),
            "the refusal must not create the file either"
        );
    }

    /// SQLite treats a zero-byte file exactly like a missing one, so Refuse
    /// must too.
    #[test]
    fn refuse_fresh_errs_on_a_zero_byte_db() {
        let path = test_store_path("fresh-refuse-empty");
        std::fs::write(&path, b"").unwrap();
        let Err(err) = Store::open_with_policy(&path, Network::Testnet(10), FreshDb::Refuse) else {
            panic!("a zero-byte file is a fresh database in sqlite's eyes");
        };
        assert!(err.to_string().contains("KASCOV_FRESH_OK"));
    }

    #[test]
    fn refuse_fresh_opens_an_existing_db() {
        let path = test_store_path("fresh-refuse-existing");
        drop(Store::open(&path, Network::Testnet(10)).unwrap());
        Store::open_with_policy(&path, Network::Testnet(10), FreshDb::Refuse)
            .expect("an existing archive opens under Refuse");
    }

    /// `Store::open` stays the Allow wrapper: the whole test suite (and any
    /// first-time setup) creates its databases through it.
    #[test]
    fn allow_fresh_still_creates_a_missing_db() {
        let path = test_store_path("fresh-allow-missing");
        Store::open(&path, Network::Testnet(10)).expect("Allow creates");
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
    }

    fn block_with_events(hash: u8, daa: u64, events: Vec<(u8, EventKind, u8)>) -> BlockEvents {
        BlockEvents {
            accepting_block: BlockHash([hash; 32]),
            accepting_daa: daa,
            accepting_time_ms: daa * 1000,
            accepting_blue_score: daa,
            events: events
                .into_iter()
                .enumerate()
                .map(|(i, (cov, kind, tx))| NewEvent {
                    covenant_id: CovenantId([cov; 32]),
                    kind,
                    txid: TxId([tx; 32]),
                    tx_index: i as u32,
                    payload: None,
                    lane_namespace: None,
                })
                .collect(),
            created_utxos: vec![],
            spent_utxos: vec![],
        }
    }

    /// A genesis payload is written by whoever deployed the covenant, so every
    /// string in it is attacker-controlled. KCC-0021 requires an `image` to
    /// carry an `https://` or `ipfs://` scheme and says anything else "MUST be
    /// rejected at parse and the field dropped, so that a non-conforming scheme
    /// can never reach the fetch pipeline". Dropping it here, rather than
    /// trusting every consumer to re-check, is what keeps a `javascript:` URL
    /// off the token page, where it would render as a clickable href that
    /// escaping and rel/target attributes do nothing to defuse.
    #[test]
    fn claimed_image_keeps_only_conforming_schemes() {
        let cases = [
            ("https://example.test/a.png", true),
            ("ipfs://bafyexample", true),
            ("HTTPS://EXAMPLE.TEST/A.PNG", true), // scheme is case-insensitive
            ("javascript:alert(document.domain)", false),
            ("  javascript:alert(1)", false), // trimmed before the check
            ("data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=", false),
            ("http://example.test/a.png", false),
            ("//example.test/a.png", false),
        ];
        for (i, (url, keep)) in cases.iter().enumerate() {
            let cov = 0xD0 + i as u8;
            let mut store = test_store(&format!("claim-img-{i}"));
            let mut blk =
                block_with_events(1, 100, vec![(cov, EventKind::Genesis, 0xA0 + i as u8)]);
            let payload = serde_json::json!({
                "name": "Example", "ticker": "EX", "image": url,
            });
            blk.events[0].payload = Some(payload.to_string().into_bytes());
            store.apply(&blk, BlockHash([1; 32])).unwrap();

            let meta = store
                .claimed_token_meta(&CovenantId([cov; 32]))
                .unwrap()
                .unwrap();
            // The sibling fields survive either way: one bad field is dropped,
            // it never invalidates the whole object.
            assert_eq!(meta.name.as_deref(), Some("Example"), "{url}");
            assert_eq!(meta.ticker.as_deref(), Some("EX"), "{url}");
            assert_eq!(
                meta.image.is_some(),
                *keep,
                "image {url:?} should {} have survived",
                if *keep { "" } else { "NOT" }
            );
        }
    }

    /// Regression: a tag lane's detail queries (lane_stats / lane_recent /
    /// lane_activity) must count the SAME rows lanes.json advertises. Generic
    /// based-app tag lanes live in payload_tag as 'tag:<hex>' with
    /// lane_namespace NULL, so the old `WHERE lane_namespace = ?1` filter read
    /// 0 for every tag lane while lanes.json reported thousands.
    #[test]
    fn lane_detail_includes_payload_tag_lanes() {
        let mut store = test_store("lane-tag");
        // Two covenants whose payloads lead with the ASCII tag "GZ4M"
        // (0x47 0x5a 0x34 0x4d) => payload_tag "tag:475a344d", lane_namespace
        // NULL — a generic tag lane, not a strict KIP-21 namespace.
        let mut blk = block_with_events(
            1,
            100,
            vec![
                (0xC1, EventKind::Genesis, 0x0A),
                (0xC2, EventKind::Genesis, 0x0B),
            ],
        );
        blk.events[0].payload = Some(b"GZ4M-hello".to_vec());
        blk.events[1].payload = Some(b"GZ4M-world".to_vec());
        store.apply(&blk, BlockHash([1; 32])).unwrap();

        let ns = "475a344d";
        assert_eq!(
            store.lane_stats(ns).unwrap(),
            (2, 2),
            "tag-lane events must be counted"
        );
        assert_eq!(
            store.lane_recent(ns, 50).unwrap().len(),
            2,
            "recent must include tag-lane events"
        );
        let total: u64 = store
            .lane_activity(ns, 36_000)
            .unwrap()
            .iter()
            .map(|(_, c)| c)
            .sum();
        assert_eq!(total, 2, "activity buckets must include tag-lane events");
        // An unrelated namespace stays empty — no cross-lane leakage.
        assert_eq!(store.lane_stats("deadbeef").unwrap(), (0, 0));

        // Disjointness: once a STRICT KIP-21 lane exists under the same hex,
        // the detail view must report that lane alone. lanes.json counts strict
        // lanes and tag lanes as separate entries (its tag aggregation filters
        // to lane_namespace IS NULL), so unioning them would double-count.
        let mut blk = block_with_events(2, 200, vec![(0xC3, EventKind::Transition, 0x0C)]);
        blk.events[0].payload = Some(b"GZ4M-strict".to_vec());
        blk.events[0].lane_namespace = Some(ns.to_string());
        store.apply(&blk, BlockHash([2; 32])).unwrap();
        assert_eq!(
            store.lane_stats(ns).unwrap(),
            (1, 1),
            "a strict lane must not be unioned with the same-hex tag lane"
        );
        assert_eq!(store.lane_recent(ns, 50).unwrap().len(), 1);
    }

    /// A sink-reset gap is the widest discontinuity in the DAA distribution;
    /// routine quiet stretches below the threshold are never called a gap.
    #[test]
    fn find_daa_gap_spots_the_reset_discontinuity() {
        let mut store = test_store("gap-find");
        for (hash, daa) in [(1u8, 100u64), (2, 200), (3, 2_000_000), (4, 2_000_050)] {
            store
                .apply(
                    &block_with_events(hash, daa, vec![(0xA1, EventKind::Transition, hash)]),
                    BlockHash([hash; 32]),
                )
                .unwrap();
        }
        assert_eq!(store.find_daa_gap(100_000).unwrap(), Some((200, 2_000_000)));
        // A threshold above the widest discontinuity finds nothing.
        assert_eq!(store.find_daa_gap(3_000_000).unwrap(), None);
        // An empty-ish window (single distinct DAA) can't produce a gap.
        let store2 = test_store("gap-find-empty");
        assert_eq!(store2.find_daa_gap(1).unwrap(), None);
    }

    /// finalize_gap_recovery renumbers merged rows chronologically via the
    /// negative-temp two-step (no (covenant_id, seq) PK collision), refreshes
    /// the covenant summary from the merged truth, and re-derives any token
    /// whose derived rows cite the covenant — stale (covenant_id, seq)
    /// citations cannot survive the renumber.
    #[test]
    fn finalize_gap_recovery_resequences_and_rederives_citing_tokens() {
        let mut store = test_store("gap-finalize");
        let cov = CovenantId([0xA1; 32]);
        // Pre-gap genesis (daa 100) + post-gap transition (daa 2_000_000) —
        // exactly the shape a sink reset leaves behind.
        store
            .apply(
                &block_with_events(1, 100, vec![(0xA1, EventKind::Genesis, 0x0A)]),
                BlockHash([1; 32]),
            )
            .unwrap();
        store
            .apply(
                &block_with_events(2, 2_000_000, vec![(0xA1, EventKind::Transition, 0x0C)]),
                BlockHash([2; 32]),
            )
            .unwrap();
        // The gap event arrives out of order through the merge path (twice —
        // the second offer must dedup away).
        let gap_block = block_with_events(3, 1_000_000, vec![(0xA1, EventKind::Transition, 0x0B)]);
        assert_eq!(
            store
                .merge_recovered_block(&gap_block)
                .unwrap()
                .events_added,
            1
        );
        assert_eq!(
            store
                .merge_recovered_block(&gap_block)
                .unwrap()
                .events_added,
            0
        );
        // A token whose derived rows cite the covenant's about-to-move seq 1.
        store
            .raw_conn()
            .execute(
                "INSERT INTO tokens (token_id, status) VALUES (?1, 'unvalidated')",
                [[0xEEu8; 32].as_slice()],
            )
            .unwrap();
        store
            .raw_conn()
            .execute(
                "INSERT INTO token_events (token_id, covenant_id, seq, delta_idx, kind, amount, accepting_daa)
                 VALUES (?1, ?2, 1, 0, 'transfer', NULL, 2000000)",
                params![[0xEEu8; 32].as_slice(), [0xA1u8; 32].as_slice()],
            )
            .unwrap();

        let counts = store
            .finalize_gap_recovery(100, 2_000_000, &MergeCounts::default())
            .unwrap();
        assert_eq!(counts.covenants_refreshed, 1);
        assert_eq!(counts.covenants_resequenced, 1);
        assert_eq!(counts.tokens_rederived, 1);

        // Chronological seqs: 0x0A (100) → 0x0B (1M) → 0x0C (2M).
        let events = store.events(&cov).unwrap();
        let view: Vec<(u64, TxId)> = events.iter().map(|e| (e.seq, e.txid)).collect();
        assert_eq!(
            view,
            [
                (0, TxId([0x0A; 32])),
                (1, TxId([0x0B; 32])),
                (2, TxId([0x0C; 32]))
            ]
        );
        // Summary refreshed from the merged truth.
        let sum = store.summary(&cov).unwrap().unwrap();
        assert_eq!(sum.event_count, 3);
        assert_eq!(sum.last_activity_daa, 2_000_000);
        assert!(sum.lineage_complete);
        assert_eq!(sum.genesis_txid, Some(TxId([0x0A; 32])));
        // The citing token was re-derived: with no real KCC20 evidence and no
        // minter pin, derive_token deletes its rows — the stale seq-1
        // citation is gone with them.
        assert!(store.token_row(&CovenantId([0xEE; 32])).unwrap().is_none());
        assert_eq!(store.token_event_count(&CovenantId([0xEE; 32])).unwrap(), 0);
        // Honest history recorded, and it doubles as the idempotence marker.
        assert_eq!(store.gap_recoveries().unwrap(), [(100, 2_000_000)]);

        // Finalizing again is byte-stable on the data (marker grows, which is
        // what makes recover_gap itself a hard no-op before ever re-walking).
        let counts = store
            .finalize_gap_recovery(100, 2_000_000, &MergeCounts::default())
            .unwrap();
        assert_eq!(
            counts.covenants_resequenced, 0,
            "already in chronological order"
        );
        assert_eq!(
            store
                .events(&cov)
                .unwrap()
                .iter()
                .map(|e| (e.seq, e.txid))
                .collect::<Vec<_>>(),
            view
        );
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
            accepting_time_ms: 100_000,
            accepting_blue_score: 100,
            events: vec![
                NewEvent {
                    covenant_id: CovenantId([0xa0; 32]),
                    kind: EventKind::Genesis,
                    txid: TxId([1; 32]),
                    tx_index: 0,
                    payload: None,
                    lane_namespace: None,
                },
                NewEvent {
                    covenant_id: CovenantId(id_a1_zero),
                    kind: EventKind::Genesis,
                    txid: TxId([2; 32]),
                    tx_index: 1,
                    payload: None,
                    lane_namespace: None,
                },
                NewEvent {
                    covenant_id: CovenantId([0xa1; 32]),
                    kind: EventKind::Genesis,
                    txid: TxId([3; 32]),
                    tx_index: 2,
                    payload: None,
                    lane_namespace: None,
                },
                NewEvent {
                    covenant_id: CovenantId([0xb0; 32]),
                    kind: EventKind::Genesis,
                    txid: TxId([4; 32]),
                    tx_index: 3,
                    payload: None,
                    lane_namespace: None,
                },
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
        assert!(store
            .covenants_by_id_range(&lo2, &hi2, 20)
            .unwrap()
            .is_empty());
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
            accepting_time_ms: 100_000,
            accepting_blue_score: 100,
            events: vec![
                NewEvent {
                    covenant_id: CovenantId([1; 32]),
                    kind: EventKind::Genesis,
                    txid: TxId([1; 32]),
                    tx_index: 0,
                    payload: Some(lane_payload.clone()),
                    lane_namespace: Some(lane_ns.clone()),
                },
                // A generic (non-lane) tagged payload — must stay in the tag
                // buckets and never appear as a lane.
                NewEvent {
                    covenant_id: CovenantId([2; 32]),
                    kind: EventKind::Genesis,
                    txid: TxId([2; 32]),
                    tx_index: 1,
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

    /// A real TN10 reveal (a PURE covenant program) round-trips the whole
    /// classifier lifecycle: write-time naming, a version bump re-deriving a
    /// stamp the old classifier left generic, and the version gate keeping
    /// later opens from touching stamps again.
    #[test]
    fn classifier_bump_reclassifies_generic_stamps() {
        let path = test_store_path("classifier-bump");
        let program = include_bytes!("../../kascov-decode/fixtures/pure_a.bin");
        let hash = blake2b_simd::Params::new().hash_length(32).hash(program);
        let mut spk = vec![0xaa, 0x20];
        spk.extend_from_slice(hash.as_bytes());
        spk.push(0x87);
        // spend witness: junk arg, then the revealed program
        let mut sig = kascov_decode::encode_push(&[0x01, 0x02]);
        sig.extend_from_slice(&kascov_decode::encode_push(program));
        let outpoint = Outpoint {
            txid: TxId([7; 32]),
            index: 0,
        };
        let named = vec![("PURE".to_string(), 1u64)];

        {
            let mut store = Store::open(&path, Network::Testnet(10)).unwrap();
            let block = BlockEvents {
                accepting_block: BlockHash([1; 32]),
                accepting_daa: 10,
                accepting_time_ms: 10_000,
                accepting_blue_score: 10,
                events: vec![],
                created_utxos: vec![NewUtxo {
                    outpoint,
                    covenant_id: CovenantId([9; 32]),
                    value: 1,
                    spk_version: 0,
                    spk_script: spk,
                }],
                spent_utxos: vec![(outpoint, TxId([8; 32]), sig, 0, 0)],
            };
            store.apply(&block, BlockHash([1; 32])).unwrap();
            // Write-time recognition names the real program immediately.
            assert_eq!(store.revealed_template_counts().unwrap(), named);
            store.simulate_old_classifier_for_test().unwrap();
            assert!(store.revealed_template_counts().unwrap().is_empty());
        }
        // Version mismatch on open: the generic stamp is cleared and the
        // backfill re-derives the name from the stored reveal bytes.
        {
            let store = Store::open(&path, Network::Testnet(10)).unwrap();
            assert_eq!(store.revealed_template_counts().unwrap(), named);
        }
        // Same-version reopen is gated: a planted generic verdict survives
        // (nothing cleared, nothing re-stamped), so the pass is idempotent
        // and costs nothing once the version matches.
        {
            let store = Store::open(&path, Network::Testnet(10)).unwrap();
            store.plant_generic_stamps_for_test().unwrap();
        }
        {
            let store = Store::open(&path, Network::Testnet(10)).unwrap();
            assert!(store.revealed_template_counts().unwrap().is_empty());
        }
    }

    /// Inscriptions whose JSON runs past the old 512-byte window parse under
    /// the widened one, and a version bump re-stamps rows the old window had
    /// given up on.
    #[test]
    fn classifier_bump_rescans_long_inscriptions() {
        let path = test_store_path("insc-window");
        let payload = format!(
            "{{\"p\":\"krc-20\",\"op\":\"mint\",\"tick\":\"LONG\",\"pad\":\"{}\"}}",
            "a".repeat(600)
        )
        .into_bytes();
        assert!(payload.len() > 512 && payload.len() <= INSCRIPTION_WINDOW);
        let want = vec![("krc-20 · mint · LONG".to_string(), 1u64, 1u64)];

        {
            let mut store = Store::open(&path, Network::Testnet(10)).unwrap();
            let block = BlockEvents {
                accepting_block: BlockHash([1; 32]),
                accepting_daa: 10,
                accepting_time_ms: 10_000,
                accepting_blue_score: 10,
                events: vec![NewEvent {
                    covenant_id: CovenantId([1; 32]),
                    kind: EventKind::Genesis,
                    txid: TxId([1; 32]),
                    tx_index: 0,
                    payload: Some(payload),
                    lane_namespace: None,
                }],
                created_utxos: vec![],
                spent_utxos: vec![],
            };
            store.apply(&block, BlockHash([1; 32])).unwrap();
            assert_eq!(store.inscription_breakdown().unwrap(), want);
            // A database stamped by the 512-byte-window binary: the long
            // payload's parse came up empty.
            store.simulate_old_classifier_for_test().unwrap();
            assert!(store.inscription_breakdown().unwrap().is_empty());
        }
        {
            let store = Store::open(&path, Network::Testnet(10)).unwrap();
            assert_eq!(store.inscription_breakdown().unwrap(), want);
            assert!(!store.payload_tags_pending().unwrap());
        }
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
            tx_index: tx as u32,
            payload,
            lane_namespace: lane,
        };
        let block = BlockEvents {
            accepting_block: BlockHash([9; 32]),
            accepting_daa: 100,
            accepting_time_ms: 100_000,
            accepting_blue_score: 100,
            events: vec![
                ev(1, 1, Some(lane_payload), Some(lane_ns.clone())),
                ev(2, 2, Some(vec![0xaa, 0xbb, 0xcc, 0xdd, 0x01]), None),
                ev(3, 3, Some(json.clone()), None),
                ev(4, 4, Some(json), None), // same kind, second covenant
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
            vec![
                ("krc-20 · mint · KAS".to_string(), 2, 2),
                ("note".to_string(), 1, 1)
            ]
        );
        // Complement: the lane row lives in lane_namespaces, never in tags.
        assert_eq!(store.lane_namespaces().unwrap(), vec![(lane_ns, 1, 1)]);

        // Wipe the stamps (rows as an old binary would have written them):
        // both public fns must notice and fall back to the legacy scans.
        store
            .conn
            .execute(
                "UPDATE covenant_events SET payload_tag = NULL, inscription_kind = NULL",
                [],
            )
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
            .apply(
                &block_with_events(1, 100, vec![(0xA1, EventKind::Genesis, 0x01)]),
                BlockHash([1; 32]),
            )
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
            .apply(
                &block_with_events(1, 100, vec![(0xA1, EventKind::Genesis, 0x01)]),
                BlockHash([1; 32]),
            )
            .unwrap();
        store
            .apply(
                &block_with_events(
                    2,
                    200,
                    vec![
                        (0xA1, EventKind::Transition, 0x02),
                        (0xB2, EventKind::Genesis, 0x03),
                    ],
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
            .apply(
                &block_with_events(1, 1_000, vec![(0xA1, EventKind::Genesis, 0x01)]),
                BlockHash([1; 32]),
            )
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
            outpoint: Outpoint {
                txid: TxId([0x03; 32]),
                index: 0,
            },
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
        assert_eq!(
            d.biggest_birth,
            Some((CovenantId([0xB2; 32]), 5_000_000_000))
        );
    }

    #[test]
    fn activity_buckets_and_bounds() {
        // empty store: no bounds, no buckets
        let empty = test_store("activity-empty");
        assert_eq!(empty.event_daa_bounds().unwrap(), None);
        assert!(empty.activity(14_400, 0).unwrap().is_empty());

        let mut store = test_store("activity");
        store
            .apply(
                &block_with_events(1, 1_000, vec![(0xA1, EventKind::Genesis, 0x01)]),
                BlockHash([1; 32]),
            )
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
        assert_eq!(
            (rows[0].daa, rows[0].births, rows[0].moves, rows[0].burns),
            (0, 1, 0, 0)
        );
        assert_eq!(
            (rows[1].daa, rows[1].births, rows[1].moves, rows[1].burns),
            (69 * 14_400, 1, 2, 1)
        );

        // a cutoff at the newest bucket edge drops the old genesis
        let tail = store.activity(14_400, 993_600).unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!(
            (tail[0].daa, tail[0].births, tail[0].moves, tail[0].burns),
            (993_600, 1, 2, 1)
        );
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
            outpoint: Outpoint {
                txid: TxId([tx; 32]),
                index: 0,
            },
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
            (
                Outpoint {
                    txid: TxId([0x01; 32]),
                    index: 0,
                },
                TxId([0x04; 32]),
                vec![],
                0,
                0,
            ),
            (
                Outpoint {
                    txid: TxId([0x05; 32]),
                    index: 0,
                },
                TxId([0x06; 32]),
                vec![],
                0,
                1,
            ),
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
            outpoint: Outpoint {
                txid: TxId([tx; 32]),
                index: 0,
            },
            covenant_id: CovenantId([cov; 32]),
            value: 1_000,
            spk_version: 1,
            spk_script: script,
        };

        let mut b1 = BlockEvents::empty(BlockHash([1; 32]));
        b1.accepting_daa = 100;
        b1.created_utxos = vec![
            utxo(0x01, 0xA1, p2pk),
            utxo(0x02, 0xB2, junk),
            utxo(0x03, 0xC3, p2sh),
        ];
        store.apply(&b1, BlockHash([1; 32])).unwrap();

        let by_name = |stats: &[TemplateStat], name: Option<&str>| {
            stats
                .iter()
                .find(|s| s.template.as_deref() == name)
                .cloned()
                .unwrap()
        };
        let stats = store.template_stats().unwrap();
        assert_eq!(stats.len(), 3); // p2pk state, p2sh commitment, unrecognized
        let p2pk_row = by_name(&stats, Some("p2pk state"));
        assert_eq!(
            (p2pk_row.live_states, p2pk_row.ever_seen, p2pk_row.covenants),
            (1, 1, 1)
        );
        assert_eq!(p2pk_row.live_value, 1_000);
        let unrec = by_name(&stats, None); // '' sentinel: decoded, nothing matched
        assert_eq!(
            (unrec.live_states, unrec.ever_seen, unrec.covenants),
            (1, 1, 1)
        );
        assert!(store.revealed_template_counts().unwrap().is_empty());

        // spend the p2sh state, revealing its (template-less) program
        let mut sig = vec![0x03];
        sig.extend_from_slice(&redeem);
        let mut b2 = BlockEvents::empty(BlockHash([2; 32]));
        b2.accepting_daa = 200;
        b2.spent_utxos = vec![(
            Outpoint {
                txid: TxId([0x03; 32]),
                index: 0,
            },
            TxId([0x04; 32]),
            sig,
            0,
            0,
        )];
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
        b3.spent_utxos = vec![(
            Outpoint {
                txid: TxId([0x05; 32]),
                index: 0,
            },
            TxId([0x06; 32]),
            sig2,
            0,
            0,
        )];
        store.apply(&b3, BlockHash([3; 32])).unwrap();
        assert_eq!(
            store.revealed_template_counts().unwrap(),
            vec![("p2pk state".to_string(), 1)]
        );
    }

    /// The templates panel aggregates by the covenant's RESOLVED name (the
    /// grid-row precedence): a p2sh coin whose program revealed a semantic
    /// template at spend counts under the revealed name — every cell of the
    /// coin, live ones included — while a genuinely-unrevealed p2sh coin
    /// stays in the "p2sh commitment" bucket.
    #[test]
    fn template_stats_aggregate_by_resolved_covenant_name() {
        let mut store = test_store("templates-resolved");
        let p2sh = |seed: u8| {
            let mut s = vec![0xaa, 0x20];
            s.extend([seed; 32]);
            s.push(0x87);
            s
        };
        let utxo = |tx: u8, cov: u8, script: Vec<u8>| NewUtxo {
            outpoint: Outpoint {
                txid: TxId([tx; 32]),
                index: 0,
            },
            covenant_id: CovenantId([cov; 32]),
            value: 1_000,
            spk_version: 1,
            spk_script: script,
        };
        // 0xE5: two p2sh cells (0x01 spent below, 0x02 stays live);
        // 0xF6: one live p2sh cell, never revealed.
        let mut b1 = BlockEvents::empty(BlockHash([1; 32]));
        b1.accepting_daa = 100;
        b1.created_utxos = vec![
            utxo(0x01, 0xE5, p2sh(0x11)),
            utxo(0x02, 0xE5, p2sh(0x22)),
            utxo(0x03, 0xF6, p2sh(0x33)),
        ];
        store.apply(&b1, BlockHash([1; 32])).unwrap();
        let mut b2 = BlockEvents::empty(BlockHash([2; 32]));
        b2.accepting_daa = 200;
        b2.spent_utxos = vec![(
            Outpoint {
                txid: TxId([0x01; 32]),
                index: 0,
            },
            TxId([0x04; 32]),
            vec![],
            0,
            0,
        )];
        store.apply(&b2, BlockHash([2; 32])).unwrap();

        // every cell classifies as a commitment until a reveal names the coin
        let by_name = |stats: &[TemplateStat], name: Option<&str>| {
            stats
                .iter()
                .find(|s| s.template.as_deref() == name)
                .cloned()
                .unwrap()
        };
        let stats = store.template_stats().unwrap();
        let p2sh_row = by_name(&stats, Some("p2sh commitment"));
        assert_eq!(
            (p2sh_row.live_states, p2sh_row.ever_seen, p2sh_row.covenants),
            (2, 3, 2)
        );

        // stamp 0xE5's spent cell with a semantic reveal (the pick rule is
        // under test here, not reveal decoding — which recognize_and_bucket
        // already covers)
        store
            .conn
            .execute(
                "UPDATE covenant_utxos SET revealed_template = 'genesis0 · list' WHERE txid = ?1",
                [[0x01u8; 32].as_slice()],
            )
            .unwrap();

        let stats = store.template_stats().unwrap();
        // the revealed name owns ALL of 0xE5's cells — the live unrevealed
        // one included (that's the coin's effective name now)
        let named = by_name(&stats, Some("genesis0 · list"));
        assert_eq!(
            (named.live_states, named.ever_seen, named.covenants),
            (1, 2, 1)
        );
        assert_eq!(named.live_value, 1_000);
        // "p2sh commitment" shrinks to the genuinely-unrevealed coin
        let p2sh_row = by_name(&stats, Some("p2sh commitment"));
        assert_eq!(
            (p2sh_row.live_states, p2sh_row.ever_seen, p2sh_row.covenants),
            (1, 1, 1)
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
        let mut b1 = block_with_events(
            1,
            100,
            vec![
                (0xA1, EventKind::Genesis, 0x01),
                (0xB2, EventKind::Genesis, 0x02),
            ],
        );
        b1.created_utxos = vec![
            NewUtxo {
                outpoint: Outpoint {
                    txid: TxId([0x01; 32]),
                    index: 0,
                },
                covenant_id: CovenantId([0xA1; 32]),
                value: 5,
                spk_version: 0,
                spk_script: vec![],
            },
            NewUtxo {
                outpoint: Outpoint {
                    txid: TxId([0x02; 32]),
                    index: 0,
                },
                covenant_id: CovenantId([0xB2; 32]),
                value: 7,
                spk_version: 0,
                spk_script: vec![],
            },
        ];
        store.apply(&b1, BlockHash([1; 32])).unwrap();
        let mut b2 = block_with_events(2, 200, vec![(0xB2, EventKind::Burn, 0x03)]);
        b2.spent_utxos = vec![(
            Outpoint {
                txid: TxId([0x02; 32]),
                index: 0,
            },
            TxId([0x03; 32]),
            vec![],
            0,
            0,
        )];
        store.apply(&b2, BlockHash([2; 32])).unwrap();

        let flags = store.active_flags().unwrap();
        for c in store.list(u64::MAX).unwrap() {
            assert_eq!(
                flags.get(&c.covenant_id).copied().unwrap_or(false),
                c.live_utxos > 0,
                "flag mismatch for {:?}",
                c.covenant_id
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
            outpoint: Outpoint {
                txid: TxId([tx; 32]),
                index: 0,
            },
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
        b1.created_utxos = vec![
            utxo(0x01, 0xA1, 5),
            utxo(0x08, 0xA1, 7),
            utxo(0x02, 0xB2, 9),
        ];
        store.apply(&b1, BlockHash([1; 32])).unwrap();
        // later block: A1 gains a post-genesis state (NOT born value), B2 is swept
        let mut b2 = block_with_events(
            2,
            200,
            vec![
                (0xA1, EventKind::Transition, 0x03),
                (0xB2, EventKind::Burn, 0x04),
            ],
        );
        b2.created_utxos = vec![utxo(0x03, 0xA1, 11)];
        b2.spent_utxos = vec![(
            Outpoint {
                txid: TxId([0x02; 32]),
                index: 0,
            },
            TxId([0x04; 32]),
            vec![],
            0,
            0,
        )];
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
                "born_value mismatch for {:?}",
                c.covenant_id
            );
            assert_eq!(
                c.born_value,
                store.born_value(&c.covenant_id).unwrap(),
                "point born_value mismatch for {:?}",
                c.covenant_id
            );
            assert_eq!(
                c.template.as_ref(),
                templates.get(&c.covenant_id),
                "template mismatch for {:?}",
                c.covenant_id
            );
            let s = store.summary(&c.covenant_id).unwrap().unwrap();
            assert_eq!((s.born_value, &s.template), (c.born_value, &c.template));
        }
        // pinned expectations, so the folded columns and the maps can't both
        // drift in the same direction unnoticed
        let a1 = store.summary(&CovenantId([0xA1; 32])).unwrap().unwrap();
        assert_eq!(
            (a1.born_value, a1.template.as_deref()),
            (12, Some("mecenas"))
        );
        let b2 = store.summary(&CovenantId([0xB2; 32])).unwrap().unwrap();
        assert_eq!(
            (b2.born_value, b2.template.as_deref()),
            (9, Some("p2sh commitment"))
        );
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
            tx_index: tx as u32,
            payload: Some(lane_payload.clone()),
            lane_namespace: lane.map(str::to_string),
        };
        // daa 100: two lane events (two covenants) + one foreign-lane event.
        let mut b1 = BlockEvents::empty(BlockHash([1; 32]));
        b1.accepting_daa = 100;
        b1.events = vec![
            ev(1, 1, Some(&ns)),
            ev(2, 2, Some(&ns)),
            ev(3, 3, Some("cafebabe")),
        ];
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
        assert_eq!(
            store.lane_activity(&ns, 100).unwrap(),
            vec![(100, 3), (200, 1)]
        );
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
        let outpoint = Outpoint {
            txid: TxId([0x10; 32]),
            index: 0,
        };
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
        b2.spent_utxos = vec![(outpoint, spender, vec![0x01, 0x51], 60, 0)];
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

    /// The tx-scoped queries behind /data/{network}/tx/{txid}: a genesis tx,
    /// a multi-covenant tx (events + created + spent cells), and a token mint
    /// tx (token_events joined through covenant_events).
    #[test]
    fn tx_scoped_queries_cover_genesis_multi_covenant_and_token_mint() {
        let mut store = test_store("tx-scoped");
        let cov_a = CovenantId([0xA1; 32]);
        let cov_b = CovenantId([0xB2; 32]);
        let tx1 = TxId([0x01; 32]); // genesis of A
        let tx2 = TxId([0x02; 32]); // multi-covenant: transitions A, births B
        let tx3 = TxId([0x03; 32]); // mint event on B (token delta wired below)

        let mut b1 = BlockEvents::empty(BlockHash([1; 32]));
        b1.accepting_daa = 100;
        b1.events = vec![NewEvent {
            covenant_id: cov_a,
            kind: EventKind::Genesis,
            txid: tx1,
            tx_index: 0,
            payload: None,
            lane_namespace: None,
        }];
        b1.created_utxos = vec![NewUtxo {
            outpoint: Outpoint {
                txid: tx1,
                index: 0,
            },
            covenant_id: cov_a,
            value: 5_000,
            spk_version: 1,
            spk_script: vec![0x51],
        }];
        store.apply(&b1, BlockHash([1; 32])).unwrap();

        let mut b2 = BlockEvents::empty(BlockHash([2; 32]));
        b2.accepting_daa = 200;
        b2.events = vec![
            NewEvent {
                covenant_id: cov_a,
                kind: EventKind::Transition,
                txid: tx2,
                tx_index: 3,
                payload: None,
                lane_namespace: None,
            },
            NewEvent {
                covenant_id: cov_b,
                kind: EventKind::Genesis,
                txid: tx2,
                tx_index: 3,
                payload: None,
                lane_namespace: None,
            },
        ];
        b2.created_utxos = vec![
            NewUtxo {
                outpoint: Outpoint {
                    txid: tx2,
                    index: 0,
                },
                covenant_id: cov_a,
                value: 4_000,
                spk_version: 1,
                spk_script: vec![0x51],
            },
            NewUtxo {
                outpoint: Outpoint {
                    txid: tx2,
                    index: 1,
                },
                covenant_id: cov_b,
                value: 1_000,
                spk_version: 1,
                spk_script: vec![0x52],
            },
        ];
        b2.spent_utxos = vec![(
            Outpoint {
                txid: tx1,
                index: 0,
            },
            tx2,
            vec![0x01, 0x51],
            60,
            0,
        )];
        store.apply(&b2, BlockHash([2; 32])).unwrap();

        let mut b3 = BlockEvents::empty(BlockHash([3; 32]));
        b3.accepting_daa = 300;
        b3.events = vec![NewEvent {
            covenant_id: cov_b,
            kind: EventKind::Transition,
            txid: tx3,
            tx_index: 0,
            payload: None,
            lane_namespace: None,
        }];
        store.apply(&b3, BlockHash([3; 32])).unwrap();
        // The synthetic covenants carry no KCC20 templates, so the derivation
        // writes nothing — wire the delta by hand to exercise the join.
        store
            .conn
            .execute(
                "INSERT INTO token_events (token_id, covenant_id, seq, delta_idx, kind, amount,
                                           owner_from, owner_to, accepting_daa, tx_index)
                 VALUES (?1, ?2, 1, 0, 'mint', 42, NULL, ?3, 300, 0)",
                params![cov_b.0.as_slice(), cov_b.0.as_slice(), "00".repeat(33)],
            )
            .unwrap();

        // genesis tx: one event, one created cell, nothing spent, no deltas
        let events = store.events_by_txid(&tx1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].covenant_id, cov_a);
        assert_eq!(events[0].kind, "genesis");
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[0].accepting_block, BlockHash([1; 32]));
        assert_eq!(events[0].accepting_daa, 100);
        assert_eq!(events[0].tx_index, Some(0));
        let created = store.cells_created_by_txid(&tx1).unwrap();
        assert_eq!(created.len(), 1);
        assert_eq!(
            (created[0].covenant_id, created[0].index, created[0].value),
            (cov_a, 0, 5_000)
        );
        assert_eq!(created[0].template, None); // NULL and '' both read as None
        assert!(store.cells_spent_by_txid(&tx1).unwrap().is_empty());
        assert!(store.token_actions_by_txid(&tx1).unwrap().is_empty());

        // multi-covenant tx: both events, both created cells, the spent cell
        let events = store.events_by_txid(&tx2).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events
                .iter()
                .map(|e| (e.covenant_id, e.kind.as_str()))
                .collect::<Vec<_>>(),
            vec![(cov_a, "transition"), (cov_b, "genesis")],
        );
        let created = store.cells_created_by_txid(&tx2).unwrap();
        assert_eq!(
            created
                .iter()
                .map(|c| (c.covenant_id, c.index, c.value))
                .collect::<Vec<_>>(),
            vec![(cov_a, 0, 4_000), (cov_b, 1, 1_000)],
        );
        let spent = store.cells_spent_by_txid(&tx2).unwrap();
        assert_eq!(spent.len(), 1);
        assert_eq!(
            (spent[0].covenant_id, spent[0].txid, spent[0].index),
            (cov_a, tx1, 0)
        );
        assert_eq!(spent[0].value, 5_000);
        assert_eq!(spent[0].revealed_template, None);
        assert!(spent[0].has_witness);

        // token mint tx: the delta reaches back through (covenant_id, seq)
        let actions = store.token_actions_by_txid(&tx3).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].token_id, cov_b);
        assert_eq!(actions[0].kind, "mint");
        assert_eq!(actions[0].amount, Some(42));

        // a txid kascov never saw is empty everywhere, not an error
        let unknown = TxId([0xEE; 32]);
        assert!(store.events_by_txid(&unknown).unwrap().is_empty());
        assert!(store.cells_created_by_txid(&unknown).unwrap().is_empty());
        assert!(store.cells_spent_by_txid(&unknown).unwrap().is_empty());
        assert!(store.token_actions_by_txid(&unknown).unwrap().is_empty());
    }

    /// The tx_index/accepting_time_ms/accepting_blue_score ALTERs must apply
    /// once and no-op forever after (duplicate-column swallow), and captured
    /// values must survive a reopen; wiped rows read back as NULL.
    #[test]
    fn tx_index_migration_idempotent_and_roundtrip() {
        let path = test_store_path("tx-index-migrate");
        let mut store = Store::open(&path, Network::Testnet(10)).unwrap();
        store
            .apply(
                &block_with_events(
                    1,
                    100,
                    vec![
                        (0xA1, EventKind::Genesis, 0x01),
                        (0xB2, EventKind::Genesis, 0x02),
                    ],
                ),
                BlockHash([1; 32]),
            )
            .unwrap();
        drop(store);

        // Second and third opens rerun the migration list — must be no-ops.
        let store = Store::open(&path, Network::Testnet(10)).unwrap();
        drop(store);
        let store = Store::open(&path, Network::Testnet(10)).unwrap();
        assert_eq!(
            store.events(&CovenantId([0xA1; 32])).unwrap()[0].tx_index,
            Some(0)
        );
        assert_eq!(
            store.events(&CovenantId([0xB2; 32])).unwrap()[0].tx_index,
            Some(1)
        );
        // The bundled header fields landed too (block_with_events: daa*1000, daa).
        let (time_ms, blue): (u64, u64) = store
            .conn
            .query_row(
                "SELECT accepting_time_ms, accepting_blue_score FROM covenant_events LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((time_ms, blue), (100_000, 100));

        // Pre-capture rows read back as None and serialize without the field.
        store.wipe_tx_indices_for_test().unwrap();
        let event = &store.events(&CovenantId([0xA1; 32])).unwrap()[0];
        assert_eq!(event.tx_index, None);
        let json = serde_json::to_value(event).unwrap();
        assert!(
            json.get("tx_index").is_none(),
            "None tx_index must be omitted"
        );
    }

    /// The backfill's write helper: only NULL rows are stamped, unknown txids
    /// are no-ops, and already-stamped rows are never overwritten.
    #[test]
    fn stamp_tx_indices_fills_only_null_rows() {
        let mut store = test_store("tx-index-stamp");
        store
            .apply(
                &block_with_events(
                    1,
                    100,
                    vec![
                        (0xA1, EventKind::Genesis, 0x01),
                        (0xB2, EventKind::Genesis, 0x02),
                    ],
                ),
                BlockHash([1; 32]),
            )
            .unwrap();
        store
            .apply(
                &block_with_events(2, 200, vec![(0xA1, EventKind::Transition, 0x03)]),
                BlockHash([2; 32]),
            )
            .unwrap();
        store.wipe_tx_indices_for_test().unwrap();

        // Stamp block 1 only, with the coinbase offset a real accepted list
        // has (index 0 = coinbase, never a covenant event) and an accepted
        // txid we never indexed (a plain payment — must be a no-op).
        let stamped = store
            .stamp_tx_indices(&[(
                BlockHash([1; 32]),
                vec![
                    (TxId([0xEE; 32]), 0),
                    (TxId([0x01; 32]), 1),
                    (TxId([0x02; 32]), 2),
                ],
            )])
            .unwrap();
        assert_eq!(stamped, 2);
        assert_eq!(
            store.events(&CovenantId([0xA1; 32])).unwrap()[0].tx_index,
            Some(1)
        );
        assert_eq!(
            store.events(&CovenantId([0xB2; 32])).unwrap()[0].tx_index,
            Some(2)
        );
        // Block 2 was not in the batch: still NULL.
        assert_eq!(
            store.events(&CovenantId([0xA1; 32])).unwrap()[1].tx_index,
            None
        );

        // Re-stamping with different indices must not touch stamped rows.
        let restamped = store
            .stamp_tx_indices(&[(BlockHash([1; 32]), vec![(TxId([0x01; 32]), 9)])])
            .unwrap();
        assert_eq!(restamped, 0);
        assert_eq!(
            store.events(&CovenantId([0xA1; 32])).unwrap()[0].tx_index,
            Some(1)
        );
    }

    /// The consumer ordering contract: (accepting_daa, tx_index) sorts an
    /// interleaving of blocks and intra-block positions into acceptance order,
    /// regardless of insertion (rowid) order.
    #[test]
    fn ordering_key_daa_then_tx_index_sorts_interleaving() {
        let mut store = test_store("tx-index-order");
        let ev = |cov: u8, tx: u8, tx_index: u32| NewEvent {
            covenant_id: CovenantId([cov; 32]),
            kind: EventKind::Genesis,
            txid: TxId([tx; 32]),
            tx_index,
            payload: None,
            lane_namespace: None,
        };
        // Newer block applied first, and intra-block events inserted with
        // indices out of rowid order — the key alone must recover the order.
        let mut newer = BlockEvents::empty(BlockHash([2; 32]));
        newer.accepting_daa = 200;
        newer.events = vec![ev(0xC3, 0x30, 7)];
        store.apply(&newer, BlockHash([2; 32])).unwrap();
        let mut older = BlockEvents::empty(BlockHash([1; 32]));
        older.accepting_daa = 100;
        older.events = vec![ev(0xA1, 0x10, 5), ev(0xB2, 0x20, 2)];
        store.apply(&older, BlockHash([1; 32])).unwrap();

        let mut rows = store.recent_events(10).unwrap();
        rows.sort_by_key(|r| (r.accepting_daa, r.tx_index));
        let order: Vec<TxId> = rows.iter().map(|r| r.txid).collect();
        assert_eq!(
            order,
            vec![TxId([0x20; 32]), TxId([0x10; 32]), TxId([0x30; 32])]
        );
    }

    /// The canonical event shape: exactly these keys, with tx_index and
    /// payload_len omitted (never null) when absent.
    #[test]
    fn feed_event_row_canonical_shape() {
        let row = FeedEventRow {
            covenant_id: CovenantId([1; 32]),
            seq: 3,
            kind: "transition".into(),
            txid: TxId([2; 32]),
            accepting_daa: 100,
            accepting_block: BlockHash([3; 32]),
            tx_index: Some(4),
            payload_len: Some(20),
        };
        let v = serde_json::to_value(&row).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        let mut expect = vec![
            "covenant_id",
            "seq",
            "kind",
            "txid",
            "accepting_daa",
            "accepting_block",
            "tx_index",
            "payload_len",
        ];
        let mut got = keys.clone();
        got.sort_unstable();
        expect.sort_unstable();
        assert_eq!(got, expect);
        assert_eq!(v["tx_index"], serde_json::json!(4));
        assert_eq!(v["payload_len"], serde_json::json!(20));

        let bare = FeedEventRow {
            tx_index: None,
            payload_len: None,
            ..row
        };
        let v = serde_json::to_value(&bare).unwrap();
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("tx_index"), "absent, not null");
        assert!(!obj.contains_key("payload_len"), "absent, not null");
        assert_eq!(obj.len(), 6);
    }

    /// events_after pages a synthetic interleaving — ties on DAA across
    /// blocks, tx_index ties resolved by txid, a multi-covenant tx (two
    /// events sharing txid AND tx_index) resolved by covenant_id, legacy
    /// NULL tx_index rows last in their group — identically to one big
    /// query, from any page size.
    #[test]
    fn events_feed_cursor_walks_interleavings() {
        let mut store = test_store("events-feed");
        let ev = |cov: u8, tx: u8, tx_index: u32| NewEvent {
            covenant_id: CovenantId([cov; 32]),
            kind: EventKind::Genesis,
            txid: TxId([tx; 32]),
            tx_index,
            payload: (tx == 0x20).then(|| vec![0u8; tx as usize]),
            lane_namespace: None,
        };
        // Two blocks share DAA 100 (tx_index collides across them → txid
        // breaks the tie); insertion order is deliberately not feed order.
        let mut b1 = BlockEvents::empty(BlockHash([1; 32]));
        b1.accepting_daa = 100;
        b1.events = vec![ev(0xA, 0x50, 0), ev(0xB, 0x10, 1), ev(0xA, 0x60, 2)];
        let mut b2 = BlockEvents::empty(BlockHash([2; 32]));
        b2.accepting_daa = 100;
        b2.events = vec![ev(0xC, 0x20, 0), ev(0xD, 0x70, 1)];
        let mut b3 = BlockEvents::empty(BlockHash([3; 32]));
        b3.accepting_daa = 105;
        // tx 0x40 moves two covenants at once: same txid, same tx_index.
        b3.events = vec![
            ev(0xE, 0x30, 0),
            ev(0xA, 0x40, 1),
            ev(0x9, 0x40, 1),
            ev(0xF, 0x80, 2),
        ];
        let mut b4 = BlockEvents::empty(BlockHash([4; 32]));
        b4.accepting_daa = 110;
        b4.events = vec![ev(0xB, 0x90, 0)];
        for b in [&b3, &b1, &b4, &b2] {
            store.apply(b, b.accepting_block).unwrap();
        }
        // Two legacy rows (pre-capture): NULL tx_index sorts last in-group.
        store
            .conn
            .execute(
                "UPDATE covenant_events SET tx_index = NULL WHERE txid IN (?1, ?2)",
                params![[0x10u8; 32].as_slice(), [0x30u8; 32].as_slice()],
            )
            .unwrap();

        let all = store.events_after(0, 0, 1000).unwrap();
        assert_eq!(all.len(), 10);
        // The canonical order as (txid, covenant) bytes, by hand:
        // DAA 100 → indices 0 (0x20 < 0x50 by txid), 1, 2, then NULL (0x10);
        // DAA 105 → the 0x40 pair (covenant 0x9 before 0xA), 0x80, NULL 0x30;
        // DAA 110 → 0x90.
        let expect: Vec<(u8, u8)> = vec![
            (0x20, 0xC),
            (0x50, 0xA),
            (0x70, 0xD),
            (0x60, 0xA),
            (0x10, 0xB),
            (0x40, 0x9),
            (0x40, 0xA),
            (0x80, 0xF),
            (0x30, 0xE),
            (0x90, 0xB),
        ];
        let key = |e: &FeedEventRow| (e.txid.0[0], e.covenant_id.0[0]);
        let got: Vec<(u8, u8)> = all.iter().map(key).collect();
        assert_eq!(got, expect);
        // payload_len rides along only where a payload exists.
        assert_eq!(all[0].payload_len, Some(0x20));
        assert_eq!(all[1].payload_len, None);

        // Walk at every page size, computing the (after_daa, after_seq)
        // cursor the way the /events handler does — each walk must re-yield
        // the full list exactly.
        for page in 1..=10u64 {
            let mut walked: Vec<(u8, u8)> = Vec::new();
            let (mut daa, mut seq) = (0u64, 0u64);
            loop {
                let rows = store.events_after(daa, seq, page).unwrap();
                if rows.is_empty() {
                    break;
                }
                let last_daa = rows.last().unwrap().accepting_daa;
                let in_group = rows.iter().filter(|e| e.accepting_daa == last_daa).count() as u64;
                seq = if last_daa == daa {
                    seq + in_group
                } else {
                    in_group
                };
                daa = last_daa;
                walked.extend(rows.iter().map(key));
                if rows.len() < page as usize {
                    break;
                }
            }
            assert_eq!(walked, expect, "page size {page}");
        }
        // A cursor mid-group resumes exactly after what it consumed: group
        // 100 is [0x20, 0x50, 0x70, 0x60, 0x10], so seq 2 resumes at 0x70.
        assert_eq!(
            store
                .events_after(100, 2, 3)
                .unwrap()
                .iter()
                .map(key)
                .collect::<Vec<_>>(),
            vec![(0x70, 0xD), (0x60, 0xA), (0x10, 0xB)]
        );
        // A cursor past the tip yields nothing.
        assert!(store.events_after(110, 1, 5).unwrap().is_empty());
    }

    /// Subscription secrets: stored, returned to the delivery loop, and
    /// enforced on unsubscribe — while legacy NULL-secret rows keep deleting
    /// by id alone.
    #[test]
    fn subscription_secret_roundtrip() {
        let store = test_store("sub-secret");
        let id = store
            .add_subscription(
                None,
                Some("genesis"),
                "https://example.com/hook",
                Some("aa11"),
                1,
            )
            .unwrap();
        let legacy = store
            .add_subscription(None, None, "https://example.com/legacy", None, 2)
            .unwrap();

        let subs = store
            .subscriptions_matching([0u8; 32].as_slice(), "genesis")
            .unwrap();
        assert!(subs.contains(&(id, "https://example.com/hook".into(), Some("aa11".into()))));
        assert!(subs.contains(&(legacy, "https://example.com/legacy".into(), None)));

        assert_eq!(
            store.delete_subscription_secured(id, None).unwrap(),
            UnsubscribeOutcome::WrongSecret
        );
        assert_eq!(
            store.delete_subscription_secured(id, Some("bb22")).unwrap(),
            UnsubscribeOutcome::WrongSecret
        );
        assert_eq!(
            store.subscription_count().unwrap(),
            2,
            "wrong secret must not delete"
        );
        assert_eq!(
            store.delete_subscription_secured(id, Some("aa11")).unwrap(),
            UnsubscribeOutcome::Deleted
        );
        assert_eq!(
            store.delete_subscription_secured(id, Some("aa11")).unwrap(),
            UnsubscribeOutcome::NotFound
        );
        // Legacy row: no secret stored, id alone (with or without a guess).
        assert_eq!(
            store
                .delete_subscription_secured(legacy, Some("anything"))
                .unwrap(),
            UnsubscribeOutcome::Deleted
        );
        assert_eq!(store.subscription_count().unwrap(), 0);
    }
}
