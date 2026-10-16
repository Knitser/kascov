//! The official KCC-2 conformance vectors (kccs authority-schemes.json,
//! vendored unmodified), every one run through `kcc2`.
//!
//! A `false` vector passes here only when it is refused for the rule it was
//! written to exercise: a reject for the wrong reason is how the keyed P2PKH
//! hash once made the wrong-key vectors pass vacuously. Below the vectors,
//! the rules the official set does not reach (the Draft's keyed hash, the
//! ECDSA prefix, the input-index path).

use kascov_decode::kcc1;
use kascov_decode::kcc2::{self, Evidence, P2pkhHash, Refusal, Scheme, ScriptPublicKey};
use serde_json::Value;

fn unhex(s: &Value) -> Vec<u8> {
    hex::decode(s.as_str().unwrap()).unwrap()
}

fn id32(v: &Value) -> [u8; 32] {
    unhex(v).try_into().unwrap()
}

fn vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/kccs/kcc-0002/authority-schemes.json")).unwrap()
}

fn scheme_of(row: &Value) -> Scheme {
    let [byte] = unhex(&row["scheme_byte"])[..] else { panic!("one scheme byte") };
    Scheme::from_byte(byte).unwrap()
}

#[test]
fn the_vendored_vectors_are_the_tracked_revision() {
    let readme = include_str!("fixtures/kccs/README.md");
    assert!(readme.contains(kcc2::SPEC_REVISION), "fixtures and SPEC_REVISION disagree");
}

#[test]
fn every_construction_reproduces() {
    let v = vectors();
    let rows = v["constructions"].as_array().unwrap();
    for row in rows {
        let id = row["id"].as_str().unwrap();
        let scheme = scheme_of(row);
        assert_eq!(Scheme::from_name(row["scheme"].as_str().unwrap()), Some(scheme), "{id}");
        assert_eq!(scheme.name(), row["scheme"], "{id}");
        assert_eq!(scheme.value_type().abi_name(), row["value_type"], "{id}");

        let value = match scheme {
            Scheme::P2pkSchnorr => kcc2::authority_value(scheme, &unhex(&row["public_key"])).unwrap(),
            Scheme::P2pkhSchnorr | Scheme::P2pkhEcdsa => {
                kcc2::p2pkh_authority(scheme, P2pkhHash::Unkeyed, &unhex(&row["public_key"])).unwrap()
            }
            Scheme::P2sh => {
                let value = kcc2::p2sh_authority(&unhex(&row["redeem_script"]));
                let spk = &row["script_public_key"];
                assert_eq!(spk["version"].as_u64(), Some(u64::from(kcc2::P2SH_VERSION)), "{id}");
                let script = unhex(&spk["script"]);
                assert_eq!(kcc2::p2sh_script(&value), script, "{id}");
                assert!(ScriptPublicKey { version: 0, script: &script }.is_p2sh_of(&value), "{id}");
                value
            }
            Scheme::CovenantId => kcc2::authority_value(scheme, &unhex(&row["covenant_id"])).unwrap(),
        };
        assert_eq!(hex::encode(value), row["authority_value"], "{id}: authority value");
        assert_eq!(hex::encode(kcc2::argument_encoding(&value)), row["argument_encoding"], "{id}");
        assert_eq!(hex::encode(kcc2::state_encoding(&value)), row["state_encoding"], "{id}");
    }
    assert_eq!(rows.len(), 5);
}

/// The refusal each `false` approval vector exists to provoke.
fn expected_refusal(id: &str) -> Refusal {
    match id {
        "p2pk-invalid-signature" | "schnorr-invalid-signature" | "ecdsa-invalid-signature" => {
            Refusal::InvalidSignature
        }
        "p2pk-wrong-key" | "schnorr-wrong-key" | "ecdsa-wrong-key" => Refusal::WrongKey,
        "schnorr-wrong-length" => Refusal::PublicKeyLength { scheme: Scheme::P2pkhSchnorr, len: 33 },
        "ecdsa-wrong-length" => Refusal::PublicKeyLength { scheme: Scheme::P2pkhEcdsa, len: 32 },
        "p2sh-wrong-version" | "p2sh-wrong-hash" | "p2sh-missing-input" => Refusal::NoP2shInput,
        "covenant-wrong-id" | "covenant-output-only" => Refusal::NoCovenantInput,
        other => panic!("no expected refusal for {other}"),
    }
}

