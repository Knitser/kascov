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

/// One row of the pin provenance ledger: who first pinned an audited build,
/// when, and in which commit. A family leaves the unknowns board the moment
/// it is pinned, so without this record the audit surface forgets its own
/// history — the board promises a permanent "first pinned by" stamp and this
/// is the only place it can come from. Append-only: a rename or re-pin gets
/// its own context in `market.rs`, it never rewrites a row here.
pub struct PinEntry {
    pub skeleton: &'static str,
    pub first_pinned_by: &'static str,
    /// YYYY-MM-DD of the pinning commit.
    pub date: &'static str,
    /// Short hash of the commit that first pinned the skeleton — dates and
    /// hashes are the repository's own history, recoverable with
    /// `git log --oneline --all -G "<skeleton>"` (oldest hit).
    pub commit: &'static str,
}

const FOUNDER: &str = "kascov core (Knitser)";

/// Every skeleton ever pinned, in pin order. The completeness guard in the
/// tests below forces a row for each `MATCHED_SKELETONS` entry, so pinning
/// a build without stamping its provenance does not compile past CI.
pub const PIN_LEDGER: [PinEntry; 8] = [
    PinEntry {
        skeleton: "KRON curve v1",
        first_pinned_by: FOUNDER,
        date: "2026-07-28",
        commit: "5859e81",
    },
    PinEntry {
        skeleton: "KRON pool v1",
        first_pinned_by: FOUNDER,
        date: "2026-07-28",
        commit: "ff798a8",
    },
    PinEntry {
        skeleton: "KRON curve v2",
        first_pinned_by: FOUNDER,
        date: "2026-08-02",
        commit: "31e7450",
    },
    PinEntry {
        skeleton: "KRON pool v2",
        first_pinned_by: FOUNDER,
        date: "2026-08-02",
        commit: "31e7450",
    },
    PinEntry {
        skeleton: "KRON pool tn-a",
        first_pinned_by: FOUNDER,
        date: "2026-08-03",
        commit: "ba02d80",
    },
    PinEntry {
        skeleton: "KRON curve v3",
        first_pinned_by: FOUNDER,
        date: "2026-08-06",
        commit: "61a41eb",
    },
    PinEntry {
        skeleton: "KRON pool v3",
        first_pinned_by: FOUNDER,
        date: "2026-08-06",
        commit: "61a41eb",
    },
    PinEntry {
        skeleton: "curve tn-b",
        first_pinned_by: FOUNDER,
        date: "2026-08-06",
        commit: "29e9979",
    },
];

/// The fee models worth trying, in the order the pinned builds use them:
/// (bracket_fee_bps, growth_fee_bps, label).
const FEE_TRIALS: [(i128, i128, &str); 3] =
    [(0, 0, "no fee"), (0, 125, "curve 125bps growth"), (20, 20, "pool 20bps")];

