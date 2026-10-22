//! The official KCC-1 conformance vectors (kaspanet/kccs 1ed87ae, Final,
//! the file unchanged since 1516a6c,
//! kcc-0001/vectors/conformance.json, vendored unmodified), every one run
//! through `kcc1`'s own functions. The glue below only turns the file's JSON
//! into `kcc1` types and values; each operation a vector names (encode,
//! decode, validate, hash, cut) is a library call, so a pass here is a pass
//! by the code kascov serves from.
//!
//! The run is all or nothing on purpose: the file is the spec's
//! authoritative form (§6), and a reader that passed most of it would still
//! accept something the spec refuses.

use kascov_decode::encode_push;
use kascov_decode::kcc1::{self, RecordDecl, Type, Value};
use serde_json::Value as Json;

const VECTORS: &str = include_str!("fixtures/kccs/kcc-0001/conformance.json");

fn h(text: &str) -> Vec<u8> {
    hex::decode(text).unwrap()
}

/// A byte string in either of the file's forms: `{"hex"}` or
/// `{"repeat_hex", "count"}`.
fn bytes_of(spec: &Json) -> Vec<u8> {
    match spec["hex"].as_str() {
        Some(text) => h(text),
        None => h(spec["repeat_hex"].as_str().unwrap()).repeat(spec["count"].as_u64().unwrap() as usize),
    }
}

/// An encoding in either form: literal hex, or a prefix before the payload.
fn encoding_of(spec: &Json, payload: &[u8]) -> Vec<u8> {
    match spec["hex"].as_str() {
        Some(text) => h(text),
        None => {
            assert_eq!(spec["payload"], "unchanged");
            let mut out = h(spec["encoded_prefix_hex"].as_str().unwrap());
            out.extend_from_slice(payload);
            out
        }
    }
}

fn decls_of(records: &[Json]) -> Vec<RecordDecl> {
    records
        .iter()
        .map(|r| RecordDecl {
            name: r["name"].as_str().unwrap().to_string(),
            fields: r["fields"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| (f["name"].as_str().unwrap().to_string(), f["type"].as_str().unwrap().to_string()))
                .collect(),
        })
        .collect()
}

/// The record definitions a vector carries, under `records` or a single
/// `record`.
fn vector_decls(v: &Json) -> Vec<RecordDecl> {
    let mut decls = decls_of(v["records"].as_array().map(Vec::as_slice).unwrap_or_default());
    if v["record"].is_object() {
        decls.extend(decls_of(std::slice::from_ref(&v["record"])));
    }
    decls
}

/// A JSON value as the file writes it, read as a value of `ty`: integers as
/// numbers or exact decimal strings, byte strings as hex, records as objects
/// keyed by field name (with `_hex` on byte-string fields).
fn value_of(ty: &Type, json: &Json) -> Value {
    match ty {
        Type::Int => Value::Int(match json {
            Json::String(decimal) => kcc1::parse_int(decimal).unwrap(),
            n => n.as_i64().unwrap(),
        }),
        Type::Bool => Value::Bool(json.as_bool().unwrap()),
        Type::String => Value::Bytes(json.as_str().unwrap().as_bytes().to_vec()),
        Type::Byte | Type::PubKey | Type::Sig | Type::DataSig => Value::Bytes(h(json.as_str().unwrap())),
        Type::Array(elem, _) if **elem == Type::Byte => Value::Bytes(h(json.as_str().unwrap())),
        Type::Array(elem, _) => Value::Array(json.as_array().unwrap().iter().map(|j| value_of(elem, j)).collect()),
        Type::Record(r) => Value::Record(
            r.fields
                .iter()
                .map(|(name, t)| {
                    let j = json.get(name).or_else(|| json.get(format!("{name}_hex"))).unwrap();
                    value_of(t, j)
                })
                .collect(),
        ),
    }
}

