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
const IDX_VKAS: [usize; 12] = [
    185, 187, 311, 313, 457, 459, 579, 582, 1237, 1239, 2084, 2087,
];
const IDX_GRADUATION: usize = 724;

/// The second-generation KRON curve build (the "vesting" generation: it pins
/// an approved vesting-lock template and adds an optional partner-fee branch).
/// Pinned from SEVEN mainnet deployments, each recovered from its own spend
/// and blake2b-proven against its P2SH commitment: 2,645 pushes each, opcode
/// gaps byte-identical across all seven, divergent at exactly these 33 push
/// positions — and every one of the 33 has a meaning, none is a mystery slot.
const CURVE2_FIXTURE: &[u8] = include_bytes!("../../kascov-decode/fixtures/kron_curve_v2.bin");
const CURVE2_PUSHES: usize = 2645;
const CURVE2_SLOTS: [usize; 33] = [
    1, 2, 25, 88, 148, 150, 192, 194, 320, 322, 335, 337, 400, 466, 468, 590, 593, 606, 608, 676,
    735, 870, 872, 890, 891, 906, 1033, 1035, 1899, 1902, 1916, 1918, 2603,
];
const IDX2_TOKEN_COVENANT: usize = 1;
const IDX2_TOKEN_RESERVE: usize = 2;
const IDX2_CREATOR: [usize; 5] = [25, 88, 400, 676, 2603];
const IDX2_VKAS: [usize; 12] = [
    192, 194, 320, 322, 466, 468, 590, 593, 1033, 1035, 1899, 1902,
];
const IDX2_GRADUATION: usize = 735;
/// v2 pushes its OWN byte length at ten positions (the covenant re-commits to
/// its continuation). They vary between deployments only because slot widths
/// vary, so they are slots — but within one program every one of them must
/// equal the actual length. A free lie detector v1 never had.
const IDX2_SELF_LEN: [usize; 10] = [148, 150, 335, 337, 606, 608, 870, 872, 1916, 1918];
/// The curve embeds the pool template it graduates into, twice (59,032 bytes
/// each), plus a 32-byte commitment to it. Per-deployment (the template bakes
/// the deployment's creator in), so they are slots — but the two copies must
/// be byte-identical or the program would graduate into something other than
/// what it displays.
const IDX2_POOL_TPL: [usize; 2] = [890, 906];

/// The third-generation KRON curve build: v2 plus the value-continuation fix
/// KRON announced on 2026-08-04. Three copies of a nine-byte check ending in
/// GREATERTHANOREQUAL VERIFY (the continuation output's value must cover the
/// input's) are inserted into the v2 opcode stream — each contributes three
/// small-constant pushes, so v2's 2,645 pushes become 2,654 and every v2 slot
/// keeps its role at a shifted position. Pinned from THREE mainnet
/// deployments, each recovered from its own spend and blake2b-proven against
/// its P2SH commitment: opcode gaps byte-identical across all three,
/// divergent at exactly the same 33 roles as v2.
const CURVE3_FIXTURE: &[u8] = include_bytes!("../../kascov-decode/fixtures/kron_curve_v3.bin");
const CURVE3_PUSHES: usize = 2654;
const CURVE3_SLOTS: [usize; 33] = [
    1, 2, 25, 88, 148, 150, 192, 194, 320, 322, 335, 337, 403, 469, 471, 593, 596, 609, 611, 682,
    741, 876, 878, 896, 897, 912, 1042, 1044, 1908, 1911, 1925, 1927, 2612,
];
const IDX3_TOKEN_COVENANT: usize = 1;
const IDX3_TOKEN_RESERVE: usize = 2;
const IDX3_CREATOR: [usize; 5] = [25, 88, 403, 682, 2612];
const IDX3_VKAS: [usize; 12] = [
    192, 194, 320, 322, 469, 471, 593, 596, 1042, 1044, 1908, 1911,
];
const IDX3_GRADUATION: usize = 741;
/// The three specimens happen to share one byte length, so these ten never
/// show up as cross-instance diffs — they are the pushes whose value IS the
/// program's length, same ten-position structure as v2 shifted by the
/// insertions, and the matcher still demands they tell the truth.
const IDX3_SELF_LEN: [usize; 10] = [148, 150, 335, 337, 609, 611, 876, 878, 1925, 1927];
const IDX3_POOL_TPL: [usize; 2] = [896, 912];

/// The curves' optional fee branch, as a growth cap in bps of the quote.
///
/// Evidence, three independent lines agreeing: (1) BOTH curve generations
/// carry a `25 / 2000` fraction cluster (= 1.25%) in their bytecode — v1 at
/// pushes 158/430/704/1210, v2 across its five entry branches — at positions
/// byte-identical in every deployment; (2) the two v2 trades whose reserve
/// product grew beyond quantisation imply a tokens-side toll of 1.20-1.22%,
/// just under 1.25% exactly where constant-product rounding puts a 125 bps
/// fee; (3) the first v1 trade to exercise the branch (2026-08-01) implies
/// 123 bps — same shape, older build. The branch predates its first use: it
/// sat unexercised through 4,305 v1 trades, which is why the old 10 ppm
/// detector only tripped now. Trades that skip the branch still replay at
/// growth ~0, well inside this cap; extraction (k shrinking) stays a hard
/// failure regardless.
const CURVE_FEE_GROWTH_BPS: i128 = 125;

