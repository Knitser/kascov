//! KCC-0020 conformance primitives — the merged token convention
//! (kaspanet/kccs, Draft at commit ea5176aa), byte layouts only.
//!
//! Everything here is verifiable from bytes plus one declared input, a pin
//! (kascov's committed description of a program, keyed by its KCC-1 template
//! hash). Nothing is wired into the indexer yet, and nothing may publish a
//! balance: recognising a shape is not proving a state.
//!
//! Five programs carry this layout on chain: kascov's own reduced vector
//! and four generations of ZealousSwap's token cell (all pinned below, each
//! read and exercised on the engine before it was). Every other
//! deployed token (KRON, KaspaCom, KaspaKaha) uses the gist-era 46-byte state that
//! `kcc20.rs` reads; the spec state is six fields, 112 bytes encoded, with
//! different scheme bytes. No shipped legacy build carries a decodable spec
//! window (tests check every fixture, both directions), and a window alone
//! never claims a program anyway: only reconstruction against the family
//! outputs does, and that is the part an adversary cannot forge.
//!
//! The one structural gift of the spec: output states are PROVEN, not
//! recovered. The leader's `transfer` arguments declare every successor
//! state, and `Blake2b(prefix ‖ EncodeState(next_states[i]) ‖ suffix)` must
//! equal the i-th covenant-family output's P2SH commitment. So the reader
//! reconstructs and compares, and the witness brute-force the 46-byte layout
//! needs never runs here.

use crate::encode_push;
use crate::kcc1;

use serde::Deserialize;
use std::sync::OnceLock;

/// The kccs commit whose text defines every rule in this module. A spec
/// change re-derives everything; fixtures are regenerated, never patched.
pub const SPEC_COMMIT: &str = "ea5176aa";

/// The template name a pinned KCC-0020 build is stamped with when its pin
/// declares no family of its own, everywhere a legacy build says
/// "KCC20 token". A pin that declares a `family` stamps that instead (see
/// [`Pin::family_label`]); [`is_spec_template`] recognises either.
pub const SPEC_TOKEN_TEMPLATE: &str = "KCC-0020 token";

/// The default configuration's entrypoint signatures (§3) written under
/// KCC-1 §6.1, and their dispatch tags. §6.1 expands a record to its
/// ordered field types with the record's name omitted, so `KCC20State[]`
/// is `{int,byte[32],byte,byte,byte[32],byte[32]}[]` in the signature the
/// tag is hashed from; the spec's own `transfer(KCC20State[],byte[])` is
/// the readable form, not the preimage. A non-default configuration
/// renames the entrypoints; which encoding a real program uses (KCC-1 tag
/// or the fork silverc's numeric selector) is pin knowledge, never
/// inferred from bytes.
pub const TRANSFER_SIGNATURE: &str =
    "transfer({int,byte[32],byte,byte,byte[32],byte[32]}[],byte[])";
pub const DELEGATOR_SIGNATURE: &str = "transfer_delegator(byte[])";

pub fn transfer_tag() -> [u8; 4] {
    kcc1::dispatch_tag(TRANSFER_SIGNATURE)
}

pub fn delegator_tag() -> [u8; 4] {
    kcc1::dispatch_tag(DELEGATOR_SIGNATURE)
}

/// Owner scheme bytes (§3). NOT the gist-era bytes: legacy 0x03 means
/// presence, spec 0x03 means p2sh; legacy 0x02 is covenant-id, spec 0x04 is.
/// Every consumer must key on the token's layout before reading these.
pub const OWNER_P2PK_SCHNORR: u8 = 0x00;
pub const OWNER_P2PKH_SCHNORR: u8 = 0x01;
pub const OWNER_P2PKH_ECDSA: u8 = 0x02;
pub const OWNER_P2SH: u8 = 0x03;
pub const OWNER_COVENANT_ID: u8 = 0x04;

/// The KCC20State record (§1), in ABI field order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    /// Token quantity in base units. Non-negative by construction: decode
    /// rejects a set §5.3 sign bit, negative zero included.
    pub amount: i64,
    /// Owner bytes, interpreted per `owner_scheme` (KCC-2).
    pub owner: [u8; 32],
    pub owner_scheme: u8,
    /// Borrow rule selector (§5); `borrow_guard` is scheme-specific.
    pub borrow_scheme: u8,
    pub borrow_guard: [u8; 32],
    /// 32-byte digest of the extended state — the fungibility class. The
    /// standard transfer preserves it and refuses to consolidate across
    /// values; the value's meaning is artifact knowledge, never guessed.
    pub extension_commitment: [u8; 32],
}

/// The §8.1 encoded width: six `PushExplicit` fields,
/// `(1+8) + (1+32) + (1+1) + (1+1) + (1+32) + (1+32)`.
pub const STATE_LEN: usize = 112;

/// DERIVED from the field widths, not spec text: the push opcode each field
/// must open with, at its fixed offset inside the encoded state.
const OPCODE_AT: [(usize, u8); 6] = [
    (0, 0x08),
    (9, 0x20),
    (42, 0x01),
    (44, 0x01),
    (46, 0x20),
    (79, 0x20),
];

impl State {
    /// The canonical 112-byte encoding. `None` for a negative amount, which
    /// no valid state carries.
    pub fn encode(&self) -> Option<[u8; STATE_LEN]> {
        if self.amount < 0 {
            return None;
        }
        let mut out = Vec::with_capacity(STATE_LEN);
        out.extend_from_slice(&kcc1::push_explicit(&kcc1::encode_state_int(self.amount)?));
        out.extend_from_slice(&kcc1::push_explicit(&self.owner));
        out.extend_from_slice(&kcc1::push_explicit(&[self.owner_scheme]));
        out.extend_from_slice(&kcc1::push_explicit(&[self.borrow_scheme]));
        out.extend_from_slice(&kcc1::push_explicit(&self.borrow_guard));
        out.extend_from_slice(&kcc1::push_explicit(&self.extension_commitment));
        out.try_into().ok()
    }

    /// Strict §8.1 decode: exactly 112 bytes, the six explicit pushes at
    /// their fixed offsets, nothing else. A set amount sign bit (negative
    /// zero included) is rejected, never absolute-valued.
    pub fn decode(bytes: &[u8]) -> Option<State> {
        if bytes.len() != STATE_LEN {
            return None;
        }
        for (at, op) in OPCODE_AT {
            if bytes[at] != op {
                return None;
            }
        }
        let amount_payload: [u8; 8] = bytes[1..9].try_into().ok()?;
        if amount_payload[7] & 0x80 != 0 {
            return None;
        }
        Some(State {
            amount: kcc1::decode_state_int(&amount_payload)?,
            owner: bytes[10..42].try_into().ok()?,
            owner_scheme: bytes[43],
            borrow_scheme: bytes[45],
            borrow_guard: bytes[47..79].try_into().ok()?,
            extension_commitment: bytes[80..112].try_into().ok()?,
        })
    }

    /// The class this cell belongs to, as the accounting layer will key it:
    /// the commitment's lowercase hex, exactly as the chain carries it.
    /// Never collapsed — an all-zero digest is a class like any other,
    /// because the spec defines no null class.
    pub fn class(&self) -> String {
        hex::encode(self.extension_commitment)
    }
}

/// A template cut: the immutable bytes around the state window. The KCC-1
/// name of the template is `hash()`; the P2SH commitment a successor must
/// satisfy is `reconstruct_spk`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    pub prefix: Vec<u8>,
    pub suffix: Vec<u8>,
}

impl Template {
    /// Cut `program` at `state_start`; `None` when 112 state bytes don't fit
    /// or don't decode there.
    pub fn cut(program: &[u8], state_start: usize) -> Option<Template> {
        let end = state_start.checked_add(STATE_LEN)?;
        State::decode(program.get(state_start..end)?)?;
        Some(Template {
            prefix: program[..state_start].to_vec(),
            suffix: program[end..].to_vec(),
        })
    }

    /// KCC-1 §8.3 TemplateHash (BLAKE3) — the template's canonical name.
    pub fn hash(&self) -> [u8; 32] {
        kcc1::template_hash(&self.prefix, &self.suffix)
    }

    /// The program this template yields for `state`.
    pub fn program(&self, state: &State) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(self.prefix.len() + STATE_LEN + self.suffix.len());
        out.extend_from_slice(&self.prefix);
        out.extend_from_slice(&state.encode()?);
        out.extend_from_slice(&self.suffix);
        Some(out)
    }

    /// The version-0 P2SH script public key a continuation carrying `state`
    /// must be locked to: `aa20 Blake2b(prefix ‖ EncodeState ‖ suffix) 87`.
    pub fn reconstruct_spk(&self, state: &State) -> Option<Vec<u8>> {
        Some(kcc1::envelope_spk(&self.program(state)?))
    }
}

/// Every offset where `program` carries a decodable 112-byte state. A match
/// is a CANDIDATE cut, nothing more: only reconstruction confirms it
/// ([`confirm_cut`]), because state bytes can imitate the pattern by chance
/// and the true `state.start` is artifact knowledge (K1 §8.2).
pub fn locate_state_cuts(program: &[u8]) -> Vec<usize> {
    if program.len() < STATE_LEN {
        return Vec::new();
    }
    (0..=program.len() - STATE_LEN)
        .filter(|&at| State::decode(&program[at..at + STATE_LEN]).is_some())
        .collect()
}

/// Prove the leader's declared successors against the covenant family's
/// actual outputs, in family output order (the i-th OUTPUT carrying the
/// covenant id — interleaved non-family outputs shift nothing). True only
/// when the counts match and every reconstruction equals the output's SPK
/// byte for byte. The caller MUST pass version-0 output scripts only (K1
/// §7); `check_family` enforces that with [`FamilyOutput::spk_version`].
///
/// What a `true` proves, exactly: UNDER THE GIVEN TEMPLATE, the declared
/// states are what the outputs commit to. Whether the template is a
/// conformant KCC-0020 program at all is pin knowledge; an arbitrary script
/// with a decodable 112-byte window and cooperating outputs reconstructs
/// too, and only the pin's reviewed bytecode turns candidate into token.
pub fn prove_next_states(template: &Template, next_states: &[State], family_spks: &[Vec<u8>]) -> bool {
    next_states.len() == family_spks.len()
        && next_states.iter().zip(family_spks).all(|(state, spk)| {
            template
                .reconstruct_spk(state)
                .is_some_and(|got| &got == spk)
        })
}

/// Find THE cut of `program` under which every declared successor
/// reconstructs to its family output. Exactly one candidate may confirm;
/// zero or several is `None`, and the caller must treat the program as
/// unknown, never as "KCC-0020 with an odd layout". A `Some` names a
/// CANDIDATE template, nothing more — classification as a KCC-0020 token
/// still requires the pin.
pub fn confirm_cut(
    program: &[u8],
    next_states: &[State],
    family_spks: &[Vec<u8>],
) -> Option<(usize, Template)> {
    if next_states.is_empty() {
        // nothing to prove against proves nothing
        return None;
    }
    let mut confirmed = None;
    for at in locate_state_cuts(program) {
        let template = Template::cut(program, at)?;
        if prove_next_states(&template, next_states, family_spks) {
            if confirmed.is_some() {
                return None;
            }
            confirmed = Some((at, template));
        }
    }
    confirmed
}

/// `transfer(KCC20State[] next_states, byte[] witness)` — the leader's
/// argument payloads. A `KCC20State[]` lowers field-major (K1 §5.6): six
/// grouped pushes, one per field, each the concatenation of that field's
/// fixed payloads across all elements, every count equal; then the witness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferArgs {
    pub next_states: Vec<State>,
    pub witness: Vec<u8>,
}

/// The grouped-field payload widths, in ABI order.
const FIELD_WIDTHS: [usize; 6] = [8, 32, 1, 1, 32, 32];

/// Far above any real family (bounds default to 3) and far below anything a
/// consensus-valid push could smuggle in — an allocation guard, not policy.
const MAX_SOA_ELEMENTS: usize = 4096;

impl TransferArgs {
    /// Read the seven argument payloads (six grouped fields, then witness).
    /// The element count comes from the amounts group and every group must
    /// agree; a count mismatch, a torn payload, or a negative amount is
    /// `None`. `n = 0` is legal (a transfer that only burns KAS cannot
    /// exist, but the encoding allows the empty family and the verdict
    /// layer, not the codec, judges it).
    pub fn from_args(args: &[Vec<u8>]) -> Option<TransferArgs> {
        let [amounts, owners, owner_schemes, borrow_schemes, guards, commitments, witness] = args
        else {
            return None;
        };
        if amounts.len() % 8 != 0 {
            return None;
        }
        let n = amounts.len() / 8;
        // consensus push limits bound real inputs long before this, but the
        // codec must not allocate unboundedly on hostile bytes either
        if n > MAX_SOA_ELEMENTS {
            return None;
        }
        for (payload, width) in [
            (owners, 32),
            (owner_schemes, 1),
            (borrow_schemes, 1),
            (guards, 32),
            (commitments, 32),
        ] {
            if payload.len() != n * width {
                return None;
            }
        }
        let mut next_states = Vec::with_capacity(n);
        for i in 0..n {
            let amount_payload: [u8; 8] = amounts[i * 8..(i + 1) * 8].try_into().ok()?;
            if amount_payload[7] & 0x80 != 0 {
                return None;
            }
            next_states.push(State {
                amount: kcc1::decode_state_int(&amount_payload)?,
                owner: owners[i * 32..(i + 1) * 32].try_into().ok()?,
                owner_scheme: owner_schemes[i],
                borrow_scheme: borrow_schemes[i],
                borrow_guard: guards[i * 32..(i + 1) * 32].try_into().ok()?,
                extension_commitment: commitments[i * 32..(i + 1) * 32].try_into().ok()?,
            });
        }
        Some(TransferArgs {
            next_states,
            witness: witness.clone(),
        })
    }

