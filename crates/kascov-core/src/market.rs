//! Market-program verification: reading a bonding curve's own constants out
//! of its hash-committed bytecode, and replaying its trades against them.
//!
//! This module is what LICENSES publishing a price. The trade extraction in
//! `tokens.rs` records integer chain facts, but a fact is not yet a price: a
//! per-owner delta is not access-controlled (anyone can create a cell owned by
//! `0x02||C`), so a single transaction that buys 1,000 tokens and donates 999
//! back to the curve would read as "1 token for 6 KAS" — a published price
//! three orders of magnitude wrong, from under a dollar of spend. The gate
//! that stops it is the bracket test: an executed price must lie between the
//! marginal prices the curve program itself computes before and after the
//! transaction, and computing those needs `vKas` — a constant that lives
//! INSIDE the program whose blake2b already equals the P2SH commitment kascov
//! verifies. So the program is not trusted; it is read, and then every one of
//! its historical trades is replayed against its own formula before a single
//! figure is published. No recognised program, no price — a vesting vault or
//! an escrow also "holds inventory", and its owner can move KAS against their
//! own token at any ratio they like.
//!
//! The skeleton is pinned from three independent mainnet deployments (PUBIC,
//! SHAY, KASBOY curves — 172 KB programs, 2,206 pushes each): every byte
//! outside 37 slot positions is identical across them, and the named slots
//! carry each token's own covenant id, reserve, creator and curve constants.
//! A program that differs anywhere else simply does not match, and matching is
//! never widened to make a program fit.

use std::collections::BTreeSet;

use rusqlite::{params, Connection, OptionalExtension};

use crate::store::db_err;
use crate::Result;

/// The curve moves KAS in whole multiples of this (0.01 KAS): the bracket
/// gets exactly one quantum of slack for the program's own quantisation.
pub const KAS_QUANTUM_SOMPI: i128 = 1_000_000;
/// Two-sided cap on constant-product growth per trade. Measured residuals are
/// ~3e-8 relative; 1e-5 is 300x headroom, while an in-covenant fee of even a
/// few bps overshoots it by orders of magnitude and refuses the parameters.
pub const INVARIANT_MAX_GROWTH_PPM: i128 = 10;
/// A program's constants value nothing until this many real trades replayed
/// cleanly against them.
pub const MIN_EXERCISED_TRADES: i64 = 3;

/// The audited KRON curve build: one full program as fixture (itself a
/// blake2b-verified reveal from mainnet), plus the push positions that vary
/// per deployment. Everything OUTSIDE these positions must match the fixture
/// byte for byte — gaps, opcodes and constant pushes alike.
const CURVE_FIXTURE: &[u8] = include_bytes!("../../kascov-decode/fixtures/kron_curve_v1.bin");
const CURVE_PUSHES: usize = 2206;
const SLOT_INDICES: [usize; 37] = [
    1, 2, 19, 81, 141, 143, 185, 187, 311, 313, 326, 328, 391, 457, 459, 579, 582, 595, 597, 665,
    724, 857, 859, 877, 878, 893, 1006, 1015, 1190, 1192, 1237, 1239, 2084, 2087, 2101, 2103, 2167,
];
const IDX_TOKEN_COVENANT: usize = 1;
const IDX_TOKEN_RESERVE: usize = 2;
const IDX_CREATOR: [usize; 2] = [19, 81];
const IDX_VKAS: [usize; 12] = [185, 187, 311, 313, 457, 459, 579, 582, 1237, 1239, 2084, 2087];
const IDX_GRADUATION: usize = 724;

/// What a matched curve program states about itself. Every field is read from
/// bytes the chain committed to; none is taken from any published list.
#[derive(Clone, Debug)]
pub struct CurveParams {
    pub token_covenant_id: [u8; 32],
    /// The program's virtual KAS constant, in the program's own units.
    /// `V` in sompi is this times [`KAS_QUANTUM_SOMPI`].
    pub v_kas_units: i64,
    pub graduation_kas_sompi: i64,
    pub creator_fee_owner: [u8; 32],
    /// The tokenReserve STATE field of this particular reveal — it moves per
    /// trade, so it describes the state that was spent, not the live one.
    pub token_reserve: i64,
}

impl CurveParams {
    pub fn v_sompi(&self) -> i128 {
        self.v_kas_units as i128 * KAS_QUANTUM_SOMPI
    }
}

/// One push unit: the byte range of the push (length prefix included) and the
/// value it pushes. Mirrors `tokens::arg_pushes` semantics exactly — OP_0 and
/// OP_1..OP_16 count as pushes of the number they carry — because the slot
/// indices were derived under those semantics.
fn push_units(program: &[u8]) -> Vec<(std::ops::Range<usize>, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let n = program.len();
    while i < n {
        let op = program[i];
        let (data_start, len): (usize, usize) = match op {
            0x01..=0x4b => (i + 1, op as usize),
            0x4c if i + 1 < n => (i + 2, program[i + 1] as usize),
            0x4d if i + 2 < n => {
                (i + 3, u16::from_le_bytes([program[i + 1], program[i + 2]]) as usize)
            }
            0x4e if i + 4 < n => (
                i + 5,
                u32::from_le_bytes([
                    program[i + 1],
                    program[i + 2],
                    program[i + 3],
                    program[i + 4],
                ]) as usize,
            ),
            0x00 => {
                out.push((i..i + 1, Vec::new()));
                i += 1;
                continue;
            }
            0x51..=0x60 => {
                out.push((i..i + 1, vec![op - 0x50]));
                i += 1;
                continue;
            }
            _ => {
                i += 1;
                continue;
            }
        };
        if data_start + len > n {
            break;
        }
        out.push((i..data_start + len, program[data_start..data_start + len].to_vec()));
        i = data_start + len;
    }
    out
}

