//! KCC-23 MJ (Metadata JSON): reading and verifying the metadata a
//! deployer binds to a covenant through a preparation transaction.
//!
//! Follows "MJ: Metadata JSON" (kaspanet/kccs kcc-0023.md, Draft, merged at
//! 1516a6c). Section numbers in doc comments refer to that text. The
//! reference crate (kcc-0023/implementation) is mirrored step for step, with
//! one deliberate difference: the declaration is read by the I-JSON rules of
//! the §3.1 TEXT, which the reference `Codec` does not enforce. Where the two
//! disagree (duplicate names, noncharacters, nesting depth 65 to 127) this
//! module refuses and the reference accepts; each choice is marked at the
//! rule that makes it. Every official vector agrees either way.
//!
//! The hashing is rebuilt from the kaspanet/rusty-kaspa sources the vectors
//! cite (consensus/core/src/hashing/tx.rs and covenant_id.rs, hashers.rs),
//! because this crate takes no git dependencies: it is published on its own.
//! The conformance test reproduces each intermediate value the vectors pin
//! (payload digest, rest preimage, rest digest, txid, covenant id), so a
//! drift in any step fails there rather than in the field.
//!
//! What a verified binding proves, and what it does not: the covenant id
//! commits to the spent outpoint, and the outpoint's txid commits to the
//! payload, so the metadata is fixed for that covenant id (§3.2). It is NOT
//! unique to it. Any output of one preparation transaction can authorize a
//! genesis, and one outpoint can authorize several groups in one genesis,
//! all of which verify with the same declaration. A reader that shows a
//! declaration should count the other covenants bound to the same
//! preparation txid (the `shared_with` the API exposes) and keep calling the
//! fields claims, never identity. The checks also say nothing about either
//! transaction's validity or acceptance (§3.3).

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::cell::Cell;
use std::fmt;

/// The one codec byte KCC-23 defines (§3.1): a UTF-8 JSON object follows.
pub const CODEC_JSON: u8 = 0x00;

/// §3.1: object/array nesting may not exceed 64 levels, the root object
/// counting as depth 1.
pub const MAX_DEPTH: usize = 64;

/// A transaction outpoint: the id of the transaction that created an output
/// and the output's index in it. Ids are in the byte order kaspa hashes
/// them, which is also the order their hex is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outpoint {
    pub transaction_id: [u8; 32],
    pub index: u32,
}

/// The fields of a transaction input that a version 1 transaction id
/// covers. Signature scripts, sig-op counts and compute budgets are absent
/// on purpose: the id excludes them, so no verifier input depends on them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxInput {
    pub previous_outpoint: Outpoint,
    pub sequence: u64,
}

/// A KIP-20 covenant binding on an output: which input of the same
/// transaction authorized it, and the covenant id it claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CovenantBinding {
    pub authorizing_input: u16,
    pub covenant_id: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxOutput {
    pub value: u64,
    pub script_version: u16,
    pub script: Vec<u8>,
    pub covenant: Option<CovenantBinding>,
}

/// A transaction as its version 1 id sees it. Storage mass is absent for
/// the same reason as the input fields above: the id never commits to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tx {
    pub version: u16,
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub lock_time: u64,
    pub subnetwork_id: [u8; 20],
    pub gas: u64,
    pub payload: Vec<u8>,
}

/// BLAKE3 under a rusty-kaspa domain key: the ASCII name zero-padded to 32
/// bytes (`blake3_hasher!` in crypto/hashes/src/hashers.rs).
fn blake3_keyed(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[..domain.len()].copy_from_slice(domain);
    let mut hasher = blake3::Hasher::new_keyed(&key);
    for part in parts {
        hasher.update(part);
    }
    *hasher.finalize().as_bytes()
}

/// `PayloadDigest(payload)` (§3.3): BLAKE3 keyed with "PayloadDigest". The
/// node keeps the empty-payload value as a precomputed constant, which its
/// own test pins to this same hash of no bytes, so no special case here.
pub fn payload_digest(payload: &[u8]) -> [u8; 32] {
    blake3_keyed(b"PayloadDigest", &[payload])
}

