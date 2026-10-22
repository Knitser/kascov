//! KCC-20 conformance primitives — the fungible token convention
//! (kaspanet/kccs kcc-0020.md, tracking the Final text: Final at kccs
//! 1ed87ae, its text unchanged since 1516a6c), byte layouts and transition
//! rules.
//!
//! Everything here is verifiable from bytes plus one declared input, a pin
//! (kascov's committed description of a program, keyed by its KCC-1 template
//! hash). Recognising a shape is not proving a state, and nothing here
//! publishes a balance.
//!
//! The rules moved between the merged Draft (ea5176aa) and Last Call, whose
//! text went Final unchanged: the hash-chain borrow commits to a one-time
//! key (#18), a negative threshold behaves as zero (#31), P2PKH owners hash
//! unkeyed (KCC-2 #30), and every input and output of a standard transfer
//! shares one extension commitment (#16). A program keeps the rules it was
//! compiled with, so each pin states which ones its bytes check ([`Rules`]);
//! a pin that says nothing gets the Final rules.
//!
//! Seven pinned programs carry this layout on chain. Five were
//! compiled against the Draft: kascov's own reduced vector and four
//! generations of ZealousSwap's token cell, each read and exercised on the
//! engine before it was pinned, and none of them is conformant to the Final
//! text ([`Pin::conformant`]). The sixth is the KCC20 actor of the
//! official reference app (argent-lang/kcc20-reference c8a0871), read from
//! its published artifact, then reviewed like the others: its fixtures are
//! leader reveals fetched from mainnet and testnet-10, every arm was read
//! instruction by instruction, and the real engine exercised every rule
//! with each check attributed by ablation (kascov-labkit's
//! `kcc20_reference`). The seventh is KaspaRocket's token program
//! (ff3ae732): that actor with its delegate and output bounds raised to 9
//! and 10, aligned against the reference's bytes and run through the same
//! ladder at the new bounds (`kcc20_kasparocket`). Its publisher released no
//! program artifact, so kascov gives that build no conformance verdict
//! either way ([`PinArtifact`]). Every other deployed
//! token (KRON, KaspaCom, KaspaKaha) uses the gist-era 46-byte state that
//! `kcc20.rs` reads; the spec state is six fields, 112 bytes encoded, with
//! different scheme bytes. No shipped legacy build carries a
//! decodable spec window (tests check every fixture, both directions), and a
//! window alone never claims a program anyway: only reconstruction against
//! the family outputs does, and that is the part an adversary cannot forge.
//!
//! The one structural gift of the spec: output states are PROVEN, not
//! recovered. The leader's `transfer` arguments declare every successor
//! state, and `Blake2b(prefix ‖ EncodeState(next_states[i]) ‖ suffix)` must
//! equal the i-th covenant-family output's P2SH commitment. So the reader
//! reconstructs and compares, and the witness brute-force the 46-byte layout
//! needs never runs here.

use crate::encode_push;
use crate::kcc1;
use crate::kcc2;

use serde::Deserialize;
use std::fmt;
use std::sync::OnceLock;

/// The kccs commit whose text this module tracks: KCC-20 Final (1ed87ae,
/// 2026-10-21). Its text is unchanged since the Last Call commit 1516a6c,
/// which this constant named before: the diff between the two is the status
/// lines alone, and the vendored vectors are byte-identical at both. A pin
/// described against it can be judged conformant ([`Pin::conformant`]); a
/// spec change re-derives everything, and fixtures are regenerated, never
/// patched.
pub const SPEC_COMMIT: &str = "1ed87ae";

/// The merged Draft the first five pins were reviewed against. Its rules
/// stay readable per pin ([`Rules`]), because the programs compiled against
/// it check them on chain for good.
pub const DRAFT_SPEC_COMMIT: &str = "ea5176aa";

/// The name a TOKEN is shown under when its build is a reviewed pin that
/// departs from no rule of the Final text ([`REVIEWED_BUILD_TEMPLATE`]) and the
/// token itself is verified: its whole history replayed under that build.
/// A token-level name, never stamped on a reveal: a cell names a build, and
/// a build being reviewed says nothing about whether one token's genesis
/// and issues are proven. The accounting layer, which knows the token's
/// verdict, picks it.
pub const SPEC_TOKEN_TEMPLATE: &str = "KCC-20 token";

/// The template name a pinned build without a family of its own is stamped
/// with once kascov has reviewed it against [`SPEC_COMMIT`]: its bytes were
/// reviewed and no declared rule departs. That is a conformance verdict
/// ([`Pin::conformant`]) unless the build's program artifact is unpublished,
/// which leaves the verdict open without making the build any less reviewed
/// (see [`PinArtifact`]). It names the build, so every cell
/// of it carries it whatever its token's standing; a token is shown as a
/// [`SPEC_TOKEN_TEMPLATE`] only once it is verified. A pin that declares a
/// `family` stamps that instead (see [`Pin::family_label`]);
/// [`is_spec_template`] recognises every label.
pub const REVIEWED_BUILD_TEMPLATE: &str = "reviewed KCC-20 build";

/// The name a token of a [`REVIEWED_BUILD_TEMPLATE`] build is shown under
/// while kascov has not proven the token itself (its genesis or an issue it
/// cannot read): the build is reviewed, the token is not a KCC-20 token in
/// kascov's words yet. Token-level, like [`SPEC_TOKEN_TEMPLATE`].
pub const UNPROVEN_TOKEN_TEMPLATE: &str = "reviewed KCC-20 build, this token is not proven yet";

/// The name for a build described against [`SPEC_COMMIT`] that kascov has
/// not judged conformant (an unreviewed pin, say): it carries the state
/// layout, and the name claims nothing more.
pub const LAYOUT_TEMPLATE: &str = "KCC-20 state layout";

/// The name for a build compiled against a Draft before Last Call. Such a
/// program carries the state layout but checks rules the spec has since
/// replaced, so it must never read as a token of the standard.
pub const DRAFT_BUILD_TEMPLATE: &str = "KCC-20 state layout, pre-Final draft build";

/// The generic name rows were stamped with before the Last Call labels.
/// Still recognised, so a stored row keeps routing as a spec cell until the
/// re-stamp pass rewrites it.
pub const RETIRED_SPEC_TEMPLATE: &str = "KCC-0020 token";

/// The default configuration's entrypoint signatures (§3) written under
/// KCC-1 §3.5.1, and their dispatch tags. §3.5.1 expands a record to its
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
    /// rejects a set KCC-1 §3.4.3 sign bit, negative zero included.
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

/// The KCC-1 §3.7.1 encoded width: six `PushExplicit` fields,
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

    /// Strict KCC-1 §3.7.1 decode: exactly 112 bytes, the six explicit pushes at
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

    /// KCC-1 §3.7.3 TemplateHash (BLAKE3) — the template's canonical name.
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
/// and the true `state.start` is artifact knowledge (K1 §3.4.7).
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
/// §3.6); `check_family` enforces that with [`FamilyOutput::spk_version`].
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
/// argument payloads. A `KCC20State[]` lowers field-major (K1 §3.4.6): six
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
/// under the `P2PKHHash` the program checks (KCC-2, per [`Rules::p2pkh_hash`]).
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
/// the owner under `p2pkh` (an ECDSA key must also be in compressed form,
/// KCC-2's rule). A `Some` never claims a signature was valid.
pub fn owner_auth(state: &State, auth: &[u8], p2pkh: kcc2::P2pkhHash) -> Option<OwnerAuth> {
    match state.owner_scheme {
        OWNER_P2PK_SCHNORR => (auth.len() == 65).then_some(OwnerAuth::P2pkSignature),
        OWNER_P2PKH_SCHNORR => {
            if auth.len() != 32 + 65 {
                return None;
            }
            let pubkey: [u8; 32] = auth[..32].try_into().ok()?;
            kcc2::bind_public_key(kcc2::Scheme::P2pkhSchnorr, &state.owner, &pubkey, p2pkh).ok()?;
            Some(OwnerAuth::P2pkhSchnorr { pubkey })
        }
        OWNER_P2PKH_ECDSA => {
            if auth.len() != 33 + 65 {
                return None;
            }
            let pubkey: [u8; 33] = auth[..33].try_into().ok()?;
            kcc2::bind_public_key(kcc2::Scheme::P2pkhEcdsa, &state.owner, &pubkey, p2pkh).ok()?;
            Some(OwnerAuth::P2pkhEcdsa { pubkey })
        }
        OWNER_P2SH => {
            let [index] = auth else { return None };
            Some(OwnerAuth::P2shAuthorityInput(*index))
        }
        OWNER_COVENANT_ID => auth.is_empty().then_some(OwnerAuth::CovenantId),
        _ => None,
    }
}

/// Which hash-chain borrow rule a program checks. Per program, because a
/// program keeps the rule it was compiled with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HashChainRule {
    /// #18 (Last Call): the witness is exactly `next_borrow_guard[32] ‖
    /// one_time_pubkey[32] ‖ sig[65]`, `Hash(next ‖ pubkey)` must equal the
    /// current guard, the guard advances to `next`, and the one-time key
    /// must sign. The signature is consensus's to verify, so the best this
    /// reader can say is structural.
    #[default]
    Keyed,
    /// The Draft's rule (ea5176aa): the witness is a 32-byte preimage that
    /// must hash to the guard, which then advances to it. Anyone who sees the
    /// preimage in the mempool can race the borrow, which is why #18 replaced
    /// it, but the programs compiled against it check it on chain for good.
    Preimage,
}

/// The rules a pinned program checks wherever the Draft and the Final text
/// differ. The default is the Final text (the Last Call text, unchanged), so
/// a pin that declares nothing is read by the current rules, never by a
/// retired one. Each rule is the pin's own, never derived from
/// [`SPEC_COMMIT`]: moving that constant, as it moved from the Last Call
/// commit to the Final one, must not change what an existing build is held
/// to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    #[serde(default)]
    pub hash_chain: HashChainRule,
    /// How a p2pkh owner is bound to the revealed key (KCC-2 #30 made it
    /// unkeyed; the Draft's programs check the keyed form).
    #[serde(default)]
    pub p2pkh_hash: kcc2::P2pkhHash,
    /// The program refuses an amount-threshold guard whose DECODED int is
    /// below zero. The spec lets a contract require a non-negative threshold
    /// (#31 MAY); without this flag a negative one acts as an effective
    /// threshold of zero. Negative zero decodes to 0 and passes: every pinned
    /// program that sets this flag reads the guard with OpBin2Num and
    /// compares, and the engine accepts a `-0` guard under each of them.
    #[serde(default)]
    pub threshold_non_negative: bool,
    /// #16 (Last Call): a standard transfer carries ONE extension
    /// commitment, so a family transition that moves several classes side by
    /// side breaches it even when each class balances. The Draft allowed
    /// that, and a build of it is held to per-class balance alone.
    #[serde(default = "Rules::last_call_single_class")]
    pub single_class: bool,
}

impl Rules {
    fn last_call_single_class() -> bool {
        true
    }
}

impl Default for Rules {
    fn default() -> Rules {
        Rules {
            hash_chain: HashChainRule::default(),
            p2pkh_hash: kcc2::P2pkhHash::default(),
            threshold_non_negative: false,
            single_class: Rules::last_call_single_class(),
        }
    }
}

/// The fixed borrow-scheme mapping (§3, restated with guard semantics in §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorrowScheme {
    Disabled,
    /// The guard's first eight bytes as a KCC-1 signed-magnitude int,
    /// negative zero read as zero. The increase must strictly exceed
    /// `max(threshold, 0)`; guard bytes 8..32 carry no meaning but must be
    /// preserved with the rest of the guard (§5).
    AmountThreshold(i64),
    /// The guard is a Schnorr pubkey; validity of the witness signature is
    /// consensus's, so a check here can only ever be structural.
    SchnorrSignature([u8; 32]),
    /// The guard is the current chain head; each borrow reveals the next
    /// head and advances the guard to it, under the program's
    /// [`HashChainRule`].
    HashChain([u8; 32]),
    Reserved(u8),
}

impl BorrowScheme {
    /// Read a leader's borrow scheme and guard. `Err` only for a threshold
    /// that decodes below zero under a program that requires non-negative.
    pub fn of(state: &State, rules: &Rules) -> Result<BorrowScheme, BorrowRefusal> {
        Ok(match state.borrow_scheme {
            0x00 => BorrowScheme::Disabled,
            0x01 => {
                let mut magnitude = [0u8; 8];
                magnitude.copy_from_slice(&state.borrow_guard[..8]);
                let sign = magnitude[7] & 0x80 != 0;
                magnitude[7] &= 0x7f;
                // 63 magnitude bits always fit an i64
                let magnitude = u64::from_le_bytes(magnitude) as i64;
                // the sign bit over a zero magnitude is negative zero, which
                // decodes to 0: the rule is on the value, not on the bit
                let threshold = if sign { -magnitude } else { magnitude };
                if threshold < 0 && rules.threshold_non_negative {
                    return Err(BorrowRefusal::NegativeThreshold);
                }
                BorrowScheme::AmountThreshold(threshold)
            }
            0x02 => BorrowScheme::SchnorrSignature(state.borrow_guard),
            0x03 => BorrowScheme::HashChain(state.borrow_guard),
            other => BorrowScheme::Reserved(other),
        })
    }
}

/// `Hash(next ‖ pubkey)`: one link of a #18 hash chain. Built backwards,
/// `x_i = Hash(x_(i-1) ‖ pubkey_i)`, so revealing `x_(i-1)` with `pubkey_i`
/// proves the link and commits the borrow to that key.
pub fn hash_chain_link(next: &[u8; 32], pubkey: &[u8; 32]) -> [u8; 32] {
    let mut preimage = [0u8; 64];
    preimage[..32].copy_from_slice(next);
    preimage[32..].copy_from_slice(pubkey);
    kcc1::hash(&preimage)
}

/// The #18 borrow witness, `next[32] ‖ one_time_pubkey[32] ‖ sig[65]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyedLink {
    pub next: [u8; 32],
    pub pubkey: [u8; 32],
    pub signature: [u8; 65],
}

impl KeyedLink {
    pub const LEN: usize = 32 + 32 + 65;

    /// Exactly 129 bytes, or `None`.
    pub fn parse(witness: &[u8]) -> Option<KeyedLink> {
        if witness.len() != Self::LEN {
            return None;
        }
        Some(KeyedLink {
            next: witness[..32].try_into().ok()?,
            pubkey: witness[32..64].try_into().ok()?,
            signature: witness[64..].try_into().ok()?,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::LEN);
        out.extend_from_slice(&self.next);
        out.extend_from_slice(&self.pubkey);
        out.extend_from_slice(&self.signature);
        out
    }
}

/// Why a borrowed receive is invalid. Each rule has its own reason, so a
/// refusal says which rule failed and a test can hold a vector to the rule
/// it was written to break. The texts of the reasons that predate Last Call
/// are unchanged: stored verdicts carry them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorrowRefusal {
    /// owner, owner_scheme, borrow_scheme or extension_commitment changed.
    NotPreserved,
    /// The output carrying `next_states[0]` holds less KAS than the
    /// borrowed input did.
    KasReduced,
    /// #31: a state whose owner scheme KCC-2 does not assign cannot be
    /// borrowed.
    UnassignedOwnerScheme(u8),
    /// A threshold that decodes below zero under a program that requires
    /// non-negative (negative zero is 0 and is not refused).
    NegativeThreshold,
    Disabled,
    ThresholdWitness,
    SchnorrWitness,
    /// [`HashChainRule::Preimage`]: the witness is not 32 bytes.
    PreimageWitness,
    /// [`HashChainRule::Preimage`]: `Hash(preimage) != guard`.
    PreimageMismatch,
    /// [`HashChainRule::Keyed`]: the witness is not 129 bytes.
    KeyedWitness,
    /// [`HashChainRule::Keyed`]: `Hash(next ‖ pubkey) != guard`.
    LinkMismatch,
    /// An unassigned borrow scheme byte (§3: reserved, MUST be invalid).
    Reserved(u8),
    /// The increase does not strictly exceed the effective threshold, a
    /// decrease included.
    NotIncreased,
    /// The successor's guard is not the leader's guard (threshold, schnorr)
    /// or the revealed next head (hash chain).
    GuardNotExpected,
    /// `next_states[0]` was not proven to be the cell behind family output
    /// zero, so nothing about it can be judged.
    SuccessorNotProven,
    NoSuccessor,
}

impl fmt::Display for BorrowRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BorrowRefusal::NotPreserved => f.write_str(
                "the borrowed successor must preserve owner, owner_scheme, borrow_scheme and extension_commitment",
            ),
            BorrowRefusal::KasReduced => f.write_str("the borrowed cell's KAS value cannot be reduced"),
            BorrowRefusal::UnassignedOwnerScheme(byte) => {
                // KCC-2 splits the bytes it does not assign: reserved ones
                // MUST be rejected, custom ones are defined elsewhere; name
                // which, as the owner-scheme refusal in core does
                let class = if kcc2::is_custom(*byte) { "custom (defined outside KCC-2)" } else { "reserved" };
                write!(
                    f,
                    "a cell under {class} owner scheme 0x{byte:02x} cannot be borrowed; KCC-2 assigns 0x00-0x04"
                )
            }
            BorrowRefusal::NegativeThreshold => f.write_str("the threshold guard is not a non-negative int payload"),
            BorrowRefusal::Disabled => f.write_str("borrowing is disabled for this cell"),
            BorrowRefusal::ThresholdWitness => f.write_str("the amount-threshold borrow witness must be empty"),
            BorrowRefusal::SchnorrWitness => f.write_str("the schnorr borrow witness must be a 65-byte signature"),
            BorrowRefusal::PreimageWitness => f.write_str("the hash-chain borrow witness must be a 32-byte preimage"),
            BorrowRefusal::PreimageMismatch => f.write_str("the preimage does not hash to the current guard"),
            BorrowRefusal::KeyedWitness => f.write_str(
                "the hash-chain borrow witness must be 129 bytes: next guard, one-time key, signature",
            ),
            BorrowRefusal::LinkMismatch => {
                f.write_str("the revealed next guard and one-time key do not hash to the current guard")
            }
            BorrowRefusal::Reserved(_) => f.write_str("a reserved borrow scheme byte; the pinned text rejects it"),
            BorrowRefusal::NotIncreased => f.write_str("the token increase must strictly exceed the threshold"),
            BorrowRefusal::GuardNotExpected => {
                f.write_str("the successor does not carry the expected next borrow guard")
            }
            BorrowRefusal::SuccessorNotProven => {
                f.write_str("the borrowed successor is not proven against the family outputs")
            }
            BorrowRefusal::NoSuccessor => {
                f.write_str("a borrowed receive needs a successor at family position zero")
            }
        }
    }
}

/// What the bytes allow saying about a borrowed receive, in two tiers:
///
/// - `Proven`: kascov reproduced EVERY condition the scheme imposes, from
///   bytes alone (amount-threshold; the Draft's preimage hash chain).
/// - `Structural`: kascov reproduced every condition except a signature that
///   consensus checks and this reader does not re-run (schnorr-signature;
///   the #18 keyed hash chain, whose one-time key must sign).
///
/// Consensus accepted every transaction this judges, so `Structural` is not
/// doubt about the chain; it is a statement of what kascov itself checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BorrowVerdict {
    Proven(BorrowScheme),
    Structural(BorrowScheme),
    Invalid(BorrowRefusal),
}

impl BorrowVerdict {
    /// The tier as event rows carry it: "proven" or "structural"; `None`
    /// when invalid.
    pub fn tier(&self) -> Option<&'static str> {
        match self {
            BorrowVerdict::Proven(_) => Some("proven"),
            BorrowVerdict::Structural(_) => Some("structural"),
            BorrowVerdict::Invalid(_) => None,
        }
    }
}

