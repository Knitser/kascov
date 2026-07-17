//! KCC20 token accounting: a deterministic derivation of per-token supply,
//! balances, event classification, and rule-validation verdicts from
//! `covenant_events` + `covenant_utxos` alone.
//!
//! Design contract (the conservative core of the feature):
//!
//! * One pure function, [`derive_token`], recomputes a token's entire derived
//!   state from the two source tables. The write hook in `Store::apply`, the
//!   reorg rewind in `Store::rollback`, and the versioned boot pass
//!   [`Store::derive_tokens_if_stale`] all call it — one truth, no
//!   incremental delta-patcher that could drift (spend-time reveals
//!   retro-resolve cells created by earlier events, so incremental code would
//!   rewrite history rows anyway).
//! * A token is `verified` ONLY when every event in its history classified
//!   against a known KCC20 rule with every input/output state hash-proven,
//!   no conservation/minter violation, and the live frontier sums exactly to
//!   genesis + mints − burns. Anything unknown or ambiguous is `unvalidated`
//!   with the first reason stamped; `invalid` is reserved for hash-proven
//!   rule violations. Never a false "verified".
//! * States are proven three ways, all proof-grade: a bare consensus state
//!   script that decodes as a KCC20 build; a spend-time P2SH reveal
//!   (blake2b-verified against the committed hash); or witness recovery —
//!   the spending tx's sigscript carries the new states as struct-of-arrays
//!   pushes, and splicing candidate fields into a same-build program is
//!   accepted iff the splice hashes to the output's committed hash
//!   ([`kascov_decode::kcc20::prove_output_state`]). Hash equality is the
//!   sole acceptance criterion, so a misparse fails closed.
//! * Event order: token events anchor exclusively to the token covenant's
//!   own `covenant_events` rows, whose `seq` is a total order that agrees
//!   with the canonical (accepting_daa, tx_index) feed order for a single
//!   covenant — so pre-capture NULL `tx_index` rows never make ordering
//!   ambiguous here, and the per-tx conservation checks are order-free
//!   anyway. Minter (vault) covenants link through `token_minters` instead
//!   of injecting a second event stream.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::model::{CovenantId, TxId};
use crate::store::{db_err, registry};
use crate::Result;
use kascov_decode::kcc20;

/// Version of the token derivation (rules + KCC20 skeletons). Bump on any
/// change to `derive_token`'s classification/validation logic or to the
/// KCC20 skeletons in kascov-decode (a KCC20-relevant `CLASSIFIER_VERSION`
/// bump implies a bump here too); the boot pass then rederives everything.
pub const TOKEN_DERIVATION_VERSION: &str = "1";

/// Meta key holding the last completed derivation version.
pub(crate) const TOKEN_DERIVATION_META: &str = "token_derivation_version";

pub const STATUS_VERIFIED: &str = "verified";
pub const STATUS_INVALID: &str = "invalid";
pub const STATUS_UNVALIDATED: &str = "unvalidated";

/// One row of the tokens directory — the `tokens` table joined with live
/// UTXO aggregates. `validation` is the verdict; liveness (`active|burned`)
/// stays the worker's `status` field, derived from `live_utxos`.
#[derive(Clone, Debug, Serialize)]
pub struct TokenDirRow {
    pub token_id: CovenantId,
    pub validation: String,
    pub invalid_reason: Option<String>,
    pub supply: Option<i64>,
    pub minted: Option<i64>,
    pub burned: Option<i64>,
    pub holders: u64,
    pub unresolved_cells: u64,
    pub last_activity_daa: u64,
    /// Latest hash-proven state fields as a JSON object (label → hex value),
    /// same shape the per-request registry decode used to produce.
    pub fields_json: Option<String>,
    pub derived_at_daa: Option<u64>,
    pub live_utxos: u64,
    pub live_value: u64,
    pub template: Option<String>,
}

/// One holder of a token: aggregated over live hash-proven cells.
#[derive(Clone, Debug, Serialize)]
pub struct TokenBalanceRow {
    /// hex(identifier_type || owner_identifier) — 66 hex chars.
    pub owner: String,
    pub balance: i64,
    pub cells: u64,
}

/// One classified token-event delta, joined to its covenant event for txid.
#[derive(Clone, Debug, Serialize)]
pub struct TokenEventRow {
    pub seq: u64,
    pub delta_idx: u64,
    /// genesis | mint | transfer | split | merge | burn | unknown
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_to: Option<String>,
    pub accepting_daa: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_index: Option<u64>,
    pub txid: TxId,
    /// The underlying covenant-event kind (genesis | transition | burn).
    pub event_kind: String,
}

/// A vault/controller covenant registered by "KCC20 minter" reveals, with
/// the token covenants its program pins.
#[derive(Clone, Debug, Serialize)]
pub struct TokenMinterRow {
    pub covenant_id: CovenantId,
    pub governs: Vec<CovenantId>,
    pub last_activity_daa: u64,
    pub live_utxos: u64,
    pub live_value: u64,
}

fn outpoint_str(txid: &[u8; 32], index: u32) -> String {
    format!("{}:{index}", hex::encode(txid))
}

/// API/display form of a stored owner key (`hex(identifier_type || owner)`,
/// 66 hex chars): a bare 64-hex pubkey for type 0x00 (routable as an address
/// page), a typed prefix for everything else — a covenant id or script hash
/// must never be mistaken for a pubkey.
pub fn owner_display(owner_hex: &str) -> String {
    if owner_hex.len() != 66 {
        return owner_hex.to_string();
    }
    let (id_type, rest) = owner_hex.split_at(2);
    match id_type {
        "00" => rest.to_string(),
        "01" => format!("script:{rest}"),
        "02" => format!("covenant:{rest}"),
        _ => owner_hex.to_string(),
    }
}

/// A token state cell: one `covenant_utxos` row of the token covenant.
struct Cell {
    txid: [u8; 32],
    index: u32,
    spk_version: u16,
    spk_script: Vec<u8>,
    spent_txid: Option<[u8; 32]>,
    spent_sig: Option<Vec<u8>>,
    /// Hash-proven state + the proven program bytes (splice base for
    /// recovering other cells of the same build).
    proven: Option<(kcc20::TokenState, Vec<u8>)>,
    /// Why the state is unproven, when a proof was attempted and failed.
    unproven: Option<String>,
}

impl Cell {
    fn live(&self) -> bool {
        self.spent_txid.is_none()
    }
}

fn load_cells(conn: &Connection, token_id: &[u8; 32]) -> Result<Vec<Cell>> {
    let mut stmt = conn
        .prepare(
            "SELECT txid, output_index, spk_version, spk_script, spent_txid, spent_sig
             FROM covenant_utxos WHERE covenant_id = ?1
             ORDER BY created_daa, txid, output_index",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map([token_id.as_slice()], |r| {
            Ok(Cell {
                txid: r.get(0)?,
                index: r.get(1)?,
                spk_version: r.get(2)?,
                spk_script: r.get(3)?,
                spent_txid: r.get(4)?,
                spent_sig: r.get(5)?,
                proven: None,
                unproven: None,
            })
        })
        .map_err(db_err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_err)?;
    Ok(rows)
}

/// Proof pass A: bare consensus states and spend-time reveals.
fn prove_direct(cells: &mut [Cell]) {
    let registry = registry();
    for cell in cells.iter_mut() {
        // A bare (non-P2SH) state script that decodes as a KCC20 build IS the
        // state — consensus data, no hash check needed.
        if let Some(st) = kcc20::decode_token_state(registry, cell.spk_version, &cell.spk_script) {
            let program = cell.spk_script.clone();
            cell.proven = Some((st, program));
            continue;
        }
        match (&cell.spent_txid, &cell.spent_sig) {
            (Some(_), Some(sig)) => {
                match kascov_decode::p2sh_reveal(&cell.spk_script, sig) {
                    Some(program) => {
                        match kcc20::decode_token_state(registry, cell.spk_version, &program) {
                            Some(st) => cell.proven = Some((st, program)),
                            None => {
                                cell.unproven = Some(format!(
                                    "reveal of {} is not a recognized KCC20 build",
                                    outpoint_str(&cell.txid, cell.index)
                                ))
                            }
                        }
                    }
                    None => {
                        cell.unproven = Some(format!(
                            "spend of {} does not reveal its committed program",
                            outpoint_str(&cell.txid, cell.index)
                        ))
                    }
                }
            }
            (Some(_), None) => {
                cell.unproven = Some(format!(
                    "reveal missing for spent output {}",
                    outpoint_str(&cell.txid, cell.index)
                ))
            }
            (None, _) => {} // live P2SH commitment: pass B may recover it
        }
    }
}

