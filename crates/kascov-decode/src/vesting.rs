//! Proving a creator through a KRON vesting lock.
//!
//! A vested launch mints the dev allocation into a schedule covenant instead
//! of the creator's own address, so the token's genesis owners show a covenant
//! id where every other launch shows a key. The creator did not disappear:
//! their pubkey is the first field of the lock's state, committed at genesis
//! behind a P2SH hash, and the curve that ran the launch enforced on chain
//! that this lock is the one audited vesting program at `claimed = 0`.
//!
//! That commitment is checkable the same way [`crate::kcc20::prove_output_state`]
//! checks a token cell: splice a CANDIDATE state into the known template,
//! hash, and compare against what the chain committed to. The candidate here
//! is the publisher's own claim — the listed creator key and schedule — so a
//! list can only ever prove itself right; a wrong key or a wrong schedule
//! fails to reproduce the commitment and proves nothing. Fails closed on
//! every malformed input.
//!
//! Template provenance. The prefix/suffix fixtures are the vesting program of
//! KRON template generation `3297abfdaf8e…`, compiled from the sources KRON
//! ships in its frontend with the compiler KRON ships beside them, around the
//! kcc20 `(maxIns=4, maxOuts=5)` launch-default template. Two chain anchors
//! keep that from being trust in a download: the template's hash below is the
//! `APPROVED_VESTING_TEMPLATE_HASH` baked into every current-generation curve
//! deployment on mainnet (the curve refuses any other vesting program at
//! `initVested`), and the tests reproduce a real mainnet genesis commitment
//! byte for byte from this template plus published state.
//!
//! Note the hash convention: KRON commits to `blake2b256(prefix || suffix)`
//! with NO length framing. That is deliberately NOT [`crate::kcc1`]'s
//! length-framed TemplateHash — the two conventions must never be mixed.

use crate::kcc20::blake2b_256;
use crate::p2sh_hash;
use std::collections::BTreeSet;

/// The vesting template of KRON generation `3297abfdaf8e…`. The 1-byte prefix
/// is the alt-stack guard opening the state block; the program's remaining
/// 18,177 bytes follow the 69-byte state region.
const PREFIX: &[u8] = include_bytes!("../fixtures/vesting_kron_prefix.bin");
const SUFFIX: &[u8] = include_bytes!("../fixtures/vesting_kron_suffix.bin");

/// `blake2b256(PREFIX || SUFFIX)`, plain concatenation — the value every
/// current-generation curve pins as `APPROVED_VESTING_TEMPLATE_HASH`.
pub const KRON_VESTING_TEMPLATE_HASH: [u8; 32] = [
    0x4e, 0xd8, 0x40, 0x6f, 0x89, 0x60, 0x76, 0xf0, 0x33, 0x4f, 0xf2, 0x97, 0xd0, 0x21, 0x5d, 0x60,
    0xdb, 0x16, 0x4d, 0xd8, 0x5e, 0x99, 0x5b, 0x62, 0x93, 0x91, 0xe4, 0xaf, 0x98, 0x42, 0xce, 0x18,
];

/// The lock's state block: `[0x20‖creator32][0x08‖total][0x08‖start]
/// [0x08‖duration][0x08‖claimed]`, ints 8-byte little-endian — the same
/// fixed-width serialization every KCC20 state block uses.
fn splice_state(
    creator: &[u8; 32],
    total: u64,
    start: u64,
    duration: u64,
    claimed: u64,
) -> Vec<u8> {
    let mut s = Vec::with_capacity(69);
    s.push(0x20);
    s.extend_from_slice(creator);
    for v in [total, start, duration, claimed] {
        s.push(0x08);
        s.extend_from_slice(&v.to_le_bytes());
    }
    s
}

/// The five values committed by one state of the audited KRON vesting lock.
/// All schedule coordinates are DAA scores, not timestamps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VestingState {
    pub creator: [u8; 32],
    pub total: u64,
    pub start_score: u64,
    pub duration_score: u64,
    pub claimed: u64,
}

fn program_for_state(state: &VestingState) -> Vec<u8> {
    let encoded = splice_state(
        &state.creator,
        state.total,
        state.start_score,
        state.duration_score,
        state.claimed,
    );
    let mut program = Vec::with_capacity(PREFIX.len() + encoded.len() + SUFFIX.len());
    program.extend_from_slice(PREFIX);
    program.extend_from_slice(&encoded);
    program.extend_from_slice(SUFFIX);
    program
}