/// A vector's value: `value`, `value_decimal` or `value_hex`.
fn vector_value(ty: &Type, v: &Json) -> Value {
    ["value", "value_decimal", "value_hex"]
        .iter()
        .find(|k| !v[**k].is_null())
        .map(|k| value_of(ty, &v[*k]))
        .unwrap_or_else(|| panic!("no value in {v}"))
}

fn parse(text: &str, decls: &[RecordDecl]) -> Type {
    Type::parse(text, decls).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn types_of(v: &Json, decls: &[RecordDecl]) -> Vec<Type> {
    v["argument_types"].as_array().unwrap().iter().map(|t| parse(t.as_str().unwrap(), decls)).collect()
}

fn fields_of(fields: &Json) -> Vec<(String, Type)> {
    fields
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["name"].as_str().unwrap().to_string(), parse(f["type"].as_str().unwrap(), &[])))
        .collect()
}

fn eq<T: PartialEq + std::fmt::Debug>(what: &str, got: T, want: T) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: got {got:?}, want {want:?}"))
    }
}

/// Every vector as `(id, outcome)`; `Err` says what went wrong, so a failing
/// run names each failure instead of stopping at the first.
#[derive(Default)]
struct Run {
    results: Vec<(String, Result<(), String>)>,
}

impl Run {
    fn check(&mut self, id: impl Into<String>, outcome: impl FnOnce() -> Result<(), String>) {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(outcome)).unwrap_or_else(|p| {
            Err(p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panicked".into()))
        });
        self.results.push((id.into(), outcome));
    }
}

fn hash_function(run: &mut Run, doc: &Json) {
    for v in doc["hash_function"].as_array().unwrap() {
        run.check(format!("hash_function/{}", v["id"].as_str().unwrap()), || {
            let input = h(v["input_hex"].as_str().unwrap());
            let want = h(v["hash_hex"].as_str().unwrap());
            let Some(key) = v["key_hex"].as_str() else {
                return eq("Hash(x)", kcc1::hash(&input).to_vec(), want);
            };
            eq("Hash(x, key)", kcc1::hash_keyed(&input, &h(key)).map(|d| d.to_vec()), Some(want.clone()))?;
            // Key32 is the zero-padded key: keying with it spelled out is
            // the same function
            let key32 = h(v["key32_hex"].as_str().unwrap());
            eq("Hash(x, Key32(key))", kcc1::hash_keyed(&input, &key32).map(|d| d.to_vec()), Some(want))
        });
    }
}

fn canonical_type_names(run: &mut Run, doc: &Json) {
    // the file names a record Transfer without defining it; any definition
    // spells the same TypeName
    let decls = [RecordDecl {
        name: "Transfer".into(),
        fields: vec![("amount".into(), "int".into())],
    }];
    for v in doc["canonical_type_names"].as_array().unwrap() {
        let described = v["type"].as_str().unwrap();
        run.check(format!("canonical_type_names/{described}"), || {
            // the type each description names, built by hand, then spelled
            let ty = match described {
                "integer" => Type::Int,
                "boolean" => Type::Bool,
                "byte" => Type::Byte,
                "UTF-8 string" => Type::String,
                "32-byte public key" => Type::PubKey,
                "65-byte transaction signature" => Type::Sig,
                "64-byte data signature" => Type::DataSig,
                "fixed byte string of length 32" => Type::Array(Box::new(Type::Byte), Some(32)),
                "fixed array of three booleans" => Type::Array(Box::new(Type::Bool), Some(3)),
                "dynamic array of integers" => Type::Array(Box::new(Type::Int), None),
                "record named Transfer" => Type::Record(kcc1::Record {
                    name: "Transfer".into(),
                    fields: vec![("amount".into(), Type::Int)],
                }),
                other => return Err(format!("no type for {other:?}")),
            };
            let name = v["type_name"].as_str().unwrap();
            eq("TypeName", ty.type_name().as_str(), name)?;
            eq("parse(TypeName)", Type::parse(name, &decls), Ok(ty))
        });
    }
}