/// The bytes `TransactionRest` hashes (§3.3, `rest_preimage`): the
/// transaction encoded with the payload, signature scripts and mass commit
/// excluded. An excluded variable-length field still writes its length, as
/// zero, so the encoding stays unambiguous. Defined for version 1 and up;
/// the per-output covenant flag is a version 1 field.
pub fn rest_preimage(tx: &Tx) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&tx.version.to_le_bytes());
    out.extend_from_slice(&(tx.inputs.len() as u64).to_le_bytes());
    for input in &tx.inputs {
        out.extend_from_slice(&input.previous_outpoint.transaction_id);
        out.extend_from_slice(&input.previous_outpoint.index.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&input.sequence.to_le_bytes());
    }
    out.extend_from_slice(&(tx.outputs.len() as u64).to_le_bytes());
    for output in &tx.outputs {
        out.extend_from_slice(&output.value.to_le_bytes());
        out.extend_from_slice(&output.script_version.to_le_bytes());
        out.extend_from_slice(&(output.script.len() as u64).to_le_bytes());
        out.extend_from_slice(&output.script);
        match &output.covenant {
            None => out.push(0),
            Some(binding) => {
                out.push(1);
                out.extend_from_slice(&binding.authorizing_input.to_le_bytes());
                out.extend_from_slice(&binding.covenant_id);
            }
        }
    }
    out.extend_from_slice(&tx.lock_time.to_le_bytes());
    out.extend_from_slice(&tx.subnetwork_id);
    out.extend_from_slice(&tx.gas.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    out
}

/// `rest_digest = TransactionRest(rest_preimage)` (§3.3).
pub fn rest_digest(tx: &Tx) -> [u8; 32] {
    blake3_keyed(b"TransactionRest", &[&rest_preimage(tx)])
}

/// `TransactionV1Id(payload_digest || rest_digest)` (§3.3). Takes the two
/// digests rather than a transaction so a reader holding only the payload
/// and a stored `rest_digest` can rebuild the id, as the spec intends.
pub fn txid_from_digests(payload_digest: &[u8; 32], rest_digest: &[u8; 32]) -> [u8; 32] {
    blake3_keyed(b"TransactionV1Id", &[payload_digest, rest_digest])
}

/// The version 1 transaction id, recomputed from the fields and payload.
pub fn txid_v1(tx: &Tx) -> [u8; 32] {
    txid_from_digests(&payload_digest(&tx.payload), &rest_digest(tx))
}

/// The KIP-20 covenant id: BLAKE2b keyed with "CovenantID" over the genesis
/// outpoint and each output of the group, by its index in the genesis
/// transaction (hashing/covenant_id.rs). The binding itself is left out, so
/// the id can be written into the outputs it names.
pub fn covenant_id<'a>(
    outpoint: Outpoint,
    group: impl ExactSizeIterator<Item = (u32, &'a TxOutput)>,
) -> [u8; 32] {
    let mut state = blake2b_simd::Params::new()
        .hash_length(32)
        .key(b"CovenantID")
        .to_state();
    state.update(&outpoint.transaction_id);
    state.update(&outpoint.index.to_le_bytes());
    state.update(&(group.len() as u64).to_le_bytes());
    for (index, output) in group {
        state.update(&index.to_le_bytes());
        state.update(&output.value.to_le_bytes());
        state.update(&output.script_version.to_le_bytes());
        state.update(&(output.script.len() as u64).to_le_bytes());
        state.update(&output.script);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(state.finalize().as_bytes());
    out
}

/// Why a payload is not a KCC-23 declaration (§3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclarationRefusal {
    /// No codec byte at all.
    Empty,
    /// A codec byte this KCC does not define; readers must reject it.
    UnknownCodec(u8),
    InvalidUtf8,
    ByteOrderMark,
    /// Not exactly one well-formed JSON value, or a number outside binary64;
    /// the parser's message says which.
    Json(String),
    NotAnObject,
    DuplicateName(String),
    Noncharacter(char),
    TooDeep,
}