    /// The seven PushMinimal-encoded argument pushes, ready to sit ahead of
    /// the dispatch element in a signature script. Inverse of [`Self::from_args`].
    /// `None` for a negative amount.
    pub fn encode(&self) -> Option<Vec<u8>> {
        let mut groups: [Vec<u8>; 6] = Default::default();
        for state in &self.next_states {
            if state.amount < 0 {
                return None;
            }
            groups[0].extend_from_slice(&kcc1::encode_state_int(state.amount)?);
            groups[1].extend_from_slice(&state.owner);
            groups[2].push(state.owner_scheme);
            groups[3].push(state.borrow_scheme);
            groups[4].extend_from_slice(&state.borrow_guard);
            groups[5].extend_from_slice(&state.extension_commitment);
        }
        let mut out = Vec::new();
        for group in &groups {
            out.extend_from_slice(&encode_push(group));
        }
        out.extend_from_slice(&encode_push(&self.witness));
        Some(out)
    }

    /// Sanity check the widths line up with the lowering this module assumes.
    pub const fn field_widths() -> [usize; 6] {
        FIELD_WIDTHS
    }
}

/// `transfer_delegator(byte[] witness)` — a delegator authenticates only its
/// local owner; its witness has no path prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelegatorArgs {
    pub witness: Vec<u8>,
}

impl DelegatorArgs {
    pub fn from_args(args: &[Vec<u8>]) -> Option<DelegatorArgs> {
        let [witness] = args else { return None };
        Some(DelegatorArgs {
            witness: witness.clone(),
        })
    }

    /// The single PushMinimal-encoded witness push. Inverse of [`Self::from_args`].
    pub fn encode(&self) -> Vec<u8> {
        encode_push(&self.witness)
    }
}

/// The leader's path, selected by the first witness byte (§2). Anything
/// other than 0x00/0x01 is invalid by the text, so it decodes to `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WitnessPath<'a> {
    /// 0x00 — the rest authenticates the leader's owner per its scheme.
    Normal(&'a [u8]),
    /// 0x01 — the rest is borrow-authorization data per the borrow scheme.
    Borrowed(&'a [u8]),
}

pub fn witness_path(witness: &[u8]) -> Option<WitnessPath<'_>> {
    match witness.split_first()? {
        (0x00, rest) => Some(WitnessPath::Normal(rest)),
        (0x01, rest) => Some(WitnessPath::Borrowed(rest)),
        _ => None,
    }
}

/// The default configuration's owner-authentication shape (§2). Structure
/// only — signatures are consensus's to verify — EXCEPT the p2pkh binding,
/// which is byte-provable here: the revealed pubkey must hash to the owner
/// under KCC-2's keyed `P2PKHHash`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnerAuth {
    /// p2pk-schnorr/v1: a 65-byte transaction signature; the owner bytes are
    /// the pubkey itself.
    P2pkSignature,
    /// p2pkh-schnorr/v1: the revealed 32-byte pubkey (hash-checked against
    /// the owner) then a 65-byte signature.
    P2pkhSchnorr { pubkey: [u8; 32] },
    /// p2pkh-ecdsa/v1: the revealed 33-byte compressed pubkey (hash-checked)
    /// then a 65-byte signature.
    P2pkhEcdsa { pubkey: [u8; 33] },
    /// p2sh/v1: one unsigned byte naming the authority input index.
    P2shAuthorityInput(u8),
    /// covenant-id/v1: empty; approval is the co-spend itself.
    CovenantId,
}

/// Check the owner-auth bytes of a NORMAL path against `state`. `None` for a
/// wrong shape, an unknown scheme, or a p2pkh pubkey that does not hash to
/// the owner. A `Some` never claims a signature was valid.
pub fn owner_auth(state: &State, auth: &[u8]) -> Option<OwnerAuth> {
    match state.owner_scheme {
        OWNER_P2PK_SCHNORR => (auth.len() == 65).then_some(OwnerAuth::P2pkSignature),
        OWNER_P2PKH_SCHNORR => {
            if auth.len() != 32 + 65 {
                return None;
            }
            let pubkey: [u8; 32] = auth[..32].try_into().ok()?;
            (kcc1::p2pkh_hash(&pubkey) == state.owner).then_some(OwnerAuth::P2pkhSchnorr { pubkey })
        }
        OWNER_P2PKH_ECDSA => {
            if auth.len() != 33 + 65 {
                return None;
            }
            let pubkey: [u8; 33] = auth[..33].try_into().ok()?;
            (kcc1::p2pkh_hash(&pubkey) == state.owner).then_some(OwnerAuth::P2pkhEcdsa { pubkey })
        }
        OWNER_P2SH => {
            let [index] = auth else { return None };
            Some(OwnerAuth::P2shAuthorityInput(*index))
        }
        OWNER_COVENANT_ID => auth.is_empty().then_some(OwnerAuth::CovenantId),
        _ => None,
    }
}

/// The fixed borrow-scheme mapping (§3, restated with guard semantics in §5). `None` for a threshold guard whose
/// first eight bytes are not a non-negative KCC-1 int payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorrowScheme {
    Disabled,
    /// The increase must strictly exceed this threshold; guard bytes 8..32
    /// are unused and deliberately unconstrained (§5).
    AmountThreshold(i64),
    /// The guard is a Schnorr pubkey; validity of the witness signature is
    /// consensus's, so a check here can only ever be structural.
    SchnorrSignature([u8; 32]),
    /// The guard is the current chain head; each borrow reveals its preimage
    /// and advances the guard to it.
    HashChain([u8; 32]),
    Reserved(u8),
}

impl BorrowScheme {
    pub fn of(state: &State) -> Option<BorrowScheme> {
        Some(match state.borrow_scheme {
            0x00 => BorrowScheme::Disabled,
            0x01 => {
                let payload: [u8; 8] = state.borrow_guard[..8].try_into().ok()?;
                if payload[7] & 0x80 != 0 {
                    return None;
                }
                BorrowScheme::AmountThreshold(kcc1::decode_state_int(&payload)?)
            }
            0x02 => BorrowScheme::SchnorrSignature(state.borrow_guard),
            0x03 => BorrowScheme::HashChain(state.borrow_guard),
            other => BorrowScheme::Reserved(other),
        })
    }
}

/// What the bytes allow saying about a borrowed receive. Two tiers on
/// purpose: hash-chain and amount-threshold are proven from bytes alone;
/// schnorr-signature is structural, because kascov does not re-run the
/// sighash and must never claim it did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BorrowVerdict {
    Proven(BorrowScheme),
    Structural(BorrowScheme),
    Invalid(&'static str),
}

/// The §5 borrowed-receive algorithm as the pinned text states it:
/// preservation, the per-scheme witness rule, `amount_difference >
/// amount_threshold` (0 unless the threshold scheme raises it), the expected
/// next guard, and the KAS floor (the family output carrying
/// `next_states[0]` may not hold less KAS than the borrowed input did). A
/// reserved scheme byte is `reject` under the pinned algorithm's final
/// `else` (§5), so it verdicts Invalid, not unknown; a future commit that
/// assigns the byte re-derives this module.
pub fn verify_borrow(
    leader: &State,
    next0: &State,
    borrow_witness: &[u8],
    leader_kas: u64,
    output0_kas: u64,
) -> BorrowVerdict {
    if next0.owner != leader.owner
        || next0.owner_scheme != leader.owner_scheme
        || next0.borrow_scheme != leader.borrow_scheme
        || next0.extension_commitment != leader.extension_commitment
    {
        return BorrowVerdict::Invalid(
            "the borrowed successor must preserve owner, owner_scheme, borrow_scheme and extension_commitment",
        );
    }
    if output0_kas < leader_kas {
        return BorrowVerdict::Invalid("the borrowed cell's KAS value cannot be reduced");
    }
    let Some(scheme) = BorrowScheme::of(leader) else {
        return BorrowVerdict::Invalid("the threshold guard is not a non-negative int payload");
    };
    let (threshold, next_guard, structural) = match scheme {
        BorrowScheme::Disabled => return BorrowVerdict::Invalid("borrowing is disabled for this cell"),
        BorrowScheme::AmountThreshold(threshold) => {
            if !borrow_witness.is_empty() {
                return BorrowVerdict::Invalid("the amount-threshold borrow witness must be empty");
            }
            (threshold, leader.borrow_guard, false)
        }
        BorrowScheme::SchnorrSignature(_) => {
            if borrow_witness.len() != 65 {
                return BorrowVerdict::Invalid("the schnorr borrow witness must be a 65-byte signature");
            }
            (0, leader.borrow_guard, true)
        }
        BorrowScheme::HashChain(guard) => {
            if borrow_witness.len() != 32 {
                return BorrowVerdict::Invalid("the hash-chain borrow witness must be a 32-byte preimage");
            }
            if kcc1::hash(borrow_witness) != guard {
                return BorrowVerdict::Invalid("the preimage does not hash to the current guard");
            }
            let mut advanced = [0u8; 32];
            advanced.copy_from_slice(borrow_witness);
            (0, advanced, false)
        }
        BorrowScheme::Reserved(_) => {
            return BorrowVerdict::Invalid("a reserved borrow scheme byte; the pinned text rejects it")
        }
    };
    let difference = i128::from(next0.amount) - i128::from(leader.amount);
    if difference <= i128::from(threshold) {
        return BorrowVerdict::Invalid("the token increase must strictly exceed the threshold");
    }
    if next0.borrow_guard != next_guard {
        return BorrowVerdict::Invalid("the successor does not carry the expected next borrow guard");
    }
    if structural {
        BorrowVerdict::Structural(scheme)
    } else {
        BorrowVerdict::Proven(scheme)
    }
}

/// One spent family input, as the transition checker consumes it.
#[derive(Clone, Debug)]
pub struct FamilyInput {
    /// Position within the spending transaction; the LEADER is the lowest
    /// (K1 §9.1) and must carry the `transfer` invocation.
    pub input_index: u32,
    pub state: State,
    /// The spent UTXO's KAS value.
    pub kas: u64,
}

/// One continuation output of the family, in family output order. A KCC-1
/// instance lives under scriptPublicKey VERSION 0 (K1 §7); the version
/// travels with the script bytes so the proof cannot be satisfied by
/// identical bytes under another version.
#[derive(Clone, Debug)]
pub struct FamilyOutput {
    pub spk_version: u16,
    pub spk: Vec<u8>,
    pub kas: u64,
}

/// The bounds and scheme sets a transition is judged against — pin
/// knowledge, defaulting to the spec's default configuration (§3).
#[derive(Clone, Debug)]
pub struct Bounds {
    pub max_token_inputs: usize,
    pub max_token_outputs: usize,
    pub owner_schemes: Vec<u8>,
    pub borrow_schemes: Vec<u8>,
}

impl Default for Bounds {
    fn default() -> Self {
        Bounds {
            max_token_inputs: 3,
            max_token_outputs: 3,
            owner_schemes: vec![
                OWNER_P2PK_SCHNORR,
                OWNER_P2PKH_SCHNORR,
                OWNER_P2PKH_ECDSA,
                OWNER_P2SH,
                OWNER_COVENANT_ID,
            ],
            borrow_schemes: vec![0x00, 0x01, 0x02, 0x03],
        }
    }
}