fn le_i64(bytes: &[u8]) -> Option<i64> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    let mut buf = [0u8; 8];
    buf[..bytes.len()].copy_from_slice(bytes);
    let v = i64::from_le_bytes(buf);
    (v >= 0).then_some(v)
}

/// Match a revealed program against the pinned curve build and read its
/// constants. `None` on ANY divergence outside the slots — a program that is
/// almost the audited build is not the audited build.
pub fn match_kron_curve(program: &[u8]) -> Option<CurveParams> {
    let cand = push_units(program);
    if cand.len() != CURVE_PUSHES {
        return None;
    }
    let fixture = push_units(CURVE_FIXTURE);
    debug_assert_eq!(fixture.len(), CURVE_PUSHES);
    let slots: BTreeSet<usize> = SLOT_INDICES.into_iter().collect();

    // Lockstep walk: the gap BEFORE each push and (for non-slot pushes) the
    // whole push unit must be byte-identical to the fixture.
    let mut fpos = 0usize;
    let mut cpos = 0usize;
    for i in 0..CURVE_PUSHES {
        let (fr, fdata) = &fixture[i];
        let (cr, cdata) = &cand[i];
        if CURVE_FIXTURE[fpos..fr.start] != program[cpos..cr.start] {
            return None;
        }
        if !slots.contains(&i) && (fdata != cdata || CURVE_FIXTURE[fr.clone()] != program[cr.clone()])
        {
            return None;
        }
        fpos = fr.end;
        cpos = cr.end;
    }
    if CURVE_FIXTURE[fpos..] != program[cpos..] {
        return None;
    }

    // Internal consistency: repeated slots must agree with themselves.
    let vkas_vals: BTreeSet<Option<i64>> =
        IDX_VKAS.iter().map(|&i| le_i64(&cand[i].1)).collect();
    let [Some(v_kas_units)] = *vkas_vals.into_iter().collect::<Vec<_>>().as_slice() else {
        return None;
    };
    let creators: BTreeSet<&Vec<u8>> = IDX_CREATOR.iter().map(|&i| &cand[i].1).collect();
    let [creator] = *creators.into_iter().collect::<Vec<_>>().as_slice() else { return None };
    let creator_fee_owner: [u8; 32] = creator.as_slice().try_into().ok()?;
    let token_covenant_id: [u8; 32] = cand[IDX_TOKEN_COVENANT].1.as_slice().try_into().ok()?;
    Some(CurveParams {
        token_covenant_id,
        v_kas_units,
        graduation_kas_sompi: le_i64(&cand[IDX_GRADUATION].1)?,
        creator_fee_owner,
        token_reserve: le_i64(&cand[IDX_TOKEN_RESERVE].1)?,
    })
}

/// The pool build the curve graduates into: a 94-byte state block (guard,
/// kasReserve, tokenReserve, tokenCovenantId, shares, lpTokenCovenantId) in
/// front of the same 57,475-byte template every curve embeds. The template is
/// pinned from the copy inside a blake2b-proven curve program; only the two
/// creator slots vary per deployment, and only the two reserves vary per
/// state.
const POOL_FIXTURE: &[u8] = include_bytes!("../../kascov-decode/fixtures/kron_pool_v1.bin");
/// "unmatched" is a verdict of a particular MATCHER, not of the program: when
/// the matcher learns a new build, old unmatched rows must be retried even
/// though neither the program nor its hash moved. The tag records which
/// matcher gave up.
///
/// This is module-level and not a local const for a reason that cost a real
/// bug: the retry only ever happens if something invalidates the market gate
/// in `derive_tokens_if_stale`. While this lived inside the matcher, bumping
/// it re-tagged nothing, because the gate never noticed and returned early —
/// the tag existed but the retry it promised could not fire. `market_stamp()`
/// folds it into that gate so a bump mechanically forces re-verification.
pub(crate) const MATCHER_VERSION: &str = "2";

/// The only skeletons that mean "this program byte-matched an audited build".
/// Every tally and every publish gate reads this ALLOWLIST rather than testing
/// `!= unmatched`: a denylist silently promotes a future give-up tag (say
/// `unmatched:3`) into "matched", which is match-widening by accident.
pub(crate) const MATCHED_SKELETONS: [&str; 2] = ["KRON curve v1", "KRON pool v1"];

pub(crate) fn unmatched_tag() -> String {
    format!("unmatched:{MATCHER_VERSION}")
}

/// The market verification gate, composite so that either half moving forces
/// every stored market program to be read again.
pub(crate) fn market_stamp() -> String {
    format!("3-pool-fee-model/{MATCHER_VERSION}")
}

const POOL_STATE_LEN: usize = 94;
const POOL_TEMPLATE_CREATOR_SLOTS: [usize; 2] = [159, 345];

/// What a matched pool program states about itself, all from committed bytes.
#[derive(Clone, Debug)]
pub struct PoolParams {
    pub token_covenant_id: [u8; 32],
    /// KAS reserve in the program's 0.01 KAS units; sompi is this x 1e6.
    pub kas_reserve_units: i64,
    pub token_reserve: i64,
    pub shares: i64,
    pub lp_token_covenant_id: [u8; 32],
    pub creator: [u8; 32],
}

