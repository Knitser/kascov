//! vprogs on L1: what a settlement and an exit claim say, read from the
//! transaction alone.
//!
//! A vprog (kaspanet/vprogs) settles its lane through a covenant. Each
//! settlement spends the covenant's cell and writes the next one; its
//! witness proves a RISC Zero receipt (OpZkPrecompile) and ends with the new
//! lane tip, the new state root, the L1 chain block the batch proves to, and
//! the redeem program itself. When a batch lets funds leave the lane, the
//! settlement also opens an exit permission, spent later by one exit claim
//! per exit. Layouts follow kaspanet/vprogs (zk/backend/risc0/covenant
//! src/settlement.rs and src/script.rs, zk/backend/risc0/api
//! src/permission_script.rs) and were checked against the tic-tac-toe
//! vprog's own spends on testnet-10.
//!
//! Nothing here verifies a proof. It reads what the transaction states, and
//! names what that is.

/// The settlement build this crate pins (see observed.rs).
pub const SETTLEMENT_FAMILY: &str = "vprogs · tic-tac-toe settlement";
/// The depth-1 exit permission this crate pins (see observed.rs).
pub const EXIT_PERMISSION_FAMILY: &str = "vprogs · exit permission (depth 1)";

/// A real settlement program of the pinned build. Every settlement of this
/// build is these bytes with a different lane tip and state in its prefix.
const SETTLEMENT: &[u8] = include_bytes!("../fixtures/vprogs_ttt_settlement_a.bin");
const PREFIX_LEN: usize = 99;

/// The 20-byte subnetwork id of the coinbase, whose lane key consensus
/// precomputes (rusty-kaspa consensus/seq-commit, COINBASE_LANE_KEY).
pub const SUBNETWORK_COINBASE: [u8; 20] = {
    let mut s = [0u8; 20];
    s[0] = 1;
    s
};

/// What one settlement moved the lane from and to.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Settlement {
    #[serde(with = "hex32")]
    pub prev_lane_tip: [u8; 32],
    #[serde(with = "hex32")]
    pub prev_state: [u8; 32],
    /// The lane this vprog settles: `lane_key(subnetwork)` of its L2 traffic.
    #[serde(with = "hex32")]
    pub lane_key: [u8; 32],
    #[serde(with = "hex32")]
    pub new_lane_tip: [u8; 32],
    #[serde(with = "hex32")]
    pub new_state: [u8; 32],
    /// The L1 chain block whose sequencing commitment the batch proves to.
    #[serde(with = "hex32")]
    pub block_prove_to: [u8; 32],
    /// Zero when the batch credited no deposits.
    #[serde(with = "hex32")]
    pub deposit_spk_hash: [u8; 32],
    /// Bytes of the proof push (the RISC Zero seal, or a Groth16 proof).
    pub proof_len: usize,
}

impl Settlement {
    pub fn state_changed(&self) -> bool {
        self.prev_state != self.new_state
    }
    pub fn credited_deposits(&self) -> bool {
        self.deposit_spk_hash != [0u8; 32]
    }
}

/// One exit claimed against an exit permission.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ExitClaim {
    #[serde(with = "hex32")]
    pub exit_root: [u8; 32],
    pub unclaimed_before: u64,
    /// The exit holder's script (for a plain key, `0x20 <key> OP_CHECKSIG`).
    #[serde(with = "hex_vec")]
    pub leaf_spk: Vec<u8>,
    /// The exit's full amount, in sompi.
    pub amount: u64,
    /// What this claim takes out of it, in sompi.
    pub deduct: u64,
    /// The tree's depth, from the witness's proof pushes.
    pub depth: usize,
}

impl ExitClaim {
    /// True when this claim empties the exit and it was the last one left:
    /// no permission continues after it.
    pub fn is_final(&self) -> bool {
        self.unclaimed_before == 1 && self.deduct == self.amount
    }
}

