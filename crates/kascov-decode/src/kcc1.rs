//! KCC-0001 conformance primitives — byte layouts, the Value ABI and hash
//! derivations from "Covenant Concepts, Byte Layouts, and ABI" (kaspanet/kccs
//! kcc-0001.md), tracking the Last Call text at commit 1516a6c. Section
//! numbers in doc comments refer to that text. The Draft kascov first
//! followed (ea5176aa) numbered the same rules differently (its §5.x Value
//! ABI is §3.4.x here, §6.1 is §3.5.1, §7 is §3.6, §8.x is §3.7.x, §10.1 is
//! §3.9.1, §11 is §6), and no encoding or hash changed between the two, so
//! nothing derived under the Draft needs recomputing. The spec's 103
//! machine-readable vectors run against this module in
//! `tests/kcc1_vectors.rs`. Invocation arguments use PushMinimal, which is
//! `encode_push` in the crate root; only the KCC1-specific encodings live
//! here.
//!
//! Two hash functions, and the split is the spec's, not ours (§3.2.1, §3.6):
//! `Hash` is BLAKE3-256 and names things (dispatch tags, template hashes,
//! virtual-element commitments, and the keyed form later specs key); the
//! P2SH envelope alone uses BLAKE2b, because that is what `OP_BLAKE2B`
//! computes on chain.
//! The pre-merge draft (55b28d8) used BLAKE2b for both; every identity this
//! module produced under it is a different number now. The store keeps those
//! old names resolving as aliases so published links keep working, but the
//! canonical name of a template is the BLAKE3 one.

use std::collections::BTreeMap;

use crate::encode_push;

/// Unkeyed BLAKE3 with 32-byte output — the spec's `Hash(x)` (§3.2.1).
pub fn hash(input: &[u8]) -> [u8; 32] {
    *blake3::hash(input).as_bytes()
}

fn hash32(input: &[u8]) -> [u8; 32] {
    hash(input)
}

/// Keyed `Hash(x, key)` (§3.2.1): BLAKE3 keyed mode under
/// `Key32(key) = key || 00^(32 - len(key))`. `None` for a key longer than
/// 32 bytes, which the spec does not define.
pub fn hash_keyed(input: &[u8], key: &[u8]) -> Option<[u8; 32]> {
    if key.len() > 32 {
        return None;
    }
    let mut key32 = [0u8; 32];
    key32[..key.len()].copy_from_slice(key);
    let mut hasher = blake3::Hasher::new_keyed(&key32);
    hasher.update(input);
    Some(*hasher.finalize().as_bytes())
}

/// KCC-2 §3 `P2PKHHash(pubkey) = Hash(pubkey, "PublicKeyHash")`: the owner
/// bytes a p2pkh-schnorr / p2pkh-ecdsa KCC20 cell carries. The preimage is
/// only learned when the owner spends, which is why a passive reader cannot
/// turn such an owner into an address on its own.
/// This keyed form is the KCC-2 text of the Draft at ea5176aa.
pub fn p2pkh_hash(pubkey: &[u8]) -> [u8; 32] {
    hash_keyed(pubkey, b"PublicKeyHash").expect("13-byte key")
}

/// `PushExplicit(b)` (§3.4.2): `OP_0` for the empty payload; the length-based
/// forms (`OP_DATA_n` / `OP_PUSHDATA1/2/4`) for every non-empty payload —
/// never the numeric opcodes `OP_1..OP_16` / `OP_1NEGATE`.
pub fn push_explicit(payload: &[u8]) -> Vec<u8> {
    match payload.len() {
        0 => vec![0x00],
        n @ 1..=75 => {
            let mut out = Vec::with_capacity(n + 1);
            out.push(n as u8);
            out.extend_from_slice(payload);
            out
        }
        n @ 76..=0xff => {
            let mut out = vec![0x4c, n as u8];
            out.extend_from_slice(payload);
            out
        }
        n @ 0x100..=0xffff => {
            let mut out = vec![0x4d, (n & 0xff) as u8, (n >> 8) as u8];
            out.extend_from_slice(payload);
            out
        }
        n => {
            let n = n as u32;
            let mut out = vec![
                0x4e,
                (n & 0xff) as u8,
                (n >> 8 & 0xff) as u8,
                (n >> 16 & 0xff) as u8,
                (n >> 24) as u8,
            ];
            out.extend_from_slice(payload);
            out
        }
    }
}

/// Decode exactly one `PushExplicit` at the start of `script` (§3.4.2), giving
/// the payload and the bytes consumed. §3.7.1 requires the consumed bytes to
/// equal `PushExplicit(payload)` byte-for-byte, so numeric opcodes,
/// non-canonical length forms (`OP_PUSHDATA1` over a payload that fits
/// `OP_DATA_n`, …), and truncated pushes are all `None`.
pub fn read_push_explicit(script: &[u8]) -> Option<(&[u8], usize)> {
    let (&op, rest) = script.split_first()?;
    let (len, header) = match op {
        0x00 => return Some((&[], 1)),
        1..=75 => (op as usize, 1),
        0x4c => {
            let n = *rest.first()? as usize;
            if n < 76 {
                return None;
            }
            (n, 2)
        }
        0x4d => {
            let n = u16::from_le_bytes(rest.get(..2)?.try_into().ok()?) as usize;
            if n < 0x100 {
                return None;
            }
            (n, 3)
        }
        0x4e => {
            let n = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
            if n < 0x10000 {
                return None;
            }
            (n, 5)
        }
        _ => return None,
    };
    let payload = script.get(header..header + len)?;
    Some((payload, header + len))
}

/// Eight-byte little-endian signed-magnitude `int` state payload (§3.4.3/§3.4.4):
/// magnitude in the low 63 bits, sign in the top bit of the last byte.
/// `None` for `i64::MIN` — the §3.4.3 range is symmetric and its magnitude
/// does not fit.
pub fn encode_state_int(value: i64) -> Option<[u8; 8]> {
    if value == i64::MIN {
        return None;
    }
    let mut out = value.unsigned_abs().to_le_bytes();
    if value < 0 {
        out[7] |= 0x80;
    }
    Some(out)
}

/// Inverse of `encode_state_int`. A set sign bit over a zero magnitude is
/// `None`: no in-range value encodes to it, and accepting it would give zero
/// two encodings, breaking §3.7.1's byte-exactness requirement.
pub fn decode_state_int(bytes: &[u8; 8]) -> Option<i64> {
    let mut magnitude = *bytes;
    magnitude[7] &= 0x7f;
    let magnitude = u64::from_le_bytes(magnitude) as i64;
    match (bytes[7] & 0x80 != 0, magnitude) {
        (false, m) => Some(m),
        (true, 0) => None,
        (true, m) => Some(-m),
    }
}

/// Minimal ScriptNum for an `int` invocation argument (§3.4.3): little-endian
/// magnitude, sign carried by the top bit of the last byte, one extension
/// byte only when the magnitude's own top bit is set. The crate root's
/// `snum` is documented non-negative-only; this codec also covers negative
/// values. `None` for `i64::MIN` (out of the §3.4.3 range).
pub fn encode_arg_int(value: i64) -> Option<Vec<u8>> {
    if value == i64::MIN {
        return None;
    }
    let mut magnitude = value.unsigned_abs();
    let mut out = Vec::new();
    while magnitude > 0 {
        out.push((magnitude & 0xff) as u8);
        magnitude >>= 8;
    }
    if out.last().is_some_and(|b| b & 0x80 != 0) {
        out.push(0);
    }
    if value < 0 {
        // value < 0 implies a non-empty magnitude
        let last = out.len() - 1;
        out[last] |= 0x80;
    }
    Some(out)
}

/// Inverse of `encode_arg_int`: `None` unless `bytes` is the minimal
/// ScriptNum of a value in the §3.4.3 range — padded encodings, negative
/// zero, and out-of-range magnitudes are all rejected.
pub fn decode_arg_int(bytes: &[u8]) -> Option<i64> {
    let Some((&last, head)) = bytes.split_last() else {
        return Some(0);
    };
    // minimality: a last byte carrying only the sign bit must be shielding
    // the previous byte's high bit
    if last & 0x7f == 0 && head.last().is_none_or(|b| b & 0x80 == 0) {
        return None;
    }
    // an in-range value needs at most nine bytes even with an extension byte
    if bytes.len() > 9 {
        return None;
    }
    let mut magnitude = ((last & 0x7f) as u128) << (8 * head.len());
    for (i, &b) in head.iter().enumerate() {
        magnitude |= (b as u128) << (8 * i);
    }
    let magnitude = i64::try_from(magnitude).ok()?;
    Some(if last & 0x80 != 0 {
        -magnitude
    } else {
        magnitude
    })
}

