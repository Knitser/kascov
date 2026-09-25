//! KCC-0001 conformance primitives — byte layouts and hash derivations from
//! "Covenant definition, concepts, bytes layout and ABI" (kaspanet/kccs,
//! merged Draft at commit ea5176aa). Section numbers in doc comments refer
//! to that text. Invocation arguments use PushMinimal, which is `encode_push`
//! in the crate root; only the KCC1-specific encodings live here.
//!
//! Two hash functions, and the split is the spec's, not ours (§3.1, §7):
//! `Hash` is BLAKE3-256 and names things (dispatch tags, template hashes,
//! virtual-element commitments, KCC-2's keyed P2PKHHash); the P2SH envelope
//! alone uses BLAKE2b, because that is what `OP_BLAKE2B` computes on chain.
//! The pre-merge draft (55b28d8) used BLAKE2b for both; every identity this
//! module produced under it is a different number now. The store keeps those
//! old names resolving as aliases so published links keep working, but the
//! canonical name of a template is the BLAKE3 one.

use crate::encode_push;

/// Unkeyed BLAKE3 with 32-byte output — the spec's `Hash(x)` (§3.1).
pub fn hash(input: &[u8]) -> [u8; 32] {
    *blake3::hash(input).as_bytes()
}

fn hash32(input: &[u8]) -> [u8; 32] {
    hash(input)
}

/// Keyed `Hash(x, key)` (§3.1): BLAKE3 keyed mode under
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
pub fn p2pkh_hash(pubkey: &[u8]) -> [u8; 32] {
    hash_keyed(pubkey, b"PublicKeyHash").expect("13-byte key")
}

/// `PushExplicit(b)` (§5.2): `OP_0` for the empty payload; the length-based
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

/// Decode exactly one `PushExplicit` at the start of `script` (§5.2), giving
/// the payload and the bytes consumed. §8.1 requires the consumed bytes to
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

/// Eight-byte little-endian signed-magnitude `int` state payload (§5.3/§5.4):
/// magnitude in the low 63 bits, sign in the top bit of the last byte.
/// `None` for `i64::MIN` — the §5.3 range is symmetric and its magnitude
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
/// two encodings, breaking §8.1's byte-exactness requirement.
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

/// Minimal ScriptNum for an `int` invocation argument (§5.3): little-endian
/// magnitude, sign carried by the top bit of the last byte, one extension
/// byte only when the magnitude's own top bit is set. The crate root's
/// `snum` is documented non-negative-only; this codec also covers negative
/// values. `None` for `i64::MIN` (out of the §5.3 range).
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
/// ScriptNum of a value in the §5.3 range — padded encodings, negative
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

/// Dispatch tag (§6.1): the first four bytes of `Hash(UTF8(signature))`,
/// where `signature` is `"{name}({comma-separated dispatch type names})"`
/// with no whitespace — e.g. `"step(int,byte[4],bool,byte)"`.
///
/// `DispatchTypeName(T)` is the spec's recursive rule, and the caller owes
/// this function a signature already written under it: a scalar is its
/// `TypeName`; a fixed array of `T` with length `N` is
/// `DispatchTypeName(T)[N]`; a dynamic array is `DispatchTypeName(T)[]`; a
/// record is `{DispatchTypeName(T_1),...,DispatchTypeName(T_n)}` over its
/// ordered field types, with the record's name and its field names
/// omitted. So `transfer(KCC20State[],byte[])` is not a §6.1 signature;
/// `transfer({int,byte[32],byte,byte,byte[32],byte[32]}[],byte[])` is. The
/// silverscript 1.0 compiler writes its tags under the same rule
/// (`write_dispatch_type_name`, compile/helpers.rs at 3ed9733).
pub fn dispatch_tag(signature: &str) -> [u8; 4] {
    let mut tag = [0u8; 4];
    tag.copy_from_slice(&hash32(signature.as_bytes())[..4]);
    tag
}

/// `TemplateHash(prefix, suffix)` (§8.3):
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

/// Version-0 P2SH script public key committing to `program` (§7):
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

/// Signature script spending a KCC1 P2SH output (§7): the pre-encoded
/// argument pushes, then `OP_DATA_4 dispatch_tag` (mandatory for every
/// program since the merge, single-entrypoint ones included), then
/// `PushMinimal(R)` as the mandatory final push.
pub fn signature_script(arg_pushes: &[u8], dispatch: &[u8; 4], program: &[u8]) -> Vec<u8> {
    let mut out = arg_pushes.to_vec();
    out.push(0x04);
    out.extend_from_slice(dispatch);
    out.extend_from_slice(&encode_push(program));
    out
}

/// Decode exactly one `PushMinimal` at the start of `script` (§5.2), giving
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