/// Every legal (identifier_type, is_minter) byte pair. Recovery brute-forces
/// these six instead of requiring them as witness pushes — the observed arg
/// shapes don't always carry them as clean per-output arrays (vault swaps
/// pack them differently), and hash-gating means wrong guesses only cost a
/// hash. Values outside this domain could never validate anyway.
const TYPE_MINTER_DOMAIN: [(u8, u8); 6] =
    [(0x00, 0x00), (0x00, 0x01), (0x01, 0x00), (0x01, 0x01), (0x02, 0x00), (0x02, 0x01)];

/// Proof pass B: witness recovery. For each tx that created still-unproven
/// cells, the sigscripts of the tx's inputs — the token's own inputs plus
/// any co-spent covenant's inputs (a vault leader carries args for the
/// token runs it drives) — carry the new output states as struct-of-arrays
/// pushes: owners n×32B, amounts n×8B, where n is the tx's token-output
/// count; identifier_type/is_minter come from the six-value legal domain.
/// Each candidate assignment is accepted per output iff the splice-and-hash
/// check passes — wrong guesses cost a hash, never a wrong accept. Runs to a
/// fixpoint so recovered inputs can serve as splice bases downstream.
fn prove_recovered(conn: &Connection, token_id: &[u8; 32], cells: &mut Vec<Cell>) -> Result<()> {
    // creating txid -> output cell indices (output_index ascending — the
    // load order), spending txid -> input cell indices.
    let mut outs_of: BTreeMap<[u8; 32], Vec<usize>> = BTreeMap::new();
    let mut ins_of: BTreeMap<[u8; 32], Vec<usize>> = BTreeMap::new();
    for (i, cell) in cells.iter().enumerate() {
        outs_of.entry(cell.txid).or_default().push(i);
        if let Some(spender) = cell.spent_txid {
            ins_of.entry(spender).or_default().push(i);
        }
    }
    let mut foreign_sigs = conn
        .prepare(
            "SELECT spent_sig FROM covenant_utxos
             WHERE spent_txid = ?1 AND covenant_id != ?2 AND spent_sig IS NOT NULL",
        )
        .map_err(db_err)?;
    loop {
        let mut changed = false;
        for (txid, outs) in &outs_of {
            if outs.iter().all(|&i| cells[i].proven.is_some()) {
                continue;
            }
            let Some(ins) = ins_of.get(txid) else { continue }; // genesis: nothing to recover from
            let n = outs.len();
            // Proven programs of this tx's inputs are same-build splice bases.
            let bases: Vec<Vec<u8>> = ins
                .iter()
                .filter_map(|&i| cells[i].proven.as_ref().map(|(_, p)| p.clone()))
                .filter(|p| kcc20::has_state_block(p))
                .collect();
            if bases.is_empty() {
                continue;
            }
            // Argument carriers: this token's input sigs, then the sigs of
            // co-spent inputs of other covenants in the same tx.
            let mut sigs: Vec<Vec<u8>> =
                ins.iter().filter_map(|&i| cells[i].spent_sig.clone()).collect();
            let foreign: Vec<Vec<u8>> = foreign_sigs
                .query_map(params![txid.as_slice(), token_id.as_slice()], |r| r.get(0))
                .map_err(db_err)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_err)?;
            sigs.extend(foreign);
            for sig in &sigs {
                let (instructions, _) = kascov_decode::disasm::disassemble(sig);
                let pushes: Vec<Vec<u8>> =
                    instructions.into_iter().filter_map(|inst| inst.data).collect();
                let owners: Vec<&Vec<u8>> = pushes.iter().filter(|p| p.len() == n * 32).collect();
                let amounts: Vec<&Vec<u8>> = pushes.iter().filter(|p| p.len() == n * 8).collect();
                for base in &bases {
                    for ow in &owners {
                        for am in &amounts {
                            for (k, &out_idx) in outs.iter().enumerate() {
                                if cells[out_idx].proven.is_some() {
                                    continue;
                                }
                                let owner: [u8; 32] =
                                    ow[k * 32..(k + 1) * 32].try_into().expect("32-byte slice");
                                let amount: [u8; 8] =
                                    am[k * 8..(k + 1) * 8].try_into().expect("8-byte slice");
                                for (id_type, minter) in TYPE_MINTER_DOMAIN {
                                    if let Some(st) = kcc20::prove_output_state(
                                        base,
                                        &cells[out_idx].spk_script,
                                        &owner,
                                        id_type,
                                        &amount,
                                        minter,
                                    ) {
                                        let program = kcc20::splice_token_state(
                                            base, &owner, id_type, &amount, minter,
                                        )
                                        .expect("base had a state block");
                                        cells[out_idx].proven = Some((st, program));
                                        cells[out_idx].unproven = None;
                                        changed = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    Ok(())
}

/// A fully judged cell state: proven identity AND in-model field values.
struct Judged {
    amount: i64,
    minter: bool,
    owner_key: String,
}

/// Judge one cell for rule checking. `Err(reason)` is a human-readable
/// account of why this side of a transaction cannot be validated.
fn judge(cell: &Cell) -> std::result::Result<Judged, String> {
    let Some((st, _)) = &cell.proven else {
        return Err(cell.unproven.clone().unwrap_or_else(|| {
            if cell.live() {
                format!("live state unproven for {}", outpoint_str(&cell.txid, cell.index))
            } else {
                format!("state unproven for spent output {}", outpoint_str(&cell.txid, cell.index))
            }
        }));
    };
    if !matches!(st.identifier_type, 0x00 | 0x01 | 0x02) {
        return Err(format!(
            "unknown identifier type 0x{:02x} on {}",
            st.identifier_type,
            outpoint_str(&cell.txid, cell.index)
        ));
    }
    let Some(amount) = st.amount_i64() else {
        return Err(format!(
            "amount out of script-int range on {}",
            outpoint_str(&cell.txid, cell.index)
        ));
    };
    let Some(minter) = st.is_minter() else {
        return Err(format!(
            "non-boolean isMinter on {}",
            outpoint_str(&cell.txid, cell.index)
        ));
    };
    Ok(Judged { amount, minter, owner_key: st.owner_key() })
}

/// The verdict lattice: `invalid` (hash-proven violation) beats
/// `unvalidated` (anything unknown/ambiguous) beats `verified`. The FIRST
/// reason of the winning class is stamped.
#[derive(Default)]
struct Verdict {
    invalid: Option<String>,
    unvalidated: Option<String>,
}

impl Verdict {
    fn flag_invalid(&mut self, reason: String) {
        self.invalid.get_or_insert(reason);
    }
    fn flag_unvalidated(&mut self, reason: String) {
        self.unvalidated.get_or_insert(reason);
    }
    fn status(&self) -> &'static str {
        if self.invalid.is_some() {
            STATUS_INVALID
        } else if self.unvalidated.is_some() {
            STATUS_UNVALIDATED
        } else {
            STATUS_VERIFIED
        }
    }
    fn reason(&self) -> Option<&str> {
        self.invalid.as_deref().or(self.unvalidated.as_deref())
    }
}

/// One delta row to be written to `token_events`.
struct Delta {
    amount: Option<i64>,
    owner_from: Option<String>,
    owner_to: Option<String>,
}

struct ClassifiedEvent {
    seq: u64,
    kind: &'static str,
    accepting_daa: u64,
    tx_index: Option<u64>,
    deltas: Vec<Delta>,
}

/// Does any covenant_utxos row evidence this covenant as a KCC20 token?
pub(crate) fn has_token_evidence(conn: &Connection, id: &[u8; 32]) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM covenant_utxos WHERE covenant_id = ?1
             AND (template = 'KCC20 token' OR revealed_template = 'KCC20 token'))",
        [id.as_slice()],
        |r| r.get(0),
    )
    .map_err(db_err)
}

/// Does any covenant_utxos row evidence this covenant as a KCC20 minter?
/// (The write-time stamp equivalent of apply()'s `kcc20_seen` minter bit —
/// used by gap recovery, which stamps templates the same way apply does.)
pub(crate) fn has_minter_evidence(conn: &Connection, id: &[u8; 32]) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM covenant_utxos WHERE covenant_id = ?1
             AND (template = 'KCC20 minter' OR revealed_template = 'KCC20 minter'))",
        [id.as_slice()],
        |r| r.get(0),
    )
    .map_err(db_err)
}

fn pinned_by_minter(conn: &Connection, id: &[u8; 32]) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM token_minters WHERE token_id = ?1)",
        [id.as_slice()],
        |r| r.get(0),
    )
    .map_err(db_err)
}

