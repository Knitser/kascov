//! KCC-2 authority schemes: the canonical identifiers, encodings and
//! minimum approval checks for authorities named in KCC-1 Program ABIs
//! (kaspanet/kccs "Authority Schemes", tracking the Final text at
//! [`SPEC_REVISION`], unchanged since the Last Call commit 1516a6c). Section
//! numbers in doc comments refer to that text.
//!
//! KCC-2 deliberately stops at the minimum: which key, script or lineage
//! must take part. The signed message, the witness layout and every further
//! condition belong to the consuming convention (KCC-20 is one), so nothing
//! here verifies a signature. Callers pass the verdict of whoever did, which
//! for a reader of confirmed transactions is consensus.
//!
//! One definition moved under kascov's feet: the merged Draft (ea5176aa)
//! committed P2PKH keys with BLAKE3 keyed by `"PublicKeyHash"`, and PR #30
//! made it plain unkeyed BLAKE3. Programs compiled against the Draft are
//! already on chain and check the keyed form forever, so the hash is a
//! per-program choice ([`P2pkhHash`]) rather than a global switch.

use std::fmt;

use serde::Deserialize;

use crate::{encode_push, kcc1};

/// The kccs revision whose KCC-2 text this module implements: Final
/// (1ed87ae, 2026-10-21), its text unchanged since the Last Call commit
/// 1516a6c, which this constant named before.
pub const SPEC_REVISION: &str = "1ed87ae";

/// The key the merged Draft (ea5176aa) gave KCC-2's `P2PKHHash`; PR #30
/// removed it. Kept only so Draft-built programs can still be read.
pub const DRAFT_P2PKH_KEY: &[u8] = b"PublicKeyHash";

/// Every authority value KCC-2 assigns is exactly 32 bytes (§2.1).
pub const VALUE_LEN: usize = 32;

/// Script public key version of the KCC-1 P2SH envelope (§2.3.1). A
/// matching script under any other version is a different output.
pub const P2SH_VERSION: u16 = 0;

/// An assigned standard scheme (§2.1). The discriminant is the canonical
/// registry byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Scheme {
    /// `p2pk-schnorr/v1`: the value is the 32-byte Schnorr public key itself.
    P2pkSchnorr = 0x00,
    /// `p2pkh-schnorr/v1`: the value is `Hash(pubkey)` of a 32-byte key.
    P2pkhSchnorr = 0x01,
    /// `p2pkh-ecdsa/v1`: the value is `Hash(pubkey)` of a compressed 33-byte
    /// ECDSA key.
    P2pkhEcdsa = 0x02,
    /// `p2sh/v1`: the value is the KCC-1 §7 commitment to one exact redeem
    /// script.
    P2sh = 0x03,
    /// `covenant-id/v1`: the value is a KIP-20 Covenant ID, one continuing
    /// lineage whose program and state may change.
    CovenantId = 0x04,
}

/// The KCC-1 type an authority value has under its scheme (§2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    Pubkey,
    Byte32,
}

impl ValueType {
    /// The type as KCC-1 spells it in an ABI: `pubkey` or `byte[32]`.
    pub fn abi_name(self) -> &'static str {
        match self {
            ValueType::Pubkey => "pubkey",
            ValueType::Byte32 => "byte[32]",
        }
    }
}

impl Scheme {
    /// The five assigned schemes in registry order.
    pub const ALL: [Scheme; 5] = [
        Scheme::P2pkSchnorr,
        Scheme::P2pkhSchnorr,
        Scheme::P2pkhEcdsa,
        Scheme::P2sh,
        Scheme::CovenantId,
    ];

    /// The canonical registry byte.
    pub fn byte(self) -> u8 {
        self as u8
    }