/// One covenant's program, recovered from its own most recent spends. The
/// bench uses this over every unmatched covenant; `dump-program` uses it to
/// lift a fixture for a human to pin. Same reveal path either way: the bytes
/// are the ones the chain accepted against the commitment.
pub fn recover_program(conn: &Connection, covenant_id: &[u8; 32]) -> Result<Option<Vec<u8>>> {
    let rows: Vec<(Vec<u8>, Vec<u8>)> = conn
        .prepare_cached(
            "SELECT spk_script, spent_sig FROM covenant_utxos
             WHERE covenant_id = ?1 AND spent_sig IS NOT NULL
             ORDER BY created_daa DESC LIMIT 8",
        )
        .map_err(db_err)?
        .query_map([covenant_id.as_slice()], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_err)?
        .collect::<std::result::Result<_, _>>()
        .map_err(db_err)?;
    Ok(rows
        .iter()
        .find_map(|(spk, sig)| kascov_decode::p2sh_reveal(spk, sig)))
}

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
        let sample_hash = hex::encode(kascov_decode::kcc20::blake2b_256(&sample.program));
        // Everything a human needs to REVIEW a pin instead of deriving one:
        // the slot list, the repeated-value role groups the matchers would
        // turn into agreement checks, and the trial that already replayed.
        // Only for families the slot diff can reach — a single deployment
        // has no slots to propose.
        let pin_proposal = (members.len() >= 2).then(|| {
            propose_pin(
                *pushes,
                &slots,
                &member_units,
                &members
                    .iter()
                    .map(|&i| specimens[i].program.len())
                    .collect::<Vec<_>>(),
                best.as_ref(),
                &sample_hash,
            )
        });
        out.push(serde_json::json!({
            "push_count": pushes,
            "program_lens": lens.iter().collect::<Vec<_>>(),
            "instances": members.len(),
            "trades": total_trades,
            "slots": slots.len(),
            "slot_indices": if slots.len() <= 64 { serde_json::json!(slots) } else { serde_json::json!(null) },
            "sample_covenant": hex::encode(sample.covenant_id),
            "sample_program_hash": sample_hash,
            "pool_shaped": sample.program.first() == Some(&0x6b),
            "replay_trial": best,
            "pin_proposal": pin_proposal,
            "note": if members.len() < 2 {
                "single deployment: slots cannot be derived; only an exact-hash pin would cover it"
            } else {
                "byte-identical outside the slots; a fixture + these slots is a pinnable skeleton"
            },
        }));
    }
    out.sort_by_key(|f| -(f["trades"].as_i64().unwrap_or(0)));

    // 6. The complement of the unknowns: families a human already pinned.
    //    Stamps come from PIN_LEDGER (git history, immutable), instance
    //    counts from the live verdict table — a ledger row with no current
    //    instances still reports, at zero, because provenance does not
    //    expire with a family's deployments.
    let mut instances: BTreeMap<String, i64> = BTreeMap::new();
    let mut stmt = conn
        .prepare("SELECT skeleton, COUNT(*) FROM market_programs GROUP BY skeleton")
        .map_err(db_err)?;
    let counted = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(db_err)?;
    for row in counted {
        let (skeleton, n) = row.map_err(db_err)?;
        instances.insert(skeleton, n);
    }
    let pinned: Vec<serde_json::Value> = PIN_LEDGER
        .iter()
        .map(|p| {
            serde_json::json!({
                "skeleton": p.skeleton,
                "first_pinned_by": p.first_pinned_by,
                "date": p.date,
                "commit": p.commit,
                "instances": instances.get(p.skeleton).copied().unwrap_or(0),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "note": "automated forensics on builds the matcher gave up on — proposals for a human to \
                 pin, never verdicts: nothing here is priced or published until a person reads \
                 the build and pins it with fixtures and tests",
        "unmatched_covenants": unmatched.len(),
        "recovered": specimens.len(),
        "unrecoverable": unrecoverable,
        "families": out,
        "pinned": pinned,
    }))
}