fn delete_token_rows(conn: &Connection, id: &[u8; 32]) -> Result<()> {
    for sql in [
        "DELETE FROM token_events WHERE token_id = ?1",
        "DELETE FROM token_balances WHERE token_id = ?1",
        "DELETE FROM tokens WHERE token_id = ?1",
    ] {
        conn.execute(sql, [id.as_slice()]).map_err(db_err)?;
    }
    Ok(())
}

fn processed_daa(conn: &Connection) -> Result<Option<u64>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = 'processed_daa'", [], |r| {
            r.get::<_, String>(0)
        })
        .optional()
        .map_err(db_err)?
        .and_then(|s| s.parse().ok()))
}

/// Recompute one token's derived rows from `covenant_events` +
/// `covenant_utxos` — deterministic, idempotent, transactional with the
/// caller. A token with no surviving KCC20 evidence and no minter pin has
/// its rows deleted entirely.
pub(crate) fn derive_token(conn: &Connection, token_id: &[u8; 32]) -> Result<()> {
    let evidence = has_token_evidence(conn, token_id)?;
    let pinned = pinned_by_minter(conn, token_id)?;
    if !evidence && !pinned {
        return delete_token_rows(conn, token_id);
    }
    // Idempotent rewrite: clear this token's derived rows, then re-insert.
    conn.execute("DELETE FROM token_events WHERE token_id = ?1", [token_id.as_slice()])
        .map_err(db_err)?;
    conn.execute("DELETE FROM token_balances WHERE token_id = ?1", [token_id.as_slice()])
        .map_err(db_err)?;
    let derived_at = processed_daa(conn)?;

    if !evidence {
        // Pinned by a minter program but no KCC20 token reveal ever decoded:
        // an honest placeholder, never a verdict.
        let last_activity: u64 = conn
            .query_row(
                "SELECT last_activity_daa FROM covenants WHERE covenant_id = ?1",
                [token_id.as_slice()],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?
            .unwrap_or(0);
        conn.execute(
            "INSERT OR REPLACE INTO tokens (token_id, status, invalid_reason, supply, minted,
                 burned, holders, unresolved_cells, last_activity_daa, fields_json, derived_at_daa)
             VALUES (?1, ?2, ?3, NULL, NULL, NULL, 0, 0, ?4, NULL, ?5)",
            params![
                token_id.as_slice(),
                STATUS_UNVALIDATED,
                "pinned by minter; no KCC20 token reveal decoded",
                last_activity,
                derived_at
            ],
        )
        .map_err(db_err)?;
        return Ok(());
    }

    let mut verdict = Verdict::default();

    // Covenant gate: only a KIP-20-proven, fully-watched lineage can verify.
    let cov: Option<(Option<[u8; 32]>, bool, u64)> = conn
        .query_row(
            "SELECT genesis_txid, lineage_complete, last_activity_daa
             FROM covenants WHERE covenant_id = ?1",
            [token_id.as_slice()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(db_err)?;
    let last_activity = match &cov {
        Some((genesis_txid, lineage_complete, last_activity)) => {
            if !lineage_complete || genesis_txid.is_none() {
                verdict.flag_unvalidated(
                    "incomplete lineage — covenant history predates indexing".into(),
                );
            }
            *last_activity
        }
        None => {
            verdict.flag_unvalidated("covenant row missing".into());
            0
        }
    };

    // Prove every cell state we can (bare / reveal / witness recovery).
    let mut cells = load_cells(conn, token_id)?;
    prove_direct(&mut cells);
    prove_recovered(conn, token_id, &mut cells)?;

    // Group cells into per-tx in/out sets.
    let mut outs_of: BTreeMap<[u8; 32], Vec<usize>> = BTreeMap::new();
    let mut ins_of: BTreeMap<[u8; 32], Vec<usize>> = BTreeMap::new();
    for (i, cell) in cells.iter().enumerate() {
        outs_of.entry(cell.txid).or_default().push(i);
        if let Some(spender) = cell.spent_txid {
            ins_of.entry(spender).or_default().push(i);
        }
    }

    // The token's own events, in seq order — a total order that agrees with
    // the canonical feed order for a single covenant.
    let mut stmt = conn
        .prepare(
            "SELECT seq, kind, txid, accepting_daa, tx_index FROM covenant_events
             WHERE covenant_id = ?1 ORDER BY seq",
        )
        .map_err(db_err)?;
    let events: Vec<(u64, String, [u8; 32], u64, Option<u64>)> = stmt
        .query_map([token_id.as_slice()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .map_err(db_err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_err)?;

    let mut classified: Vec<ClassifiedEvent> = Vec::with_capacity(events.len());
    let mut all_events_clean = true;
    let mut seen_txids: BTreeSet<[u8; 32]> = BTreeSet::new();
    // i128 accumulators: every judged amount is a non-negative i64, so sums
    // over any realistic history cannot overflow i128.
    let mut supply: i128 = 0;
    let mut minted: i128 = 0;
    let mut burned: i128 = 0;

    for (seq, ev_kind, txid, accepting_daa, tx_index) in &events {
        seen_txids.insert(*txid);
        let ins: &[usize] = ins_of.get(txid).map(Vec::as_slice).unwrap_or(&[]);
        let outs: &[usize] = outs_of.get(txid).map(Vec::as_slice).unwrap_or(&[]);
        let anchor = |detail: &str| format!("seq {seq} (daa {accepting_daa}): {detail}");
        let unknown = |classified: &mut Vec<ClassifiedEvent>,
                           verdict: &mut Verdict,
                           all_clean: &mut bool,
                           reason: String| {
            verdict.flag_unvalidated(reason);
            *all_clean = false;
            classified.push(ClassifiedEvent {
                seq: *seq,
                kind: "unknown",
                accepting_daa: *accepting_daa,
                tx_index: *tx_index,
                deltas: vec![Delta { amount: None, owner_from: None, owner_to: None }],
            });
        };

        if ins.is_empty() && outs.is_empty() {
            unknown(
                &mut classified,
                &mut verdict,
                &mut all_events_clean,
                anchor("no state cells recorded for this event's tx"),
            );
            continue;
        }
        // Judge every side; the first failure downgrades the whole event.
        let in_judged: std::result::Result<Vec<Judged>, String> =
            ins.iter().map(|&i| judge(&cells[i])).collect();
        let out_judged: std::result::Result<Vec<Judged>, String> =
            outs.iter().map(|&i| judge(&cells[i])).collect();
        let (in_states, out_states) = match (in_judged, out_judged) {
            (Ok(i), Ok(o)) => (i, o),
            (Err(reason), _) | (_, Err(reason)) => {
                unknown(&mut classified, &mut verdict, &mut all_events_clean, anchor(&reason));
                continue;
            }
        };

        if in_states.is_empty() {
            // Outputs without token inputs: legal only as the KIP-20-proven
            // genesis. Consensus forbids it anywhere else, so any other
            // sighting is out of model — unvalidated, never guessed at.
            if ev_kind != "genesis" {
                unknown(
                    &mut classified,
                    &mut verdict,
                    &mut all_events_clean,
                    anchor("token outputs created without token inputs outside genesis"),
                );
                continue;
            }
            let sum: i128 = out_states.iter().map(|s| s.amount as i128).sum();
            supply += sum;
            classified.push(ClassifiedEvent {
                seq: *seq,
                kind: "genesis",
                accepting_daa: *accepting_daa,
                tx_index: *tx_index,
                deltas: out_states
                    .iter()
                    .map(|s| Delta {
                        amount: Some(s.amount),
                        owner_from: None,
                        owner_to: Some(s.owner_key.clone()),
                    })
                    .collect(),
            });
            continue;
        }

        let isum: i128 = in_states.iter().map(|s| s.amount as i128).sum();
        let osum: i128 = out_states.iter().map(|s| s.amount as i128).sum();
        let minter_in = in_states.iter().any(|s| s.minter);
        let minter_out = out_states.iter().any(|s| s.minter);
        let single_in_owner = {
            let owners: BTreeSet<&str> =
                in_states.iter().map(|s| s.owner_key.as_str()).collect();
            (owners.len() == 1).then(|| in_states[0].owner_key.clone())
        };

        if out_states.is_empty() {
            // Terminal burn: the whole covenant input set leaves circulation.
            // The contract's conservation branch should make a non-minter
            // terminal burn of a positive amount impossible; none is observed
            // on chain, so an occurrence is out of model — unvalidated.
            if !minter_in && isum > 0 {
                verdict.flag_unvalidated(anchor(
                    "terminal burn without a minter input — shape unobserved on chain",
                ));
            }
            burned += isum;
            supply -= isum;
            classified.push(ClassifiedEvent {
                seq: *seq,
                kind: "burn",
                accepting_daa: *accepting_daa,
                tx_index: *tx_index,
                deltas: in_states
                    .iter()
                    .map(|s| Delta {
                        amount: Some(s.amount),
                        owner_from: Some(s.owner_key.clone()),
                        owner_to: None,
                    })
                    .collect(),
            });
            continue;
        }

        // Minter escalation: creating a minter state requires holding one
        // (checkMintingTransfer). Hash-proven on both sides → a violation.
        if minter_out && !minter_in {
            verdict.flag_invalid(anchor("minter state created without a minter input"));
        }
        let kind = if osum > isum {
            if !minter_in {
                verdict.flag_invalid(anchor(&format!(
                    "outputs sum {osum} > inputs {isum} with no minter input"
                )));
            }
            minted += osum - isum;
            supply += osum - isum;
            "mint"
        } else if osum < isum {
            if !minter_in {
                verdict.flag_invalid(anchor(&format!(
                    "outputs sum {osum} < inputs {isum} with no minter input"
                )));
            }
            burned += isum - osum;
            supply -= isum - osum;
            "burn"
        } else if in_states.len() > 1 {
            "merge"
        } else if out_states.len() > 1 {
            "split"
        } else {
            "transfer"
        };
        let mut deltas: Vec<Delta> = out_states
            .iter()
            .map(|s| Delta {
                amount: Some(s.amount),
                owner_from: single_in_owner.clone(),
                owner_to: Some(s.owner_key.clone()),
            })
            .collect();
        if osum < isum {
            // The destroyed difference of a supply burn, as an explicit delta.
            deltas.push(Delta {
                amount: i64::try_from(isum - osum).ok(),
                owner_from: single_in_owner.clone(),
                owner_to: None,
            });
        }
        classified.push(ClassifiedEvent {
            seq: *seq,
            kind,
            accepting_daa: *accepting_daa,
            tx_index: *tx_index,
            deltas,
        });
    }

    // Cells whose tx never produced an event row: index inconsistency.
    for txid in outs_of.keys().chain(ins_of.keys()) {
        if !seen_txids.contains(txid) {
            verdict.flag_unvalidated(format!(
                "no event row for tx {} despite state cells",
                hex::encode(txid)
            ));
            all_events_clean = false;
            break;
        }
    }

    // Live frontier: balances over hash-proven live cells; anything else is
    // an unresolved cell (and has already downgraded its event).
    let mut balances: BTreeMap<String, (i128, u64)> = BTreeMap::new();
    let mut unresolved_cells = 0u64;
    let mut newest_fields: Option<&kcc20::TokenState> = None;
    for cell in &cells {
        if let Some((st, _)) = &cell.proven {
            newest_fields = Some(st); // load order is created_daa ascending
        }
        if !cell.live() {
            continue;
        }
        match judge(cell) {
            Ok(j) => {
                let slot = balances.entry(j.owner_key).or_insert((0, 0));
                slot.0 += j.amount as i128;
                slot.1 += 1;
            }
            Err(_) => unresolved_cells += 1,
        }
    }
    let holders = balances.len() as u64;

    // Sums are stamped only when the full history is provable and clean.
    let provable = all_events_clean && verdict.invalid.is_none();
    let mut supply_out: Option<i64> = None;
    let mut minted_out: Option<i64> = None;
    let mut burned_out: Option<i64> = None;
    if provable {
        match (i64::try_from(supply), i64::try_from(minted), i64::try_from(burned)) {
            (Ok(s), Ok(m), Ok(b)) if supply >= 0 => {
                // Final audit: the hash-proven live frontier must equal
                // genesis + mints − burns exactly.
                let frontier: i128 = balances.values().map(|(bal, _)| bal).sum();
                if frontier == supply && unresolved_cells == 0 {
                    supply_out = Some(s);
                    minted_out = Some(m);
                    burned_out = Some(b);
                } else {
                    verdict.flag_unvalidated(format!(
                        "live frontier sums {frontier} but event history derives supply {supply}"
                    ));
                }
            }
            _ => verdict.flag_unvalidated(format!(
                "derived sums out of i64 range (supply {supply}, minted {minted}, burned {burned})"
            )),
        }
    }

    let fields_json = newest_fields.map(|st| {
        serde_json::json!({
            "owner_identifier": hex::encode(st.owner),
            "identifier_type": hex::encode([st.identifier_type]),
            "amount": hex::encode(&st.amount_raw),
            "is_minter": hex::encode(&st.minter_raw),
        })
        .to_string()
    });

    conn.execute(
        "INSERT OR REPLACE INTO tokens (token_id, status, invalid_reason, supply, minted, burned,
             holders, unresolved_cells, last_activity_daa, fields_json, derived_at_daa)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            token_id.as_slice(),
            verdict.status(),
            verdict.reason(),
            supply_out,
            minted_out,
            burned_out,
            holders,
            unresolved_cells,
            last_activity,
            fields_json,
            derived_at,
        ],
    )
    .map_err(db_err)?;
    {
        let mut insert_event = conn
            .prepare(
                "INSERT INTO token_events (token_id, covenant_id, seq, delta_idx, kind, amount,
                     owner_from, owner_to, accepting_daa, tx_index)
                 VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .map_err(db_err)?;
        for ev in &classified {
            for (delta_idx, d) in ev.deltas.iter().enumerate() {
                insert_event
                    .execute(params![
                        token_id.as_slice(),
                        ev.seq,
                        delta_idx as u64,
                        ev.kind,
                        d.amount,
                        d.owner_from,
                        d.owner_to,
                        ev.accepting_daa,
                        ev.tx_index,
                    ])
                    .map_err(db_err)?;
            }
        }
        let mut insert_balance = conn
            .prepare(
                "INSERT INTO token_balances (token_id, owner, balance, cells)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(db_err)?;
        for (owner, (balance, cell_count)) in &balances {
            let Ok(balance) = i64::try_from(*balance) else {
                // Out-of-range balances never verify (flagged above via the
                // sums gate); the row is skipped rather than stored wrong.
                continue;
            };
            insert_balance
                .execute(params![token_id.as_slice(), owner, balance, cell_count])
                .map_err(db_err)?;
        }
    }
    Ok(())
}

/// Recompute a minter/vault covenant's pinned-token links from its decoded
/// "KCC20 minter" reveals. Returns every token id that was linked before or
/// after — the caller re-derives those tokens.
pub(crate) fn derive_minter(conn: &Connection, minter_id: &[u8; 32]) -> Result<BTreeSet<[u8; 32]>> {
    let registry = registry();
    let cells = load_cells(conn, minter_id)?;
    let mut pins: BTreeSet<[u8; 32]> = BTreeSet::new();
    for cell in &cells {
        let program = {
            let bare = registry.decode(cell.spk_version, &cell.spk_script);
            if bare.template == Some(kcc20::MINTER_TEMPLATE) {
                Some(cell.spk_script.clone())
            } else {
                cell.spent_sig
                    .as_deref()
                    .and_then(|sig| kascov_decode::p2sh_reveal(&cell.spk_script, sig))
            }
        };
        let Some(program) = program else { continue };
        let d = registry.decode(cell.spk_version, &program);
        if d.template != Some(kcc20::MINTER_TEMPLATE) {
            continue;
        }
        for field in &d.fields {
            if matches!(field.name, "kcc20_covenant_a" | "kcc20_covenant_b") {
                if let Ok(id) = <[u8; 32]>::try_from(field.value.as_slice()) {
                    pins.insert(id);
                }
            }
        }
    }
    let mut affected = pins.clone();
    {
        let mut stmt = conn
            .prepare("SELECT token_id FROM token_minters WHERE minter_covenant_id = ?1")
            .map_err(db_err)?;
        let old = stmt
            .query_map([minter_id.as_slice()], |r| r.get::<_, [u8; 32]>(0))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        affected.extend(old);
    }
    conn.execute("DELETE FROM token_minters WHERE minter_covenant_id = ?1", [minter_id.as_slice()])
        .map_err(db_err)?;
    for pin in &pins {
        conn.execute(
            "INSERT OR IGNORE INTO token_minters (minter_covenant_id, token_id) VALUES (?1, ?2)",
            params![minter_id.as_slice(), pin.as_slice()],
        )
        .map_err(db_err)?;
    }
    Ok(affected)
}

/// Is this covenant registered in the tokens table?
pub(crate) fn is_token(conn: &Connection, id: &[u8; 32]) -> Result<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM tokens WHERE token_id = ?1)", [id.as_slice()], |r| {
        r.get(0)
    })
    .map_err(db_err)
}