/// Dispatch tag (§3.5.1): the first four bytes of `Hash(UTF8(signature))`,
/// where `signature` is `"{name}({comma-separated dispatch type names})"`
/// with no whitespace — e.g. `"step(int,byte[4],bool,byte)"`.
///
/// `DispatchTypeName(T)` is the spec's recursive rule, and the caller owes
/// this function a signature already written under it: a scalar is its
/// `TypeName`; a fixed array of `T` with length `N` is
/// `DispatchTypeName(T)[N]`; a dynamic array is `DispatchTypeName(T)[]`; a
/// record is `{DispatchTypeName(T_1),...,DispatchTypeName(T_n)}` over its
/// ordered field types, with the record's name and its field names
/// omitted. So `transfer(KCC20State[],byte[])` is not a §3.5.1 signature;
/// `transfer({int,byte[32],byte,byte,byte[32],byte[32]}[],byte[])` is. The
/// silverscript 1.0 compiler writes its tags under the same rule
/// (`write_dispatch_type_name`, compile/helpers.rs at 3ed9733).
pub fn dispatch_tag(signature: &str) -> [u8; 4] {
    let mut tag = [0u8; 4];
    tag.copy_from_slice(&hash32(signature.as_bytes())[..4]);
    tag
}

/// `TemplateHash(prefix, suffix)` (§3.7.3):
/// `Hash(LE64(len(prefix)) || prefix || LE64(len(suffix)) || suffix)`,
/// BLAKE3 like every other `Hash` in the spec. The length fields bind the
/// prefix/suffix boundary — a plain `Hash(prefix || suffix)` would collide
/// across different cuts.
///
/// This is the NAME of a template, computed by readers; nothing on chain
/// carries it. The commitment an argent app checks in-script is the same
/// framing under BLAKE2b (that is what `OP_BLAKE2B` gives a program), so it
/// lives in `crate::argent` as its own function and must stay there.
pub fn template_hash(prefix: &[u8], suffix: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(prefix.len() as u64).to_le_bytes());
    hasher.update(prefix);
    hasher.update(&(suffix.len() as u64).to_le_bytes());
    hasher.update(suffix);
    *hasher.finalize().as_bytes()
}

/// Version-0 P2SH script public key committing to `program` (§3.6):
/// `OP_BLAKE2B OP_DATA_32 Blake2b(R) OP_EQUAL`. The one place the spec uses
/// BLAKE2b: unkeyed, 32-byte output, no salt, exactly what consensus computes.
pub fn envelope_spk(program: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(35);
    out.push(0xaa);
    out.push(0x20);
    out.extend_from_slice(
        blake2b_simd::Params::new()
            .hash_length(32)
            .hash(program)
            .as_bytes(),
    );
    out.push(0x87);
    out
}

/// Signature script spending a KCC1 P2SH output (§3.6): the pre-encoded
/// argument pushes, then `OP_DATA_4 dispatch_tag` (mandatory for every
/// program since the merge, single-entrypoint ones included), then
/// `PushMinimal(R)` as the mandatory final push.
pub fn signature_script(arg_pushes: &[u8], dispatch: &[u8; 4], program: &[u8]) -> Vec<u8> {
    let mut out = arg_pushes.to_vec();
    out.extend_from_slice(&tag_push(dispatch));
    out.extend_from_slice(&encode_push(program));
    out
}

/// The dispatch-tag element of a signature script (§3.6): `OP_DATA_4` and
/// the four tag bytes, for every entrypoint, zero-argument ones included.
pub fn tag_push(dispatch: &[u8; 4]) -> [u8; 5] {
    let [a, b, c, d] = *dispatch;
    [0x04, a, b, c, d]
}

/// Decode exactly one `PushMinimal` at the start of `script` (§3.4.2), giving
/// the payload and the bytes consumed. Byte-exact like `read_push_explicit`:
/// the consumed bytes must equal `encode_push(payload)`, so a data-form push
/// of a value the numeric opcodes own (`01 05` for what is `OP_5`), an
/// oversized length form, and a truncated push are all `None`. The crate
/// root's disassembler deliberately tolerates those shapes; an ABI reader
/// must not.
pub fn read_push_minimal(script: &[u8]) -> Option<(Vec<u8>, usize)> {
    let (&op, rest) = script.split_first()?;
    let (payload, consumed) = match op {
        0x00 => (Vec::new(), 1),
        0x4f => (vec![0x81], 1),
        0x51..=0x60 => (vec![op - 0x50], 1),
        1..=75 => (rest.get(..op as usize)?.to_vec(), 1 + op as usize),
        0x4c => {
            let n = *rest.first()? as usize;
            (rest.get(1..1 + n)?.to_vec(), 2 + n)
        }
        0x4d => {
            let n = u16::from_le_bytes(rest.get(..2)?.try_into().ok()?) as usize;
            (rest.get(2..2 + n)?.to_vec(), 3 + n)
        }
        0x4e => {
            let n = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
            (rest.get(4..4 + n)?.to_vec(), 5 + n)
        }
        _ => return None,
    };
    let canonical = encode_push(&payload).as_slice() == &script[..consumed];
    canonical.then_some((payload, consumed))
}

/// Every push of a push-only script, each one `PushMinimal` (§3.4.2), as
/// payloads in push order. `None` at the first non-push opcode, non-minimal
/// push or truncated push: a KCC-1 reader takes the canonical form or
/// nothing, because signature-script pushes can be re-encoded without
/// changing the txid (§7.4).
pub fn read_pushes_minimal(script: &[u8]) -> Option<Vec<Vec<u8>>> {
    let mut pushes = Vec::new();
    let mut at = 0usize;
    while at < script.len() {
        let (payload, consumed) = read_push_minimal(&script[at..])?;
        pushes.push(payload);
        at += consumed;
    }
    Some(pushes)
}

/// A KCC1 invocation read back from the signature script that spends a
/// version-0 P2SH envelope (§3.6): argument pushes, then the dispatch tag,
/// then the revealed program as the mandatory final push. The inverse of
/// [`signature_script`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    /// Argument push payloads, in push order.
    pub args: Vec<Vec<u8>>,
    /// The four dispatch-tag bytes (§3.5.1).
    pub dispatch: [u8; 4],
    /// The revealed program `R`. Claimed, not yet proven: check it against
    /// the spent envelope ([`read_invocation_checked`]) before trusting it.
    pub program: Vec<u8>,
}

/// Read a §3.6 signature script: every push minimal, the tag exactly four
/// bytes directly before the program, the program non-empty and last.
///
/// This reads the SPEC's layout and only that. Which layout a program uses
/// is a fact about its compiler: silverscript v1.0.0 (3ed9733) builds
/// dispatch on this four-byte tag, while programs from the michaelsutton
/// fork at d57e5df (the compiler behind kascov's `/compile` today, and the
/// one every Zealous family on testnet-10 was built with) dispatch on a
/// one-byte numeric selector instead. A trailing `byte[4]` argument of a
/// selector program is indistinguishable from a tag by bytes alone, so the
/// layout is pin knowledge, and a reader that guessed would misfile one as
/// the other. This function is for programs a pin declares to carry KCC-1
/// tags; it never infers the layout.
///
/// `None` on anything unexpected: a non-push opcode anywhere, a non-minimal
/// push, an empty program, or a tag push that is not exactly four bytes.
pub fn read_invocation(sig_script: &[u8]) -> Option<Invocation> {
    let mut pushes = read_pushes_minimal(sig_script)?;
    let program = pushes.pop()?;
    if program.is_empty() {
        return None;
    }
    let dispatch: [u8; 4] = pushes.pop()?.as_slice().try_into().ok()?;
    Some(Invocation {
        args: pushes,
        dispatch,
        program,
    })
}

/// [`read_invocation`], additionally requiring the revealed program to hash
/// to the envelope it spends (§3.6): `envelope_spk(program) == spk`. The only
/// form whose `program` is proven rather than claimed.
pub fn read_invocation_checked(spk: &[u8], sig_script: &[u8]) -> Option<Invocation> {
    let inv = read_invocation(sig_script)?;
    (envelope_spk(&inv.program) == spk).then_some(inv)
}

// ---- The Value ABI (§3.4): types, arrays, records, and their codecs --------

