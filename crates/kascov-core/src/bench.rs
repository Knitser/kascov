//! The audit bench: automated forensics on unmatched market programs.
//!
//! Everything mechanical about auditing an unknown build happens here, the
//! same way it was first done by hand for the KRON v2 generation: recover
//! each covenant's program from its own spend and blake2b-prove it against
//! its P2SH commitment, cluster programs into build families by structure,
//! diff family members push-by-push to find the per-deployment slots, and
//! trial-replay every trade to locate the constants that make the build's
//! own arithmetic hold.
//!
//! What deliberately does NOT happen here: promotion. The bench writes a
//! report of PROPOSED skeletons; a human reads the diff, names the build,
//! and pins it in `market.rs` with fixtures and tests. A verifier that
//! auto-approves programs nobody read would be worthless — the bench turns
//! hours of forensics into minutes of judgment, never into zero judgment.
//!
//! The bench is read-only with respect to verdicts: it never writes
//! `market_programs`, only its own report file, which the worker serves as
//! an `audit_bench` section of the verification page.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use crate::market::{bracket_holds, invariant_holds, push_units_of, KAS_QUANTUM_SOMPI};
use crate::store::db_err;
use crate::Result;

/// One recovered, hash-proven program.
struct Specimen {
    covenant_id: [u8; 32],
    program: Vec<u8>,
    trades: i64,
}

/// The fee models worth trying, in the order the pinned builds use them:
/// (bracket_fee_bps, growth_fee_bps, label).
const FEE_TRIALS: [(i128, i128, &str); 3] =
    [(0, 0, "no fee"), (0, 125, "curve 125bps growth"), (20, 20, "pool 20bps")];

