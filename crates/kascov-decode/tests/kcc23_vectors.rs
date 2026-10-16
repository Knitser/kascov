//! KCC-23 official conformance vectors, read as kaspanet/kccs ships them.
//!
//! The fixture is vendored unmodified (see tests/fixtures/kccs/README.md); this
//! runner parses it directly, applies the mutations by their JSON pointers
//! as its `mutation_rule` says, and reproduces every intermediate value it
//! pins. Twenty-four checks in all: 9 on the binding, 3 on the mutations,
//! 2 on each of the 6 declarations.
//!
//! The second half pins where kascov reads §3.1 more strictly than the
//! reference `Codec`: five JSON edge cases the reference accepts and the
//! I-JSON text forbids, plus the edge cases on which the two agree, so a
//! change to either side shows up here.

use kascov_decode::kcc23::{
    self, CovenantBinding, DeclarationRefusal, Outpoint, Refusal, Tx, TxInput, TxOutput,
};
use serde_json::Value;

fn conformance() -> Value {
    serde_json::from_str(include_str!("fixtures/kccs/kcc-0023/conformance.json")).unwrap()
}

fn unhex(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().unwrap()).unwrap()
}

fn hash32(v: &Value) -> [u8; 32] {
    unhex(v).try_into().unwrap()
}

fn tx_of(v: &Value) -> Tx {
    Tx {
        version: v["version"].as_u64().unwrap() as u16,
        inputs: v["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| TxInput {
                previous_outpoint: Outpoint {
                    transaction_id: hash32(&input["previous_outpoint"]["transaction_id"]),
                    index: input["previous_outpoint"]["index"].as_u64().unwrap() as u32,
                },
                sequence: input["sequence"].as_u64().unwrap(),
            })
            .collect(),
        outputs: v["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|output| TxOutput {
                value: output["value"].as_u64().unwrap(),
                script_version: output["script_public_key"]["version"].as_u64().unwrap() as u16,
                script: unhex(&output["script_public_key"]["script_hex"]),
                covenant: (!output["covenant"].is_null()).then(|| CovenantBinding {
                    authorizing_input: output["covenant"]["authorizing_input"].as_u64().unwrap()
                        as u16,
                    covenant_id: hash32(&output["covenant"]["covenant_id"]),
                }),
            })
            .collect(),
        lock_time: v["lock_time"].as_u64().unwrap(),
        subnetwork_id: unhex(&v["subnetwork_id_hex"]).try_into().unwrap(),
        gas: v["gas"].as_u64().unwrap(),
        payload: unhex(&v["payload_hex"]),
    }
}

/// The parts of the vector file the runner does not interpret must still
/// match what it was written against, or the counts below mean nothing.
#[test]
fn fixture_is_the_kcc23_format_this_runner_reads() {
    let c = conformance();
    assert_eq!(c["kcc"], 23);
    assert_eq!(c["format_version"], 1);
    assert_eq!(c["codec_prefix_hex"], "00");
}