/// Is this covenant registered as a minter/vault?
pub(crate) fn is_minter(conn: &Connection, id: &[u8; 32]) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM token_minters WHERE minter_covenant_id = ?1)",
        [id.as_slice()],
        |r| r.get(0),
    )
    .map_err(db_err)
}

/// Tokens governed (pinned) by touched minters + touched tokens, re-derived
/// in one deterministic pass. Shared by the apply hook and the reorg rewind.
pub(crate) fn rederive_affected(
    conn: &Connection,
    minters: &BTreeSet<[u8; 32]>,
    tokens: &BTreeSet<[u8; 32]>,
) -> Result<()> {
    let mut todo = tokens.clone();
    for minter in minters {
        todo.extend(derive_minter(conn, minter)?);
    }
    for token in &todo {
        derive_token(conn, token)?;
    }
    Ok(())
}

const DIR_SELECT: &str = "SELECT t.token_id, t.status, t.invalid_reason, t.supply, t.minted,
        t.burned, t.holders, t.unresolved_cells, t.last_activity_daa, t.fields_json,
        t.derived_at_daa,
        (SELECT COUNT(*) FROM covenant_utxos u WHERE u.covenant_id = t.token_id AND u.spent_block IS NULL),
        (SELECT COALESCE(SUM(u.value), 0) FROM covenant_utxos u WHERE u.covenant_id = t.token_id AND u.spent_block IS NULL),
        CASE WHEN EXISTS(SELECT 1 FROM covenant_utxos u WHERE u.covenant_id = t.token_id
                           AND (u.template = 'KCC20 token' OR u.revealed_template = 'KCC20 token'))
             THEN 'KCC20 token' END
 FROM tokens t";