    /// The canonical textual name with its version, e.g. `p2pk-schnorr/v1`.
    pub fn name(self) -> &'static str {
        match self {
            Scheme::P2pkSchnorr => "p2pk-schnorr/v1",
            Scheme::P2pkhSchnorr => "p2pkh-schnorr/v1",
            Scheme::P2pkhEcdsa => "p2pkh-ecdsa/v1",
            Scheme::P2sh => "p2sh/v1",
            Scheme::CovenantId => "covenant-id/v1",
        }
    }

    pub fn value_type(self) -> ValueType {
        match self {
            Scheme::P2pkSchnorr => ValueType::Pubkey,
            _ => ValueType::Byte32,
        }
    }

    /// The scheme a registry byte names. A reserved byte and a custom byte
    /// are refused for different reasons: the first is one KCC-2 says every
    /// implementation MUST reject (§2.1), the second may be perfectly valid
    /// under a convention that defines it, which KCC-2 alone cannot read.
    pub fn from_byte(byte: u8) -> Result<Scheme, Refusal> {
        match byte {
            0x00 => Ok(Scheme::P2pkSchnorr),
            0x01 => Ok(Scheme::P2pkhSchnorr),
            0x02 => Ok(Scheme::P2pkhEcdsa),
            0x03 => Ok(Scheme::P2sh),
            0x04 => Ok(Scheme::CovenantId),
            b if is_custom(b) => Err(Refusal::CustomScheme(b)),
            b => Err(Refusal::ReservedScheme(b)),
        }
    }

    /// Inverse of [`Scheme::name`]; exact match, version included.
    pub fn from_name(name: &str) -> Option<Scheme> {
        Scheme::ALL.into_iter().find(|s| s.name() == name)
    }
}

/// `0x00`-`0x04`: a scheme KCC-2 assigns.
pub fn is_assigned(byte: u8) -> bool {
    byte <= Scheme::CovenantId.byte()
}

/// `0x05`-`0x7f`: held for future standard schemes, which only a Final KCC
/// can assign. Implementations MUST reject these and MUST NOT give them a
/// local meaning (§2.1).
pub fn is_reserved(byte: u8) -> bool {
    !is_assigned(byte) && !is_custom(byte)
}

/// `0x80`-`0xff`: custom schemes defined outside KCC-2. The same byte may
/// mean different schemes in different conventions (§3), so its meaning
/// comes only from the convention that declares it.
pub fn is_custom(byte: u8) -> bool {
    byte >= 0x80
}

/// Which P2PKH commitment a program checks. Per program, because a program
/// keeps the rule it was compiled with: a pin chooses, and a pin that says
/// nothing gets the Final rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum P2pkhHash {
    /// `Hash(pubkey)`, unkeyed BLAKE3 (§2.2, since PR #30).
    #[default]
    Unkeyed,
    /// `Hash(pubkey, "PublicKeyHash")`, BLAKE3 keyed per KCC-1 `Key32`: the
    /// merged Draft's `P2PKHHash` (kccs ea5176aa), which programs compiled
    /// against that Draft (the ZealousSwap TN10 builds among them) verify.
    KeyedDraft,
}

impl P2pkhHash {
    pub fn hash(self, pubkey: &[u8]) -> [u8; 32] {
        match self {
            P2pkhHash::Unkeyed => kcc1::hash(pubkey),
            P2pkhHash::KeyedDraft => {
                kcc1::hash_keyed(pubkey, DRAFT_P2PKH_KEY).expect("a 13-byte key fits Key32")
            }
        }
    }
}

/// Why an authority was not approved, or a scheme byte not accepted. Each
/// reason is distinct so a reader can say which rule failed, not just that
/// one did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A byte in `0x05`-`0x7f` (§2.1: MUST reject).
    ReservedScheme(u8),
    /// A byte in `0x80`-`0xff` that no convention in hand defines.
    CustomScheme(u8),
    /// The authority value is not the scheme's exact 32-byte payload.
    ValueLength { scheme: Scheme, len: usize },
    /// The revealed key has the wrong length for the scheme: 32 for Schnorr,
    /// 33 for compressed ECDSA (§2.2).
    PublicKeyLength { scheme: Scheme, len: usize },
    /// A 33-byte ECDSA key whose first byte is not the compressed `02`/`03`
    /// prefix (§2.2: MUST use the compressed encoding).
    EcdsaKeyNotCompressed(u8),
    /// p2pk: the key is not the authority value. p2pkh: the key does not
    /// hash to it.
    WrongKey,
    /// The key is the right one but its signature did not verify.
    InvalidSignature,
    /// The evidence offered is not the kind the scheme is approved by, e.g.
    /// a signature for a p2sh authority.
    WrongEvidence(Scheme),
    /// A convention-supplied input index past the last input (§2.3.1).
    InputIndexOutOfRange { index: usize, inputs: usize },
    /// The indexed input's script public key is not the version-0 envelope
    /// for the authority, version included (§2.3.1, §6).
    InputNotAuthority { index: usize },
    /// No spent input carries the P2SH envelope for the authority.
    NoP2shInput,
    /// No spent input belongs to the authority's covenant lineage.
    NoCovenantInput,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::ReservedScheme(b) => write!(
                f,
                "authority scheme 0x{b:02x} is reserved for a future standard scheme; KCC-2 says it must be rejected"
            ),
            Refusal::CustomScheme(b) => write!(
                f,
                "authority scheme 0x{b:02x} is a custom scheme defined outside KCC-2, and no definition of it is in hand"
            ),
            Refusal::ValueLength { scheme, len } => write!(
                f,
                "a {} authority value is {VALUE_LEN} bytes, not {len}",
                scheme.name()
            ),
            Refusal::PublicKeyLength { scheme, len } => write!(
                f,
                "a {} public key is {} bytes, not {len}",
                scheme.name(),
                if *scheme == Scheme::P2pkhEcdsa { 33 } else { 32 }
            ),
            Refusal::EcdsaKeyNotCompressed(prefix) => write!(
                f,
                "the ECDSA public key starts with 0x{prefix:02x}, not the compressed 02 or 03"
            ),
            Refusal::WrongKey => f.write_str("the public key is not the one the authority names"),
            Refusal::InvalidSignature => f.write_str("the signature by the authority's key is not valid"),
            Refusal::WrongEvidence(scheme) => {
                write!(f, "that evidence cannot approve a {} authority", scheme.name())
            }
            Refusal::InputIndexOutOfRange { index, inputs } => write!(
                f,
                "authority input {index} does not exist; the transaction spends {inputs}"
            ),
            Refusal::InputNotAuthority { index } => write!(
                f,
                "input {index} is not the version-0 P2SH output the authority commits to"
            ),
            Refusal::NoP2shInput => {
                f.write_str("no spent input is the P2SH output the authority commits to")
            }
            Refusal::NoCovenantInput => {
                f.write_str("no spent input belongs to the authority's covenant lineage")
            }
        }
    }
}