/// The facts a family transition establishes from bytes. Facts, not policy:
/// which false flag becomes invalid and which merely unvalidated is the
/// accounting layer's decision, made upstream where the pin's review status
/// is known.
#[derive(Clone, Debug)]
pub struct Verdict {
    /// The declared successors reconstruct to the family outputs, every one
    /// of them version-0 P2SH, and at least one successor was declared (an
    /// empty declaration proves nothing). When this is false nothing
    /// downstream is meaningful and every other field describes the CLAIMED
    /// transition only.
    pub successors_proven: bool,
    /// Σ input amounts == Σ successor amounts (§2's family-total rule).
    pub family_total_ok: bool,
    pub bounds_ok: bool,
    /// Every successor's owner scheme sits in the pin's supported subset and
    /// every borrow scheme byte sits in the spec's FIXED mapping 0x00..=0x03
    /// — both are the spec's own rules (§2, §3).
    pub schemes_ok: bool,
    /// Every successor's borrow scheme also sits in the pin's declared set.
    /// An observation about the pinned program, not a spec rule: the spec
    /// grants no configuration freedom over borrow schemes (§3), so a pin
    /// narrower than the fixed mapping describes a reduced, non-conformant
    /// program.
    pub borrow_schemes_in_pin: bool,
    /// §2/§4: `extension_commitment` must be preserved unchanged. True when
    /// every successor's class appears among the input classes AND the
    /// per-class sums balance. A successor class matching no input is a flat
    /// breach and lands in `violations`; an imbalance among existing classes
    /// is left to the accounting layer's judgment (its resolution on kccs
    /// issue #14 may still move), so it clears this flag without a
    /// violation string.
    pub classes_preserved: bool,
    /// A single-successor family drew all inputs from one class equal to the
    /// successor's (§2's consolidation sentence). Vacuously true otherwise.
    pub consolidation_within_class: bool,
    /// More than one distinct class among inputs and successors. Not by
    /// itself a breach when every class is preserved — a family may carry a
    /// rock cell and a paper cell side by side — but the authors' stated
    /// intent is uniformity, so the accounting layer treats it cautiously.
    pub mixed_classes: bool,
    /// The leader's path; `None` when the witness carries an invalid prefix.
    pub path_normal: Option<bool>,
    /// On the normal path: the leader's owner-auth bytes checked against its
    /// scheme's shape, including the byte-provable p2pkh binding. `None`
    /// with a violation when the shape or binding fails; never a claim that
    /// a signature was verified.
    pub owner_auth: Option<OwnerAuth>,
    pub borrow: Option<BorrowVerdict>,
    /// class hex -> (Σ in, Σ out).
    pub per_class_sums: Vec<(String, i128, i128)>,
    pub violations: Vec<String>,
}

/// Check one covenant-family transition. `inputs` are every spent family
/// input; `family_outputs` every continuation output in family order.
pub fn check_family(
    inputs: &[FamilyInput],
    transfer: &TransferArgs,
    template: &Template,
    family_outputs: &[FamilyOutput],
    bounds: &Bounds,
) -> Option<Verdict> {
    let leader = inputs.iter().min_by_key(|input| input.input_index)?;
    // duplicate input indexes mean the caller's family assembly is broken:
    // refuse rather than quietly judge whichever state sorted first
    let mut seen_indexes: Vec<u32> = inputs.iter().map(|input| input.input_index).collect();
    seen_indexes.sort_unstable();
    seen_indexes.dedup();
    if seen_indexes.len() != inputs.len() {
        return None;
    }
    let mut violations = Vec::new();

    let family_spks: Vec<Vec<u8>> = family_outputs.iter().map(|out| out.spk.clone()).collect();
    let versions_ok = family_outputs.iter().all(|out| out.spk_version == 0);
    let successors_proven = versions_ok
        && !transfer.next_states.is_empty()
        && prove_next_states(template, &transfer.next_states, &family_spks);
    if !versions_ok {
        violations.push("a family output is not a version-0 script public key".into());
    } else if !successors_proven {
        violations.push("declared successor states do not reconstruct to the family outputs".into());
    }

    let sum_in: i128 = inputs.iter().map(|input| i128::from(input.state.amount)).sum();
    let sum_out: i128 = transfer
        .next_states
        .iter()
        .map(|state| i128::from(state.amount))
        .sum();
    let family_total_ok = sum_in == sum_out;
    if !family_total_ok {
        violations.push(format!(
            "family total not preserved: {sum_in} in, {sum_out} out"
        ));
    }

    let bounds_ok = inputs.len() <= bounds.max_token_inputs
        && transfer.next_states.len() <= bounds.max_token_outputs;
    if !bounds_ok {
        violations.push(format!(
            "family exceeds bounds: {} inputs, {} outputs (max {}/{})",
            inputs.len(),
            transfer.next_states.len(),
            bounds.max_token_inputs,
            bounds.max_token_outputs
        ));
    }

    let schemes_ok = transfer.next_states.iter().all(|state| {
        bounds.owner_schemes.contains(&state.owner_scheme) && state.borrow_scheme <= 0x03
    });
    if !schemes_ok {
        violations.push(
            "a successor carries an owner scheme outside the supported subset or a borrow scheme outside the fixed mapping".into(),
        );
    }
    let borrow_schemes_in_pin = transfer
        .next_states
        .iter()
        .all(|state| bounds.borrow_schemes.contains(&state.borrow_scheme));

    let mut classes: Vec<[u8; 32]> = inputs.iter().map(|i| i.state.extension_commitment).collect();
    classes.extend(transfer.next_states.iter().map(|s| s.extension_commitment));
    classes.sort_unstable();
    classes.dedup();
    let mixed_classes = classes.len() > 1;

    // §2/§4: the commitment is preserved unchanged. A successor whose class
    // matches no input is a flat breach; an imbalance among existing classes
    // clears the flag and is judged upstream.
    let fresh_class = transfer.next_states.iter().find(|state| {
        !inputs
            .iter()
            .any(|input| input.state.extension_commitment == state.extension_commitment)
    });
    if let Some(state) = fresh_class {
        violations.push(format!(
            "a successor carries extension commitment {} that no input carries; the transfer must preserve it unchanged",
            hex::encode(state.extension_commitment)
        ));
    }

    let consolidation_within_class = if transfer.next_states.len() == 1 {
        let class = transfer.next_states[0].extension_commitment;
        inputs.iter().all(|input| input.state.extension_commitment == class)
    } else {
        true
    };
    if !consolidation_within_class {
        violations.push(
            "inputs consolidated into one successor carry different extension commitments".into(),
        );
    }

    let mut per_class: Vec<(String, i128, i128)> = Vec::new();
    let sum_for = |class: [u8; 32], amount: i128, out: bool, per_class: &mut Vec<(String, i128, i128)>| {
        let key = hex::encode(class);
        match per_class.iter_mut().find(|(existing, _, _)| *existing == key) {
            Some(row) => {
                if out {
                    row.2 += amount;
                } else {
                    row.1 += amount;
                }
            }
            None => per_class.push(if out { (key, 0, amount) } else { (key, amount, 0) }),
        }
    };
    for input in inputs {
        sum_for(input.state.extension_commitment, i128::from(input.state.amount), false, &mut per_class);
    }
    for state in &transfer.next_states {
        sum_for(state.extension_commitment, i128::from(state.amount), true, &mut per_class);
    }
    let classes_preserved =
        fresh_class.is_none() && per_class.iter().all(|(_, sum_in, sum_out)| sum_in == sum_out);

    let path = witness_path(&transfer.witness);
    let path_normal = match path {
        Some(WitnessPath::Normal(_)) => Some(true),
        Some(WitnessPath::Borrowed(_)) => Some(false),
        None => {
            violations.push("the leader witness carries an invalid path byte".into());
            None
        }
    };
    let owner_auth = match path {
        Some(WitnessPath::Normal(auth_bytes)) => {
            let auth = owner_auth(&leader.state, auth_bytes);
            if auth.is_none() {
                violations.push(
                    "the leader's owner-auth bytes do not fit its scheme (shape or p2pkh binding)".into(),
                );
            }
            auth
        }
        _ => None,
    };
    let borrow = match path {
        Some(WitnessPath::Borrowed(borrow_witness)) => {
            // Proven/Structural presuppose that next_states[0] IS the cell
            // behind family output zero; only reconstruction establishes
            // that, so an unproven family cannot carry a proven borrow.
            let verdict = if !successors_proven {
                BorrowVerdict::Invalid("the borrowed successor is not proven against the family outputs")
            } else {
                match (transfer.next_states.first(), family_outputs.first()) {
                    (Some(next0), Some(output0)) => {
                        verify_borrow(&leader.state, next0, borrow_witness, leader.kas, output0.kas)
                    }
                    _ => BorrowVerdict::Invalid("a borrowed receive needs a successor at family position zero"),
                }
            };
            if let BorrowVerdict::Invalid(reason) = &verdict {
                violations.push(format!("borrowed receive: {reason}"));
            }
            Some(verdict)
        }
        _ => None,
    };

    Some(Verdict {
        successors_proven,
        family_total_ok,
        bounds_ok,
        schemes_ok,
        borrow_schemes_in_pin,
        classes_preserved,
        consolidation_within_class,
        mixed_classes,
        path_normal,
        owner_auth,
        borrow,
        per_class_sums: per_class,
        violations,
    })
}

// ───────────────────────────── pins ──────────────────────────────