/// The §5 borrowed-receive algorithm under the program's `rules`:
/// preservation, the KAS floor (the family output carrying `next_states[0]`
/// may not hold less KAS than the borrowed input did), the leader's owner
/// scheme being assigned (#31), the per-scheme witness rule,
/// `amount_increase > max(threshold, 0)` (the threshold is 0 unless the
/// threshold scheme raises it), and the expected next guard. A reserved scheme byte is
/// `reject` under the algorithm's final `else`, so it verdicts Invalid, not
/// unknown.
pub fn verify_borrow(
    rules: &Rules,
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
        return BorrowVerdict::Invalid(BorrowRefusal::NotPreserved);
    }
    if output0_kas < leader_kas {
        return BorrowVerdict::Invalid(BorrowRefusal::KasReduced);
    }
    if !kcc2::is_assigned(leader.owner_scheme) {
        return BorrowVerdict::Invalid(BorrowRefusal::UnassignedOwnerScheme(leader.owner_scheme));
    }
    let scheme = match BorrowScheme::of(leader, rules) {
        Ok(scheme) => scheme,
        Err(refusal) => return BorrowVerdict::Invalid(refusal),
    };
    let (threshold, next_guard, structural) = match scheme {
        BorrowScheme::Disabled => return BorrowVerdict::Invalid(BorrowRefusal::Disabled),
        BorrowScheme::AmountThreshold(threshold) => {
            if !borrow_witness.is_empty() {
                return BorrowVerdict::Invalid(BorrowRefusal::ThresholdWitness);
            }
            // the whole 32-byte guard is preserved, not just the int it starts with
            (threshold.max(0), leader.borrow_guard, false)
        }
        BorrowScheme::SchnorrSignature(_) => {
            if borrow_witness.len() != 65 {
                return BorrowVerdict::Invalid(BorrowRefusal::SchnorrWitness);
            }
            (0, leader.borrow_guard, true)
        }
        BorrowScheme::HashChain(guard) => match rules.hash_chain {
            HashChainRule::Keyed => {
                let Some(link) = KeyedLink::parse(borrow_witness) else {
                    return BorrowVerdict::Invalid(BorrowRefusal::KeyedWitness);
                };
                if hash_chain_link(&link.next, &link.pubkey) != guard {
                    return BorrowVerdict::Invalid(BorrowRefusal::LinkMismatch);
                }
                (0, link.next, true)
            }
            HashChainRule::Preimage => {
                let Ok(preimage) = <[u8; 32]>::try_from(borrow_witness) else {
                    return BorrowVerdict::Invalid(BorrowRefusal::PreimageWitness);
                };
                if kcc1::hash(&preimage) != guard {
                    return BorrowVerdict::Invalid(BorrowRefusal::PreimageMismatch);
                }
                (0, preimage, false)
            }
        },
        BorrowScheme::Reserved(byte) => return BorrowVerdict::Invalid(BorrowRefusal::Reserved(byte)),
    };
    let difference = i128::from(next0.amount) - i128::from(leader.amount);
    if difference <= i128::from(threshold) {
        return BorrowVerdict::Invalid(BorrowRefusal::NotIncreased);
    }
    if next0.borrow_guard != next_guard {
        return BorrowVerdict::Invalid(BorrowRefusal::GuardNotExpected);
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
    /// (K1 §3.8.1) and must carry the `transfer` invocation.
    pub input_index: u32,
    pub state: State,
    /// The spent UTXO's KAS value.
    pub kas: u64,
}

/// One continuation output of the family, in family output order. A KCC-1
/// instance lives under scriptPublicKey VERSION 0 (K1 §3.6); the version
/// travels with the script bytes so the proof cannot be satisfied by
/// identical bytes under another version.
#[derive(Clone, Debug)]
pub struct FamilyOutput {
    pub spk_version: u16,
    pub spk: Vec<u8>,
    pub kas: u64,
}

/// The bounds, scheme sets and rules a transition is judged against — pin
/// knowledge, defaulting to the spec's default configuration (§3) under the
/// Final rules.
#[derive(Clone, Debug)]
pub struct Bounds {
    pub max_token_inputs: usize,
    pub max_token_outputs: usize,
    pub owner_schemes: Vec<u8>,
    pub borrow_schemes: Vec<u8>,
    pub rules: Rules,
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
            rules: Rules::default(),
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
    /// breach and lands in `violations`; so is any mixing ([`Self::mixed_classes`]),
    /// which is how an imbalance between classes would have to arise.
    pub classes_preserved: bool,
    /// A single-successor family drew all inputs from one class equal to the
    /// successor's (§2's consolidation sentence). Vacuously true otherwise.
    pub consolidation_within_class: bool,
    /// More than one distinct class among inputs and successors. A breach on
    /// its own since #16 (Last Call §2): every input and continuation output
    /// of a standard transfer shares ONE extension commitment, so a rock
    /// cell and a paper cell may not even travel side by side, each class
    /// preserved. Before #16 that shape was legal and only consolidation
    /// across classes was refused; no official vector covers it, so kascov
    /// carries its own (tests/kcc20_vectors.rs).
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

    // §1: an amount is a non-negative quantity. The codec cannot carry a
    // negative successor, but a caller-built declaration can, and the total
    // check below would wave through a -600/+1600 split of 1000.
    if transfer.next_states.iter().any(|state| state.amount < 0) {
        violations.push("a successor amount is negative".into());
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
    if mixed_classes {
        violations.push(format!(
            "the inputs and successors carry {} different extension commitments; a standard transfer keeps one",
            classes.len()
        ));
    }

    // §2/§4: the commitment is preserved unchanged. A successor whose class
    // matches no input is a flat breach of its own, named for what it is.
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
            let auth = owner_auth(&leader.state, auth_bytes, bounds.rules.p2pkh_hash);
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
                BorrowVerdict::Invalid(BorrowRefusal::SuccessorNotProven)
            } else {
                match (transfer.next_states.first(), family_outputs.first()) {
                    (Some(next0), Some(output0)) => verify_borrow(
                        &bounds.rules,
                        &leader.state,
                        next0,
                        borrow_witness,
                        leader.kas,
                        output0.kas,
                    ),
                    _ => BorrowVerdict::Invalid(BorrowRefusal::NoSuccessor),
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
/// silverscript fork (d57e5df) emits a one-byte numeric selector and ends a
/// chain with `6a`; silverscript
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

/// Where a program's state window sits, and where the 112-byte KCC20State
/// record sits inside it. The canonical KCC-1 TemplateHash is taken around
/// the WHOLE window (§3.7.3); the record view, the bytes around the record
/// alone, is what a successor is rebuilt from, because whatever else the
/// window holds is context the family never changes.
#[derive(Clone, Debug, Deserialize)]
pub struct PinStateRange {
    pub start: usize,
    pub len: usize,
    /// The record's offset inside the window: zero where the window IS the
    /// record, 33 for an Argent actor that keeps its own template hash
    /// (`gen__kcc20_template`, one 32-byte push) ahead of it.
    #[serde(default)]
    pub record_offset: usize,
}

impl PinStateRange {
    pub fn record_start(&self) -> usize {
        self.start + self.record_offset
    }
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

/// The standard entrypoints' declared argument types (§2), as a pin writes
/// them. An entry is read as the standard transfer or delegator only when
/// both its role and these types say so; the entry's name decides nothing.
pub const STANDARD_TRANSFER_TYPES: &str = "KCC20State[],byte[]";
pub const STANDARD_DELEGATOR_TYPES: &str = "byte[]";

/// An issuer's mint or create: one KCC20State for the recipient, then the
/// KCC20 template halves the issuer rebuilds the recipient program from.
pub const ISSUE_TYPES: &str = "KCC20State,byte[],byte[]";

#[derive(Clone, Debug, Deserialize)]
pub struct PinEntrypoint {
    pub name: String,
    pub types: String,
    /// Tag hex or selector decimal, matching the dispatch kind.
    pub select: String,
    /// What the reader does with a reveal of this entry:
    /// - "transfer" / "delegator": the standard entrypoints, read by their
    ///   declared types ([`STANDARD_TRANSFER_TYPES`],
    ///   [`STANDARD_DELEGATOR_TYPES`]); any other types are not the standard
    ///   layout and are not read as it.
    /// - "issue": an issuer actor's mint or create ([`ISSUE_TYPES`]).
    /// - "program-specific": a venue's own arm, read only when its name is
    ///   one kascov implements a reader for (the `zealous-*` arms of
    ///   [`parse_tag_reveal`]), never because it happens to be called
    ///   "transfer".
    /// - "extension-update": declared, not read yet.
    pub role: String,
    #[serde(default)]
    pub opening_arg: Option<usize>,
}

/// What a pin records about its build's published program artifact.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PinArtifact {
    /// The publisher released no program artifact (KaspaRocket's ff3ae732:
    /// its repository holds the .sil source and a README that states the
    /// limits in prose). The pin is then kascov's own description of the
    /// build, read from bytes, and kascov cannot check what KCC-20 asks the
    /// artifact to say, so [`Pin::conformant`] gives no verdict either way.
    /// It is not a departure: [`Pin::nonconformities`] lists nothing for it,
    /// and the build keeps its reviewed standing.
    Unpublished,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PinReview {
    /// "reviewed" | "unreviewed" — only a reviewed pin can carry a token to
    /// verified, and only a reviewed pin's labels may name a class.
    pub status: String,
    /// Fixture basenames under fixtures/kcc0020/ whose reveals must
    /// recompute the pin's template hash.
    pub fixtures: Vec<String>,
    /// True when the chain offers no two cells of this build that differ in
    /// class, so the fixture pair cannot show the extension commitment
    /// varying: either the chain carries one launch of the build (the
    /// 2,641 B Zealous generation) or every cell of it on chain carries the
    /// same class (KaspaRocket's ff3ae732: about a hundred launches, every
    /// cell all-zero). The fixture test then holds the pair to the other
    /// state fields, and the pin's note must say which of the two holds and
    /// how the window's end is proven instead (from the program's own length
    /// and suffix-offset constants).
    #[serde(default)]
    pub fixtures_share_class: bool,
    pub note: String,
}

/// What an issuer actor does for the token family.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum IssuerRole {
    /// Issues a positive amount out of an allowance it carries in its own
    /// state (the reference app's `PublicMint`).
    Mint,
    /// Creates a zero-amount receiving cell and keeps its own state (the
    /// reference app's `TokenSeed`).
    Seed,
}

/// Which window fields of an issuer carry meaning kascov reads, as indexes
/// into [`PinIssuer::state_types`].
#[derive(Clone, Debug, Deserialize)]
pub struct IssuerFields {
    /// The KCC20 template the issuer builds recipient cells of; it must be
    /// the token pin's own canonical hash, or the issuer issues something
    /// else.
    pub kcc20_template: usize,
    pub extension_commitment: usize,
    /// The allowance still issuable (mint role).
    #[serde(default)]
    pub remaining: Option<usize>,
    /// The most one issue may hand out (mint role).
    #[serde(default)]
    pub per_issue_limit: Option<usize>,
    /// The issuer's own template hash, which the reference app keeps in its
    /// window for its reclaim entries to name a partner by. A genesis rebuild
    /// ([`PinIssuer::seed_program`]) sets it to the issuer's real hash, so a
    /// seed that planted any other value is never rebuilt.
    #[serde(default)]
    pub self_template: Option<usize>,
}

/// An issuer actor inside the token's covenant family: a separate program,
/// with its own template, that creates KCC20 cells. KCC-20 itself defines no
/// issuance; the reference app (argent-lang/kcc20-reference) mints through
/// `PublicMint` and seeds receiving cells through `TokenSeed`, both sharing
/// the token's covenant id. Without knowing them, every mint would read as a
/// transfer whose outputs exceed its inputs.
#[derive(Clone, Debug, Deserialize)]
pub struct PinIssuer {
    /// The actor's name in its artifact.
    pub name: String,
    pub role: IssuerRole,
    /// The issuer's canonical KCC-1 template hash, around its whole window.
    pub template_hash: String,
    pub state: PinStateRange,
    /// The window's KCC-1 field types, in order, comma-separated.
    pub state_types: String,
    pub fields: IssuerFields,
    pub entrypoints: Vec<PinEntrypoint>,
    /// Fixture basenames that must recompute `template_hash`.
    pub fixtures: Vec<String>,
}

/// kascov's committed description of one KCC-20 program — the artifact as
/// this reader holds it, keyed by the KCC-1 template hash. The hash is never
/// trusted from the file: every fixture the review names must recompute it
/// (`every_committed_pin_parses_and_names_its_spec`).
#[derive(Clone, Debug, Deserialize)]
pub struct Pin {
    /// The kccs commit whose text the program was compiled against:
    /// [`SPEC_COMMIT`] or [`DRAFT_SPEC_COMMIT`].
    pub spec_commit: String,
    /// The canonical KCC-1 TemplateHash, around the whole state window.
    pub template_hash: String,
    /// The record view's hash, where the window holds more than the record:
    /// the TemplateHash around the 112-byte record alone (an Argent
    /// `actor_type_handle`). Registered as an alias: [`pin_for`] resolves it
    /// to this pin, and a program is only claimed when both hashes
    /// reproduce, so the context bytes in the window are pinned too.
    #[serde(default)]
    pub view_template_hash: Option<String>,
    pub name: String,
    /// The display family a reveal of this program is stamped with, where
    /// the pin declares one: "Zealous · token" for ZealousSwap's cell, so
    /// the venue prefix rules ("Zealous · ") own it the way they own every
    /// other family kascov names from bytes. A pin without one stamps its
    /// [`Pin::standing`]. Display only: every spec-route decision keys on
    /// the pin match, never on this string, and the label must not collide
    /// with a legacy cell family or an observed skeleton name, or the legacy
    /// decoders would claim a spec program by name (guarded by
    /// `every_committed_pin_parses_and_names_its_spec`).
    #[serde(default)]
    pub family: Option<String>,
    pub dispatch: PinDispatch,
    pub state: PinStateRange,
    pub owner_schemes: Vec<u8>,
    pub max_token_inputs: usize,
    pub max_token_outputs: usize,
    pub borrow_schemes: Vec<u8>,
    /// The Draft-versus-Final rules the program checks.
    #[serde(default)]
    pub rules: Rules,
    pub extension: PinExtension,
    pub entrypoints: Vec<PinEntrypoint>,
    /// Issuer actors sharing the token's covenant family.
    #[serde(default)]
    pub issuers: Vec<PinIssuer>,
    /// Which never-spent outputs of a token's genesis the derivation may
    /// prove by rebuilding this build's reviewed programs; absent, none.
    #[serde(default)]
    pub genesis_rebuild: Option<GenesisRebuildDecl>,
    /// Whether the build's publisher released the program artifact KCC-20
    /// asks of every covenant (§3; §2 has it describe the maximum token
    /// input and output counts). Absent: the pin says nothing against it
    /// (the reference app publishes its artifact.json). See [`PinArtifact`].
    #[serde(default)]
    pub artifact: Option<PinArtifact>,
    /// What the program's own in-script templateHash builtin computes:
    /// "blake2b" for builds of the michaelsutton fork at d57e5df, "blake3"
    /// for silverscript v1.0.0 (3ed9733) and argentc builds, which store the
    /// KCC-1 §3.7.3 value on chain and check it with OpBlake3. Distinct from
    /// the BLAKE3 pin key either way.
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
            rules: self.rules,
        }
    }

    /// The canonical TemplateHash of `program` under this pin's window: the
    /// bytes before the window and the bytes after it. `None` when the
    /// window does not fit.
    pub fn canonical_hash(&self, program: &[u8]) -> Option<[u8; 32]> {
        let end = self.state.start.checked_add(self.state.len)?;
        (end <= program.len()).then(|| kcc1::template_hash(&program[..self.state.start], &program[end..]))
    }

    /// The record view of `program`: the template a successor is rebuilt
    /// from, cut around the 112-byte record. For a window that is only the
    /// record this is the canonical template.
    pub fn cut(&self, program: &[u8]) -> Option<Template> {
        if self.state.record_offset + STATE_LEN > self.state.len {
            return None;
        }
        Template::cut(program, self.state.record_start())
    }

    /// [`Pin::cut`] of a program of this very build: its canonical hash and,
    /// where the pin declares one, its record-view hash reproduce the pin's,
    /// as [`pinned_program_state`] requires of a reveal. `None` for any other
    /// program, however its record decodes, so a template taken from it holds
    /// this build's bytes outside the record and no other build's.
    pub fn own_cut(&self, program: &[u8]) -> Option<Template> {
        if self.canonical_hash(program).map(hex::encode).as_deref() != Some(self.template_hash.as_str()) {
            return None;
        }
        let template = self.cut(program)?;
        if self
            .view_template_hash
            .as_deref()
            .is_some_and(|want| hex::encode(template.hash()) != want)
        {
            return None;
        }
        Some(template)
    }

    /// The entry this pin declares for a standard role, by role and types.
    pub fn standard_entry(&self, role: &str) -> Option<&PinEntrypoint> {
        let types = match role {
            "transfer" => STANDARD_TRANSFER_TYPES,
            "delegator" => STANDARD_DELEGATOR_TYPES,
            _ => return None,
        };
        self.entrypoints.iter().find(|e| e.role == role && e.types == types)
    }

    /// Every way this pin's program departs from the Final text
    /// ([`SPEC_COMMIT`]), read off the pin's own declarations, one reason
    /// per rule. Empty when every declared rule is the spec's. A reduced
    /// owner-scheme subset is a configuration the spec allows (§3) and is
    /// not listed.
    pub fn nonconformities(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.spec_commit != SPEC_COMMIT {
            out.push(format!(
                "built to a pre-Final draft (read by kccs {}), not the Final text ({SPEC_COMMIT})",
                self.spec_commit
            ));
        }
        if self.dispatch.kind != DispatchKind::Kcc1Tag {
            out.push("dispatches on a numeric selector, not the KCC-1 dispatch tags".into());
        }
        for (role, tag) in [("transfer", transfer_tag()), ("delegator", delegator_tag())] {
            let tag = hex::encode(tag);
            if self.standard_entry(role).is_none_or(|e| e.select != tag) {
                out.push(format!("no standard {role} entrypoint dispatched by {tag}"));
            }
        }
        if self.rules.hash_chain != HashChainRule::Keyed {
            out.push("its hash-chain borrow is the Draft's preimage rule, not the keyed link of #18".into());
        }
        let p2pkh_owned = self
            .owner_schemes
            .iter()
            .any(|s| matches!(*s, OWNER_P2PKH_SCHNORR | OWNER_P2PKH_ECDSA));
        if p2pkh_owned && self.rules.p2pkh_hash != kcc2::P2pkhHash::Unkeyed {
            out.push("its p2pkh owners hash under the Draft's keyed P2PKHHash, not KCC-2's unkeyed one".into());
        }
        if !self.rules.single_class {
            out.push("it is held to the Draft's per-class balance, not #16's one class per transfer".into());
        }
        if self.borrow_schemes != [0x00, 0x01, 0x02, 0x03] {
            out.push("its borrow schemes are not the fixed mapping 0x00-0x03".into());
        }
        if self.owner_schemes.is_empty() || !self.owner_schemes.iter().all(|s| kcc2::is_assigned(*s)) {
            out.push("its owner schemes are not a subset of KCC-2's assigned schemes".into());
        }
        if self.in_script_template_hash_fn != "blake3" {
            out.push(format!(
                "its in-script template hash is {}, not KCC-1's BLAKE3",
                self.in_script_template_hash_fn
            ));
        }
        if self.max_token_inputs == 0 || self.max_token_outputs == 0 {
            out.push("its family bounds admit no transfer".into());
        }
        out
    }

    /// Whether the pin is reviewed (its declarations checked against the
    /// bytes) and no declared rule departs from the Final text: what the
    /// build's reviewed standing rests on.
    fn reviewed_without_departure(&self) -> bool {
        self.review.status == "reviewed" && self.nonconformities().is_empty()
    }

    /// The conformance verdict the API's `standard` object serves:
    /// `Some(false)` when any declared rule departs from the Final text,
    /// `Some(true)` when none does AND the pin is reviewed (the declarations
    /// were checked against the bytes) AND its program artifact is not
    /// recorded as unpublished, `None` otherwise: the pin is unreviewed, or
    /// the build's publisher released no artifact, so what KCC-20 asks the
    /// artifact to describe is not assessed (not assessed is not a
    /// departure). A verdict about one program, the BUILD: it never speaks
    /// for kascov, and never for a token of the build, whose own history is
    /// judged by the derivation (a token of a conformant build can still be
    /// unproven).
    pub fn conformant(&self) -> Option<bool> {
        if !self.nonconformities().is_empty() {
            Some(false)
        } else if self.reviewed_without_departure() && self.artifact != Some(PinArtifact::Unpublished) {
            Some(true)
        } else {
            None
        }
    }

    /// What this build is, measured against the spec, in the words kascov
    /// shows for it: [`DRAFT_BUILD_TEMPLATE`] for a program compiled against
    /// a Draft, [`REVIEWED_BUILD_TEMPLATE`] once reviewed with no declared
    /// rule departing (conformant, or with its artifact unpublished and the
    /// verdict open), [`LAYOUT_TEMPLATE`] otherwise. A venue family names the build; this
    /// says where it stands, and is never replaced by it. It speaks for the
    /// build only: what a token of it is shown as is decided per token, by
    /// the accounting layer that knows the token's own verdict.
    pub fn standing(&self) -> &'static str {
        if self.spec_commit != SPEC_COMMIT {
            DRAFT_BUILD_TEMPLATE
        } else if self.reviewed_without_departure() {
            REVIEWED_BUILD_TEMPLATE
        } else {
            LAYOUT_TEMPLATE
        }
    }