impl fmt::Display for DeclarationRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeclarationRefusal::Empty => {
                f.write_str("the payload is empty, so it has no codec byte")
            }
            DeclarationRefusal::UnknownCodec(byte) => {
                write!(
                    f,
                    "codec byte {byte:#04x} is not defined; only 0x00 (JSON object) is"
                )
            }
            DeclarationRefusal::InvalidUtf8 => f.write_str("the JSON text is not valid UTF-8"),
            DeclarationRefusal::ByteOrderMark => {
                f.write_str("the JSON text starts with a byte-order mark")
            }
            DeclarationRefusal::Json(message) => write!(f, "not one JSON value: {message}"),
            DeclarationRefusal::NotAnObject => f.write_str("the JSON value is not an object"),
            DeclarationRefusal::DuplicateName(name) => {
                write!(f, "the member name {name:?} appears twice in one object")
            }
            DeclarationRefusal::Noncharacter(c) => {
                write!(
                    f,
                    "a string holds the Unicode noncharacter U+{:04X}",
                    u32::from(*c)
                )
            }
            DeclarationRefusal::TooDeep => {
                write!(f, "objects and arrays nest deeper than {MAX_DEPTH} levels")
            }
        }
    }
}

/// A §3.1 declaration: the exact JSON text after the codec byte, and the
/// object it parses to. Field names and meanings belong to ecosystem
/// conventions, not to KCC-23, so the object is kept whole.
#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    /// Byte for byte what the payload carries after the codec byte. The
    /// commitment covers these bytes, whitespace and member order included.
    pub json: String,
    pub object: Map<String, Value>,
}

/// The convention fields token deployers use, each read only when present
/// with the JSON type the convention gives it; anything else stays `None`
/// and remains in [`Declaration::object`] untouched. `ticker` (KRC-20 and
/// earlier kascov payloads) and `symbol` (the §4 example) are kept apart:
/// choosing between them is a display rule, not something the bytes say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conventions {
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub symbol: Option<String>,
    /// A non-negative integer; `8.0` or `"8"` is not read as one.
    pub decimals: Option<u64>,
    pub image: Option<String>,
    pub image_hash: Option<String>,
}

impl Declaration {
    pub fn conventions(&self) -> Conventions {
        let text = |key: &str| {
            self.object
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        Conventions {
            name: text("name"),
            ticker: text("ticker"),
            symbol: text("symbol"),
            decimals: self.object.get("decimals").and_then(Value::as_u64),
            image: text("image"),
            image_hash: text("image_hash"),
        }
    }
}

/// Read a preparation payload as a §3.1 declaration: codec byte 0x00, then
/// exactly one I-JSON object. The spec sets no size limit, so neither does
/// this; a caller storing declarations bounds what it accepts.
pub fn decode_declaration(payload: &[u8]) -> Result<Declaration, DeclarationRefusal> {
    let (&codec, rest) = payload.split_first().ok_or(DeclarationRefusal::Empty)?;
    if codec != CODEC_JSON {
        return Err(DeclarationRefusal::UnknownCodec(codec));
    }
    let json = std::str::from_utf8(rest).map_err(|_| DeclarationRefusal::InvalidUtf8)?;
    if json.starts_with('\u{feff}') {
        return Err(DeclarationRefusal::ByteOrderMark);
    }
    let breach = Cell::new(None);
    let mut parser = serde_json::Deserializer::from_str(json);
    let parsed = Strict {
        depth: 0,
        breach: &breach,
    }
    .deserialize(&mut parser)
    .and_then(|value| parser.end().map(|()| value));
    // An I-JSON breach surfaces from serde as a generic error; the typed
    // reason recorded at the breach is the one to report.
    let value = match (parsed, breach.take()) {
        (_, Some(refusal)) => return Err(refusal),
        (Err(error), None) => return Err(DeclarationRefusal::Json(error.to_string())),
        (Ok(value), None) => value,
    };
    match value {
        Value::Object(object) => Ok(Declaration {
            json: json.to_owned(),
            object,
        }),
        _ => Err(DeclarationRefusal::NotAnObject),
    }
}

/// Unicode noncharacters: U+FDD0..U+FDEF and the last two code points of
/// every plane. RFC 7493 §2.1 forbids them in I-JSON strings, escaped or
/// raw.
fn is_noncharacter(c: char) -> bool {
    let c = u32::from(c);
    (0xFDD0..=0xFDEF).contains(&c) || c & 0xFFFE == 0xFFFE
}

/// A serde visitor that builds the JSON value while applying the §3.1 rules
/// serde_json itself does not. serde_json already refuses lone surrogates,
/// numbers that overflow binary64 ("number out of range"), raw control
/// characters, trailing values and nesting past 128; everything below is
/// where the reference `Codec`, a plain `serde_json` object parse, is looser
/// than the text. `depth` is that of the container being read, 0 above the
/// root.
#[derive(Clone, Copy)]
struct Strict<'a> {
    depth: usize,
    breach: &'a Cell<Option<DeclarationRefusal>>,
}