/// A KCC1 invocation read back from the signature script that spends a
/// version-0 P2SH envelope (§7): argument pushes, then the dispatch tag,
/// then the revealed program as the mandatory final push. The inverse of
/// [`signature_script`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    /// Argument push payloads, in push order.
    pub args: Vec<Vec<u8>>,
    /// The four dispatch-tag bytes (§6.1).
    pub dispatch: [u8; 4],
    /// The revealed program `R`. Claimed, not yet proven: check it against
    /// the spent envelope ([`read_invocation_checked`]) before trusting it.
    pub program: Vec<u8>,
}

/// Read a §7 signature script: every push minimal, the tag exactly four
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
    let mut pushes = Vec::new();
    let mut at = 0usize;
    while at < sig_script.len() {
        let (payload, consumed) = read_push_minimal(&sig_script[at..])?;
        pushes.push(payload);
        at += consumed;
    }
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
/// to the envelope it spends (§7): `envelope_spk(program) == spk`. The only
/// form whose `program` is proven rather than claimed.
pub fn read_invocation_checked(spk: &[u8], sig_script: &[u8]) -> Option<Invocation> {
    let inv = read_invocation(sig_script)?;
    (envelope_spk(&inv.program) == spk).then_some(inv)
}

/// Scalar field types with a defined state lowering (§5.1/§5.4). Arrays and
/// records lower to sequences of these before encoding (§5.5/§5.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    Int,
    Bool,
    Byte,
    /// Variable byte string (`bytes`); no fixed width, so invalid in packed
    /// virtual-element payloads (§10.1).
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

/// Canonical state payload of one lowered field (§5.3/§5.4). `None` when the
/// value does not fit the type: wrong variant, wrong width, invalid UTF-8,
/// or an out-of-range int.
pub fn state_payload(ty: FieldType, value: &StateValue) -> Option<Vec<u8>> {
    match (ty, value) {
        (FieldType::Int, StateValue::Int(v)) => Some(encode_state_int(*v)?.to_vec()),
        (FieldType::Bool, StateValue::Bool(b)) => Some(vec![*b as u8]),
        (_, StateValue::Bytes(b)) => {
            let ok = match ty {
                FieldType::Byte => b.len() == 1,
                FieldType::Bytes => true,
                FieldType::String => std::str::from_utf8(b).is_ok(),
                FieldType::PubKey => b.len() == 32,
                FieldType::Sig => b.len() == 65,
                FieldType::DataSig => b.len() == 64,
                FieldType::FixedBytes(n) => b.len() == n,
                FieldType::Int | FieldType::Bool => false,
            };
            ok.then(|| b.clone())
        }
        _ => None,
    }
}

/// Encode ordered state fields (§8.1): each field's canonical payload
/// wrapped in `PushExplicit`, concatenated in declaration order.
pub fn encode_state(fields: &[(FieldType, StateValue)]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (ty, value) in fields {
        out.extend_from_slice(&push_explicit(&state_payload(*ty, value)?));
    }
    Some(out)
}

/// Decode an encoded state block against its declared field types (§8.1):
/// exactly one canonical `PushExplicit` per field, each payload validated
/// per type, trailing bytes rejected.
pub fn decode_state(types: &[FieldType], encoded: &[u8]) -> Option<Vec<StateValue>> {
    let mut at = 0;
    let mut values = Vec::with_capacity(types.len());
    for &ty in types {
        let (payload, consumed) = read_push_explicit(&encoded[at..])?;
        values.push(decode_payload(ty, payload)?);
        at += consumed;
    }
    (at == encoded.len()).then_some(values)
}

fn decode_payload(ty: FieldType, payload: &[u8]) -> Option<StateValue> {
    match ty {
        FieldType::Int => Some(StateValue::Int(decode_state_int(payload.try_into().ok()?)?)),
        FieldType::Bool => match payload {
            [0x00] => Some(StateValue::Bool(false)),
            [0x01] => Some(StateValue::Bool(true)),
            _ => None,
        },
        _ => {
            // width and content rules are the encoder's, re-checked in reverse
            let value = StateValue::Bytes(payload.to_vec());
            state_payload(ty, &value).map(|_| value)
        }
    }
}

/// `Packed(value)` (§10.1): the fields' fixed payloads concatenated without
/// push opcodes. Defined only for layouts with a statically known packed
/// width, so the variable-width `bytes`/`string` types are `None`.
pub fn packed(fields: &[(FieldType, StateValue)]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (ty, value) in fields {
        if matches!(ty, FieldType::Bytes | FieldType::String) {
            return None;
        }
        out.extend_from_slice(&state_payload(*ty, value)?);
    }
    Some(out)
}

/// Hash-committed virtual element (§10.1): `commitment = Hash(Packed(value))`,
/// stored in state as a `byte[32]` field.
pub fn commitment(payload: &[u8]) -> [u8; 32] {
    hash32(payload)
}