/// A KCC-1 type (§3.4.1). `byte[N]` and `byte[]` are arrays of `byte`,
/// exactly as the grammar spells them, so the fixed and the dynamic byte
/// string need no variants of their own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Int,
    Bool,
    Byte,
    /// UTF-8 string, no terminator; no fixed width.
    String,
    /// 32-byte public key.
    PubKey,
    /// 65-byte transaction signature.
    Sig,
    /// 64-byte data signature.
    DataSig,
    /// `T[N]` with a length, `T[]` without.
    Array(Box<Type>, Option<usize>),
    Record(Record),
}

/// A resolved record (§3.4.6): its exact case-sensitive name and its fields,
/// in declaration order, which is part of the ABI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub fields: Vec<(String, Type)>,
}

/// A record as a Program ABI declares it: field types written as `TypeName`
/// text, possibly naming other declared records. [`Type::parse`] resolves
/// these, so an ABI written by hand (§3.4) needs no compiler to be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordDecl {
    pub name: String,
    pub fields: Vec<(String, String)>,
}

/// A KCC-1 value, paired with a [`Type`] when encoding or decoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    /// Every byte-like type: `byte`, `byte[N]`, `byte[]`, `string`,
    /// `pubkey`, `sig`, `datasig`. Width and content are checked against
    /// the type.
    Bytes(Vec<u8>),
    /// The elements of an array of anything but `byte`, in index order.
    Array(Vec<Value>),
    /// A record's field values, in declaration order.
    Record(Vec<Value>),
}

fn scalar_named(name: &str) -> Option<Type> {
    Some(match name {
        "int" => Type::Int,
        "bool" => Type::Bool,
        "byte" => Type::Byte,
        "string" => Type::String,
        "pubkey" => Type::PubKey,
        "sig" => Type::Sig,
        "datasig" => Type::DataSig,
        _ => return None,
    })
}

/// `[A-Za-z_][A-Za-z0-9_]*`, the one shape every type identifier takes
/// (§3.4.1).
fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// An array length as §3.4.1 writes it: base ten, no sign, no leading zero
/// except the lone `0`.
fn parse_len(text: &str) -> Option<usize> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    canonical.then(|| text.parse().ok()).flatten()
}

impl Type {
    /// Read a `TypeName` (§3.4.1) against the records an ABI declares. `Err`
    /// names what is not KCC-1: a malformed name or length, an undeclared
    /// record, a record declared twice or containing itself, or a field
    /// name that is not an identifier or repeats.
    pub fn parse(text: &str, records: &[RecordDecl]) -> Result<Type, String> {
        for (i, decl) in records.iter().enumerate() {
            if !is_identifier(&decl.name) || scalar_named(&decl.name).is_some() {
                return Err(format!("{:?} cannot name a record", decl.name));
            }
            if records[..i].iter().any(|d| d.name == decl.name) {
                return Err(format!("record {} is declared twice", decl.name));
            }
        }
        parse_type(text, records, &mut Vec::new())
    }

    /// `TypeName(T)` (§3.4.1): a record is its name.
    pub fn type_name(&self) -> String {
        match self {
            Type::Int => "int".into(),
            Type::Bool => "bool".into(),
            Type::Byte => "byte".into(),
            Type::String => "string".into(),
            Type::PubKey => "pubkey".into(),
            Type::Sig => "sig".into(),
            Type::DataSig => "datasig".into(),
            Type::Array(elem, Some(n)) => format!("{}[{n}]", elem.type_name()),
            Type::Array(elem, None) => format!("{}[]", elem.type_name()),
            Type::Record(r) => r.name.clone(),
        }
    }

    /// `DispatchTypeName(T)` (§3.5.1): a record is its ordered field types
    /// in braces, its own name and its field names omitted.
    pub fn dispatch_type_name(&self) -> String {
        match self {
            Type::Array(elem, Some(n)) => format!("{}[{n}]", elem.dispatch_type_name()),
            Type::Array(elem, None) => format!("{}[]", elem.dispatch_type_name()),
            Type::Record(r) => {
                let fields: Vec<String> = r.fields.iter().map(|(_, t)| t.dispatch_type_name()).collect();
                format!("{{{}}}", fields.join(","))
            }
            scalar => scalar.type_name(),
        }
    }

    /// The fixed payload width `width(T)` (§3.4.5), `None` for a type
    /// without one (`string`, any dynamic array, and whatever contains
    /// them). A record's is the sum of its fields', which is what `Packed`
    /// (§3.9.1) lays out.
    pub fn width(&self) -> Option<usize> {
        match self {
            Type::Int => Some(8),
            Type::Bool | Type::Byte => Some(1),
            Type::PubKey => Some(32),
            Type::Sig => Some(65),
            Type::DataSig => Some(64),
            Type::String | Type::Array(_, None) => None,
            Type::Array(elem, Some(n)) => elem.width()?.checked_mul(*n),
            Type::Record(r) => r
                .fields
                .iter()
                .try_fold(0usize, |sum, (_, t)| sum.checked_add(t.width()?)),
        }
    }

    fn has_record(&self) -> bool {
        match self {
            Type::Record(_) => true,
            Type::Array(elem, _) => elem.has_record(),
            _ => false,
        }
    }

    /// The types of the stack elements this type lowers to (§3.4.6): a
    /// record is its fields' leaves in order; an array of records is one
    /// array per leaf, grouping that leaf across every element; anything
    /// else is itself.
    fn leaves(&self) -> Vec<Type> {
        match self {
            Type::Record(r) => r.fields.iter().flat_map(|(_, t)| t.leaves()).collect(),
            Type::Array(elem, len) if elem.has_record() => elem
                .leaves()
                .into_iter()
                .map(|leaf| Type::Array(Box::new(leaf), *len))
                .collect(),
            other => vec![other.clone()],
        }
    }
}

fn parse_type(text: &str, records: &[RecordDecl], resolving: &mut Vec<String>) -> Result<Type, String> {
    let (base, mut dims) = text.split_at(text.find('[').unwrap_or(text.len()));
    if !is_identifier(base) {
        return Err(format!("{text:?} is not a KCC-1 type name"));
    }
    let mut ty = match scalar_named(base) {
        Some(scalar) => scalar,
        None => Type::Record(resolve_record(base, records, resolving)?),
    };
    while !dims.is_empty() {
        let close = dims
            .find(']')
            .filter(|_| dims.starts_with('['))
            .ok_or_else(|| format!("{text:?} is not a KCC-1 type name"))?;
        let len = match &dims[1..close] {
            "" => None,
            n => Some(parse_len(n).ok_or_else(|| format!("{text:?}: {n:?} is not a canonical array length"))?),
        };
        ty = Type::Array(Box::new(ty), len);
        dims = &dims[close + 1..];
    }
    Ok(ty)
}

fn resolve_record(name: &str, records: &[RecordDecl], resolving: &mut Vec<String>) -> Result<Record, String> {
    if resolving.iter().any(|r| r == name) {
        return Err(format!("record {name} contains itself"));
    }
    let decl = records
        .iter()
        .find(|d| d.name == name)
        .ok_or_else(|| format!("{name} is neither a KCC-1 scalar type nor a declared record"))?;
    resolving.push(name.to_string());
    let mut fields: Vec<(String, Type)> = Vec::with_capacity(decl.fields.len());
    for (field, ty) in &decl.fields {
        if !is_identifier(field) {
            return Err(format!("record {name}: {field:?} is not a field name"));
        }
        if fields.iter().any(|(f, _)| f == field) {
            return Err(format!("record {name} declares field {field} twice"));
        }
        fields.push((field.clone(), parse_type(ty, records, resolving)?));
    }
    resolving.pop();
    Ok(Record {
        name: name.to_string(),
        fields,
    })
}

fn width_words(ty: &Type) -> String {
    match ty.width() {
        None => "no fixed width".into(),
        Some(0) => "zero width".into(),
        Some(w) => format!("width {w}"),
    }
}

/// Whether a type may appear as an argument or a state field (§3.4.5,
/// §3.4.6, §3.7.1, which applies the same restrictions to state): array
/// elements need a positive fixed payload width, and so does every
/// recursively lowered leaf of an array of records. A record outside an
/// array may hold variable-width fields; lowering gives each its own push.
pub fn validate_type(ty: &Type) -> Result<(), String> {
    match ty {
        Type::Array(elem, _) => {
            validate_type(elem)?;
            if elem.has_record() {
                let leaves = elem.leaves();
                if leaves.is_empty() {
                    return Err(format!(
                        "{}: an array of records needs a leaf field to count its elements by",
                        ty.type_name()
                    ));
                }
                if let Some(leaf) = leaves.iter().find(|l| l.width().is_none_or(|w| w == 0)) {
                    return Err(format!(
                        "{}: every lowered leaf of an array of records needs a positive fixed payload width, and {} has {} (§3.4.6)",
                        ty.type_name(),
                        leaf.type_name(),
                        width_words(leaf)
                    ));
                }
            } else if elem.width().is_none_or(|w| w == 0) {
                return Err(format!(
                    "{}: array elements need a positive fixed payload width, and {} has {} (§3.4.5)",
                    ty.type_name(),
                    elem.type_name(),
                    width_words(elem)
                ));
            }
            Ok(())
        }
        Type::Record(r) => r.fields.iter().try_for_each(|(_, t)| validate_type(t)),
        _ => Ok(()),
    }
}