fn data_pushes(run: &mut Run, doc: &Json) {
    for v in doc["data_pushes"].as_array().unwrap() {
        run.check(format!("data_pushes/{}", v["id"].as_str().unwrap()), || {
            let payload = bytes_of(&v["payload"]);
            let minimal = encoding_of(&v["push_minimal"], &payload);
            let explicit = encoding_of(&v["push_explicit"], &payload);
            eq("PushMinimal", encode_push(&payload), minimal.clone())?;
            eq("PushExplicit", kcc1::push_explicit(&payload), explicit.clone())?;
            eq("read PushMinimal", kcc1::read_push_minimal(&minimal), Some((payload.clone(), minimal.len())))?;
            eq("read PushExplicit", kcc1::read_push_explicit(&explicit), Some((payload.as_slice(), explicit.len())))
        });
    }
}

fn scalar_encodings(run: &mut Run, doc: &Json) {
    for (i, v) in doc["scalar_encodings"].as_array().unwrap().iter().enumerate() {
        run.check(format!("scalar_encodings/{i}-{}", v["type"].as_str().unwrap()), || {
            let ty = parse(v["type"].as_str().unwrap(), &[]);
            let value = vector_value(&ty, v);
            let arg = h(v["standalone_argument_hex"].as_str().unwrap());
            let state = h(v["state_payload_hex"].as_str().unwrap());
            eq("standalone argument", kcc1::encode_args(&[(ty.clone(), value.clone())]), Ok(arg.clone()))?;
            eq("argument read back", kcc1::decode_args(std::slice::from_ref(&ty), &arg), Ok(vec![value.clone()]))?;
            eq("state payload", kcc1::payload(&ty, &value), Ok(state.clone()))?;
            eq("state payload read back", kcc1::decode_payload(&ty, &state), Ok(value))
        });
    }
}

fn composite_encodings(run: &mut Run, doc: &Json) {
    for v in doc["composite_encodings"].as_array().unwrap() {
        run.check(format!("composite_encodings/{}", v["id"].as_str().unwrap()), || {
            let decls = vector_decls(v);
            let ty = parse(v["type"].as_str().or(v["record"]["name"].as_str()).unwrap(), &decls);
            let value = vector_value(&ty, v);
            if let Some(payload) = v["payload_hex"].as_str() {
                eq("payload", kcc1::payload(&ty, &value), Ok(h(payload)))?;
                eq("payload read back", kcc1::decode_payload(&ty, &h(payload)), Ok(value.clone()))?;
            }
            let lowered = ["lowered_argument_hex", "grouped_argument_hex"].iter().find_map(|k| v[*k].as_array());
            if let Some(pushes) = lowered {
                let want: Vec<Vec<u8>> = pushes.iter().map(|p| h(p.as_str().unwrap())).collect();
                eq("lowered pushes", kcc1::arg_pushes(&ty, &value), Ok(want))?;
            }
            let combined = h(v["argument_hex"].as_str().or(v["combined_hex"].as_str()).unwrap());
            eq("PushArguments", kcc1::encode_args(&[(ty.clone(), value.clone())]), Ok(combined.clone()))?;
            eq("arguments read back", kcc1::decode_args(&[ty], &combined), Ok(vec![value]))
        });
    }
}

/// `name(types)` checked against the vector's signature bytes and tag.
fn check_signature(v: &Json, types: &[Type]) -> Result<[u8; 4], String> {
    let signature = kcc1::function_signature(v["entrypoint_name"].as_str().unwrap(), types);
    eq("FunctionSignature", signature.as_bytes().to_vec(), h(v["function_signature_utf8_hex"].as_str().unwrap()))?;
    let tag = kcc1::dispatch_tag(&signature);
    eq("dispatch tag", tag.to_vec(), h(v["dispatch_tag_hex"].as_str().unwrap()))?;
    Ok(tag)
}