fn map_dir_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TokenDirRow> {
    Ok(TokenDirRow {
        token_id: CovenantId(row.get(0)?),
        validation: row.get(1)?,
        invalid_reason: row.get(2)?,
        supply: row.get(3)?,
        minted: row.get(4)?,
        burned: row.get(5)?,
        holders: row.get(6)?,
        unresolved_cells: row.get(7)?,
        last_activity_daa: row.get(8)?,
        fields_json: row.get(9)?,
        derived_at_daa: row.get(10)?,
        live_utxos: row.get(11)?,
        live_value: row.get(12)?,
        template: row.get(13)?,
    })
}

pub(crate) fn token_directory(conn: &Connection) -> Result<Vec<TokenDirRow>> {
    let sql = format!("{DIR_SELECT} ORDER BY t.last_activity_daa DESC, t.token_id DESC");
    let mut stmt = conn.prepare(&sql).map_err(db_err)?;
    let rows = stmt
        .query_map([], map_dir_row)
        .map_err(db_err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_err)?;
    Ok(rows)
}

pub(crate) fn token_row(conn: &Connection, id: &[u8; 32]) -> Result<Option<TokenDirRow>> {
    let sql = format!("{DIR_SELECT} WHERE t.token_id = ?1");
    let mut stmt = conn.prepare(&sql).map_err(db_err)?;
    let row = stmt
        .query_map([id.as_slice()], map_dir_row)
        .map_err(db_err)?
        .next()
        .transpose()
        .map_err(db_err)?;
    Ok(row)
}

pub(crate) fn token_balances(conn: &Connection, id: &[u8; 32], limit: u64) -> Result<Vec<TokenBalanceRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT owner, balance, cells FROM token_balances WHERE token_id = ?1
             ORDER BY balance DESC, owner LIMIT ?2",
        )
        .map_err(db_err)?;
    let limit = limit.min(i64::MAX as u64) as i64;
    let rows = stmt
        .query_map(params![id.as_slice(), limit], |r| {
            Ok(TokenBalanceRow { owner: r.get(0)?, balance: r.get(1)?, cells: r.get(2)? })
        })
        .map_err(db_err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_err)?;
    Ok(rows)
}

/// One page of a token's classified event deltas, oldest first. `after_seq`
/// is an exclusive cursor (`seq` is strictly increasing per covenant and
/// agrees with the canonical feed order for a single covenant).
pub(crate) fn token_events_page(
    conn: &Connection,
    id: &[u8; 32],
    after_seq: Option<u64>,
    limit: u64,
) -> Result<Vec<TokenEventRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT e.seq, e.delta_idx, e.kind, e.amount, e.owner_from, e.owner_to,
                    e.accepting_daa, e.tx_index, ce.txid, ce.kind
             FROM token_events e
             JOIN covenant_events ce ON ce.covenant_id = e.covenant_id AND ce.seq = e.seq
             WHERE e.token_id = ?1 AND e.seq > ?2
             ORDER BY e.seq, e.delta_idx LIMIT ?3",
        )
        .map_err(db_err)?;
    let after = after_seq.map(|s| s as i64).unwrap_or(-1);
    let limit = limit.min(i64::MAX as u64) as i64;
    let rows = stmt
        .query_map(params![id.as_slice(), after, limit], |r| {
            Ok(TokenEventRow {
                seq: r.get(0)?,
                delta_idx: r.get(1)?,
                kind: r.get(2)?,
                amount: r.get(3)?,
                owner_from: r.get(4)?,
                owner_to: r.get(5)?,
                accepting_daa: r.get(6)?,
                tx_index: r.get(7)?,
                txid: TxId(r.get(8)?),
                event_kind: r.get(9)?,
            })
        })
        .map_err(db_err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_err)?;
    Ok(rows)
}