fn out_of_range(value: i64) -> String {
    format!("{value} is outside the KCC-1 int range -(2^63 - 1)..2^63 - 1 (§3.4.3)")
}

/// An `int` written in base ten, as an ABI or a vector file carries one,
/// held to the KCC-1 range (§3.4.3). `-2^63` parses as an `i64` but is not
/// a KCC-1 int; neither is anything wider.
pub fn parse_int(decimal: &str) -> Result<i64, String> {
    let digits = decimal.strip_prefix('-').unwrap_or(decimal);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("{decimal:?} is not a base-ten integer"));
    }
    match decimal.parse::<i64>() {
        Ok(i64::MIN) | Err(_) => Err(format!(
            "{decimal} is outside the KCC-1 int range -(2^63 - 1)..2^63 - 1 (§3.4.3)"
        )),
        Ok(v) => Ok(v),
    }
}

/// The canonical payload of one value (§3.4.4, §3.4.5): `StatePayload` for
/// a lowered state field (§3.7.1), and what an array element contributes to
/// its array. An `int` is its eight-byte form here; only a standalone
/// argument uses the minimal ScriptNum. A record has no payload of its own:
/// it is lowered to its fields first.
pub fn payload(ty: &Type, value: &Value) -> Result<Vec<u8>, String> {
    let width_error = |got: usize| format!("{got} bytes where {} was declared", ty.type_name());
    match (ty, value) {
        (Type::Int, Value::Int(v)) => encode_state_int(*v)
            .map(|b| b.to_vec())
            .ok_or_else(|| out_of_range(*v)),
        (Type::Bool, Value::Bool(b)) => Ok(vec![*b as u8]),
        (Type::String, Value::Bytes(b)) => std::str::from_utf8(b)
            .map(|_| b.clone())
            .map_err(|_| "a string payload must be UTF-8".to_string()),
        (Type::Byte | Type::PubKey | Type::Sig | Type::DataSig, Value::Bytes(b)) => {
            if Some(b.len()) == ty.width() {
                Ok(b.clone())
            } else {
                Err(width_error(b.len()))
            }
        }
        (Type::Array(elem, len), Value::Bytes(b)) if **elem == Type::Byte => {
            if len.is_some_and(|n| n != b.len()) {
                Err(width_error(b.len()))
            } else {
                Ok(b.clone())
            }
        }
        (Type::Array(elem, len), Value::Array(items)) if **elem != Type::Byte => {
            if len.is_some_and(|n| n != items.len()) {
                return Err(format!(
                    "{} elements where {} was declared",
                    items.len(),
                    ty.type_name()
                ));
            }
            let mut out = Vec::new();
            for item in items {
                out.extend(payload(elem, item)?);
            }
            Ok(out)
        }
        (Type::Record(_), _) => Err(format!(
            "{} is a record, lowered to its fields, with no payload of its own (§3.4.6)",
            ty.type_name()
        )),
        _ => Err(format!("the value is not a {}", ty.type_name())),
    }
}

/// Inverse of [`payload`], rejecting every payload that would not re-encode
/// to the same bytes: a wrong width, a bool other than `00`/`01`, an `int`
/// that is not eight bytes or is negative zero, invalid UTF-8, an array
/// whose length is not a whole number of elements or not its declared
/// count (§3.4.4, §3.4.5, §3.7.1).
pub fn decode_payload(ty: &Type, bytes: &[u8]) -> Result<Value, String> {
    match ty {
        Type::Int => {
            let raw: &[u8; 8] = bytes
                .try_into()
                .map_err(|_| format!("{} bytes where an int's eight-byte payload was declared", bytes.len()))?;
            decode_state_int(raw)
                .map(Value::Int)
                .ok_or_else(|| "negative zero is not a canonical int payload".to_string())
        }
        Type::Bool => match bytes {
            [0x00] => Ok(Value::Bool(false)),
            [0x01] => Ok(Value::Bool(true)),
            _ => Err(format!("{:?} is not a bool payload, which is exactly 00 or 01", hex::encode(bytes))),
        },
        Type::Array(elem, len) if **elem != Type::Byte => {
            let w = elem
                .width()
                .filter(|w| *w > 0)
                .ok_or_else(|| format!("{}: array elements need a positive fixed payload width (§3.4.5)", ty.type_name()))?;
            if !bytes.len().is_multiple_of(w) {
                return Err(format!(
                    "{} bytes is not a whole number of {w}-byte {} elements (§3.4.5)",
                    bytes.len(),
                    elem.type_name()
                ));
            }
            let count = bytes.len() / w;
            if len.is_some_and(|n| n != count) {
                return Err(format!("{count} elements where {} was declared", ty.type_name()));
            }
            bytes
                .chunks(w)
                .map(|chunk| decode_payload(elem, chunk))
                .collect::<Result<_, _>>()
                .map(Value::Array)
        }
        _ => {
            // width and content rules are the encoder's, re-checked in reverse
            let value = Value::Bytes(bytes.to_vec());
            payload(ty, &value).map(|_| value)
        }
    }
}

/// An array value of `elem` from its elements: `byte` arrays are one byte
/// string, everything else a list.
fn array_value(elem: &Type, items: Vec<Value>) -> Value {
    if *elem != Type::Byte {
        return Value::Array(items);
    }
    let mut bytes = Vec::with_capacity(items.len());
    for item in items {
        if let Value::Bytes(b) = item {
            bytes.extend(b);
        }
    }
    Value::Bytes(bytes)
}

/// The elements of an array value, one value each; the inverse of
/// [`array_value`].
fn array_items(value: Value) -> Vec<Value> {
    match value {
        Value::Bytes(bytes) => bytes.into_iter().map(|b| Value::Bytes(vec![b])).collect(),
        Value::Array(items) => items,
        other => vec![other],
    }
}

/// Recursive lowering (§3.4.6): a record becomes its fields in order, an
/// array of records one grouped array per leaf field. Each pair pushed is
/// one stack element: one push as an argument, one `PushExplicit` in state.
fn lower(ty: &Type, value: &Value, out: &mut Vec<(Type, Value)>) -> Result<(), String> {
    match (ty, value) {
        (Type::Record(r), Value::Record(values)) => {
            if values.len() != r.fields.len() {
                return Err(format!(
                    "{} values where record {} has {} fields",
                    values.len(),
                    r.name,
                    r.fields.len()
                ));
            }
            r.fields
                .iter()
                .zip(values)
                .try_for_each(|((_, t), v)| lower(t, v, out))
        }
        (Type::Record(r), _) => Err(format!("the value is not a {} record", r.name)),
        (Type::Array(elem, len), Value::Array(items)) if elem.has_record() => {
            if len.is_some_and(|n| n != items.len()) {
                return Err(format!(
                    "{} elements where {} was declared",
                    items.len(),
                    ty.type_name()
                ));
            }
            let leaves = elem.leaves();
            let mut groups: Vec<Vec<Value>> = vec![Vec::with_capacity(items.len()); leaves.len()];
            for item in items {
                let mut lowered = Vec::with_capacity(leaves.len());
                lower(elem, item, &mut lowered)?;
                for (group, (_, v)) in groups.iter_mut().zip(lowered) {
                    group.push(v);
                }
            }
            for (leaf, group) in leaves.into_iter().zip(groups) {
                let grouped = array_value(&leaf, group);
                out.push((Type::Array(Box::new(leaf), *len), grouped));
            }
            Ok(())
        }
        _ => {
            out.push((ty.clone(), value.clone()));
            Ok(())
        }
    }
}

