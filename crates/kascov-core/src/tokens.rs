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
/// 2: the state-block locator replaced the pinned-skeleton match, so the
///    re-stamp pass reclassifies stored reveals and discovery must run again
///    to pick up tokens previously filed as generic p2sh commitments.
/// 3: the supply gate now consults `unvalidated`, so already-derived rows that
///    published a supply under an unvalidated badge must be rebuilt to drop it.
/// 4: identifier type 0x03 admitted, so cells previously rejected as an
///    unknown owner type now resolve and their tokens must be re-derived.
/// 5: supply is now split by owner type (covenant / wallet / script), so
///    existing rows must be rebuilt to populate the new columns.
/// 6: recovery reaches genesis cells — sibling outputs serve as splice bases
///    and per-field arguments are read, including the numbers carried in
///    OP_0/OP_1..OP_16 — so launch-time creator allocations that were stuck
///    unproven now resolve and their tokens must be re-derived.
/// 7: per-trade chain facts (`token_trades`) and the market-covenant link —
///    every token must be rebuilt so its trades are extracted.
pub const TOKEN_DERIVATION_VERSION: &str = "7";

/// The stored derivation stamp COMPOSES every version whose bump changes what
/// a stored program decodes as. Three passes rewrite decoder stamps without
/// touching the derivation constant (`backfill_templates`,
/// `reclassify_if_stale`, `restamp_kcc20_if_stale`); composing them here makes
/// any decoder learning invalidate every stored trade and price mechanically
/// rather than by someone remembering to bump two constants together.
pub fn token_derivation_stamp() -> String {
    format!(
        "{TOKEN_DERIVATION_VERSION}/{}/{}",
        crate::store::CLASSIFIER_VERSION,
        crate::store::KCC20_RESTAMP_VERSION
    )
}

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
    /// Proven supply split by decoded owner type: covenant-held (a bonding
    /// curve's inventory or a locked pool), wallet-held, script-held. `None`
    /// on the same gate as `supply`.
    pub held_covenant: Option<i64>,
    pub held_wallet: Option<i64>,
    pub held_script: Option<i64>,
    pub unresolved_cells: u64,
    pub last_activity_daa: u64,
    /// Latest hash-proven state fields as a JSON object (label → hex value),
    /// same shape the per-request registry decode used to produce.
    pub fields_json: Option<String>,
    pub derived_at_daa: Option<u64>,
    /// The unique covenant owner of this token's live inventory — the bonding
    /// curve or pool it trades against. `None` when zero or several covenants
    /// hold balances: nothing downstream may then attribute a reserve.
    pub market_covenant_id: Option<CovenantId>,
    /// Admitted trades extracted from hash-proven state deltas (see
    /// `extract_trade_candidate`). Raw counts; pricing is gated elsewhere.
    pub trades: i64,
    /// Admitted trades that also moved a third covenant — stored, never priced.
    pub co_moved_trades: i64,
    /// Admitted trades whose event predates timestamp capture. Nonzero nulls
    /// every 24h window for this token: a partial window is never published.
    pub trades_missing_time: i64,
    pub live_utxos: u64,
    pub live_value: u64,
    pub template: Option<String>,
}