/// Decode a revealed vesting program only when its fixed template bytes and
/// state markers match exactly. A merely similar program is not this build.
pub fn decode_state(program: &[u8]) -> Option<VestingState> {
    const STATE_LEN: usize = 69;
    if program.len() != PREFIX.len() + STATE_LEN + SUFFIX.len()
        || !program.starts_with(PREFIX)
        || !program.ends_with(SUFFIX)
    {
        return None;
    }
    let state = &program[PREFIX.len()..PREFIX.len() + STATE_LEN];
    if state.first().copied() != Some(0x20) {
        return None;
    }
    let creator = state.get(1..33)?.try_into().ok()?;
    let mut at = 33usize;
    let mut next_u64 = || {
        if state.get(at).copied() != Some(0x08) {
            return None;
        }
        let value = u64::from_le_bytes(state.get(at + 1..at + 9)?.try_into().ok()?);
        at += 9;
        Some(value)
    };
    let decoded = VestingState {
        creator,
        total: next_u64()?,
        start_score: next_u64()?,
        duration_score: next_u64()?,
        claimed: next_u64()?,
    };
    (at == STATE_LEN && decoded.claimed <= decoded.total && decoded.duration_score > 0)
        .then_some(decoded)
}

/// Compute the commitment carried by an output for this exact audited state.
pub fn state_commitment(state: &VestingState) -> [u8; 32] {
    blake2b_256(&program_for_state(state))
}

/// Prove that an output commits to exactly this audited template and state.
pub fn prove_state(output_spk: &[u8], state: &VestingState) -> bool {
    let Some(want) = p2sh_hash(output_spk) else {
        return false;
    };
    state_commitment(state) == want
}

/// Recover a continuation's `claimed` value from the transaction witness
/// which created it. Every small script integer is tried as a candidate, but
/// a candidate is returned only when the reconstructed full program hashes to
/// the continuation output's own P2SH commitment. Ambiguity fails closed.
pub fn recover_continuation_state(
    output_spk: &[u8],
    creator: &[u8; 32],
    total: u64,
    start_score: u64,
    duration_score: u64,
    witness: &[u8],
) -> Option<VestingState> {
    let (instructions, _) = crate::disasm::disassemble(witness);
    let mut candidates = BTreeSet::from([0u64]);
    for inst in instructions {
        let bytes = match (inst.opcode, inst.data) {
            (_, Some(data)) if data.len() <= 8 => data,
            (0x00, None) => Vec::new(),
            (op @ 0x51..=0x60, None) => vec![op - 0x50],
            _ => continue,
        };
        let mut padded = [0u8; 8];
        padded[..bytes.len()].copy_from_slice(&bytes);
        let value = u64::from_le_bytes(padded);
        if value <= total {
            candidates.insert(value);
        }
    }
    let matches: Vec<_> = candidates
        .into_iter()
        .map(|claimed| VestingState {
            creator: *creator,
            total,
            start_score,
            duration_score,
            claimed,
        })
        .filter(|state| prove_state(output_spk, state))
        .collect();
    let [only] = matches.as_slice() else {
        return None;
    };
    Some(*only)
}