impl Strict<'_> {
    fn refuse<E: de::Error>(&self, refusal: DeclarationRefusal) -> E {
        let message = refusal.to_string();
        self.breach.set(Some(refusal));
        E::custom(message)
    }

    /// The seed for a container's members. Stricter than the reference: it
    /// parses depth 65 to 127 (serde_json stops only at 128); the text caps
    /// nesting at 64.
    fn nested<E: de::Error>(self) -> Result<Self, E> {
        if self.depth == MAX_DEPTH {
            return Err(self.refuse(DeclarationRefusal::TooDeep));
        }
        Ok(Strict {
            depth: self.depth + 1,
            ..self
        })
    }

    /// Stricter than the reference: it keeps strings holding U+FFFF,
    /// U+FDD0 and the other noncharacters; I-JSON forbids them.
    fn checked<E: de::Error>(&self, s: &str) -> Result<(), E> {
        match s.chars().find(|&c| is_noncharacter(c)) {
            Some(c) => Err(self.refuse(DeclarationRefusal::Noncharacter(c))),
            None => Ok(()),
        }
    }
}

impl<'de> DeserializeSeed<'de> for Strict<'_> {
    type Value = Value;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict<'_> {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an I-JSON value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("a number is not a finite binary64 value"))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        self.checked(v)?;
        Ok(Value::String(v.to_owned()))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let member = self.nested()?;
        let mut items = Vec::new();
        while let Some(item) = seq.next_element_seed(member)? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let member = self.nested()?;
        let mut object = Map::new();
        while let Some(name) = map.next_key::<String>()? {
            self.checked(&name)?;
            // Stricter than the reference: serde_json's map keeps the last
            // of two equal names, so the reference reads `{"name":"A",
            // "name":"B"}` as B while other parsers read A. I-JSON makes
            // names unique, which removes that ambiguity.
            if object.contains_key(&name) {
                return Err(self.refuse(DeclarationRefusal::DuplicateName(name)));
            }
            let value = map.next_value_seed(member)?;
            object.insert(name, value);
        }
        Ok(Value::Object(object))
    }
}

/// Why a preparation transaction does not bind its declaration to the
/// expected covenant id (§3.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// §3.1 and §3.2 require a version 1 preparation transaction.
    PreparationNotVersion1(u16),
    /// Covenant bindings exist only from version 1, as in the reference.
    GenesisBeforeVersion1(u16),
    Declaration(DeclarationRefusal),
    /// No genesis input spends an existing output of the preparation
    /// transaction under its recomputed id. A changed payload lands here:
    /// it changes the id, so the genesis no longer spends it.
    NotSpentAtGenesis,
    /// An input spends a preparation output, but no output group it
    /// authorizes declares the expected covenant id and hashes to it.
    CovenantIdMismatch,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::PreparationNotVersion1(v) => {
                write!(f, "the preparation transaction is version {v}, not 1")
            }
            Refusal::GenesisBeforeVersion1(v) => {
                write!(f, "the genesis transaction is version {v}, before covenant bindings")
            }
            Refusal::Declaration(refusal) => write!(f, "invalid declaration: {refusal}"),
            Refusal::NotSpentAtGenesis => f.write_str(
                "the genesis spends no existing output of the preparation transaction's recomputed id",
            ),
            Refusal::CovenantIdMismatch => f.write_str(
                "no genesis output group authorized by the spent outpoint hashes to the expected covenant id",
            ),
        }
    }
}