    /// The template name a reveal of this program is filed under: the pin's
    /// declared family, or its [`Pin::standing`] when it declares none.
    /// Borrowed from the pin, so on a committed pin (`pins()`) it is
    /// `'static` and fits the reveal-time stamp directly.
    pub fn family_label(&self) -> &str {
        self.family.as_deref().unwrap_or(self.standing())
    }
}

/// Every template name a pinned KCC-20 reveal can be stamped with: the
/// three standings, the retired generic label (rows stamped before the
/// Last Call labels, until the re-stamp pass rewrites them), then each
/// committed pin's declared family, deduplicated in pin order.
pub fn spec_template_names() -> Vec<&'static str> {
    let mut names = STANDING_LABELS.to_vec();
    // a pin without a family is stamped with a standing, listed already
    for family in pins().iter().filter_map(|pin| pin.family.as_deref()) {
        if !names.contains(&family) {
            names.push(family);
        }
    }
    names
}

/// The labels a pin's standing can stamp, and the one they replaced. The
/// token-level names ([`SPEC_TOKEN_TEMPLATE`], [`UNPROVEN_TOKEN_TEMPLATE`])
/// are not among them: no reveal is ever stamped with a token's standing.
const STANDING_LABELS: [&str; 4] = [REVIEWED_BUILD_TEMPLATE, LAYOUT_TEMPLATE, DRAFT_BUILD_TEMPLATE, RETIRED_SPEC_TEMPLATE];

/// Whether a stored template name means "a pinned KCC-20 build": any label
/// [`spec_template_names`] lists. Callers that need to know a row is a spec
/// cell test this, never one literal, so a pin gaining a family or a
/// standing cannot silently drop its rows out of a name-keyed gate.
pub fn is_spec_template(name: &str) -> bool {
    // the store asks this for every row it writes, so no pin's standing is
    // computed here: a familyless pin's label is one of the standings
    STANDING_LABELS.contains(&name) || pins().iter().any(|pin| pin.family.as_deref() == Some(name))
}

/// Pin sources are embedded at compile time — layouts are data, but the data
/// ships inside the binary and is self-checked by tests, never read from
/// disk at runtime (the same stance as `CELL_FAMILIES`).
const PIN_SOURCES: &[(&str, &str)] = &[
    ("kcc0020-vector", include_str!("../pins/kcc0020/4abcc188.json")),
    ("zealous-token-cell", include_str!("../pins/kcc0020/653012f1.json")),
    ("zealous-token-cell-2641", include_str!("../pins/kcc0020/2f2671fc.json")),
    ("zealous-token-cell-3037", include_str!("../pins/kcc0020/58c24260.json")),
    ("zealous-token-cell-3223", include_str!("../pins/kcc0020/50ff50d8.json")),
    ("kcc20-reference", include_str!("../pins/kcc0020/9703112e.json")),
    ("kasparocket-kcc20", include_str!("../pins/kcc0020/ff3ae732.json")),
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

/// A short fingerprint over every committed pin's identity, review status
/// and every declaration that changes what a token derives as. The
/// accounting layer composes it into its derivation stamp: reviewing a pin,
/// adding one, or changing any such declaration re-derives every stored
/// token mechanically, instead of by someone remembering to bump a
/// constant. A declaration left out of here would ship without a re-derive,
/// so a test changes each one and watches this move.
pub fn pins_fingerprint() -> String {
    fingerprint_of(pins())
}

/// A short fingerprint over the label each committed pin stamps on its
/// reveals ([`Pin::family_label`]). A pin without a family is labelled by its
/// standing, which moves with its review and with [`SPEC_COMMIT`], and a
/// stored label changes only when the store's restamp walk rewrites it; the
/// walk's gate carries this, so whatever moves a label also reruns the walk.
pub fn labels_fingerprint() -> String {
    labels_fingerprint_of(pins())
}

fn labels_fingerprint_of(pins: &[Pin]) -> String {
    let mut hasher = blake3::Hasher::new();
    for pin in pins {
        for field in [pin.template_hash.as_str(), pin.family_label()] {
            hasher.update(&(field.len() as u64).to_le_bytes());
            hasher.update(field.as_bytes());
        }
    }
    hex::encode(&hasher.finalize().as_bytes()[..4])
}

fn fingerprint_of(pins: &[Pin]) -> String {
    let mut hasher = blake3::Hasher::new();
    // every field length-framed, so adjacent fields cannot trade bytes
    let mut put = |bytes: &[u8]| {
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    };
    let window = |put: &mut dyn FnMut(&[u8]), state: &PinStateRange| {
        for n in [state.start, state.len, state.record_offset] {
            put(&(n as u64).to_le_bytes());
        }
    };
    let entries = |put: &mut dyn FnMut(&[u8]), entries: &[PinEntrypoint]| {
        put(&(entries.len() as u64).to_le_bytes());
        for e in entries {
            put(e.name.as_bytes());
            put(e.types.as_bytes());
            put(e.select.as_bytes());
            put(e.role.as_bytes());
        }
    };
    for pin in pins {
        put(pin.template_hash.as_bytes());
        put(pin.review.status.as_bytes());
        put(&[pin.max_token_inputs as u8, pin.max_token_outputs as u8]);
        put(&pin.owner_schemes);
        put(&pin.borrow_schemes);
        put(pin.spec_commit.as_bytes());
        put(pin.view_template_hash.as_deref().unwrap_or("").as_bytes());
        window(&mut put, &pin.state);
        put(&[
            match pin.dispatch.kind {
                DispatchKind::Kcc1Tag => 0,
                DispatchKind::SilvercSelector => 1,
            },
            match pin.rules.hash_chain {
                HashChainRule::Keyed => 0,
                HashChainRule::Preimage => 1,
            },
            match pin.rules.p2pkh_hash {
                kcc2::P2pkhHash::Unkeyed => 0,
                kcc2::P2pkhHash::KeyedDraft => 1,
            },
            u8::from(pin.rules.threshold_non_negative),
            u8::from(pin.rules.single_class),
        ]);
        entries(&mut put, &pin.entrypoints);
        put(&(pin.issuers.len() as u64).to_le_bytes());
        for issuer in &pin.issuers {
            put(issuer.name.as_bytes());
            put(&[match issuer.role {
                IssuerRole::Mint => 0,
                IssuerRole::Seed => 1,
            }]);
            put(issuer.template_hash.as_bytes());
            window(&mut put, &issuer.state);
            put(issuer.state_types.as_bytes());
            let f = &issuer.fields;
            for index in [
                Some(f.kcc20_template),
                Some(f.extension_commitment),
                f.remaining,
                f.per_issue_limit,
                f.self_template,
            ] {
                // u64::MAX stands for an undeclared field: no real index
                put(&index.map_or(u64::MAX, |i| i as u64).to_le_bytes());
            }
            entries(&mut put, &issuer.entrypoints);
        }
        // the genesis rebuild a pin declares: which seeds, and whether
        // declared cells, each length-framed like every field above
        match &pin.genesis_rebuild {
            None => put(&[0]),
            Some(decl) => {
                put(&[1, u8::from(decl.declared_cells)]);
                put(&(decl.seeds.len() as u64).to_le_bytes());
                for seed in &decl.seeds {
                    put(seed.as_bytes());
                }
            }
        }
    }
    hex::encode(&hasher.finalize().as_bytes()[..4])
}

/// The pin describing a template, by its canonical (BLAKE3) hash or by the
/// record-view hash it registers as an alias.
pub fn pin_for(template_hash: &[u8; 32]) -> Option<&'static Pin> {
    let hex = hex::encode(template_hash);
    pins()
        .iter()
        .find(|pin| pin.template_hash == hex || pin.view_template_hash.as_deref() == Some(hex.as_str()))
}

/// Recognise a revealed program as a pinned KCC-20 build: the canonical
/// hash around the pin's declared window must reproduce, and the record
/// view's too where the pin declares one, then the embedded record decodes.
/// Pin-gated on purpose — an unpinned program is never claimed, however
/// spec-shaped its bytes look, so arbitrary scripts cannot become token
/// candidates by accident.
pub fn pinned_program_state(program: &[u8]) -> Option<(&'static Pin, State)> {
    for pin in pins() {
        if pin.state.record_offset + STATE_LEN > pin.state.len {
            continue;
        }
        // The record decode first: it refuses almost every program for the
        // price of a few header compares, where the hashes cost a full pass
        // over the program per pin. This runs on every reveal the follower
        // stamps and on every row the boot restamp walks.
        let at = pin.state.record_start();
        let Some(state) = program.get(at..at + STATE_LEN).and_then(State::decode) else {
            continue;
        };
        if pin.canonical_hash(program).map(hex::encode).as_deref() != Some(pin.template_hash.as_str()) {
            continue;
        }
        if pin.view_template_hash.as_deref().is_some_and(|want| {
            hex::encode(kcc1::template_hash(&program[..at], &program[at + STATE_LEN..])) != want
        }) {
            continue;
        }
        return Some((pin, state));
    }
    None
}

// ── issuers ──────────────────────────────────────────────────────────

/// The KCC20State record as a KCC-1 type, for the codec's argument reader.
fn state_record_type() -> kcc1::Type {
    let bytes32 = || kcc1::Type::Array(Box::new(kcc1::Type::Byte), Some(32));
    kcc1::Type::Record(kcc1::Record {
        name: "KCC20State".into(),
        fields: vec![
            ("amount".into(), kcc1::Type::Int),
            ("owner".into(), bytes32()),
            ("owner_scheme".into(), kcc1::Type::Byte),
            ("borrow_scheme".into(), kcc1::Type::Byte),
            ("borrow_guard".into(), bytes32()),
            ("extension_commitment".into(), bytes32()),
        ],
    })
}

fn state_from_record(value: &kcc1::Value) -> Option<State> {
    let kcc1::Value::Record(fields) = value else { return None };
    let [kcc1::Value::Int(amount), kcc1::Value::Bytes(owner), kcc1::Value::Bytes(owner_scheme), kcc1::Value::Bytes(borrow_scheme), kcc1::Value::Bytes(borrow_guard), kcc1::Value::Bytes(extension_commitment)] =
        fields.as_slice()
    else {
        return None;
    };
    Some(State {
        amount: *amount,
        owner: owner.as_slice().try_into().ok()?,
        owner_scheme: *owner_scheme.first()?,
        borrow_scheme: *borrow_scheme.first()?,
        borrow_guard: borrow_guard.as_slice().try_into().ok()?,
        extension_commitment: extension_commitment.as_slice().try_into().ok()?,
    })
}

/// An issuer's window, decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssuerState {
    pub kcc20_template: [u8; 32],
    pub extension_commitment: [u8; 32],
    /// The allowance still issuable, where the pin declares it readable.
    pub remaining: Option<i64>,
    pub per_issue_limit: Option<i64>,
    /// Every window field, so a successor re-encodes with one field changed.
    values: Vec<kcc1::Value>,
}

impl PinIssuer {
    fn types(&self) -> Option<Vec<kcc1::Type>> {
        self.state_types
            .split(',')
            .map(|t| kcc1::Type::parse(t, &[]).ok())
            .collect()
    }

    /// Read `program` as this issuer: its canonical hash around the declared
    /// window must reproduce, and the window must decode under the declared
    /// types with each named field of the right shape.
    pub fn read_state(&self, program: &[u8]) -> Option<IssuerState> {
        let end = self.state.start.checked_add(self.state.len)?;
        let window = program.get(self.state.start..end)?;
        // the window decode first: it refuses almost every program without
        // the full-program hash (see pinned_program_state)
        let values = kcc1::decode_state_values(&self.types()?, window).ok()?;
        if hex::encode(kcc1::template_hash(&program[..self.state.start], &program[end..])) != self.template_hash {
            return None;
        }
        let bytes32 = |i: usize| match values.get(i)? {
            kcc1::Value::Bytes(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
            _ => None,
        };
        let int = |i: Option<usize>| -> Option<Option<i64>> {
            match i {
                None => Some(None),
                Some(i) => match values.get(i)? {
                    kcc1::Value::Int(v) => Some(Some(*v)),
                    _ => None,
                },
            }
        };
        Some(IssuerState {
            kcc20_template: bytes32(self.fields.kcc20_template)?,
            extension_commitment: bytes32(self.fields.extension_commitment)?,
            remaining: int(self.fields.remaining)?,
            per_issue_limit: int(self.fields.per_issue_limit)?,
            values,
        })
    }

    /// The program this issuer continues as after issuing `amount`: a mint
    /// with its allowance reduced by it, a seed unchanged.
    fn successor_program(&self, program: &[u8], state: &IssuerState, amount: i64) -> Option<Vec<u8>> {
        let mut values = state.values.clone();
        if self.role == IssuerRole::Mint {
            let at = self.fields.remaining?;
            values[at] = kcc1::Value::Int(state.remaining?.checked_sub(amount)?);
        }
        let fields: Vec<(kcc1::Type, kcc1::Value)> = self.types()?.into_iter().zip(values).collect();
        let window = kcc1::encode_state_values(&fields).ok()?;
        let end = self.state.start + self.state.len;
        Some([&program[..self.state.start], window.as_slice(), &program[end..]].concat())
    }

    /// The entry this issuer issues through: the one the pin declares with
    /// the issue role and [`ISSUE_TYPES`] (`mint` or `create` in the
    /// reference app).
    pub fn issue_entry(&self) -> Option<&PinEntrypoint> {
        self.entrypoints.iter().find(|e| e.role == "issue" && e.types == ISSUE_TYPES)
    }

    /// The window's one `pubkey` field, found by its declared type rather
    /// than by a pin index, so no pin declaration (and no pin fingerprint)
    /// changes for it. Per the reference source it is the key the split and
    /// reclaim entries use; this reads the field and nothing more, it does not
    /// say what the bytecode enforces with it. `None` when the issuer
    /// declares no such field, or more than one.
    pub fn pubkey(&self, state: &IssuerState) -> Option<[u8; 32]> {
        let types = self.types()?;
        let mut at = types
            .iter()
            .enumerate()
            .filter(|(_, t)| matches!(t, kcc1::Type::PubKey))
            .map(|(i, _)| i);
        let (Some(i), None) = (at.next(), at.next()) else {
            return None;
        };
        match state.values.get(i)? {
            kcc1::Value::Bytes(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
            _ => None,
        }
    }
}

/// A revealed program read as a pinned token's issuer.
pub fn pinned_issuer_state(program: &[u8]) -> Option<(&'static Pin, &'static PinIssuer, IssuerState)> {
    pins().iter().find_map(|pin| {
        pin.issuers
            .iter()
            .find_map(|issuer| Some((pin, issuer, issuer.read_state(program)?)))
    })
}

// ── genesis rebuilds ─────────────────────────────────────────────────

/// What a pin lets the token derivation prove of a genesis output no spend
/// has revealed (pass 2c, "declared genesis"): such an output is proven when
/// one of this build's reviewed programs, rebuilt with a state the genesis
/// transaction itself determines, commits to it byte for byte, and exactly
/// one candidate does. The derivation never fills a state in: a candidate
/// that does not hash to the output proves nothing.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct GenesisRebuildDecl {
    /// Issuers, by name, whose program a genesis output may be rebuilt as:
    /// each a seed ([`IssuerRole::Seed`]) that declares `self_template` and
    /// whose reviewed bytes this crate carries ([`PinIssuer::reviewed_halves`]).
    #[serde(default)]
    pub seeds: Vec<String>,
    /// Whether a genesis output may be rebuilt as this build's token cell,
    /// with an owner tuple and amount the genesis transaction declares.
    #[serde(default)]
    pub declared_cells: bool,
}

/// Reviewed issuer programs a genesis rebuild takes its bytes outside the
/// window from, by issuer template hash. Each is the first fixture its pin's
/// issuer names, a chain reveal (testnet-10 ec214f90..:1, revealed by
/// 05060a38.. input 0); only its halves are used, and they must recompute
/// the template hash, or the issuer is not rebuilt at all.
const ISSUER_PROGRAMS: &[(&str, &[u8])] = &[(
    "f79a3fccf6f87d7076662410a17e54d1531889d9b60f630f9fbb8465a384e62d",
    include_bytes!("../fixtures/kcc0020/reference_token_seed_a.bin"),
)];

/// The most argument pushes a spend may carry before its program and still
/// be read as declaring cells of a build ([`declares_build`]).
pub const DECLARER_ARG_CAP: usize = 64;

impl PinIssuer {
    /// The reviewed program halves around this issuer's window: the bytes
    /// before it and after it, from the program this crate carries for the
    /// issuer's template hash. `None` unless those halves recompute
    /// `template_hash` under this issuer's own window.
    pub fn reviewed_halves(&self) -> Option<(&'static [u8], &'static [u8])> {
        let (_, program) = ISSUER_PROGRAMS.iter().find(|(hash, _)| *hash == self.template_hash)?;
        let end = self.state.start.checked_add(self.state.len)?;
        let (prefix, suffix) = (program.get(..self.state.start)?, program.get(end..)?);
        (hex::encode(kcc1::template_hash(prefix, suffix)) == self.template_hash).then_some((prefix, suffix))
    }