#[test]
fn every_approval_check_reaches_its_verdict_for_its_reason() {
    let v = vectors();
    let constructions = v["constructions"].as_array().unwrap();
    let rows = v["approval_checks"].as_array().unwrap();
    for row in rows {
        let id = row["id"].as_str().unwrap();
        let construction = constructions
            .iter()
            .find(|c| c["id"] == row["construction"])
            .unwrap();
        let scheme = scheme_of(construction);
        let authority = unhex(&construction["authority_value"]);
        let ctx = &row["context"];

        let public_key = ctx.get("public_key").map(unhex);
        let spks: Vec<(u16, Vec<u8>)> = ctx
            .get("input_script_public_keys")
            .map(|inputs| {
                let inputs = inputs.as_array().unwrap().iter();
                inputs
                    .map(|spk| (spk["version"].as_u64().unwrap() as u16, unhex(&spk["script"])))
                    .collect()
            })
            .unwrap_or_default();
        let spk_refs: Vec<ScriptPublicKey> = spks
            .iter()
            .map(|(version, script)| ScriptPublicKey { version: *version, script })
            .collect();
        let covenant_ids: Vec<Option<[u8; 32]>> = ctx
            .get("input_covenant_ids")
            .map(|ids| {
                let ids = ids.as_array().unwrap().iter();
                ids.map(|c| (!c.is_null()).then(|| id32(c))).collect()
            })
            .unwrap_or_default();

        let evidence = if let Some(public_key) = &public_key {
            Evidence::Signature {
                public_key,
                signature_valid: ctx["signature_valid"].as_bool().unwrap(),
            }
        } else if ctx.get("input_script_public_keys").is_some() {
            Evidence::P2shInputs { inputs: &spk_refs, index: None }
        } else {
            // the active input is among the inputs and may itself match;
            // the outputs are never evidence, so they are not passed at all
            assert!((ctx["active_input_index"].as_u64().unwrap() as usize) < covenant_ids.len());
            Evidence::CovenantInputs { input_covenant_ids: &covenant_ids }
        };
        let verdict = kcc2::approve(scheme, &authority, &evidence, P2pkhHash::Unkeyed);
        if row["expected"].as_bool().unwrap() {
            assert_eq!(verdict, Ok(()), "{id}");
        } else {
            assert_eq!(verdict, Err(expected_refusal(id)), "{id}");
        }
    }
    assert_eq!(rows.len(), 20);
}

#[test]
fn every_registry_check_matches() {
    let v = vectors();
    let rows = v["registry_checks"].as_array().unwrap();
    for row in rows {
        let [byte] = unhex(&row["scheme_byte"])[..] else { panic!("one scheme byte") };
        let assigned = row["assigned"].as_bool().unwrap();
        let custom = row["custom"].as_bool().unwrap();
        assert_eq!(kcc2::is_assigned(byte), assigned, "{byte:02x}");
        assert_eq!(kcc2::is_custom(byte), custom, "{byte:02x}");
        assert_eq!(kcc2::is_reserved(byte), !assigned && !custom, "{byte:02x}");
        let want = match (assigned, custom) {
            (true, _) => Ok(byte),
            (false, true) => Err(Refusal::CustomScheme(byte)),
            (false, false) => Err(Refusal::ReservedScheme(byte)),
        };
        assert_eq!(Scheme::from_byte(byte).map(Scheme::byte), want, "{byte:02x}");
    }
    assert_eq!(rows.len(), 10);
}

#[test]
fn the_registry_partitions_the_whole_byte() {
    for byte in 0..=u8::MAX {
        let classes = [kcc2::is_assigned(byte), kcc2::is_reserved(byte), kcc2::is_custom(byte)];
        assert_eq!(classes.iter().filter(|c| **c).count(), 1, "{byte:02x}");
    }
    for scheme in Scheme::ALL {
        assert_eq!(Scheme::from_byte(scheme.byte()), Ok(scheme));
        assert_eq!(Scheme::from_name(scheme.name()), Some(scheme));
    }
    assert_eq!(Scheme::from_name("p2pk-schnorr"), None, "the version is part of the name");
}

const G: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