/// Match a revealed program against the pinned pool build. The state block is
/// parsed at fixed offsets (its five pushes have fixed widths), the template
/// part must byte-equal the fixture outside the two creator slots, and the
/// two creator slots must agree with each other.
pub fn match_kron_pool(program: &[u8]) -> Option<PoolParams> {
    if program.len() != POOL_STATE_LEN + POOL_FIXTURE.len() || program[0] != 0x6b {
        return None;
    }
    // 0x6b | 08 k | 08 t | 20 cov | 08 shares | 20 lp  == 94 bytes
    if program[1] != 0x08
        || program[10] != 0x08
        || program[19] != 0x20
        || program[52] != 0x08
        || program[61] != 0x20
    {
        return None;
    }
    let kas_reserve_units = le_i64(&program[2..10])?;
    let token_reserve = le_i64(&program[11..19])?;
    let token_covenant_id: [u8; 32] = program[20..52].try_into().ok()?;
    let shares = le_i64(&program[53..61])?;
    let lp_token_covenant_id: [u8; 32] = program[62..94].try_into().ok()?;

    let tpl = &program[POOL_STATE_LEN..];
    let fixture_units = push_units(POOL_FIXTURE);
    let cand_units = push_units(tpl);
    if cand_units.len() != fixture_units.len() {
        return None;
    }
    let slots: BTreeSet<usize> = POOL_TEMPLATE_CREATOR_SLOTS.into_iter().collect();
    let mut fpos = 0usize;
    let mut cpos = 0usize;
    for i in 0..fixture_units.len() {
        let (fr, _) = &fixture_units[i];
        let (cr, _) = &cand_units[i];
        if POOL_FIXTURE[fpos..fr.start] != tpl[cpos..cr.start] {
            return None;
        }
        if !slots.contains(&i) && POOL_FIXTURE[fr.clone()] != tpl[cr.clone()] {
            return None;
        }
        fpos = fr.end;
        cpos = cr.end;
    }
    if POOL_FIXTURE[fpos..] != tpl[cpos..] {
        return None;
    }
    let creators: BTreeSet<&Vec<u8>> =
        POOL_TEMPLATE_CREATOR_SLOTS.iter().map(|&i| &cand_units[i].1).collect();
    let [creator] = *creators.into_iter().collect::<Vec<_>>().as_slice() else { return None };
    Some(PoolParams {
        token_covenant_id,
        kas_reserve_units,
        token_reserve,
        shares,
        lp_token_covenant_id,
        creator: creator.as_slice().try_into().ok()?,
    })
}

/// D6: the executed price must lie between the marginal prices the program
/// computes before and after the trade, with one KAS quantum of slack. This
/// is the anti-donation gate: a same-tx giveback shrinks `base_amount` while
/// the reserve delta stays, and the resulting "price" lands orders of
/// magnitude outside the bracket.
pub fn bracket_holds(
    v_sompi: i128,
    k0: i128,
    k1: i128,
    b0: i128,
    b1: i128,
    quote_sompi: i128,
    base_amount: i128,
    is_buy: bool,
    fee_in_bps: i128,
) -> bool {
    if base_amount <= 0 || quote_sompi <= 0 || b0 <= 0 || b1 <= 0 {
        return false;
    }
    // One KAS quantum for the program's own quantisation, plus the fee the
    // build declares it keeps INSIDE the reserve (an AMM pool's LP fee): that
    // fee moves the executed price off the pure marginal by a bounded, known
    // amount. Zero for the curve, whose price is exact.
    let q = KAS_QUANTUM_SOMPI + quote_sompi * fee_in_bps / 10_000;
    let lo_num = if is_buy { v_sompi + k0 - q } else { v_sompi + k1 - q };
    let lo_den = if is_buy { b0 } else { b1 };
    let hi_num = if is_buy { v_sompi + k1 + q } else { v_sompi + k0 + q };
    let hi_den = if is_buy { b1 } else { b0 };
    let (Some(lo), Some(px), Some(hi)) = (
        lo_num.checked_mul(base_amount),
        quote_sompi.checked_mul(lo_den),
        hi_num.checked_mul(base_amount),
    ) else {
        return false;
    };
    // price >= lower marginal:  lo_num*base <= quote*lo_den
    if lo > px {
        return false;
    }
    let Some(px_hi) = quote_sompi.checked_mul(hi_den) else { return false };
    // price <= upper marginal:  quote*hi_den <= hi_num*base
    px_hi <= hi
}

/// D7: two-sided constant-product replay. `k` may only grow, and only within
/// [`INVARIANT_MAX_GROWTH_PPM`] — the upper bound is the in-covenant-fee
/// detector: a fee routed into the reserve grows `k` orders of magnitude
/// faster and refuses the parameters outright.
pub fn invariant_holds(
    v_sompi: i128,
    k0: i128,
    k1: i128,
    b0: i128,
    b1: i128,
    quote_sompi: i128,
    fee_in_bps: i128,
) -> bool {
    let (Some(before), Some(after)) =
        ((v_sompi + k0).checked_mul(b0), (v_sompi + k1).checked_mul(b1))
    else {
        return false;
    };
    if after < before {
        return false;
    }
    let Some(growth) = after.checked_sub(before) else { return false };
    if fee_in_bps == 0 {
        // the curve: k is constant to within quantisation — 10 ppm is 300x
        // the measured residual and orders below any hidden fee
        let (Some(g), Some(cap)) =
            (growth.checked_mul(1_000_000), before.checked_mul(INVARIANT_MAX_GROWTH_PPM))
        else {
            return false;
        };
        return g <= cap;
    }
    // the pool: k grows by exactly the LP fee held in-reserve, so the cap is
    // that fee (plus a quantum) scaled by the larger token side — dk from a
    // KAS-side deposit f is f x B
    let fee = quote_sompi * fee_in_bps / 10_000 + KAS_QUANTUM_SOMPI;
    let Some(cap) = fee.checked_mul(b0.max(b1)) else { return false };
    growth <= cap
}