/// How many classified events (distinct seqs) the validator walked for one
/// token — the "N events checked" figure of the validation summary.
pub(crate) fn token_event_count(conn: &Connection, id: &[u8; 32]) -> Result<u64> {
    conn.query_row(
        "SELECT COUNT(DISTINCT seq) FROM token_events WHERE token_id = ?1",
        [id.as_slice()],
        |r| r.get(0),
    )
    .map_err(db_err)
}

pub(crate) fn token_minter_directory(conn: &Connection) -> Result<Vec<TokenMinterRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT m.minter_covenant_id,
                    COALESCE(c.last_activity_daa, 0),
                    (SELECT COUNT(*) FROM covenant_utxos u WHERE u.covenant_id = m.minter_covenant_id AND u.spent_block IS NULL),
                    (SELECT COALESCE(SUM(u.value), 0) FROM covenant_utxos u WHERE u.covenant_id = m.minter_covenant_id AND u.spent_block IS NULL)
             FROM (SELECT DISTINCT minter_covenant_id FROM token_minters) m
             LEFT JOIN covenants c ON c.covenant_id = m.minter_covenant_id
             ORDER BY 2 DESC, m.minter_covenant_id DESC",
        )
        .map_err(db_err)?;
    let mut rows: Vec<TokenMinterRow> = stmt
        .query_map([], |r| {
            Ok(TokenMinterRow {
                covenant_id: CovenantId(r.get(0)?),
                governs: vec![],
                last_activity_daa: r.get(1)?,
                live_utxos: r.get(2)?,
                live_value: r.get(3)?,
            })
        })
        .map_err(db_err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_err)?;
    let mut pins_stmt = conn
        .prepare("SELECT token_id FROM token_minters WHERE minter_covenant_id = ?1 ORDER BY token_id")
        .map_err(db_err)?;
    for row in &mut rows {
        row.governs = pins_stmt
            .query_map([row.covenant_id.0.as_slice()], |r| Ok(CovenantId(r.get(0)?)))
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BlockHash, Network, Outpoint, TxId};
    use crate::store::{BlockEvents, EventKind, NewEvent, NewUtxo, Store};

    /// A real on-chain KCC20 build (1568-byte reveal program) as the splice
    /// base — synthetic programs can't decode against the observed skeletons,
    /// so every test state is a real-build program with real field offsets.
    const BASE: &[u8] = include_bytes!("../../kascov-decode/fixtures/kcc20_a_a.bin");

    /// (owner, identifier_type, amount LE bytes, is_minter) of one state.
    type St = ([u8; 32], u8, [u8; 8], u8);

    fn program(st: &St) -> Vec<u8> {
        kcc20::splice_token_state(BASE, &st.0, st.1, &st.2, st.3).unwrap()
    }

    fn spk(program: &[u8]) -> Vec<u8> {
        let mut s = vec![0xaa, 0x20];
        s.extend_from_slice(&kcc20::blake2b_256(program));
        s.push(0x87);
        s
    }

    fn push(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        match data.len() {
            0..=0x4b => out.push(data.len() as u8),
            0x4c..=0xff => out.extend_from_slice(&[0x4c, data.len() as u8]),
            _ => {
                out.push(0x4d);
                out.extend_from_slice(&(data.len() as u16).to_le_bytes());
            }
        }
        out.extend_from_slice(data);
        out
    }

    /// The spend sigscript of a token input: the new output states as
    /// struct-of-arrays pushes (owners, types, amounts, minters — the shape
    /// witness recovery proves against), then the input's reveal push.
    fn sig(outs: &[St], reveal: &St) -> Vec<u8> {
        let mut s = Vec::new();
        if !outs.is_empty() {
            let mut owners = Vec::new();
            let mut types = Vec::new();
            let mut amounts = Vec::new();
            let mut minters = Vec::new();
            for (o, t, a, m) in outs {
                owners.extend_from_slice(o);
                types.push(*t);
                amounts.extend_from_slice(a);
                minters.push(*m);
            }
            s.extend(push(&owners));
            s.extend(push(&types));
            s.extend(push(&amounts));
            s.extend(push(&minters));
        }
        s.extend(push(&program(reveal)));
        s
    }

    /// A reveal-only sigscript (no recoverable output args) — what makes a
    /// spending tx's outputs an opaque frontier.
    fn sig_no_args(reveal: &St) -> Vec<u8> {
        push(&program(reveal))
    }

    fn amt(v: i64) -> [u8; 8] {
        v.to_le_bytes()
    }

    fn owner(n: u8) -> [u8; 32] {
        [n; 32]
    }

    fn test_store(name: &str) -> Store {
        let path = std::env::temp_dir()
            .join(format!("kascov-tokens-test-{}-{name}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Store::open(&path, Network::Testnet(10)).unwrap()
    }

    struct BlockBuilder {
        block: BlockEvents,
    }

    impl BlockBuilder {
        fn new(hash: u8, daa: u64) -> Self {
            let mut block = BlockEvents::empty(BlockHash([hash; 32]));
            block.accepting_daa = daa;
            block.accepting_time_ms = daa * 1000;
            block.accepting_blue_score = daa;
            Self { block }
        }
        fn event(mut self, cov: [u8; 32], kind: EventKind, txid: [u8; 32]) -> Self {
            let tx_index = self.block.events.len() as u32;
            self.block.events.push(NewEvent {
                covenant_id: CovenantId(cov),
                kind,
                txid: TxId(txid),
                tx_index,
                payload: None,
                lane_namespace: None,
            });
            self
        }
        fn out(mut self, cov: [u8; 32], txid: [u8; 32], index: u32, st: &St) -> Self {
            self.block.created_utxos.push(NewUtxo {
                outpoint: Outpoint { txid: TxId(txid), index },
                covenant_id: CovenantId(cov),
                value: 1000,
                spk_version: 0,
                spk_script: spk(&program(st)),
            });
            self
        }
        fn spend(mut self, prev_txid: [u8; 32], index: u32, spender: [u8; 32], sig: Vec<u8>) -> Self {
            self.block.spent_utxos.push((
                Outpoint { txid: TxId(prev_txid), index },
                TxId(spender),
                sig,
                0,
                0,
            ));
            self
        }
        fn apply(self, store: &mut Store) {
            let hash = self.block.accepting_block;
            store.apply(&self.block, hash).unwrap();
        }
    }

    fn row(store: &Store, cov: [u8; 32]) -> Option<TokenDirRow> {
        store.token_row(&CovenantId(cov)).unwrap()
    }

    fn kinds(store: &Store, cov: [u8; 32]) -> Vec<(u64, String)> {
        let mut out: Vec<(u64, String)> = store
            .token_events_page(&CovenantId(cov), None, u64::MAX)
            .unwrap()
            .into_iter()
            .map(|e| (e.seq, e.kind))
            .collect();
        out.dedup();
        out
    }

    const COV: [u8; 32] = [0xC1; 32];
    const TX_G: [u8; 32] = [0xA0; 32];
    const TX_M: [u8; 32] = [0xA1; 32];
    const TX_T: [u8; 32] = [0xA2; 32];

    fn minter_state(amount: i64) -> St {
        (owner(0x10), 0x02, amt(amount), 1)
    }
    fn holder(n: u8, amount: i64) -> St {
        (owner(n), 0x00, amt(amount), 0)
    }

    /// genesis (minter branch, 0) → mint 100 → split 60/40: the happy path.
    /// Every state proven by reveal or witness recovery; verified end to end.
    fn apply_happy_path(store: &mut Store) {
        let g0 = minter_state(0);
        BlockBuilder::new(1, 100)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(store);
        // mint: minter continues at 0, holder 0x20 receives 100
        let m_outs = [minter_state(0), holder(0x20, 100)];
        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig(&m_outs, &g0))
            .out(COV, TX_M, 0, &m_outs[0])
            .out(COV, TX_M, 1, &m_outs[1])
            .apply(store);
        // split: holder 0x20's 100 → 60 (0x30) + 40 (0x40)
        let t_outs = [holder(0x30, 60), holder(0x40, 40)];
        BlockBuilder::new(3, 300)
            .event(COV, EventKind::Transition, TX_T)
            .spend(TX_M, 1, TX_T, sig(&t_outs, &m_outs[1]))
            .out(COV, TX_T, 0, &t_outs[0])
            .out(COV, TX_T, 1, &t_outs[1])
            .apply(store);
    }

    #[test]
    fn happy_path_verifies_with_exact_supply_and_balances() {
        let mut store = test_store("happy");
        apply_happy_path(&mut store);
        let t = row(&store, COV).expect("token derived by the apply hook");
        assert_eq!(t.validation, STATUS_VERIFIED);
        assert_eq!(t.invalid_reason, None);
        assert_eq!(t.supply, Some(100));
        assert_eq!(t.minted, Some(100));
        assert_eq!(t.burned, Some(0));
        assert_eq!(t.unresolved_cells, 0);
        // live frontier: minter branch (0), holder 0x30 (60), holder 0x40 (40)
        assert_eq!(t.holders, 3);
        let balances = store.token_balances(&CovenantId(COV), 10).unwrap();
        let by_owner: std::collections::HashMap<String, i64> =
            balances.iter().map(|b| (b.owner.clone(), b.balance)).collect();
        assert_eq!(by_owner[&holder(0x30, 60).into_key()], 60);
        assert_eq!(by_owner[&holder(0x40, 40).into_key()], 40);
        assert_eq!(by_owner[&minter_state(0).into_key()], 0);
        // classification: genesis, mint, split
        assert_eq!(
            kinds(&store, COV),
            vec![(0, "genesis".into()), (1, "mint".into()), (2, "split".into())]
        );
        // deltas of the mint carry the recipient
        let evs = store.token_events_page(&CovenantId(COV), Some(0), 10).unwrap();
        let mint_deltas: Vec<_> = evs.iter().filter(|e| e.seq == 1).collect();
        assert_eq!(mint_deltas.len(), 2);
        assert!(mint_deltas.iter().any(|d| d.amount == Some(100)
            && d.owner_to.as_deref() == Some(holder(0x20, 100).into_key().as_str())));
    }

    /// Conservation violation: non-minter inputs, outputs sum higher.
    #[test]
    fn conservation_violation_is_invalid() {
        let mut store = test_store("violation");
        let g0 = holder(0x20, 100);
        BlockBuilder::new(1, 100)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(&mut store);
        let v_outs = [holder(0x30, 150)];
        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig(&v_outs, &g0))
            .out(COV, TX_M, 0, &v_outs[0])
            .apply(&mut store);
        let t = row(&store, COV).unwrap();
        assert_eq!(t.validation, STATUS_INVALID);
        let reason = t.invalid_reason.unwrap();
        assert!(reason.contains("150 > inputs 100 with no minter input"), "{reason}");
        // sums are never stamped on an invalid token
        assert_eq!(t.supply, None);
        assert_eq!(t.minted, None);
    }

    /// Minter escalation: conserved amounts but a minter state appears
    /// without any minter input.
    #[test]
    fn minter_escalation_is_invalid() {
        let mut store = test_store("escalation");
        let g0 = holder(0x20, 100);
        BlockBuilder::new(1, 100)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(&mut store);
        let e_outs = [(owner(0x30), 0x00, amt(100), 1u8)];
        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig(&e_outs, &g0))
            .out(COV, TX_M, 0, &e_outs[0])
            .apply(&mut store);
        let t = row(&store, COV).unwrap();
        assert_eq!(t.validation, STATUS_INVALID);
        assert!(t.invalid_reason.unwrap().contains("minter state created without a minter input"));
    }

    /// Opaque frontier: a spend whose sigscript carries no recoverable
    /// output args leaves the live outputs unproven — unvalidated, exact
    /// unresolved-cell count, never a guessed verdict.
    #[test]
    fn opaque_frontier_is_unvalidated() {
        let mut store = test_store("opaque");
        let g0 = minter_state(0);
        BlockBuilder::new(1, 100)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(&mut store);
        let m_out = holder(0x20, 100);
        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig_no_args(&g0))
            .out(COV, TX_M, 0, &m_out)
            .apply(&mut store);
        let t = row(&store, COV).unwrap();
        assert_eq!(t.validation, STATUS_UNVALIDATED);
        assert!(t.invalid_reason.unwrap().contains("live state unproven"));
        assert_eq!(t.unresolved_cells, 1);
        assert_eq!(t.supply, None);
        // the unprovable event classified as unknown
        assert_eq!(kinds(&store, COV)[1].1, "unknown");
    }

    /// Terminal burns: legal with a minter input; out of model without one.
    #[test]
    fn terminal_burn_requires_minter() {
        let mut store = test_store("burn-minter");
        let g0 = minter_state(50);
        BlockBuilder::new(1, 100)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(&mut store);
        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Burn, TX_M)
            .spend(TX_G, 0, TX_M, sig_no_args(&g0))
            .apply(&mut store);
        let t = row(&store, COV).unwrap();
        assert_eq!(t.validation, STATUS_VERIFIED);
        assert_eq!(t.supply, Some(0));
        assert_eq!(t.burned, Some(50));
        assert_eq!(kinds(&store, COV)[1].1, "burn");

        let mut store = test_store("burn-nonminter");
        let g0 = holder(0x20, 50);
        BlockBuilder::new(1, 100)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(&mut store);
        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Burn, TX_M)
            .spend(TX_G, 0, TX_M, sig_no_args(&g0))
            .apply(&mut store);
        let t = row(&store, COV).unwrap();
        assert_eq!(t.validation, STATUS_UNVALIDATED);
        assert!(t
            .invalid_reason
            .unwrap()
            .contains("terminal burn without a minter input"));
    }

    /// The reorg gold test: apply, roll back mid-history, re-apply a
    /// different branch — the token tables must be byte-identical to a
    /// from-scratch index that only ever saw the surviving chain.
    #[test]
    fn rollback_reapply_equals_from_scratch() {
        let dump = |store: &Store| -> serde_json::Value {
            let t = row(store, COV);
            serde_json::json!({
                "row": t.map(|mut t| { t.derived_at_daa = None; serde_json::to_value(&t).unwrap() }),
                "events": serde_json::to_value(
                    store.token_events_page(&CovenantId(COV), None, u64::MAX).unwrap()).unwrap(),
                "balances": serde_json::to_value(
                    store.token_balances(&CovenantId(COV), u64::MAX).unwrap()).unwrap(),
            })
        };

        let mut reorged = test_store("gold-reorged");
        apply_happy_path(&mut reorged);
        // roll back the split AND the mint (blocks 3 and 2, tip first)
        reorged.rollback(&[BlockHash([3; 32]), BlockHash([2; 32])]).unwrap();
        // replacement branch: a different mint (250 to holder 0x50)
        let g0 = minter_state(0);
        let m2_outs = [minter_state(0), holder(0x50, 250)];
        BlockBuilder::new(4, 250)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig(&m2_outs, &g0))
            .out(COV, TX_M, 0, &m2_outs[0])
            .out(COV, TX_M, 1, &m2_outs[1])
            .apply(&mut reorged);

        let mut fresh = test_store("gold-fresh");
        BlockBuilder::new(1, 100)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(&mut fresh);
        BlockBuilder::new(4, 250)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig(&m2_outs, &g0))
            .out(COV, TX_M, 0, &m2_outs[0])
            .out(COV, TX_M, 1, &m2_outs[1])
            .apply(&mut fresh);

        assert_eq!(dump(&reorged), dump(&fresh));
        let t = row(&reorged, COV).unwrap();
        assert_eq!(t.validation, STATUS_VERIFIED);
        assert_eq!(t.supply, Some(250));
    }

    /// Rolling back the block whose reveals were a token's ONLY KCC20
    /// evidence must remove the token from the directory entirely (exactly
    /// what a from-scratch index at that height would contain: nothing
    /// provably KCC20). The remaining live cell is a plain P2SH commitment.
    #[test]
    fn rollback_of_only_evidence_removes_the_token() {
        let mut store = test_store("rollback-evidence");
        let g0 = minter_state(0);
        BlockBuilder::new(1, 100)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(&mut store);
        // genesis alone: a P2SH commitment, no KCC20 evidence yet
        assert!(row(&store, COV).is_none());
        let m_outs = [minter_state(0), holder(0x20, 100)];
        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig(&m_outs, &g0))
            .out(COV, TX_M, 0, &m_outs[0])
            .out(COV, TX_M, 1, &m_outs[1])
            .apply(&mut store);
        assert_eq!(row(&store, COV).unwrap().validation, STATUS_VERIFIED);
        // the reveal that proved everything reorgs out
        store.rollback(&[BlockHash([2; 32])]).unwrap();
        assert!(row(&store, COV).is_none(), "unprovable token must not stay listed");
        assert_eq!(store.token_events_page(&CovenantId(COV), None, 10).unwrap().len(), 0);
        assert_eq!(store.token_balances(&CovenantId(COV), 10).unwrap().len(), 0);
    }

    /// A rolled-back spend regresses a once-proven cell to unproven when
    /// other evidence keeps the token listed: verified → unvalidated, never
    /// a stale "verified".
    #[test]
    fn reveal_rollback_regresses_verdict() {
        let mut store = test_store("rollback-regress");
        let g0 = minter_state(0);
        BlockBuilder::new(1, 100)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(&mut store);
        // mint whose sig carries NO recoverable args — the outputs are only
        // provable once THEY are spent
        let m_outs = [minter_state(0), holder(0x20, 100)];
        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig_no_args(&g0))
            .out(COV, TX_M, 0, &m_outs[0])
            .out(COV, TX_M, 1, &m_outs[1])
            .apply(&mut store);
        assert_eq!(row(&store, COV).unwrap().validation, STATUS_UNVALIDATED);
        // the split's reveals + args prove the mint outputs retroactively
        let t_outs = [holder(0x30, 60), holder(0x40, 40)];
        BlockBuilder::new(3, 300)
            .event(COV, EventKind::Transition, TX_T)
            .spend(TX_M, 1, TX_T, sig(&t_outs, &m_outs[1]))
            .out(COV, TX_T, 0, &t_outs[0])
            .out(COV, TX_T, 1, &t_outs[1])
            .apply(&mut store);
        // still unresolved: M:0 (the continuing minter branch) never revealed
        let t = row(&store, COV).unwrap();
        assert_eq!(t.validation, STATUS_UNVALIDATED);
        assert_eq!(t.unresolved_cells, 1);
        // rolling back the split deletes the reveal that proved M:1 — the
        // verdict must regress with it, not stay cached
        store.rollback(&[BlockHash([3; 32])]).unwrap();
        let t = row(&store, COV).unwrap();
        assert_eq!(t.validation, STATUS_UNVALIDATED);
        assert_eq!(t.unresolved_cells, 2, "both mint outputs unproven again");
    }

    /// The versioned boot pass: derives from scratch, agrees with the
    /// apply-hook derivation, and is an O(1) no-op while the version stamp
    /// is current (a planted sentinel survives untouched).
    #[test]
    fn boot_pass_is_version_gated_and_idempotent() {
        let mut store = test_store("boot-pass");
        apply_happy_path(&mut store);
        let hook_derived = serde_json::to_value(row(&store, COV).unwrap()).unwrap();
        // full pass from scratch (no version stamp yet on this store)
        assert_eq!(store.derive_tokens_if_stale().unwrap(), 1);
        assert_eq!(serde_json::to_value(row(&store, COV).unwrap()).unwrap(), hook_derived);
        // current version: no-op — a sentinel survives
        store
            .raw_conn()
            .execute("UPDATE tokens SET holders = 999", [])
            .unwrap();
        assert_eq!(store.derive_tokens_if_stale().unwrap(), 0);
        assert_eq!(row(&store, COV).unwrap().holders, 999);
        // stale version: the pass wipes and re-derives
        store
            .raw_conn()
            .execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('token_derivation_version', '0')",
                [],
            )
            .unwrap();
        assert_eq!(store.derive_tokens_if_stale().unwrap(), 1);
        assert_eq!(serde_json::to_value(row(&store, COV).unwrap()).unwrap(), hook_derived);
    }

    /// Amount encoding at the extremes: i64::MAX round-trips exactly; a
    /// sign-bit amount (negative script number) never parses into supply —
    /// the token is unvalidated, not misread as a huge unsigned value.
    #[test]
    fn amount_bounds_are_conservative() {
        let mut store = test_store("amount-max");
        let g0 = minter_state(0);
        BlockBuilder::new(1, 100)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(&mut store);
        let m_outs = [minter_state(0), holder(0x20, i64::MAX)];
        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig(&m_outs, &g0))
            .out(COV, TX_M, 0, &m_outs[0])
            .out(COV, TX_M, 1, &m_outs[1])
            .apply(&mut store);
        let t = row(&store, COV).unwrap();
        assert_eq!(t.validation, STATUS_VERIFIED);
        assert_eq!(t.supply, Some(i64::MAX));
        assert_eq!(t.minted, Some(i64::MAX));

        let mut store = test_store("amount-negative");
        let neg: St = (owner(0x20), 0x00, [0, 0, 0, 0, 0, 0, 0, 0x80], 0);
        BlockBuilder::new(1, 100)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &neg)
            .apply(&mut store);
        let n_outs = [holder(0x30, 1)];
        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig(&n_outs, &neg))
            .out(COV, TX_M, 0, &n_outs[0])
            .apply(&mut store);
        let t = row(&store, COV).unwrap();
        assert_eq!(t.validation, STATUS_UNVALIDATED);
        assert!(t.invalid_reason.unwrap().contains("amount out of script-int range"));
        assert_eq!(t.supply, None);
    }

    /// Pre-capture rows (NULL tx_index) never block validation: a token's
    /// own event seq is a total order, so ordering is provably irrelevant.
    #[test]
    fn null_tx_index_does_not_block_verification() {
        let mut store = test_store("null-txindex");
        apply_happy_path(&mut store);
        store
            .raw_conn()
            .execute("UPDATE covenant_events SET tx_index = NULL", [])
            .unwrap();
        store
            .raw_conn()
            .execute("DELETE FROM meta WHERE key = 'token_derivation_version'", [])
            .unwrap();
        assert!(store.derive_tokens_if_stale().unwrap() >= 1);
        let t = row(&store, COV).unwrap();
        assert_eq!(t.validation, STATUS_VERIFIED);
        assert_eq!(t.supply, Some(100));
    }

    /// Owner display encoding: pubkeys route bare, everything else carries a
    /// type prefix that can never be mistaken for a pubkey.
    #[test]
    fn owner_display_encoding() {
        let pk = format!("00{}", hex::encode([0xab; 32]));
        assert_eq!(owner_display(&pk), hex::encode([0xab; 32]));
        let cov = format!("02{}", hex::encode([0xcd; 32]));
        assert_eq!(owner_display(&cov), format!("covenant:{}", hex::encode([0xcd; 32])));
        let script = format!("01{}", hex::encode([0xef; 32]));
        assert_eq!(owner_display(&script), format!("script:{}", hex::encode([0xef; 32])));
        assert_eq!(owner_display("zz"), "zz");
    }

    trait IntoKey {
        fn into_key(&self) -> String;
    }
    impl IntoKey for St {
        fn into_key(&self) -> String {
            let mut b = vec![self.1];
            b.extend_from_slice(&self.0);
            hex::encode(b)
        }
    }
}