/// Run the bench over every unmatched market program. Returns the report as
/// JSON; the caller decides where it lives.
pub fn run_bench(conn: &Connection) -> Result<serde_json::Value> {
    // 1. Every covenant the current matcher gave up on.
    let mut stmt = conn
        .prepare(
            "SELECT covenant_id FROM market_programs WHERE skeleton GLOB 'unmatched*'
             ORDER BY covenant_id",
        )
        .map_err(db_err)?;
    let unmatched: Vec<[u8; 32]> = stmt
        .query_map([], |r| r.get(0))
        .map_err(db_err)?
        .collect::<std::result::Result<_, _>>()
        .map_err(db_err)?;

    // 2. Recover and hash-prove each program from its own spends.
    let mut specimens: Vec<Specimen> = Vec::new();
    let mut unrecoverable = 0usize;
    for cid in &unmatched {
        let rows: Vec<(Vec<u8>, Vec<u8>)> = conn
            .prepare_cached(
                "SELECT spk_script, spent_sig FROM covenant_utxos
                 WHERE covenant_id = ?1 AND spent_sig IS NOT NULL
                 ORDER BY created_daa DESC LIMIT 8",
            )
            .map_err(db_err)?
            .query_map([cid.as_slice()], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        let Some(program) = rows
            .iter()
            .find_map(|(spk, sig)| kascov_decode::p2sh_reveal(spk, sig))
        else {
            unrecoverable += 1;
            continue;
        };
        let trades: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM token_trades WHERE market_covenant_id = ?1",
                [cid.as_slice()],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        specimens.push(Specimen {
            covenant_id: *cid,
            program,
            trades,
        });
    }

    // 3. Cluster into families: same push count AND identical opcode gaps.
    //    Total length may vary WITHIN a family (slot widths move, exactly as
    //    the v2 curve proved), so length is reported, never used as the key.
    let mut families: BTreeMap<(usize, Vec<u8>), Vec<usize>> = BTreeMap::new();
    for (i, s) in specimens.iter().enumerate() {
        let units = push_units_of(&s.program);
        let key = (units.len(), gap_digest(&s.program, &units));
        families.entry(key).or_default().push(i);
    }

    // 4. Analyse each family, biggest trade volume first.
    let mut out: Vec<serde_json::Value> = Vec::new();
    for ((pushes, _), members) in &families {
        let total_trades: i64 = members.iter().map(|&i| specimens[i].trades).sum();
        let lens: BTreeSet<usize> = members.iter().map(|&i| specimens[i].program.len()).collect();

        // Slots: pushes whose value differs anywhere across the family.
        // One member means no diff is possible; the family is then a single
        // deployment and only an exact-hash pin would cover it.
        let member_units: Vec<Vec<(std::ops::Range<usize>, Vec<u8>)>> = members
            .iter()
            .map(|&i| push_units_of(&specimens[i].program))
            .collect();
        let slots: Vec<usize> = if members.len() >= 2 {
            (0..*pushes)
                .filter(|&p| {
                    let first = &member_units[0][p].1;
                    member_units[1..].iter().any(|u| &u[p].1 != first)
                })
                .collect()
        } else {
            Vec::new()
        };

        // 5. Constant trial: which (v-slot, fee model) makes the trades
        //    replay? v candidates are slots that decode small on EVERY
        //    member, plus v=0 (pool-shaped builds have no virtual reserve).
        let mut v_candidates: Vec<Option<usize>> = vec![None]; // None => v = 0
        for &sl in &slots {
            let all_small = member_units.iter().all(|u| {
                le_i64(&u[sl].1).is_some_and(|v| (1..=1_000_000_000_000).contains(&v))
            });
            if all_small {
                v_candidates.push(Some(sl));
            }
        }
        let mut best: Option<serde_json::Value> = None;
        let mut best_score = -1i64;
        for vc in &v_candidates {
            for (bracket_fee, growth_fee, fee_label) in FEE_TRIALS {
                let (mut clean, mut off, mut inv_bad) = (0i64, 0i64, 0i64);
                for (mi, &si) in members.iter().enumerate() {
                    let v: i128 = match vc {
                        None => 0,
                        Some(sl) => {
                            le_i64(&member_units[mi][*sl].1).unwrap_or(0) as i128
                                * KAS_QUANTUM_SOMPI
                        }
                    };
                    replay_into(
                        conn,
                        &specimens[si].covenant_id,
                        v,
                        bracket_fee,
                        growth_fee,
                        &mut clean,
                        &mut off,
                        &mut inv_bad,
                    )?;
                }
                // A trial only counts when it explains trades without a
                // single invariant break; among those, most-clean wins.
                if inv_bad == 0 && clean > best_score {
                    best_score = clean;
                    best = Some(serde_json::json!({
                        "v_slot": vc,
                        "fee_model": fee_label,
                        "trades_clean": clean,
                        "trades_off_curve": off,
                    }));
                }
            }
        }

        let sample = &specimens[members[0]];
        out.push(serde_json::json!({
            "push_count": pushes,
            "program_lens": lens.iter().collect::<Vec<_>>(),
            "instances": members.len(),
            "trades": total_trades,
            "slots": slots.len(),
            "slot_indices": if slots.len() <= 64 { serde_json::json!(slots) } else { serde_json::json!(null) },
            "sample_covenant": hex::encode(sample.covenant_id),
            "sample_program_hash": hex::encode(kascov_decode::kcc20::blake2b_256(&sample.program)),
            "pool_shaped": sample.program.first() == Some(&0x6b),
            "replay_trial": best,
            "note": if members.len() < 2 {
                "single deployment: slots cannot be derived; only an exact-hash pin would cover it"
            } else {
                "byte-identical outside the slots; a fixture + these slots is a pinnable skeleton"
            },
        }));
    }
    out.sort_by_key(|f| -(f["trades"].as_i64().unwrap_or(0)));

    Ok(serde_json::json!({
        "note": "automated forensics on builds the matcher gave up on — proposals for a human to \
                 pin, never verdicts: nothing here is priced or published until a person reads \
                 the build and pins it with fixtures and tests",
        "unmatched_covenants": unmatched.len(),
        "recovered": specimens.len(),
        "unrecoverable": unrecoverable,
        "families": out,
    }))
}

/// Concatenated non-push bytes: two programs with equal digests here have
/// identical opcode structure, whatever their slots carry.
fn gap_digest(program: &[u8], units: &[(std::ops::Range<usize>, Vec<u8>)]) -> Vec<u8> {
    let mut gaps = Vec::new();
    let mut prev = 0usize;
    for (r, _) in units {
        gaps.extend_from_slice(&program[prev..r.start]);
        prev = r.end;
    }
    gaps.extend_from_slice(&program[prev..]);
    kascov_decode::kcc20::blake2b_256(&gaps).to_vec()
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

#[allow(clippy::too_many_arguments)]
fn replay_into(
    conn: &Connection,
    covenant_id: &[u8; 32],
    v: i128,
    bracket_fee: i128,
    growth_fee: i128,
    clean: &mut i64,
    off: &mut i64,
    inv_bad: &mut i64,
) -> Result<()> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT side, base_amount, quote_sompi, kas_before_sompi, kas_after_sompi,
                    base_before, base_after, co_covenants
             FROM token_trades WHERE market_covenant_id = ?1 ORDER BY seq",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map([covenant_id.as_slice()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })
        .map_err(db_err)?;
    for row in rows {
        let (side, base, quote, k0, k1, b0, b1, co) = row.map_err(db_err)?;
        if co > 0 {
            continue;
        }
        let (k0, k1, b0, b1) = (k0 as i128, k1 as i128, b0 as i128, b1 as i128);
        if !bracket_holds(v, k0, k1, b0, b1, quote as i128, base as i128, side == "buy", bracket_fee)
        {
            *off += 1;
            continue;
        }
        if !invariant_holds(v, k0, k1, b0, b1, quote as i128, growth_fee) {
            *inv_bad += 1;
            continue;
        }
        *clean += 1;
    }
    Ok(())
}