/// Inverse of [`lower`]: rebuild one declared value from its decoded leaves.
/// Every field group of an array of records must count the same records,
/// and a fixed one its declared count (§3.4.6).
fn raise(ty: &Type, leaves: &mut impl Iterator<Item = Value>) -> Result<Value, String> {
    match ty {
        Type::Record(r) => r
            .fields
            .iter()
            .map(|(_, t)| raise(t, leaves))
            .collect::<Result<_, _>>()
            .map(Value::Record),
        Type::Array(elem, len) if elem.has_record() => {
            let groups: Vec<Vec<Value>> = elem
                .leaves()
                .iter()
                .map(|_| leaves.next().map(array_items))
                .collect::<Option<_>>()
                .ok_or("fewer stack elements than the declared types lower to")?;
            let count = groups.first().map_or(0, Vec::len);
            if let Some(other) = groups.iter().map(Vec::len).find(|n| *n != count) {
                return Err(format!(
                    "{}: one field group holds {count} records and another {other} (§3.4.6)",
                    ty.type_name()
                ));
            }
            if len.is_some_and(|n| n != count) {
                return Err(format!("{count} records where {} was declared (§3.4.6)", ty.type_name()));
            }
            let mut columns: Vec<_> = groups.into_iter().map(Vec::into_iter).collect();
            (0..count)
                .map(|_| {
                    let mut row = columns.iter_mut().filter_map(Iterator::next);
                    raise(elem, &mut row)
                })
                .collect::<Result<_, _>>()
                .map(Value::Array)
        }
        _ => leaves
            .next()
            .ok_or_else(|| "fewer stack elements than the declared types lower to".to_string()),
    }
}

/// The push one lowered argument takes (§3.4.3, §3.4.4): an `int` as its
/// minimal ScriptNum, a `bool` as `OP_0`/`OP_1`, everything else as its
/// payload, always under `PushMinimal`.
fn arg_push(ty: &Type, value: &Value) -> Result<Vec<u8>, String> {
    match (ty, value) {
        (Type::Int, Value::Int(v)) => encode_arg_int(*v)
            .map(|b| encode_push(&b))
            .ok_or_else(|| out_of_range(*v)),
        (Type::Bool, Value::Bool(b)) => {
            let push: &[u8] = if *b { &[0x01] } else { &[] };
            Ok(encode_push(push))
        }
        _ => payload(ty, value).map(|p| encode_push(&p)),
    }
}

/// Inverse of [`arg_push`] over the push's payload.
fn decode_arg(ty: &Type, push: &[u8]) -> Result<Value, String> {
    match ty {
        Type::Int => decode_arg_int(push)
            .map(Value::Int)
            .ok_or_else(|| format!("{:?} is not a minimal ScriptNum in the KCC-1 int range (§3.4.3)", hex::encode(push))),
        Type::Bool => match push {
            [] => Ok(Value::Bool(false)),
            [0x01] => Ok(Value::Bool(true)),
            _ => Err(format!(
                "a standalone bool is OP_0 or OP_1, not a push of {:?} (§3.4.4)",
                hex::encode(push)
            )),
        },
        _ => decode_payload(ty, push),
    }
}

/// The encoded pushes one argument lowers to (§3.4.6), in stack order: one
/// for a scalar or an array of scalars, one per leaf for a record, one per
/// leaf field group for an array of records.
pub fn arg_pushes(ty: &Type, value: &Value) -> Result<Vec<Vec<u8>>, String> {
    validate_type(ty)?;
    let mut leaves = Vec::new();
    lower(ty, value, &mut leaves)?;
    leaves.iter().map(|(t, v)| arg_push(t, v)).collect()
}

/// `PushArguments(arguments)` (§3.5.2): every argument's lowered pushes, in
/// parameter order, concatenated.
pub fn encode_args(args: &[(Type, Value)]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for (ty, value) in args {
        out.extend(arg_pushes(ty, value)?.concat());
    }
    Ok(out)
}

/// Decode `PushArguments` for the declared types: every push minimal, one
/// per lowered leaf, every payload canonical for its type. The inverse of
/// [`encode_args`], and as strict as it.
pub fn decode_args(types: &[Type], script: &[u8]) -> Result<Vec<Value>, String> {
    let pushes =
        read_pushes_minimal(script).ok_or("the arguments are not a run of minimal data pushes (§3.4.2)")?;
    decode_arg_pushes(types, &pushes)
}

/// [`decode_args`] over push payloads already read, as [`read_invocation`]
/// gives them.
pub fn decode_arg_pushes(types: &[Type], pushes: &[Vec<u8>]) -> Result<Vec<Value>, String> {
    types.iter().try_for_each(validate_type)?;
    let leaves: Vec<Type> = types.iter().flat_map(Type::leaves).collect();
    if leaves.len() != pushes.len() {
        return Err(format!(
            "{} pushes where the declared types lower to {}",
            pushes.len(),
            leaves.len()
        ));
    }
    let decoded = leaves
        .iter()
        .zip(pushes)
        .map(|(t, p)| decode_arg(t, p))
        .collect::<Result<Vec<_>, _>>()?;
    let mut decoded = decoded.into_iter();
    types.iter().map(|ty| raise(ty, &mut decoded)).collect()
}

/// `EncodeState` (§3.7.1): each field lowered, each lowered field's payload
/// in its own `PushExplicit`, in declaration order.
pub fn encode_state_values(fields: &[(Type, Value)]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for (ty, value) in fields {
        validate_type(ty)?;
        let mut leaves = Vec::new();
        lower(ty, value, &mut leaves)?;
        for (t, v) in &leaves {
            out.extend(push_explicit(&payload(t, v)?));
        }
    }
    Ok(out)
}

/// Read one declared state field from the front of `encoded`: exactly one
/// canonical `PushExplicit` per lowered leaf, each payload canonical for
/// its type (§3.7.1). Gives the value and the bytes consumed, so a caller
/// can name the field a refusal is about.
pub fn read_state_field(ty: &Type, encoded: &[u8]) -> Result<(Value, usize), String> {
    validate_type(ty)?;
    let mut at = 0;
    let mut decoded = Vec::new();
    for leaf in ty.leaves() {
        let rest = &encoded[at..];
        if rest.is_empty() {
            return Err("the state ends before this field".into());
        }
        let (payload, consumed) = read_push_explicit(rest)
            .ok_or("not one canonical PushExplicit data push (§3.7.1)")?;
        decoded.push(decode_payload(&leaf, payload)?);
        at += consumed;
    }
    Ok((raise(ty, &mut decoded.into_iter())?, at))
}

/// Decode a complete encoded state against its declared field types
/// (§3.7.1): every field read by [`read_state_field`], trailing bytes
/// refused.
pub fn decode_state_values(types: &[Type], encoded: &[u8]) -> Result<Vec<Value>, String> {
    let mut at = 0;
    let mut values = Vec::with_capacity(types.len());
    for (i, ty) in types.iter().enumerate() {
        let (value, consumed) = read_state_field(ty, &encoded[at..])
            .map_err(|e| format!("state field {i} ({}): {e}", ty.type_name()))?;
        values.push(value);
        at += consumed;
    }
    if at != encoded.len() {
        return Err(format!(
            "{} trailing bytes after the last state field (§3.7.1)",
            encoded.len() - at
        ));
    }
    Ok(values)
}

/// `FunctionSignature` (§3.5.1): `name(DispatchTypeName,...)`, no
/// whitespace. [`dispatch_tag`] cuts the tag from it.
pub fn function_signature(name: &str, args: &[Type]) -> String {
    let types: Vec<String> = args.iter().map(Type::dispatch_type_name).collect();
    format!("{name}({})", types.join(","))
}

/// The program-level ABI checks (§3.5, §3.5.1): at least one entrypoint,
/// every argument type allowed, and every dispatch tag distinct, whether
/// two signatures are identical once record and field names drop out or
/// merely collide in the four-byte truncation.
pub fn validate_entrypoints(entrypoints: &[(String, Vec<Type>)]) -> Result<(), String> {
    if entrypoints.is_empty() {
        return Err("a covenant program needs at least one entrypoint (§3.5)".into());
    }
    let mut seen: BTreeMap<[u8; 4], String> = BTreeMap::new();
    for (name, args) in entrypoints {
        for ty in args {
            validate_type(ty).map_err(|e| format!("{name}: {e}"))?;
        }
        let signature = function_signature(name, args);
        let tag = dispatch_tag(&signature);
        if let Some(first) = seen.insert(tag, signature.clone()) {
            return Err(if first == signature {
                format!(
                    "two entrypoints share the signature {signature}, so they share dispatch tag {} (§3.5.1)",
                    hex::encode(tag)
                )
            } else {
                format!("{first} and {signature} share dispatch tag {} (§3.5.1)", hex::encode(tag))
            });
        }
    }
    Ok(())
}