/// Verify one market covenant: find a blake2b-proven reveal of its program,
/// match the pinned skeleton, and replay every bracket-passing trade of the
/// token it NAMES against its own constants. Writes one `market_programs`
/// row; the skip gate makes the steady state one hash comparison.
pub(crate) fn derive_market_program(conn: &Connection, covenant_id: &[u8; 32]) -> Result<()> {
    // newest proof-grade reveal of this covenant's program
    let mut stmt = conn
        .prepare_cached(
            "SELECT spk_script, spent_sig FROM covenant_utxos
             WHERE covenant_id = ?1 AND spent_sig IS NOT NULL
             ORDER BY created_daa DESC LIMIT 8",
        )
        .map_err(db_err)?;
    let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
        .query_map([covenant_id.as_slice()], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_err)?;
    let mut revealed: Option<Vec<u8>> = None;
    for (spk, sig) in &rows {
        if let Some(program) = kascov_decode::p2sh_reveal(spk, sig) {
            revealed = Some(program);
            break;
        }
    }
    let Some(program) = revealed else { return Ok(()) }; // nothing proof-grade yet
    let program_hash = kascov_decode::kcc20::blake2b_256(&program);

    // Skip gate: same program, already judged — nothing to redo but the
    // incremental replay below.
    let known: Option<(Vec<u8>, String, i64)> = conn
        .query_row(
            "SELECT program_hash, skeleton, v_kas_units FROM market_programs WHERE covenant_id = ?1",
            [covenant_id.as_slice()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(db_err)?;
    let unmatched_tag = unmatched_tag();
    let params_known = match &known {
        Some((h, skel, v)) if h.as_slice() == program_hash && skel == "KRON curve v1" => Some(*v),
        Some((h, skel, _)) if h.as_slice() == program_hash && *skel == unmatched_tag => {
            return Ok(()); // this matcher already gave up on these exact bytes
        }
        _ => None,
    };

    // Unify the two builds behind one shape: the pool is the same formula
    // with no virtual reserve (V = 0), and it additionally names its LP token.
    struct Matched {
        skeleton: &'static str,
        token_covenant_id: [u8; 32],
        v_kas_units: i64,
        token_reserve: i64,
        graduation_kas_sompi: Option<i64>,
        creator: [u8; 32],
        kas_reserve_sompi: Option<i64>,
        lp_token_covenant_id: Option<[u8; 32]>,
        shares: Option<i64>,
    }
    let _ = params_known; // the re-match below is cheap either way
    let matched: Option<Matched> = match_kron_curve(&program)
        .map(|c| Matched {
            skeleton: "KRON curve v1",
            token_covenant_id: c.token_covenant_id,
            v_kas_units: c.v_kas_units,
            token_reserve: c.token_reserve,
            graduation_kas_sompi: Some(c.graduation_kas_sompi),
            creator: c.creator_fee_owner,
            kas_reserve_sompi: None,
            lp_token_covenant_id: None,
            shares: None,
        })
        .or_else(|| {
            match_kron_pool(&program).map(|p| Matched {
                skeleton: "KRON pool v1",
                token_covenant_id: p.token_covenant_id,
                v_kas_units: 0,
                token_reserve: p.token_reserve,
                graduation_kas_sompi: None,
                creator: p.creator,
                kas_reserve_sompi: p.kas_reserve_units.checked_mul(1_000_000),
                lp_token_covenant_id: Some(p.lp_token_covenant_id),
                shares: Some(p.shares),
            })
        });
    let Some(p) = matched else {
        conn.execute(
            "INSERT OR REPLACE INTO market_programs (covenant_id, program_hash, skeleton)
             VALUES (?1, ?2, ?3)",
            params![covenant_id.as_slice(), program_hash.as_slice(), unmatched_tag],
        )
        .map_err(db_err)?;
        tracing::warn!(
            "market covenant {} runs an unrecognised program ({} bytes) — its tokens get no price",
            hex::encode(covenant_id),
            program.len()
        );
        return Ok(());
    };

    // Full replay of the named token's admitted trades against the program's
    // own formula. Cheap at observed scale (thousands of rows, integer math);
    // incrementality can come with the first covenant that needs it.
    let v = p.v_kas_units as i128 * KAS_QUANTUM_SOMPI;
    // What the build keeps inside its reserve per trade: nothing on the
    // curve, the 20 bps LP fee on the pool (a constant of the pinned
    // template, identical across every deployment).
    let fee_in_bps: i128 = if p.skeleton == "KRON pool v1" { 20 } else { 0 };
    let mut trades = conn
        .prepare_cached(
            "SELECT seq, side, base_amount, quote_sompi, kas_before_sompi, kas_after_sompi,
                    base_before, base_after, co_covenants
             FROM token_trades WHERE token_id = ?1 AND market_covenant_id = ?2 ORDER BY seq",
        )
        .map_err(db_err)?;
    let mut exercised: i64 = 0;
    let mut ok = true;
    let mut checked_through: i64 = -1;
    let rows = trades
        .query_map(params![p.token_covenant_id.as_slice(), covenant_id.as_slice()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
            ))
        })
        .map_err(db_err)?;
    for row in rows {
        let (seq, side, base, quote, k0, k1, b0, b1, co) = row.map_err(db_err)?;
        checked_through = seq;
        if co > 0 {
            continue; // stored, never judged: another covenant moved too
        }
        let (k0, k1, b0, b1) = (k0 as i128, k1 as i128, b0 as i128, b1 as i128);
        if !bracket_holds(v, k0, k1, b0, b1, quote as i128, base as i128, side == "buy", fee_in_bps)
        {
            continue; // off-curve: never priced, never counted
        }
        if !invariant_holds(v, k0, k1, b0, b1, quote as i128, fee_in_bps) {
            ok = false; // one violation poisons the whole program's figures
            break;
        }
        exercised += 1;
    }

    conn.execute(
        "INSERT OR REPLACE INTO market_programs (covenant_id, program_hash, skeleton,
             v_kas_units, token_reserve, token_covenant_id, graduation_kas_sompi,
             fee_owners_json, kas_reserve_sompi, lp_token_covenant_id, shares,
             invariant_checked_through_seq, invariant_trades,
             invariant_ok, exercised_trades)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            covenant_id.as_slice(),
            program_hash.as_slice(),
            p.skeleton,
            p.v_kas_units,
            p.token_reserve,
            p.token_covenant_id.as_slice(),
            p.graduation_kas_sompi,
            serde_json::json!({ "creator_fee_owner": hex::encode(p.creator) }).to_string(),
            p.kas_reserve_sompi,
            p.lp_token_covenant_id.as_ref().map(|l| l.as_slice().to_vec()),
            p.shares,
            checked_through,
            exercised,
            ok as i64,
            exercised,
        ],
    )
    .map_err(db_err)?;
    Ok(())
}