/// The authority value as a `byte[32]`, refusing any other length: the
/// exact-length rule (§2.1) is what stops one payload being read under a
/// verification rule it was not meant for (§6).
pub fn authority_value(scheme: Scheme, value: &[u8]) -> Result<[u8; 32], Refusal> {
    value
        .try_into()
        .map_err(|_| Refusal::ValueLength { scheme, len: value.len() })
}

/// The value as an invocation argument: KCC-1 `PushMinimal`. The scheme
/// byte is not part of it (§4.1); KCC-2 defines no combined serialization.
pub fn argument_encoding(value: &[u8; 32]) -> Vec<u8> {
    encode_push(value)
}

/// The value as a state field: KCC-1 `PushExplicit`.
pub fn state_encoding(value: &[u8; 32]) -> Vec<u8> {
    kcc1::push_explicit(value)
}

/// The p2pkh authority value for `pubkey`, after the key-form rules of
/// §2.2: exactly 32 bytes for Schnorr; 33 bytes with a `02`/`03` prefix for
/// ECDSA, so one key never has two commitments. `WrongEvidence` for a
/// scheme that is not P2PKH.
pub fn p2pkh_authority(scheme: Scheme, kind: P2pkhHash, pubkey: &[u8]) -> Result<[u8; 32], Refusal> {
    match scheme {
        Scheme::P2pkhSchnorr if pubkey.len() != 32 => {
            Err(Refusal::PublicKeyLength { scheme, len: pubkey.len() })
        }
        Scheme::P2pkhEcdsa if pubkey.len() != 33 => {
            Err(Refusal::PublicKeyLength { scheme, len: pubkey.len() })
        }
        Scheme::P2pkhEcdsa if !matches!(pubkey[0], 0x02 | 0x03) => {
            Err(Refusal::EcdsaKeyNotCompressed(pubkey[0]))
        }
        Scheme::P2pkhSchnorr | Scheme::P2pkhEcdsa => Ok(kind.hash(pubkey)),
        other => Err(Refusal::WrongEvidence(other)),
    }
}

/// The p2sh authority value for redeem script `redeem`: the KCC-1 §7
/// commitment, i.e. the 32 bytes its P2SH envelope carries.
pub fn p2sh_authority(redeem: &[u8]) -> [u8; 32] {
    kcc1::envelope_spk(redeem)[2..34]
        .try_into()
        .expect("the envelope carries a 32-byte hash")
}

/// The script of the version-0 P2SH output an input must spend to approve
/// for `authority`: `OP_BLAKE2B OP_DATA_32 authority OP_EQUAL`.
pub fn p2sh_script(authority: &[u8; 32]) -> Vec<u8> {
    [&[0xaa, 0x20][..], authority, &[0x87]].concat()
}