/// How a program's signature script selects entrypoints. The michaelsutton
/// fork at d57e5df (the compiler every pinned token so far was built with)
/// emits a one-byte numeric selector and ends a chain with `6a`; silverscript
/// v1.0.0 (3ed9733) emits the KCC-1 `OP_DATA_4` tag rung. A trailing
/// `byte[4]` argument is indistinguishable from a tag by bytes alone, so the
/// encoding is declared, never inferred (`kcc1::read_invocation` documents
/// the misfiling hazard).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DispatchKind {
    Kcc1Tag,
    SilvercSelector,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PinDispatch {
    pub kind: DispatchKind,
    /// Hex tag (kcc1-tag) or decimal selector (silverc-selector) of the
    /// transfer entrypoint.
    pub transfer: String,
    pub transfer_delegator: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PinStateRange {
    pub start: usize,
    pub len: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PinExtension {
    /// "kcc1-10.1-packed" when the artifact describes an enumerable packed
    /// domain (labels become checkable), "opaque" otherwise.
    pub scheme: String,
    /// digest hex -> label; only trusted after the loader recomputes every
    /// digest from the declared domain (Phase 5).
    #[serde(default)]
    pub labels: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub plain_commitment: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PinEntrypoint {
    pub name: String,
    pub types: String,
    /// Tag hex or selector decimal, matching the dispatch kind.
    pub select: String,
    /// "transfer" | "delegator" | "extension-update" | "program-specific"
    pub role: String,
    #[serde(default)]
    pub opening_arg: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PinReview {
    /// "reviewed" | "unreviewed" — only a reviewed pin can carry a token to
    /// verified, and only a reviewed pin's labels may name a class.
    pub status: String,
    /// Fixture basenames under fixtures/kcc0020/ whose reveals must
    /// recompute the pin's template hash (enforcement lands with the first
    /// fixtures, Phase 2).
    pub fixtures: Vec<String>,
    /// True when both fixtures are chain reveals of the one launch the
    /// chain carries of this build, so their extension commitments cannot
    /// differ. The fixture test then holds the pair to the other state
    /// fields and the pin's note must say how the window's end is proven
    /// instead (the 2,641 B Zealous generation: from the program's own
    /// length and suffix-offset constants).
    #[serde(default)]
    pub fixtures_share_class: bool,
    pub note: String,
}

/// kascov's committed description of one KCC-0020 program — the artifact as
/// this reader holds it, keyed by the KCC-1 template hash. The hash MUST be
/// recomputed from the pin's fixture reveals before anything trusts it;
/// that recompute lands with the first pinned fixtures (Phase 2) and is not
/// enforced yet, which is safe only because the pin set ships empty.
#[derive(Clone, Debug, Deserialize)]
pub struct Pin {
    pub spec_commit: String,
    pub template_hash: String,
    pub name: String,
    /// The display family a reveal of this program is stamped with, where
    /// the pin declares one: "Zealous · token" for ZealousSwap's cell, so
    /// the venue prefix rules ("Zealous · ") own it the way they own every
    /// other family kascov names from bytes. A pin without one stamps the
    /// generic [`SPEC_TOKEN_TEMPLATE`]. Display only: every spec-route
    /// decision keys on the pin match, never on this string, and the label
    /// must not collide with a legacy cell family or an observed skeleton
    /// name, or the legacy decoders would claim a spec program by name
    /// (guarded by `every_committed_pin_parses_and_names_this_spec`).
    #[serde(default)]
    pub family: Option<String>,
    pub dispatch: PinDispatch,
    pub state: PinStateRange,
    pub owner_schemes: Vec<u8>,
    pub max_token_inputs: usize,
    pub max_token_outputs: usize,
    pub borrow_schemes: Vec<u8>,
    pub extension: PinExtension,
    pub entrypoints: Vec<PinEntrypoint>,
    /// What the program's own in-script templateHash builtin computes:
    /// "blake2b" for builds of the michaelsutton fork at d57e5df (the
    /// generation every pinned token runs), "blake3" for silverscript
    /// v1.0.0 (3ed9733) builds, which store the §8.3 value on chain and
    /// check it with OpBlake3. Distinct from the BLAKE3 pin key either way.
    pub in_script_template_hash_fn: String,
    pub review: PinReview,
}

impl Pin {
    pub fn bounds(&self) -> Bounds {
        Bounds {
            max_token_inputs: self.max_token_inputs,
            max_token_outputs: self.max_token_outputs,
            owner_schemes: self.owner_schemes.clone(),
            borrow_schemes: self.borrow_schemes.clone(),
        }
    }

    /// The template name a reveal of this program is filed under: the pin's
    /// declared family, or [`SPEC_TOKEN_TEMPLATE`] when it declares none.
    /// Borrowed from the pin, so on a committed pin (`pins()`) it is
    /// `'static` and fits the reveal-time stamp directly.
    pub fn family_label(&self) -> &str {
        self.family.as_deref().unwrap_or(SPEC_TOKEN_TEMPLATE)
    }
}

/// Every template name a pinned KCC-0020 reveal can be stamped with: the
/// generic [`SPEC_TOKEN_TEMPLATE`] first (a pin without a family, and every
/// row stamped before pins carried families), then each committed pin's
/// declared family, deduplicated in pin order.
pub fn spec_template_names() -> Vec<&'static str> {
    let mut names = vec![SPEC_TOKEN_TEMPLATE];
    for pin in pins() {
        let label = pin.family_label();
        if !names.contains(&label) {
            names.push(label);
        }
    }
    names
}

/// Whether a stored template name means "a pinned KCC-0020 build": the
/// generic label or any pin's family. Callers that need to know a row is a
/// spec cell test this, never the one literal, so a pin gaining a family
/// cannot silently drop its rows out of a name-keyed gate.
pub fn is_spec_template(name: &str) -> bool {
    name == SPEC_TOKEN_TEMPLATE || pins().iter().any(|pin| pin.family_label() == name)
}

/// Pin sources are embedded at compile time — layouts are data, but the data
/// ships inside the binary and is self-checked by tests, never read from
/// disk at runtime (the same stance as `CELL_FAMILIES`). Phase 2 adds the
/// first entry: `("kcc0020-vector", include_str!("../pins/kcc0020/….json"))`.
const PIN_SOURCES: &[(&str, &str)] = &[
    ("kcc0020-vector", include_str!("../pins/kcc0020/4abcc188.json")),
    ("zealous-token-cell", include_str!("../pins/kcc0020/653012f1.json")),
    ("zealous-token-cell-2641", include_str!("../pins/kcc0020/2f2671fc.json")),
    ("zealous-token-cell-3037", include_str!("../pins/kcc0020/58c24260.json")),
    ("zealous-token-cell-3223", include_str!("../pins/kcc0020/50ff50d8.json")),
];

/// Every committed pin, parsed once. A pin that fails to parse is a build
/// defect, so this panics loudly rather than skipping it silently — the
/// swallowed-`None` lesson from the observed-family pins.
pub fn pins() -> &'static [Pin] {
    static PINS: OnceLock<Vec<Pin>> = OnceLock::new();
    PINS.get_or_init(|| {
        PIN_SOURCES
            .iter()
            .map(|(name, source)| {
                serde_json::from_str::<Pin>(source)
                    .unwrap_or_else(|e| panic!("pin {name} does not parse: {e}"))
            })
            .collect()
    })
}

/// A short fingerprint over every committed pin's identity and review
/// status. What a token DERIVES AS depends on these, so the accounting
/// layer composes this into its derivation stamp: reviewing a pin, adding
/// one, or changing a declared bound re-derives every stored token
/// mechanically, instead of by someone remembering to bump a constant.
pub fn pins_fingerprint() -> String {
    let mut hasher = blake3::Hasher::new();
    for pin in pins() {
        hasher.update(pin.template_hash.as_bytes());
        hasher.update(pin.review.status.as_bytes());
        hasher.update(&[pin.max_token_inputs as u8, pin.max_token_outputs as u8]);
        hasher.update(&pin.owner_schemes);
        hasher.update(&pin.borrow_schemes);
        hasher.update(pin.spec_commit.as_bytes());
    }
    hex::encode(&hasher.finalize().as_bytes()[..4])
}

/// The pin describing a template, by its canonical (BLAKE3) hash.
pub fn pin_for(template_hash: &[u8; 32]) -> Option<&'static Pin> {
    let hex = hex::encode(template_hash);
    pins().iter().find(|pin| pin.template_hash == hex)
}

/// Recognise a revealed program as a pinned KCC-0020 build: cut it at the
/// pin's declared state offset, require the template hash to reproduce, and
/// decode the embedded state. Pin-gated on purpose — an unpinned program is
/// never claimed, however spec-shaped its bytes look, so arbitrary scripts
/// cannot become token candidates by accident.
pub fn pinned_program_state(program: &[u8]) -> Option<(&'static Pin, State)> {
    for pin in pins() {
        let Some(template) = Template::cut(program, pin.state.start) else {
            continue;
        };
        if hex::encode(template.hash()) != pin.template_hash {
            continue;
        }
        let state = State::decode(&program[pin.state.start..pin.state.start + STATE_LEN])?;
        return Some((pin, state));
    }
    None
}

/// The push payloads of a signature script, split; `None` on any non-minimal
/// or torn push.
fn split_pushes(sig_script: &[u8]) -> Option<Vec<Vec<u8>>> {
    let mut payloads = Vec::new();
    let mut at = 0;
    while at < sig_script.len() {
        let (payload, consumed) = kcc1::read_push_minimal(&sig_script[at..])?;
        payloads.push(payload);
        at += consumed;
    }
    Some(payloads)
}

/// The selector payload a silverc-dispatched entrypoint pushes: script-number
/// bytes of the decimal in the pin (`snum(0)` is the empty payload).
fn selector_payload(select: &str) -> Option<Vec<u8>> {
    Some(crate::snum(select.parse::<i64>().ok()?))
}

/// Parse a leader reveal against a pin: the program last, the entrypoint
/// selector before it (per the pin's dispatch), the transfer arguments before
/// that. `None` when the script is not this pin's transfer invocation — which
/// includes every delegator reveal, so callers probe both.
pub fn parse_transfer_reveal(sig_script: &[u8], pin: &Pin) -> Option<TransferArgs> {
    if pin.dispatch.kind != DispatchKind::SilvercSelector {
        return None;
    }
    let mut payloads = split_pushes(sig_script)?;
    let _program = payloads.pop()?;
    let selector = payloads.pop()?;
    if selector != selector_payload(&pin.dispatch.transfer)? {
        return None;
    }
    TransferArgs::from_args(&payloads)
}

/// What a KCC-1-tag leader reveal declares, read against the pin's
/// entrypoint table. Builds that dispatch on a 4-byte tag do not lower
/// `KCC20State[]` field-major the way the pinned silverc does: the ones
/// seen on chain push the successor's fields one by one and let the program
/// compute the rest (the change cell, or a merge's sum), so the reader
/// derives every successor from the leader's inputs and hash-checks each
/// candidate against the output's own commitment. Nothing here is trusted
/// until that check passes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagReveal {
    /// `transfer`: one declared successor; the leader's owner keeps the
    /// remainder in a second cell when there is one.
    Transfer { amount: i64, owner: [u8; 32], owner_scheme: u8, borrow_scheme: u8, borrow_guard: [u8; 32] },
    /// `merge`: every family input folds into one cell owned like the leader.
    Merge,
    /// `transfer_whole`: the leader's whole amount moves to a declared owner.
    Whole { owner: [u8; 32], owner_scheme: u8, borrow_scheme: u8, borrow_guard: [u8; 32] },
    /// The borrowed receive (the venue's name for it is unknown, kascov
    /// files it as `transfer_borrowed`): `amount` flows from the one
    /// follower into this leader under the leader's borrow rule, no owner
    /// signature. `preimage` is the hash-chain reveal, the value the
    /// successor's guard becomes under scheme 0x03, and `signature` a
    /// transaction signature (each zero under the schemes that do not use
    /// it); `follower_change` is what the follower keeps. The program
    /// requires `amount + follower_change` to equal the follower's amount,
    /// so the reader derives both successors from the two proven inputs and
    /// hash-checks each. Two layouts exist, declared per pin by the
    /// entrypoint's types: the 2,590 B build (tag `a85e5b91`) takes four
    /// slots, the 2,641 B build and the 3,037 B and 3,223 B builds after
    /// it (tag `13f87a5b`) five, their hash chain committing to a key that
    /// must sign as well; the key slot is not carried here because the
    /// reader never re-runs a signature.
    Borrowed { amount: i64, preimage: [u8; 32], signature: [u8; 65], follower_change: i64 },
}

/// A minimally encoded, non-negative script number of at most eight bytes;
/// the empty push (script zero) is refused, since no arm takes a zero here.
fn minimal_amount(payload: &[u8]) -> Option<i64> {
    if payload.is_empty() {
        return None;
    }
    minimal_amount_or_zero(payload)
}

/// As [`minimal_amount`], with the empty push reading as zero: the borrowed
/// receive's follower remainder may legally be nothing.
fn minimal_amount_or_zero(payload: &[u8]) -> Option<i64> {
    if payload.is_empty() {
        return Some(0);
    }
    if payload.len() > 8 {
        return None;
    }
    if payload[payload.len() - 1] & 0x80 != 0 {
        return None;
    }
    let mut b = [0u8; 8];
    b[..payload.len()].copy_from_slice(payload);
    Some(i64::from_le_bytes(b))
}

/// Parse a leader reveal of a KCC-1-tag pin: the program last, the 4-byte
/// tag before it, the entrypoint's arguments before that. `None` for a
/// follower (delegator) reveal, an unknown tag, or arguments that do not fit
/// the entrypoint's declared layout.
pub fn parse_tag_reveal(sig_script: &[u8], pin: &Pin) -> Option<TagReveal> {
    if pin.dispatch.kind != DispatchKind::Kcc1Tag {
        return None;
    }
    let mut payloads = split_pushes(sig_script)?;
    let _program = payloads.pop()?;
    let tag = payloads.pop()?;
    if tag.len() != 4 {
        return None;
    }
    let tag_hex = hex::encode(&tag);
    let entry = pin.entrypoints.iter().find(|e| e.select == tag_hex)?;
    let fixed = |i: usize, n: usize| -> Option<&[u8]> {
        payloads.get(i).filter(|p| p.len() == n).map(|p| p.as_slice())
    };
    match entry.name.as_str() {
        "transfer" => {
            if payloads.len() < 6 {
                return None;
            }
            let amount = minimal_amount(payloads.first()?)?;
            let owner: [u8; 32] = fixed(1, 32)?.try_into().ok()?;
            let owner_scheme = fixed(2, 1)?[0];
            let borrow_scheme = fixed(3, 1)?[0];
            let borrow_guard: [u8; 32] = fixed(4, 32)?.try_into().ok()?;
            Some(TagReveal::Transfer { amount, owner, owner_scheme, borrow_scheme, borrow_guard })
        }
        "merge" => Some(TagReveal::Merge),
        "transfer_whole" => {
            if payloads.len() < 5 {
                return None;
            }
            let owner: [u8; 32] = fixed(0, 32)?.try_into().ok()?;
            let owner_scheme = fixed(1, 1)?[0];
            let borrow_scheme = fixed(2, 1)?[0];
            let borrow_guard: [u8; 32] = fixed(3, 32)?.try_into().ok()?;
            Some(TagReveal::Whole { owner, owner_scheme, borrow_scheme, borrow_guard })
        }
        // The argument list is the pin's declared one, never inferred from
        // the pushes: a script with the other generation's slot count is
        // not this pin's borrowed receive, and an undeclared layout parses
        // nothing.
        "transfer_borrowed" => match entry.types.as_str() {
            // the 2,590 B build: amount, preimage, signature, remainder
            "int,byte[32],byte[65],byte[]" => {
                if payloads.len() < 4 {
                    return None;
                }
                let amount = minimal_amount(payloads.first()?)?;
                let preimage: [u8; 32] = fixed(1, 32)?.try_into().ok()?;
                let signature: [u8; 65] = fixed(2, 65)?.try_into().ok()?;
                let follower_change = minimal_amount_or_zero(payloads.get(3)?)?;
                Some(TagReveal::Borrowed { amount, preimage, signature, follower_change })
            }
            // the 2,641 B build and the two after it: amount, next head,
            // committed key, signature, remainder; the key is read by the
            // program, not by the reader
            "int,byte[32],byte[32],byte[65],byte[]" => {
                if payloads.len() < 5 {
                    return None;
                }
                let amount = minimal_amount(payloads.first()?)?;
                let preimage: [u8; 32] = fixed(1, 32)?.try_into().ok()?;
                fixed(2, 32)?;
                let signature: [u8; 65] = fixed(3, 65)?.try_into().ok()?;
                let follower_change = minimal_amount_or_zero(payloads.get(4)?)?;
                Some(TagReveal::Borrowed { amount, preimage, signature, follower_change })
            }
            _ => None,
        },
        _ => None,
    }
}