/// Re-verify every market covenant in the set (the distinct
/// `market_covenant_id`s the derivation just linked).
pub(crate) fn rederive_market_programs(
    conn: &Connection,
    covenants: &BTreeSet<[u8; 32]>,
) -> Result<()> {
    for c in covenants {
        derive_market_program(conn, c)?;
    }
    Ok(())
}

/// A trade below this resolution never sets a price: the curve moves KAS in
/// whole quanta, so a dust trade's price is mostly quantisation error. It
/// still counts in volume — the KAS it moved is exact.
pub const MIN_PRICEABLE_QUOTE_SOMPI: i64 = 100_000_000; // 1 KAS -> <=100 bps error
const WINDOW_SPAN_MS: i64 = 86_400_000;

/// The market summary the API publishes for one token: every figure a gated
/// chain derivation, every gap an explicit reason. There is deliberately no
/// market cap and no FDV anywhere in this struct — multiplying a marginal
/// price by supply overstates what could actually be taken out (13x on the
/// flagship token), and kascov does not publish numbers it can prove wrong.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct MarketSummary {
    /// Where this token's market lives: 'bonding' while its curve sells,
    /// 'graduated' once a pool holds the liquidity, 'lp shares' when the
    /// token IS a pool's share token (never priced).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// Progress toward graduation in basis points, while bonding: the curve's
    /// live reserve against the graduation target its own bytes state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grad_progress_bps: Option<i64>,
    /// Why nothing below is populated, when nothing is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpriced_reason: Option<String>,
    /// Last executed price as the exact integer pair, plus its rendering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_quote_sompi: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_base_amount: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_side: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_daa: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_time_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_24h_sompi: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trades_24h: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_24h_bps: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserve_sompi: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserve_note: Option<String>,
    /// Marginal next-trade price pair (V + reserve, T). Token page only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spot_num_sompi: Option<i128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spot_den: Option<i64>,
    /// What selling all circulating supply into this market would return,
    /// capped by the reserve. Decimal string: the product overflows i64.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_value_sompi: Option<String>,
    /// When this token IS a pool's share token: the pool that names it, and
    /// how its shares split. `locked_shares` is what the pool counts that no
    /// LP token backs — liquidity nobody can ever withdraw. Derived, never
    /// assumed: the pool's own share counter minus the shares actually issued.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lp_of_pool: Option<crate::CovenantId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_shares: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked_shares: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked_bps: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<MarketProgramRow>,
}