    /// The program of this seed with `owner` in its one `pubkey` field and
    /// `class` as its extension commitment, its `kcc20_template` field set to
    /// `pin`'s canonical hash and its `self_template` field to its own hash,
    /// around the reviewed halves. `None` for an issuer that is no seed, has
    /// no reviewed halves, does not declare `self_template`, or whose window
    /// holds any field besides these four.
    pub fn seed_program(&self, pin: &Pin, owner: &[u8; 32], class: &[u8; 32]) -> Option<Vec<u8>> {
        if self.role != IssuerRole::Seed {
            return None;
        }
        let (prefix, suffix) = self.reviewed_halves()?;
        let types = self.types()?;
        let mut keys = types.iter().enumerate().filter(|(_, t)| matches!(t, kcc1::Type::PubKey)).map(|(i, _)| i);
        let (Some(key), None) = (keys.next(), keys.next()) else {
            return None;
        };
        let f = &self.fields;
        let assigned = [
            (f.kcc20_template, hex::decode(&pin.template_hash).ok()?),
            (f.self_template?, hex::decode(&self.template_hash).ok()?),
            (key, owner.to_vec()),
            (f.extension_commitment, class.to_vec()),
        ];
        let mut values: Vec<Option<kcc1::Value>> = vec![None; types.len()];
        for (at, bytes) in assigned {
            let slot = values.get_mut(at)?;
            if slot.is_some() {
                // two declarations name one field
                return None;
            }
            *slot = Some(kcc1::Value::Bytes(bytes));
        }
        let fields: Vec<(kcc1::Type, kcc1::Value)> =
            types.into_iter().zip(values).map(|(t, v)| Some((t, v?))).collect::<Option<_>>()?;
        let window = kcc1::encode_state_values(&fields).ok()?;
        (window.len() == self.state.len).then(|| [prefix, window.as_slice(), suffix].concat())
    }
}

/// A spend of another covenant read as declaring cells of `pin`'s build:
/// its signature script splits into minimal pushes (KCC-1 §3.4.2, the policy
/// of every reveal parser here), the last one its program, at most
/// [`DECLARER_ARG_CAP`] arguments before it, and two adjacent arguments, in
/// order, are byte-equal to `template`'s halves around the pin's window: the
/// halves an issue passes, which hash to the pin's canonical hash. Returns
/// the arguments; `None` otherwise. What the program does with them is not
/// read: the arguments only say that the spend names this build, so that an
/// unrelated program is never searched for numbers.
pub fn declares_build(sig_script: &[u8], pin: &Pin, template: &Template) -> Option<Vec<Vec<u8>>> {
    let prefix = template.prefix.get(..pin.state.start)?;
    let suffix = template.suffix.as_slice();
    if hex::encode(kcc1::template_hash(prefix, suffix)) != pin.template_hash {
        return None;
    }
    let mut args = kcc1::read_pushes_minimal(sig_script)?;
    args.pop()?;
    if args.len() > DECLARER_ARG_CAP {
        return None;
    }
    args.windows(2).any(|pair| pair[0] == prefix && pair[1] == suffix).then_some(args)
}

/// A pushed payload read as a token amount: 1 to 8 bytes, little-endian,
/// the last byte's sign bit clear (KCC-1 §3.4.3 reads a set one as a
/// negative number, and no amount is negative).
pub fn push_amount(payload: &[u8]) -> Option<i64> {
    let (&last, _) = payload.split_last()?;
    if payload.len() > 8 || last & 0x80 != 0 {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes[..payload.len()].copy_from_slice(payload);
    i64::try_from(u64::from_le_bytes(bytes)).ok()
}

/// Every number of 1 to 8 bytes `script` pushes ([`push_amount`]), walked
/// with the disassembler (non-push opcodes skipped), distinct and ascending.
/// A truncated script contributes nothing. `None` past `cap` distinct
/// numbers: the caller then searches none of them.
pub fn pushed_amounts(script: &[u8], cap: usize) -> Option<Vec<i64>> {
    let (instructions, truncated) = crate::disasm::disassemble(script);
    let mut out = std::collections::BTreeSet::new();
    if truncated {
        return Some(Vec::new());
    }
    for data in instructions.iter().filter_map(|i| i.data.as_deref()) {
        if let Some(amount) = push_amount(data) {
            out.insert(amount);
            if out.len() > cap {
                return None;
            }
        }
    }
    Some(out.into_iter().collect())
}

/// What an issuer's mint or create declares: the recipient's state, and the
/// KCC20 template halves the issuer rebuilds the recipient program from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub recipient: State,
    pub kcc20_prefix: Vec<u8>,
    pub kcc20_suffix: Vec<u8>,
}

/// Parse an issuer reveal: the program last, the issue entry's tag before
/// it, its three arguments (the record's six leaves, prefix, suffix) before
/// that, every push canonical (KCC-1 §3.4.2). `None` for any other entry.
pub fn parse_issue_reveal(sig_script: &[u8], issuer: &PinIssuer) -> Option<Issue> {
    let entry = issuer.issue_entry()?;
    let mut payloads = split_pushes(sig_script)?;
    let _program = payloads.pop()?;
    if payloads.pop()? != hex::decode(&entry.select).ok()? {
        return None;
    }
    let bytes = kcc1::Type::Array(Box::new(kcc1::Type::Byte), None);
    let values = kcc1::decode_arg_pushes(&[state_record_type(), bytes.clone(), bytes], &payloads).ok()?;
    let [record, kcc1::Value::Bytes(kcc20_prefix), kcc1::Value::Bytes(kcc20_suffix)] = values.as_slice() else {
        return None;
    };
    Some(Issue {
        recipient: state_from_record(record)?,
        kcc20_prefix: kcc20_prefix.clone(),
        kcc20_suffix: kcc20_suffix.clone(),
    })
}

/// The event kind and auth an issue is recorded under. A seed's zero-amount
/// cell is a mint of nothing: it moves no supply, and keeping one kind keeps
/// the token_kind vocabulary closed.
pub const ISSUE_EVENT_KIND: &str = "mint";
pub const ISSUER_AUTH: &str = "issuer";

/// The facts an issue establishes from bytes, as [`check_family`] does for
/// a transfer.
#[derive(Clone, Debug)]
pub struct IssueVerdict {
    pub role: IssuerRole,
    pub recipient: State,
    /// The recipient output is a version-0 P2SH of the pinned KCC20 build
    /// carrying exactly `recipient`: the template halves hash to the pin's
    /// canonical hash, which the issuer's own state also names, and the
    /// rebuilt program commits to the output.
    pub recipient_proven: bool,
    /// The issuer's continuation is its own program with the allowance
    /// reduced by the amount (mint) or unchanged (seed), at no less KAS.
    pub issuer_successor_proven: bool,
    /// That continuation's program, rebuilt and hash-checked against the
    /// output: `Some` exactly when `issuer_successor_proven`, so a reader can
    /// serve the bytes a spender of the continuation must reveal.
    pub issuer_successor_program: Option<Vec<u8>>,
    pub remaining_before: Option<i64>,
    pub remaining_after: Option<i64>,
    pub violations: Vec<String>,
}

/// Check one issue against the issuer's own rules, as the reference app's
/// `mint` and `create` state them: a mint issues `0 < amount <=
/// min(per_issue_limit, remaining)` and decrements its allowance by it; a
/// seed creates an amount of exactly zero and continues unchanged; either
/// way the recipient carries the issuer's extension commitment, an owner
/// scheme the token pin supports and a borrow scheme of the fixed mapping,
/// and the issuer's continuation keeps the issuer input's KAS. The outputs
/// are the two the issuer input authorises, in order: its continuation,
/// then the recipient. `None` when `issuer_program` is not this issuer.
pub fn check_issue(
    pin: &Pin,
    issuer: &PinIssuer,
    issuer_program: &[u8],
    issuer_kas: u64,
    issue: &Issue,
    issuer_output: &FamilyOutput,
    recipient_output: &FamilyOutput,
) -> Option<IssueVerdict> {
    let state = issuer.read_state(issuer_program)?;
    let recipient = issue.recipient;
    let mut violations = Vec::new();

    let canonical = hex::encode(kcc1::template_hash(&issue.kcc20_prefix, &issue.kcc20_suffix));
    let builds_pinned = canonical == pin.template_hash && hex::encode(state.kcc20_template) == pin.template_hash;
    if !builds_pinned {
        violations.push("the issue names a KCC20 template other than the pinned build".into());
    }
    // The recipient program is the template halves around the window, whose
    // context is the template field the issuer carries, then the record.
    let head = kcc1::push_explicit(&state.kcc20_template);
    let view = Template {
        prefix: [issue.kcc20_prefix.as_slice(), head.as_slice()].concat(),
        suffix: issue.kcc20_suffix.clone(),
    };
    let view_ok = head.len() == pin.state.record_offset
        && pin.state.record_offset + STATE_LEN == pin.state.len
        && pin
            .view_template_hash
            .as_deref()
            .is_none_or(|want| hex::encode(view.hash()) == want);
    let recipient_proven = builds_pinned
        && view_ok
        && recipient_output.spk_version == 0
        && view.reconstruct_spk(&recipient).is_some_and(|spk| spk == recipient_output.spk);
    if !recipient_proven {
        violations.push("the recipient output does not commit to the declared state under the pinned build".into());
    }

    match issuer.role {
        IssuerRole::Mint => {
            let within = |limit: Option<i64>| limit.is_some_and(|l| recipient.amount <= l);
            if recipient.amount <= 0 || !within(state.per_issue_limit) || !within(state.remaining) {
                violations.push(format!(
                    "a mint of {} is outside 0 < amount <= min(per-mint limit, remaining allowance)",
                    recipient.amount
                ));
            }
        }
        IssuerRole::Seed => {
            if recipient.amount != 0 {
                violations.push(format!("a seed creates a zero-amount cell, not {}", recipient.amount));
            }
        }
    }
    if recipient.extension_commitment != state.extension_commitment {
        violations.push("the recipient's extension commitment is not the issuer's".into());
    }
    if !pin.owner_schemes.contains(&recipient.owner_scheme) || recipient.borrow_scheme > 0x03 {
        violations.push(
            "the recipient carries an owner scheme outside the supported subset or a borrow scheme outside the fixed mapping".into(),
        );
    }

    let remaining_after = match issuer.role {
        IssuerRole::Mint => state.remaining.and_then(|r| r.checked_sub(recipient.amount)),
        IssuerRole::Seed => state.remaining,
    };
    let issuer_successor_program = issuer
        .successor_program(issuer_program, &state, recipient.amount)
        .filter(|program| {
            issuer_output.spk_version == 0
                && issuer_output.kas == issuer_kas
                && kcc1::envelope_spk(program) == issuer_output.spk
        });
    let issuer_successor_proven = issuer_successor_program.is_some();
    if !issuer_successor_proven {
        violations.push(match issuer.role {
            IssuerRole::Mint => "the minter's continuation is not itself with the allowance reduced by the amount, at its KAS",
            IssuerRole::Seed => "the seed's continuation is not itself unchanged, at its KAS",
        }
        .into());
    }

    Some(IssueVerdict {
        role: issuer.role,
        recipient,
        recipient_proven,
        issuer_successor_proven,
        issuer_successor_program,
        remaining_before: state.remaining,
        remaining_after,
        violations,
    })
}

// ── reveals ──────────────────────────────────────────────────────────

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

/// The payload a spender pushes before the program to select `entry`: the
/// 4-byte tag of a KCC-1-tag pin, or for a silverc pin the script-number
/// bytes of the decimal selector (`snum(0)` is the empty payload).
fn selector_payload(pin: &Pin, entry: &PinEntrypoint) -> Option<Vec<u8>> {
    match pin.dispatch.kind {
        DispatchKind::Kcc1Tag => hex::decode(&entry.select).ok().filter(|tag| tag.len() == 4),
        DispatchKind::SilvercSelector => Some(crate::snum(entry.select.parse::<i64>().ok()?)),
    }
}

/// The argument payloads of a reveal of the pin's standard `role` entry:
/// the program last, the entry's selector before it, the arguments before
/// that. `None` when the pin declares no standard entry for the role or the
/// script selects another one.
fn standard_args(sig_script: &[u8], pin: &Pin, role: &str) -> Option<Vec<Vec<u8>>> {
    let entry = pin.standard_entry(role)?;
    let mut payloads = split_pushes(sig_script)?;
    let _program = payloads.pop()?;
    if payloads.pop()? != selector_payload(pin, entry)? {
        return None;
    }
    Some(payloads)
}

/// Parse a leader reveal of the pin's standard transfer, under either
/// dispatch kind: `transfer(KCC20State[] next_states, byte[] witness)`,
/// field-major (K1 §3.4.6), the tag `79c71c23` for a conformant build. `None`
/// when the script is not this pin's standard transfer — which includes every
/// delegator reveal and every program-specific arm, so callers probe those
/// too.
pub fn parse_transfer_reveal(sig_script: &[u8], pin: &Pin) -> Option<TransferArgs> {
    TransferArgs::from_args(&standard_args(sig_script, pin, "transfer")?)
}

/// What a program-specific leader reveal declares, read against the pin's
/// entrypoint table. The ZealousSwap builds dispatch on 4-byte tags but do
/// not lower `KCC20State[]` field-major: they push the successor's fields
/// one by one and let the program compute the rest (the change cell, or a
/// merge's sum), so the reader derives every successor from the leader's
/// inputs and hash-checks each candidate against the output's own
/// commitment. Nothing here is trusted until that check passes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagReveal {
    /// `zealous-transfer`: one declared successor; the leader's owner keeps
    /// the remainder in a second cell when there is one.
    Transfer { amount: i64, owner: [u8; 32], owner_scheme: u8, borrow_scheme: u8, borrow_guard: [u8; 32] },
    /// `zealous-merge`: every family input folds into one cell owned like
    /// the leader.
    Merge,
    /// `zealous-transfer-whole`: the leader's whole amount moves to a
    /// declared owner.
    Whole { owner: [u8; 32], owner_scheme: u8, borrow_scheme: u8, borrow_guard: [u8; 32] },
    /// `zealous-borrowed-receive` (the venue's name for it is unknown):
    /// `amount` flows from the one follower into this leader under the
    /// leader's borrow rule, no owner signature. `preimage` is the
    /// hash-chain reveal, the value the successor's guard becomes under
    /// scheme 0x03, `key` the one-time key the 2,641 B build and the two
    /// after it commit each link to (`None` for the 2,590 B build's
    /// unkeyed chain), and `signature` a transaction signature (each zero
    /// under the schemes that do not use it); `follower_change` is what the
    /// follower keeps. The program requires `amount + follower_change` to
    /// equal the follower's amount, so the reader derives both successors
    /// from the two proven inputs and hash-checks each. Two layouts exist,
    /// declared per pin by the entrypoint's types: the 2,590 B build (tag
    /// `a85e5b91`) takes four slots, the later builds (tag `13f87a5b`) five.
    Borrowed { amount: i64, preimage: [u8; 32], key: Option<[u8; 32]>, signature: [u8; 65], follower_change: i64 },
}