/// The payloads of a signature script that is nothing but pushes, in order.
/// OP_0 pushes the empty item and OP_1..OP_16 / OP_1NEGATE a one-byte number,
/// as the engine reads them (an exit claim's path directions are OP_0 or
/// OP_1).
fn pushes(sig: &[u8]) -> Option<Vec<Vec<u8>>> {
    let (instructions, truncated) = crate::disasm::disassemble(sig);
    if truncated {
        return None;
    }
    instructions
        .into_iter()
        .map(|i| match i.opcode {
            0x00 => Some(Vec::new()),
            0x4f => Some(vec![0x81]),
            op @ 0x51..=0x60 => Some(vec![op - 0x50]),
            _ => i.data,
        })
        .collect()
}

fn arr32(b: &[u8]) -> Option<[u8; 32]> {
    b.try_into().ok()
}

/// True when `redeem` is a settlement of the pinned build: the pinned
/// program with some lane tip and state in its prefix.
pub fn is_settlement(redeem: &[u8]) -> bool {
    redeem.len() == SETTLEMENT.len()
        && redeem[0] == 0x20
        && redeem[33] == 0x20
        && redeem[66..] == SETTLEMENT[66..]
}

/// A settlement, from the program it spent and the signature script that
/// spent it. Pushes are read from the end, where both proof systems vprogs
/// supports agree: `… proof, deposit_spk_hash, new_lane_tip, new_state,
/// block_prove_to, redeem`.
pub fn parse_settlement(redeem: &[u8], sig: &[u8]) -> Option<Settlement> {
    if !is_settlement(redeem) {
        return None;
    }
    let p = pushes(sig)?;
    if p.len() < 6 || p[p.len() - 1] != redeem {
        return None;
    }
    let at = |from_end: usize| &p[p.len() - from_end];
    Some(Settlement {
        prev_lane_tip: arr32(&redeem[1..33])?,
        prev_state: arr32(&redeem[34..66])?,
        lane_key: arr32(&redeem[67..PREFIX_LEN])?,
        block_prove_to: arr32(at(2))?,
        new_state: arr32(at(3))?,
        new_lane_tip: arr32(at(4))?,
        deposit_spk_hash: arr32(at(5))?,
        proof_len: at(6).len(),
    })
}

/// A settlement of the pinned build, from a signature script alone: its
/// redeem is its last push, 561 bytes behind OP_PUSHDATA2, so the tail says
/// whether it can be one before anything is parsed. The follower runs this on
/// every input it walks.
pub fn settlement_in(sig: &[u8]) -> Option<Settlement> {
    let len = SETTLEMENT.len();
    let head = sig.len().checked_sub(len + 3)?;
    if sig[head] != 0x4d || sig[head + 1..head + 3] != (len as u16).to_le_bytes() {
        return None;
    }
    let redeem = &sig[head + 3..];
    if !is_settlement(redeem) {
        return None;
    }
    parse_settlement(redeem, sig)
}