/// One admitted trade, as stored: the integer price pair plus the market
/// covenant's before/after balances on both legs.
#[derive(Clone, Debug, Serialize)]
pub struct TokenTradeRow {
    pub seq: u64,
    pub txid: TxId,
    pub market_covenant_id: CovenantId,
    pub side: String,
    pub base_amount: i64,
    pub quote_sompi: i64,
    pub kas_before_sompi: i64,
    pub kas_after_sompi: i64,
    pub base_before: i64,
    pub base_after: i64,
    pub co_covenants: i64,
    pub accepting_daa: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepting_time_ms: Option<i64>,
    /// Who took the other side, when exactly one key owner did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty: Option<String>,
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
        // 0x03 is the same entity as 0x00, an x-only pubkey, so it routes to
        // the same address page. It is kept distinguishable because the
        // authorization differs: the cell carries no signature and is spent by
        // presenting a co-present P2PK input.
        "03" => format!("presence:{rest}"),
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
            (Some(_), Some(sig)) => match kascov_decode::p2sh_reveal(&cell.spk_script, sig) {
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
            },
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
const TYPE_MINTER_DOMAIN: [(u8, u8); 8] = [
    (0x00, 0x00),
    (0x00, 0x01),
    (0x01, 0x00),
    (0x01, 0x01),
    (0x02, 0x00),
    (0x02, 0x01),
    // 0x03: an ordinary wallet pubkey authorized by a CO-PRESENT P2PK input
    // rather than by a signature on the token cell itself. Proven from chain,
    // not from any published spec: the arm at offset 0x0144 of the deployed
    // program calls OpTxInputSpk (KIP-17) and rebuilds 0x0000 || 0x20 ||
    // owner || 0xac, which is verbatim Kaspa Schnorr P2PK; all 143 distinct
    // type-0x03 owners on mainnet are valid secp256k1 x coordinates (a chance
    // coincidence would be 2^-143); and 72 such cells have already been spent,
    // so mainnet consensus itself has executed this arm.
    (0x03, 0x00),
    (0x03, 0x01),
];

/// Cap on per-field candidates taken from a single sigscript. Observed launch
/// transactions carry a handful of each; a script with more distinct 32-byte
/// pushes than this is a vault, whose arguments arrive struct-of-arrays anyway.
/// The cap bounds the brute force, and hitting it can only leave a cell
/// unproven — never mis-prove one, since every candidate is still hash-gated.
const FIELD_CANDIDATE_CAP: usize = 64;

/// Every value a sigscript pushes, including the ones the opcode carries
/// itself. `OP_0` and `OP_1`..`OP_16` encode their number in the opcode rather
/// than as pushed data, so reading only `inst.data` makes a small amount — a
/// one-unit creator allocation, say — invisible to recovery.
fn arg_pushes(sig: &[u8]) -> Vec<Vec<u8>> {
    let (instructions, _) = kascov_decode::disasm::disassemble(sig);
    instructions
        .into_iter()
        .filter_map(|inst| match (inst.opcode, inst.data) {
            (_, Some(data)) => Some(data),
            (0x00, None) => Some(Vec::new()),
            (op @ 0x51..=0x60, None) => Some(vec![op - 0x50]),
            _ => None,
        })
        .collect()
}

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
            let no_inputs = Vec::new();
            let ins = ins_of.get(txid).unwrap_or(&no_inputs);
            let n = outs.len();
            // Same-build splice bases: the proven programs of this tx's own
            // token inputs AND of its sibling outputs. Siblings matter because
            // a genesis transaction has no token inputs at all, so a cell
            // created there that nobody has spent — a creator allocation held
            // back at launch — could never be recovered without them. Where a
            // base comes from cannot make a proof wrong: `prove_output_state`
            // accepts only a splice whose blake2b equals the output's own
            // commitment, so anything it returns IS the committed program.
            let bases: BTreeSet<Vec<u8>> = ins
                .iter()
                .chain(outs.iter())
                .filter_map(|&i| cells[i].proven.as_ref().map(|(_, p)| p.clone()))
                .filter(|p| kcc20::has_state_block(p))
                .collect();
            if bases.is_empty() {
                continue;
            }
            // Argument carriers: this token's input sigs, then the sigs of
            // co-spent inputs of other covenants in the same tx. A genesis
            // transaction contributes none of its own, but the launch covenant
            // it spends is exactly such a co-spent input.
            let mut sigs: Vec<Vec<u8>> = ins
                .iter()
                .filter_map(|&i| cells[i].spent_sig.clone())
                .collect();
            let foreign: Vec<Vec<u8>> = foreign_sigs
                .query_map(params![txid.as_slice(), token_id.as_slice()], |r| r.get(0))
                .map_err(db_err)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_err)?;
            sigs.extend(foreign);
            for sig in &sigs {
                let pushes = arg_pushes(sig);
                // Two observed argument shapes. Struct-of-arrays: one push
                // carrying all n owners and one carrying all n amounts, which
                // is how a covenant run passes the states it is about to
                // create. Per-field: a separate push per value, which is how a
                // launch transaction passes its genesis states.
                let owners_soa: Vec<&Vec<u8>> =
                    pushes.iter().filter(|p| p.len() == n * 32).collect();
                let amounts_soa: Vec<&Vec<u8>> =
                    pushes.iter().filter(|p| p.len() == n * 8).collect();
                let owners_flat: Vec<&Vec<u8>> = pushes.iter().filter(|p| p.len() == 32).collect();
                let amounts_flat: Vec<&Vec<u8>> = pushes.iter().filter(|p| p.len() <= 8).collect();
                for (k, &out_idx) in outs.iter().enumerate() {
                    if cells[out_idx].proven.is_some() {
                        continue;
                    }
                    let mut owner_cands: BTreeSet<[u8; 32]> = BTreeSet::new();
                    for ow in &owners_soa {
                        owner_cands
                            .insert(ow[k * 32..(k + 1) * 32].try_into().expect("32-byte slice"));
                    }
                    for ow in owners_flat.iter().take(FIELD_CANDIDATE_CAP) {
                        owner_cands.insert(ow.as_slice().try_into().expect("32-byte push"));
                    }
                    let mut amount_cands: BTreeSet<[u8; 8]> = BTreeSet::new();
                    for am in &amounts_soa {
                        amount_cands
                            .insert(am[k * 8..(k + 1) * 8].try_into().expect("8-byte slice"));
                    }
                    for am in amounts_flat.iter().take(FIELD_CANDIDATE_CAP) {
                        // Script integers are little-endian and minimally
                        // encoded, so a one-unit allocation arrives as a single
                        // byte and widens by zero-extension on the right.
                        let mut a = [0u8; 8];
                        a[..am.len()].copy_from_slice(am);
                        amount_cands.insert(a);
                    }
                    'cell: for base in &bases {
                        for owner in &owner_cands {
                            for amount in &amount_cands {
                                for (id_type, minter) in TYPE_MINTER_DOMAIN {
                                    if let Some(st) = kcc20::prove_output_state(
                                        base,
                                        &cells[out_idx].spk_script,
                                        owner,
                                        id_type,
                                        amount,
                                        minter,
                                    ) {
                                        let program = kcc20::splice_token_state(
                                            base, owner, id_type, amount, minter,
                                        )
                                        .expect("base had a state block");
                                        cells[out_idx].proven = Some((st, program));
                                        cells[out_idx].unproven = None;
                                        changed = true;
                                        break 'cell;
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

/// One admitted trade candidate: this token's cells moved against exactly one
/// covenant owner's inventory, with KAS moving the opposite way in the same
/// transaction. Raw chain facts only — pricing, bracketing and windows are
/// decided at serve time so this table never has to be rewritten for a
/// policy change.
struct TradeRow {
    seq: u64,
    txid: [u8; 32],
    market: [u8; 32],
    side: &'static str,
    base_amount: i64,
    quote_sompi: i64,
    kas_before: i64,
    kas_after: i64,
    base_before: i64,
    base_after: i64,
    co_covenants: i64,
    accepting_daa: u64,
    blue_score: Option<i64>,
    tx_index: Option<u64>,
    time_ms: Option<i64>,
    /// Who took the other side, when exactly one key owner did.
    counterparty: Option<String>,
}

/// D1-D4 of the trade design, over states that are ALREADY hash-proven.
///
/// The admission rules, each rejecting a real transaction shape:
/// - exactly ONE covenant owner with a nonzero net delta (a graduation moves
///   two covenant owners; it is not a trade);
/// - KAS and tokens moved in OPPOSITE directions (a launch hands the curve
///   both, same sign; a self-consolidation merge moves no net tokens at all);
/// - both magnitudes fit i64.
///
/// `co_covenants` counts OTHER covenants evented by the same tx, from
/// covenant_events — a source-table fact no decoder pass ever rewrites. A
/// template test here fails open: a token output is a P2SH, stamped
/// 'p2sh commitment' until some future spend reveals it.
#[allow(clippy::too_many_arguments)]
fn extract_trade_candidate(
    conn: &Connection,
    token_id: &[u8; 32],
    seq: u64,
    txid: &[u8; 32],
    accepting_daa: u64,
    tx_index: Option<u64>,
    blue_score: Option<i64>,
    time_ms: Option<i64>,
    in_states: &[Judged],
    out_states: &[Judged],
) -> Result<Option<TradeRow>> {
    // per-owner net token delta, i128 against overflow
    let mut delta: BTreeMap<&str, i128> = BTreeMap::new();
    for s in out_states {
        *delta.entry(s.owner_key.as_str()).or_insert(0) += s.amount as i128;
    }
    for s in in_states {
        *delta.entry(s.owner_key.as_str()).or_insert(0) -= s.amount as i128;
    }
    let movers: Vec<(&str, i128)> = delta
        .iter()
        .filter(|(k, v)| k.starts_with("02") && **v != 0)
        .map(|(k, v)| (*k, *v))
        .collect();
    let [(market_key, d_tok)] = movers.as_slice() else {
        return Ok(None);
    };
    // WHO traded. The market took one side; the counterparty is the single
    // non-covenant owner that took the other. More than one and it is
    // ambiguous (a batched settlement moves several), so it stays NULL rather
    // than guessing: an identity column that is sometimes wrong is worse than
    // one that is sometimes blank.
    let counterparty: Option<String> = {
        let others: Vec<&str> = delta
            .iter()
            .filter(|(k, v)| !k.starts_with("02") && **v != 0 && (**v > 0) != (*d_tok > 0))
            .map(|(k, _)| *k)
            .collect();
        match others.as_slice() {
            [one] => Some((*one).to_string()),
            _ => None,
        }
    };
    let Ok(market_bytes) = hex::decode(&market_key[2..]) else {
        return Ok(None);
    };
    let Ok(market) = <[u8; 32]>::try_from(market_bytes.as_slice()) else {
        return Ok(None);
    };

    // K0/K1: the market covenant's KAS consumed and re-created by this tx.
    // The created side anchors on txid (the PK prefix) with covenant_id
    // filtered in Rust: without ANALYZE the planner otherwise picks the
    // covenant index and walks every cell the market has ever had.
    let mut k0: i128 = 0;
    {
        let mut stmt = conn
            .prepare_cached(
                "SELECT COALESCE(SUM(value), 0) FROM covenant_utxos
                 WHERE spent_txid = ?1 AND covenant_id = ?2",
            )
            .map_err(db_err)?;
        k0 = stmt
            .query_row(params![txid.as_slice(), market.as_slice()], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(db_err)? as i128;
    }
    let mut k1: i128 = 0;
    {
        let mut stmt = conn
            .prepare_cached("SELECT covenant_id, value FROM covenant_utxos WHERE txid = ?1")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([txid.as_slice()], |r| {
                Ok((r.get::<_, [u8; 32]>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(db_err)?;
        for row in rows {
            let (cov, value) = row.map_err(db_err)?;
            if cov == market {
                k1 += value as i128;
            }
        }
    }
    let d_kas = k1 - k0;
    // opposite directions or it is not a trade
    if d_kas == 0 || (d_kas > 0) == (*d_tok > 0) {
        return Ok(None);
    }
    let b0: i128 = in_states
        .iter()
        .filter(|s| s.owner_key == *market_key)
        .map(|s| s.amount as i128)
        .sum();
    let b1: i128 = out_states
        .iter()
        .filter(|s| s.owner_key == *market_key)
        .map(|s| s.amount as i128)
        .sum();
    let (Ok(quote_sompi), Ok(base_amount)) =
        (i64::try_from(d_kas.abs()), i64::try_from(d_tok.abs()))
    else {
        return Ok(None);
    };
    let (Ok(kas_before), Ok(kas_after), Ok(base_before), Ok(base_after)) = (
        i64::try_from(k0),
        i64::try_from(k1),
        i64::try_from(b0),
        i64::try_from(b1),
    ) else {
        return Ok(None);
    };

    // other covenants evented by the same tx (beyond the token and its market)
    let co_covenants: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT covenant_id) FROM covenant_events
             WHERE txid = ?1 AND covenant_id NOT IN (?2, ?3)",
            params![txid.as_slice(), token_id.as_slice(), market.as_slice()],
            |r| r.get(0),
        )
        .map_err(db_err)?;

    Ok(Some(TradeRow {
        seq,
        txid: *txid,
        market,
        side: if d_kas > 0 { "buy" } else { "sell" },
        base_amount,
        quote_sompi,
        kas_before,
        kas_after,
        base_before,
        base_after,
        co_covenants,
        accepting_daa,
        blue_score,
        tx_index,
        time_ms,
        counterparty,
    }))
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
                format!(
                    "live state unproven for {}",
                    outpoint_str(&cell.txid, cell.index)
                )
            } else {
                format!(
                    "state unproven for spent output {}",
                    outpoint_str(&cell.txid, cell.index)
                )
            }
        }));
    };
    // 0x03 is a pubkey owner like 0x00, differing only in how a spend is
    // authorized (a co-present P2PK input instead of an inline signature).
    // It is an AUTHORIZATION mode, not an accounting one: the owner is still a
    // 32 byte key and conservation is untouched, so admitting it cannot move a
    // supply figure, only who a balance is attributed to.
    if !matches!(st.identifier_type, 0x00 | 0x01 | 0x02 | 0x03) {
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
    Ok(Judged {
        amount,
        minter,
        owner_key: st.owner_key(),
    })
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
        "DELETE FROM token_trades WHERE token_id = ?1",
        "DELETE FROM tokens WHERE token_id = ?1",
    ] {
        conn.execute(sql, [id.as_slice()]).map_err(db_err)?;
    }
    Ok(())
}

fn processed_daa(conn: &Connection) -> Result<Option<u64>> {
    Ok(conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'processed_daa'",
            [],
            |r| r.get::<_, String>(0),
        )
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
    conn.execute(
        "DELETE FROM token_events WHERE token_id = ?1",
        [token_id.as_slice()],
    )
    .map_err(db_err)?;
    conn.execute(
        "DELETE FROM token_balances WHERE token_id = ?1",
        [token_id.as_slice()],
    )
    .map_err(db_err)?;
    conn.execute(
        "DELETE FROM token_trades WHERE token_id = ?1",
        [token_id.as_slice()],
    )
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
            "SELECT seq, kind, txid, accepting_daa, tx_index, accepting_blue_score,
                    accepting_time_ms FROM covenant_events
             WHERE covenant_id = ?1 ORDER BY seq",
        )
        .map_err(db_err)?;
    #[allow(clippy::type_complexity)]
    let events: Vec<(
        u64,
        String,
        [u8; 32],
        u64,
        Option<u64>,
        Option<i64>,
        Option<i64>,
    )> = stmt
        .query_map([token_id.as_slice()], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
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

    let mut trade_rows: Vec<TradeRow> = Vec::new();
    for (seq, ev_kind, txid, accepting_daa, tx_index, blue_score, time_ms) in &events {
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
                deltas: vec![Delta {
                    amount: None,
                    owner_from: None,
                    owner_to: None,
                }],
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
                unknown(
                    &mut classified,
                    &mut verdict,
                    &mut all_events_clean,
                    anchor(&reason),
                );
                continue;
            }
        };
        // Trade extraction rides the SAME proven states the verdict rides —
        // never token_events rows, whose entries are absolute cell amounts
        // (summing those reads every merge as a phantom inflow). Genesis and
        // migration fall out of the admission rules by themselves.
        if let Some(t) = extract_trade_candidate(
            conn,
            token_id,
            *seq,
            txid,
            *accepting_daa,
            *tx_index,
            *blue_score,
            *time_ms,
            &in_states,
            &out_states,
        )? {
            trade_rows.push(t);
        }

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
            let owners: BTreeSet<&str> = in_states.iter().map(|s| s.owner_key.as_str()).collect();
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

    // Who actually holds the supply, split by the owner type kascov already
    // decoded per cell. "Total supply" alone is misleading for a bonding-curve
    // token: before graduation a large share sits in the curve covenant's own
    // inventory, and after graduation in the locked AMM pool, neither of which
    // is in anyone's hands. Both are covenant-owned (type 0x02), while wallet
    // holdings are pubkey-owned (0x00 signature-authorized, 0x03 authorized by
    // a co-present P2PK input). Splitting them settles total versus circulating
    // by MEASUREMENT rather than by argument, and every input is already
    // hash-proven, so this adds no new trust.
    let (mut held_covenant, mut held_wallet, mut held_script) = (0i128, 0i128, 0i128);
    for (owner_key, (bal, _)) in &balances {
        match owner_key.get(..2) {
            Some("02") => held_covenant += bal,
            Some("00") | Some("03") => held_wallet += bal,
            Some("01") => held_script += bal,
            // An owner type kascov does not classify cannot be attributed, so
            // it is deliberately counted in none of the three buckets: the
            // parts must never silently absorb something unexplained.
            _ => {}
        }
    }

    // Sums are stamped only when the full history is provable and clean.
    //
    // `verdict.unvalidated` is part of this gate, not just `invalid`. Most
    // unvalidated paths route through the `unknown` closure, which also clears
    // all_events_clean, so the two agreed by construction. The terminal-burn
    // branch flags unvalidated WITHOUT clearing it, and that one gap let a
    // token kascov had explicitly declared out of model still stamp a supply:
    // two testnet-10 tokens were publishing one under an `unvalidated` badge.
    // Reading the verdict directly makes the invariant hold by definition
    // rather than by every future caller remembering to clear a flag.
    let provable = all_events_clean && verdict.invalid.is_none() && verdict.unvalidated.is_none();
    let mut supply_out: Option<i64> = None;
    let mut minted_out: Option<i64> = None;
    let mut burned_out: Option<i64> = None;
    // Published on the same gate as supply: a breakdown of a number kascov
    // could not prove would be worse than no breakdown at all.
    let mut held_covenant_out: Option<i64> = None;
    let mut held_wallet_out: Option<i64> = None;
    let mut held_script_out: Option<i64> = None;
    if provable {
        match (
            i64::try_from(supply),
            i64::try_from(minted),
            i64::try_from(burned),
        ) {
            (Ok(s), Ok(m), Ok(b)) if supply >= 0 => {
                // Final audit: the hash-proven live frontier must equal
                // genesis + mints − burns exactly.
                let frontier: i128 = balances.values().map(|(bal, _)| bal).sum();
                if frontier == supply && unresolved_cells == 0 {
                    supply_out = Some(s);
                    minted_out = Some(m);
                    burned_out = Some(b);
                    // The frontier already equals supply exactly, and the three
                    // buckets partition the same balances, so they sum to it by
                    // construction. Only publish them if that actually holds:
                    // an unclassified owner type would leave a remainder, and a
                    // breakdown that does not add up must not ship.
                    if held_covenant + held_wallet + held_script == frontier {
                        held_covenant_out = i64::try_from(held_covenant).ok();
                        held_wallet_out = i64::try_from(held_wallet).ok();
                        held_script_out = i64::try_from(held_script).ok();
                    }
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

    // The market link, from the live frontier: the UNIQUE covenant owner with
    // a nonzero balance. Zero or several means no reserve, no spot, no exit
    // value can ever be attributed — each trade still carries its own
    // counterparty, so the trade history survives the ambiguity.
    let market_covenant_id: Option<Vec<u8>> = {
        let covs: Vec<&String> = balances
            .iter()
            .filter(|(k, (bal, _))| k.starts_with("02") && *bal != 0)
            .map(|(k, _)| k)
            .collect();
        match covs.as_slice() {
            [one] => hex::decode(&one[2..]).ok().filter(|b| b.len() == 32),
            _ => None,
        }
    };
    let trades_stored = trade_rows.len() as i64;
    let co_moved_trades = trade_rows.iter().filter(|t| t.co_covenants > 0).count() as i64;
    let trades_missing_time = trade_rows.iter().filter(|t| t.time_ms.is_none()).count() as i64;

    conn.execute(
        "INSERT OR REPLACE INTO tokens (token_id, status, invalid_reason, supply, minted, burned,
             holders, held_covenant, held_wallet, held_script,
             unresolved_cells, last_activity_daa, fields_json, derived_at_daa,
             market_covenant_id, trades, co_moved_trades, trades_missing_time)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            token_id.as_slice(),
            verdict.status(),
            verdict.reason(),
            supply_out,
            minted_out,
            burned_out,
            holders,
            held_covenant_out,
            held_wallet_out,
            held_script_out,
            unresolved_cells,
            last_activity,
            fields_json,
            derived_at,
            market_covenant_id,
            trades_stored,
            co_moved_trades,
            trades_missing_time,
        ],
    )
    .map_err(db_err)?;
    {
        let mut insert_trade = conn
            .prepare_cached(
                "INSERT INTO token_trades (token_id, seq, txid, market_covenant_id, side,
                     base_amount, quote_sompi, kas_before_sompi, kas_after_sompi,
                     base_before, base_after, co_covenants, accepting_daa,
                     accepting_blue_score, tx_index, accepting_time_ms, counterparty)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            )
            .map_err(db_err)?;
        for t in &trade_rows {
            insert_trade
                .execute(params![
                    token_id.as_slice(),
                    t.seq,
                    t.txid.as_slice(),
                    t.market.as_slice(),
                    t.side,
                    t.base_amount,
                    t.quote_sompi,
                    t.kas_before,
                    t.kas_after,
                    t.base_before,
                    t.base_after,
                    t.co_covenants,
                    t.accepting_daa,
                    t.blue_score,
                    t.tx_index,
                    t.time_ms,
                    t.counterparty.as_deref(),
                ])
                .map_err(db_err)?;
        }
    }
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
    conn.execute(
        "DELETE FROM token_minters WHERE minter_covenant_id = ?1",
        [minter_id.as_slice()],
    )
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
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM tokens WHERE token_id = ?1)",
        [id.as_slice()],
        |r| r.get(0),
    )
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
    let mut markets: BTreeSet<[u8; 32]> = BTreeSet::new();
    for token in &todo {
        derive_token(conn, token)?;
        if let Some(m) = conn
            .query_row(
                "SELECT market_covenant_id FROM tokens WHERE token_id = ?1",
                [token.as_slice()],
                |r| r.get::<_, Option<[u8; 32]>>(0),
            )
            .optional()
            .map_err(db_err)?
            .flatten()
        {
            markets.insert(m);
        }
    }
    // Re-verify each touched token's market program. The skip gate makes the
    // steady state one hash compare plus an incremental-cheap replay.
    crate::market::rederive_market_programs(conn, &markets)?;
    Ok(())
}

const DIR_SELECT: &str = "SELECT t.token_id, t.status, t.invalid_reason, t.supply, t.minted,
        t.burned, t.holders, t.held_covenant, t.held_wallet, t.held_script,
        t.unresolved_cells, t.last_activity_daa, t.fields_json,
        t.derived_at_daa, t.market_covenant_id, t.trades, t.co_moved_trades,
        t.trades_missing_time,
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
        held_covenant: row.get(7)?,
        held_wallet: row.get(8)?,
        held_script: row.get(9)?,
        unresolved_cells: row.get(10)?,
        last_activity_daa: row.get(11)?,
        fields_json: row.get(12)?,
        derived_at_daa: row.get(13)?,
        market_covenant_id: row.get::<_, Option<[u8; 32]>>(14)?.map(CovenantId),
        trades: row.get(15)?,
        co_moved_trades: row.get(16)?,
        trades_missing_time: row.get(17)?,
        live_utxos: row.get(18)?,
        live_value: row.get(19)?,
        template: row.get(20)?,
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

pub(crate) fn token_trades_page(
    conn: &Connection,
    id: &[u8; 32],
    limit: u64,
) -> Result<Vec<TokenTradeRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT seq, txid, market_covenant_id, side, base_amount, quote_sompi,
                    kas_before_sompi, kas_after_sompi, base_before, base_after,
                    co_covenants, accepting_daa, accepting_time_ms, counterparty
             FROM token_trades WHERE token_id = ?1 ORDER BY seq DESC LIMIT ?2",
        )
        .map_err(db_err)?;
    let limit = limit.min(i64::MAX as u64) as i64;
    let rows = stmt
        .query_map(params![id.as_slice(), limit], |r| {
            Ok(TokenTradeRow {
                seq: r.get(0)?,
                txid: TxId(r.get(1)?),
                market_covenant_id: CovenantId(r.get(2)?),
                side: r.get(3)?,
                base_amount: r.get(4)?,
                quote_sompi: r.get(5)?,
                kas_before_sompi: r.get(6)?,
                kas_after_sompi: r.get(7)?,
                base_before: r.get(8)?,
                base_after: r.get(9)?,
                co_covenants: r.get(10)?,
                accepting_daa: r.get(11)?,
                accepting_time_ms: r.get(12)?,
                counterparty: r.get(13)?,
            })
        })
        .map_err(db_err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_err)?;
    Ok(rows)
}