impl TagReveal {
    /// A borrowed receive's witness as [`verify_borrow`] reads it under the
    /// leader's borrow scheme and the pin's rules: nothing under a
    /// threshold, the signature under a guard key, the preimage under the
    /// Draft's chain, `next ‖ key ‖ signature` under the keyed chain. `None`
    /// for any other reveal, or a keyed rule with no key slot (a pin whose
    /// rule and layout disagree).
    pub fn borrow_witness(&self, borrow_scheme: u8, rules: &Rules) -> Option<Vec<u8>> {
        let TagReveal::Borrowed { preimage, key, signature, .. } = self else {
            return None;
        };
        Some(match (borrow_scheme, rules.hash_chain) {
            (0x02, _) => signature.to_vec(),
            (0x03, HashChainRule::Preimage) => preimage.to_vec(),
            (0x03, HashChainRule::Keyed) => {
                KeyedLink { next: *preimage, pubkey: (*key)?, signature: *signature }.encode()
            }
            _ => Vec::new(),
        })
    }
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

/// Parse a leader reveal of one of a KCC-1-tag pin's program-specific arms:
/// the program last, the 4-byte tag before it, the entrypoint's arguments
/// before that. The arm is chosen by the entry's name, which names a reader
/// kascov implements, and only for an entry whose role is
/// "program-specific"; the standard roles are [`parse_transfer_reveal`]'s.
/// `None` for a follower (delegator) reveal, an unknown tag, or arguments
/// that do not fit the entrypoint's declared layout.
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
    let entry = pin
        .entrypoints
        .iter()
        .find(|e| e.select == tag_hex && e.role == "program-specific")?;
    let fixed = |i: usize, n: usize| -> Option<&[u8]> {
        payloads.get(i).filter(|p| p.len() == n).map(|p| p.as_slice())
    };
    match entry.name.as_str() {
        "zealous-transfer" => {
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
        "zealous-merge" => Some(TagReveal::Merge),
        "zealous-transfer-whole" => {
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
        "zealous-borrowed-receive" => match entry.types.as_str() {
            // the 2,590 B build: amount, preimage, signature, remainder
            "int,byte[32],byte[65],byte[]" => {
                if payloads.len() < 4 {
                    return None;
                }
                let amount = minimal_amount(payloads.first()?)?;
                let preimage: [u8; 32] = fixed(1, 32)?.try_into().ok()?;
                let signature: [u8; 65] = fixed(2, 65)?.try_into().ok()?;
                let follower_change = minimal_amount_or_zero(payloads.get(3)?)?;
                Some(TagReveal::Borrowed { amount, preimage, key: None, signature, follower_change })
            }
            // the 2,641 B build and the two after it: amount, next head,
            // the one-time key that link commits to, signature, remainder
            "int,byte[32],byte[32],byte[65],byte[]" => {
                if payloads.len() < 5 {
                    return None;
                }
                let amount = minimal_amount(payloads.first()?)?;
                let preimage: [u8; 32] = fixed(1, 32)?.try_into().ok()?;
                let key: [u8; 32] = fixed(2, 32)?.try_into().ok()?;
                let signature: [u8; 65] = fixed(3, 65)?.try_into().ok()?;
                let follower_change = minimal_amount_or_zero(payloads.get(4)?)?;
                Some(TagReveal::Borrowed { amount, preimage, key: Some(key), signature, follower_change })
            }
            _ => None,
        },
        _ => None,
    }
}

/// Parse a reveal of the pin's standard delegator: one witness push, the
/// delegator selector, the program.
pub fn parse_delegator_reveal(sig_script: &[u8], pin: &Pin) -> Option<DelegatorArgs> {
    DelegatorArgs::from_args(&standard_args(sig_script, pin, "delegator")?)
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
        // the KCC-1 §3.5.1 preimage expands KCC20State to its six field types; the
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
        assert!(prove_next_states(&t, &[a], std::slice::from_ref(&spk)));
        // any drifted field is a different program, hence a different SPK
        for drift in [
            State { amount: a.amount + 1, ..a },
            State { owner: [0x45; 32], ..a },
            State { owner_scheme: OWNER_P2SH, ..a },
            State { borrow_scheme: 0x01, ..a },
            State { borrow_guard: [1; 32], ..a },
            State { extension_commitment: [1; 32], ..a },
        ] {
            assert!(!prove_next_states(&t, &[drift], std::slice::from_ref(&spk)));
        }
        // count mismatches prove nothing
        assert!(!prove_next_states(&t, &[a, a], std::slice::from_ref(&spk)));
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

        use kcc2::P2pkhHash::{KeyedDraft, Unkeyed};
        let p2pk = state(0x66);
        assert_eq!(owner_auth(&p2pk, &[0u8; 65], Unkeyed), Some(OwnerAuth::P2pkSignature));
        assert_eq!(owner_auth(&p2pk, &[0u8; 64], Unkeyed), None);

        // p2pkh binds the revealed pubkey to the owner under the hash the
        // program checks: unkeyed since KCC-2 #30, keyed for Draft builds,
        // and an owner hashed one way never passes under the other
        let pubkey = [0x07u8; 32];
        let mut auth = pubkey.to_vec();
        auth.extend_from_slice(&[0u8; 65]);
        for (kind, other) in [(Unkeyed, KeyedDraft), (KeyedDraft, Unkeyed)] {
            let mut p2pkh = state(0);
            p2pkh.owner_scheme = OWNER_P2PKH_SCHNORR;
            p2pkh.owner = kind.hash(&pubkey);
            assert_eq!(owner_auth(&p2pkh, &auth, kind), Some(OwnerAuth::P2pkhSchnorr { pubkey }));
            assert_eq!(owner_auth(&p2pkh, &auth, other), None, "{kind:?} owner under {other:?}");
            let mut wrong = auth.clone();
            wrong[0] ^= 1;
            assert_eq!(owner_auth(&p2pkh, &wrong, kind), None, "a foreign pubkey must not pass");
        }
        // an ECDSA key must be compressed: 0x02/0x03, never 0x04
        let mut ecdsa = state(0);
        ecdsa.owner_scheme = OWNER_P2PKH_ECDSA;
        for prefix in [0x02u8, 0x04] {
            let mut key = [0x55u8; 33];
            key[0] = prefix;
            ecdsa.owner = Unkeyed.hash(&key);
            let mut auth = key.to_vec();
            auth.extend_from_slice(&[0u8; 65]);
            assert_eq!(owner_auth(&ecdsa, &auth, Unkeyed).is_some(), prefix == 0x02, "prefix {prefix:02x}");
        }

        let mut p2sh = state(0);
        p2sh.owner_scheme = OWNER_P2SH;
        assert_eq!(owner_auth(&p2sh, &[3], Unkeyed), Some(OwnerAuth::P2shAuthorityInput(3)));
        assert_eq!(owner_auth(&p2sh, &[3, 4], Unkeyed), None);

        let mut covenant = state(0);
        covenant.owner_scheme = OWNER_COVENANT_ID;
        assert_eq!(owner_auth(&covenant, &[], Unkeyed), Some(OwnerAuth::CovenantId));
        assert_eq!(owner_auth(&covenant, &[0], Unkeyed), None);

        let mut unknown = state(0);
        unknown.owner_scheme = 0x05;
        assert_eq!(owner_auth(&unknown, &[], Unkeyed), None);
    }

    // ── borrow schemes ───────────────────────────────────────────

    fn borrowable(scheme: u8, guard: [u8; 32]) -> State {
        State { borrow_scheme: scheme, borrow_guard: guard, ..state(0x77) }
    }

    /// The Draft-era rules the vector pin and the 2,590 B Zealous build check.
    const DRAFT: Rules = Rules {
        hash_chain: HashChainRule::Preimage,
        p2pkh_hash: kcc2::P2pkhHash::KeyedDraft,
        threshold_non_negative: true,
        single_class: false,
    };
    const LAST_CALL: Rules = Rules {
        hash_chain: HashChainRule::Keyed,
        p2pkh_hash: kcc2::P2pkhHash::Unkeyed,
        threshold_non_negative: false,
        single_class: true,
    };

    #[test]
    fn the_default_rules_are_the_last_call_text() {
        assert_eq!(Rules::default(), LAST_CALL);
        assert_eq!(Bounds::default().rules, LAST_CALL);
        let pin: Rules = serde_json::from_str("{}").unwrap();
        assert_eq!(pin, LAST_CALL, "a pin that says nothing gets the current rules");
        let draft: Rules =
            serde_json::from_str(
                r#"{"hash_chain": "preimage", "p2pkh_hash": "keyed-draft", "threshold_non_negative": true, "single_class": false}"#,
            )
            .unwrap();
        assert_eq!(draft, DRAFT);
        assert!(serde_json::from_str::<Rules>(r#"{"hash_chain": "keyed", "typo": 1}"#).is_err());
    }

    #[test]
    fn threshold_borrow_is_proven_and_strict() {
        let mut guard = [0xCC; 32]; // bytes 8..32 unused and non-zero on purpose
        guard[..8].copy_from_slice(&kcc1::encode_state_int(100).unwrap());
        let leader = borrowable(0x01, guard);
        let next = |amount: i64| State { amount, ..leader };
        for rules in [DRAFT, LAST_CALL] {
            // delta 101 > 100: proven
            assert_eq!(
                verify_borrow(&rules, &leader, &next(leader.amount + 101), &[], 5, 5),
                BorrowVerdict::Proven(BorrowScheme::AmountThreshold(100))
            );
            // delta == threshold is invalid, strictly
            assert_eq!(
                verify_borrow(&rules, &leader, &next(leader.amount + 100), &[], 5, 5),
                BorrowVerdict::Invalid(BorrowRefusal::NotIncreased)
            );
            // the witness must be empty
            assert_eq!(
                verify_borrow(&rules, &leader, &next(leader.amount + 101), &[0x01], 5, 5),
                BorrowVerdict::Invalid(BorrowRefusal::ThresholdWitness)
            );
            // less KAS on the rebuilt cell is invalid
            assert_eq!(
                verify_borrow(&rules, &leader, &next(leader.amount + 101), &[], 5, 4),
                BorrowVerdict::Invalid(BorrowRefusal::KasReduced)
            );
        }
        // a negative threshold: refused by a program that requires
        // non-negative, an effective zero for one that does not (#31)
        let mut negative = guard;
        negative[7] |= 0x80;
        let bad = borrowable(0x01, negative);
        let grown = State { amount: bad.amount + 1, ..bad };
        assert_eq!(
            verify_borrow(&DRAFT, &bad, &grown, &[], 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::NegativeThreshold)
        );
        assert_eq!(
            verify_borrow(&LAST_CALL, &bad, &grown, &[], 5, 5),
            BorrowVerdict::Proven(BorrowScheme::AmountThreshold(-100))
        );
        assert_eq!(
            verify_borrow(&LAST_CALL, &bad, &State { amount: bad.amount, ..bad }, &[], 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::NotIncreased),
            "an effective threshold of zero still needs an increase"
        );
        // negative zero decodes to 0, so no program refuses it as negative;
        // an increase of 1 clears it and no increase does not
        let mut negative_zero = [0xCC; 32];
        negative_zero[..8].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x80]);
        let nz = borrowable(0x01, negative_zero);
        for rules in [DRAFT, LAST_CALL] {
            assert_eq!(
                verify_borrow(&rules, &nz, &State { amount: nz.amount + 1, ..nz }, &[], 5, 5),
                BorrowVerdict::Proven(BorrowScheme::AmountThreshold(0))
            );
            assert_eq!(
                verify_borrow(&rules, &nz, &State { amount: nz.amount, ..nz }, &[], 5, 5),
                BorrowVerdict::Invalid(BorrowRefusal::NotIncreased)
            );
        }
        // the stored refusal text for the Draft rule is unchanged
        assert_eq!(
            BorrowRefusal::NegativeThreshold.to_string(),
            "the threshold guard is not a non-negative int payload"
        );
    }

    #[test]
    fn the_preimage_chain_advances_the_guard() {
        let preimage = [0x5A; 32];
        let leader = borrowable(0x03, kcc1::hash(&preimage));
        let mut next = State { amount: leader.amount + 1, ..leader };
        next.borrow_guard = preimage;
        assert_eq!(
            verify_borrow(&DRAFT, &leader, &next, &preimage, 5, 5),
            BorrowVerdict::Proven(BorrowScheme::HashChain(leader.borrow_guard))
        );
        // a forged preimage
        assert_eq!(
            verify_borrow(&DRAFT, &leader, &next, &[0x5B; 32], 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::PreimageMismatch)
        );
        // a successor that does not advance the guard
        let stale = State { borrow_guard: leader.borrow_guard, ..next };
        assert_eq!(
            verify_borrow(&DRAFT, &leader, &stale, &preimage, 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::GuardNotExpected)
        );
        // no increase, no borrow
        let flat = State { amount: leader.amount, ..next };
        assert_eq!(
            verify_borrow(&DRAFT, &leader, &flat, &preimage, 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::NotIncreased)
        );
        // the Last Call rule does not take a bare preimage
        assert_eq!(
            verify_borrow(&LAST_CALL, &leader, &next, &preimage, 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::KeyedWitness)
        );
    }

    /// #18: the link commits to a one-time key, so a front-runner who sees
    /// `next` cannot reuse it, and the key's signature is consensus's: the
    /// best kascov can say is structural.
    #[test]
    fn the_keyed_chain_is_structural_and_commits_to_its_key() {
        let link = KeyedLink { next: [0x5A; 32], pubkey: [0xB1; 32], signature: [0x44; 65] };
        let leader = borrowable(0x03, hash_chain_link(&link.next, &link.pubkey));
        let next = State { amount: leader.amount + 1, borrow_guard: link.next, ..leader };
        let verdict = verify_borrow(&LAST_CALL, &leader, &next, &link.encode(), 5, 5);
        assert_eq!(verdict, BorrowVerdict::Structural(BorrowScheme::HashChain(leader.borrow_guard)));
        assert_eq!(verdict.tier(), Some("structural"));
        assert_eq!(KeyedLink::parse(&link.encode()), Some(link));
        // another key over the same next head breaks the link
        let foreign = KeyedLink { pubkey: [0xB2; 32], ..link };
        assert_eq!(
            verify_borrow(&LAST_CALL, &leader, &next, &foreign.encode(), 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::LinkMismatch)
        );
        // the Draft rule reads the same witness as a wrong-length preimage
        assert_eq!(
            verify_borrow(&DRAFT, &leader, &next, &link.encode(), 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::PreimageWitness)
        );
        assert_eq!(
            verify_borrow(&LAST_CALL, &leader, &next, &link.encode()[..128], 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::KeyedWitness)
        );
        // the guard must advance to the revealed head
        assert_eq!(
            verify_borrow(&LAST_CALL, &leader, &State { borrow_guard: leader.borrow_guard, ..next }, &link.encode(), 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::GuardNotExpected)
        );
    }

    #[test]
    fn schnorr_borrow_is_never_more_than_structural() {
        let leader = borrowable(0x02, [0xAB; 32]);
        let next = State { amount: leader.amount + 1, ..leader };
        assert_eq!(
            verify_borrow(&LAST_CALL, &leader, &next, &[0u8; 65], 5, 5),
            BorrowVerdict::Structural(BorrowScheme::SchnorrSignature([0xAB; 32]))
        );
        assert_eq!(
            verify_borrow(&LAST_CALL, &leader, &next, &[0u8; 64], 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::SchnorrWitness)
        );
    }

    #[test]
    fn disabled_reserved_and_drifted_borrows_refuse() {
        let leader = borrowable(0x00, [0; 32]);
        let next = State { amount: leader.amount + 1, ..leader };
        assert_eq!(
            verify_borrow(&LAST_CALL, &leader, &next, &[], 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::Disabled)
        );
        // a reserved byte is `reject` under the algorithm's final else
        let reserved = borrowable(0x04, [0; 32]);
        assert_eq!(
            verify_borrow(&LAST_CALL, &reserved, &State { amount: reserved.amount + 1, ..reserved }, &[], 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::Reserved(0x04))
        );
        // any drifted preserved field refuses before scheme logic runs
        let threshold = borrowable(0x01, [0; 32]);
        let mut drifted = State { amount: threshold.amount + 1, ..threshold };
        drifted.extension_commitment = [1; 32];
        assert_eq!(
            verify_borrow(&LAST_CALL, &threshold, &drifted, &[], 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::NotPreserved)
        );
        // #31: a cell under an unassigned owner scheme cannot be borrowed
        let custom = State { owner_scheme: 0x80, ..threshold };
        assert_eq!(
            verify_borrow(&LAST_CALL, &custom, &State { amount: custom.amount + 1, ..custom }, &[], 5, 5),
            BorrowVerdict::Invalid(BorrowRefusal::UnassignedOwnerScheme(0x80))
        );
        // the reason names reserved and custom bytes apart, as KCC-2 does
        let custom_why = BorrowRefusal::UnassignedOwnerScheme(0x80).to_string();
        let reserved_why = BorrowRefusal::UnassignedOwnerScheme(0x05).to_string();
        assert!(custom_why.contains("custom") && !custom_why.contains("reserved"), "{custom_why}");
        assert!(reserved_why.contains("reserved") && !reserved_why.contains("custom"), "{reserved_why}");
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
        let inputs = [
            // the RECIPIENT's cell is the leader (lowest input index)
            FamilyInput { input_index: 1, state: recipient, kas: 7 },
            FamilyInput { input_index: 4, state: sender, kas: 2 },
        ];
        let draft = Bounds { rules: DRAFT, ..Bounds::default() };
        let verdict = check_family(&inputs, &transfer, &t, &outputs, &draft).unwrap();
        assert_eq!(verdict.path_normal, Some(false));
        assert!(matches!(verdict.borrow, Some(BorrowVerdict::Proven(BorrowScheme::HashChain(_)))));
        assert!(verdict.family_total_ok);
        assert!(verdict.violations.is_empty(), "{:?}", verdict.violations);
        // the bounds carry the pin's rules: under the Last Call rule the same
        // bare preimage is not a witness at all
        let verdict = check_family(&inputs, &transfer, &t, &outputs, &Bounds::default()).unwrap();
        assert_eq!(verdict.borrow, Some(BorrowVerdict::Invalid(BorrowRefusal::KeyedWitness)));
    }

    /// #16: one extension commitment across a standard transfer's inputs and
    /// outputs. Two classes travelling side by side, each preserved, passed
    /// every pre-#16 check; it is a breach now. kascov's own vector, since
    /// the official set does not cover the shape (tests/kcc20_vectors.rs
    /// runs it in the official format).
    #[test]
    fn two_classes_side_by_side_are_a_breach() {
        let t = test_template();
        let rock = state(0x10);
        let paper = State { extension_commitment: [0xAA; 32], amount: 50, ..state(0x10) };
        let mut witness = vec![0x00];
        witness.extend_from_slice(&[0u8; 65]);
        let transfer = TransferArgs { next_states: vec![rock, paper], witness };
        let outputs: Vec<FamilyOutput> = transfer
            .next_states
            .iter()
            .map(|s| FamilyOutput { spk_version: 0, spk: t.reconstruct_spk(s).unwrap(), kas: 1 })
            .collect();
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
        assert!(verdict.successors_proven && verdict.family_total_ok && verdict.classes_preserved);
        assert!(verdict.mixed_classes);
        assert!(verdict.violations.iter().any(|v| v.contains("2 different extension commitments")), "{:?}", verdict.violations);
    }

    /// §1: a declared negative successor is named for what it is, even when
    /// the family total balances.
    #[test]
    fn a_negative_successor_is_named() {
        let t = test_template();
        let input = state(0x10);
        let mut witness = vec![0x00];
        witness.extend_from_slice(&[0u8; 65]);
        let low = State { amount: -600, ..input };
        let high = State { amount: input.amount + 600, ..input };
        let transfer = TransferArgs { next_states: vec![low, high], witness };
        let verdict = check_family(
            &[FamilyInput { input_index: 0, state: input, kas: 1 }],
            &transfer,
            &t,
            &[FamilyOutput { spk_version: 0, spk: t.reconstruct_spk(&high).unwrap(), kas: 1 }],
            &Bounds::default(),
        )
        .unwrap();
        assert!(verdict.family_total_ok, "the total hides it");
        assert!(verdict.violations.iter().any(|v| v == "a successor amount is negative"));
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
            "reference_kcc20_a" => REFERENCE_A.to_vec(),
            "reference_kcc20_b" => include_bytes!("../fixtures/kcc0020/reference_kcc20_b.bin").to_vec(),
            "reference_public_mint_a" => MINTER_A.to_vec(),
            "reference_public_mint_b" => include_bytes!("../fixtures/kcc0020/reference_public_mint_b.bin").to_vec(),
            "reference_token_seed_a" => SEED_A.to_vec(),
            "reference_token_seed_b" => include_bytes!("../fixtures/kcc0020/reference_token_seed_b.bin").to_vec(),
            "kasparocket_kcc20_a" => KASPAROCKET_A.to_vec(),
            "kasparocket_kcc20_b" => include_bytes!("../fixtures/kcc0020/kasparocket_kcc20_b.bin").to_vec(),
            other => panic!("a pin names fixture {other} that this test does not carry"),
        }
    }

    const REFERENCE_A: &[u8] = include_bytes!("../fixtures/kcc0020/reference_kcc20_a.bin");
    const MINTER_A: &[u8] = include_bytes!("../fixtures/kcc0020/reference_public_mint_a.bin");
    const SEED_A: &[u8] = include_bytes!("../fixtures/kcc0020/reference_token_seed_a.bin");
    const HASH_REFERENCE: &str = "9703112ee6e3555107cd168858992b77d3b74f655205b2b463b1f9ec2ec73cf7";
    const VIEW_REFERENCE: &str = "734850b0af0aeef49f009167bf9ddd238fd8c5cdbe214234f97181ac6f5cc498";
    const KASPAROCKET_A: &[u8] = include_bytes!("../fixtures/kcc0020/kasparocket_kcc20_a.bin");
    const HASH_KASPAROCKET: &str = "ff3ae732e3a2819bf9181f5aed6b1e9d0f7abea70a1dd655b8e777cbcbd2422b";
    const VIEW_KASPAROCKET: &str = "3d689d6273b7631e69f672d5ca744cb266ae997b04974196439858675cc04fe1";