/// The program the next cell of a settlement locks: the same build with the
/// new lane tip and state in its prefix. Its blake2b-256 is the P2SH hash of
/// the settlement's output 0, which is how a settlement chain is checked.
pub fn next_redeem(redeem: &[u8], new_lane_tip: &[u8; 32], new_state: &[u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(redeem.len());
    out.push(0x20);
    out.extend_from_slice(new_lane_tip);
    out.push(0x20);
    out.extend_from_slice(new_state);
    out.extend_from_slice(&redeem[66..]);
    out
}

/// True when `redeem` is a depth-1 exit permission: `OpData32 root,
/// OpData8 unclaimed` and the fixed body.
pub fn is_exit_permission(redeem: &[u8]) -> bool {
    const PERMISSION: &[u8] = include_bytes!("../fixtures/vprogs_exit_perm_d1_a.bin");
    redeem.len() == PERMISSION.len() && redeem[0] == 0x20 && redeem[33] == 0x08 && redeem[42..] == PERMISSION[42..]
}

/// An exit claim, from the permission it spent and the signature script that
/// spent it. Read from the end: redeem, deduct (a script number), amount
/// (8 bytes LE), the exit holder's script, then a sibling and a direction for
/// each level of the old tree and of the new one.
pub fn parse_exit_claim(redeem: &[u8], sig: &[u8]) -> Option<ExitClaim> {
    if !is_exit_permission(redeem) {
        return None;
    }
    let p = pushes(sig)?;
    if p.len() < 4 || p[p.len() - 1] != redeem {
        return None;
    }
    let at = |from_end: usize| &p[p.len() - from_end];
    let deduct = crate::argent::script_num(at(2)).filter(|&d| d > 0)? as u64;
    let amount = u64::from_le_bytes(at(3).as_slice().try_into().ok()?);
    let proof_pushes = p.len() - 4;
    if proof_pushes % 4 != 0 || deduct > amount {
        return None;
    }
    Some(ExitClaim {
        exit_root: arr32(&redeem[1..33])?,
        unclaimed_before: u64::from_le_bytes(redeem[34..42].try_into().ok()?),
        leaf_spk: at(4).clone(),
        amount,
        deduct,
        depth: proof_pushes / 4,
    })
}

/// Lanes kascov can name. A lane key is a hash, so its subnetwork is never
/// read back from it, only found by recomputing candidates: the tic-tac-toe
/// vprog's lane is namespace subnetwork 865832ec (lane id 2253927148 as
/// `SubnetworkId::from_namespace`), found by searching every 4-byte
/// namespace, and its game transactions travel on exactly that subnetwork.
const KNOWN_LANES: [[u8; 20]; 1] = [{
    let mut s = [0u8; 20];
    s[0] = 0x86;
    s[1] = 0x58;
    s[2] = 0x32;
    s[3] = 0xec;
    s
}];

/// The subnetwork whose lane key is `lane_key`, if it is one kascov knows.
pub fn known_lane(lane_key: &[u8; 32]) -> Option<[u8; 20]> {
    KNOWN_LANES.into_iter().find(|s| &self::lane_key(s) == lane_key)
}

/// The P2SH hash of a vprog's delegate-entry script, the address deposits
/// into the covenant pay: `blake2b(PREFIX || covenant_id || SUFFIX)`, as
/// vprogs' api `delegate_entry_spk_hash` builds it. A settlement whose
/// deposit hash equals this credited deposits made to this very covenant.
pub fn delegate_spk_hash(covenant_id: &[u8; 32]) -> [u8; 32] {
    const PREFIX: [u8; 7] = [0xb9, 0x00, 0xa0, 0x69, 0x00, 0xcf, 0x20];
    const SUFFIX: [u8; 14] = [0x88, 0x00, 0x00, 0xc9, 0x76, 0x52, 0x94, 0x7c, 0xbc, 0x02, 0x51, 0x75, 0x88, 0x51];
    let mut script = Vec::with_capacity(PREFIX.len() + 32 + SUFFIX.len());
    script.extend_from_slice(&PREFIX);
    script.extend_from_slice(covenant_id);
    script.extend_from_slice(&SUFFIX);
    let hash = blake2b_simd::Params::new().hash_length(32).hash(&script);
    hash.as_bytes().try_into().expect("32-byte hash")
}

/// An exit claim, from a signature script alone: like a settlement's, its
/// last push is the program it spends (489 bytes, behind OP_PUSHDATA2), so
/// the tail says whether it can be one before anything is parsed.
pub fn exit_claim_in(sig: &[u8]) -> Option<ExitClaim> {
    const LEN: usize = 489;
    let head = sig.len().checked_sub(LEN + 3)?;
    if sig[head] != 0x4d || sig[head + 1..head + 3] != (LEN as u16).to_le_bytes() {
        return None;
    }
    parse_exit_claim(&sig[head + 3..], sig)
}

/// The lane key of a subnetwork: BLAKE3 keyed with the domain
/// "SeqCommitLaneKey" (zero-padded to 32 bytes) over the 20-byte subnetwork
/// id, as rusty-kaspa's consensus/seq-commit `lane_key` computes it. A vprog
/// settlement pins its lane key at bytes 67..99, so this is what ties the
/// lane's L2 transactions on L1 to the covenant that settles them.
pub fn lane_key(subnetwork: &[u8; 20]) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[..16].copy_from_slice(b"SeqCommitLaneKey");
    *blake3::keyed_hash(&key, subnetwork).as_bytes()
}