/// The §6.6 `step` arguments as typed values.
fn step_args(doc: &Json) -> Vec<(Type, Value)> {
    let step = &doc["dispatch_and_arguments"];
    types_of(step, &[])
        .into_iter()
        .zip(step["argument_values"].as_array().unwrap())
        .map(|(t, j)| {
            let value = value_of(&t, j);
            (t, value)
        })
        .collect()
}

fn dispatch(run: &mut Run, doc: &Json) {
    let v = &doc["dispatch_and_arguments"];
    run.check("dispatch_and_arguments/step", || {
        let args = step_args(doc);
        let types: Vec<Type> = args.iter().map(|(t, _)| t.clone()).collect();
        check_signature(v, &types)?;
        let mut all = Vec::new();
        for (arg, want) in args.iter().zip(v["argument_encodings_hex"].as_array().unwrap()) {
            let want = h(want.as_str().unwrap());
            eq("argument", kcc1::encode_args(std::slice::from_ref(arg)), Ok(want.clone()))?;
            all.extend(want);
        }
        let values: Vec<Value> = args.into_iter().map(|(_, v)| v).collect();
        eq("arguments read back", kcc1::decode_args(&types, &all), Ok(values))
    });

    let v = &doc["zero_argument_dispatch"];
    run.check("zero_argument_dispatch/tick", || {
        let tag = check_signature(v, &types_of(v, &[]))?;
        eq("PushArguments", kcc1::encode_args(&[]), Ok(h(v["push_arguments_hex"].as_str().unwrap())))?;
        eq("tag push", kcc1::tag_push(&tag).to_vec(), h(v["dispatch_tag_push_hex"].as_str().unwrap()))
    });

    for v in doc["record_dispatch"].as_array().unwrap() {
        run.check(format!("record_dispatch/{}", v["id"].as_str().unwrap()), || {
            let types = types_of(v, &vector_decls(v));
            let names: Vec<String> = types.iter().map(Type::dispatch_type_name).collect();
            let want: Vec<&str> = v["dispatch_type_names"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
            eq("DispatchTypeName", names.iter().map(String::as_str).collect(), want)?;
            check_signature(v, &types).map(|_| ())
        });
    }
}

fn p2sh_envelope(run: &mut Run, doc: &Json) {
    let v = &doc["p2sh_envelope"];
    run.check("p2sh_envelope", || {
        let program = h(v["redeem_script_hex"].as_str().unwrap());
        let spk = kcc1::envelope_spk(&program);
        eq("script public key", spk.clone(), h(v["script_public_key"]["script_hex"].as_str().unwrap()))?;
        eq("Blake2b(R)", spk[2..34].to_vec(), h(v["blake2b_256_hex"].as_str().unwrap()))?;
        // the §6.6 invocation, spent through this envelope and read back
        let args = step_args(doc);
        let types: Vec<Type> = args.iter().map(|(t, _)| t.clone()).collect();
        let tag = kcc1::dispatch_tag(&kcc1::function_signature("step", &types));
        let sig_script = kcc1::signature_script(&kcc1::encode_args(&args)?, &tag, &program);
        eq("signature script", sig_script.clone(), h(v["signature_script_hex"].as_str().unwrap()))?;
        let inv = kcc1::read_invocation_checked(&spk, &sig_script).ok_or("the invocation does not read back")?;
        eq("program", inv.program, program)?;
        eq("tag", inv.dispatch, tag)?;
        let values: Vec<Value> = args.into_iter().map(|(_, v)| v).collect();
        eq("arguments", kcc1::decode_arg_pushes(&types, &inv.args), Ok(values))
    });
}

fn state_encoding(run: &mut Run, doc: &Json) {
    let v = &doc["state_encoding"];
    run.check("state_encoding", || {
        let mut fields = Vec::new();
        for f in v["fields"].as_array().unwrap() {
            let ty = parse(f["type"].as_str().unwrap(), &[]);
            let value = vector_value(&ty, f);
            let one = (ty, value);
            eq("field", kcc1::encode_state_values(std::slice::from_ref(&one)), Ok(h(f["encoded_hex"].as_str().unwrap())))?;
            fields.push(one);
        }
        let encoded = h(v["encoded_hex"].as_str().unwrap());
        eq("EncodeState", kcc1::encode_state_values(&fields), Ok(encoded.clone()))?;
        eq("encoded length", encoded.len() as u64, v["encoded_length"].as_u64().unwrap())?;
        let (types, values): (Vec<Type>, Vec<Value>) = fields.into_iter().unzip();
        eq("state read back", kcc1::decode_state_values(&types, &encoded), Ok(values))
    });
}

fn templates(run: &mut Run, doc: &Json) {
    for (i, v) in doc["template_hashes"].as_array().unwrap().iter().enumerate() {
        run.check(format!("template_hashes/{i}"), || {
            let got = kcc1::template_hash(&h(v["prefix_hex"].as_str().unwrap()), &h(v["suffix_hex"].as_str().unwrap()));
            eq("TemplateHash", got.to_vec(), h(v["hash_hex"].as_str().unwrap()))
        });
    }

    let tv = &doc["template_views"];
    let program = h(tv["program_hex"].as_str().unwrap());
    let fields = fields_of(&tv["fields"]);
    let field = |name: &str| fields.iter().find(|(f, _)| f == name).unwrap().1.clone();
    let at = |v: &Json, key: &str| v[key].as_u64().unwrap() as usize;
    for view in tv["views"].as_array().unwrap() {
        let names: Vec<&str> = view["fields"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
        run.check(format!("template_views/{}", names.join("+")), || {
            // the complete state decodes by its layout
            let (_, state, _) = kcc1::state_cut(&program, at(tv, "state_start"), at(tv, "state_len"))
                .ok_or("the state range runs past the program")?;
            let all: Vec<Type> = fields.iter().map(|(_, t)| t.clone()).collect();
            kcc1::decode_state_values(&all, state)?;
            let (prefix, encoded, suffix) =
                kcc1::state_cut(&program, at(view, "start"), at(view, "len")).ok_or("the view runs past the program")?;
            eq("view prefix", prefix.to_vec(), h(view["prefix_hex"].as_str().unwrap()))?;
            eq("view state", encoded.to_vec(), h(view["encoded_state_hex"].as_str().unwrap()))?;
            eq("view suffix", suffix.to_vec(), h(view["suffix_hex"].as_str().unwrap()))?;
            // a view is whole fields: exactly the named ones decode from it
            let selected: Vec<Type> = names.iter().map(|n| field(n)).collect();
            kcc1::decode_state_values(&selected, encoded)?;
            eq("view template hash", kcc1::template_hash(prefix, suffix).to_vec(), h(view["template_hash_hex"].as_str().unwrap()))
        });
    }

    let v = &doc["continuation_program"];
    run.check("continuation_program", || {
        let next: Vec<(Type, Value)> = v["view_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| {
                let ty = field(n.as_str().unwrap());
                let value = value_of(&ty, &v["next_state"][n.as_str().unwrap()]);
                (ty, value)
            })
            .collect();
        eq("EncodeState(next)", kcc1::encode_state_values(&next), Ok(h(v["encoded_next_state_hex"].as_str().unwrap())))?;
        let r_next = kcc1::continuation(&h(v["prefix_hex"].as_str().unwrap()), &next, &h(v["suffix_hex"].as_str().unwrap()))?;
        eq("R_next", r_next.clone(), h(v["redeem_script_hex"].as_str().unwrap()))?;
        let spk = kcc1::envelope_spk(&r_next);
        eq("Blake2b(R_next)", spk[2..34].to_vec(), h(v["blake2b_256_hex"].as_str().unwrap()))?;
        eq("continuation spk", spk, h(v["script_public_key"]["script_hex"].as_str().unwrap()))
    });
}

fn virtual_element(run: &mut Run, doc: &Json) {
    let v = &doc["hash_committed_virtual_element"];
    run.check("hash_committed_virtual_element", || {
        let fields: Vec<(kcc1::FieldType, kcc1::StateValue)> = v["record"]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| match f["type"].as_str().unwrap() {
                "int" => (kcc1::FieldType::Int, kcc1::StateValue::Int(f["value"].as_i64().unwrap())),
                "bool" => (kcc1::FieldType::Bool, kcc1::StateValue::Bool(f["value"].as_bool().unwrap())),
                other => panic!("no field type {other}"),
            })
            .collect();
        let payload = kcc1::packed(&fields).ok_or("Packed refused a fixed layout")?;
        eq("Packed", payload.clone(), h(v["packed_hex"].as_str().unwrap()))?;
        let commitment = h(v["commitment_hex"].as_str().unwrap());
        eq("commitment", kcc1::commitment(&payload).to_vec(), commitment.clone())?;
        eq("opening verifies", kcc1::verify_commitment(&commitment, &payload), true)
    });
}