/// Per-skeleton fee model: (bracket_fee_bps, growth_fee_bps). The first is a
/// fee the trader pays on the quote, so the price bracket slacks for it; the
/// second is what the build may keep INSIDE its reserve per trade, so the
/// invariant's growth cap allows it. They differ on the v2 curve: its optional
/// partner-fee branch withholds tokens (the reserve product grows) without
/// moving the executed price beyond ordinary slack.
fn fee_model(skeleton: &str) -> (i128, i128) {
    match skeleton {
        "KRON pool v1" | "KRON pool v2" | "KRON pool v3" | "KRON pool tn-a" => (20, 20),
        "KRON curve v1" | "KRON curve v2" | "KRON curve v3" => (0, CURVE_FEE_GROWTH_BPS),
        _ => (0, 0),
    }
}

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
            0x4d if i + 2 < n => (
                i + 3,
                u16::from_le_bytes([program[i + 1], program[i + 2]]) as usize,
            ),
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
        out.push((
            i..data_start + len,
            program[data_start..data_start + len].to_vec(),
        ));
        i = data_start + len;
    }
    out
}

/// The bench diffs raw programs with the exact same push semantics the
/// matchers use — a different parser would derive slots the matcher then
/// disagrees with.
pub(crate) fn push_units_of(program: &[u8]) -> Vec<(std::ops::Range<usize>, Vec<u8>)> {
    push_units(program)
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
        if !slots.contains(&i)
            && (fdata != cdata || CURVE_FIXTURE[fr.clone()] != program[cr.clone()])
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
    let vkas_vals: BTreeSet<Option<i64>> = IDX_VKAS.iter().map(|&i| le_i64(&cand[i].1)).collect();
    let [Some(v_kas_units)] = *vkas_vals.into_iter().collect::<Vec<_>>().as_slice() else {
        return None;
    };
    let creators: BTreeSet<&Vec<u8>> = IDX_CREATOR.iter().map(|&i| &cand[i].1).collect();
    let [creator] = *creators.into_iter().collect::<Vec<_>>().as_slice() else {
        return None;
    };
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

/// Match a revealed program against the pinned v2 curve build. Same lockstep
/// walk as v1, plus the internal consistency v2's own structure demands: the
/// ten self-length pushes must state the program's real byte length, and the
/// two embedded pool templates must be identical copies.
pub fn match_kron_curve_v2(program: &[u8]) -> Option<CurveParams> {
    let cand = push_units(program);
    if cand.len() != CURVE2_PUSHES {
        return None;
    }
    let fixture = push_units(CURVE2_FIXTURE);
    debug_assert_eq!(fixture.len(), CURVE2_PUSHES);
    let slots: BTreeSet<usize> = CURVE2_SLOTS.into_iter().collect();

    let mut fpos = 0usize;
    let mut cpos = 0usize;
    for i in 0..CURVE2_PUSHES {
        let (fr, fdata) = &fixture[i];
        let (cr, cdata) = &cand[i];
        if CURVE2_FIXTURE[fpos..fr.start] != program[cpos..cr.start] {
            return None;
        }
        if !slots.contains(&i)
            && (fdata != cdata || CURVE2_FIXTURE[fr.clone()] != program[cr.clone()])
        {
            return None;
        }
        fpos = fr.end;
        cpos = cr.end;
    }
    if CURVE2_FIXTURE[fpos..] != program[cpos..] {
        return None;
    }

    // Internal consistency. Repeated slots must agree with themselves, the
    // self-length pushes must tell the truth, and the two embedded pool
    // templates must be the same bytes.
    let vkas_vals: BTreeSet<Option<i64>> = IDX2_VKAS.iter().map(|&i| le_i64(&cand[i].1)).collect();
    let [Some(v_kas_units)] = *vkas_vals.into_iter().collect::<Vec<_>>().as_slice() else {
        return None;
    };
    let creators: BTreeSet<&Vec<u8>> = IDX2_CREATOR.iter().map(|&i| &cand[i].1).collect();
    let [creator] = *creators.into_iter().collect::<Vec<_>>().as_slice() else {
        return None;
    };
    let self_lens: BTreeSet<Option<i64>> =
        IDX2_SELF_LEN.iter().map(|&i| le_i64(&cand[i].1)).collect();
    if self_lens != BTreeSet::from([Some(program.len() as i64)]) {
        return None;
    }
    if cand[IDX2_POOL_TPL[0]].1 != cand[IDX2_POOL_TPL[1]].1 {
        return None;
    }
    let creator_fee_owner: [u8; 32] = creator.as_slice().try_into().ok()?;
    let token_covenant_id: [u8; 32] = cand[IDX2_TOKEN_COVENANT].1.as_slice().try_into().ok()?;
    Some(CurveParams {
        token_covenant_id,
        v_kas_units,
        graduation_kas_sompi: le_i64(&cand[IDX2_GRADUATION].1)?,
        creator_fee_owner,
        token_reserve: le_i64(&cand[IDX2_TOKEN_RESERVE].1)?,
    })
}

/// Match a revealed program against the pinned v3 curve build. Identical
/// discipline to v2: lockstep byte walk outside the slots, repeated slots must
/// agree with themselves, the ten self-length pushes must state the program's
/// real byte length, and the two embedded pool templates must be identical.
pub fn match_kron_curve_v3(program: &[u8]) -> Option<CurveParams> {
    let cand = push_units(program);
    if cand.len() != CURVE3_PUSHES {
        return None;
    }
    let fixture = push_units(CURVE3_FIXTURE);
    debug_assert_eq!(fixture.len(), CURVE3_PUSHES);
    let slots: BTreeSet<usize> = CURVE3_SLOTS.into_iter().collect();

    let mut fpos = 0usize;
    let mut cpos = 0usize;
    for i in 0..CURVE3_PUSHES {
        let (fr, fdata) = &fixture[i];
        let (cr, cdata) = &cand[i];
        if CURVE3_FIXTURE[fpos..fr.start] != program[cpos..cr.start] {
            return None;
        }
        if !slots.contains(&i)
            && (fdata != cdata || CURVE3_FIXTURE[fr.clone()] != program[cr.clone()])
        {
            return None;
        }
        fpos = fr.end;
        cpos = cr.end;
    }
    if CURVE3_FIXTURE[fpos..] != program[cpos..] {
        return None;
    }

    let vkas_vals: BTreeSet<Option<i64>> = IDX3_VKAS.iter().map(|&i| le_i64(&cand[i].1)).collect();
    let [Some(v_kas_units)] = *vkas_vals.into_iter().collect::<Vec<_>>().as_slice() else {
        return None;
    };
    let creators: BTreeSet<&Vec<u8>> = IDX3_CREATOR.iter().map(|&i| &cand[i].1).collect();
    let [creator] = *creators.into_iter().collect::<Vec<_>>().as_slice() else {
        return None;
    };
    let self_lens: BTreeSet<Option<i64>> =
        IDX3_SELF_LEN.iter().map(|&i| le_i64(&cand[i].1)).collect();
    if self_lens != BTreeSet::from([Some(program.len() as i64)]) {
        return None;
    }
    if cand[IDX3_POOL_TPL[0]].1 != cand[IDX3_POOL_TPL[1]].1 {
        return None;
    }
    let creator_fee_owner: [u8; 32] = creator.as_slice().try_into().ok()?;
    let token_covenant_id: [u8; 32] = cand[IDX3_TOKEN_COVENANT].1.as_slice().try_into().ok()?;
    Some(CurveParams {
        token_covenant_id,
        v_kas_units,
        graduation_kas_sompi: le_i64(&cand[IDX3_GRADUATION].1)?,
        creator_fee_owner,
        token_reserve: le_i64(&cand[IDX3_TOKEN_RESERVE].1)?,
    })
}

/// The pool build the curve graduates into: a 94-byte state block (guard,
/// kasReserve, tokenReserve, tokenCovenantId, shares, lpTokenCovenantId) in
/// front of the same 57,475-byte template every curve embeds. The template is
/// pinned from the copy inside a blake2b-proven curve program; only the two
/// creator slots vary per deployment, and only the two reserves vary per
/// state.
const POOL_FIXTURE: &[u8] = include_bytes!("../../kascov-decode/fixtures/kron_pool_v1.bin");
/// The v2 pool: same design, new build — 59,032-byte template (pinned from
/// the copy inside a blake2b-proven v2 curve, byte-identical to the deployed
/// pool that graduated on mainnet), creator slots at [256, 546], and the same
/// 94-byte state block in front. The 20 bps LP fee constant sits at the same
/// structural position as v1's (the 10000·20·10000 arithmetic cluster) and
/// held on all 580 replayed trades of the first graduated market.
const POOL2_FIXTURE: &[u8] = include_bytes!("../../kascov-decode/fixtures/kron_pool_v2.bin");
const POOL2_TEMPLATE_CREATOR_SLOTS: [usize; 2] = [256, 546];
/// The v3 pool: same design once more — a 59,089-byte template carrying the
/// same value-continuation insertion as the v3 curve, pinned from the copy
/// embedded (twice, byte-identically) inside the blake2b-proven v3 curve
/// fixture. Creator slots at [259, 552]; the same 94-byte state block in
/// front. The first v3-graduated mainnet pool matches this template outside
/// those two slots exactly.
const POOL3_FIXTURE: &[u8] = include_bytes!("../../kascov-decode/fixtures/kron_pool_v3.bin");
const POOL3_TEMPLATE_CREATOR_SLOTS: [usize; 2] = [259, 552];
/// The oldest KRON pool build, live only on testnet-10: ten deployments, all
/// recovered from their own spends and hash-proven, byte-identical outside
/// FOUR creator slots. Same 94-byte state block as every generation since,
/// same 20 bps LP fee at the same position in its arithmetic (10000-20-10000),
/// confirmed by replaying all 349 admitted trades with zero invariant breaks.
/// Named plainly: a testnet build gets a tag, not a marketing name.
const POOL_TN_A_FIXTURE: &[u8] =
    include_bytes!("../../kascov-decode/fixtures/kron_pool_tn_a.bin");
const POOL_TN_A_CREATOR_SLOTS: [usize; 4] = [136, 148, 295, 307];
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
pub(crate) const MATCHER_VERSION: &str = "5";

/// The only skeletons that mean "this program byte-matched an audited build".
/// Every tally and every publish gate reads this ALLOWLIST rather than testing
/// `!= unmatched`: a denylist silently promotes a future give-up tag (say
/// `unmatched:3`) into "matched", which is match-widening by accident.
pub(crate) const MATCHED_SKELETONS: [&str; 7] = [
    "KRON curve v1",
    "KRON pool v1",
    "KRON curve v2",
    "KRON pool v2",
    "KRON curve v3",
    "KRON pool v3",
    "KRON pool tn-a",
];

pub(crate) fn unmatched_tag() -> String {
    format!("unmatched:{MATCHER_VERSION}")
}

/// Every pinned skeleton is either a launch curve (phase "bonding", with a
/// graduation target read from its slots) or a pool ("graduated"). A new
/// generation must be added HERE as well as to MATCHED_SKELETONS — the v3 pin
/// missed this map on its first pass and every v3 market served phase
/// "unknown" with no graduation progress; the test that walks the allowlist
/// through this map is what keeps that from happening again.
fn phase_for_skeleton(skeleton: &str) -> Option<&'static str> {
    match skeleton {
        "KRON curve v1" | "KRON curve v2" | "KRON curve v3" => Some("bonding"),
        "KRON pool v1" | "KRON pool v2" | "KRON pool v3" | "KRON pool tn-a" => Some("graduated"),
        _ => None,
    }
}