/// A program cut at a state range (§3.7, §3.7.4): `(prefix, encoded_state,
/// suffix)` = `R[0:start]`, `R[start:start+len]`, `R[start+len:]`. The same
/// cut serves the canonical template and every template view. `None` when
/// the range runs past the program: §3.7.2 has a reader check the bounds
/// before it decodes anything.
pub fn state_cut(program: &[u8], start: usize, len: usize) -> Option<(&[u8], &[u8], &[u8])> {
    let end = start.checked_add(len).filter(|end| *end <= program.len())?;
    Some((&program[..start], &program[start..end], &program[end..]))
}

/// `R_next = prefix || EncodeState(next_state) || suffix` (§3.7.5): the
/// continuation program under a template, `next_state` holding only the
/// fields that template's range selects.
pub fn continuation(prefix: &[u8], next_state: &[(Type, Value)], suffix: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = prefix.to_vec();
    out.extend(encode_state_values(next_state)?);
    out.extend_from_slice(suffix);
    Ok(out)
}

// ---- The scalar field API, over the general codec -------------------------

/// Scalar field types with a defined state lowering (§3.4.1/§3.4.4), the
/// shape most state layouts need. [`Type`] covers arrays and records too;
/// these encode and decode through it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    Int,
    Bool,
    Byte,
    /// Variable byte string (`byte[]`); no fixed width, so invalid in packed
    /// virtual-element payloads (§3.9.1).
    Bytes,
    /// UTF-8 string, no terminator (`string`); variable width like `Bytes`.
    String,
    /// 32-byte public key (`pubkey`).
    PubKey,
    /// 65-byte transaction signature (`sig`).
    Sig,
    /// 64-byte data signature (`datasig`).
    DataSig,
    /// `byte[N]`.
    FixedBytes(usize),
}

/// A field value; paired with a `FieldType` when encoding or decoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateValue {
    Int(i64),
    Bool(bool),
    /// Every bytes-like type; width and content checked against the type.
    Bytes(Vec<u8>),
}

impl From<FieldType> for Type {
    fn from(ty: FieldType) -> Type {
        match ty {
            FieldType::Int => Type::Int,
            FieldType::Bool => Type::Bool,
            FieldType::Byte => Type::Byte,
            FieldType::Bytes => Type::Array(Box::new(Type::Byte), None),
            FieldType::String => Type::String,
            FieldType::PubKey => Type::PubKey,
            FieldType::Sig => Type::Sig,
            FieldType::DataSig => Type::DataSig,
            FieldType::FixedBytes(n) => Type::Array(Box::new(Type::Byte), Some(n)),
        }
    }
}

impl From<&StateValue> for Value {
    fn from(value: &StateValue) -> Value {
        match value {
            StateValue::Int(v) => Value::Int(*v),
            StateValue::Bool(b) => Value::Bool(*b),
            StateValue::Bytes(b) => Value::Bytes(b.clone()),
        }
    }
}

impl StateValue {
    /// The scalar a [`Value`] decoded from a [`FieldType`] always is.
    fn from_scalar(value: Value) -> Option<StateValue> {
        match value {
            Value::Int(v) => Some(StateValue::Int(v)),
            Value::Bool(b) => Some(StateValue::Bool(b)),
            Value::Bytes(b) => Some(StateValue::Bytes(b)),
            Value::Array(_) | Value::Record(_) => None,
        }
    }
}

fn typed(fields: &[(FieldType, StateValue)]) -> Vec<(Type, Value)> {
    fields.iter().map(|(t, v)| (Type::from(*t), Value::from(v))).collect()
}

/// Canonical state payload of one lowered field (§3.4.3/§3.4.4). `None` when the
/// value does not fit the type: wrong variant, wrong width, invalid UTF-8,
/// or an out-of-range int.
pub fn state_payload(ty: FieldType, value: &StateValue) -> Option<Vec<u8>> {
    payload(&ty.into(), &value.into()).ok()
}

/// Encode ordered state fields (§3.7.1): each field's canonical payload
/// wrapped in `PushExplicit`, concatenated in declaration order.
pub fn encode_state(fields: &[(FieldType, StateValue)]) -> Option<Vec<u8>> {
    encode_state_values(&typed(fields)).ok()
}

/// Decode an encoded state block against its declared field types (§3.7.1):
/// exactly one canonical `PushExplicit` per field, each payload validated
/// per type, trailing bytes rejected.
pub fn decode_state(types: &[FieldType], encoded: &[u8]) -> Option<Vec<StateValue>> {
    let types: Vec<Type> = types.iter().map(|&t| t.into()).collect();
    decode_state_values(&types, encoded)
        .ok()?
        .into_iter()
        .map(StateValue::from_scalar)
        .collect()
}

/// `Packed(value)` (§3.9.1): the fields' fixed payloads concatenated without
/// push opcodes. Defined only for layouts with a statically known packed
/// width, so the variable-width `bytes`/`string` types are `None`.
pub fn packed(fields: &[(FieldType, StateValue)]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (ty, value) in typed(fields) {
        ty.width()?;
        out.extend(payload(&ty, &value).ok()?);
    }
    Some(out)
}

/// Hash-committed virtual element (§3.9.1): `commitment = Hash(Packed(value))`,
/// stored in state as a `byte[32]` field.
pub fn commitment(payload: &[u8]) -> [u8; 32] {
    hash32(payload)
}