/// The reviewable half of a pin: what a human would otherwise re-derive by
/// hand before writing a `SkeletonPin` entry in `market.rs`. Role guesses
/// are the REPEATED-value groups the matchers turn into agreement checks —
/// slots that carry one value per program in EVERY member. Guesses, never
/// verdicts: a name still has to come from a person, because a slot nobody
/// can name is a slot nobody audited.
fn propose_pin(
    pushes: usize,
    slots: &[usize],
    member_units: &[Vec<(std::ops::Range<usize>, Vec<u8>)>],
    member_lens: &[usize],
    best: Option<&serde_json::Value>,
    fixture_hash: &str,
) -> serde_json::Value {
    // Group slots by the value they carry in the first member, then keep
    // only groups that stay internally equal in every other member (each
    // with its own value — that is what makes them one inlined constant).
    let mut by_value: BTreeMap<&Vec<u8>, Vec<usize>> = BTreeMap::new();
    for &sl in slots {
        by_value.entry(&member_units[0][sl].1).or_default().push(sl);
    }
    let mut creator_like: Vec<Vec<usize>> = Vec::new();
    let mut i64_groups: Vec<serde_json::Value> = Vec::new();
    for (val, group) in by_value {
        if group.len() < 2 {
            continue;
        }
        let coherent = member_units.iter().all(|u| {
            let first = &u[group[0]].1;
            group.iter().all(|&sl| &u[sl].1 == first)
        });
        if !coherent {
            continue;
        }
        if val.len() == 32 && member_units.iter().all(|u| u[group[0]].1.len() == 32) {
            creator_like.push(group);
        } else if member_units.iter().all(|u| le_i64(&u[group[0]].1).is_some()) {
            i64_groups.push(serde_json::json!({
                "slots": group,
                "sample_value": le_i64(val),
            }));
        }
    }
    // Self-length candidates need no repetition: one slot stating each
    // member's own byte length is already the v2-style lie detector.
    let self_len: Vec<usize> = slots
        .iter()
        .copied()
        .filter(|&sl| {
            member_units
                .iter()
                .zip(member_lens)
                .all(|(u, &len)| le_i64(&u[sl].1) == Some(len as i64))
        })
        .collect();
    serde_json::json!({
        "push_count": pushes,
        "slot_indices": slots,
        "role_guesses": {
            "creator_like_32b": creator_like,
            "i64_groups": i64_groups,
            "self_len_candidates": self_len,
        },
        "v_slot": best.map_or(serde_json::Value::Null, |b| b["v_slot"].clone()),
        "fee_model_trial": best.map_or(serde_json::Value::Null, |b| b["fee_model"].clone()),
        "fixture_hash": fixture_hash,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::MATCHED_SKELETONS;
    use crate::model::Network;
    use crate::store::Store;

    /// Completeness guard, same shape as `every_pinned_skeleton_has_a_phase`:
    /// a build the matcher proves must also carry its provenance stamp, or
    /// the unknowns board shows a matched family with a dash where "first
    /// pinned by" belongs.
    #[test]
    fn every_matched_skeleton_has_a_ledger_row() {
        for s in MATCHED_SKELETONS {
            assert!(
                PIN_LEDGER.iter().any(|p| p.skeleton == s),
                "{s} is matched but has no PIN_LEDGER row — its pin would vanish from the audit surface"
            );
        }
    }

    /// A stamp with a hole in it renders as the dash the ledger exists to
    /// kill, and a duplicated skeleton would make the join ambiguous.
    #[test]
    fn the_ledger_is_unique_and_fully_stamped() {
        for (i, p) in PIN_LEDGER.iter().enumerate() {
            assert!(
                !p.first_pinned_by.is_empty(),
                "{}: who pinned it?",
                p.skeleton
            );
            assert_eq!(p.date.len(), 10, "{}: date is YYYY-MM-DD", p.skeleton);
            assert_eq!(p.commit.len(), 7, "{}: commit is a short hash", p.skeleton);
            assert!(
                PIN_LEDGER[..i].iter().all(|q| q.skeleton != p.skeleton),
                "{} appears twice",
                p.skeleton
            );
        }
    }

    fn push(out: &mut Vec<u8>, data: &[u8]) {
        out.push(data.len() as u8);
        out.extend_from_slice(data);
    }

    /// One deployment of a synthetic build: seven pushes separated by
    /// OP_CHECKSIG gaps — a creator inlined twice, an i64 constant inlined
    /// twice, a truthful self-length, one width-varying odd slot, and one
    /// constant shared by every member (so it is never a slot).
    fn synthetic_member(creator: u8, v: i64, odd: &[u8]) -> Vec<u8> {
        let mut p = Vec::new();
        push(&mut p, &[creator; 32]); // 0: creator
        p.push(0xac);
        push(&mut p, &v.to_le_bytes()); // 1: repeated i64
        p.push(0xac);
        push(&mut p, &[creator; 32]); // 2: creator again
        p.push(0xac);
        push(&mut p, &v.to_le_bytes()); // 3: repeated i64 again
        p.push(0xac);
        let self_len_at = p.len() + 1;
        push(&mut p, &0i64.to_le_bytes()); // 4: self-length, patched below
        p.push(0xac);
        push(&mut p, odd); // 5: width varies, so member lengths differ
        p.push(0xac);
        push(&mut p, &[0x77; 32]); // 6: shared constant, not a slot
        let n = p.len() as i64;
        p[self_len_at..self_len_at + 8].copy_from_slice(&n.to_le_bytes());
        p
    }

    /// Store one specimen the way the chain would present it: an unmatched
    /// verdict row, plus a spent P2SH cell whose signature reveals the
    /// program against its own blake2b commitment.
    fn insert_specimen(conn: &Connection, cid: [u8; 32], program: &[u8]) {
        let hash = kascov_decode::kcc20::blake2b_256(program);
        let mut spk = vec![0xaa, 0x20];
        spk.extend_from_slice(&hash);
        spk.push(0x87);
        let mut sig = vec![0x4c, u8::try_from(program.len()).expect("test program < 256 bytes")];
        sig.extend_from_slice(program);
        conn.execute(
            "INSERT INTO market_programs (covenant_id, program_hash, skeleton)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![cid.as_slice(), hash.as_slice(), crate::market::unmatched_tag()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO covenant_utxos (txid, output_index, covenant_id, value, spk_version,
                 spk_script, created_block, created_daa, spent_sig)
             VALUES (?1, 0, ?2, 1, 0, ?3, ?4, 1, ?5)",
            rusqlite::params![cid.as_slice(), cid.as_slice(), spk, [0u8; 32].as_slice(), sig],
        )
        .unwrap();
    }

    /// Two deployments cluster into one family and earn a pin_proposal whose
    /// role guesses are exactly the repeated-value structure the programs
    /// carry; the single-deployment stranger next to them earns none.
    #[test]
    fn a_two_instance_family_gets_a_pin_proposal() {
        let path = std::env::temp_dir().join(format!(
            "kascov-bench-test-{}-proposal.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = Store::open(&path, Network::Testnet(10)).unwrap();
        let conn = store.raw_conn();
        insert_specimen(conn, [1u8; 32], &synthetic_member(0xAA, 1_000, &[1, 2]));
        insert_specimen(conn, [2u8; 32], &synthetic_member(0xBB, 2_000, &[1, 2, 3]));
        let mut lone = Vec::new();
        push(&mut lone, &[0x55; 32]);
        lone.push(0xac);
        insert_specimen(conn, [9u8; 32], &lone);

        let report = run_bench(conn).unwrap();
        let fams = report["families"].as_array().expect("families array");
        assert_eq!(fams.len(), 2, "the pair clusters, the stranger stands alone");

        let fam = fams.iter().find(|f| f["instances"] == 2).expect("pair family");
        let prop = &fam["pin_proposal"];
        assert_eq!(prop["push_count"], 7);
        assert_eq!(prop["slot_indices"], serde_json::json!([0, 1, 2, 3, 4, 5]));
        let roles = &prop["role_guesses"];
        assert_eq!(roles["creator_like_32b"], serde_json::json!([[0, 2]]));
        assert_eq!(
            roles["i64_groups"],
            serde_json::json!([{ "slots": [1, 3], "sample_value": 1_000 }])
        );
        assert_eq!(roles["self_len_candidates"], serde_json::json!([4]));
        // no trades recorded, so the v=0 no-fee trial is the best that exists
        assert!(prop["v_slot"].is_null());
        assert_eq!(prop["fee_model_trial"], "no fee");
        // the proposed fixture is the sample program itself
        assert_eq!(prop["fixture_hash"], fam["sample_program_hash"]);

        let stranger = fams.iter().find(|f| f["instances"] == 1).expect("lone family");
        assert!(
            stranger["pin_proposal"].is_null(),
            "a single deployment proposes nothing: its slots cannot be derived"
        );
    }

    /// The report joins the ledger against the live verdict table: stamps
    /// from PIN_LEDGER, instance counts from SQL, zero when a pinned family
    /// has no current deployments.
    #[test]
    fn run_bench_reports_the_pin_ledger_with_instance_counts() {
        let path = std::env::temp_dir().join(format!(
            "kascov-bench-test-{}-pinned.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = Store::open(&path, Network::Testnet(10)).unwrap();
        let conn = store.raw_conn();
        for cid in [[1u8; 32], [3u8; 32]] {
            conn.execute(
                "INSERT INTO market_programs (covenant_id, program_hash, skeleton)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![cid.as_slice(), [2u8; 32].as_slice(), "KRON curve v1"],
            )
            .unwrap();
        }
        let report = run_bench(conn).unwrap();
        let pinned = report["pinned"].as_array().expect("pinned array");
        assert_eq!(pinned.len(), PIN_LEDGER.len());
        let v1 = pinned
            .iter()
            .find(|p| p["skeleton"] == "KRON curve v1")
            .expect("curve v1 row");
        assert_eq!(v1["instances"], 2);
        assert_eq!(v1["commit"], "5859e81");
        assert_eq!(v1["date"], "2026-07-28");
        assert!(pinned
            .iter()
            .all(|p| p["first_pinned_by"] == "kascov core (Knitser)"));
        let tn_a = pinned
            .iter()
            .find(|p| p["skeleton"] == "KRON pool tn-a")
            .expect("tn-a row");
        assert_eq!(tn_a["instances"], 0);
    }
}