mod hex32 {
    pub fn serialize<S: serde::Serializer>(v: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(v))
    }
}
mod hex_vec {
    pub fn serialize<S: serde::Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &[u8] = include_bytes!("../fixtures/vprogs_ttt_settlement_a.bin");
    const B: &[u8] = include_bytes!("../fixtures/vprogs_ttt_settlement_b.bin");
    const PERM_A: &[u8] = include_bytes!("../fixtures/vprogs_exit_perm_d1_a.bin");
    const PERM_C: &[u8] = include_bytes!("../fixtures/vprogs_exit_perm_d1_c.bin");

    fn push(out: &mut Vec<u8>, data: &[u8]) {
        match data.len() {
            0 => out.push(0x00),
            n if n <= 75 => { out.push(n as u8); out.extend_from_slice(data); }
            n if n <= 255 => { out.push(0x4c); out.push(n as u8); out.extend_from_slice(data); }
            n if n <= 65535 => { out.push(0x4d); out.extend_from_slice(&(n as u16).to_le_bytes()); out.extend_from_slice(data); }
            n => { out.push(0x4e); out.extend_from_slice(&(n as u32).to_le_bytes()); out.extend_from_slice(data); }
        }
    }

    #[test]
    fn a_settlement_chain_links_on_real_bytes() {
        // Transaction 5947b00f… spent the cell holding program b and created
        // the one holding program a, so a is b's next program.
        let tip: [u8; 32] = A[1..33].try_into().unwrap();
        let state: [u8; 32] = A[34..66].try_into().unwrap();
        assert_eq!(next_redeem(B, &tip, &state), A);
        let hash = blake2b_simd::Params::new().hash_length(32).hash(A);
        assert_eq!(hex::encode(hash.as_bytes()), "929ea6106c657debb3ee53ab4fd9c7b1e019671837f2a2a9a341ce04fdba0125");
        assert!(is_settlement(A) && is_settlement(B));
        let mut other = A.to_vec();
        other[300] ^= 1;
        assert!(!is_settlement(&other), "a different body is a different build");
    }

    #[test]
    fn a_settlement_witness_reads_from_the_end() {
        let tip: [u8; 32] = A[1..33].try_into().unwrap();
        let state: [u8; 32] = A[34..66].try_into().unwrap();
        let block = [0xb7u8; 32];
        let mut sig = Vec::new();
        push(&mut sig, &[0x11; 32]); // claim
        push(&mut sig, &[0x16]); // control index
        push(&mut sig, &[0x22; 256]); // control digests
        push(&mut sig, &vec![0x33; 1000]); // the seal, cut short
        push(&mut sig, &[0; 32]); // no deposits
        push(&mut sig, &tip);
        push(&mut sig, &state);
        push(&mut sig, &block);
        push(&mut sig, B);
        let s = parse_settlement(B, &sig).unwrap();
        assert_eq!((s.prev_lane_tip.as_slice(), s.prev_state.as_slice()), (&B[1..33], &B[34..66]));
        assert_eq!((s.new_lane_tip, s.new_state, s.block_prove_to), (tip, state, block));
        assert_eq!(s.proof_len, 1000);
        assert!(s.state_changed() && !s.credited_deposits());
        assert_eq!(next_redeem(B, &s.new_lane_tip, &s.new_state), A);
        // a witness that does not end with the program it spends is not one
        let mut wrong = sig.clone();
        wrong.truncate(wrong.len() - B.len() - 3);
        push(&mut wrong, A);
        assert!(parse_settlement(B, &wrong).is_none());
    }