#[test]
fn the_keyed_draft_hash_is_the_ea5176aa_value() {
    // Draft-built programs check the keyed form; this is the value kascov's
    // ea5176aa reader computed for the generator key, so a pin that keeps
    // the Draft rule keeps every owner it already had.
    let key = hex::decode(G).unwrap();
    assert_eq!(
        P2pkhHash::KeyedDraft.hash(&key),
        kcc1::hash_keyed(&key, b"PublicKeyHash").unwrap()
    );
    assert_eq!(
        hex::encode(P2pkhHash::KeyedDraft.hash(&key)),
        "91438cfb14e4fbb78c740ab72162162f203eafae88940707d43ce7e97268d582"
    );
    assert_ne!(P2pkhHash::KeyedDraft.hash(&key), P2pkhHash::Unkeyed.hash(&key));
    assert_eq!(P2pkhHash::default(), P2pkhHash::Unkeyed);
}

#[test]
fn a_keyed_draft_program_approves_its_own_owner_and_not_the_unkeyed_one() {
    let key = hex::decode(G).unwrap();
    let keyed = P2pkhHash::KeyedDraft.hash(&key);
    let evidence = Evidence::Signature { public_key: &key, signature_valid: true };
    assert_eq!(kcc2::approve(Scheme::P2pkhSchnorr, &keyed, &evidence, P2pkhHash::KeyedDraft), Ok(()));
    assert_eq!(
        kcc2::approve(Scheme::P2pkhSchnorr, &keyed, &evidence, P2pkhHash::Unkeyed),
        Err(Refusal::WrongKey)
    );
}

#[test]
fn p2pkh_hash_kind_reads_from_a_pin() {
    let kind: P2pkhHash = serde_json::from_str("\"keyed-draft\"").unwrap();
    assert_eq!(kind, P2pkhHash::KeyedDraft);
    let kind: P2pkhHash = serde_json::from_str("\"unkeyed\"").unwrap();
    assert_eq!(kind, P2pkhHash::Unkeyed);
}

#[test]
fn an_uncompressed_ecdsa_prefix_is_refused_before_hashing() {
    let mut key = hex::decode(format!("02{G}")).unwrap();
    key[0] = 0x04;
    assert_eq!(
        kcc2::p2pkh_authority(Scheme::P2pkhEcdsa, P2pkhHash::Unkeyed, &key),
        Err(Refusal::EcdsaKeyNotCompressed(0x04))
    );
}

#[test]
fn an_indexed_p2sh_input_must_exist_and_match_in_full() {
    let authority = kcc2::p2sh_authority(&[0x51]);
    let script = kcc2::p2sh_script(&authority);
    let other = kcc2::p2sh_script(&[0; 32]);
    let inputs = [
        ScriptPublicKey { version: 0, script: &other },
        ScriptPublicKey { version: 1, script: &script },
        ScriptPublicKey { version: 0, script: &script },
    ];
    let at = |index| {
        kcc2::approve(
            Scheme::P2sh,
            &authority,
            &Evidence::P2shInputs { inputs: &inputs, index: Some(index) },
            P2pkhHash::Unkeyed,
        )
    };
    assert_eq!(at(2), Ok(()));
    assert_eq!(at(1), Err(Refusal::InputNotAuthority { index: 1 }), "the version is compared");
    assert_eq!(at(0), Err(Refusal::InputNotAuthority { index: 0 }));
    assert_eq!(at(3), Err(Refusal::InputIndexOutOfRange { index: 3, inputs: 3 }));
}

#[test]
fn evidence_of_the_wrong_kind_and_short_values_are_refused() {
    let authority = [7u8; 32];
    let key = hex::decode(G).unwrap();
    let signature = Evidence::Signature { public_key: &key, signature_valid: true };
    assert_eq!(
        kcc2::approve(Scheme::P2sh, &authority, &signature, P2pkhHash::Unkeyed),
        Err(Refusal::WrongEvidence(Scheme::P2sh))
    );
    assert_eq!(
        kcc2::approve(
            Scheme::P2pkSchnorr,
            &authority,
            &Evidence::CovenantInputs { input_covenant_ids: &[Some(authority)] },
            P2pkhHash::Unkeyed,
        ),
        Err(Refusal::WrongEvidence(Scheme::P2pkSchnorr))
    );
    assert_eq!(
        kcc2::approve(Scheme::CovenantId, &authority[..31], &signature, P2pkhHash::Unkeyed),
        Err(Refusal::ValueLength { scheme: Scheme::CovenantId, len: 31 })
    );
}

#[test]
fn reserved_and_custom_refusals_say_different_things() {
    let reserved = Refusal::ReservedScheme(0x05).to_string();
    let custom = Refusal::CustomScheme(0x80).to_string();
    assert!(reserved.contains("0x05") && reserved.contains("reserved"), "{reserved}");
    assert!(custom.contains("0x80") && custom.contains("custom"), "{custom}");
}