/// Compute the gated market summary for one token at serve time. Aggregates
/// are never stored: derive_token stays a pure function of the source tables,
/// and the 24h window moves with the tip.
pub(crate) fn market_summary(
    conn: &Connection,
    token_id: &[u8; 32],
    market_covenant_id: Option<&[u8; 32]>,
    held_covenant: Option<i64>,
    held_wallet: Option<i64>,
    held_script: Option<i64>,
    trades_missing_time: i64,
    tip_at_ms: Option<i64>,
    scan: u64,
) -> Result<MarketSummary> {
    let mut out = MarketSummary::default();
    // A pool's SHARE token is not a token anyone trades against a curve: its
    // "price" would be the pool's net position per share, which is a different
    // instrument. Labelled, never priced.
    let lp_of: Option<[u8; 32]> = conn
        .query_row(
            "SELECT covenant_id FROM market_programs WHERE lp_token_covenant_id = ?1",
            [token_id.as_slice()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_err)?;
    if let Some(pool) = lp_of {
        out.phase = Some("lp shares".into());
        out.unpriced_reason =
            Some("this token is a pool's LP share token; kascov does not price LP shares".into());
        out.lp_of_pool = Some(crate::CovenantId(pool));
        // The pool counts shares its own state block states; the LP token
        // records which of those were actually issued. The difference is
        // liquidity no share can redeem, so it can never leave the pool.
        // Both halves are proven, so the gap is proven too — nothing here is
        // a protocol constant taken on faith.
        if let Some(prog) = market_program_row(conn, &pool)? {
            if let Some(shares) = prog.shares {
                out.pool_shares = Some(shares);
                let issued = held_wallet.unwrap_or(0) + held_script.unwrap_or(0);
                if shares > 0 && issued >= 0 && issued <= shares {
                    let locked = shares - issued;
                    out.locked_shares = Some(locked);
                    out.locked_bps =
                        Some(((locked as i128 * 10_000) / shares as i128) as i64);
                }
            }
            out.program = Some(prog);
        }
        return Ok(out);
    }
    let Some(market) = market_covenant_id else {
        out.unpriced_reason =
            Some("no single covenant holds this token's inventory".into());
        return Ok(out);
    };
    let program = market_program_row(conn, market)?;
    out.program = program.clone();
    let Some(prog) = program else {
        out.unpriced_reason = Some("the market covenant's program is not yet verified".into());
        return Ok(out);
    };
    out.phase = Some(match prog.skeleton.as_str() {
        "KRON curve v1" => "bonding".into(),
        "KRON pool v1" => "graduated".into(),
        _ => "unknown".into(),
    });
    // Allowlist, never a denylist: testing `!= unmatched` would promote a
    // future give-up tag into "priceable", which is match-widening by accident.
    if !MATCHED_SKELETONS.contains(&prog.skeleton.as_str()) {
        out.unpriced_reason = Some(
            "the covenant holding the inventory runs a program kascov does not recognise,              so no exchange rate it produces can be verified"
                .into(),
        );
        return Ok(out);
    }
    if !prog.invariant_ok {
        out.unpriced_reason = Some(
            "a recorded trade violates the program's own formula — nothing it produced is priced"
                .into(),
        );
        return Ok(out);
    }
    if prog.exercised_trades < MIN_EXERCISED_TRADES {
        out.unpriced_reason = Some(format!(
            "only {} trade(s) have exercised this program's constants; kascov prices after {}",
            prog.exercised_trades, MIN_EXERCISED_TRADES
        ));
        return Ok(out);
    }

    let v = prog.v_kas_units as i128 * KAS_QUANTUM_SOMPI;
    let fee_in_bps: i128 = if prog.skeleton == "KRON pool v1" { 20 } else { 0 };
    let trades = crate::tokens::token_trades_page(conn, token_id, scan)?;
    // publishable: same-tx-clean, on this market, bracket-passing
    let publishable: Vec<&crate::tokens::TokenTradeRow> = trades
        .iter()
        .filter(|t| {
            t.co_covenants == 0
                && t.market_covenant_id.0 == *market
                && bracket_holds(
                    v,
                    t.kas_before_sompi as i128,
                    t.kas_after_sompi as i128,
                    t.base_before as i128,
                    t.base_after as i128,
                    t.quote_sompi as i128,
                    t.base_amount as i128,
                    t.side == "buy",
                    fee_in_bps,
                )
        })
        .collect();

    // last price: newest publishable AND resolvable (>= 1 KAS moved)
    if let Some(last) =
        publishable.iter().find(|t| t.quote_sompi >= MIN_PRICEABLE_QUOTE_SOMPI)
    {
        out.last_quote_sompi = Some(last.quote_sompi);
        out.last_base_amount = Some(last.base_amount);
        out.last_side = Some(last.side.clone());
        out.last_daa = Some(last.accepting_daa);
        out.last_time_ms = last.accepting_time_ms;
    } else if out.unpriced_reason.is_none() {
        out.unpriced_reason = Some(format!(
            "no bracket-passing trade of at least 1 KAS in the newest {scan} scanned"
        ));
    }

    // 24h window: fails closed whole when any trade predates timestamps
    if trades_missing_time > 0 {
        out.window_note =
            Some("some of this token's trades predate timestamp capture".into());
    } else if let Some(end) = tip_at_ms {
        let start = end - WINDOW_SPAN_MS;
        let in_window: Vec<&&crate::tokens::TokenTradeRow> = publishable
            .iter()
            .filter(|t| t.accepting_time_ms.is_some_and(|ms| ms >= start && ms <= end))
            .collect();
        if !in_window.is_empty() {
            let vol: i128 = in_window.iter().map(|t| t.quote_sompi as i128).sum();
            out.volume_24h_sompi = i64::try_from(vol).ok();
            out.trades_24h = Some(in_window.len() as i64);
        }
        // 24h change: newest resolvable vs newest resolvable before the window
        let newest = publishable.iter().find(|t| t.quote_sompi >= MIN_PRICEABLE_QUOTE_SOMPI);
        let reference = publishable.iter().find(|t| {
            t.quote_sompi >= MIN_PRICEABLE_QUOTE_SOMPI
                && t.accepting_time_ms.is_some_and(|ms| ms < start)
        });
        if let (Some(n), Some(r)) = (newest, reference) {
            let (ql, bl) = (n.quote_sompi as i128, n.base_amount as i128);
            let (qr, br) = (r.quote_sompi as i128, r.base_amount as i128);
            out.change_24h_bps = (|| {
                let num = ql.checked_mul(br)?.checked_sub(qr.checked_mul(bl)?)?;
                let bps = num.checked_mul(10_000)? / qr.checked_mul(bl)?;
                i64::try_from(bps).ok()
            })();
        }
    }

    // reserve: exactly one live cell, attributed by the program NAMING this token
    let (cells, value): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(value), 0) FROM covenant_utxos
             WHERE covenant_id = ?1 AND spent_block IS NULL",
            [market.as_slice()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(db_err)?;
    let names_this_token: bool = conn
        .query_row(
            "SELECT token_covenant_id FROM market_programs WHERE covenant_id = ?1",
            [market.as_slice()],
            |r| r.get::<_, Option<[u8; 32]>>(0),
        )
        .optional()
        .map_err(db_err)?
        .flatten()
        .is_some_and(|t| t == *token_id);
    if cells == 1 && names_this_token {
        out.reserve_sompi = Some(value);
        if let (Some(grad), true) = (prog.graduation_kas_sompi, prog.skeleton == "KRON curve v1") {
            if grad > 0 {
                out.grad_progress_bps =
                    (value as i128).checked_mul(10_000).map(|n| (n / grad as i128) as i64);
            }
        }
    } else if cells != 1 {
        out.reserve_note = Some("the market covenant's KAS sits in more than one cell".into());
    } else {
        out.reserve_note =
            Some("the market program names a different token; its KAS is not attributable".into());
    }

    // spot + exit value: need reserve, and the program's live token reserve
    // to equal what the covenant actually holds — a donated cell reopens the
    // gap and kascov will not value the difference.
    // the live token reserve is the newest trade's after-balance
    let t_live = publishable.first().map(|t| t.base_after);
    if let (Some(reserve), Some(t_live), Some(hc)) = (out.reserve_sompi, t_live, held_covenant) {
        if t_live == hc && t_live > 0 {
            out.spot_num_sompi = Some(v + reserve as i128);
            out.spot_den = Some(t_live);
            if let (Some(hw), Some(hs)) = (held_wallet, held_script) {
                let circulating = hw as i128 + hs as i128;
                let exit = (|| {
                    let num = (v + reserve as i128).checked_mul(circulating)?;
                    let den = (t_live as i128).checked_add(circulating)?;
                    Some((num / den).min(reserve as i128))
                })();
                out.exit_value_sompi = exit.map(|e| e.to_string());
            }
        }
    }
    Ok(out)
}

/// The published verification row for one market covenant, as the API and the
/// price gates consume it.
#[derive(Clone, Debug, serde::Serialize)]
pub struct MarketProgramRow {
    pub covenant_id: crate::CovenantId,
    /// blake2b-256 of the revealed program — the same digest the chain's own
    /// P2SH commitment carries, republished so anyone can recompute it from
    /// any spending transaction and compare.
    pub program_hash: String,
    pub skeleton: String,
    pub v_kas_units: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_reserve: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graduation_kas_sompi: Option<i64>,
    pub invariant_ok: bool,
    pub exercised_trades: i64,
    /// Pool builds only: the rest of the state block this program committed.
    /// These are the figures AS OF the newest proven reveal, which is one
    /// spend behind the live cell — a pool page must say so rather than call
    /// them current.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shares: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kas_reserve_sompi: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_covenant_id: Option<crate::CovenantId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lp_token_covenant_id: Option<crate::CovenantId>,
}

pub(crate) fn market_program_row(
    conn: &Connection,
    covenant_id: &[u8; 32],
) -> Result<Option<MarketProgramRow>> {
    conn.query_row(
        // New columns append at the end: these r.get indices are positional,
        // so inserting mid-list silently mis-maps every field after it.
        "SELECT covenant_id, program_hash, skeleton, v_kas_units, token_reserve,
                graduation_kas_sompi, invariant_ok, exercised_trades,
                shares, kas_reserve_sompi, token_covenant_id, lp_token_covenant_id
         FROM market_programs WHERE covenant_id = ?1",
        [covenant_id.as_slice()],
        |r| {
            let cid = |v: Option<Vec<u8>>| -> Option<crate::CovenantId> {
                v.and_then(|b| b.as_slice().try_into().ok()).map(crate::CovenantId)
            };
            Ok(MarketProgramRow {
                covenant_id: crate::CovenantId(r.get(0)?),
                program_hash: hex::encode(r.get::<_, Vec<u8>>(1)?),
                skeleton: r.get(2)?,
                v_kas_units: r.get(3)?,
                token_reserve: r.get(4)?,
                graduation_kas_sompi: r.get(5)?,
                invariant_ok: r.get::<_, i64>(6)? == 1,
                exercised_trades: r.get(7)?,
                shares: r.get(8)?,
                kas_reserve_sompi: r.get(9)?,
                token_covenant_id: cid(r.get(10)?),
                lp_token_covenant_id: cid(r.get(11)?),
            })
        },
    )
    .optional()
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The retry promise the 'unmatched:N' tag makes is only kept if bumping
    /// the matcher also invalidates the market gate. While MATCHER_VERSION was
    /// a local const inside the matcher, a bump re-tagged nothing: the gate in
    /// derive_tokens_if_stale never noticed and returned early, so every stored
    /// unmatched row kept its old verdict and the newly-taught build stayed
    /// invisible. Pin the relationship so that cannot regress.
    #[test]
    fn the_market_gate_moves_when_the_matcher_does() {
        assert!(
            market_stamp().ends_with(&format!("/{MATCHER_VERSION}")),
            "market_stamp must carry MATCHER_VERSION or a matcher bump re-verifies nothing"
        );
        assert!(unmatched_tag().ends_with(MATCHER_VERSION));
    }

    /// A give-up tag must never be mistaken for a match. This is the exact
    /// shape a denylist gets wrong.
    #[test]
    fn a_give_up_tag_is_not_a_match() {
        assert!(!MATCHED_SKELETONS.contains(&unmatched_tag().as_str()));
        assert!(!MATCHED_SKELETONS.contains(&"unmatched:3"));
        assert!(MATCHED_SKELETONS.contains(&"KRON curve v1"));
        assert!(MATCHED_SKELETONS.contains(&"KRON pool v1"));
    }

    #[test]
    fn the_fixture_matches_itself_and_reads_its_own_constants() {
        let p = match_kron_curve(CURVE_FIXTURE).expect("the fixture IS the build");
        // the fixture is PUBIC's curve, whose published constants are known
        assert_eq!(p.v_kas_units, 6_187_625);
        assert_eq!(p.graduation_kas_sompi, 24_750_500_000_000);
        assert_eq!(
            hex::encode(p.token_covenant_id),
            "caa73fcc3081b1477f14ea03e45af81e21b1371dde7b2c45871eae0384e52bce"
        );
        assert_eq!(
            hex::encode(p.creator_fee_owner),
            "af84fec297a9650ad6d6bafcf2c8bf33a75b2cde7f42dae77746164fba57144e"
        );
    }

    #[test]
    fn one_flipped_byte_outside_the_slots_is_a_different_program() {
        // flip one data byte of a NON-slot push (the program's midpoint sits
        // inside the 57 KB pool-template slot, where a flip is legal)
        let units = push_units(CURVE_FIXTURE);
        let target = (0..CURVE_PUSHES)
            .find(|i| !SLOT_INDICES.contains(i) && !units[*i].1.is_empty())
            .expect("a non-slot data push exists");
        let mut evil = CURVE_FIXTURE.to_vec();
        let at = units[target].0.end - 1;
        evil[at] ^= 0x01;
        assert!(match_kron_curve(&evil).is_none(), "almost the build is not the build");
        assert!(match_kron_curve(b"\x51\x52\x53").is_none());
        assert!(match_kron_curve(&[]).is_none());
    }

    #[test]
    fn the_pool_build_matches_and_reads_its_state() {
        // assemble a pool program: guard + five state pushes + the template
        let mut prog = vec![0x6bu8];
        let push = |out: &mut Vec<u8>, data: &[u8]| {
            out.push(data.len() as u8);
            out.extend_from_slice(data);
        };
        push(&mut prog, &67_924_883i64.to_le_bytes()); // kasReserve (0.01 KAS units)
        push(&mut prog, &31_255_037i64.to_le_bytes()); // tokenReserve
        push(&mut prog, &[0x7a; 32]); // token covenant
        push(&mut prog, &1_057_766i64.to_le_bytes()); // shares
        push(&mut prog, &[0x18; 32]); // lp share token
        assert_eq!(prog.len(), POOL_STATE_LEN);
        prog.extend_from_slice(POOL_FIXTURE);
        let p = match_kron_pool(&prog).expect("state + pinned template must match");
        assert_eq!(p.kas_reserve_units, 67_924_883);
        assert_eq!(p.token_reserve, 31_255_037);
        assert_eq!(p.token_covenant_id, [0x7a; 32]);
        assert_eq!(p.lp_token_covenant_id, [0x18; 32]);
        assert_eq!(p.shares, 1_057_766);
        // a flipped template byte outside the creator slots is a different build
        let mut evil = prog.clone();
        let units = push_units(POOL_FIXTURE);
        let t = (0..units.len())
            .find(|i| !POOL_TEMPLATE_CREATOR_SLOTS.contains(i) && !units[*i].1.is_empty())
            .unwrap();
        evil[POOL_STATE_LEN + units[t].0.end - 1] ^= 0x01;
        assert!(match_kron_pool(&evil).is_none());
    }

    /// The donation attack the bracket exists for: buy 1,000, give 999 back
    /// in the same tx. The reserve delta says ~6.19 KAS for a net 1 token —
    /// three orders of magnitude off the curve's marginal price.
    #[test]
    fn the_bracket_rejects_the_donation_shape_and_passes_real_trades() {
        // an EXACT constant-product buy: (V+K)·B = 1e19 before and after.
        // V+K0 = 1e12, B0 = 1e7; buyer pays 2,500 KAS, receives 2M tokens:
        // V+K1 = 1.25e12, B1 = 8e6, product unchanged.
        let v: i128 = 900_000_000_000;
        let k0: i128 = 100_000_000_000;
        let quote: i128 = 250_000_000_000;
        let k1 = k0 + quote;
        let (b0, b1): (i128, i128) = (10_000_000, 8_000_000);
        assert!(bracket_holds(v, k0, k1, b0, b1, quote, 2_000_000, true, 0));
        // the same KAS against a NET one token: the donation. The bracket's
        // upper bound is the after-marginal price, and this lands orders of
        // magnitude above it.
        assert!(
            !bracket_holds(v, k0, k1, b0, b0 - 1, quote, 1, true, 0),
            "a donated giveback never prices the trade"
        );
        // the invariant is two-sided: k held exactly passes, k shrinking and
        // k growing past the fee threshold both refuse
        assert!(invariant_holds(v, k0, k1, b0, b1, quote, 0));
        assert!(!invariant_holds(v, k0, k0 - quote, b0, b1, quote, 0), "k may not shrink");
        assert!(
            !invariant_holds(v, k0, k1 + k1 / 100, b0, b1, quote, 0),
            "an in-covenant fee grows k past the cap and refuses the parameters"
        );
    }
}