/// One rejection vector through the operation it names: `Err` is kascov's
/// refusal, `Ok` an acceptance the spec forbids.
fn attempt(v: &Json) -> Result<(), String> {
    let decls = vector_decls(v);
    match v["operation"].as_str().unwrap() {
        "encode_scalar" => {
            let ty = parse(v["type"].as_str().unwrap(), &decls);
            let n = kcc1::parse_int(v["value_decimal"].as_str().unwrap())?;
            kcc1::encode_args(&[(ty, Value::Int(n))]).map(|_| ())
        }
        "decode_arguments" => kcc1::decode_args(&types_of(v, &decls), &h(v["argument_hex"].as_str().unwrap())).map(|_| ()),
        "decode_state" => {
            let types: Vec<Type> = fields_of(&v["fields"]).into_iter().map(|(_, t)| t).collect();
            kcc1::decode_state_values(&types, &h(v["encoded_hex"].as_str().unwrap())).map(|_| ())
        }
        "validate_argument_type" => kcc1::validate_type(&Type::parse(v["type"].as_str().unwrap(), &decls)?),
        "validate_entrypoints" => {
            let entrypoints: Vec<(String, Vec<Type>)> = v["entrypoints"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| (e["name"].as_str().unwrap().to_string(), types_of(e, &decls)))
                .collect();
            kcc1::validate_entrypoints(&entrypoints)
        }
        other => panic!("unknown operation {other}"),
    }
}