/// Parse a delegator reveal against a pin: one witness push, the delegator
/// selector, the program.
pub fn parse_delegator_reveal(sig_script: &[u8], pin: &Pin) -> Option<DelegatorArgs> {
    if pin.dispatch.kind != DispatchKind::SilvercSelector {
        return None;
    }
    let mut payloads = split_pushes(sig_script)?;
    let _program = payloads.pop()?;
    let selector = payloads.pop()?;
    if selector != selector_payload(&pin.dispatch.transfer_delegator)? {
        return None;
    }
    DelegatorArgs::from_args(&payloads)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> Vec<u8> {
        hex::decode(s).unwrap()
    }

    fn state(seed: u8) -> State {
        State {
            amount: 1_000 + i64::from(seed),
            owner: [seed; 32],
            owner_scheme: OWNER_P2PK_SCHNORR,
            borrow_scheme: 0x00,
            borrow_guard: [0; 32],
            extension_commitment: [0xE0 ^ seed; 32],
        }
    }

    // ── tags ─────────────────────────────────────────────────────

    #[test]
    fn default_dispatch_tags_are_pinned() {
        // the §6.1 preimage expands KCC20State to its six field types; the
        // readable `transfer(KCC20State[],byte[])` would hash to 98da4203,
        // which no conformant program dispatches on
        assert_eq!(hex::encode(transfer_tag()), "79c71c23");
        assert_eq!(hex::encode(delegator_tag()), "fd3ef14a");
        assert_eq!(
            hex::encode(kcc1::dispatch_tag("transfer(KCC20State[],byte[])")),
            "98da4203"
        );
    }

    /// A fork-silverc (d57e5df) build dispatches on a one-byte selector
    /// before the program; read as a KCC-1 tag script that fails closed
    /// instead of misfiling.
    #[test]
    fn a_selector_script_is_not_a_tag_script() {
        let mut sig = encode_push(&[0xAA; 8]); // some argument
        sig.push(0x51); // silverc selector 1 (OP_1 push, one byte)
        sig.extend_from_slice(&encode_push(&[0x63, 0x51, 0x68])); // program
        assert_eq!(kcc1::read_invocation(&sig), None);
    }

    // ── state codec ──────────────────────────────────────────────

    #[test]
    fn state_round_trips_and_is_112_bytes() {
        for amount in [0, 1, 255, 1_000_000, i64::MAX] {
            let mut s = state(0x11);
            s.amount = amount;
            let bytes = s.encode().unwrap();
            assert_eq!(bytes.len(), STATE_LEN);
            assert_eq!(State::decode(&bytes), Some(s));
        }
        // scheme byte 0x01 stays PushExplicit `01 01`, never OP_1
        let bytes = State { owner_scheme: 0x01, ..state(0) }.encode().unwrap();
        assert_eq!(&bytes[42..44], &[0x01, 0x01]);
    }

    #[test]
    fn state_decode_fails_closed() {
        let good = state(0x22).encode().unwrap();
        // wrong opcode at each of the six offsets
        for (at, _) in OPCODE_AT {
            let mut bad = good;
            bad[at] ^= 0x40;
            assert_eq!(State::decode(&bad), None, "opcode at {at}");
        }
        // a set sign bit is rejected, negative zero included
        let mut negative = good;
        negative[8] |= 0x80;
        assert_eq!(State::decode(&negative), None);
        let mut negative_zero = good;
        negative_zero[1..9].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x80]);
        assert_eq!(State::decode(&negative_zero), None);
        // wrong lengths
        assert_eq!(State::decode(&good[..111]), None);
        let mut long = good.to_vec();
        long.push(0x00);
        assert_eq!(State::decode(&long), None);
        // a negative amount cannot even be encoded
        assert_eq!(State { amount: -1, ..state(0) }.encode(), None);
    }

    // ── the two layouts never claim each other ───────────────────

    #[test]
    fn the_scan_never_claims_a_46_byte_legacy_program_and_vice_versa() {
        for fixture in [
            include_bytes!("../fixtures/kcc20_a_a.bin").as_slice(),
            include_bytes!("../fixtures/kcc20_b_a.bin").as_slice(),
            include_bytes!("../fixtures/kcc20_unguarded_kron.bin").as_slice(),
        ] {
            assert!(
                locate_state_cuts(fixture).is_empty(),
                "a legacy build must never look like a spec state"
            );
        }
        // and a spec program's state block is not a 46-byte head
        let mut program = vec![0x6b];
        program.extend_from_slice(&state(0x33).encode().unwrap());
        program.extend_from_slice(&[0x6c, 0x75, 0x51]);
        assert!(crate::kcc20::decode_state_block(&program).is_none());
        assert_eq!(locate_state_cuts(&program), vec![1]);
    }

    // ── reconstruction ───────────────────────────────────────────

    fn test_template() -> Template {
        Template { prefix: vec![0x6b], suffix: vec![0x6c, 0x75, 0x51] }
    }

    #[test]
    fn reconstruction_accepts_only_the_committed_state() {
        let t = test_template();
        let a = state(0x44);
        let spk = t.reconstruct_spk(&a).unwrap();
        assert_eq!(spk.len(), 35);
        assert!(prove_next_states(&t, &[a], &[spk.clone()]));
        // any drifted field is a different program, hence a different SPK
        for drift in [
            State { amount: a.amount + 1, ..a },
            State { owner: [0x45; 32], ..a },
            State { owner_scheme: OWNER_P2SH, ..a },
            State { borrow_scheme: 0x01, ..a },
            State { borrow_guard: [1; 32], ..a },
            State { extension_commitment: [1; 32], ..a },
        ] {
            assert!(!prove_next_states(&t, &[drift], &[spk.clone()]));
        }
        // count mismatches prove nothing
        assert!(!prove_next_states(&t, &[a, a], &[spk.clone()]));
        assert!(!prove_next_states(&t, &[], &[spk]));
    }

    #[test]
    fn confirm_cut_demands_exactly_one_confirming_candidate() {
        let t = test_template();
        let a = state(0x55);
        let program = t.program(&a).unwrap();
        let next = state(0x56);
        let spks = vec![t.reconstruct_spk(&next).unwrap()];
        let (at, cut) = confirm_cut(&program, &[next], &spks).unwrap();
        assert_eq!(at, 1);
        assert_eq!(cut, t);
        assert_eq!(cut.hash(), kcc1::template_hash(&[0x6b], &[0x6c, 0x75, 0x51]));
        // an unrelated output confirms no cut: unknown program, not a guess
        let foreign = vec![kcc1::envelope_spk(&[0x51])];
        assert_eq!(confirm_cut(&program, &[next], &foreign), None);
        // and declaring nothing proves nothing
        assert_eq!(confirm_cut(&program, &[], &[]), None);
    }

    // ── transfer argument lowering ───────────────────────────────

    #[test]
    fn transfer_args_round_trip_field_major() {
        for n in [0usize, 1, 3] {
            let states: Vec<State> = (0..n as u8).map(state).collect();
            let args = TransferArgs { next_states: states, witness: vec![0x00, 0xAB] };
            let pushes = args.encode().unwrap();
            // re-split the push stream and parse it back
            let mut payloads = Vec::new();
            let mut at = 0;
            while at < pushes.len() {
                let (payload, consumed) = kcc1::read_push_minimal(&pushes[at..]).unwrap();
                payloads.push(payload);
                at += consumed;
            }
            assert_eq!(payloads.len(), 7);
            assert_eq!(TransferArgs::from_args(&payloads), Some(args));
        }
    }

    /// A single-element group whose one byte is 0x01 lowers to the bare
    /// numeric opcode under PushMinimal; the reader must reverse it.
    #[test]
    fn a_one_byte_scheme_group_collapses_to_op_1() {
        let mut s = state(0x01);
        s.owner_scheme = 0x01;
        let args = TransferArgs { next_states: vec![s], witness: vec![0x00] };
        let pushes = args.encode().unwrap();
        // the owner_schemes group (third push) is the single byte 0x01 = OP_1
        let mut at = 0;
        let mut raw_ops = Vec::new();
        while at < pushes.len() {
            raw_ops.push(pushes[at]);
            let (_, consumed) = kcc1::read_push_minimal(&pushes[at..]).unwrap();
            at += consumed;
        }
        assert_eq!(raw_ops[2], 0x51);
    }

    #[test]
    fn transfer_args_fail_closed() {
        let good = TransferArgs { next_states: vec![state(1), state(2)], witness: vec![0x00] };
        let widths = TransferArgs::field_widths();
        assert_eq!(widths, [8, 32, 1, 1, 32, 32]);
        let mut payloads: Vec<Vec<u8>> = {
            let pushes = good.encode().unwrap();
            let mut out = Vec::new();
            let mut at = 0;
            while at < pushes.len() {
                let (payload, consumed) = kcc1::read_push_minimal(&pushes[at..]).unwrap();
                out.push(payload);
                at += consumed;
            }
            out
        };
        assert!(TransferArgs::from_args(&payloads).is_some());
        // unequal element counts across the groups
        payloads[1].truncate(32);
        assert_eq!(TransferArgs::from_args(&payloads), None);
        // wrong arity
        assert_eq!(TransferArgs::from_args(&payloads[..6]), None);
        // a torn amounts group
        let torn = vec![vec![0u8; 7], vec![], vec![], vec![], vec![], vec![], vec![]];
        assert_eq!(TransferArgs::from_args(&torn), None);
        // a negative amount hidden in the group
        let mut negative = vec![vec![0u8; 8]; 6];
        negative[0][7] = 0x80;
        negative[1] = vec![0u8; 32];
        negative[2] = vec![0];
        negative[3] = vec![0];
        negative[4] = vec![0u8; 32];
        negative[5] = vec![0u8; 32];
        negative.push(vec![0x00]);
        assert_eq!(TransferArgs::from_args(&negative), None);
    }

    // ── witness paths and owner auth ─────────────────────────────

    #[test]
    fn witness_paths_and_owner_auth_shapes() {
        assert_eq!(witness_path(&[0x00, 0xAA]), Some(WitnessPath::Normal(&[0xAA][..])));
        assert_eq!(witness_path(&[0x01]), Some(WitnessPath::Borrowed(&[][..])));
        assert_eq!(witness_path(&[0x02, 0xAA]), None);
        assert_eq!(witness_path(&[]), None);

        let p2pk = state(0x66);
        assert_eq!(owner_auth(&p2pk, &[0u8; 65]), Some(OwnerAuth::P2pkSignature));
        assert_eq!(owner_auth(&p2pk, &[0u8; 64]), None);

        // p2pkh binds the revealed pubkey to the owner by keyed hash
        let pubkey = [0x07u8; 32];
        let mut p2pkh = state(0);
        p2pkh.owner_scheme = OWNER_P2PKH_SCHNORR;
        p2pkh.owner = kcc1::p2pkh_hash(&pubkey);
        let mut auth = pubkey.to_vec();
        auth.extend_from_slice(&[0u8; 65]);
        assert_eq!(owner_auth(&p2pkh, &auth), Some(OwnerAuth::P2pkhSchnorr { pubkey }));
        let mut wrong = auth.clone();
        wrong[0] ^= 1;
        assert_eq!(owner_auth(&p2pkh, &wrong), None, "a foreign pubkey must not pass");

        let mut p2sh = state(0);
        p2sh.owner_scheme = OWNER_P2SH;
        assert_eq!(owner_auth(&p2sh, &[3]), Some(OwnerAuth::P2shAuthorityInput(3)));
        assert_eq!(owner_auth(&p2sh, &[3, 4]), None);

        let mut covenant = state(0);
        covenant.owner_scheme = OWNER_COVENANT_ID;
        assert_eq!(owner_auth(&covenant, &[]), Some(OwnerAuth::CovenantId));
        assert_eq!(owner_auth(&covenant, &[0]), None);

        let mut unknown = state(0);
        unknown.owner_scheme = 0x05;
        assert_eq!(owner_auth(&unknown, &[]), None);
    }

    // ── borrow schemes ───────────────────────────────────────────

    fn borrowable(scheme: u8, guard: [u8; 32]) -> State {
        State { borrow_scheme: scheme, borrow_guard: guard, ..state(0x77) }
    }

    #[test]
    fn threshold_borrow_is_proven_and_strict() {
        let mut guard = [0xCC; 32]; // bytes 8..32 unused and non-zero on purpose
        guard[..8].copy_from_slice(&kcc1::encode_state_int(100).unwrap());
        let leader = borrowable(0x01, guard);
        let next = |amount: i64| State { amount, ..leader };
        // delta 101 > 100: proven
        assert_eq!(
            verify_borrow(&leader, &next(leader.amount + 101), &[], 5, 5),
            BorrowVerdict::Proven(BorrowScheme::AmountThreshold(100))
        );
        // delta == threshold is invalid, strictly
        assert!(matches!(
            verify_borrow(&leader, &next(leader.amount + 100), &[], 5, 5),
            BorrowVerdict::Invalid(_)
        ));
        // the witness must be empty
        assert!(matches!(
            verify_borrow(&leader, &next(leader.amount + 101), &[0x01], 5, 5),
            BorrowVerdict::Invalid(_)
        ));
        // less KAS on the rebuilt cell is invalid
        assert!(matches!(
            verify_borrow(&leader, &next(leader.amount + 101), &[], 5, 4),
            BorrowVerdict::Invalid(_)
        ));
        // a negative threshold payload is an invalid guard
        let mut bad_guard = guard;
        bad_guard[7] |= 0x80;
        let bad = borrowable(0x01, bad_guard);
        assert!(matches!(
            verify_borrow(&bad, &State { amount: bad.amount + 1, ..bad }, &[], 5, 5),
            BorrowVerdict::Invalid(_)
        ));
    }

    #[test]
    fn hash_chain_borrow_advances_the_guard() {
        let preimage = [0x5A; 32];
        let leader = borrowable(0x03, kcc1::hash(&preimage));
        let mut next = State { amount: leader.amount + 1, ..leader };
        next.borrow_guard = preimage;
        assert_eq!(
            verify_borrow(&leader, &next, &preimage, 5, 5),
            BorrowVerdict::Proven(BorrowScheme::HashChain(leader.borrow_guard))
        );
        // a forged preimage
        assert!(matches!(
            verify_borrow(&leader, &next, &[0x5B; 32], 5, 5),
            BorrowVerdict::Invalid(_)
        ));
        // a successor that does not advance the guard
        let stale = State { borrow_guard: leader.borrow_guard, ..next };
        assert!(matches!(
            verify_borrow(&leader, &stale, &preimage, 5, 5),
            BorrowVerdict::Invalid(_)
        ));
        // no increase, no borrow
        let flat = State { amount: leader.amount, ..next };
        assert!(matches!(
            verify_borrow(&leader, &flat, &preimage, 5, 5),
            BorrowVerdict::Invalid(_)
        ));
    }

    #[test]
    fn schnorr_borrow_is_never_more_than_structural() {
        let leader = borrowable(0x02, [0xAB; 32]);
        let next = State { amount: leader.amount + 1, ..leader };
        assert_eq!(
            verify_borrow(&leader, &next, &[0u8; 65], 5, 5),
            BorrowVerdict::Structural(BorrowScheme::SchnorrSignature([0xAB; 32]))
        );
        assert!(matches!(
            verify_borrow(&leader, &next, &[0u8; 64], 5, 5),
            BorrowVerdict::Invalid(_)
        ));
    }

    #[test]
    fn disabled_reserved_and_drifted_borrows_refuse() {
        let leader = borrowable(0x00, [0; 32]);
        let next = State { amount: leader.amount + 1, ..leader };
        assert!(matches!(verify_borrow(&leader, &next, &[], 5, 5), BorrowVerdict::Invalid(_)));
        // a reserved byte is `reject` under the pinned algorithm's final else
        let reserved = borrowable(0x04, [0; 32]);
        assert!(matches!(
            verify_borrow(&reserved, &State { amount: reserved.amount + 1, ..reserved }, &[], 5, 5),
            BorrowVerdict::Invalid(_)
        ));
        // any drifted preserved field refuses before scheme logic runs
        let threshold = borrowable(0x01, [0; 32]);
        let mut drifted = State { amount: threshold.amount + 1, ..threshold };
        drifted.extension_commitment = [1; 32];
        assert!(matches!(verify_borrow(&threshold, &drifted, &[], 5, 5), BorrowVerdict::Invalid(_)));
    }

    // ── family transitions ───────────────────────────────────────

    #[test]
    fn a_clean_split_verdicts_clean_and_interleaving_shifts_nothing() {
        let t = test_template();
        let input = state(0x10);
        let mut a = state(0x10);
        a.amount = 600;
        let mut b = state(0x10);
        b.amount = input.amount - 600;
        let mut witness = vec![0x00];
        witness.extend_from_slice(&[0u8; 65]); // p2pk owner auth: a 65-byte signature shape
        let transfer = TransferArgs { next_states: vec![a, b], witness };
        // the transaction also pays change and a fee output: family filtering
        // is positional over FAMILY outputs only, so interleaving non-family
        // outputs between them must not shift the proof
        let family_outputs = vec![
            FamilyOutput { spk_version: 0, spk: t.reconstruct_spk(&a).unwrap(), kas: 1 },
            FamilyOutput { spk_version: 0, spk: t.reconstruct_spk(&b).unwrap(), kas: 1 },
        ];
        let verdict = check_family(
            &[FamilyInput { input_index: 2, state: input, kas: 1 }],
            &transfer,
            &t,
            &family_outputs,
            &Bounds::default(),
        )
        .unwrap();
        assert!(verdict.successors_proven);
        assert!(verdict.family_total_ok);
        assert!(verdict.bounds_ok);
        assert!(verdict.schemes_ok);
        assert!(verdict.consolidation_within_class);
        assert!(verdict.classes_preserved);
        assert!(verdict.borrow_schemes_in_pin);
        assert!(!verdict.mixed_classes);
        assert_eq!(verdict.path_normal, Some(true));
        assert_eq!(verdict.owner_auth, Some(OwnerAuth::P2pkSignature));
        assert!(verdict.violations.is_empty(), "{:?}", verdict.violations);
        assert_eq!(verdict.per_class_sums.len(), 1);
        assert_eq!(verdict.per_class_sums[0].1, i128::from(input.amount));
        assert_eq!(verdict.per_class_sums[0].2, i128::from(input.amount));
    }

    #[test]
    fn cross_class_consolidation_and_total_drift_are_named() {
        let t = test_template();
        let rock = state(0x10);
        let mut paper = state(0x10);
        paper.extension_commitment = [0xAA; 32];
        let mut merged = state(0x10);
        merged.amount = rock.amount + paper.amount;
        let mut witness = vec![0x00];
        witness.extend_from_slice(&[0u8; 65]);
        let transfer = TransferArgs { next_states: vec![merged], witness };
        let outputs = vec![FamilyOutput { spk_version: 0, spk: t.reconstruct_spk(&merged).unwrap(), kas: 1 }];
        let verdict = check_family(
            &[
                FamilyInput { input_index: 0, state: rock, kas: 1 },
                FamilyInput { input_index: 1, state: paper, kas: 1 },
            ],
            &transfer,
            &t,
            &outputs,
            &Bounds::default(),
        )
        .unwrap();
        assert!(!verdict.consolidation_within_class);
        assert!(!verdict.classes_preserved, "the paper class went in and never came out");
        assert!(verdict.mixed_classes);
        assert!(verdict.family_total_ok);
        assert!(verdict
            .violations
            .iter()
            .any(|v| v.contains("different extension commitments")));
        // and a total that drifts is named too
        let mut minted = merged;
        minted.amount += 1;
        let mut witness = vec![0x00];
        witness.extend_from_slice(&[0u8; 65]);
        let transfer = TransferArgs { next_states: vec![minted], witness };
        let outputs = vec![FamilyOutput { spk_version: 0, spk: t.reconstruct_spk(&minted).unwrap(), kas: 1 }];
        let verdict = check_family(
            &[FamilyInput { input_index: 0, state: rock, kas: 1 }],
            &transfer,
            &t,
            &outputs,
            &Bounds::default(),
        )
        .unwrap();
        assert!(!verdict.family_total_ok);
        assert!(verdict.violations.iter().any(|v| v.contains("family total")));
    }

    #[test]
    fn the_borrowed_path_reaches_verify_borrow_with_the_leader() {
        let t = test_template();
        let preimage = [0x5A; 32];
        let mut recipient = state(0x20);
        recipient.borrow_scheme = 0x03;
        recipient.borrow_guard = kcc1::hash(&preimage);
        let sender = State { borrow_scheme: 0x00, borrow_guard: [0; 32], ..state(0x20) };
        let mut rebuilt = recipient;
        rebuilt.amount += sender.amount;
        rebuilt.borrow_guard = preimage;
        let mut witness = vec![0x01];
        witness.extend_from_slice(&preimage);
        let transfer = TransferArgs { next_states: vec![rebuilt], witness };
        let outputs = vec![FamilyOutput { spk_version: 0, spk: t.reconstruct_spk(&rebuilt).unwrap(), kas: 7 }];
        let verdict = check_family(
            &[
                // the RECIPIENT's cell is the leader (lowest input index)
                FamilyInput { input_index: 1, state: recipient, kas: 7 },
                FamilyInput { input_index: 4, state: sender, kas: 2 },
            ],
            &transfer,
            &t,
            &outputs,
            &Bounds::default(),
        )
        .unwrap();
        assert_eq!(verdict.path_normal, Some(false));
        assert!(matches!(verdict.borrow, Some(BorrowVerdict::Proven(BorrowScheme::HashChain(_)))));
        assert!(verdict.family_total_ok);
        assert!(verdict.violations.is_empty(), "{:?}", verdict.violations);
    }

    #[test]
    fn a_fresh_successor_class_is_a_flat_breach() {
        let t = test_template();
        let input = state(0x10);
        let mut kept = state(0x10);
        kept.amount = input.amount - 3;
        let mut fresh = state(0x10);
        fresh.amount = 3;
        fresh.extension_commitment = [0xBB; 32]; // matches no input
        let mut witness = vec![0x00];
        witness.extend_from_slice(&[0u8; 65]);
        let transfer = TransferArgs { next_states: vec![kept, fresh], witness };
        let outputs = vec![
            FamilyOutput { spk_version: 0, spk: t.reconstruct_spk(&kept).unwrap(), kas: 1 },
            FamilyOutput { spk_version: 0, spk: t.reconstruct_spk(&fresh).unwrap(), kas: 1 },
        ];
        let verdict = check_family(
            &[FamilyInput { input_index: 0, state: input, kas: 1 }],
            &transfer,
            &t,
            &outputs,
            &Bounds::default(),
        )
        .unwrap();
        assert!(verdict.family_total_ok, "the total hides the class movement");
        assert!(!verdict.classes_preserved);
        assert!(verdict.violations.iter().any(|v| v.contains("no input carries")));
    }

    #[test]
    fn guards_of_the_verdict_layer_fail_closed() {
        let t = test_template();
        let input = state(0x10);
        let mut witness = vec![0x00];
        witness.extend_from_slice(&[0u8; 65]);
        // an empty declaration proves nothing
        let empty = TransferArgs { next_states: vec![], witness: witness.clone() };
        let verdict = check_family(
            &[FamilyInput { input_index: 0, state: input, kas: 1 }],
            &empty,
            &t,
            &[],
            &Bounds::default(),
        )
        .unwrap();
        assert!(!verdict.successors_proven);
        // a non-zero SPK version is not a KCC-1 envelope
        let out = state(0x10);
        let transfer = TransferArgs { next_states: vec![out], witness: witness.clone() };
        let verdict = check_family(
            &[FamilyInput { input_index: 0, state: input, kas: 1 }],
            &transfer,
            &t,
            &[FamilyOutput { spk_version: 1, spk: t.reconstruct_spk(&out).unwrap(), kas: 1 }],
            &Bounds::default(),
        )
        .unwrap();
        assert!(!verdict.successors_proven);
        assert!(verdict.violations.iter().any(|v| v.contains("version-0")));
        // wrong-shaped owner auth on the normal path is named
        let short = TransferArgs { next_states: vec![out], witness: vec![0x00, 0x01] };
        let verdict = check_family(
            &[FamilyInput { input_index: 0, state: input, kas: 1 }],
            &short,
            &t,
            &[FamilyOutput { spk_version: 0, spk: t.reconstruct_spk(&out).unwrap(), kas: 1 }],
            &Bounds::default(),
        )
        .unwrap();
        assert_eq!(verdict.owner_auth, None);
        assert!(verdict.violations.iter().any(|v| v.contains("owner-auth")));
        // an unproven borrowed receive can never be Proven
        let mut borrowed = input;
        borrowed.borrow_scheme = 0x03;
        borrowed.borrow_guard = kcc1::hash(&[0x5A; 32]);
        let mut bw = vec![0x01];
        bw.extend_from_slice(&[0x5A; 32]);
        let transfer = TransferArgs { next_states: vec![out], witness: bw };
        let verdict = check_family(
            &[FamilyInput { input_index: 0, state: borrowed, kas: 1 }],
            &transfer,
            &t,
            &[FamilyOutput { spk_version: 0, spk: kcc1::envelope_spk(&[0x51]), kas: 1 }],
            &Bounds::default(),
        )
        .unwrap();
        assert!(!verdict.successors_proven);
        assert!(matches!(verdict.borrow, Some(BorrowVerdict::Invalid(_))));
        // duplicate input indexes refuse outright
        assert!(check_family(
            &[
                FamilyInput { input_index: 0, state: input, kas: 1 },
                FamilyInput { input_index: 0, state: input, kas: 1 },
            ],
            &TransferArgs { next_states: vec![out], witness: vec![0x00] },
            &t,
            &[FamilyOutput { spk_version: 0, spk: t.reconstruct_spk(&out).unwrap(), kas: 1 }],
            &Bounds::default(),
        )
        .is_none());
    }

    /// A hostile program CAN contain a decodable 112-byte window — that is
    /// exactly why a candidate alone claims nothing. Only reconstruction
    /// against the actual family outputs confirms, and an adversary cannot
    /// mine outputs it does not control.
    #[test]
    fn a_planted_window_yields_a_candidate_but_never_a_confirmation() {
        let mut hostile = include_bytes!("../fixtures/kcc20_a_a.bin").to_vec();
        hostile.extend_from_slice(&state(0x5C).encode().unwrap());
        assert!(!locate_state_cuts(&hostile).is_empty(), "the planted window is found");
        assert_eq!(
            confirm_cut(&hostile, &[state(0x5D)], &[kcc1::envelope_spk(&[0x51])]),
            None,
            "and confirms nothing without cooperating outputs"
        );
    }

    // ── pins ─────────────────────────────────────────────────────

    fn fixture_bytes(name: &str) -> Vec<u8> {
        match name {
            "vector_template_a" => include_bytes!("../fixtures/kcc0020/vector_template_a.bin").to_vec(),
            "vector_template_b" => include_bytes!("../fixtures/kcc0020/vector_template_b.bin").to_vec(),
            "zealous_token_a" => include_bytes!("../fixtures/kcc0020/zealous_token_a.bin").to_vec(),
            "zealous_token_b" => include_bytes!("../fixtures/kcc0020/zealous_token_b.bin").to_vec(),
            "zealous_token_2641_a" => include_bytes!("../fixtures/kcc0020/zealous_token_2641_a.bin").to_vec(),
            "zealous_token_2641_b" => include_bytes!("../fixtures/kcc0020/zealous_token_2641_b.bin").to_vec(),
            "zealous_token_3037_a" => include_bytes!("../fixtures/kcc0020/zealous_token_3037_a.bin").to_vec(),
            "zealous_token_3037_b" => include_bytes!("../fixtures/kcc0020/zealous_token_3037_b.bin").to_vec(),
            "zealous_token_3223_a" => include_bytes!("../fixtures/kcc0020/zealous_token_3223_a.bin").to_vec(),
            "zealous_token_3223_b" => include_bytes!("../fixtures/kcc0020/zealous_token_3223_b.bin").to_vec(),
            other => panic!("a pin names fixture {other} that this test does not carry"),
        }
    }

    #[test]
    fn every_committed_pin_parses_and_names_this_spec() {
        for pin in pins() {
            assert_eq!(pin.spec_commit, SPEC_COMMIT);
            assert_eq!(pin.template_hash.len(), 64);
            assert!(pin.template_hash.chars().all(|c| c.is_ascii_hexdigit()));
            assert_eq!(pin.state.len, STATE_LEN);
            assert!(!pin.review.fixtures.is_empty(), "a pin without fixtures proves nothing");
            assert!(matches!(pin.review.status.as_str(), "reviewed" | "unreviewed"));
            // A declared family is a venue-owned display name: it carries the
            // "Builder · " separator so a prefix rule can own it, and it must
            // not collide with any name the legacy decoders key on, or a
            // spec program would be routed into the 46-byte splice by name.
            if let Some(family) = &pin.family {
                assert!(family.contains(" · "), "{family}: a pin family carries the venue separator");
                assert_ne!(family, SPEC_TOKEN_TEMPLATE);
                assert_ne!(family, crate::kcc20::TOKEN_TEMPLATE);
                assert!(
                    crate::kcc20::CELL_FAMILIES.iter().all(|f| f.template != family),
                    "{family} is a legacy cell family name"
                );
                assert!(
                    crate::observed::observed_skeletons().iter().all(|s| s.name != family),
                    "{family} is an observed skeleton name"
                );
                assert_eq!(pin.family_label(), family);
            } else {
                assert_eq!(pin.family_label(), SPEC_TOKEN_TEMPLATE);
            }
            assert!(is_spec_template(pin.family_label()));
            // The claimed template hash is never trusted from the file: every
            // fixture must recompute it at the pin's own cut, and the pair
            // must differ in every state field so the pin cannot silently
            // narrow (the two-sentinel discipline).
            let programs: Vec<Vec<u8>> = pin.review.fixtures.iter().map(|f| fixture_bytes(f)).collect();
            assert!(programs.len() >= 2, "a pin needs a fixture pair");
            for program in &programs {
                let template = Template::cut(program, pin.state.start).expect("fixture cuts");
                assert_eq!(hex::encode(template.hash()), pin.template_hash, "fixture recomputes the pin's hash");
                assert!(pinned_program_state(program).is_some(), "the pin recognises its own fixture");
            }
            let sa = State::decode(&programs[0][pin.state.start..pin.state.start + STATE_LEN]).unwrap();
            let sb = State::decode(&programs[1][pin.state.start..pin.state.start + STATE_LEN]).unwrap();
            assert_ne!(sa.amount, sb.amount);
            assert_ne!(sa.owner, sb.owner);
            assert_ne!(sa.owner_scheme, sb.owner_scheme);
            // A pin whose fixtures are two cells of the one launch the chain
            // carries of its build cannot show the class varying; it says
            // so, and must then prove the window's end from the program's
            // own bytes (the pin note and the ladder carry that proof).
            if pin.review.fixtures_share_class {
                assert_eq!(sa.extension_commitment, sb.extension_commitment, "the flag is not a free pass");
                assert!(pin.review.note.contains("fixtures_share_class"));
            } else {
                assert_ne!(sa.extension_commitment, sb.extension_commitment);
            }
            // A pin that declares ONE borrow scheme fixes the borrow fields by
            // policy, so its fixtures cannot differ there (every cell of such
            // a family carries the same scheme and guard on chain); they must
            // carry exactly the declared scheme instead. A pin declaring
            // several either shows the pair differing (sentinel compiles
            // chosen for that, like the vector's) or, when its fixtures are
            // chain-fetched cells that happen to agree, carries a scheme the
            // pin declares (the Zealous pair: both cells unborrowable).
            if pin.borrow_schemes.len() == 1 {
                assert_eq!(sa.borrow_scheme, pin.borrow_schemes[0]);
                assert_eq!(sb.borrow_scheme, pin.borrow_schemes[0]);
            } else if sa.borrow_scheme == sb.borrow_scheme {
                assert!(pin.borrow_schemes.contains(&sa.borrow_scheme), "a shared fixture scheme must be one the pin declares");
            } else {
                assert_ne!(sa.borrow_guard, sb.borrow_guard);
            }
        }
        // an unpinned program is never claimed, spec-shaped or not
        let mut foreign = vec![0x6b];
        foreign.extend_from_slice(&state(0x11).encode().unwrap());
        foreign.extend_from_slice(&[0x6c, 0x75, 0x75, 0x51]);
        assert!(pinned_program_state(&foreign).is_none());
        // the committed set: all four Zealous pins declare the venue family
        // (one label, so the candidate list did not widen), the reference
        // vector deliberately keeps the spec's own name
        let h32 = |s: &str| -> [u8; 32] { h(s).try_into().unwrap() };
        assert_eq!(pin_for(&h32("653012f1e68160bd87d6ac478ff3167bd154059b1c98b6dc76fd56b7ed3411cf")).unwrap().family_label(), "Zealous · token");
        assert_eq!(pin_for(&h32("2f2671fcbe40e7acb8c3992cd61a5a68b2f8b47f983dd5af1612d4d38dfebed3")).unwrap().family_label(), "Zealous · token");
        assert_eq!(pin_for(&h32("58c24260f2d81c34fc5ad0890f49f9670297cc6ac0a5136f6131b16a04540728")).unwrap().family_label(), "Zealous · token");
        assert_eq!(pin_for(&h32("50ff50d8f65a057b5a14f26fa976340ee28e22e5a68328e847bd1707e7831f09")).unwrap().family_label(), "Zealous · token");
        assert_eq!(pin_for(&h32("4abcc18873220d197d2dd5ddf46b24180bdb7fac48afd19f80bae6e1dd64e7eb")).unwrap().family_label(), SPEC_TOKEN_TEMPLATE);
        assert_eq!(spec_template_names(), vec![SPEC_TOKEN_TEMPLATE, "Zealous · token"]);
        assert!(is_spec_template("Zealous · token"));
        assert!(!is_spec_template("Zealous · token cell"), "the legacy 46-byte family is not a spec label");
        assert!(!is_spec_template(crate::kcc20::TOKEN_TEMPLATE));
        // the schema itself is exercised even while the pin set is empty
        let sample = r#"{
            "spec_commit": "ea5176aa",
            "template_hash": "2ed46a7edf5b168e67dba56998c58255235bebac436940a85115ca31d5c559f2",
            "name": "sample",
            "family": "Sample · token",
            "dispatch": {"kind": "silverc-selector", "transfer": "0", "transfer_delegator": "1"},
            "state": {"start": 1, "len": 112},
            "owner_schemes": [0, 3],
            "max_token_inputs": 3,
            "max_token_outputs": 3,
            "borrow_schemes": [0, 1, 3],
            "extension": {"scheme": "opaque"},
            "entrypoints": [
                {"name": "transfer", "types": "KCC20State[],byte[]", "select": "0", "role": "transfer"}
            ],
            "in_script_template_hash_fn": "blake2b",
            "review": {"status": "unreviewed", "fixtures": ["vector_a", "vector_b"], "note": "sample"}
        }"#;
        let pin: Pin = serde_json::from_str(sample).unwrap();
        assert_eq!(pin.dispatch.kind, DispatchKind::SilvercSelector);
        assert_eq!(pin.bounds().max_token_inputs, 3);
        assert_eq!(pin.family.as_deref(), Some("Sample · token"), "the family round-trips");
        assert_eq!(pin.family_label(), "Sample · token");
        // the field is optional: a pin written before families existed
        // still parses and stamps the generic label
        let bare: Pin = serde_json::from_str(&sample.replace("\"family\": \"Sample · token\",\n", "")).unwrap();
        assert_eq!(bare.family, None);
        assert_eq!(bare.family_label(), SPEC_TOKEN_TEMPLATE);
        assert!(pin_for(&[0u8; 32]).is_none());
        assert_eq!(h("00"), vec![0x00]);
    }

    // ── KCC-1 tag dispatch (ZealousSwap token cell) ──────────────────────
    fn zealous_pin() -> &'static Pin {
        pins().iter().find(|p| p.template_hash.starts_with("653012f1")).expect("pin committed")
    }

    fn tag_script_for(program: &[u8], args: &[&[u8]], tag: &str) -> Vec<u8> {
        let mut sig = Vec::new();
        for a in args {
            sig.extend(crate::encode_push(a));
        }
        sig.extend(crate::encode_push(&h(tag)));
        sig.extend(crate::encode_push(program));
        sig
    }

    fn tag_script(args: &[&[u8]], tag: &str) -> Vec<u8> {
        tag_script_for(include_bytes!("../fixtures/kcc0020/zealous_token_a.bin"), args, tag)
    }

    /// Verbatim from testnet-10 spend 8df38dc9…:1 of WBLF's token cell: the
    /// curve sends 25,000,000,000,000,010 base units to the pool (owner
    /// scheme 0x04) and keeps the remainder itself.
    #[test]
    fn a_real_zealous_transfer_declares_one_successor() {
        let pin = zealous_pin();
        let sig = tag_script(
            &[
                &h("0a8062175ed158"),
                &h("f5d20a2fb9c32a5c87f9081a5348f7bc540ef0a9080dea59e8ce3875e3ea1cfe"),
                &[0x04],
                &[0x00],
                &[0u8; 32],
                &[0u8; 65],
                &[0u8; 32],
                &[],
            ],
            "ddeb2d66",
        );
        let got = parse_tag_reveal(&sig, pin).expect("parses");
        let TagReveal::Transfer { amount, owner, owner_scheme, borrow_scheme, .. } = got else {
            panic!("not a transfer: {got:?}");
        };
        assert_eq!(amount, 25_000_000_000_000_010);
        assert_eq!(hex::encode(owner), "f5d20a2fb9c32a5c87f9081a5348f7bc540ef0a9080dea59e8ce3875e3ea1cfe");
        assert_eq!((owner_scheme, borrow_scheme), (0x04, 0x00));
    }

    #[test]
    fn merge_whole_and_follower_reveals_are_told_apart() {
        let pin = zealous_pin();
        let merge = tag_script(&[&[0u8; 65], &[0u8; 32], &[]], "bdaf5113");
        assert_eq!(parse_tag_reveal(&merge, pin), Some(TagReveal::Merge));
        let whole = tag_script(&[&[0x11u8; 32], &[0x01], &[0x00], &[0u8; 32], &[0u8; 65], &[0u8; 32], &[]], "5f4c9820");
        assert!(matches!(parse_tag_reveal(&whole, pin), Some(TagReveal::Whole { owner_scheme: 0x01, .. })));
        let follower = tag_script(&[&[0u8; 65], &[0u8; 32], &[]], "d64f42e5");
        assert_eq!(parse_tag_reveal(&follower, pin), None, "a follower is never a leader");
        let unknown = tag_script(&[&[0u8; 65], &[0u8; 32], &[]], "deadbeef");
        assert_eq!(parse_tag_reveal(&unknown, pin), None);
        // a selector-dispatched pin never parses a tag script
        let vector = pins().iter().find(|p| p.dispatch.kind == DispatchKind::SilvercSelector).unwrap();
        assert_eq!(parse_tag_reveal(&merge, vector), None);
    }

    /// The borrowed receive's four arguments: the amount received, the
    /// hash-chain preimage slot, the guard-key signature slot, and what the
    /// follower keeps (an empty push when it keeps nothing).
    #[test]
    fn a_borrowed_receive_reveal_parses_with_an_empty_remainder() {
        let pin = zealous_pin();
        let full = tag_script(&[&crate::snum(250), &[0x5A; 32], &[0x7B; 65], &crate::snum(150)], "a85e5b91");
        let Some(TagReveal::Borrowed { amount, preimage, signature, follower_change }) = parse_tag_reveal(&full, pin) else {
            panic!("not a borrowed receive");
        };
        assert_eq!((amount, preimage, signature, follower_change), (250, [0x5A; 32], [0x7B; 65], 150));
        let nothing_kept = tag_script(&[&crate::snum(400), &[0u8; 32], &[0u8; 65], &[]], "a85e5b91");
        assert!(matches!(parse_tag_reveal(&nothing_kept, pin), Some(TagReveal::Borrowed { amount: 400, follower_change: 0, .. })));
        // a zero amount is not a receive; a torn slot is not a reveal
        let zero = tag_script(&[&[], &[0u8; 32], &[0u8; 65], &[]], "a85e5b91");
        assert_eq!(parse_tag_reveal(&zero, pin), None);
        let torn = tag_script(&[&crate::snum(250), &[0u8; 31], &[0u8; 65], &[]], "a85e5b91");
        assert_eq!(parse_tag_reveal(&torn, pin), None);
    }

    #[test]
    fn a_negative_or_oversized_amount_is_refused() {
        let pin = zealous_pin();
        let neg = tag_script(&[&h("80"), &[0x22u8; 32], &[0x04], &[0x00], &[0u8; 32], &[0u8; 65], &[0u8; 32], &[]], "ddeb2d66");
        assert_eq!(parse_tag_reveal(&neg, pin), None);
        let wide = tag_script(&[&[1u8; 9], &[0x22u8; 32], &[0x04], &[0x00], &[0u8; 32], &[0u8; 65], &[0u8; 32], &[]], "ddeb2d66");
        assert_eq!(parse_tag_reveal(&wide, pin), None);
    }

    // ── the 2,641 B generation (ZLNCH) and the two after it ──────────────
    const ZEALOUS_2641_A: &[u8] = include_bytes!("../fixtures/kcc0020/zealous_token_2641_a.bin");
    const ZEALOUS_3037_A: &[u8] = include_bytes!("../fixtures/kcc0020/zealous_token_3037_a.bin");
    const ZEALOUS_3037_B: &[u8] = include_bytes!("../fixtures/kcc0020/zealous_token_3037_b.bin");
    const ZEALOUS_3223_A: &[u8] = include_bytes!("../fixtures/kcc0020/zealous_token_3223_a.bin");
    const ZEALOUS_3223_B: &[u8] = include_bytes!("../fixtures/kcc0020/zealous_token_3223_b.bin");

    const HASH_2590: &str = "653012f1e68160bd87d6ac478ff3167bd154059b1c98b6dc76fd56b7ed3411cf";
    const HASH_2641: &str = "2f2671fcbe40e7acb8c3992cd61a5a68b2f8b47f983dd5af1612d4d38dfebed3";
    const HASH_3037: &str = "58c24260f2d81c34fc5ad0890f49f9670297cc6ac0a5136f6131b16a04540728";
    const HASH_3223: &str = "50ff50d8f65a057b5a14f26fa976340ee28e22e5a68328e847bd1707e7831f09";

    fn zealous_2641_pin() -> &'static Pin {
        pins().iter().find(|p| p.template_hash.starts_with("2f2671fc")).expect("pin committed")
    }

    /// The four Zealous generations on testnet-10, one fixture each: the
    /// pinned 2,590 B build and the three after it.
    fn zealous_generations() -> [(&'static [u8], usize, &'static str); 4] {
        [
            (include_bytes!("../fixtures/kcc0020/zealous_token_a.bin"), 2_590, HASH_2590),
            (ZEALOUS_2641_A, 2_641, HASH_2641),
            (ZEALOUS_3037_A, 3_037, HASH_3037),
            (ZEALOUS_3223_A, 3_223, HASH_3223),
        ]
    }

    /// The generation that followed the pinned 2,590 B build is its own pin
    /// under the same venue family; neither pin claims the other's program.
    #[test]
    fn the_2641_generation_is_named_by_its_own_pin() {
        let (pin, state) = pinned_program_state(ZEALOUS_2641_A).expect("the 2,641 B reveal is recognised");
        assert_eq!(pin.template_hash, HASH_2641);
        assert_eq!(pin.family_label(), "Zealous · token");
        assert!(pin.review.fixtures_share_class);
        // ZLNCH's platform-fee cell: 1e14 base units under scheme 0x01
        assert_eq!(state.amount, 100_000_000_000_000);
        assert_eq!(state.owner_scheme, OWNER_P2PKH_SCHNORR);
        assert_eq!(hex::encode(state.owner), "01150ea3137303565132988cbd65d1bb0a2e8cfcf3551d7d976221a5442b4924");
        assert_eq!(ZEALOUS_2641_A.len(), 2_641);
        // the pinned 2,590 B build still names as itself
        let (old, _) = pinned_program_state(include_bytes!("../fixtures/kcc0020/zealous_token_a.bin")).unwrap();
        assert!(old.template_hash.starts_with("653012f1"));
        assert_ne!(old.template_hash, pin.template_hash);
        // the borrowed tag moved with the generation: 13f87a5b is in this
        // program, a85e5b91 is not, and the other four tags are shared
        let has = |program: &[u8], tag: &str| program.windows(4).any(|w| w == h(tag).as_slice());
        assert!(has(ZEALOUS_2641_A, "13f87a5b") && !has(ZEALOUS_2641_A, "a85e5b91"));
        for tag in ["5f4c9820", "ddeb2d66", "bdaf5113", "d64f42e5"] {
            assert!(has(ZEALOUS_2641_A, tag), "{tag}");
        }
        assert_eq!(zealous_2641_pin().entrypoints.iter().find(|e| e.select == "13f87a5b").unwrap().types, "int,byte[32],byte[32],byte[65],byte[]");
    }

    /// The 3,037 B and 3,223 B generations are each named by their own pin
    /// under the same venue family, and each of the four Zealous pins
    /// refuses the other three generations by hash: every program cuts
    /// cleanly at offset 1, so nothing is refused by a failed cut, only by
    /// the template hash the pin is keyed on.
    #[test]
    fn the_later_generations_are_named_and_each_pin_refuses_the_other_three_by_hash() {
        for (program, len, hash) in zealous_generations() {
            assert_eq!(program.len(), len);
            let (pin, state) = pinned_program_state(program).unwrap_or_else(|| panic!("{len} B is named"));
            assert_eq!(pin.template_hash, hash, "{len} B is named by its own pin");
            assert_eq!(pin.family_label(), "Zealous · token");
            assert_eq!(pin.review.status, "reviewed");
            // fixture A of each generation after 2,590 B is the venue's
            // platform-key-owned cell (2,590 B's is a pool-owned one)
            if len > 2_590 {
                assert_eq!(state.owner_scheme, OWNER_P2PKH_SCHNORR);
                assert_eq!(hex::encode(state.owner), "01150ea3137303565132988cbd65d1bb0a2e8cfcf3551d7d976221a5442b4924");
            }
            let own = pin_for(&Template::cut(program, 1).unwrap().hash()).expect("keyed by hash");
            assert_eq!(own.template_hash, hash);
            for (other, other_len, other_hash) in zealous_generations() {
                if other_hash == hash {
                    continue;
                }
                let template = Template::cut(other, pin.state.start).expect("every generation keeps the window at offset 1");
                assert_ne!(hex::encode(template.hash()), pin.template_hash, "{len} B's pin does not claim {other_len} B");
                assert_ne!(pin_for(&template.hash()).unwrap().template_hash, pin.template_hash);
            }
        }
        // the second fixture of each later generation is another launch's
        // pool-owned cell, so the pair varies in every state field
        for (a, b, hash) in [(ZEALOUS_3037_A, ZEALOUS_3037_B, HASH_3037), (ZEALOUS_3223_A, ZEALOUS_3223_B, HASH_3223)] {
            let (pin_a, sa) = pinned_program_state(a).unwrap();
            let (pin_b, sb) = pinned_program_state(b).unwrap();
            assert_eq!(pin_a.template_hash, hash);
            assert_eq!(pin_b.template_hash, hash);
            assert!(!pin_a.review.fixtures_share_class, "two launches, so the class varies without the relaxation");
            assert_eq!(sb.owner_scheme, OWNER_COVENANT_ID);
            assert_ne!(sa.extension_commitment, sb.extension_commitment);
            assert_ne!(sa.owner, sb.owner);
            assert_ne!(sa.amount, sb.amount);
        }
        // MPC's platform cell and FFF's: 1e14 base units each
        assert_eq!(pinned_program_state(ZEALOUS_3037_A).unwrap().1.amount, 100_000_000_000_000);
        assert_eq!(pinned_program_state(ZEALOUS_3223_A).unwrap().1.amount, 100_000_000_000_000);
        // the same five tags in the same order, the 2,590 B tag absent
        let has = |program: &[u8], tag: &str| program.windows(4).any(|w| w == h(tag).as_slice());
        for program in [ZEALOUS_3037_A, ZEALOUS_3223_A] {
            for tag in ["5f4c9820", "ddeb2d66", "bdaf5113", "13f87a5b", "d64f42e5"] {
                assert!(has(program, tag), "{tag}");
            }
            assert!(!has(program, "a85e5b91"));
        }
        // and the later pins declare the 2,641 B argument lists verbatim
        let entry_types = |hash: &str| -> Vec<(String, String)> {
            let pin = pins().iter().find(|p| p.template_hash == hash).unwrap();
            pin.entrypoints.iter().map(|e| (e.select.clone(), e.types.clone())).collect()
        };
        assert_eq!(entry_types(HASH_3037), entry_types(HASH_2641));
        assert_eq!(entry_types(HASH_3223), entry_types(HASH_2641));
    }

    /// The borrowed receive's argument list is the pin's declared one: the
    /// 2,590 B build's four slots, the three later generations' five. Each
    /// pin parses only its own layout, and the key slot must be 32 bytes.
    #[test]
    fn the_keyed_borrowed_receive_parses_by_its_declared_layout() {
        for (program, hash) in [(ZEALOUS_2641_A, HASH_2641), (ZEALOUS_3037_A, HASH_3037), (ZEALOUS_3223_A, HASH_3223)] {
            let pin = pins().iter().find(|p| p.template_hash == hash).expect("pin committed");
            let keyed = |args: &[&[u8]]| tag_script_for(program, args, "13f87a5b");
            let full = keyed(&[&crate::snum(250), &[0x5A; 32], &[0x6B; 32], &[0x7B; 65], &crate::snum(150)]);
            assert_eq!(
                parse_tag_reveal(&full, pin),
                Some(TagReveal::Borrowed { amount: 250, preimage: [0x5A; 32], signature: [0x7B; 65], follower_change: 150 })
            );
            let nothing_kept = keyed(&[&crate::snum(400), &[0u8; 32], &[0u8; 32], &[0u8; 65], &[]]);
            assert!(matches!(parse_tag_reveal(&nothing_kept, pin), Some(TagReveal::Borrowed { amount: 400, follower_change: 0, .. })));
            // the 2,590 B build's four-slot shape is not this arm
            let four = keyed(&[&crate::snum(250), &[0x5A; 32], &[0x7B; 65], &crate::snum(150)]);
            assert_eq!(parse_tag_reveal(&four, pin), None);
            // a torn key slot, a zero amount, a negative remainder
            assert_eq!(parse_tag_reveal(&keyed(&[&crate::snum(250), &[0x5A; 32], &[0x6B; 31], &[0x7B; 65], &[]]), pin), None);
            assert_eq!(parse_tag_reveal(&keyed(&[&[], &[0x5A; 32], &[0x6B; 32], &[0x7B; 65], &[]]), pin), None);
            assert_eq!(parse_tag_reveal(&keyed(&[&crate::snum(401), &[0x5A; 32], &[0x6B; 32], &[0x7B; 65], &h("81")]), pin), None);
            // the retired tag is not an entrypoint of this pin, and the 2,590 B
            // pin does not know the new one
            assert_eq!(parse_tag_reveal(&tag_script_for(program, &[&crate::snum(250), &[0x5A; 32], &[0x7B; 65], &[]], "a85e5b91"), pin), None);
            assert_eq!(parse_tag_reveal(&full, zealous_pin()), None);
            // the shared arms parse the same way under every pin
            let whole = tag_script_for(program, &[&[0x11u8; 32], &[0x01], &[0x00], &[0u8; 32], &[0u8; 65], &[0u8; 32], &[]], "5f4c9820");
            assert!(matches!(parse_tag_reveal(&whole, pin), Some(TagReveal::Whole { owner_scheme: 0x01, .. })));
            assert_eq!(parse_tag_reveal(&tag_script_for(program, &[&[0u8; 65], &[0u8; 32], &[]], "bdaf5113"), pin), Some(TagReveal::Merge));
            assert_eq!(parse_tag_reveal(&tag_script_for(program, &[&[0u8; 65], &[0u8; 32], &[]], "d64f42e5"), pin), None);
        }
    }
}