/// A spent input's authenticated script public key. The version is part of
/// the comparison: a matching script under another version is not the
/// envelope (§6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptPublicKey<'a> {
    pub version: u16,
    pub script: &'a [u8],
}

impl ScriptPublicKey<'_> {
    /// Whether this is exactly the P2SH envelope for `authority`.
    pub fn is_p2sh_of(&self, authority: &[u8; 32]) -> bool {
        self.version == P2SH_VERSION && self.script == p2sh_script(authority).as_slice()
    }
}

/// What a transaction offers as approval. Every field must come from
/// authenticated data: a signature verdict from verification under the
/// convention's message, inputs from the spent UTXOs consensus validated.
/// There is no output variant on purpose: a matching output alone
/// guarantees nothing (§6).
#[derive(Clone, Copy, Debug)]
pub enum Evidence<'a> {
    /// A signature scheme: the key the signature was verified under, and
    /// the verification result.
    Signature { public_key: &'a [u8], signature_valid: bool },
    /// p2sh: the spent inputs' script public keys. `index` is the authority
    /// input when the convention's witness names one; without it any input
    /// may match.
    P2shInputs { inputs: &'a [ScriptPublicKey<'a>], index: Option<usize> },
    /// covenant-id: the spent inputs' authenticated Covenant IDs, `None` for
    /// an input without a covenant binding. The active input counts: it may
    /// itself be the authority (§2.3.2).
    CovenantInputs { input_covenant_ids: &'a [Option<[u8; 32]>] },
}

/// The KCC-2 minimum approval check (§2.2, §2.3) for `authority` under
/// `scheme`. `Ok` means only that the minimum holds: a participating input
/// may still be anyone-can-spend or a permissive branch, and the consuming
/// convention adds whatever its own rules require (§6). `p2pkh` matters
/// only to the two P2PKH schemes.
pub fn approve(
    scheme: Scheme,
    authority: &[u8],
    evidence: &Evidence<'_>,
    p2pkh: P2pkhHash,
) -> Result<(), Refusal> {
    let authority = authority_value(scheme, authority)?;
    match (scheme, *evidence) {
        (_, Evidence::Signature { public_key, signature_valid }) => {
            bind_public_key(scheme, &authority, public_key, p2pkh)?;
            if signature_valid {
                Ok(())
            } else {
                Err(Refusal::InvalidSignature)
            }
        }
        (Scheme::P2sh, Evidence::P2shInputs { inputs, index: Some(index) }) => {
            let spk = inputs
                .get(index)
                .ok_or(Refusal::InputIndexOutOfRange { index, inputs: inputs.len() })?;
            if spk.is_p2sh_of(&authority) {
                Ok(())
            } else {
                Err(Refusal::InputNotAuthority { index })
            }
        }
        (Scheme::P2sh, Evidence::P2shInputs { inputs, index: None }) => {
            if inputs.iter().any(|spk| spk.is_p2sh_of(&authority)) {
                Ok(())
            } else {
                Err(Refusal::NoP2shInput)
            }
        }
        (Scheme::CovenantId, Evidence::CovenantInputs { input_covenant_ids }) => {
            if input_covenant_ids.contains(&Some(authority)) {
                Ok(())
            } else {
                Err(Refusal::NoCovenantInput)
            }
        }
        (scheme, _) => Err(Refusal::WrongEvidence(scheme)),
    }
}

/// The key half of a signature scheme's approval: `public_key` is the key
/// `authority` names (p2pk), or a well-formed key that hashes to it under
/// `p2pkh` (§2.2). For a reader that leaves signature verification to
/// consensus, this is everything KCC-2 adds on top. `WrongEvidence` for a
/// script or covenant scheme, which no key can approve.
pub fn bind_public_key(
    scheme: Scheme,
    authority: &[u8; 32],
    public_key: &[u8],
    p2pkh: P2pkhHash,
) -> Result<(), Refusal> {
    let named = match scheme {
        Scheme::P2pkSchnorr if public_key.len() != 32 => {
            return Err(Refusal::PublicKeyLength { scheme, len: public_key.len() })
        }
        Scheme::P2pkSchnorr => public_key == authority,
        Scheme::P2pkhSchnorr | Scheme::P2pkhEcdsa => {
            p2pkh_authority(scheme, p2pkh, public_key)? == *authority
        }
        Scheme::P2sh | Scheme::CovenantId => return Err(Refusal::WrongEvidence(scheme)),
    };
    if named {
        Ok(())
    } else {
        Err(Refusal::WrongKey)
    }
}