pub(crate) fn token_balances(
    conn: &Connection,
    id: &[u8; 32],
    limit: u64,
) -> Result<Vec<TokenBalanceRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT owner, balance, cells FROM token_balances WHERE token_id = ?1
             ORDER BY balance DESC, owner LIMIT ?2",
        )
        .map_err(db_err)?;
    let limit = limit.min(i64::MAX as u64) as i64;
    let rows = stmt
        .query_map(params![id.as_slice(), limit], |r| {
            Ok(TokenBalanceRow {
                owner: r.get(0)?,
                balance: r.get(1)?,
                cells: r.get(2)?,
            })
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

/// One page of a token's classified event deltas, NEWEST first. `before_seq`
/// is an exclusive cursor walking backwards; `None` starts at the tip.
///
/// A history is read from the present backwards. Serving only the ascending
/// page meant a reader of an active token saw its first minutes and nothing
/// since: KRON has 1,346 events and a first page holds 100 of them.
pub(crate) fn token_events_page_before(
    conn: &Connection,
    id: &[u8; 32],
    before_seq: Option<u64>,
    limit: u64,
) -> Result<Vec<TokenEventRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT e.seq, e.delta_idx, e.kind, e.amount, e.owner_from, e.owner_to,
                    e.accepting_daa, e.tx_index, ce.txid, ce.kind
             FROM token_events e
             JOIN covenant_events ce ON ce.covenant_id = e.covenant_id AND ce.seq = e.seq
             WHERE e.token_id = ?1 AND e.seq < ?2
             ORDER BY e.seq DESC, e.delta_idx DESC LIMIT ?3",
        )
        .map_err(db_err)?;
    // `None` means "from the tip": every real seq is below this bound.
    let before = before_seq.map(|s| s as i64).unwrap_or(i64::MAX);
    let limit = limit.min(i64::MAX as u64) as i64;
    let rows = stmt
        .query_map(params![id.as_slice(), before, limit], |r| {
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
        .prepare(
            "SELECT token_id FROM token_minters WHERE minter_covenant_id = ?1 ORDER BY token_id",
        )
        .map_err(db_err)?;
    for row in &mut rows {
        row.governs = pins_stmt
            .query_map([row.covenant_id.0.as_slice()], |r| {
                Ok(CovenantId(r.get(0)?))
            })
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
        let path = std::env::temp_dir().join(format!(
            "kascov-tokens-test-{}-{name}.db",
            std::process::id()
        ));
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
                outpoint: Outpoint {
                    txid: TxId(txid),
                    index,
                },
                covenant_id: CovenantId(cov),
                value: 1000,
                spk_version: 0,
                spk_script: spk(&program(st)),
            });
            self
        }
        /// An output with an explicit KAS value — the market-covenant side of
        /// a trade is measured purely in cell values.
        fn out_v(mut self, cov: [u8; 32], txid: [u8; 32], index: u32, st: &St, value: u64) -> Self {
            self.block.created_utxos.push(NewUtxo {
                outpoint: Outpoint {
                    txid: TxId(txid),
                    index,
                },
                covenant_id: CovenantId(cov),
                value,
                spk_version: 0,
                spk_script: spk(&program(st)),
            });
            self
        }
        /// An output whose committed script is supplied verbatim, for tests
        /// driven by real on-chain commitments rather than synthesised states.
        fn out_spk(mut self, cov: [u8; 32], txid: [u8; 32], index: u32, spk: Vec<u8>) -> Self {
            self.block.created_utxos.push(NewUtxo {
                outpoint: Outpoint {
                    txid: TxId(txid),
                    index,
                },
                covenant_id: CovenantId(cov),
                value: 1000,
                spk_version: 0,
                spk_script: spk,
            });
            self
        }
        fn spend(
            mut self,
            prev_txid: [u8; 32],
            index: u32,
            spender: [u8; 32],
            sig: Vec<u8>,
        ) -> Self {
            self.block.spent_utxos.push((
                Outpoint {
                    txid: TxId(prev_txid),
                    index,
                },
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
    /// Covenant-owned balance (type 0x02): a launchpad curve's own inventory,
    /// or a locked pool after graduation. Not in anyone's hands.
    fn covenant_held(n: u8, amount: i64) -> St {
        (owner(n), 0x02, amt(amount), 0)
    }
    /// Presence-authorized wallet balance (type 0x03): same entity kind as a
    /// 0x00 holder, spent via a co-present P2PK input rather than a signature.
    fn presence_holder(n: u8, amount: i64) -> St {
        (owner(n), 0x03, amt(amount), 0)
    }

    /// Total supply alone misreads a bonding-curve token: much of it is the
    /// curve's unsold inventory. The split by owner type is what makes
    /// "circulating" answerable by measurement instead of by argument, and the
    /// parts must always account for the whole.
    #[test]
    fn supply_splits_by_owner_type() {
        let mut store = test_store("held-split");
        let outs = [
            covenant_held(0x11, 700),   // the curve still holds this
            holder(0x22, 200),          // a signature-owned wallet
            presence_holder(0x33, 100), // a presence-owned wallet (KRON's mode)
        ];
        // Genesis mints into a minter cell, then one transition distributes it
        // into the three owner kinds. The spend is what reveals the program and
        // lets witness recovery prove the live cells, exactly as on chain.
        let g0 = minter_state(1000);
        BlockBuilder::new(1, 100)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(&mut store);
        let mut b = BlockBuilder::new(2, 200)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig(&outs, &g0));
        for (i, o) in outs.iter().enumerate() {
            b = b.out(COV, TX_M, i as u32, o);
        }
        b.apply(&mut store);

        let t = row(&store, COV).unwrap();
        assert_eq!(t.validation, STATUS_VERIFIED);
        assert_eq!(t.supply, Some(1000));
        assert_eq!(
            t.held_covenant,
            Some(700),
            "curve inventory is not circulating"
        );
        assert_eq!(t.held_wallet, Some(300), "0x00 and 0x03 are both wallets");
        // The invariant that makes the breakdown trustworthy at all.
        assert_eq!(
            t.held_covenant.unwrap() + t.held_wallet.unwrap() + t.held_script.unwrap_or(0),
            t.supply.unwrap(),
            "the parts must account for the whole"
        );
    }

    /// A launch holds part of the supply back in a creator cell nobody has
    /// spent, so nothing ever reveals that cell's program. Recovery reaches it
    /// anyway: the sibling curve output of the same genesis is a same-build
    /// splice base, and the launch covenant's sigscript carries the fields per
    /// output rather than as arrays — with a one-unit allocation encoded as
    /// bare OP_1, in the opcode itself. Modelled on mainnet eager-teal-magpie
    /// (genesis c3faf69d), whose curve took 999,999,999 and creator cell 1.
    #[test]
    fn genesis_creator_allocation_recovers_without_a_reveal() {
        const LAUNCH: [u8; 32] = [0xC2; 32];
        const TX_L: [u8; 32] = [0xA3; 32];
        let mut store = test_store("genesis-holdback");
        let curve = covenant_held(0x20, 999_999_999);
        let dev = presence_holder(0x30, 1);

        // The launch covenant has to exist before the genesis can spend it.
        BlockBuilder::new(1, 100)
            .event(LAUNCH, EventKind::Genesis, TX_L)
            .out(LAUNCH, TX_L, 0, &minter_state(0))
            .apply(&mut store);

        // Per-field launch arguments: each owner as its own push, each amount
        // minimally encoded — 999,999,999 in four bytes, and 1 as bare OP_1.
        let mut launch_sig = Vec::new();
        launch_sig.extend(push(&curve.0));
        launch_sig.push(0x52); // OP_2, the curve's identifier type
        launch_sig.extend(push(&999_999_999i64.to_le_bytes()[..4]));
        launch_sig.push(0x00); // OP_0, not a minter
        launch_sig.extend(push(&dev.0));
        launch_sig.push(0x53); // OP_3, presence-owned
        launch_sig.push(0x51); // OP_1 — the entire creator allocation
        launch_sig.push(0x00);
        launch_sig.extend(push(&program(&minter_state(0)))); // the launch reveal

        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Genesis, TX_G)
            .event(LAUNCH, EventKind::Burn, TX_G)
            .spend(TX_L, 0, TX_G, launch_sig)
            .out(COV, TX_G, 0, &curve)
            .out(COV, TX_G, 1, &dev)
            .apply(&mut store);

        // The curve trades on, which is what reveals its program. The creator
        // cell is never touched, so only the sibling can serve as its base.
        BlockBuilder::new(3, 300)
            .event(COV, EventKind::Transition, TX_T)
            .spend(TX_G, 0, TX_T, sig(&[curve], &curve))
            .out(COV, TX_T, 0, &curve)
            .apply(&mut store);

        let t = row(&store, COV).unwrap();
        assert_eq!(
            t.validation, STATUS_VERIFIED,
            "reason: {:?}",
            t.invalid_reason
        );
        assert_eq!(
            t.supply,
            Some(1_000_000_000),
            "the round total the launch intended"
        );
        assert_eq!(t.held_covenant, Some(999_999_999));
        assert_eq!(
            t.held_wallet,
            Some(1),
            "the creator allocation is a wallet balance"
        );
    }

    /// A history is served newest first and walked backwards a page at a time,
    /// so the cursor has to cover every delta exactly once and then stop. The
    /// bug that motivated this: the page served the OLDEST events and never
    /// paged, so an active token showed its first minutes and nothing since.
    #[test]
    fn history_pages_backwards_over_every_event_exactly_once() {
        let mut store = test_store("events-desc");
        apply_happy_path(&mut store);
        let id = CovenantId(COV);

        let ascending = store.token_events_page(&id, None, u64::MAX).unwrap();
        assert!(ascending.len() >= 4, "need a few deltas to page over");
        let newest = ascending.last().unwrap().seq;
        assert!(newest > 0, "the token must have moved after genesis");

        // First descending page really is the tip, in reverse order.
        let head = store.token_events_page_before(&id, None, u64::MAX).unwrap();
        assert_eq!(head.len(), ascending.len(), "same rows, other direction");
        assert_eq!(head.first().unwrap().seq, newest, "newest first");
        assert!(
            head.windows(2)
                .all(|w| (w[0].seq, w[0].delta_idx) >= (w[1].seq, w[1].delta_idx)),
            "a descending page must not wobble"
        );

        // Walk it one delta at a time. Small pages are where an off-by-one in
        // an exclusive cursor shows up as a skipped or repeated event.
        let mut seen: Vec<(u64, u64)> = Vec::new();
        let mut before: Option<u64> = None;
        for _ in 0..64 {
            let page = store.token_events_page_before(&id, before, 1).unwrap();
            let Some(row) = page.first() else { break };
            seen.push((row.seq, row.delta_idx));
            // the cursor is exclusive on seq, so a seq with several deltas is
            // consumed whole by the caller before it advances
            before = Some(row.seq);
            let rest = store
                .token_events_page_before(&id, before, u64::MAX)
                .unwrap();
            if rest.is_empty() && row.seq == 0 {
                break;
            }
        }
        assert_eq!(
            seen.first().unwrap().0,
            newest,
            "the walk starts at the tip"
        );
        assert!(
            seen.windows(2).all(|w| w[0].0 > w[1].0),
            "each step must land on a strictly older event, never repeat one"
        );
        assert_eq!(
            *seen.last().unwrap(),
            (0, 0),
            "the walk reaches genesis and stops"
        );
    }

    /// The same recovery, end to end on real mainnet bytes: the launch
    /// arguments transaction c3faf69d actually passed, the two commitments its
    /// token outputs actually carry, and the deployed program those
    /// commitments commit to. Output :2 has never been spent, so nothing ever
    /// reveals it and only recovery can reach it. This is the live token
    /// eager-teal-magpie, reproduced offline.
    #[test]
    fn real_mainnet_launch_recovers_the_creator_cell() {
        /// The 2,433-byte unguarded KCC20 program KRON deploys, from chain.
        const KRON: &[u8] = include_bytes!("../../kascov-decode/fixtures/kcc20_unguarded_kron.bin");
        /// Input 0's sigscript of c3faf69d, argument pushes only. The 172 KB
        /// redeem push that follows is omitted: every candidate filter rejects
        /// it on length, so carrying it would only bloat the fixture.
        const ARGS: &[u8] =
            include_bytes!("../../kascov-decode/fixtures/kcc20_kron_launch_args.bin");
        const LAUNCH: [u8; 32] = [0xC2; 32];
        const TX_L: [u8; 32] = [0xA3; 32];

        fn bytes(h: &str) -> Vec<u8> {
            hex::decode(h).unwrap()
        }
        let mut store = test_store("kron-launch");
        // What mainnet committed to at c3faf69d:1 (the bonding curve's unsold
        // inventory) and c3faf69d:2 (the allocation the creator held back).
        let curve_spk =
            bytes("aa20a5b8fe20d75829e4b9f481bf3f018734ce11ee3bd8856bbdddc28729c49f3c2487");
        let creator_spk =
            bytes("aa201ed943cf928024fc6ae30f230641d582c80a41070a917c5d2732f2d31cb52b6c87");
        let curve_owner: [u8; 32] =
            bytes("81dd058295fc39ec31ea3c70adceb7580fd39a36affb2e584786b7bb245c9f89")
                .try_into()
                .unwrap();
        let curve_program =
            kcc20::splice_token_state(KRON, &curve_owner, 0x02, &amt(999_999_999), 0).unwrap();

        BlockBuilder::new(1, 100)
            .event(LAUNCH, EventKind::Genesis, TX_L)
            .out(LAUNCH, TX_L, 0, &minter_state(0))
            .apply(&mut store);

        let mut launch_sig = ARGS.to_vec();
        launch_sig.extend(push(&program(&minter_state(0))));
        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Genesis, TX_G)
            .event(LAUNCH, EventKind::Burn, TX_G)
            .spend(TX_L, 0, TX_G, launch_sig)
            .out_spk(COV, TX_G, 1, curve_spk.clone())
            .out_spk(COV, TX_G, 2, creator_spk)
            .apply(&mut store);

        // The curve trades on, which is what reveals its program. The creator
        // cell stays untouched, exactly as it still is on mainnet.
        let mut curve_sig = Vec::new();
        curve_sig.extend(push(&curve_owner));
        curve_sig.extend(push(&amt(999_999_999)));
        curve_sig.extend(push(&curve_program));
        BlockBuilder::new(3, 300)
            .event(COV, EventKind::Transition, TX_T)
            .spend(TX_G, 1, TX_T, curve_sig)
            .out_spk(COV, TX_T, 0, curve_spk)
            .apply(&mut store);

        let t = row(&store, COV).unwrap();
        assert_eq!(
            t.validation, STATUS_VERIFIED,
            "reason: {:?}",
            t.invalid_reason
        );
        assert_eq!(
            t.supply,
            Some(1_000_000_000),
            "the launch minted a round billion"
        );
        assert_eq!(
            t.held_covenant,
            Some(999_999_999),
            "the curve's unsold inventory"
        );
        assert_eq!(t.held_wallet, Some(1), "the creator kept one unit");
    }

    /// The trade layer's foundation: a token delta against exactly one
    /// covenant owner, with the market covenant's KAS moving the opposite way
    /// in the same tx, is stored as a trade with the integer price pair. A
    /// launch (KAS and inventory arriving together) is not a trade, and the
    /// numbers stored are the market's actual before/after cell values.
    #[test]
    fn trades_extract_from_proven_deltas_and_opposite_kas() {
        const MKT: [u8; 32] = [0x11; 32]; // == the curve owner in covenant_held(0x11, ..)
        const TX_MKT: [u8; 32] = [0xB0; 32];
        let mut store = test_store("trade-extract");

        let g0 = minter_state(1000);
        let launch = [covenant_held(0x11, 700), holder(0x22, 300)];
        let after = [covenant_held(0x11, 500), holder(0x33, 200)];

        // the market covenant exists and holds 5,000 sompi
        BlockBuilder::new(1, 100)
            .event(MKT, EventKind::Genesis, TX_MKT)
            .out_v(MKT, TX_MKT, 0, &holder(0x44, 1), 5000)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(&mut store);

        // launch: the curve receives inventory, the market's KAS does not
        // move in this tx — not a trade
        let mut b = BlockBuilder::new(2, 200)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig(&launch, &g0));
        for (i, o) in launch.iter().enumerate() {
            b = b.out(COV, TX_M, i as u32, o);
        }
        b.apply(&mut store);

        // the trade: 200 tokens leave the curve, 1,000 sompi enter the market
        let mut t = BlockBuilder::new(3, 300)
            .event(COV, EventKind::Transition, TX_T)
            .event(MKT, EventKind::Transition, TX_T)
            .spend(TX_M, 0, TX_T, sig(&after, &launch[0]))
            .spend(TX_MKT, 0, TX_T, Vec::new())
            .out_v(MKT, TX_T, 5, &holder(0x44, 1), 6000);
        for (i, o) in after.iter().enumerate() {
            t = t.out(COV, TX_T, i as u32, o);
        }
        t.apply(&mut store);

        let row = row(&store, COV).unwrap();
        assert_eq!(
            row.validation, STATUS_VERIFIED,
            "reason: {:?}",
            row.invalid_reason
        );
        assert_eq!(row.trades, 1, "the launch is not a trade; the sale is");
        assert_eq!(row.co_moved_trades, 0);
        assert_eq!(row.market_covenant_id, Some(CovenantId(MKT)));

        let trades = store.token_trades_page(&CovenantId(COV), 10).unwrap();
        assert_eq!(trades.len(), 1);
        let tr = &trades[0];
        assert_eq!(tr.side, "buy", "the market gained KAS and shed tokens");
        assert_eq!(
            (tr.base_amount, tr.quote_sompi),
            (200, 1000),
            "the integer price pair"
        );
        assert_eq!((tr.kas_before_sompi, tr.kas_after_sompi), (5000, 6000));
        assert_eq!((tr.base_before, tr.base_after), (700, 500));
        assert_eq!(tr.co_covenants, 0);
    }

    /// A graduation moves inventory from one covenant owner to another. Two
    /// covenant owners with nonzero deltas is not a trade, whatever the KAS
    /// did — the admission rule, not an event-kind allowlist, is what knows.
    #[test]
    fn a_graduation_is_not_a_trade() {
        const MKT: [u8; 32] = [0x11; 32];
        const POOL: [u8; 32] = [0x12; 32];
        const TX_MKT: [u8; 32] = [0xB0; 32];
        let mut store = test_store("trade-grad");
        let g0 = minter_state(1000);
        let curve = [covenant_held(0x11, 1000)];
        let pool = [covenant_held(0x12, 1000)];

        BlockBuilder::new(1, 100)
            .event(MKT, EventKind::Genesis, TX_MKT)
            .out_v(MKT, TX_MKT, 0, &holder(0x44, 1), 9000)
            .event(COV, EventKind::Genesis, TX_G)
            .out(COV, TX_G, 0, &g0)
            .apply(&mut store);
        BlockBuilder::new(2, 200)
            .event(COV, EventKind::Transition, TX_M)
            .spend(TX_G, 0, TX_M, sig(&curve, &g0))
            .out(COV, TX_M, 0, &curve[0])
            .apply(&mut store);
        // graduation: inventory moves curve -> pool while the market's KAS
        // leaves in the same tx
        BlockBuilder::new(3, 300)
            .event(COV, EventKind::Transition, TX_T)
            .event(MKT, EventKind::Transition, TX_T)
            .spend(TX_M, 0, TX_T, sig(&pool, &curve[0]))
            .spend(TX_MKT, 0, TX_T, Vec::new())
            .out(COV, TX_T, 0, &pool[0])
            .apply(&mut store);

        let row = row(&store, COV).unwrap();
        assert_eq!(
            row.trades, 0,
            "two covenant owners moved: a migration, not a trade"
        );
        assert!(store
            .token_trades_page(&CovenantId(COV), 10)
            .unwrap()
            .is_empty());
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
        let by_owner: std::collections::HashMap<String, i64> = balances
            .iter()
            .map(|b| (b.owner.clone(), b.balance))
            .collect();
        assert_eq!(by_owner[&holder(0x30, 60).into_key()], 60);
        assert_eq!(by_owner[&holder(0x40, 40).into_key()], 40);
        assert_eq!(by_owner[&minter_state(0).into_key()], 0);
        // classification: genesis, mint, split
        assert_eq!(
            kinds(&store, COV),
            vec![
                (0, "genesis".into()),
                (1, "mint".into()),
                (2, "split".into())
            ]
        );
        // deltas of the mint carry the recipient
        let evs = store
            .token_events_page(&CovenantId(COV), Some(0), 10)
            .unwrap();
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
        assert!(
            reason.contains("150 > inputs 100 with no minter input"),
            "{reason}"
        );
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
        assert!(t
            .invalid_reason
            .unwrap()
            .contains("minter state created without a minter input"));
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
            .as_deref()
            .unwrap()
            .contains("terminal burn without a minter input"));
        // THE INVARIANT: a token kascov could not validate must not publish a
        // number anyway. This branch flags unvalidated without clearing
        // all_events_clean, and `provable` used to consult only `invalid`, so
        // two testnet-10 tokens shipped a supply under an `unvalidated` badge.
        // Asserting the status alone is what let that through for so long.
        assert_eq!(t.supply, None, "unvalidated must never publish a supply");
        assert_eq!(t.minted, None, "unvalidated must never publish minted");
        assert_eq!(t.burned, None, "unvalidated must never publish burned");
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
        reorged
            .rollback(&[BlockHash([3; 32]), BlockHash([2; 32])])
            .unwrap();
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
        assert!(
            row(&store, COV).is_none(),
            "unprovable token must not stay listed"
        );
        assert_eq!(
            store
                .token_events_page(&CovenantId(COV), None, 10)
                .unwrap()
                .len(),
            0
        );
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
        assert_eq!(
            serde_json::to_value(row(&store, COV).unwrap()).unwrap(),
            hook_derived
        );
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
        assert_eq!(
            serde_json::to_value(row(&store, COV).unwrap()).unwrap(),
            hook_derived
        );
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
        assert!(t
            .invalid_reason
            .unwrap()
            .contains("amount out of script-int range"));
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
            .execute(
                "DELETE FROM meta WHERE key = 'token_derivation_version'",
                [],
            )
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
        assert_eq!(
            owner_display(&cov),
            format!("covenant:{}", hex::encode([0xcd; 32]))
        );
        let script = format!("01{}", hex::encode([0xef; 32]));
        assert_eq!(
            owner_display(&script),
            format!("script:{}", hex::encode([0xef; 32]))
        );
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