/// Verify an opening against a `byte[32]` commitment field (§10.1):
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

    // §5.2 worked contrast: PushMinimal(01) = OP_1, PushExplicit(01) = OP_DATA_1 01.
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

    // §11.1
    #[test]
    fn vector_11_1_dispatch_tag() {
        assert_eq!(dispatch_tag(STEP_SIGNATURE), [0x2c, 0x49, 0xed, 0x65]);
    }

    // §11.1 — arguments (17, 01020304, true, 01) plus the dispatch-tag push.
    #[test]
    fn vector_11_1_argument_encoding() {
        let mut combined = Vec::new();
        combined.extend(encode_push(&encode_arg_int(17).unwrap())); // int 17 = 0111
        combined.extend(encode_push(&h("01020304"))); // byte[4]
        combined.push(0x51); // standalone bool true = OP_1 (§5.4)
        combined.extend(encode_push(&[0x01])); // byte 01 = OP_1
        combined.push(0x04); // OP_DATA_4 dispatch tag (§7)
        combined.extend(dispatch_tag(STEP_SIGNATURE));
        assert_eq!(hex::encode(&combined), "011104010203045151042c49ed65");
    }

    // §11.2 — one-byte program R = 51.
    #[test]
    fn vector_11_2_p2sh_envelope() {
        let program = [0x51];
        assert_eq!(
            hex::encode(envelope_spk(&program)),
            "aa20ce57216285125006ec18197bd8184221cefa559bb0798410d99a5bba5b07cd1d87"
        );
        // the spec's `signature_script = 0151` is the bare envelope reveal
        assert_eq!(hex::encode(encode_push(&program)), "0151");
        // §11.1 arguments + tag ahead of the mandatory final PushMinimal(R)
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

    // §11.3 — pubkey 07^32, int -5, bool true.
    const STATE_11_3: &str =
        "2007070707070707070707070707070707070707070707070707070707070707070805000000000000800101";
    const TYPES_11_3: [FieldType; 3] = [FieldType::PubKey, FieldType::Int, FieldType::Bool];

    #[test]
    fn vector_11_3_state_encoding() {
        assert_eq!(
            hex::encode(encode_state_int(-5).unwrap()),
            "0500000000000080"
        );
        let fields = [
            (FieldType::PubKey, StateValue::Bytes(vec![0x07; 32])),
            (FieldType::Int, StateValue::Int(-5)),
            (FieldType::Bool, StateValue::Bool(true)),
        ];
        assert_eq!(hex::encode(encode_state(&fields).unwrap()), STATE_11_3);
    }

    #[test]
    fn vector_11_3_state_decoding() {
        let encoded = h(STATE_11_3);
        assert_eq!(
            decode_state(&TYPES_11_3, &encoded),
            Some(vec![
                StateValue::Bytes(vec![0x07; 32]),
                StateValue::Int(-5),
                StateValue::Bool(true),
            ])
        );
        // §8.1 rejections: trailing bytes, missing fields, wrong widths
        let mut trailing = encoded.clone();
        trailing.push(0x00);
        assert_eq!(decode_state(&TYPES_11_3, &trailing), None);
        assert_eq!(
            decode_state(&TYPES_11_3, &encoded[..encoded.len() - 2]),
            None
        );
        assert_eq!(
            decode_state(&[FieldType::Sig, FieldType::Int, FieldType::Bool], &encoded),
            None
        );
    }

    // §11.4
    #[test]
    fn vector_11_4_template_hashes() {
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

    // §11.5 — R = 5102aabb010102ccdd75, state.start = 1, state.len = 8.
    #[test]
    fn vector_11_5_template_views() {
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

    // §11.6 — fixed record (int -5, bool true).
    #[test]
    fn vector_11_6_hash_committed_virtual_element() {
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
        // Packed is undefined for variable-width layouts (§10.1)
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
        assert_eq!(encode_state_int(i64::MIN), None); // -2^63 is outside the §5.3 range
        assert_eq!(decode_state_int(&[0, 0, 0, 0, 0, 0, 0, 0x80]), None); // negative zero
    }

    #[test]
    fn arg_int_codec_is_minimal_and_signed() {
        assert_eq!(encode_arg_int(17).unwrap(), vec![0x11]); // §11.1
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

    // §11.1 + §11.2 read back: the reader inverts signature_script exactly.
    #[test]
    fn read_invocation_inverts_the_11_2_vector() {
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
        // a non-minimal argument push refuses (§5.2 is byte-exact)
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

    // §3.1 keyed form and KCC-2 §3 P2PKHHash. The spec ships no vector for
    // the keyed form; these pin kascov's derivation (Key32 zero-padding and
    // the exact key bytes) so a refactor cannot drift silently.
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