    fn pin_by(prefix: &str) -> &'static Pin {
        pins().iter().find(|p| p.template_hash.starts_with(prefix)).expect("pin committed")
    }

    #[test]
    fn every_committed_pin_parses_and_names_its_spec() {
        for pin in pins() {
            assert!(
                pin.spec_commit == SPEC_COMMIT || pin.spec_commit == DRAFT_SPEC_COMMIT,
                "{}: a pin names a commit this module reads",
                pin.spec_commit
            );
            assert_eq!(pin.template_hash.len(), 64);
            assert!(pin.template_hash.chars().all(|c| c.is_ascii_hexdigit()));
            assert_eq!(pin.state.len, pin.state.record_offset + STATE_LEN, "the record closes the window");
            // a record view differs from the canonical template only where
            // the window holds more than the record, and then it is declared
            assert_eq!(pin.view_template_hash.is_some(), pin.state.record_offset > 0);
            assert!(!pin.review.fixtures.is_empty(), "a pin without fixtures proves nothing");
            assert!(matches!(pin.review.status.as_str(), "reviewed" | "unreviewed"));
            // A declared family is a venue-owned display name: it carries the
            // "Builder · " separator so a prefix rule can own it, and it must
            // not collide with any name the legacy decoders key on, or a
            // spec program would be routed into the 46-byte splice by name.
            if let Some(family) = &pin.family {
                assert!(family.contains(" · "), "{family}: a pin family carries the venue separator");
                assert!(!spec_template_names()[..4].contains(&family.as_str()));
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
                assert_eq!(pin.family_label(), pin.standing());
            }
            assert!(is_spec_template(pin.family_label()));
            assert!(is_spec_template(pin.standing()));
            // a standard entry is selected the way the dispatch block says,
            // and a KCC-1 tag is the hash of the entry's own signature
            for (role, declared) in [("transfer", &pin.dispatch.transfer), ("delegator", &pin.dispatch.transfer_delegator)] {
                let Some(entry) = pin.standard_entry(role) else { continue };
                assert_eq!(&entry.select, declared, "{role}");
                if pin.dispatch.kind == DispatchKind::Kcc1Tag {
                    let tag = if role == "transfer" { transfer_tag() } else { delegator_tag() };
                    assert_eq!(entry.select, hex::encode(tag), "{role}");
                }
            }
            // The claimed template hash is never trusted from the file: every
            // fixture must recompute it around the pin's own window, and the
            // pair must differ in every state field so the pin cannot silently
            // narrow (the two-sentinel discipline).
            let programs: Vec<Vec<u8>> = pin.review.fixtures.iter().map(|f| fixture_bytes(f)).collect();
            assert!(programs.len() >= 2, "a pin needs a fixture pair");
            for program in &programs {
                assert_eq!(hex::encode(pin.canonical_hash(program).unwrap()), pin.template_hash, "fixture recomputes the pin's hash");
                if let Some(view) = &pin.view_template_hash {
                    assert_eq!(&hex::encode(pin.cut(program).unwrap().hash()), view);
                }
                let (named, _) = pinned_program_state(program).expect("the pin recognises its own fixture");
                assert_eq!(named.template_hash, pin.template_hash);
            }
            let at = pin.state.record_start();
            let sa = State::decode(&programs[0][at..at + STATE_LEN]).unwrap();
            let sb = State::decode(&programs[1][at..at + STATE_LEN]).unwrap();
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
            // every issuer is held to the same discipline: its fixtures
            // recompute its hash, read as it, name this token's build, and
            // differ in the fields kascov reads
            for issuer in &pin.issuers {
                assert!(issuer.issue_entry().is_some(), "{}: an issuer declares its issue entry", issuer.name);
                if issuer.role == IssuerRole::Mint {
                    assert!(issuer.fields.remaining.is_some() && issuer.fields.per_issue_limit.is_some());
                }
                let states: Vec<IssuerState> = issuer
                    .fixtures
                    .iter()
                    .map(|f| {
                        let program = fixture_bytes(f);
                        let (owner, named, state) = pinned_issuer_state(&program).expect("the issuer reads its own fixture");
                        assert_eq!((owner.template_hash.as_str(), named.name.as_str()), (pin.template_hash.as_str(), issuer.name.as_str()));
                        assert!(pinned_program_state(&program).is_none(), "an issuer is never a token cell");
                        state
                    })
                    .collect();
                assert_eq!(states.len(), 2);
                for state in &states {
                    assert_eq!(hex::encode(state.kcc20_template), pin.template_hash);
                }
                assert_ne!(states[0].extension_commitment, states[1].extension_commitment);
                if issuer.role == IssuerRole::Mint {
                    assert_ne!(states[0].remaining, states[1].remaining);
                    assert_ne!(states[0].per_issue_limit, states[1].per_issue_limit);
                }
                // a declared self-template field carries the issuer's own
                // hash on every chain fixture: the field a genesis rebuild sets
                if let Some(own) = issuer.fields.self_template {
                    for state in &states {
                        assert_eq!(state.values[own], kcc1::Value::Bytes(hex::decode(&issuer.template_hash).unwrap()), "{}", issuer.name);
                    }
                }
            }
            // A pin's genesis rebuild names only seeds it declares, each with a
            // self-template field and reviewed bytes this crate carries: a seed
            // the rebuild could not build is a defect, never a silent no-op.
            if let Some(decl) = &pin.genesis_rebuild {
                for name in &decl.seeds {
                    let issuer = pin
                        .issuers
                        .iter()
                        .find(|i| &i.name == name)
                        .unwrap_or_else(|| panic!("{name}: not an issuer of this pin"));
                    assert_eq!(issuer.role, IssuerRole::Seed, "{name}");
                    assert!(issuer.fields.self_template.is_some(), "{name}: a rebuilt seed declares self_template");
                    assert!(issuer.reviewed_halves().is_some(), "{name}: a rebuilt seed has reviewed bytes");
                    assert!(issuer.seed_program(pin, &[0x11; 32], &[0; 32]).is_some(), "{name}");
                }
                assert!(decl.declared_cells || !decl.seeds.is_empty(), "a declaration that rebuilds nothing");
                assert_eq!(pin.review.status, "reviewed", "only a reviewed build's programs are rebuilt");
            }
        }
        // only the reference build rebuilds genesis outputs today
        let declaring: Vec<&str> = pins().iter().filter(|p| p.genesis_rebuild.is_some()).map(|p| &p.template_hash[..8]).collect();
        assert_eq!(declaring, vec!["9703112e"]);
        // an unpinned program is never claimed, spec-shaped or not
        let mut foreign = vec![0x6b];
        foreign.extend_from_slice(&state(0x11).encode().unwrap());
        foreign.extend_from_slice(&[0x6c, 0x75, 0x75, 0x51]);
        assert!(pinned_program_state(&foreign).is_none());
        // the committed set: all four Zealous pins declare the venue family
        // (one label, so the candidate list did not widen); the vector and
        // the reference keep their standing, which says what they are
        for prefix in ["653012f1", "2f2671fc", "58c24260", "50ff50d8"] {
            assert_eq!(pin_by(prefix).family_label(), "Zealous · token");
            assert_eq!(pin_by(prefix).standing(), DRAFT_BUILD_TEMPLATE);
        }
        assert_eq!(pin_by("4abcc188").family_label(), DRAFT_BUILD_TEMPLATE);
        assert_eq!(pin_by("9703112e").review.status, "reviewed");
        assert_eq!(pin_by("9703112e").family_label(), REVIEWED_BUILD_TEMPLATE, "reviewed, and no rule departs from the Final text");
        // KaspaRocket's build: the reference actor at a 10/10 configuration
        // the Final text allows. No family: the label names the build's
        // standing, and the venue comes from a token's payload or market,
        // never from the build (other deployers use it too)
        assert_eq!(pin_by("ff3ae732").review.status, "reviewed");
        assert_eq!(pin_by("ff3ae732").family, None);
        assert_eq!(pin_by("ff3ae732").family_label(), REVIEWED_BUILD_TEMPLATE);
        // its name, served on every coin of the build (verified token or
        // not, KaspaRocket's or another deployer's), names the build and
        // cites its source, the reference pin's way
        assert_eq!(
            pin_by("ff3ae732").name,
            "KCC-20 reference actor at 10 token inputs / 10 outputs (La-Gauffre/KaspaRocket-Silverscript-Contract 86fe1e8e, kcc20.sil)"
        );
        // A pin's name is served as `argent_build.kcc20.name` on every coin
        // of its build, whatever that coin's token has proven, so no pin may
        // name a token standing ("KCC-20 token" is earned per token, on
        // /token/{id}) or claim conformance for a third party's build.
        for pin in pins() {
            let lower = pin.name.to_lowercase();
            for claim in ["kcc-20 token", "kcc20 token", "compliant", "conformant", "conforming"] {
                assert!(!lower.contains(claim), "{}: the pin name says {claim:?}", pin.template_hash);
            }
        }
        assert_eq!(
            spec_template_names(),
            vec![REVIEWED_BUILD_TEMPLATE, LAYOUT_TEMPLATE, DRAFT_BUILD_TEMPLATE, RETIRED_SPEC_TEMPLATE, "Zealous · token"]
        );
        // a reveal names a build, never a token's standing: the token-level
        // names are no stamp, so no cell of an unproven token carries one
        for token_level in [SPEC_TOKEN_TEMPLATE, UNPROVEN_TOKEN_TEMPLATE] {
            assert!(!is_spec_template(token_level), "{token_level}");
            assert!(pins().iter().all(|pin| pin.family_label() != token_level && pin.standing() != token_level));
        }
        assert!(is_spec_template("Zealous · token"));
        assert!(is_spec_template(RETIRED_SPEC_TEMPLATE), "rows stamped before the Last Call labels still route");
        assert!(!is_spec_template("Zealous · token cell"), "the legacy 46-byte family is not a spec label");
        assert!(!is_spec_template(crate::kcc20::TOKEN_TEMPLATE));
        // the schema itself, including the optional fields
        let sample = r#"{
            "spec_commit": "1ed87ae",
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
        assert_eq!(pin.rules, Rules::default(), "no rules declared: the Final ones");
        assert_eq!(pin.state.record_offset, 0);
        assert!(pin.issuers.is_empty() && pin.view_template_hash.is_none());
        assert_eq!(pin.genesis_rebuild, None, "a pin rebuilds no genesis output unless it says so");
        assert_eq!(pin.family.as_deref(), Some("Sample · token"), "the family round-trips");
        assert_eq!(pin.family_label(), "Sample · token");
        // the field is optional: a pin without one stamps its standing
        let bare: Pin = serde_json::from_str(&sample.replace("\"family\": \"Sample · token\",\n", "")).unwrap();
        assert_eq!(bare.family, None);
        assert_eq!(bare.family_label(), bare.standing());
        assert!(pin_for(&[0u8; 32]).is_none());
        assert_eq!(h("00"), vec![0x00]);
    }

    /// The reference actor's window is its own template hash and then the
    /// record: the pin keys on the canonical hash around the window, the
    /// record view resolves to the same pin as an alias, and both must
    /// reproduce, so a program with other bytes in the template field is
    /// not this build even though its canonical halves are.
    #[test]
    fn the_reference_window_keys_on_the_canonical_hash_with_the_view_as_alias() {
        let pin = pin_by("9703112e");
        assert_eq!(pin.template_hash, HASH_REFERENCE);
        assert_eq!(pin.view_template_hash.as_deref(), Some(VIEW_REFERENCE));
        let h32 = |s: &str| -> [u8; 32] { h(s).try_into().unwrap() };
        assert_eq!(pin_for(&h32(HASH_REFERENCE)).unwrap().template_hash, HASH_REFERENCE);
        assert_eq!(pin_for(&h32(VIEW_REFERENCE)).unwrap().template_hash, HASH_REFERENCE, "the view is an alias");
        // the template field holds the canonical hash itself
        assert_eq!(&REFERENCE_A[1..34], [&[0x20][..], &h(HASH_REFERENCE)].concat().as_slice());
        let (named, state) = pinned_program_state(REFERENCE_A).unwrap();
        assert_eq!(named.template_hash, HASH_REFERENCE);
        // fixture A is the genesis cell of the first mainnet reference token
        // (7d424a92..:0, revealed by 2a1ce335.. input 0): 2.1e15 base
        // units, owned by a key, unborrowable, the all-zero class
        assert_eq!((state.amount, state.owner_scheme, state.borrow_scheme), (2_100_000_000_000_000, OWNER_P2PK_SCHNORR, 0x00));
        assert_eq!(state.extension_commitment, [0; 32]);
        // fixture B (testnet-10 05060a38..:1, revealed by f13f6ce0.. input 0):
        // a zero-amount p2sh-owned cell under a threshold-10 borrow, class 77545d3d
        let (_, b) = pinned_program_state(&fixture_bytes("reference_kcc20_b")).unwrap();
        assert_eq!((b.amount, b.owner_scheme, b.borrow_scheme, b.borrow_guard[0]), (0, OWNER_P2SH, 0x01, 10));
        assert_eq!(hex::encode(&b.extension_commitment[..4]), "77545d3d");
        // the two chain cells differ only inside the window
        let other = fixture_bytes("reference_kcc20_b");
        assert_eq!((REFERENCE_A.len(), other.len()), (4031, 4031));
        assert_eq!((&REFERENCE_A[..2], &REFERENCE_A[146..]), (&other[..2], &other[146..]));
        // the 112-byte cut a Draft pin would make finds no record at offset 1
        assert!(Template::cut(REFERENCE_A, 1).is_none());
        // other bytes in the template field: canonical halves intact, view not
        let mut drifted = REFERENCE_A.to_vec();
        drifted[2] ^= 0x01;
        assert_eq!(hex::encode(pin.canonical_hash(&drifted).unwrap()), HASH_REFERENCE);
        assert!(pinned_program_state(&drifted).is_none());
        // a successor is rebuilt around the record and reads back as itself
        let next = State { amount: 7, owner: [0x99; 32], ..state };
        let program = pin.cut(REFERENCE_A).unwrap().program(&next).unwrap();
        let (again, read) = pinned_program_state(&program).unwrap();
        assert_eq!((again.template_hash.as_str(), read), (HASH_REFERENCE, next));
    }

    /// KaspaRocket's token build (La-Gauffre/KaspaRocket-Silverscript-Contract
    /// 86fe1e8e kcc20.sil) is the reference actor with its delegate and
    /// output bounds raised to 9 and 10: the same window, record offset,
    /// dispatch, schemes and rules, its own canonical hash and record view,
    /// both of which must reproduce. Neither pin claims the other's bytes.
    #[test]
    fn the_kasparocket_build_is_the_reference_actor_at_ten_by_ten() {
        let pin = pin_by("ff3ae732");
        let reference = pin_by("9703112e");
        let h32 = |s: &str| -> [u8; 32] { h(s).try_into().unwrap() };
        assert_eq!((pin.template_hash.as_str(), pin.view_template_hash.as_deref()), (HASH_KASPAROCKET, Some(VIEW_KASPAROCKET)));
        assert_eq!(pin_for(&h32(HASH_KASPAROCKET)).unwrap().template_hash, HASH_KASPAROCKET);
        assert_eq!(pin_for(&h32(VIEW_KASPAROCKET)).unwrap().template_hash, HASH_KASPAROCKET, "the view is an alias");
        // every declaration but the identity, the bounds and the issuers is
        // the reference pin's
        assert_eq!((pin.max_token_inputs, pin.max_token_outputs), (10, 10));
        assert_eq!((reference.max_token_inputs, reference.max_token_outputs), (3, 3));
        assert_eq!(pin.spec_commit, reference.spec_commit);
        assert_eq!(pin.dispatch.kind, reference.dispatch.kind);
        assert_eq!(
            (&pin.dispatch.transfer, &pin.dispatch.transfer_delegator),
            (&reference.dispatch.transfer, &reference.dispatch.transfer_delegator)
        );
        assert_eq!(
            (pin.state.start, pin.state.len, pin.state.record_offset),
            (reference.state.start, reference.state.len, reference.state.record_offset)
        );
        assert_eq!((&pin.owner_schemes, &pin.borrow_schemes), (&reference.owner_schemes, &reference.borrow_schemes));
        assert_eq!(pin.rules, reference.rules);
        assert_eq!(pin.extension.scheme, reference.extension.scheme);
        assert_eq!(pin.in_script_template_hash_fn, reference.in_script_template_hash_fn);
        let entries = |p: &Pin| -> Vec<(String, String, String, String)> {
            p.entrypoints.iter().map(|e| (e.name.clone(), e.types.clone(), e.select.clone(), e.role.clone())).collect()
        };
        assert_eq!(entries(pin), entries(reference));
        assert!(pin.issuers.is_empty(), "no issuer actor: a token's supply is what its genesis declares");
        assert!(pin.nonconformities().is_empty(), "{:?}", pin.nonconformities());
        // its publisher released no program artifact (KCC-20 §3), so the
        // build gets no conformance verdict either way; the reference, whose
        // artifact.json is published, keeps its true. Neither is a departure.
        assert_eq!(pin.artifact, Some(PinArtifact::Unpublished));
        assert_eq!(pin.conformant(), None, "not assessed: kascov cannot check an artifact nobody published");
        assert_eq!(reference.artifact, None);
        assert_eq!(reference.conformant(), Some(true));
        assert_eq!(pin.standing(), REVIEWED_BUILD_TEMPLATE, "still a reviewed build: its verified tokens stay KCC-20 tokens");
        // the template field holds the canonical hash itself
        assert_eq!(&KASPAROCKET_A[1..34], [&[0x20][..], &h(HASH_KASPAROCKET)].concat().as_slice());
        // fixture A is ROCKET's genesis cell on mainnet (71b6c6af..:0, revealed
        // by 427214fd.. input 0): 1e9 owned by the deployer's key
        let (named, a) = pinned_program_state(KASPAROCKET_A).unwrap();
        assert_eq!(named.template_hash, HASH_KASPAROCKET);
        assert_eq!((a.amount, a.owner_scheme, a.borrow_scheme), (1_000_000_000, OWNER_P2PK_SCHNORR, 0x00));
        assert_eq!(hex::encode(&a.owner[..4]), "1094ce6c");
        assert_eq!((a.borrow_guard, a.extension_commitment), ([0; 32], [0; 32]));
        // fixture B (9f1be102..:2, leader of b5d073a6 input 1) is owned by
        // ROCKET's market covenant
        let other = fixture_bytes("kasparocket_kcc20_b");
        let (_, b) = pinned_program_state(&other).unwrap();
        assert_eq!((b.amount, b.owner_scheme), (633_030_484, OWNER_COVENANT_ID));
        assert_eq!(hex::encode(b.owner), "a11ebeb468d494a75035cc2671d1b1e4d97e0b797fa3dad2f95075de27e1ea04");
        // the two chain cells differ only inside the window
        assert_eq!((KASPAROCKET_A.len(), other.len()), (11_458, 11_458));
        assert_eq!((&KASPAROCKET_A[..2], &KASPAROCKET_A[146..]), (&other[..2], &other[146..]));
        // the same window, other halves: neither pin claims the other's bytes
        assert_ne!(hex::encode(reference.canonical_hash(KASPAROCKET_A).unwrap()), HASH_REFERENCE);
        assert_ne!(hex::encode(pin.canonical_hash(REFERENCE_A).unwrap()), HASH_KASPAROCKET);
        assert_eq!(pinned_program_state(REFERENCE_A).unwrap().0.template_hash, HASH_REFERENCE);
        // fixtures_share_class: the window's end from the program's own bytes.
        // Its own length (11,458) and minus its suffix (-11,312) are pushed
        // once per output slot, the suffix length (11,312) twice; the
        // reference pushes 4,031 and -3,885 once per slot, 3,885 twice
        let count = |program: &[u8], needle: &[u8]| program.windows(needle.len()).filter(|w| *w == needle).count();
        assert_eq!(KASPAROCKET_A.len() - pin.state.start - pin.state.len, 11_312);
        assert_eq!(count(KASPAROCKET_A, &[0x02, 0xc2, 0x2c]), 10, "11,458 per output slot");
        assert_eq!(count(KASPAROCKET_A, &[0x02, 0x30, 0xac]), 10, "-11,312 per output slot");
        assert_eq!(count(KASPAROCKET_A, &[0x04, 0x30, 0x2c, 0x00, 0x00]), 2, "the suffix length");
        assert_eq!(count(REFERENCE_A, &[0x02, 0xbf, 0x0f]), 3, "4,031 per output slot");
        assert_eq!(count(REFERENCE_A, &[0x02, 0x2d, 0x8f]), 3, "-3,885 per output slot");
        assert_eq!(count(REFERENCE_A, &[0x04, 0x2d, 0x0f, 0x00, 0x00]), 2, "the suffix length");
        // a successor is rebuilt around the record and reads back as itself
        let next = State { amount: 7, owner: [0x99; 32], ..a };
        let program = pin.cut(KASPAROCKET_A).unwrap().program(&next).unwrap();
        let (again, read) = pinned_program_state(&program).unwrap();
        assert_eq!((again.template_hash.as_str(), read), (HASH_KASPAROCKET, next));
    }

    /// One chain transition of the KaspaRocket build as `extract_spend.py`
    /// wrote it from a read-only DB copy: each family input's 145-byte
    /// window (its program is fixture A's halves around it, proven here
    /// against the P2SH it spent), the leader's whole signature script, and
    /// the family outputs in order.
    fn kasparocket_spend(json: &str) -> (Vec<FamilyInput>, TransferArgs, Vec<FamilyOutput>) {
        let pin = pin_by("ff3ae732");
        let (start, end) = (pin.state.start, pin.state.start + pin.state.len);
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut inputs = Vec::new();
        let mut programs = Vec::new();
        for input in v["inputs"].as_array().unwrap() {
            let program = [&KASPAROCKET_A[..start], &h(input["window"].as_str().unwrap())[..], &KASPAROCKET_A[end..]].concat();
            assert_eq!(kcc1::envelope_spk(&program), h(input["spk"].as_str().unwrap()), "{} spent this program", input["outpoint"]);
            let (named, state) = pinned_program_state(&program).expect("every family input is this build");
            assert_eq!(named.template_hash, HASH_KASPAROCKET);
            inputs.push(FamilyInput {
                input_index: input["input_index"].as_u64().unwrap() as u32,
                state,
                kas: input["kas"].as_u64().unwrap(),
            });
            programs.push(program);
        }
        let leader_sig = h(v["leader_sig"].as_str().unwrap());
        // the leader is the lowest family input and reveals its own program
        assert_eq!(split_pushes(&leader_sig).unwrap().last(), programs.first());
        let transfer = parse_transfer_reveal(&leader_sig, pin).expect("the leader reads as the standard transfer");
        let outputs = v["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| FamilyOutput {
                spk_version: o["spk_version"].as_u64().unwrap() as u16,
                spk: h(o["spk"].as_str().unwrap()),
                kas: o["kas"].as_u64().unwrap(),
            })
            .collect();
        (inputs, transfer, outputs)
    }

    /// The committed pin reads two real transfers beyond the reference
    /// build's 3/3 bounds with no violation: mainnet b5d073a6 (ROCKET, a
    /// market-owned leader and three delegates) and testnet-10 30392428 (a
    /// leader and nine delegates, the build's maximum). Under the reference
    /// pin's bounds the same transitions are flagged.
    #[test]
    fn the_kasparocket_pin_reads_chain_transfers_past_the_reference_bounds() {
        let pin = pin_by("ff3ae732");
        let template = pin.cut(KASPAROCKET_A).unwrap();
        for (json, family_inputs, family_outputs, total) in [
            (include_str!("../fixtures/kcc0020/kasparocket_kcc20_spend_mainnet_b5d073a6.json"), 4, 2, 675_598_368),
            (include_str!("../fixtures/kcc0020/kasparocket_kcc20_spend_tn10_30392428.json"), 10, 1, 625_564_310),
        ] {
            let (inputs, transfer, outputs) = kasparocket_spend(json);
            assert_eq!((inputs.len(), outputs.len()), (family_inputs, family_outputs));
            assert_eq!(inputs.iter().map(|i| i128::from(i.state.amount)).sum::<i128>(), total);
            let verdict = check_family(&inputs, &transfer, &template, &outputs, &pin.bounds()).unwrap();
            assert!(verdict.violations.is_empty(), "{:?}", verdict.violations);
            assert!(verdict.successors_proven && verdict.family_total_ok && verdict.bounds_ok && verdict.schemes_ok);
            let narrow = check_family(&inputs, &transfer, &template, &outputs, &pin_by("9703112e").bounds()).unwrap();
            assert!(!narrow.bounds_ok);
            assert!(narrow.violations.iter().any(|v| v.starts_with("family exceeds bounds")), "{:?}", narrow.violations);
        }
    }

    /// `own_cut` keeps a template only when the program is the pin's own
    /// build: the plain cut of the drifted program above (canonical halves
    /// intact, other bytes in the template field) still finds the record,
    /// and so would a cut of another build with this layout, but neither is
    /// this build's template. Every reveal `pinned_program_state` gives this
    /// pin cuts to the same template.
    #[test]
    fn own_cut_keeps_only_the_pins_own_build() {
        let pin = pin_by("9703112e");
        let own = pin.own_cut(REFERENCE_A).expect("fixture A is this build");
        assert_eq!(Some(&own), pin.cut(REFERENCE_A).as_ref());
        assert_eq!(hex::encode(own.hash()), VIEW_REFERENCE);
        let b = fixture_bytes("reference_kcc20_b");
        assert_eq!(pin.own_cut(&b), Some(own.clone()), "every reveal of the build cuts to one template");
        // the template field drifted: the plain cut succeeds, the own cut does not
        let mut drifted = REFERENCE_A.to_vec();
        drifted[2] ^= 0x01;
        assert!(pin.cut(&drifted).is_some());
        assert_eq!(pin.own_cut(&drifted), None);
        // a byte outside the window: neither hash reproduces
        let mut suffix = REFERENCE_A.to_vec();
        suffix[2_000] ^= 0x01;
        assert!(pin.cut(&suffix).is_some());
        assert_eq!(pin.own_cut(&suffix), None);
        // the same layout under another canonical hash: a pin whose bytes
        // these are not refuses them, whatever the record says
        let mut other_build = pin.clone();
        other_build.template_hash.replace_range(..2, "00");
        assert!(other_build.cut(REFERENCE_A).is_some());
        assert_eq!(other_build.own_cut(REFERENCE_A), None);
        let mut other_view = pin.clone();
        other_view.view_template_hash = Some("00".repeat(32));
        assert_eq!(other_view.own_cut(REFERENCE_A), None, "the view hash is checked where declared");
        // another pin's program, either way round
        let zealous = pin_by("2f2671fc");
        assert!(zealous.own_cut(ZEALOUS_2641_A).is_some());
        assert_eq!(pin.own_cut(ZEALOUS_2641_A), None);
        assert_eq!(zealous.own_cut(REFERENCE_A), None);
    }

    fn reference_transfer_script(args: &TransferArgs) -> Vec<u8> {
        let mut sig = args.encode().unwrap();
        sig.extend_from_slice(&kcc1::tag_push(&transfer_tag()));
        sig.extend_from_slice(&encode_push(REFERENCE_A));
        sig
    }

    /// The standard transfer of a KCC-1-tag build is read by the entry's
    /// role and types, field-major, on 79c71c23; the Zealous arms are read
    /// only by their own readers, and a standard-shaped script on a Zealous
    /// tag is not a standard transfer.
    #[test]
    fn the_standard_transfer_is_read_by_role_and_types() {
        let pin = pin_by("9703112e");
        let args = TransferArgs {
            next_states: vec![state(0x21), State { owner_scheme: OWNER_P2PKH_SCHNORR, ..state(0x22) }],
            witness: [&[0x00][..], &[0x44; 65]].concat(),
        };
        let sig = reference_transfer_script(&args);
        assert_eq!(parse_transfer_reveal(&sig, pin), Some(args.clone()));
        assert_eq!(parse_tag_reveal(&sig, pin), None, "a standard entry is not a program-specific arm");
        assert_eq!(parse_delegator_reveal(&sig, pin), None);
        let mut delegator = DelegatorArgs { witness: vec![0x44; 65] }.encode();
        delegator.extend_from_slice(&kcc1::tag_push(&delegator_tag()));
        delegator.extend_from_slice(&encode_push(REFERENCE_A));
        assert_eq!(parse_delegator_reveal(&delegator, pin), Some(DelegatorArgs { witness: vec![0x44; 65] }));
        assert_eq!(parse_transfer_reveal(&delegator, pin), None);
        // the same pushes on the Zealous transfer tag: no standard entry there
        let zealous = pin_by("2f2671fc");
        assert!(zealous.standard_entry("transfer").is_none());
        let mut on_zealous = args.encode().unwrap();
        on_zealous.extend_from_slice(&encode_push(&h("ddeb2d66")));
        on_zealous.extend_from_slice(&encode_push(&fixture_bytes("zealous_token_2641_a")));
        assert_eq!(parse_transfer_reveal(&on_zealous, zealous), None);
        // the silverc vector pin still reads its selector-dispatched transfer
        let vector = pin_by("4abcc188");
        let mut silverc = args.encode().unwrap();
        silverc.push(0x00); // selector 0: the empty push
        silverc.extend_from_slice(&encode_push(&fixture_bytes("vector_template_a")));
        assert_eq!(parse_transfer_reveal(&silverc, vector), Some(args));
    }

    /// The verdict the API serves, computed from the declared rules: the
    /// five Draft builds are not conformant, each for its own reasons; the
    /// reference build declares only the Final rules and its pin is
    /// reviewed, so it reads `true`, and would read `null` unreviewed.
    #[test]
    fn conformance_is_computed_from_the_declared_rules() {
        let vector = pin_by("4abcc188");
        assert_eq!(vector.conformant(), Some(false));
        let reasons = vector.nonconformities().join("; ");
        for why in ["ea5176aa", "numeric selector", "preimage rule", "blake2b"] {
            assert!(reasons.contains(why), "{why}: {reasons}");
        }
        assert!(!reasons.contains("P2PKHHash"), "the vector has no p2pkh owners to hash");
        for prefix in ["653012f1", "2f2671fc", "58c24260", "50ff50d8"] {
            let pin = pin_by(prefix);
            assert_eq!(pin.conformant(), Some(false));
            let reasons = pin.nonconformities().join("; ");
            assert!(reasons.contains("no standard transfer entrypoint dispatched by 79c71c23"), "{reasons}");
            assert!(reasons.contains("keyed P2PKHHash"), "{reasons}");
        }
        assert!(pin_by("653012f1").nonconformities().join(" ").contains("preimage rule"));
        assert!(!pin_by("2f2671fc").nonconformities().join(" ").contains("preimage rule"));

        let reference = pin_by("9703112e");
        assert!(reference.nonconformities().is_empty(), "{:?}", reference.nonconformities());
        assert_eq!(reference.conformant(), Some(true), "reviewed, and no declared rule departs");
        assert_eq!(reference.standing(), REVIEWED_BUILD_TEMPLATE, "the build's standing, never a token's");
        let mut unreviewed = reference.clone();
        unreviewed.review.status = "unreviewed".into();
        assert_eq!(unreviewed.conformant(), None, "unreviewed: no verdict either way");
        assert_eq!(unreviewed.standing(), LAYOUT_TEMPLATE);
        let reviewed = reference.clone();
        let mut keyed = reviewed.clone();
        keyed.rules.p2pkh_hash = kcc2::P2pkhHash::KeyedDraft;
        assert_eq!(keyed.conformant(), Some(false));
        let mut mixing = reviewed.clone();
        mixing.rules.single_class = false;
        assert_eq!(mixing.conformant(), Some(false), "#16 holds a build of the Final text to one class per transfer");
        // a reduced owner subset is configuration, not a departure
        let mut reduced = reviewed.clone();
        reduced.owner_schemes = vec![OWNER_P2PK_SCHNORR, OWNER_P2SH];
        assert_eq!(reduced.conformant(), Some(true));
        reduced.owner_schemes.push(0x80);
        assert_eq!(reduced.conformant(), Some(false));
        // an unpublished artifact leaves the verdict open and lists nothing:
        // the build stays reviewed, and a real departure still reads false
        let mut unpublished = reviewed;
        unpublished.artifact = Some(PinArtifact::Unpublished);
        assert_eq!(unpublished.conformant(), None);
        assert!(unpublished.nonconformities().is_empty());
        assert_eq!(unpublished.standing(), REVIEWED_BUILD_TEMPLATE);
        let mut departs = unpublished.clone();
        departs.rules.p2pkh_hash = kcc2::P2pkhHash::KeyedDraft;
        assert_eq!(departs.conformant(), Some(false));
        let mut open = unpublished;
        open.review.status = "unreviewed".into();
        assert_eq!((open.conformant(), open.standing()), (None, LAYOUT_TEMPLATE));
        // the committed pins: only KaspaRocket's records its artifact unpublished
        let unpublished: Vec<&str> = pins()
            .iter()
            .filter(|p| p.artifact == Some(PinArtifact::Unpublished))
            .map(|p| &p.template_hash[..8])
            .collect();
        assert_eq!(unpublished, ["ff3ae732"]);
        // the field takes one word, and nothing else parses
        let base = include_str!("../pins/kcc0020/ff3ae732.json");
        assert!(base.contains("\"artifact\": \"unpublished\","));
        let bad = base.replace("\"artifact\": \"unpublished\",", "\"artifact\": \"maybe\",");
        assert!(serde_json::from_str::<Pin>(&bad).is_err());
        let absent = base.replace("  \"artifact\": \"unpublished\",\n", "");
        let absent: Pin = serde_json::from_str(&absent).unwrap();
        assert_eq!((absent.artifact, absent.conformant()), (None, Some(true)), "the artifact is the only reason it has no verdict");
    }

    /// `fixtures_share_class` relaxes the two-sentinel discipline only where
    /// the chain has no two cells of a build in different classes, and the
    /// pin's note says which of the two reasons holds. KaspaRocket's build
    /// has many launches; its reason is the class every one of them carries.
    #[test]
    fn a_shared_fixture_class_names_its_reason() {
        let flagged: Vec<&Pin> = pins().iter().filter(|p| p.review.fixtures_share_class).collect();
        assert!(flagged.len() >= 2);
        for pin in flagged {
            // the sentence that names the flag gives its reason
            let note = &pin.review.note;
            let at = note.find("fixtures_share_class").expect("the note names the flag");
            let from = note[..at].rfind(". ").map_or(0, |i| i + 2);
            let to = note[at..].find(". ").map_or(note.len(), |i| at + i);
            let why = &note[from..to];
            assert!(
                why.contains("carries one launch of this build") || why.contains("carries the all-zero class"),
                "{}: the note says why the class cannot vary",
                pin.template_hash
            );
        }
        let kr = pin_by("ff3ae732");
        assert!(kr.review.fixtures_share_class);
        assert!(kr.review.note.contains("not one, and the pair shares its class because every cell of this build"));
        let at = kr.state.record_start();
        for fixture in &kr.review.fixtures {
            let state = State::decode(&fixture_bytes(fixture)[at..at + STATE_LEN]).unwrap();
            assert_eq!(state.extension_commitment, [0; 32], "{fixture}: the all-zero class");
        }
    }

    /// A familyless pin's stamped label is its standing, so whatever moves
    /// the standing moves the labels fingerprint the store's restamp gate
    /// carries; a venue family's label does not move with its review.
    #[test]
    fn a_moved_standing_moves_the_labels_fingerprint() {
        let base: Vec<Pin> = pins().to_vec();
        assert_eq!(labels_fingerprint(), labels_fingerprint_of(&base));
        let reference = base.iter().position(|p| p.template_hash == HASH_REFERENCE).unwrap();
        let zealous = base.iter().position(|p| p.template_hash.starts_with("2f2671fc")).unwrap();
        assert_eq!(base[reference].family_label(), REVIEWED_BUILD_TEMPLATE);
        let mut unreviewed = base.clone();
        unreviewed[reference].review.status = "unreviewed".into();
        assert_eq!(unreviewed[reference].family_label(), LAYOUT_TEMPLATE);
        assert_ne!(labels_fingerprint_of(&unreviewed), labels_fingerprint_of(&base));
        let mut drafted = base.clone();
        drafted[reference].spec_commit = DRAFT_SPEC_COMMIT.into();
        assert_ne!(labels_fingerprint_of(&drafted), labels_fingerprint_of(&base));
        let mut venue = base.clone();
        venue[zealous].review.status = "unreviewed".into();
        assert_eq!(labels_fingerprint_of(&venue), labels_fingerprint_of(&base), "a family names the build either way");
    }

    /// Every declaration that changes what a token derives as moves the
    /// fingerprint, so a fix to any of them re-derives stored tokens instead
    /// of shipping beside them.
    #[test]
    fn every_semantic_pin_field_moves_the_fingerprint() {
        assert_eq!(pins_fingerprint(), fingerprint_of(pins()));
        let base: Vec<Pin> = pins().to_vec();
        let reference = base.iter().position(|p| p.template_hash == HASH_REFERENCE).unwrap();
        let zealous = base.iter().position(|p| p.template_hash.starts_with("2f2671fc")).unwrap();
        type Mutation = Box<dyn Fn(&mut Vec<Pin>)>;
        let mutations: Vec<(&str, Mutation)> = vec![
            ("template hash", Box::new(move |p| p[zealous].template_hash.replace_range(..1, "0"))),
            ("review status", Box::new(move |p| p[reference].review.status = "unreviewed".into())),
            ("spec commit", Box::new(move |p| p[zealous].spec_commit = SPEC_COMMIT.into())),
            ("bounds", Box::new(move |p| p[zealous].max_token_inputs = 3)),
            ("owner schemes", Box::new(move |p| p[zealous].owner_schemes.truncate(3))),
            ("borrow schemes", Box::new(move |p| p[zealous].borrow_schemes.truncate(3))),
            ("hash-chain rule", Box::new(move |p| p[zealous].rules.hash_chain = HashChainRule::Preimage)),
            ("p2pkh hash", Box::new(move |p| p[zealous].rules.p2pkh_hash = kcc2::P2pkhHash::Unkeyed)),
            ("threshold flag", Box::new(move |p| p[zealous].rules.threshold_non_negative = false)),
            ("class rule", Box::new(move |p| p[zealous].rules.single_class = true)),
            ("window start", Box::new(move |p| p[reference].state.start = 2)),
            ("window length", Box::new(move |p| p[reference].state.len = 146)),
            ("record offset", Box::new(move |p| p[reference].state.record_offset = 34)),
            ("view hash", Box::new(move |p| p[reference].view_template_hash = None)),
            ("dispatch kind", Box::new(move |p| p[zealous].dispatch.kind = DispatchKind::SilvercSelector)),
            ("entry name", Box::new(move |p| p[zealous].entrypoints[0].name = "transfer".into())),
            ("entry role", Box::new(move |p| p[zealous].entrypoints[0].role = "transfer".into())),
            ("entry types", Box::new(move |p| p[zealous].entrypoints[0].types = STANDARD_TRANSFER_TYPES.into())),
            ("entry tag", Box::new(move |p| p[zealous].entrypoints[0].select = "79c71c23".into())),
            ("an issuer", Box::new(move |p| drop(p[reference].issuers.pop()))),
            ("issuer role", Box::new(move |p| p[reference].issuers[0].role = IssuerRole::Seed)),
            ("issuer hash", Box::new(move |p| p[reference].issuers[0].template_hash.replace_range(..1, "0"))),
            ("issuer window", Box::new(move |p| p[reference].issuers[0].state.len = 151)),
            ("issuer types", Box::new(move |p| p[reference].issuers[0].state_types.push_str(",int"))),
            ("issuer template field", Box::new(move |p| p[reference].issuers[0].fields.kcc20_template = 1)),
            ("issuer class field", Box::new(move |p| p[reference].issuers[0].fields.extension_commitment = 4)),
            ("issuer allowance field", Box::new(move |p| p[reference].issuers[0].fields.remaining = None)),
            ("issuer limit field", Box::new(move |p| p[reference].issuers[0].fields.per_issue_limit = Some(2))),
            ("issuer entry", Box::new(move |p| p[reference].issuers[0].entrypoints[0].select = "00000000".into())),
            ("issuer self-template field", Box::new(move |p| p[reference].issuers[1].fields.self_template = None)),
            ("genesis rebuild", Box::new(move |p| p[reference].genesis_rebuild = None)),
            (
                "genesis rebuild seeds",
                Box::new(move |p| p[reference].genesis_rebuild.as_mut().unwrap().seeds.clear()),
            ),
            (
                "genesis rebuild cells",
                Box::new(move |p| p[reference].genesis_rebuild.as_mut().unwrap().declared_cells = false),
            ),
            (
                "genesis rebuild on another pin",
                Box::new(move |p| {
                    p[zealous].genesis_rebuild = Some(GenesisRebuildDecl { seeds: Vec::new(), declared_cells: true })
                }),
            ),
        ];
        let before = fingerprint_of(&base);
        for (what, mutate) in mutations {
            let mut changed = base.clone();
            mutate(&mut changed);
            assert_ne!(fingerprint_of(&changed), before, "{what} does not reach the fingerprint");
        }
    }

    // ── issuers ──────────────────────────────────────────────────

    /// The canonical halves of the reference KCC20 program, as a minter's
    /// `mint` passes them.
    fn reference_halves() -> (Vec<u8>, Vec<u8>) {
        (REFERENCE_A[..1].to_vec(), REFERENCE_A[1 + 145..].to_vec())
    }

    fn issue_script(issuer_program: &[u8], tag: &str, issue: &Issue) -> Vec<u8> {
        let s = issue.recipient;
        let bytes = kcc1::Type::Array(Box::new(kcc1::Type::Byte), None);
        let record = kcc1::Value::Record(vec![
            kcc1::Value::Int(s.amount),
            kcc1::Value::Bytes(s.owner.to_vec()),
            kcc1::Value::Bytes(vec![s.owner_scheme]),
            kcc1::Value::Bytes(vec![s.borrow_scheme]),
            kcc1::Value::Bytes(s.borrow_guard.to_vec()),
            kcc1::Value::Bytes(s.extension_commitment.to_vec()),
        ]);
        let mut sig = kcc1::encode_args(&[
            (state_record_type(), record),
            (bytes.clone(), kcc1::Value::Bytes(issue.kcc20_prefix.clone())),
            (bytes, kcc1::Value::Bytes(issue.kcc20_suffix.clone())),
        ])
        .unwrap();
        sig.extend_from_slice(&encode_push(&h(tag)));
        sig.extend_from_slice(&encode_push(issuer_program));
        sig
    }

    fn out(program: &[u8], kas: u64) -> FamilyOutput {
        FamilyOutput { spk_version: 0, spk: kcc1::envelope_spk(program), kas }
    }

    /// The reference minter's fixture A, a chain reveal (testnet-10
    /// ec214f90..:0, family df170857, revealed as `mint` by 7fa12aa5..
    /// input 0): remaining 1,000,000, 1,000 per mint, class 77545d3d. A
    /// mint is proven end to end: the recipient cell is the pinned KCC20
    /// build carrying the declared state, and the minter continues as itself
    /// with the allowance reduced by the amount.
    #[test]
    fn a_reference_mint_proves_its_recipient_and_its_allowance() {
        let pin = pin_by("9703112e");
        let (_, minter, allowance) = pinned_issuer_state(MINTER_A).unwrap();
        assert_eq!(minter.role, IssuerRole::Mint);
        assert_eq!((allowance.remaining, allowance.per_issue_limit), (Some(1_000_000), Some(1_000)));
        let class = allowance.extension_commitment;
        assert_eq!(hex::encode(&class[..4]), "77545d3d");
        let (prefix, suffix) = reference_halves();
        let recipient = State { amount: 500, extension_commitment: class, ..state(0x31) };
        let issue = Issue { recipient, kcc20_prefix: prefix, kcc20_suffix: suffix };
        let sig = issue_script(MINTER_A, "0fe04b09", &issue);
        assert_eq!(parse_issue_reveal(&sig, minter), Some(issue.clone()));
        assert_eq!(parse_issue_reveal(&issue_script(MINTER_A, "52f03341", &issue), minter), None, "split is not an issue");

        // the expected continuation, spliced by hand: `remaining` is the
        // third window field, an 8-byte state int at bytes 68..76
        let mut continued = MINTER_A.to_vec();
        continued[68..76].copy_from_slice(&kcc1::encode_state_int(1_000_000 - 500).unwrap());
        let recipient_out = FamilyOutput {
            spk_version: 0,
            spk: pin.cut(REFERENCE_A).unwrap().reconstruct_spk(&recipient).unwrap(),
            kas: 1,
        };
        let verdict = check_issue(pin, minter, MINTER_A, 9, &issue, &out(&continued, 9), &recipient_out).unwrap();
        assert!(verdict.recipient_proven && verdict.issuer_successor_proven, "{:?}", verdict.violations);
        assert!(verdict.violations.is_empty(), "{:?}", verdict.violations);
        assert_eq!((verdict.remaining_before, verdict.remaining_after), (Some(1_000_000), Some(999_500)));
        assert_eq!((ISSUE_EVENT_KIND, ISSUER_AUTH), ("mint", "issuer"));
        // the proven continuation's bytes are kept, exactly the spliced program
        assert_eq!(verdict.issuer_successor_program.as_deref(), Some(continued.as_slice()));
        // the window's pubkey field, read by its declared type
        assert_eq!(minter.pubkey(&allowance).map(hex::encode).as_deref(), Some("83cab5f7e74f0f6ce0b87e959778e3aca9712b7df0661f6ede72b7ea2d5d7fd1"));

        // a minter that keeps its allowance, or drops KAS, is not continuing,
        // and no program is kept for an output that does not prove
        let kept = check_issue(pin, minter, MINTER_A, 9, &issue, &out(MINTER_A, 9), &recipient_out).unwrap();
        assert!(!kept.issuer_successor_proven);
        assert_eq!(kept.issuer_successor_program, None);
        let poorer = check_issue(pin, minter, MINTER_A, 9, &issue, &out(&continued, 8), &recipient_out).unwrap();
        assert!(!poorer.issuer_successor_proven);
        assert_eq!(poorer.issuer_successor_program, None);
        // over the per-mint limit, outside the class, or under foreign halves
        let big = Issue { recipient: State { amount: 1_001, ..recipient }, ..issue.clone() };
        let v = check_issue(pin, minter, MINTER_A, 9, &big, &out(&continued, 9), &recipient_out).unwrap();
        assert!(v.violations.iter().any(|v| v.contains("outside 0 < amount")), "{:?}", v.violations);
        let classless = Issue { recipient: State { extension_commitment: [0xEE; 32], ..recipient }, ..issue.clone() };
        let v = check_issue(pin, minter, MINTER_A, 9, &classless, &out(&continued, 9), &recipient_out).unwrap();
        assert!(v.violations.iter().any(|v| v.contains("extension commitment is not the issuer's")));
        let mut foreign_suffix = issue.kcc20_suffix.clone();
        foreign_suffix.push(0x51);
        let foreign = Issue { kcc20_suffix: foreign_suffix, ..issue.clone() };
        let v = check_issue(pin, minter, MINTER_A, 9, &foreign, &out(&continued, 9), &recipient_out).unwrap();
        assert!(!v.recipient_proven);
        assert!(v.violations.iter().any(|v| v.contains("other than the pinned build")));
        // the recipient output must be the declared state, nothing else
        let elsewhere = FamilyOutput { spk: kcc1::envelope_spk(&[0x51]), ..recipient_out.clone() };
        assert!(!check_issue(pin, minter, MINTER_A, 9, &issue, &out(&continued, 9), &elsewhere).unwrap().recipient_proven);
        // and a program that is not this issuer is not judged at all
        assert!(check_issue(pin, minter, SEED_A, 9, &issue, &out(&continued, 9), &recipient_out).is_none());
    }

    /// The seed creates a zero-amount receiving cell under its class and
    /// continues unchanged. Fixture A is a chain reveal (testnet-10
    /// ec214f90..:1, family df170857, revealed as `create` by 05060a38..
    /// input 0), class 77545d3d.
    #[test]
    fn a_reference_seed_creates_an_empty_cell() {
        let pin = pin_by("9703112e");
        let (_, seed, seeded) = pinned_issuer_state(SEED_A).unwrap();
        assert_eq!(seed.role, IssuerRole::Seed);
        assert_eq!(seeded.remaining, None);
        assert_eq!(hex::encode(&seeded.extension_commitment[..4]), "77545d3d");
        let (prefix, suffix) = reference_halves();
        let recipient = State { amount: 0, extension_commitment: seeded.extension_commitment, ..state(0x41) };
        let issue = Issue { recipient, kcc20_prefix: prefix, kcc20_suffix: suffix };
        assert_eq!(parse_issue_reveal(&issue_script(SEED_A, "d1f8dfa0", &issue), seed), Some(issue.clone()));
        let recipient_out = FamilyOutput {
            spk_version: 0,
            spk: pin.cut(REFERENCE_A).unwrap().reconstruct_spk(&recipient).unwrap(),
            kas: 1,
        };
        let verdict = check_issue(pin, seed, SEED_A, 3, &issue, &out(SEED_A, 3), &recipient_out).unwrap();
        assert!(verdict.violations.is_empty(), "{:?}", verdict.violations);
        assert!(verdict.recipient_proven && verdict.issuer_successor_proven);
        assert_eq!(verdict.issuer_successor_program.as_deref(), Some(SEED_A), "a seed continues byte for byte");
        assert_eq!(seed.pubkey(&seeded).map(hex::encode).as_deref(), Some("51907bebce18e14c88f4ad8d714c81d9581ae81aa0e00b27cb3e25a736d6e545"));
        assert_eq!(seed.issue_entry().map(|e| (e.name.as_str(), e.select.as_str())), Some(("create", "d1f8dfa0")));
        let one = Issue { recipient: State { amount: 1, ..recipient }, ..issue };
        let v = check_issue(pin, seed, SEED_A, 3, &one, &out(SEED_A, 3), &recipient_out).unwrap();
        assert!(v.violations.iter().any(|v| v.contains("zero-amount")));
    }

    // ── genesis rebuilds ─────────────────────────────────────────

    /// The chain bytes of the genesis rebuild's fixtures (see its `about`).
    fn genesis_chain() -> serde_json::Value {
        serde_json::from_str(include_str!("../fixtures/kcc0020/genesis_rebuild_chain.json")).unwrap()
    }

    fn hex_of(value: &serde_json::Value) -> Vec<u8> {
        hex::decode(value.as_str().unwrap()).unwrap()
    }

    fn bytes32(value: &serde_json::Value) -> [u8; 32] {
        hex_of(value).try_into().unwrap()
    }

    /// One declarer row of a scenario: (covenant, sig script, program).
    fn declarer_of(scenario: &str) -> ([u8; 32], Vec<u8>, Vec<u8>) {
        let chain = genesis_chain();
        let row = &chain["scenarios"][scenario]["declarer_rows"][0];
        let (spk, sig) = (hex_of(&row[4]), hex_of(&row[5]));
        let program = crate::p2sh_reveal(&spk, &sig).expect("the declarer's spend opens its commitment");
        (bytes32(&row[0]), sig, program)
    }

    /// The seed the reference build's genesis rebuild takes its bytes from:
    /// its halves recompute TokenSeed's hash, a seed rebuilt with fixture A's
    /// own fields is fixture A byte for byte, and both template fields are
    /// set, never read: a window that names another build or another self
    /// template is not what the rebuild produces.
    #[test]
    fn the_reference_seed_rebuilds_from_its_reviewed_bytes() {
        let pin = pin_by("9703112e");
        let seed = pin.issuers.iter().find(|i| i.name == "TokenSeed").unwrap();
        let (prefix, suffix) = seed.reviewed_halves().expect("reviewed halves");
        assert_eq!(hex::encode(kcc1::template_hash(prefix, suffix)), seed.template_hash);
        assert_eq!((prefix.len(), suffix.len()), (1, 1_355 - 133));
        let (_, _, state) = pinned_issuer_state(SEED_A).unwrap();
        let key = seed.pubkey(&state).unwrap();
        assert_eq!(seed.seed_program(pin, &key, &state.extension_commitment).as_deref(), Some(SEED_A));
        // the minter is no seed, and has no reviewed bytes here either
        let minter = pin.issuers.iter().find(|i| i.name == "PublicMint").unwrap();
        assert_eq!(minter.reviewed_halves(), None);
        assert_eq!(minter.seed_program(pin, &key, &state.extension_commitment), None);
        // the fields are set from the pin and the issuer, whatever a
        // planted window would say
        let mut other_build = pin.clone();
        other_build.template_hash.replace_range(..2, "00");
        let rebuilt = seed.seed_program(&other_build, &key, &state.extension_commitment).unwrap();
        assert_eq!(&rebuilt[2..34], &hex::decode(&other_build.template_hash).unwrap()[..]);
        let mut no_self = seed.clone();
        no_self.fields.self_template = None;
        assert_eq!(no_self.seed_program(pin, &key, &state.extension_commitment), None);
        let mut doubled = seed.clone();
        doubled.fields.self_template = Some(seed.fields.kcc20_template);
        assert_eq!(doubled.seed_program(pin, &key, &state.extension_commitment), None, "two declarations of one field");
        let mut moved = seed.clone();
        moved.template_hash.replace_range(..2, "00");
        assert_eq!(moved.reviewed_halves(), None, "bytes are found by the issuer's own hash");
    }

    /// Chain vectors: the never-spent mainnet seeds of 5cbdbc59 and 0644e2c4,
    /// rebuilt with the owner key and class their genesis's :0 cell revealed,
    /// envelope to the commitments their genesis outputs carry. Any other
    /// owner or class does not.
    #[test]
    fn a_genesis_seed_rebuilds_to_its_chain_commitment() {
        let pin = pin_by("9703112e");
        let seed = pin.issuers.iter().find(|i| i.name == "TokenSeed").unwrap();
        let chain = genesis_chain();
        let vectors = chain["seed_vectors"].as_array().unwrap();
        assert_eq!(vectors.len(), 2);
        for v in vectors {
            let (owner, class, spk) = (bytes32(&v["owner"]), bytes32(&v["class"]), hex_of(&v["seed_spk"]));
            let program = seed.seed_program(pin, &owner, &class).unwrap();
            assert_eq!(kcc1::envelope_spk(&program), spk, "{}", v["token"]);
            let (_, named, state) = pinned_issuer_state(&program).expect("the rebuild reads as the seed");
            assert_eq!(named.name, "TokenSeed");
            assert_eq!((seed.pubkey(&state), state.extension_commitment), (Some(owner), class));
            let mut other = owner;
            other[31] ^= 1;
            assert_ne!(kcc1::envelope_spk(&seed.seed_program(pin, &other, &class).unwrap()), spk);
            assert_ne!(kcc1::envelope_spk(&seed.seed_program(pin, &owner, &[1; 32]).unwrap()), spk);
        }
        let prefixes: Vec<String> = vectors.iter().map(|v| v["seed_spk"].as_str().unwrap()[4..12].to_string()).collect();
        assert_eq!(prefixes, vec!["007964ad", "6fa08466"]);
    }

    /// A Kasseal curve's `init` (testnet-10 4ff44075, PEBBLE's genesis)
    /// passes the reference build's halves as its first two arguments: it is
    /// read as declaring cells of the build, and returns its arguments. One
    /// flipped suffix byte, the halves swapped or apart, a non-minimal push of
    /// the suffix, more than 64 arguments, or a template that is not the
    /// pin's: not a declaration.
    #[test]
    fn a_curve_init_declares_the_reference_build() {
        let pin = pin_by("9703112e");
        let template = pin.cut(REFERENCE_A).unwrap();
        let (_, sig, program) = declarer_of("testnet_0b64d769");
        assert_eq!(program.len(), 8_008);
        let args = declares_build(&sig, pin, &template).expect("the init names the build");
        assert_eq!(args.len(), 3);
        assert_eq!((args[0].as_slice(), args[1].as_slice()), (&REFERENCE_A[..1], &REFERENCE_A[146..]));
        assert_eq!(hex::encode(&args[2]), "a651f278");
        // rebuild the script from its pushes, altered
        let script = |args: &[Vec<u8>]| {
            let mut out: Vec<u8> = args.iter().flat_map(|a| encode_push(a)).collect();
            out.extend(encode_push(&program));
            out
        };
        assert_eq!(script(&args), sig, "the chain script is its minimal pushes");
        let mut flipped = args.clone();
        flipped[1][100] ^= 1;
        assert_eq!(declares_build(&script(&flipped), pin, &template), None);
        let swapped = vec![args[1].clone(), args[0].clone(), args[2].clone()];
        assert_eq!(declares_build(&script(&swapped), pin, &template), None);
        let apart = vec![args[0].clone(), args[2].clone(), args[1].clone()];
        assert_eq!(declares_build(&script(&apart), pin, &template), None);
        // the suffix pushed with a four-byte length: the same bytes, not minimal
        let mut loose: Vec<u8> = encode_push(&args[0]);
        loose.push(0x4e);
        loose.extend_from_slice(&(args[1].len() as u32).to_le_bytes());
        loose.extend_from_slice(&args[1]);
        loose.extend(encode_push(&args[2]));
        loose.extend(encode_push(&program));
        assert_eq!(declares_build(&loose, pin, &template), None);
        let mut many = vec![vec![0x07]; DECLARER_ARG_CAP - 3];
        many.extend(args.iter().cloned());
        assert!(declares_build(&script(&many), pin, &template).is_some(), "64 arguments are read");
        many.insert(0, vec![0x07]);
        assert_eq!(declares_build(&script(&many), pin, &template), None, "65 are not");
        // a template that does not hash to the pin declares nothing
        let mut foreign = template.clone();
        foreign.suffix.push(0x51);
        let mut foreign_args = args.clone();
        foreign_args[1].push(0x51);
        assert_eq!(declares_build(&script(&foreign_args), pin, &foreign), None);
    }

    /// The numbers a program pushes, as the rebuild reads them: the curve
    /// pushes its 400,000,000 graduation reserve and never the 800,000,000
    /// inventory it computes; the Kasseal pool pushes its 999,000,000 LP
    /// supply. Past the cap nothing is returned; a truncated script reads as
    /// pushing nothing; a payload of 9 bytes or with the sign bit is no amount.
    #[test]
    fn pushed_amounts_reads_the_numbers_a_program_pushes() {
        let (_, _, curve) = declarer_of("testnet_0b64d769");
        let numbers = pushed_amounts(&curve, 512).unwrap();
        assert!(numbers.contains(&400_000_000) && !numbers.contains(&800_000_000));
        assert!(numbers.windows(2).all(|w| w[0] < w[1]), "distinct and ascending");
        let (pool_id, _, pool) = declarer_of("testnet_29ca9e63");
        assert_eq!(pool.len(), 15_197);
        assert!(pushed_amounts(&pool, 512).unwrap().contains(&999_000_000));
        assert_eq!(hex::encode(&pool_id[..4]), "9b1c4c84");
        let mut script = Vec::new();
        for n in 1..=513u32 {
            script.extend(encode_push(&(1_000 + n).to_le_bytes()));
            script.push(0x75);
        }
        assert_eq!(pushed_amounts(&script, 513).map(|v| v.len()), Some(513));
        assert_eq!(pushed_amounts(&script, 512), None, "past the cap: nothing");
        assert_eq!(pushed_amounts(&[0x04, 0x01, 0x02], 512), Some(Vec::new()), "truncated");
        assert_eq!(push_amount(&[]), None);
        assert_eq!(push_amount(&[0x00, 0x80]), None, "the sign bit");
        assert_eq!(push_amount(&[0x01; 9]), None, "nine bytes");
        assert_eq!(push_amount(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]), Some(i64::MAX));
        assert_eq!(push_amount(&[0x00, 0x84, 0xd7, 0x17]), Some(400_000_000));
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
        let Some(TagReveal::Borrowed { amount, preimage, key, signature, follower_change }) = parse_tag_reveal(&full, pin) else {
            panic!("not a borrowed receive");
        };
        assert_eq!((amount, preimage, key, signature, follower_change), (250, [0x5A; 32], None, [0x7B; 65], 150));
        // the 2,590 B build checks the Draft's chain: the witness is the bare preimage
        let reveal = parse_tag_reveal(&full, pin).unwrap();
        assert_eq!(reveal.borrow_witness(0x03, &pin.rules), Some(vec![0x5A; 32]));
        assert_eq!(reveal.borrow_witness(0x02, &pin.rules), Some(vec![0x7B; 65]));
        assert_eq!(reveal.borrow_witness(0x01, &pin.rules), Some(vec![]));
        assert_eq!(TagReveal::Merge.borrow_witness(0x03, &pin.rules), None);
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
                Some(TagReveal::Borrowed { amount: 250, preimage: [0x5A; 32], key: Some([0x6B; 32]), signature: [0x7B; 65], follower_change: 150 })
            );
            // these builds check the keyed chain (#18's rule, a generation
            // early): the witness is next ‖ key ‖ signature, structural
            assert_eq!(pin.rules.hash_chain, HashChainRule::Keyed);
            let witness = parse_tag_reveal(&full, pin).unwrap().borrow_witness(0x03, &pin.rules).unwrap();
            assert_eq!(KeyedLink::parse(&witness), Some(KeyedLink { next: [0x5A; 32], pubkey: [0x6B; 32], signature: [0x7B; 65] }));
            let leader = State { borrow_scheme: 0x03, borrow_guard: hash_chain_link(&[0x5A; 32], &[0x6B; 32]), ..state(0x77) };
            let grown = State { amount: leader.amount + 250, borrow_guard: [0x5A; 32], ..leader };
            assert!(matches!(verify_borrow(&pin.rules, &leader, &grown, &witness, 5, 5), BorrowVerdict::Structural(_)));
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