    #[test]
    fn a_settlement_is_found_from_its_signature_script_alone() {
        let tip: [u8; 32] = A[1..33].try_into().unwrap();
        let state: [u8; 32] = A[34..66].try_into().unwrap();
        let mut sig = Vec::new();
        for p in [&[0x11u8; 32][..], &[0x16], &[0x22; 64], &[0x33; 600], &[0; 32], &tip, &state, &[0xb7; 32], B] {
            push(&mut sig, p);
        }
        let s = settlement_in(&sig).unwrap();
        assert_eq!(s.new_state, state);
        assert!(settlement_in(&sig[..sig.len() - 1]).is_none(), "a cut tail is nothing");
        assert!(settlement_in(&[0x4d, 0x31, 0x02]).is_none());
        let mut other = sig.clone();
        let at = other.len() - 200;
        other[at] ^= 1;
        assert!(settlement_in(&other).is_none(), "another build is not this one");
    }

    #[test]
    fn the_lane_key_matches_consensus_and_the_pinned_lane() {
        assert_eq!(
            hex::encode(lane_key(&SUBNETWORK_COINBASE)),
            "8aa78027db66a16cb69692ee0af5cb76738ef80ad14c9d13920d7fa3cc40b9e4"
        );
        let mut subnet = [0u8; 20];
        subnet[..4].copy_from_slice(&[0x86, 0x58, 0x32, 0xec]);
        assert_eq!(lane_key(&subnet).as_slice(), &A[67..99], "the tic-tac-toe lane, 865832ec…");
        let pinned: [u8; 32] = A[67..99].try_into().unwrap();
        assert_eq!(known_lane(&pinned), Some(subnet));
        assert_eq!(known_lane(&[0x5e; 32]), None);
    }

    #[test]
    fn the_delegate_hash_is_the_one_a_real_settlement_credited() {
        // 4c2ab3c7… credited deposits to c11eb9fe…, the delegate of the
        // tic-tac-toe vprog's covenant 85165a6f….
        let cov: [u8; 32] = hex::decode("85165a6f453dbbd1e968c238966e297046b8f1d22c00fee1bc208de00c3fb097")
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(
            hex::encode(delegate_spk_hash(&cov)),
            "c11eb9fe2cc4440045e48b2bc92e1860fb78cf4b87a83caeced660116c34ea5d"
        );
    }

    #[test]
    fn an_exit_claim_reads_the_real_first_claim() {
        // d9a42cb5…'s witness: the old and new tree paths (sibling 532f69…,
        // direction 0, twice), the holder's key script, amount 1 KAS, deduct
        // 1 KAS, and the permission it spends (2 exits unclaimed).
        let sibling = hex::decode("532f698848d02647df717283ceea1b1b06e93823ff6a0b570c65916e5b31740d").unwrap();
        let holder = hex::decode("20151c80fed36c7d7692cdf24567f25551ac8da5054a639c1ce2c1e723b39575d5ac").unwrap();
        let mut sig = Vec::new();
        push(&mut sig, &sibling);
        push(&mut sig, &[]);
        push(&mut sig, &sibling);
        push(&mut sig, &[]);
        push(&mut sig, &holder);
        push(&mut sig, &100_000_000u64.to_le_bytes());
        push(&mut sig, &hex::decode("00e1f505").unwrap());
        push(&mut sig, PERM_A);
        let c = parse_exit_claim(PERM_A, &sig).unwrap();
        assert_eq!(exit_claim_in(&sig).as_ref(), Some(&c), "found from the witness alone");
        assert!(exit_claim_in(&sig[..sig.len() - 1]).is_none());
        assert!(exit_claim_in(PERM_A).is_none(), "a program is not a witness");
        assert_eq!(c.unclaimed_before, 2);
        assert_eq!((c.amount, c.deduct, c.depth), (100_000_000, 100_000_000, 1));
        assert_eq!(c.leaf_spk, holder);
        assert_eq!(hex::encode(c.exit_root), "79dc253cf54c1b275ccb22bc9018ba6bf3e3cdd0e8d91136c54a5f83b076c2d6");
        assert!(!c.is_final(), "one exit is still unclaimed after it");
        assert!(is_exit_permission(PERM_C), "the third real permission, not used to derive the pin");
        assert!(!is_settlement(PERM_A) && !is_exit_permission(A));
    }
}