/// Prove that `output_spk` — the vesting covenant's own genesis output — is
/// this template locked to `creator` on exactly the given schedule. `claimed`
/// is pinned to 0 because that is what the curve required of the genesis lock;
/// a later continuation of the covenant carries a different `claimed` and
/// deliberately does NOT prove against this function.
///
/// True means the creator key and the full schedule are chain facts. False
/// means only that THIS candidate did not reproduce THIS commitment — a
/// different template generation, a non-KRON lock, or a wrong claim all look
/// the same, which is why a caller must treat false as "unproven", never as
/// "disproven".
pub fn prove_genesis_lock(
    output_spk: &[u8],
    creator: &[u8; 32],
    total: u64,
    start_score: u64,
    duration_score: u64,
) -> bool {
    if duration_score == 0 {
        return false;
    }
    let Some(want) = p2sh_hash(output_spk) else {
        return false;
    };
    let state = VestingState {
        creator: *creator,
        total,
        start_score,
        duration_score,
        claimed: 0,
    };
    blake2b_256(&program_for_state(&state)) == want
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture is only trustworthy while it hashes to the value the
    /// on-chain curves pin. A drifted fixture must fail loudly, not prove
    /// things quietly.
    #[test]
    fn the_template_hashes_to_the_curve_pinned_value() {
        let mut cat = Vec::with_capacity(PREFIX.len() + SUFFIX.len());
        cat.extend_from_slice(PREFIX);
        cat.extend_from_slice(SUFFIX);
        assert_eq!(blake2b_256(&cat), KRON_VESTING_TEMPLATE_HASH);
    }

    /// The real mainnet vector: token ANSEM's genesis
    /// (5c6bdf6b85bf96fb43e60849e2afdce4fdc7d6134614cbc00d72e8cd292e6912)
    /// wrote its vesting lock at output 3. The published creator key and
    /// schedule, spliced into the template, must reproduce that output's
    /// P2SH commitment exactly.
    const ANSEM_LOCK_SPK: [u8; 35] = [
        0xaa, 0x20, 0x0b, 0x1f, 0xb6, 0xee, 0xd9, 0x44, 0xc6, 0xa1, 0xd3, 0x58, 0x1f, 0xaf, 0x3c,
        0x42, 0x23, 0x00, 0xf3, 0x41, 0xf8, 0x49, 0x96, 0x82, 0x34, 0x9f, 0x0b, 0x55, 0x30, 0x1d,
        0xe3, 0xcb, 0x6d, 0x38, 0x87,
    ];
    const ANSEM_CREATOR: [u8; 32] = [
        0x98, 0x8a, 0x0b, 0x5e, 0x4d, 0xc7, 0xe8, 0xa2, 0x44, 0x9d, 0x24, 0x95, 0x4b, 0x67, 0xf8,
        0x2a, 0x30, 0x90, 0x79, 0xea, 0x83, 0x0d, 0x28, 0x64, 0x28, 0x1f, 0x86, 0xff, 0xb4, 0x94,
        0xe5, 0x6a,
    ];
    const ANSEM_TOTAL: u64 = 100_000_000;
    const ANSEM_START: u64 = 499_658_470;
    const ANSEM_DURATION: u64 = 298_796_626;

    #[test]
    fn the_ansem_mainnet_lock_proves_its_listed_creator() {
        assert!(prove_genesis_lock(
            &ANSEM_LOCK_SPK,
            &ANSEM_CREATOR,
            ANSEM_TOTAL,
            ANSEM_START,
            ANSEM_DURATION,
        ));
    }

    /// A wrong key and a wrong schedule must both fail: the proof covers the
    /// ENTIRE state, so a list cannot borrow a real lock for a false claim.
    #[test]
    fn a_wrong_claim_reproduces_nothing() {
        let mut wrong_key = ANSEM_CREATOR;
        wrong_key[0] ^= 1;
        assert!(!prove_genesis_lock(
            &ANSEM_LOCK_SPK,
            &wrong_key,
            ANSEM_TOTAL,
            ANSEM_START,
            ANSEM_DURATION,
        ));
        assert!(!prove_genesis_lock(
            &ANSEM_LOCK_SPK,
            &ANSEM_CREATOR,
            ANSEM_TOTAL,
            ANSEM_START,
            ANSEM_DURATION + 1,
        ));
    }

    /// A non-P2SH spk (here, the creator's own bare-key change output from the
    /// same genesis tx) must fail shape-first, not hash-compare garbage.
    #[test]
    fn a_non_p2sh_output_fails_closed() {
        let mut bare = vec![0x20];
        bare.extend_from_slice(&ANSEM_CREATOR);
        bare.push(0xac);
        assert!(!prove_genesis_lock(
            &bare,
            &ANSEM_CREATOR,
            ANSEM_TOTAL,
            ANSEM_START,
            ANSEM_DURATION,
        ));
    }

    fn spk_for(state: &VestingState) -> Vec<u8> {
        let mut spk = vec![0xaa, 0x20];
        spk.extend_from_slice(&blake2b_256(&program_for_state(state)));
        spk.push(0x87);
        spk
    }

    #[test]
    fn a_revealed_state_round_trips_only_for_the_pinned_template() {
        let state = VestingState {
            creator: ANSEM_CREATOR,
            total: ANSEM_TOTAL,
            start_score: ANSEM_START,
            duration_score: ANSEM_DURATION,
            claimed: 12_345_678,
        };
        let program = program_for_state(&state);
        assert_eq!(decode_state(&program), Some(state));
        assert!(prove_state(&spk_for(&state), &state));

        let mut other_build = program;
        *other_build.last_mut().unwrap() ^= 1;
        assert_eq!(decode_state(&other_build), None);
    }

    #[test]
    fn a_live_continuation_is_recovered_only_by_commitment_match() {
        let state = VestingState {
            creator: ANSEM_CREATOR,
            total: ANSEM_TOTAL,
            start_score: ANSEM_START,
            duration_score: ANSEM_DURATION,
            claimed: 25_000_000,
        };
        let mut witness = vec![0x08];
        witness.extend_from_slice(&state.claimed.to_le_bytes());
        assert_eq!(
            recover_continuation_state(
                &spk_for(&state),
                &state.creator,
                state.total,
                state.start_score,
                state.duration_score,
                &witness,
            ),
            Some(state)
        );

        witness[1] ^= 1;
        assert_eq!(
            recover_continuation_state(
                &spk_for(&state),
                &state.creator,
                state.total,
                state.start_score,
                state.duration_score,
                &witness,
            ),
            None
        );
    }

    #[test]
    fn a_zero_duration_schedule_is_never_admitted() {
        let state = VestingState {
            creator: ANSEM_CREATOR,
            total: ANSEM_TOTAL,
            start_score: ANSEM_START,
            duration_score: 0,
            claimed: 0,
        };
        assert!(!prove_genesis_lock(
            &spk_for(&state),
            &state.creator,
            state.total,
            state.start_score,
            state.duration_score,
        ));
    }
}