/// The market verification gate, composite so that either half moving forces
/// every stored market program to be read again.
pub(crate) fn market_stamp() -> String {
    format!("4-logged/{MATCHER_VERSION}")
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
    match_pool_build(program, POOL_FIXTURE, &POOL_TEMPLATE_CREATOR_SLOTS)
}

/// The v2 pool build: identical design, its own fixture and creator slots.
pub fn match_kron_pool_v2(program: &[u8]) -> Option<PoolParams> {
    match_pool_build(program, POOL2_FIXTURE, &POOL2_TEMPLATE_CREATOR_SLOTS)
}

/// The v3 pool build: identical design, its own fixture and creator slots.
pub fn match_kron_pool_v3(program: &[u8]) -> Option<PoolParams> {
    match_pool_build(program, POOL3_FIXTURE, &POOL3_TEMPLATE_CREATOR_SLOTS)
}

/// The oldest pool build, testnet-only: same design, four creator slots.
pub fn match_kron_pool_tn_a(program: &[u8]) -> Option<PoolParams> {
    match_pool_build(program, POOL_TN_A_FIXTURE, &POOL_TN_A_CREATOR_SLOTS)
}

fn match_pool_build(
    program: &[u8],
    pool_fixture: &[u8],
    creator_slots: &[usize],
) -> Option<PoolParams> {
    if program.len() != POOL_STATE_LEN + pool_fixture.len() || program[0] != 0x6b {
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
    let fixture_units = push_units(pool_fixture);
    let cand_units = push_units(tpl);
    if cand_units.len() != fixture_units.len() {
        return None;
    }
    let slots: BTreeSet<usize> = creator_slots.iter().copied().collect();
    let mut fpos = 0usize;
    let mut cpos = 0usize;
    for i in 0..fixture_units.len() {
        let (fr, _) = &fixture_units[i];
        let (cr, _) = &cand_units[i];
        if pool_fixture[fpos..fr.start] != tpl[cpos..cr.start] {
            return None;
        }
        if !slots.contains(&i) && pool_fixture[fr.clone()] != tpl[cr.clone()] {
            return None;
        }
        fpos = fr.end;
        cpos = cr.end;
    }
    if pool_fixture[fpos..] != tpl[cpos..] {
        return None;
    }
    let creators: BTreeSet<&Vec<u8>> = creator_slots
        .iter()
        .map(|&i| &cand_units[i].1)
        .collect();
    let [creator] = *creators.into_iter().collect::<Vec<_>>().as_slice() else {
        return None;
    };
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
    let lo_num = if is_buy {
        v_sompi + k0 - q
    } else {
        v_sompi + k1 - q
    };
    let lo_den = if is_buy { b0 } else { b1 };
    let hi_num = if is_buy {
        v_sompi + k1 + q
    } else {
        v_sompi + k0 + q
    };
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
    let Some(px_hi) = quote_sompi.checked_mul(hi_den) else {
        return false;
    };
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
    let (Some(before), Some(after)) = (
        (v_sompi + k0).checked_mul(b0),
        (v_sompi + k1).checked_mul(b1),
    ) else {
        return false;
    };
    if after < before {
        return false;
    }
    let Some(growth) = after.checked_sub(before) else {
        return false;
    };
    if fee_in_bps == 0 {
        // the curve: k is constant to within quantisation — 10 ppm is 300x
        // the measured residual and orders below any hidden fee
        let (Some(g), Some(cap)) = (
            growth.checked_mul(1_000_000),
            before.checked_mul(INVARIANT_MAX_GROWTH_PPM),
        ) else {
            return false;
        };
        return g <= cap;
    }
    // the pool: k grows by exactly the LP fee held in-reserve, so the cap is
    // that fee (plus a quantum) scaled by the larger token side — dk from a
    // KAS-side deposit f is f x B
    let fee = quote_sompi * fee_in_bps / 10_000 + KAS_QUANTUM_SOMPI;
    let Some(cap) = fee.checked_mul(b0.max(b1)) else {
        return false;
    };
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
    let Some(program) = revealed else {
        return Ok(());
    }; // nothing proof-grade yet
    let program_hash = kascov_decode::kcc20::blake2b_256(&program);

    // Skip gate: same program, already judged — nothing to redo but the
    // incremental replay below.
    let known: Option<(Vec<u8>, String, i64, Option<i64>)> = conn
        .query_row(
            "SELECT program_hash, skeleton, v_kas_units, program_len
             FROM market_programs WHERE covenant_id = ?1",
            [covenant_id.as_slice()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(db_err)?;
    let unmatched_tag = unmatched_tag();
    let params_known = match &known {
        Some((h, skel, v, _)) if h.as_slice() == program_hash && skel == "KRON curve v1" => {
            Some(*v)
        }
        // Give up again on the same bytes ONLY if the stored row is complete.
        // A row written before the structural fingerprint existed has no shape,
        // and skipping it would leave it blank forever — so it falls through
        // once, gets rewritten with its shape, and is skipped ever after.
        Some((h, skel, _, len))
            if h.as_slice() == program_hash && *skel == unmatched_tag && len.is_some() =>
        {
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
            match_kron_curve_v2(&program).map(|c| Matched {
                skeleton: "KRON curve v2",
                token_covenant_id: c.token_covenant_id,
                v_kas_units: c.v_kas_units,
                token_reserve: c.token_reserve,
                graduation_kas_sompi: Some(c.graduation_kas_sompi),
                creator: c.creator_fee_owner,
                kas_reserve_sompi: None,
                lp_token_covenant_id: None,
                shares: None,
            })
        })
        .or_else(|| {
            match_kron_curve_v3(&program).map(|c| Matched {
                skeleton: "KRON curve v3",
                token_covenant_id: c.token_covenant_id,
                v_kas_units: c.v_kas_units,
                token_reserve: c.token_reserve,
                graduation_kas_sompi: Some(c.graduation_kas_sompi),
                creator: c.creator_fee_owner,
                kas_reserve_sompi: None,
                lp_token_covenant_id: None,
                shares: None,
            })
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
        })
        .or_else(|| {
            match_kron_pool_tn_a(&program).map(|p| Matched {
                skeleton: "KRON pool tn-a",
                token_covenant_id: p.token_covenant_id,
                v_kas_units: 0,
                token_reserve: p.token_reserve,
                graduation_kas_sompi: None,
                creator: p.creator,
                kas_reserve_sompi: p.kas_reserve_units.checked_mul(1_000_000),
                lp_token_covenant_id: Some(p.lp_token_covenant_id),
                shares: Some(p.shares),
            })
        })
        .or_else(|| {
            match_kron_pool_v2(&program).map(|p| Matched {
                skeleton: "KRON pool v2",
                token_covenant_id: p.token_covenant_id,
                v_kas_units: 0,
                token_reserve: p.token_reserve,
                graduation_kas_sompi: None,
                creator: p.creator,
                kas_reserve_sompi: p.kas_reserve_units.checked_mul(1_000_000),
                lp_token_covenant_id: Some(p.lp_token_covenant_id),
                shares: Some(p.shares),
            })
        })
        .or_else(|| {
            match_kron_pool_v3(&program).map(|p| Matched {
                skeleton: "KRON pool v3",
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
        // The bytes are in hand right here — this is the only moment their
        // shape is free. Recording it turns the unknown queue from a list of
        // byte-unique strangers into families that can be audited together.
        conn.execute(
            "INSERT OR REPLACE INTO market_programs
                 (covenant_id, program_hash, skeleton, program_len, program_pushes)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                covenant_id.as_slice(),
                program_hash.as_slice(),
                unmatched_tag,
                program.len() as i64,
                push_units(&program).len() as i64,
            ],
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
    // What the trader pays and what the build keeps: constants of the pinned
    // templates, identical across every deployment of a build.
    let (bracket_fee_bps, growth_fee_bps) = fee_model(p.skeleton);
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
        .query_map(
            params![p.token_covenant_id.as_slice(), covenant_id.as_slice()],
            |r| {
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
            },
        )
        .map_err(db_err)?;
    for row in rows {
        let (seq, side, base, quote, k0, k1, b0, b1, co) = row.map_err(db_err)?;
        checked_through = seq;
        if co > 0 {
            continue; // stored, never judged: another covenant moved too
        }
        let (k0, k1, b0, b1) = (k0 as i128, k1 as i128, b0 as i128, b1 as i128);
        if !bracket_holds(
            v,
            k0,
            k1,
            b0,
            b1,
            quote as i128,
            base as i128,
            side == "buy",
            bracket_fee_bps,
        ) {
            continue; // off-curve: never priced, never counted
        }
        if !invariant_holds(v, k0, k1, b0, b1, quote as i128, growth_fee_bps) {
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
             invariant_ok, exercised_trades, program_len, program_pushes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
            p.lp_token_covenant_id
                .as_ref()
                .map(|l| l.as_slice().to_vec()),
            p.shares,
            checked_through,
            exercised,
            ok as i64,
            exercised,
            program.len() as i64,
            push_units(&program).len() as i64,
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
    /// The share count the pool stated in its newest PROVEN reveal — one spend
    /// behind the live cell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_shares: Option<i64>,
    /// LP tokens held outside the issuing covenant, as of the LIVE balances.
    /// A different moment from `pool_shares`: do not subtract them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_shares: Option<i64>,
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
        // Both halves are proven, but NOT at the same moment, and that makes
        // their difference unpublishable.
        //
        // `shares` is read from the newest proof-grade reveal of the pool
        // program, which is by construction one spend behind the live cell.
        // `held_wallet` is the live balance. Subtracting one from the other
        // spans however many add/remove-liquidity events happened in between,
        // so the remainder is not "shares nobody can redeem" — it is that
        // number plus an unknown drift. It briefly read as a clean 1,000,000
        // across two snapshots and then moved, which is exactly how a
        // mismatched-provenance figure fails: it looks like an invariant until
        // it doesn't.
        //
        // So publish the two facts with their provenance and let the reader
        // see they are from different moments. kascov does not subtract across
        // time and call the result proven.
        if let Some(prog) = market_program_row(conn, &pool)? {
            out.pool_shares = prog.shares;
            out.issued_shares = Some(held_wallet.unwrap_or(0) + held_script.unwrap_or(0));
            out.program = Some(prog);
        }
        return Ok(out);
    }
    let Some(market) = market_covenant_id else {
        out.unpriced_reason = Some("no single covenant holds this token's inventory".into());
        return Ok(out);
    };
    let program = market_program_row(conn, market)?;
    out.program = program.clone();
    let Some(prog) = program else {
        out.unpriced_reason = Some("the market covenant's program is not yet verified".into());
        return Ok(out);
    };
    out.phase = Some(
        phase_for_skeleton(&prog.skeleton)
            .unwrap_or("unknown")
            .into(),
    );
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
    let (fee_in_bps, _) = fee_model(&prog.skeleton);
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
    if let Some(last) = publishable
        .iter()
        .find(|t| t.quote_sompi >= MIN_PRICEABLE_QUOTE_SOMPI)
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
        out.window_note = Some("some of this token's trades predate timestamp capture".into());
    } else if let Some(end) = tip_at_ms {
        let start = end - WINDOW_SPAN_MS;
        let in_window: Vec<&&crate::tokens::TokenTradeRow> = publishable
            .iter()
            .filter(|t| {
                t.accepting_time_ms
                    .is_some_and(|ms| ms >= start && ms <= end)
            })
            .collect();
        if !in_window.is_empty() {
            let vol: i128 = in_window.iter().map(|t| t.quote_sompi as i128).sum();
            out.volume_24h_sompi = i64::try_from(vol).ok();
            out.trades_24h = Some(in_window.len() as i64);
        }
        // 24h change: newest resolvable vs newest resolvable before the window
        let newest = publishable
            .iter()
            .find(|t| t.quote_sompi >= MIN_PRICEABLE_QUOTE_SOMPI);
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
        if let (Some(grad), Some("bonding")) = (
        prog.graduation_kas_sompi,
        phase_for_skeleton(&prog.skeleton),
    ) {
            if grad > 0 {
                out.grad_progress_bps = (value as i128)
                    .checked_mul(10_000)
                    .map(|n| (n / grad as i128) as i64);
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
                v.and_then(|b| b.as_slice().try_into().ok())
                    .map(crate::CovenantId)
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
        assert!(!MATCHED_SKELETONS.contains(&"unmatched:6"));
        assert!(MATCHED_SKELETONS.contains(&"KRON curve v1"));
        assert!(MATCHED_SKELETONS.contains(&"KRON pool v1"));
        assert!(MATCHED_SKELETONS.contains(&"KRON curve v2"));
        assert!(MATCHED_SKELETONS.contains(&"KRON pool v2"));
        assert!(MATCHED_SKELETONS.contains(&"KRON curve v3"));
        assert!(MATCHED_SKELETONS.contains(&"KRON pool v3"));
        assert!(MATCHED_SKELETONS.contains(&"KRON pool tn-a"));
    }

    /// A build the matcher proves must also have a lifecycle phase, or the
    /// directory shows a verified market with a dash where bonding progress
    /// belongs — which is exactly what happened the day v3 landed.
    #[test]
    fn every_pinned_skeleton_has_a_phase() {
        for s in MATCHED_SKELETONS {
            assert!(
                phase_for_skeleton(s).is_some(),
                "{s} is matched but has no phase — its markets would serve 'unknown'"
            );
        }
        assert_eq!(phase_for_skeleton("KRON curve v3"), Some("bonding"));
        assert_eq!(phase_for_skeleton("KRON pool v3"), Some("graduated"));
        assert!(phase_for_skeleton(&unmatched_tag()).is_none());
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
        assert!(
            match_kron_curve(&evil).is_none(),
            "almost the build is not the build"
        );
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

    #[test]
    fn the_v2_fixture_matches_itself_and_reads_its_own_constants() {
        let p = match_kron_curve_v2(CURVE2_FIXTURE).expect("the v2 fixture IS the build");
        // the fixture is brave-crimson-tapir's curve, recovered from its own
        // spend and blake2b-proven against its P2SH commitment
        assert_eq!(p.v_kas_units, 6_500_000);
        assert_eq!(p.graduation_kas_sompi, 26_000_000_000_000);
        assert_eq!(p.token_reserve, 894_962_658);
        assert_eq!(
            hex::encode(p.token_covenant_id),
            "7ff8c8810f64844915fff6c1f18b108c400e5ea4cecde778d77f6f652057f051"
        );
        assert_eq!(
            hex::encode(p.creator_fee_owner),
            "e8ece1095f00072c83d799dd26c2c99f8ed06ce4ae2bc0c1560d57e048d25523"
        );
        // and the two builds never claim each other's programs
        assert!(match_kron_curve(CURVE2_FIXTURE).is_none());
        assert!(match_kron_curve_v2(CURVE_FIXTURE).is_none());
    }

    #[test]
    fn v2_one_flipped_byte_outside_the_slots_is_a_different_program() {
        let units = push_units(CURVE2_FIXTURE);
        let target = (0..CURVE2_PUSHES)
            .find(|i| !CURVE2_SLOTS.contains(i) && !units[*i].1.is_empty())
            .expect("a non-slot data push exists");
        let mut evil = CURVE2_FIXTURE.to_vec();
        let at = units[target].0.end - 1;
        evil[at] ^= 0x01;
        assert!(match_kron_curve_v2(&evil).is_none());
        assert!(match_kron_curve_v2(&[]).is_none());
    }

    #[test]
    fn v2_a_program_lying_about_its_own_length_is_rejected() {
        // Self-length pushes are SLOTS (they vary with slot widths across
        // deployments), so the byte walk alone would accept a lie. The
        // internal-consistency check must not: state 185,751 in a program of
        // 185,750 bytes and the match dies.
        let units = push_units(CURVE2_FIXTURE);
        let (range, data) = &units[IDX2_SELF_LEN[0]];
        let honest = le_i64(data).unwrap();
        assert_eq!(honest, CURVE2_FIXTURE.len() as i64, "fixture tells the truth");
        let mut evil = CURVE2_FIXTURE.to_vec();
        let lie = (honest + 1).to_le_bytes();
        let width = data.len();
        evil[range.end - width..range.end].copy_from_slice(&lie[..width]);
        assert!(match_kron_curve_v2(&evil).is_none());
    }

    #[test]
    fn v2_mismatched_embedded_pool_templates_are_rejected() {
        // Both 59 KB pool copies are slots, so flipping a byte inside ONE of
        // them passes the byte walk. The pair-equality check is what stops a
        // program that would graduate into something other than it displays.
        let units = push_units(CURVE2_FIXTURE);
        let mut evil = CURVE2_FIXTURE.to_vec();
        let at = units[IDX2_POOL_TPL[1]].0.end - 1;
        evil[at] ^= 0x01;
        assert!(match_kron_curve_v2(&evil).is_none());
    }

    #[test]
    fn the_v2_pool_build_matches_and_reads_its_state() {
        // The REAL mainnet state of the first v2-graduated pool (PEPE's), in
        // front of the pinned v2 template.
        let state = hex::decode(
            "6b08a08b32000000000008ec50de020000000020a73cdef004099b191759d320de970451be0e10\
             423a7eb15b07d5e51d050b47cd08c2892a000000000020dccec1e1255babd0e4617901a16ffd2c\
             42d55f7d346aca8866d387511eb5e507",
        )
        .unwrap();
        assert_eq!(state.len(), POOL_STATE_LEN);
        let mut prog = state;
        prog.extend_from_slice(POOL2_FIXTURE);
        let p = match_kron_pool_v2(&prog).expect("state + pinned v2 template must match");
        assert_eq!(p.kas_reserve_units, 3_312_544);
        assert_eq!(p.token_reserve, 48_124_140);
        assert_eq!(p.shares, 2_787_778);
        assert_eq!(
            hex::encode(p.token_covenant_id),
            "a73cdef004099b191759d320de970451be0e10423a7eb15b07d5e51d050b47cd"
        );
        assert_eq!(
            hex::encode(p.lp_token_covenant_id),
            "dccec1e1255babd0e4617901a16ffd2c42d55f7d346aca8866d387511eb5e507"
        );
        // the two pool builds never claim each other's programs
        assert!(match_kron_pool(&prog).is_none());
        // a flipped byte outside the creator slots is a different build
        let mut evil = prog.clone();
        let units = push_units(POOL2_FIXTURE);
        let t = (0..units.len())
            .find(|i| !POOL2_TEMPLATE_CREATOR_SLOTS.contains(i) && !units[*i].1.is_empty())
            .unwrap();
        evil[POOL_STATE_LEN + units[t].0.end - 1] ^= 0x01;
        assert!(match_kron_pool_v2(&evil).is_none());
    }

    #[test]
    fn the_v2_fee_model_is_what_the_bytes_say() {
        // bracket slack and reserve-growth allowance per build; the v2 curve
        // is the odd one out (tokens-side partner fee grows the reserve
        // without widening the executed price)
        assert_eq!(fee_model("KRON curve v1"), (0, 125));
        assert_eq!(fee_model("KRON curve v2"), (0, 125));
        assert_eq!(fee_model("KRON curve v3"), (0, 125));
        assert_eq!(fee_model("KRON pool v1"), (20, 20));
        assert_eq!(fee_model("KRON pool v2"), (20, 20));
        assert_eq!(fee_model("KRON pool v3"), (20, 20));
        assert_eq!(fee_model("KRON pool tn-a"), (20, 20));
        assert_eq!(fee_model(&unmatched_tag()), (0, 0));
    }

    #[test]
    fn the_tn_a_pool_build_matches_and_reads_its_state() {
        // A REAL testnet vector: the state block of the 76-trade pool, in
        // front of the pinned template. Ten deployments share this build.
        let state = hex::decode(
            "6b08e62200000000000008950dd40600000000209dad170417bdd934b42df41cbe1dddb23e8d\
             62ad6b6c8ec761ea812dbb9109dd0840420f000000000020aefbebbe3cffc75c56ec964d8f94\
             83e6f0027fb7a9821428a6f45db383599e73",
        )
        .unwrap();
        assert_eq!(state.len(), POOL_STATE_LEN);
        let mut prog = state;
        prog.extend_from_slice(POOL_TN_A_FIXTURE);
        let p = match_kron_pool_tn_a(&prog).expect("state + pinned tn-a template must match");
        assert_eq!(p.kas_reserve_units, 8_934);
        assert_eq!(p.token_reserve, 114_560_405);
        assert_eq!(p.shares, 1_000_000);
        assert_eq!(
            hex::encode(&p.token_covenant_id[..8]),
            "9dad170417bdd934"
        );
        // never claimed by the other pool builds, and vice versa
        assert!(match_kron_pool(&prog).is_none());
        assert!(match_kron_pool_v2(&prog).is_none());
        // a flipped template byte outside the creator slots is a different build
        let mut evil = prog.clone();
        let units = push_units(POOL_TN_A_FIXTURE);
        let t = (0..units.len())
            .find(|i| !POOL_TN_A_CREATOR_SLOTS.contains(i) && !units[*i].1.is_empty())
            .unwrap();
        evil[POOL_STATE_LEN + units[t].0.end - 1] ^= 0x01;
        assert!(match_kron_pool_tn_a(&evil).is_none());
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
        assert!(
            !invariant_holds(v, k0, k0 - quote, b0, b1, quote, 0),
            "k may not shrink"
        );
        assert!(
            !invariant_holds(v, k0, k1 + k1 / 100, b0, b1, quote, 0),
            "an in-covenant fee grows k past the cap and refuses the parameters"
        );
    }

    #[test]
    fn the_v3_fixture_matches_itself_and_reads_its_own_constants() {
        let p = match_kron_curve_v3(CURVE3_FIXTURE).expect("the v3 fixture IS the build");
        // the fixture is shy-teal-raven's curve, recovered from its own spend
        // and blake2b-proven against its P2SH commitment
        assert_eq!(p.v_kas_units, 6_750_000);
        assert_eq!(p.graduation_kas_sompi, 27_000_000_000_000);
        assert_eq!(p.token_reserve, 928_003_055);
        assert_eq!(
            hex::encode(p.token_covenant_id),
            "b7da7284276b33b755d2306bb9a0d4842dcf6327626e4aa1e46231f9f31a1b55"
        );
        assert_eq!(
            hex::encode(p.creator_fee_owner),
            "be16dc3acc4e64eb98fb1f72bce279e54bb5c26655608c9f40dc0308e55ec0bd"
        );
        // no generation ever claims another generation's programs
        assert!(match_kron_curve(CURVE3_FIXTURE).is_none());
        assert!(match_kron_curve_v2(CURVE3_FIXTURE).is_none());
        assert!(match_kron_curve_v3(CURVE_FIXTURE).is_none());
        assert!(match_kron_curve_v3(CURVE2_FIXTURE).is_none());
    }

    #[test]
    fn v3_one_flipped_byte_outside_the_slots_is_a_different_program() {
        let units = push_units(CURVE3_FIXTURE);
        let target = (0..CURVE3_PUSHES)
            .find(|i| !CURVE3_SLOTS.contains(i) && !units[*i].1.is_empty())
            .expect("a non-slot data push exists");
        let mut evil = CURVE3_FIXTURE.to_vec();
        let at = units[target].0.end - 1;
        evil[at] ^= 0x01;
        assert!(match_kron_curve_v3(&evil).is_none());
        assert!(match_kron_curve_v3(&[]).is_none());
    }

    #[test]
    fn v3_a_program_lying_about_its_own_length_is_rejected() {
        // The three specimens share one byte length, so these ten positions
        // never differed across instances — they are pinned as slots anyway
        // because their VALUE is the program's length. The consistency check
        // is what makes that pin safe: state 185,892 in a program of 185,891
        // bytes and the match dies.
        let units = push_units(CURVE3_FIXTURE);
        let (range, data) = &units[IDX3_SELF_LEN[0]];
        let honest = le_i64(data).unwrap();
        assert_eq!(honest, CURVE3_FIXTURE.len() as i64, "fixture tells the truth");
        let mut evil = CURVE3_FIXTURE.to_vec();
        let lie = (honest + 1).to_le_bytes();
        let width = data.len();
        evil[range.end - width..range.end].copy_from_slice(&lie[..width]);
        assert!(match_kron_curve_v3(&evil).is_none());
    }

    #[test]
    fn v3_mismatched_embedded_pool_templates_are_rejected() {
        let units = push_units(CURVE3_FIXTURE);
        let mut evil = CURVE3_FIXTURE.to_vec();
        let at = units[IDX3_POOL_TPL[1]].0.end - 1;
        evil[at] ^= 0x01;
        assert!(match_kron_curve_v3(&evil).is_none());
    }

    #[test]
    fn the_v3_pool_build_matches_and_reads_its_state() {
        // The pool fixture's provenance is the curve itself: it must BE the
        // template the blake2b-proven v3 curve fixture embeds (twice).
        let curve_units = push_units(CURVE3_FIXTURE);
        assert_eq!(curve_units[IDX3_POOL_TPL[0]].1.as_slice(), POOL3_FIXTURE);
        assert_eq!(curve_units[IDX3_POOL_TPL[1]].1.as_slice(), POOL3_FIXTURE);

        // The REAL mainnet state of the first v3-graduated pool, in front of
        // the pinned template. That pool belongs to a DIFFERENT deployment
        // than the fixture's curve, so this also proves the creator slots are
        // the only bytes that vary.
        let state = hex::decode(
            "6b0852fc00000000000008586f78090000000020d101341bc8c9ed351cc5f5dd6e6a813a41\
             5e2facb80a907be517ccfa0b7d64d8087e6810000000000020ac0e1d57dc2e2d56260dc913\
             e14288500670d56f46a0a961cb701a29408bd01d",
        )
        .unwrap();
        assert_eq!(state.len(), POOL_STATE_LEN);
        let mut prog = state;
        prog.extend_from_slice(POOL3_FIXTURE);
        let p = match_kron_pool_v3(&prog).expect("state + pinned v3 template must match");
        assert_eq!(p.kas_reserve_units, 64_594);
        assert_eq!(p.token_reserve, 158_887_768);
        assert_eq!(p.shares, 1_075_326);
        assert_eq!(
            hex::encode(p.token_covenant_id),
            "d101341bc8c9ed351cc5f5dd6e6a813a415e2facb80a907be517ccfa0b7d64d8"
        );
        assert_eq!(
            hex::encode(p.lp_token_covenant_id),
            "ac0e1d57dc2e2d56260dc913e14288500670d56f46a0a961cb701a29408bd01d"
        );
        // the pool generations never claim each other's programs
        assert!(match_kron_pool(&prog).is_none());
        assert!(match_kron_pool_v2(&prog).is_none());
        // a flipped byte outside the creator slots is a different build
        let mut evil = prog.clone();
        let units = push_units(POOL3_FIXTURE);
        let t = (0..units.len())
            .find(|i| !POOL3_TEMPLATE_CREATOR_SLOTS.contains(i) && !units[*i].1.is_empty())
            .unwrap();
        evil[POOL_STATE_LEN + units[t].0.end - 1] ^= 0x01;
        assert!(match_kron_pool_v3(&evil).is_none());
    }
}