/// What each rejection vector's refusal must say: the rule it was written
/// to break, not merely some refusal. A reader that refuses a vector for an
/// unrelated reason passes a naive run and is still wrong, and an id this
/// table does not know fails the run, so a vector added upstream is read.
const REJECTION_REASONS: &[(&str, &str)] = &[
    ("int-above-maximum", "outside the KCC-1 int range"),
    ("int-below-minimum", "outside the KCC-1 int range"),
    ("argument-int-redundant-sign-byte", "not a minimal ScriptNum"),
    ("argument-int-negative-zero", "not a minimal ScriptNum"),
    ("argument-int-nonminimal-push", "not a run of minimal data pushes"),
    ("argument-byte-empty", "0 bytes where byte was declared"),
    ("argument-bool-two", "a standalone bool is OP_0 or OP_1"),
    ("argument-bool-explicit-false", "a standalone bool is OP_0 or OP_1"),
    ("argument-fixed-bytes-short", "1 bytes where byte[2] was declared"),
    ("argument-int-array-partial-element", "not a whole number of 8-byte int elements"),
    ("argument-fixed-array-wrong-count", "2 elements where bool[3] was declared"),
    ("argument-bool-array-invalid-element", "not a bool payload"),
    ("argument-record-array-unequal-groups", "one field group holds 2 records and another 1"),
    ("argument-record-array-wrong-fixed-count", "where byte[2][2] was declared"),
    ("state-bool-small-integer-opcode", "not one canonical PushExplicit data push"),
    ("state-bool-nonminimal-length-prefix", "not one canonical PushExplicit data push"),
    ("state-bool-empty-payload", "not a bool payload"),
    ("state-bool-invalid-payload", "not a bool payload"),
    ("state-int-minimal-width", "where an int's eight-byte payload was declared"),
    ("state-int-negative-zero", "negative zero is not a canonical int payload"),
    ("state-missing-field", "the state ends before this field"),
    ("state-trailing-bytes", "trailing bytes after the last state field"),
    ("state-truncated-payload", "not one canonical PushExplicit data push"),
    ("state-truncated-length", "not one canonical PushExplicit data push"),
    ("state-non-push-opcode", "not one canonical PushExplicit data push"),
    ("dynamic-array-zero-width-element", "array elements need a positive fixed payload width"),
    ("record-array-variable-width-leaf", "string has no fixed width"),
    ("record-array-zero-width-leaf", "byte[0] has zero width"),
    ("identical-structural-signatures", "share dispatch tag 676b1a86"),
    ("truncated-hash-collision", "share dispatch tag 33b057f4"),
];