#[test]
fn every_official_vector_passes() {
    let c = conformance();
    let mut checks = 0;
    let mut check = |ok: bool, what: String| {
        checks += 1;
        assert!(ok, "{what}");
    };

    let binding = &c["binding"];
    let expected = &binding["expected"];
    let preparation = tx_of(&binding["preparation_transaction"]);
    let genesis = tx_of(&binding["genesis_transaction"]);
    let covenant_id = hash32(&expected["covenant_id"]);
    let preparation_txid = hash32(&expected["preparation_transaction_id"]);
    let group: Vec<u32> = binding["genesis_output_indices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i.as_u64().unwrap() as u32)
        .collect();

    check(
        preparation.payload[0] == 0x00
            && preparation.payload[1..] == *binding["metadata_text"].as_str().unwrap().as_bytes(),
        "binding: the payload is 0x00 || metadata_text".into(),
    );
    check(
        kcc23::payload_digest(&preparation.payload) == hash32(&expected["payload_digest"]),
        "binding: payload_digest".into(),
    );
    check(
        kcc23::rest_preimage(&preparation) == unhex(&expected["rest_preimage_hex"]),
        "binding: rest_preimage_hex".into(),
    );
    check(
        kcc23::rest_digest(&preparation) == hash32(&expected["rest_digest"]),
        "binding: rest_digest".into(),
    );
    check(
        kcc23::txid_v1(&preparation) == preparation_txid,
        "binding: preparation_transaction_id".into(),
    );
    check(
        genesis.inputs[0].previous_outpoint
            == Outpoint {
                transaction_id: preparation_txid,
                index: 0,
            },
        "binding: the genesis spends preparation output 0".into(),
    );
    check(
        kcc23::covenant_id(
            genesis.inputs[0].previous_outpoint,
            group.iter().map(|&i| (i, &genesis.outputs[i as usize])),
        ) == covenant_id,
        "binding: covenant_id recomputed from the outpoint and the group".into(),
    );
    check(
        group
            .iter()
            .all(|&i| genesis.outputs[i as usize].covenant.unwrap().covenant_id == covenant_id),
        "binding: the group declares the expected covenant id".into(),
    );
    let verified = kcc23::verify(&preparation, &genesis, &covenant_id);
    check(
        binding["valid"] == true
            && verified.as_ref().is_ok_and(|b| {
                b.declaration.json == binding["metadata_text"].as_str().unwrap()
                    && b.preparation_txid == preparation_txid
                    && b.output_index == 0
                    && b.authorizing_input == 0
                    && b.group == group
            }),
        format!("binding: verify gives the declaration, got {verified:?}"),
    );
    // the compact form a store keeps instead of the transaction recovers the
    // same id and reaches the same binding
    let compact = kcc23::Preparation::of(&preparation);
    assert_eq!(compact.txid(), preparation_txid);
    assert_eq!(
        kcc23::verify_prepared(&compact, &genesis, &covenant_id),
        verified
    );

    for mutation in c["binding_mutations"].as_array().unwrap() {
        let name = mutation["name"].as_str().unwrap();
        let mut mutated = binding.clone();
        for (pointer, value) in mutation["replace"].as_object().unwrap() {
            *mutated.pointer_mut(pointer).unwrap() = value.clone();
        }
        let preparation = tx_of(&mutated["preparation_transaction"]);
        let genesis = tx_of(&mutated["genesis_transaction"]);
        if let Some(id) = mutation.get("expected_preparation_transaction_id") {
            check(
                kcc23::txid_v1(&preparation) == hash32(id),
                format!("{name}: expected_preparation_transaction_id"),
            );
        }
        let verified = kcc23::verify(&preparation, &genesis, &covenant_id);
        let reason_fits = match name {
            "changed_metadata" => verified == Err(Refusal::NotSpentAtGenesis),
            "version_0" => verified == Err(Refusal::PreparationNotVersion1(0)),
            other => panic!("mutation {other} has no expected refusal in this runner"),
        };
        check(
            mutation["valid"] == false && reason_fits,
            format!("{name}: verify refuses for the vector's reason, got {verified:?}"),
        );
    }

    for declaration in c["declarations"].as_array().unwrap() {
        let name = declaration["name"].as_str().unwrap();
        let payload = unhex(&declaration["payload_hex"]);
        let decoded = kcc23::decode_declaration(&payload);
        check(
            decoded.is_ok() == declaration["valid"].as_bool().unwrap(),
            format!("{name}: validity, got {decoded:?}"),
        );
        // The second check per case: a valid declaration keeps the exact
        // bytes it commits to, and an invalid one is refused for the
        // reason its name gives, not for some other fault.
        let detail = match (name, &decoded) {
            (_, Ok(d)) => [&[0x00], d.json.as_bytes()].concat() == payload,
            ("missing_codec_prefix", Err(r)) => *r == DeclarationRefusal::UnknownCodec(b'{'),
            ("unknown_codec", Err(r)) => *r == DeclarationRefusal::UnknownCodec(0x01),
            ("non_object", Err(r)) => *r == DeclarationRefusal::NotAnObject,
            ("invalid_utf8", Err(r)) => *r == DeclarationRefusal::InvalidUtf8,
            (other, Err(r)) => {
                panic!("declaration {other} refused with {r} has no expected reason")
            }
        };
        check(
            detail,
            format!("{name}: exact bytes or refusal reason, got {decoded:?}"),
        );
    }

    assert_eq!(checks, 24, "the vector file should yield 24 checks");
}

fn declare(json: &[u8]) -> Result<kcc23::Declaration, DeclarationRefusal> {
    kcc23::decode_declaration(&[&[0x00], json].concat())
}

/// A root object holding `depth - 1` nested arrays: depth `depth`, counting
/// the root as 1 (§3.1).
fn nested(depth: usize) -> String {
    format!(
        r#"{{"a":{}{}}}"#,
        "[".repeat(depth - 1),
        "]".repeat(depth - 1)
    )
}

// The five cases where the reference Codec accepts what the §3.1 text
// forbids. kascov follows the text.

#[test]
fn a_duplicate_name_is_refused() {
    assert_eq!(
        declare(br#"{"name":"Kaspa","name":"Other"}"#),
        Err(DeclarationRefusal::DuplicateName("name".into()))
    );
    // Equal after unescaping is equal: I-JSON compares names, not spellings.
    assert_eq!(
        declare(br#"{"a":1,"\u0061":2}"#),
        Err(DeclarationRefusal::DuplicateName("a".into()))
    );
    // Uniqueness is per object; the same name in a nested object is fine.
    assert!(declare(br#"{"a":{"a":1}}"#).is_ok());
}

#[test]
fn an_escaped_uffff_noncharacter_is_refused() {
    assert_eq!(
        declare(br#"{"name":"\uffff"}"#),
        Err(DeclarationRefusal::Noncharacter('\u{ffff}'))
    );
}

#[test]
fn a_raw_ufdd0_noncharacter_is_refused() {
    assert_eq!(
        declare("{\"name\":\"\u{fdd0}\"}".as_bytes()),
        Err(DeclarationRefusal::Noncharacter('\u{fdd0}'))
    );
    // Names are strings too, and every plane ends in two noncharacters.
    assert_eq!(
        declare("{\"\u{1fffe}\":1}".as_bytes()),
        Err(DeclarationRefusal::Noncharacter('\u{1fffe}'))
    );
    // The neighbours are ordinary characters.
    assert!(declare("{\"name\":\"\u{fdcf}\u{fdf0}\u{fffd}\"}".as_bytes()).is_ok());
}

#[test]
fn depth_65_is_refused() {
    assert!(declare(nested(64).as_bytes()).is_ok());
    assert_eq!(
        declare(nested(65).as_bytes()),
        Err(DeclarationRefusal::TooDeep)
    );
    // Objects count the same as arrays.
    let objects = format!("{}1{}", r#"{"a":"#.repeat(65), "}".repeat(65));
    assert_eq!(
        declare(objects.as_bytes()),
        Err(DeclarationRefusal::TooDeep)
    );
}

#[test]
fn depth_127_is_refused() {
    assert_eq!(
        declare(nested(127).as_bytes()),
        Err(DeclarationRefusal::TooDeep)
    );
}

#[test]
fn a_strictness_breach_also_fails_the_binding() {
    let c = conformance();
    let mut preparation = tx_of(&c["binding"]["preparation_transaction"]);
    let genesis = tx_of(&c["binding"]["genesis_transaction"]);
    preparation.payload = [&[0x00], br#"{"name":"A","name":"B"}"#.as_slice()].concat();
    let covenant_id = hash32(&c["binding"]["expected"]["covenant_id"]);
    assert_eq!(
        kcc23::verify(&preparation, &genesis, &covenant_id),
        Err(Refusal::Declaration(DeclarationRefusal::DuplicateName(
            "name".into()
        )))
    );
}

// The edge cases on which the reference and the text agree.

#[test]
fn edge_cases_the_reference_and_the_text_agree_on() {
    assert!(matches!(
        declare(br#"{"x":"\ud800"}"#),
        Err(DeclarationRefusal::Json(_))
    ));
    assert!(matches!(
        declare(br#"{"x":1e400}"#),
        Err(DeclarationRefusal::Json(_))
    ));
    assert!(matches!(
        declare(nested(128).as_bytes()),
        Err(DeclarationRefusal::TooDeep)
    ));
    assert!(matches!(declare(b"{}{}"), Err(DeclarationRefusal::Json(_))));
    assert_eq!(
        declare("\u{feff}{}".as_bytes()),
        Err(DeclarationRefusal::ByteOrderMark)
    );
    assert_eq!(
        kcc23::decode_declaration(&[]),
        Err(DeclarationRefusal::Empty)
    );
    assert!(declare(br#"{"x":18446744073709551616}"#).is_ok());
    assert!(declare(b" {} \n").is_ok());
    assert!(declare(br#"{"x":"\u0000"}"#).is_ok());
    // A paired surrogate escape is one ordinary character.
    assert!(declare(br#"{"x":"\ud83d\ude00"}"#).is_ok());
}

#[test]
fn the_exact_text_is_kept_whitespace_and_order_included() {
    let d = declare(br#" { "b": 2, "a": 1 } "#).unwrap();
    assert_eq!(d.json, r#" { "b": 2, "a": 1 } "#);
}

#[test]
fn convention_fields_are_read_only_when_their_type_fits() {
    let example = declare(br#"{"name":"Example Token","symbol":"EXT","decimals":8}"#).unwrap();
    assert_eq!(
        example.conventions(),
        kcc23::Conventions {
            name: Some("Example Token".into()),
            symbol: Some("EXT".into()),
            decimals: Some(8),
            ..Default::default()
        }
    );
    let loose = declare(
        br#"{"name":7,"ticker":"KAS","decimals":8.0,"image":"ipfs://x","image_hash":"ab","extra":true}"#,
    )
    .unwrap();
    let conventions = loose.conventions();
    assert_eq!(conventions.name, None);
    assert_eq!(conventions.ticker.as_deref(), Some("KAS"));
    assert_eq!(conventions.decimals, None);
    assert_eq!(conventions.image.as_deref(), Some("ipfs://x"));
    assert_eq!(conventions.image_hash.as_deref(), Some("ab"));
    assert_eq!(loose.object["extra"], true);
}

// The reference crate's own binding mutations beyond the vector file.

fn vector_pair() -> (Tx, Tx, [u8; 32]) {
    let c = conformance();
    (
        tx_of(&c["binding"]["preparation_transaction"]),
        tx_of(&c["binding"]["genesis_transaction"]),
        hash32(&c["binding"]["expected"]["covenant_id"]),
    )
}

#[test]
fn a_genesis_spending_a_missing_preparation_output_does_not_bind() {
    let (preparation, mut genesis, id) = vector_pair();
    genesis.inputs[0].previous_outpoint.index = 1;
    assert_eq!(
        kcc23::verify(&preparation, &genesis, &id),
        Err(Refusal::NotSpentAtGenesis)
    );
}

#[test]
fn a_changed_group_does_not_bind() {
    let (preparation, genesis, id) = vector_pair();
    let mut grown = genesis.clone();
    grown.outputs.push(grown.outputs[0].clone());
    assert_eq!(
        kcc23::verify(&preparation, &grown, &id),
        Err(Refusal::CovenantIdMismatch)
    );

    let mut other_input = genesis.clone();
    other_input.outputs[0].covenant = Some(CovenantBinding {
        authorizing_input: 1,
        covenant_id: id,
    });
    assert_eq!(
        kcc23::verify(&preparation, &other_input, &id),
        Err(Refusal::CovenantIdMismatch)
    );

    let mut v0 = genesis;
    v0.version = 0;
    assert_eq!(
        kcc23::verify(&preparation, &v0, &id),
        Err(Refusal::GenesisBeforeVersion1(0))
    );
}

#[test]
fn a_group_authorized_by_a_later_input_binds() {
    let (preparation, _, _) = vector_pair();
    let outpoint = Outpoint {
        transaction_id: kcc23::txid_v1(&preparation),
        index: 0,
    };
    let output = |value| TxOutput {
        value,
        script_version: 0,
        script: vec![0x51],
        covenant: None,
    };
    let mut genesis = Tx {
        version: 1,
        inputs: vec![
            TxInput {
                previous_outpoint: Outpoint {
                    transaction_id: [0x22; 32],
                    index: 0,
                },
                sequence: 0,
            },
            TxInput {
                previous_outpoint: outpoint,
                sequence: 0,
            },
        ],
        outputs: vec![output(1), output(2), output(3)],
        lock_time: 0,
        subnetwork_id: [0; 20],
        gas: 0,
        payload: vec![],
    };
    let id = kcc23::covenant_id(
        outpoint,
        [(0, &genesis.outputs[0]), (2, &genesis.outputs[2])].into_iter(),
    );
    for i in [0, 2] {
        genesis.outputs[i].covenant = Some(CovenantBinding {
            authorizing_input: 1,
            covenant_id: id,
        });
    }
    let binding = kcc23::verify(&preparation, &genesis, &id).unwrap();
    assert_eq!((binding.authorizing_input, binding.group), (1, vec![0, 2]));
}

/// The declaration is fixed for a covenant id but not unique to one: a
/// second covenant authorized by the same preparation transaction's other
/// output verifies with the same metadata. Readers count these
/// (`shared_with`) rather than treat a declaration as identity.
#[test]
fn one_declaration_binds_every_covenant_its_preparation_authorizes() {
    let (mut preparation, _, _) = vector_pair();
    preparation.outputs.push(preparation.outputs[0].clone());
    let txid = kcc23::txid_v1(&preparation);
    let genesis_from = |index: u32| {
        let outpoint = Outpoint {
            transaction_id: txid,
            index,
        };
        let mut genesis = Tx {
            version: 1,
            inputs: vec![TxInput {
                previous_outpoint: outpoint,
                sequence: 0,
            }],
            outputs: vec![TxOutput {
                value: 90_000_000,
                script_version: 0,
                script: vec![0x51],
                covenant: None,
            }],
            lock_time: 0,
            subnetwork_id: [0; 20],
            gas: 0,
            payload: vec![],
        };
        let id = kcc23::covenant_id(outpoint, [(0, &genesis.outputs[0])].into_iter());
        genesis.outputs[0].covenant = Some(CovenantBinding {
            authorizing_input: 0,
            covenant_id: id,
        });
        (genesis, id)
    };
    let (first, first_id) = genesis_from(0);
    let (second, second_id) = genesis_from(1);
    assert_ne!(first_id, second_id);
    let a = kcc23::verify(&preparation, &first, &first_id).unwrap();
    let b = kcc23::verify(&preparation, &second, &second_id).unwrap();
    assert_eq!(a.declaration, b.declaration);
    assert_eq!((a.output_index, b.output_index), (0, 1));
}
