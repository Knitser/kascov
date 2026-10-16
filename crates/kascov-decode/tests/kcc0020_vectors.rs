//! KCC-0020 vectors, derived OUTSIDE the crate and reproduced by it.
//!
//! kccs shipped no conformance vectors for KCC-0020 at the merged Draft
//! (ea5176aa), so kascov derived its own from that text and pins them here:
//! state encodings and the field-major argument lowering built structurally
//! in python, BLAKE3 values from a standalone tool, the P2SH envelope from
//! python's blake2b. The crate reproducing every byte is the point — a drift
//! in either direction fails here, not in the field. The layouts these cover
//! did not change at Last Call; the official vectors that now exist run in
//! tests/kcc20_vectors.rs, and the rules that did change are tested there.

use kascov_decode::kcc0020::{self, State, Template, TransferArgs};
use kascov_decode::kcc1;

fn unhex(s: &str) -> Vec<u8> {
    hex::decode(s).unwrap()
}

fn vectors() -> serde_json::Value {
    serde_json::from_str(include_str!("../fixtures/kcc0020/vectors.json")).unwrap()
}

fn state_of(v: &serde_json::Value) -> State {
    State {
        amount: v["amount"].as_str().unwrap().parse().unwrap(),
        owner: unhex(v["owner"].as_str().unwrap()).try_into().unwrap(),
        owner_scheme: v["owner_scheme"].as_u64().unwrap() as u8,
        borrow_scheme: v["borrow_scheme"].as_u64().unwrap() as u8,
        borrow_guard: unhex(v["borrow_guard"].as_str().unwrap()).try_into().unwrap(),
        extension_commitment: unhex(v["extension_commitment"].as_str().unwrap())
            .try_into()
            .unwrap(),
    }
}

fn state_by_name<'a>(v: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    v["states"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap()
}

#[test]
fn the_fixture_names_the_draft_it_was_derived_from() {
    assert_eq!(vectors()["spec_commit"], kcc0020::DRAFT_SPEC_COMMIT);
}

#[test]
fn dispatch_tags_reproduce() {
    let v = vectors();
    let tags = &v["dispatch_tags"];
    assert_eq!(
        hex::encode(kcc0020::transfer_tag()),
        tags[kcc0020::TRANSFER_SIGNATURE].as_str().unwrap()
    );
    assert_eq!(
        hex::encode(kcc0020::delegator_tag()),
        tags[kcc0020::DELEGATOR_SIGNATURE].as_str().unwrap()
    );
}

#[test]
fn every_state_vector_round_trips() {
    let v = vectors();
    for row in v["states"].as_array().unwrap() {
        let state = state_of(row);
        let want = unhex(row["encoded"].as_str().unwrap());
        assert_eq!(want.len(), kcc0020::STATE_LEN, "{}", row["name"]);
        assert_eq!(
            state.encode().unwrap().as_slice(),
            want.as_slice(),
            "encode {}",
            row["name"]
        );
        assert_eq!(State::decode(&want), Some(state), "decode {}", row["name"]);
    }
}

#[test]
fn the_template_vector_reproduces_hash_program_and_envelope() {
    let v = vectors();
    let t = &v["template"];
    let template = Template {
        prefix: unhex(t["prefix"].as_str().unwrap()),
        suffix: unhex(t["suffix"].as_str().unwrap()),
    };
    assert_eq!(
        hex::encode(template.hash()),
        t["template_hash_blake3"].as_str().unwrap(),
        "the BLAKE3 template name"
    );
    let state = state_of(state_by_name(&v, t["state"].as_str().unwrap()));
    let program = unhex(t["program"].as_str().unwrap());
    assert_eq!(template.program(&state).unwrap(), program);
    assert_eq!(
        template.reconstruct_spk(&state).unwrap(),
        unhex(t["envelope_spk"].as_str().unwrap()),
        "the BLAKE2b envelope"
    );
    // and the cut is recoverable from the program plus the proven output
    let spks = vec![template.reconstruct_spk(&state).unwrap()];
    let (at, cut) = kcc0020::confirm_cut(&program, &[state], &spks).unwrap();
    assert_eq!(at, template.prefix.len());
    assert_eq!(cut, template);
}

#[test]
fn the_hash_chain_vector_reproduces() {
    let v = vectors();
    let preimage = unhex(v["hash_chain"]["preimage"].as_str().unwrap());
    assert_eq!(
        hex::encode(kcc1::hash(&preimage)),
        v["hash_chain"]["guard_blake3"].as_str().unwrap()
    );
}

#[test]
fn every_transfer_args_vector_round_trips_field_major() {
    let v = vectors();
    for row in v["transfer_args"].as_array().unwrap() {
        let states: Vec<State> = row["states"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| state_of(state_by_name(&v, name.as_str().unwrap())))
            .collect();
        let args = TransferArgs {
            next_states: states,
            witness: unhex(row["witness"].as_str().unwrap()),
        };
        let want = unhex(row["pushes"].as_str().unwrap());
        assert_eq!(args.encode().unwrap(), want, "encode n={}", args.next_states.len());
        // split the push stream and parse it back
        let mut payloads = Vec::new();
        let mut at = 0;
        while at < want.len() {
            let (payload, consumed) = kcc1::read_push_minimal(&want[at..]).unwrap();
            payloads.push(payload);
            at += consumed;
        }
        assert_eq!(TransferArgs::from_args(&payloads), Some(args));
    }
}