fn rejection_vectors(run: &mut Run, doc: &Json) {
    for v in doc["rejection_vectors"].as_array().unwrap() {
        let id = v["id"].as_str().unwrap();
        run.check(format!("rejection_vectors/{id}"), || {
            eq("expected", v["expected"].as_str(), Some("reject"))?;
            if v["operation"] == "validate_entrypoints" {
                // the colliding signatures and tag are the vector's own, so
                // the refusal is about the collision it describes
                let decls = vector_decls(v);
                for (e, want) in v["entrypoints"].as_array().unwrap().iter().zip(v["function_signatures"].as_array().unwrap()) {
                    let signature = kcc1::function_signature(e["name"].as_str().unwrap(), &types_of(e, &decls));
                    eq("FunctionSignature", signature.as_str(), want.as_str().unwrap())?;
                    eq("dispatch tag", hex::encode(kcc1::dispatch_tag(&signature)), v["dispatch_tag_hex"].as_str().unwrap().to_string())?;
                }
            }
            let Some(&(_, want)) = REJECTION_REASONS.iter().find(|(known, _)| *known == id) else {
                return Err("no expected reason for this rejection vector; read it and add one".into());
            };
            match attempt(v) {
                Ok(()) => Err(format!("accepted; the spec rejects it under §{}", v["rule"].as_str().unwrap())),
                Err(reason) if reason.contains(want) => Ok(()),
                Err(reason) => Err(format!(
                    "refused for another reason than §{}: {reason}",
                    v["rule"].as_str().unwrap()
                )),
            }
        });
    }
}

#[test]
fn every_kcc1_conformance_vector_passes_through_kcc1() {
    let doc: Json = serde_json::from_str(VECTORS).unwrap();
    assert_eq!(doc["kcc"], 1);
    assert_eq!(doc["format_version"], 1);
    let mut run = Run::default();
    hash_function(&mut run, &doc);
    canonical_type_names(&mut run, &doc);
    data_pushes(&mut run, &doc);
    scalar_encodings(&mut run, &doc);
    composite_encodings(&mut run, &doc);
    dispatch(&mut run, &doc);
    p2sh_envelope(&mut run, &doc);
    state_encoding(&mut run, &doc);
    templates(&mut run, &doc);
    rejection_vectors(&mut run, &doc);
    virtual_element(&mut run, &doc);

    let failed: Vec<String> = run
        .results
        .iter()
        .filter_map(|(id, r)| r.as_ref().err().map(|e| format!("{id}: {e}")))
        .collect();
    println!(
        "KCC-1 conformance vectors: {}/{} pass",
        run.results.len() - failed.len(),
        run.results.len()
    );
    assert!(failed.is_empty(), "{} failed:\n{}", failed.len(), failed.join("\n"));
    // 103 is the file at 1516a6c and 1ed87ae; any other count means the vendored file
    // changed or a group went unread
    assert_eq!(run.results.len(), 103);
}