/// Verify an opening against a `byte[32]` commitment field (§3.9.1):
/// `Hash(payload) = commitment`. The opening is witness data, not encoded
/// state, and MUST be verified before the value is used.
pub fn verify_commitment(commitment: &[u8], payload: &[u8]) -> bool {
    commitment == hash32(payload).as_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> Vec<u8> {
        hex::decode(s).unwrap()
    }

    const STEP_SIGNATURE: &str = "step(int,byte[4],bool,byte)";

    // §3.4.2 worked contrast: PushMinimal(01) = OP_1, PushExplicit(01) = OP_DATA_1 01.
    #[test]
    fn push_explicit_never_uses_numeric_opcodes() {
        assert_eq!(encode_push(&[0x01]), vec![0x51]);
        assert_eq!(push_explicit(&[0x01]), vec![0x01, 0x01]);
        assert_eq!(push_explicit(&[]), vec![0x00]); // OP_0 is still the empty push
        assert_eq!(push_explicit(&[0x81]), vec![0x01, 0x81]); // not OP_1NEGATE
        assert_eq!(push_explicit(&[0xee; 76])[..2], [0x4c, 76]);
        assert_eq!(push_explicit(&vec![0xee; 0x100])[..3], [0x4d, 0x00, 0x01]);
    }

    #[test]
    fn read_push_explicit_round_trips_and_rejects_non_canonical() {
        for payload in [
            vec![],
            vec![0x01],
            vec![0x81],
            vec![0x07; 75],
            vec![0x07; 76],
            vec![0x07; 0x100],
        ] {
            let encoded = push_explicit(&payload);
            assert_eq!(
                read_push_explicit(&encoded),
                Some((payload.as_slice(), encoded.len()))
            );
        }
        assert_eq!(read_push_explicit(&[0x51]), None); // OP_1
        assert_eq!(read_push_explicit(&[0x4f]), None); // OP_1NEGATE
        assert_eq!(read_push_explicit(&[0x4c, 0x01, 0xaa]), None); // PUSHDATA1 over a 1-byte payload
        assert_eq!(read_push_explicit(&[0x4d, 0x01, 0x00, 0xaa]), None); // PUSHDATA2 under 256
        assert_eq!(read_push_explicit(&[0x02, 0xaa]), None); // truncated
        assert_eq!(read_push_explicit(&[]), None);
    }

    // §6.6
    #[test]
    fn vector_6_6_dispatch_tag() {
        assert_eq!(dispatch_tag(STEP_SIGNATURE), [0x2c, 0x49, 0xed, 0x65]);
    }

    // §6.6 — arguments (17, 01020304, true, 01) plus the dispatch-tag push.
    #[test]
    fn vector_6_6_argument_encoding() {
        let mut combined = Vec::new();
        combined.extend(encode_push(&encode_arg_int(17).unwrap())); // int 17 = 0111
        combined.extend(encode_push(&h("01020304"))); // byte[4]
        combined.push(0x51); // standalone bool true = OP_1 (§3.4.4)
        combined.extend(encode_push(&[0x01])); // byte 01 = OP_1
        combined.push(0x04); // OP_DATA_4 dispatch tag (§3.6)
        combined.extend(dispatch_tag(STEP_SIGNATURE));
        assert_eq!(hex::encode(&combined), "011104010203045151042c49ed65");
    }

    // §6.7 — one-byte program R = 51.
    #[test]
    fn vector_6_7_p2sh_envelope() {
        let program = [0x51];
        assert_eq!(
            hex::encode(envelope_spk(&program)),
            "aa20ce57216285125006ec18197bd8184221cefa559bb0798410d99a5bba5b07cd1d87"
        );
        // the bare reveal PushMinimal(R); the Draft printed it as the whole
        // signature script, which the Last Call text corrected to the line below
        assert_eq!(hex::encode(encode_push(&program)), "0151");
        // §6.6 arguments + tag ahead of the mandatory final PushMinimal(R)
        let args = h("011104010203045151");
        assert_eq!(
            hex::encode(signature_script(
                &args,
                &dispatch_tag(STEP_SIGNATURE),
                &program
            )),
            "011104010203045151042c49ed650151"
        );
    }

    // §6.8 — pubkey 07^32, int -5, bool true.
    const STATE_6_8: &str =
        "2007070707070707070707070707070707070707070707070707070707070707070805000000000000800101";
    const TYPES_6_8: [FieldType; 3] = [FieldType::PubKey, FieldType::Int, FieldType::Bool];

    #[test]
    fn vector_6_8_state_encoding() {
        assert_eq!(
            hex::encode(encode_state_int(-5).unwrap()),
            "0500000000000080"
        );
        let fields = [
            (FieldType::PubKey, StateValue::Bytes(vec![0x07; 32])),
            (FieldType::Int, StateValue::Int(-5)),
            (FieldType::Bool, StateValue::Bool(true)),
        ];
        assert_eq!(hex::encode(encode_state(&fields).unwrap()), STATE_6_8);
    }

    #[test]
    fn vector_6_8_state_decoding() {
        let encoded = h(STATE_6_8);
        assert_eq!(
            decode_state(&TYPES_6_8, &encoded),
            Some(vec![
                StateValue::Bytes(vec![0x07; 32]),
                StateValue::Int(-5),
                StateValue::Bool(true),
            ])
        );
        // §3.7.1 rejections: trailing bytes, missing fields, wrong widths
        let mut trailing = encoded.clone();
        trailing.push(0x00);
        assert_eq!(decode_state(&TYPES_6_8, &trailing), None);
        assert_eq!(
            decode_state(&TYPES_6_8, &encoded[..encoded.len() - 2]),
            None
        );
        assert_eq!(
            decode_state(&[FieldType::Sig, FieldType::Int, FieldType::Bool], &encoded),
            None
        );
    }

    // §6.9
    #[test]
    fn vector_6_9_template_hashes() {
        let rows: &[(&str, &str, &str)] = &[
            (
                "",
                "",
                "e572dff82304700b856a555ac3a4558d0df3646a3727816500270a93c66aac1e",
            ),
            (
                "61",
                "6263",
                "405e183e2494cdbe2df89349cc0ffa5b77fb885ad97a1d5660ecd0692ef8142a",
            ),
            (
                "6162",
                "63",
                "a0968c014f3fc7bd1a7d9a8d1ad1177eb379bd2f05e56309eb4e20347c5e7eba",
            ),
            (
                "00ff",
                "100080",
                "6616a66757315de0221cb2acba729113cebde31f8d3ca7fa93878a0584b96905",
            ),
        ];
        for (prefix, suffix, want) in rows {
            assert_eq!(
                hex::encode(template_hash(&h(prefix), &h(suffix))),
                *want,
                "prefix={prefix} suffix={suffix}"
            );
        }
        // rows 2 and 3 concatenate identically; the LE64 length fields split them
        assert_ne!(
            template_hash(&h("61"), &h("6263")),
            template_hash(&h("6162"), &h("63"))
        );
    }

    // §6.10 — R = 5102aabb010102ccdd75, state.start = 1, state.len = 8.
    #[test]
    fn vector_6_10_template_views() {
        let r = h("5102aabb010102ccdd75");
        // fields byte[2] a = aabb, bool b = true, byte[2] c = ccdd
        assert_eq!(
            decode_state(
                &[
                    FieldType::FixedBytes(2),
                    FieldType::Bool,
                    FieldType::FixedBytes(2)
                ],
                &r[1..9]
            ),
            Some(vec![
                StateValue::Bytes(h("aabb")),
                StateValue::Bool(true),
                StateValue::Bytes(h("ccdd")),
            ])
        );
        let views: &[(usize, usize, &str, &str, &str, &str)] = &[
            // (view.start, view.len, prefix, encoded_state, suffix, hash)
            (
                1,
                5,
                "51",
                "02aabb0101",
                "02ccdd75",
                "2e2c28131760a1c0942842f8b54fe686321d0080f5161fa803e27af6a15799a4",
            ),
            (
                4,
                5,
                "5102aabb",
                "010102ccdd",
                "75",
                "0eb10580bcb608ab9efab6e256d4bac6671d724a72951a058d37b42fa205c1ee",
            ),
            (
                4,
                2,
                "5102aabb",
                "0101",
                "02ccdd75",
                "7b96ebb8023373bfe7aa81f7db8e0e0d9ce492d2b0382fb5586ac60beea8ed12",
            ),
        ];
        for (start, len, want_prefix, want_state, want_suffix, want_hash) in views {
            let (prefix, rest) = r.split_at(*start);
            let (encoded_state, suffix) = rest.split_at(*len);
            assert_eq!(hex::encode(prefix), *want_prefix);
            assert_eq!(hex::encode(encoded_state), *want_state);
            assert_eq!(hex::encode(suffix), *want_suffix);
            assert_eq!(
                hex::encode(template_hash(prefix, suffix)),
                *want_hash,
                "view [{start}, {})",
                start + len
            );
        }
    }

    // §6.12 — fixed record (int -5, bool true).
    #[test]
    fn vector_6_12_hash_committed_virtual_element() {
        let fields = [
            (FieldType::Int, StateValue::Int(-5)),
            (FieldType::Bool, StateValue::Bool(true)),
        ];
        let payload = packed(&fields).unwrap();
        assert_eq!(hex::encode(&payload), "050000000000008001");
        let want = h("15e006c7c506fb20b6de9573e31bdc47591e937c38f9fcf31cdfabe55d122bda");
        assert_eq!(commitment(&payload).as_slice(), want.as_slice());
        assert!(verify_commitment(&want, &payload));
        assert!(!verify_commitment(&want, &payload[..payload.len() - 1]));
        assert!(!verify_commitment(&want[..31], &payload));
        // Packed is undefined for variable-width layouts (§3.9.1)
        assert_eq!(
            packed(&[(FieldType::Bytes, StateValue::Bytes(vec![0x01]))]),
            None
        );
    }

    #[test]
    fn state_int_codec_edges() {
        for v in [0i64, 1, -1, 127, -128, i64::MAX, -i64::MAX] {
            assert_eq!(
                decode_state_int(&encode_state_int(v).unwrap()),
                Some(v),
                "{v}"
            );
        }
        assert_eq!(
            hex::encode(encode_state_int(i64::MAX).unwrap()),
            "ffffffffffffff7f"
        );
        assert_eq!(
            hex::encode(encode_state_int(-i64::MAX).unwrap()),
            "ffffffffffffffff"
        );
        assert_eq!(encode_state_int(i64::MIN), None); // -2^63 is outside the §3.4.3 range
        assert_eq!(decode_state_int(&[0, 0, 0, 0, 0, 0, 0, 0x80]), None); // negative zero
    }

    #[test]
    fn arg_int_codec_is_minimal_and_signed() {
        assert_eq!(encode_arg_int(17).unwrap(), vec![0x11]); // §6.6
        assert_eq!(encode_arg_int(0).unwrap(), Vec::<u8>::new());
        assert_eq!(encode_arg_int(-1).unwrap(), vec![0x81]);
        assert_eq!(encode_arg_int(-5).unwrap(), vec![0x85]);
        assert_eq!(encode_arg_int(128).unwrap(), vec![0x80, 0x00]);
        assert_eq!(encode_arg_int(-128).unwrap(), vec![0x80, 0x80]);
        assert_eq!(encode_arg_int(i64::MIN), None);
        // matches the crate root's snum on its non-negative domain
        for v in [0i64, 1, 6, 17, 127, 128, 32767, 100_000_000] {
            assert_eq!(encode_arg_int(v).unwrap(), crate::snum(v));
        }
        for v in [
            0i64,
            1,
            -1,
            17,
            -5,
            127,
            -127,
            128,
            -128,
            32767,
            -32768,
            i64::MAX,
            -i64::MAX,
        ] {
            assert_eq!(decode_arg_int(&encode_arg_int(v).unwrap()), Some(v), "{v}");
        }
        assert_eq!(decode_arg_int(&[0x05, 0x00]), None); // padded
        assert_eq!(decode_arg_int(&[0x00]), None); // padded zero
        assert_eq!(decode_arg_int(&[0x80]), None); // negative zero
        assert_eq!(decode_arg_int(&[0, 0, 0, 0, 0, 0, 0, 0x80, 0x00]), None); // 2^63 is out of range
    }

    #[test]
    fn read_push_minimal_round_trips_and_rejects_non_minimal() {
        for payload in [
            vec![],
            vec![0x01],
            vec![0x10],
            vec![0x11],
            vec![0x81],
            vec![0x07; 75],
            vec![0x07; 76],
            vec![0x07; 0x100],
        ] {
            let encoded = encode_push(&payload);
            assert_eq!(
                read_push_minimal(&encoded),
                Some((payload.clone(), encoded.len()))
            );
        }
        // data forms of values the numeric opcodes own are non-minimal
        assert_eq!(read_push_minimal(&[0x01, 0x05]), None); // OP_5's value
        assert_eq!(read_push_minimal(&[0x01, 0x81]), None); // OP_1NEGATE's value
        assert_eq!(read_push_minimal(&[0x4c, 0x02, 0xaa, 0xbb]), None); // PUSHDATA1 under 76
        assert_eq!(read_push_minimal(&[0xac]), None); // OpCheckSig is not a push
        assert_eq!(read_push_minimal(&[0x02, 0xaa]), None); // truncated
        assert_eq!(read_push_minimal(&[]), None);
    }

    // §6.6 + §6.7 read back: the reader inverts signature_script exactly.
    #[test]
    fn read_invocation_inverts_the_6_7_vector() {
        let sig = h("011104010203045151042c49ed650151");
        let inv = read_invocation(&sig).expect("the spec's own vector must read");
        assert_eq!(inv.program, vec![0x51]);
        assert_eq!(inv.dispatch, dispatch_tag(STEP_SIGNATURE));
        assert_eq!(
            inv.args,
            vec![vec![0x11], h("01020304"), vec![0x01], vec![0x01]]
        );
        // rebuilding from the parts reproduces the input byte for byte
        let mut args = Vec::new();
        for a in &inv.args {
            args.extend(encode_push(a));
        }
        assert_eq!(signature_script(&args, &inv.dispatch, &inv.program), sig);
    }

    #[test]
    fn read_invocation_fails_closed() {
        // the tag is mandatory: a bare program reveal is not an invocation
        assert_eq!(read_invocation(&h("0151")), None);
        // the tag must be exactly four bytes, directly before the program
        assert_eq!(read_invocation(&h("03aabbcc0151")), None);
        assert_eq!(read_invocation(&h("05aabbccddee0151")), None);
        // a tag with no arguments is the minimal invocation
        let inv = read_invocation(&h("042c49ed650151")).unwrap();
        assert_eq!(inv.program, vec![0x51]);
        assert!(inv.args.is_empty());
        // a non-push opcode anywhere refuses the whole script
        assert_eq!(read_invocation(&h("ac042c49ed650151")), None);
        // an empty revealed program is no reveal
        assert_eq!(read_invocation(&h("042c49ed6500")), None);
        assert_eq!(read_invocation(&[]), None);
        // a non-minimal argument push refuses (§3.4.2 is byte-exact)
        assert_eq!(read_invocation(&h("0105042c49ed650151")), None);
    }

    #[test]
    fn read_invocation_checked_requires_the_envelope() {
        let program = h("5102aabb010102ccdd75");
        let sig = signature_script(&[], &dispatch_tag(STEP_SIGNATURE), &program);
        let spk = envelope_spk(&program);
        let inv = read_invocation_checked(&spk, &sig).expect("matching envelope");
        assert_eq!(inv.program, program);
        // any other envelope refuses the claimed program
        assert_eq!(read_invocation_checked(&envelope_spk(&[0x51]), &sig), None);
    }

    // Shapes the §6 vectors do not reach: an array of fixed record arrays,
    // records in state, and the names Type::parse must refuse.
    #[test]
    fn nested_record_arrays_round_trip_and_bad_names_refuse() {
        let decls = [
            RecordDecl {
                name: "Point".into(),
                fields: vec![("id".into(), "byte[2]".into()), ("on".into(), "bool".into())],
            },
            RecordDecl {
                name: "Pair".into(),
                fields: vec![("count".into(), "int".into()), ("label".into(), "string".into())],
            },
        ];
        let point = |id: [u8; 2], on: bool| Value::Record(vec![Value::Bytes(id.to_vec()), Value::Bool(on)]);
        // Point[2][]: elements are Point[2], so each leaf groups as byte[2][2][] / bool[2][]
        let ty = Type::parse("Point[2][]", &decls).unwrap();
        let value = Value::Array(vec![
            Value::Array(vec![point([1, 2], true), point([3, 4], false)]),
            Value::Array(vec![point([5, 6], false), point([7, 8], true)]),
        ]);
        let pushes = arg_pushes(&ty, &value).unwrap();
        assert_eq!(pushes, vec![h("080102030405060708"), h("0401000001")]);
        let encoded = encode_args(&[(ty.clone(), value.clone())]).unwrap();
        assert_eq!(decode_args(std::slice::from_ref(&ty), &encoded), Ok(vec![value.clone()]));
        // the same lowering in state, each group one PushExplicit
        let state = encode_state_values(&[(ty.clone(), value.clone())]).unwrap();
        assert_eq!(decode_state_values(&[ty], &state), Ok(vec![value]));
        // a record outside an array may hold a string: each leaf its own push
        let pair = Type::parse("Pair", &decls).unwrap();
        let v = Value::Record(vec![Value::Int(5), Value::Bytes(b"Kaspa".to_vec())]);
        let state = encode_state_values(&[(pair.clone(), v.clone())]).unwrap();
        assert_eq!(hex::encode(&state), "080500000000000000054b61737061");
        assert_eq!(decode_state_values(std::slice::from_ref(&pair), &state), Ok(vec![v]));
        assert_eq!(pair.dispatch_type_name(), "{int,string}");
        // ...but not inside one
        assert!(validate_type(&Type::parse("Pair[]", &decls).unwrap()).is_err());

        let self_ref = [RecordDecl {
            name: "Loop".into(),
            fields: vec![("next".into(), "Loop".into())],
        }];
        assert_eq!(Type::parse("Loop", &self_ref), Err("record Loop contains itself".into()));
        for bad in ["int[01]", "int[", "int[]x", "[3]", "Nope", "int[-1]"] {
            assert!(Type::parse(bad, &decls).is_err(), "{bad}");
        }
        assert_eq!(Type::parse("byte[0]", &[]), Ok(Type::Array(Box::new(Type::Byte), Some(0))));
        let twice = [decls[0].clone(), decls[0].clone()];
        assert_eq!(Type::parse("int", &twice), Err("record Point is declared twice".into()));
        let scalar_name = [RecordDecl {
            name: "int".into(),
            fields: vec![],
        }];
        assert!(Type::parse("int", &scalar_name).is_err());
        assert_eq!(parse_int("-0"), Ok(0));
        assert!(parse_int("+5").is_err());
    }

    // §3.2.1 keyed form and the Draft KCC-2 §3 P2PKHHash. §6.1 now ships a
    // keyed vector (run in tests/kcc1_vectors.rs); these pin the P2PKH key
    // bytes and Key32 zero-padding so a refactor cannot drift silently.
    #[test]
    fn keyed_hash_and_p2pkh_hash() {
        assert_eq!(
            hex::encode(p2pkh_hash(&[0x07; 32])),
            "494c1139020c5eb8c60691c12471d36a5bdf90d368d3f0f310e13d82dc321c15"
        );
        // Key32 pads with zero bytes, so the padded key spelled out agrees
        let mut key32 = b"PublicKeyHash".to_vec();
        key32.resize(32, 0);
        assert_eq!(hash_keyed(&[0x07; 32], &key32), Some(p2pkh_hash(&[0x07; 32])));
        // a different key is a different function
        assert_ne!(hash_keyed(&[0x07; 32], b"x"), hash_keyed(&[0x07; 32], b"y"));
        // and the keyed form is not the unkeyed one
        assert_ne!(hash_keyed(&[0x07; 32], b"PublicKeyHash"), Some(hash(&[0x07; 32])));
        // the spec defines keys of at most 32 bytes
        assert_eq!(hash_keyed(b"", &[0u8; 33]), None);
    }

}