/// A verified binding: the declaration, and where it was bound.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub declaration: Declaration,
    pub preparation_txid: [u8; 32],
    /// The preparation output the genesis spent. Any output can serve, so
    /// this is part of the proof, not a fixed index.
    pub output_index: u32,
    pub authorizing_input: u16,
    /// The genesis outputs of the group, by index, in transaction order.
    pub group: Vec<u32>,
}

/// Everything §3.3 reads of a preparation transaction, without the rest of
/// it: the version, the payload, the digest of every other field the id
/// covers, and how many outputs a genesis may spend. A store that keeps
/// candidates for days keeps this (the payload plus 38 bytes) instead of the
/// whole transaction, and [`txid_from_digests`] gives the id back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preparation {
    pub version: u16,
    pub payload: Vec<u8>,
    pub rest_digest: [u8; 32],
    pub output_count: u32,
}

impl Preparation {
    pub fn of(tx: &Tx) -> Preparation {
        Preparation {
            version: tx.version,
            payload: tx.payload.clone(),
            rest_digest: rest_digest(tx),
            output_count: u32::try_from(tx.outputs.len()).unwrap_or(u32::MAX),
        }
    }

    /// The preparation's version 1 transaction id.
    pub fn txid(&self) -> [u8; 32] {
        txid_from_digests(&payload_digest(&self.payload), &self.rest_digest)
    }
}

/// §3.3: check that `preparation` binds its declaration to `expected` via
/// `genesis`. The steps follow the reference `Verifier` exactly; the first
/// genesis input that spends a preparation output and authorizes a group
/// hashing to `expected` is the binding returned.
pub fn verify(preparation: &Tx, genesis: &Tx, expected: &[u8; 32]) -> Result<Binding, Refusal> {
    verify_prepared(&Preparation::of(preparation), genesis, expected)
}

/// [`verify`] over a preparation kept in its compact form.
pub fn verify_prepared(
    preparation: &Preparation,
    genesis: &Tx,
    expected: &[u8; 32],
) -> Result<Binding, Refusal> {
    if preparation.version != 1 {
        return Err(Refusal::PreparationNotVersion1(preparation.version));
    }
    if genesis.version < 1 {
        return Err(Refusal::GenesisBeforeVersion1(genesis.version));
    }
    let declaration = decode_declaration(&preparation.payload).map_err(Refusal::Declaration)?;
    let preparation_txid = preparation.txid();
    let mut spent = false;
    for (input_index, input) in genesis.inputs.iter().enumerate() {
        let outpoint = input.previous_outpoint;
        if outpoint.transaction_id != preparation_txid || outpoint.index >= preparation.output_count
        {
            continue;
        }
        spent = true;
        // A binding names its authorizing input as a u16, so an input past
        // that range cannot authorize anything.
        let Ok(authorizing_input) = u16::try_from(input_index) else {
            continue;
        };
        // KIP-20 groups by (authorizing input, covenant id): one outpoint
        // may authorize several groups with different ids in one genesis.
        let group: Vec<(u32, &TxOutput)> = genesis
            .outputs
            .iter()
            .enumerate()
            .filter(|(_, output)| {
                output.covenant.is_some_and(|binding| {
                    binding.authorizing_input == authorizing_input
                        && binding.covenant_id == *expected
                })
            })
            .map(|(index, output)| (index as u32, output))
            .collect();
        if !group.is_empty() && covenant_id(outpoint, group.iter().copied()) == *expected {
            return Ok(Binding {
                declaration,
                preparation_txid,
                output_index: outpoint.index,
                authorizing_input,
                group: group.into_iter().map(|(index, _)| index).collect(),
            });
        }
    }
    Err(if spent {
        Refusal::CovenantIdMismatch
    } else {
        Refusal::NotSpentAtGenesis
    })
}
