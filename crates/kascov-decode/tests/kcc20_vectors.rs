//! KCC-20 conformance vectors, run through the crate's own functions.
//!
//! Three sets: the official `kcc-0020/vectors/conformance.json` (kaspanet/kccs
//! 1ed87ae, Final, the file unchanged since 1516a6c), Shawn Pearce's first-cut
//! set from kaspanet/kccs PR #20
//! (whose rejects were merged into the official file in reworked form), and
//! kascov's own case for the #16 class rule, which the official set does not
//! cover. The files are read as vendored, unmodified (tests/fixtures/kccs/README.md).
//!
//! Every reject must fail for the rule it was written to break, not merely
//! fail: a reader that rejects a negative threshold for the wrong reason
//! passes a naive run and is still wrong. So each case id maps to the
//! refusal or violation it must produce, and an id this file does not know
//! fails the run, which keeps a vector added upstream from passing unread.

use kascov_decode::kcc0020::{
    self, check_family, hash_chain_link, owner_auth, parse_transfer_reveal, pin_for, verify_borrow, witness_path,
    BorrowRefusal, BorrowScheme, BorrowVerdict, FamilyInput, FamilyOutput, KeyedLink, OwnerAuth, Pin, Rules, State, Template,
    TransferArgs, WitnessPath,
};
use kascov_decode::{encode_push, kcc1, kcc2};
use serde_json::Value;

const REFERENCE_A: &[u8] = include_bytes!("../fixtures/kcc0020/reference_kcc20_a.bin");

fn official() -> Value {
    serde_json::from_str(include_str!("fixtures/kccs/kcc-0020/conformance.json")).unwrap()
}

fn pearce() -> Value {
    serde_json::from_str(include_str!("fixtures/kccs/kcc-0020/pearce-pr20-vectors.json")).unwrap()
}

fn extra() -> Value {
    serde_json::from_str(include_str!("../fixtures/kcc0020/kascov-extra-vectors.json")).unwrap()
}

fn unhex(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().unwrap_or_else(|| panic!("{v} is not hex"))).unwrap()
}

fn b32(v: &Value) -> [u8; 32] {
    unhex(v).try_into().unwrap()
}

fn byte(v: &Value) -> u8 {
    let [b] = unhex(v)[..] else { panic!("{v} is not one byte") };
    b
}

/// The official format: `amount` a number, every other field `*_hex`.
fn official_state(v: &Value) -> State {
    State {
        amount: v["amount"].as_i64().unwrap(),
        owner: b32(&v["owner_hex"]),
        owner_scheme: byte(&v["owner_scheme_hex"]),
        borrow_scheme: byte(&v["borrow_scheme_hex"]),
        borrow_guard: b32(&v["borrow_guard_hex"]),
        extension_commitment: b32(&v["extension_commitment_hex"]),
    }
}

/// The pin every official vector is judged under: the official reference
/// build, which declares the Final rules and the default configuration.
fn reference() -> &'static Pin {
    let pin = kcc0020::pins()
        .iter()
        .find(|p| p.template_hash.starts_with("9703112e"))
        .unwrap();
    assert!(pin.nonconformities().is_empty());
    pin
}

fn template() -> Template {
    reference().cut(REFERENCE_A).unwrap()
}

// ── official: encodings ──────────────────────────────────────────────

fn run_dispatch(v: &Value) -> usize {
    let rows = v["dispatch"].as_array().unwrap();
    for row in rows {
        let signature = row["function_signature"].as_str().unwrap();
        let tag = row["dispatch_tag_hex"].as_str().unwrap();
        assert_eq!(hex::encode(kcc1::dispatch_tag(signature)), tag, "{signature}");
        let (ours, entry) = match row["entrypoint"].as_str().unwrap() {
            "transfer" => (kcc0020::TRANSFER_SIGNATURE, reference().standard_entry("transfer")),
            "transfer_delegator" => (kcc0020::DELEGATOR_SIGNATURE, reference().standard_entry("delegator")),
            other => panic!("unknown entrypoint {other}"),
        };
        assert_eq!(ours, signature);
        assert_eq!(entry.unwrap().select, tag, "the reference pin dispatches on it");
    }
    assert_eq!(hex::encode(kcc0020::transfer_tag()), "79c71c23");
    assert_eq!(hex::encode(kcc0020::delegator_tag()), "fd3ef14a");
    rows.len()
}

/// The record's declared field types, dispatch type name and width.
fn run_state_record(v: &Value) -> usize {
    let record = &v["state_record"];
    let types: Vec<&str> = record["field_types"].as_array().unwrap().iter().map(|t| t.as_str().unwrap()).collect();
    let dispatch = record["dispatch_type_name"].as_str().unwrap();
    assert_eq!(dispatch, format!("{{{}}}", types.join(",")));
    assert!(kcc0020::TRANSFER_SIGNATURE.starts_with(&format!("transfer({dispatch}[]")));
    let widths: Vec<usize> = types
        .iter()
        .map(|t| match *t {
            "int" => 8,
            "byte" => 1,
            "byte[32]" => 32,
            other => panic!("unexpected field type {other}"),
        })
        .collect();
    assert_eq!(widths, kcc0020::TransferArgs::field_widths());
    assert_eq!(record["encoded_state_length"].as_u64().unwrap() as usize, kcc0020::STATE_LEN);
    2
}

/// The encoding vector, plus one consistency check of the file itself: its
/// per-field encodings concatenate to the whole.
fn run_state_encoding(v: &Value) -> usize {
    let row = &v["state_encoding"];
    let state = official_state(&row["state"]);
    let want = unhex(&row["encoded_hex"]);
    assert_eq!(state.encode().unwrap().as_slice(), want.as_slice());
    assert_eq!(State::decode(&want), Some(state));
    let fields: Vec<u8> = row["encoded_fields_hex"].as_array().unwrap().iter().flat_map(unhex).collect();
    assert_eq!(fields, want);
    2
}

/// The field-major lowering, the tag push, and the whole reveal read back
/// through the reference pin.
fn run_transfer_arguments(v: &Value) -> usize {
    let row = &v["transfer_arguments"];
    let args = TransferArgs {
        next_states: row["next_states"].as_array().unwrap().iter().map(official_state).collect(),
        witness: unhex(&row["witness_hex"]),
    };
    let mut want: Vec<u8> = row["grouped_field_pushes_hex"].as_array().unwrap().iter().flat_map(unhex).collect();
    want.extend(unhex(&row["witness_push_hex"]));
    assert_eq!(args.encode().unwrap(), want);
    assert_eq!(kcc1::tag_push(&kcc0020::transfer_tag()).to_vec(), unhex(&row["dispatch_tag_push_hex"]));
    let mut sig = want;
    sig.extend(unhex(&row["dispatch_tag_push_hex"]));
    sig.extend(encode_push(REFERENCE_A));
    assert_eq!(parse_transfer_reveal(&sig, reference()), Some(args));
    1
}

// ── standard transfer ────────────────────────────────────────────────

/// One standard transfer, built from the official `common` block with the
/// case's changes, judged by `check_family` under the reference pin.
fn standard_transfer_violations(common: &Value, case: &Value, base_args: &Value) -> Vec<String> {
    let field = |name: &str| if case[name].is_null() { &common[name] } else { &case[name] };
    let leader = official_state(&common["leader_state"]);
    let mut inputs = vec![FamilyInput { input_index: 0, state: leader, kas: 1 }];
    let delegate_classes = case["delegate_extension_commitments_hex"].as_array();
    for (i, amount) in field("delegate_amounts").as_array().unwrap().iter().enumerate() {
        let mut state = State { amount: amount.as_i64().unwrap(), ..leader };
        if let Some(classes) = delegate_classes {
            state.extension_commitment = b32(&classes[i]);
        }
        inputs.push(FamilyInput { input_index: i as u32 + 1, state, kas: 1 });
    }
    let mut next_states: Vec<State> = base_args["next_states"].as_array().unwrap().iter().map(official_state).collect();
    let amounts = field("next_state_amounts").as_array().unwrap();
    let schemes = field("next_state_owner_schemes_hex").as_array().unwrap();
    let classes = field("next_state_extension_commitments_hex").as_array().unwrap();
    assert_eq!((amounts.len(), schemes.len(), classes.len()), (next_states.len(), next_states.len(), next_states.len()));
    for (i, state) in next_states.iter_mut().enumerate() {
        state.amount = amounts[i].as_i64().unwrap();
        state.owner_scheme = byte(&schemes[i]);
        state.extension_commitment = b32(&classes[i]);
    }
    let transfer = TransferArgs { next_states, witness: unhex(field("witness_hex")) };
    let template = template();
    // A successor that cannot even be encoded (a negative amount) has no
    // program to commit to; the output stands in with an unrelated one.
    let outputs: Vec<FamilyOutput> = transfer
        .next_states
        .iter()
        .map(|s| FamilyOutput {
            spk_version: 0,
            spk: template.reconstruct_spk(s).unwrap_or_else(|| kcc1::envelope_spk(&[0x51])),
            kas: 1,
        })
        .collect();
    let mut bounds = reference().bounds();
    bounds.max_token_inputs = common["max_token_inputs"].as_u64().unwrap() as usize;
    check_family(&inputs, &transfer, &template, &outputs, &bounds)
        .expect("a well-formed family")
        .violations
}

/// The violation each standard-transfer reject must raise, and whether it
/// must be the only one (a case whose change can break one rule only).
fn standard_transfer_expectation(id: &str) -> (&'static str, bool) {
    match id {
        "negative-amount" => ("a successor amount is negative", false),
        "unsupported-owner-scheme" => ("owner scheme outside the supported subset", true),
        "invalid-witness-path-byte" => ("invalid path byte", true),
        "extension-commitment-not-preserved" => ("that no input carries", false),
        "amount-not-preserved" => ("family total not preserved: 1000 in, 1100 out", true),
        "over-max-token-inputs" => ("family exceeds bounds: 4 inputs", true),
        "mixed-extension-commitments" => ("2 different extension commitments", true),
        other => panic!("standard_transfer case {other} has no expectation here; read it and add one"),
    }
}

fn run_standard_transfer(v: &Value, cases_from: &Value) -> usize {
    let common = &v["standard_transfer"]["common"];
    let base = &v["transfer_arguments"];
    let cases = cases_from["standard_transfer"]["cases"].as_array().unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        assert_eq!(case["result"], "reject", "{id}");
        let violations = standard_transfer_violations(common, case, base);
        let (want, sole) = standard_transfer_expectation(id);
        assert!(violations.iter().any(|v| v.contains(want)), "{id}: want {want:?}, got {violations:?}");
        if sole {
            assert_eq!(violations.len(), 1, "{id}: {violations:?}");
        }
    }
    cases.len()
}

/// The common block itself, unchanged, is a clean transfer: the check that
/// the harness is not rejecting everything.
fn run_standard_transfer_common(v: &Value) -> usize {
    let violations =
        standard_transfer_violations(&v["standard_transfer"]["common"], &Value::Null, &v["transfer_arguments"]);
    assert!(violations.is_empty(), "{violations:?}");
    1
}

// ── witnesses ────────────────────────────────────────────────────────

fn run_owner_witness(v: &Value) -> usize {
    let rows = v["owner_witness"].as_array().unwrap();
    let mut checks = 0;
    for row in rows {
        let name = row["scheme"].as_str().unwrap();
        let scheme = kcc2::Scheme::from_name(name).unwrap();
        assert_eq!(scheme.byte(), byte(&row["owner_scheme_hex"]), "{name}");
        let witness = unhex(&row["witness_hex"]);
        assert_eq!(witness.len() as u64, row["length"].as_u64().unwrap(), "{name}");
        let Some(WitnessPath::Normal(auth)) = witness_path(&witness) else {
            panic!("{name}: a normal-path witness");
        };
        let owner = match row["p2pkh_hash_hex"].as_str() {
            Some(_) => b32(&row["p2pkh_hash_hex"]),
            None => [0x11; 32],
        };
        let state = State {
            amount: 1,
            owner,
            owner_scheme: scheme.byte(),
            borrow_scheme: 0,
            borrow_guard: [0; 32],
            extension_commitment: [0; 32],
        };
        let rules = reference().rules;
        let got = owner_auth(&state, auth, rules.p2pkh_hash).unwrap_or_else(|| panic!("{name}: shape or binding"));
        let want = match scheme {
            kcc2::Scheme::P2pkSchnorr => OwnerAuth::P2pkSignature,
            kcc2::Scheme::P2pkhSchnorr => OwnerAuth::P2pkhSchnorr { pubkey: auth[..32].try_into().unwrap() },
            kcc2::Scheme::P2pkhEcdsa => OwnerAuth::P2pkhEcdsa { pubkey: auth[..33].try_into().unwrap() },
            kcc2::Scheme::P2sh => OwnerAuth::P2shAuthorityInput(2),
            kcc2::Scheme::CovenantId => OwnerAuth::CovenantId,
        };
        assert_eq!(got, want, "{name}");
        checks += 1;
        // the P2PKH value itself, through KCC-2's commitment function
        if row["p2pkh_hash_hex"].is_string() {
            let key_len = if scheme == kcc2::Scheme::P2pkhEcdsa { 33 } else { 32 };
            let value = kcc2::p2pkh_authority(scheme, rules.p2pkh_hash, &auth[..key_len]).unwrap();
            assert_eq!(value, owner, "{name}: P2PKHHash");
            // and the Draft's keyed form names someone else
            assert_ne!(kcc2::p2pkh_authority(scheme, kcc2::P2pkhHash::KeyedDraft, &auth[..key_len]).unwrap(), owner);
            checks += 1;
        }
    }
    checks
}

fn run_borrow_witness(v: &Value) -> usize {
    let rows = v["borrow_witness"].as_array().unwrap();
    let chain = &v["hash_chain"]["links"];
    for row in rows {
        let witness = unhex(&row["witness_hex"]);
        assert_eq!(witness.len() as u64, row["length"].as_u64().unwrap());
        let Some(WitnessPath::Borrowed(bw)) = witness_path(&witness) else {
            panic!("a borrowed-path witness");
        };
        match row["scheme"].as_str().unwrap() {
            "amount-threshold/v1" => assert!(bw.is_empty()),
            "schnorr-signature/v1" => assert_eq!(bw.len(), 65),
            "hash-chain/v1" => {
                let link = KeyedLink::parse(bw).expect("129 bytes");
                // it reveals x_2 under the third link's key
                assert_eq!(link.next, b32(&chain[1]["x_hex"]));
                assert_eq!(link.pubkey, b32(&chain[2]["pubkey_hex"]));
            }
            other => panic!("unknown borrow scheme {other}"),
        }
    }
    rows.len()
}

fn run_hash_chain(v: &Value) -> usize {
    let chain = &v["hash_chain"];
    let mut x = b32(&chain["x_0_hex"]);
    let links = chain["links"].as_array().unwrap();
    for link in links {
        x = hash_chain_link(&x, &b32(&link["pubkey_hex"]));
        assert_eq!(x, b32(&link["x_hex"]), "x_{}", link["i"]);
    }
    assert_eq!(x, b32(&chain["initial_borrow_guard_hex"]));
    links.len() + 1
}

// ── borrowed receive ─────────────────────────────────────────────────

/// What each borrowed-receive case must come to. An accept is `Proven`
/// when kascov reproduces every condition, `Structural` when a signature
/// stays consensus's (the file marks those `signature_dependent`).
fn borrowed_expectation(id: &str) -> Option<BorrowRefusal> {
    match id {
        "threshold-exceeded" | "threshold-zero" | "threshold-negative" | "threshold-negative-zero" | "hash-chain-valid" => None,
        "threshold-equal"
        | "threshold-negative-unchanged"
        | "threshold-negative-decrease"
        | "threshold-negative-zero-unchanged" => Some(BorrowRefusal::NotIncreased),
        "threshold-guard-normalized" | "hash-chain-guard-not-advanced" => Some(BorrowRefusal::GuardNotExpected),
        "disabled" => Some(BorrowRefusal::Disabled),
        "unassigned-scheme" => Some(BorrowRefusal::Reserved(0x7f)),
        "successor-changes-owner" => Some(BorrowRefusal::NotPreserved),
        "successor-reduces-kas-value" => Some(BorrowRefusal::KasReduced),
        "hash-chain-link-mismatch" => Some(BorrowRefusal::LinkMismatch),
        other => panic!("borrowed_receive case {other} has no expectation here; read it and add one"),
    }
}

fn borrowed_case(common: &Value, case: &Value, rules: &Rules) -> BorrowVerdict {
    let leader = State {
        amount: common["leader_amount"].as_i64().unwrap(),
        owner: b32(&common["owner_hex"]),
        owner_scheme: byte(&common["owner_scheme_hex"]),
        borrow_scheme: byte(&case["borrow_scheme_hex"]),
        borrow_guard: b32(&case["leader_borrow_guard_hex"]),
        extension_commitment: b32(&common["extension_commitment_hex"]),
    };
    let mut next0 = State {
        amount: case["successor_amount"].as_i64().unwrap(),
        borrow_guard: b32(&case["successor_borrow_guard_hex"]),
        ..leader
    };
    if case["successor_owner_hex"].is_string() {
        next0.owner = b32(&case["successor_owner_hex"]);
    }
    let sompi = |key: &str| case[key].as_u64().unwrap_or(100_000_000);
    let witness = unhex(&case["witness_hex"]);
    let Some(WitnessPath::Borrowed(bw)) = witness_path(&witness) else {
        panic!("a borrowed-path witness");
    };
    verify_borrow(rules, &leader, &next0, bw, sompi("leader_kas_value_sompi"), sompi("successor_kas_value_sompi"))
}

fn run_borrowed_receive(v: &Value) -> usize {
    let section = &v["borrowed_receive"];
    let cases = section["cases"].as_array().unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let verdict = borrowed_case(&section["common"], case, &reference().rules);
        match borrowed_expectation(id) {
            None => {
                assert_eq!(case["result"], "accept", "{id}");
                let structural = case["signature_dependent"].as_bool().unwrap();
                assert!(
                    matches!((&verdict, structural), (BorrowVerdict::Proven(_), false) | (BorrowVerdict::Structural(_), true)),
                    "{id}: {verdict:?}"
                );
            }
            Some(refusal) => {
                assert_eq!(case["result"], "reject", "{id}");
                assert_eq!(verdict, BorrowVerdict::Invalid(refusal), "{id}");
            }
        }
        if let Some(link) = case["revealed_link_hash_hex"].as_str() {
            let w = unhex(&case["witness_hex"]);
            let revealed = KeyedLink::parse(&w[1..]).unwrap();
            assert_eq!(hex::encode(hash_chain_link(&revealed.next, &revealed.pubkey)), link, "{id}");
        }
    }
    cases.len()
}

// ── the runs ─────────────────────────────────────────────────────────

/// 42 checks from the official file, plus two of the harness itself (the
/// common transfer accepts; the per-field encodings concatenate): 44.
#[test]
fn every_official_kcc20_check_passes() {
    let v = official();
    assert_eq!((v["kcc"].as_u64(), v["format_version"].as_u64()), (Some(20), Some(1)));
    let checks = run_dispatch(&v)
        + run_state_record(&v)
        + run_state_encoding(&v)
        + run_transfer_arguments(&v)
        + run_standard_transfer(&v, &v)
        + run_standard_transfer_common(&v)
        + run_owner_witness(&v)
        + run_borrow_witness(&v)
        + run_hash_chain(&v)
        + run_borrowed_receive(&v);
    assert_eq!(checks, 44);
}

/// The file's own note: negative-threshold cases "apply only to contracts
/// that permit those payloads". Under a pin that requires non-negative (the
/// Draft builds) the four cases whose guard decodes to -500 become refusals,
/// for that reason and no other. The two negative-zero cases decode to 0, so
/// they keep the spec's verdicts: the requirement is on the decoded int, and
/// the engine accepts a `-0` guard under every such build.
#[test]
fn negative_thresholds_follow_the_pin() {
    let v = official();
    let section = &v["borrowed_receive"];
    let strict = kcc0020::pins().iter().find(|p| p.template_hash.starts_with("2f2671fc")).unwrap();
    assert!(strict.rules.threshold_non_negative);
    let mut refused = 0;
    let mut seen = 0;
    for case in section["cases"].as_array().unwrap() {
        let guard = b32(&case["leader_borrow_guard_hex"]);
        if case["borrow_scheme_hex"] != "01" || guard[7] & 0x80 == 0 {
            continue;
        }
        seen += 1;
        let verdict = borrowed_case(&section["common"], case, &strict.rules);
        let expected = match case["id"].as_str().unwrap() {
            "threshold-negative-zero" => BorrowVerdict::Proven(BorrowScheme::AmountThreshold(0)),
            "threshold-negative-zero-unchanged" => BorrowVerdict::Invalid(BorrowRefusal::NotIncreased),
            _ => {
                refused += 1;
                BorrowVerdict::Invalid(BorrowRefusal::NegativeThreshold)
            }
        };
        assert_eq!(verdict, expected, "{}", case["id"]);
    }
    assert_eq!((seen, refused), (6, 4));
}

/// The vector pin's own program reads its guard as
/// `threshold = OpBin2Num(borrow_guard.slice(0, 8)); require(threshold >= 0)`,
/// so a `-0` guard is 0 there and an increase of 1 is a valid borrow.
#[test]
fn a_negative_zero_guard_is_zero_under_the_vector_pin() {
    let pin = kcc0020::pins().iter().find(|p| p.template_hash.starts_with("4abcc188")).unwrap();
    assert!(pin.rules.threshold_non_negative);
    let mut guard = [0u8; 32];
    guard[7] = 0x80;
    let leader = State {
        amount: 1_000,
        owner: [0x11; 32],
        owner_scheme: 0x00,
        borrow_scheme: 0x01,
        borrow_guard: guard,
        extension_commitment: [0; 32],
    };
    assert_eq!(
        kcc0020::verify_borrow(&pin.rules, &leader, &State { amount: 1_001, ..leader }, &[], 5, 5),
        BorrowVerdict::Proven(BorrowScheme::AmountThreshold(0))
    );
}

/// Shawn Pearce's reject vectors as merged into the official file
/// (saefstroem/kccs PR #1, the rework of kaspanet/kccs PR #20): all nine are
/// present and each is covered by the official run above.
#[test]
fn the_merged_pearce_rejects_are_in_the_official_file() {
    let v = official();
    let ids = |section: &str| -> Vec<String> {
        v[section]["cases"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect()
    };
    let (transfer, borrowed) = (ids("standard_transfer"), ids("borrowed_receive"));
    for id in [
        "negative-amount",
        "unsupported-owner-scheme",
        "invalid-witness-path-byte",
        "extension-commitment-not-preserved",
        "amount-not-preserved",
        "over-max-token-inputs",
    ] {
        assert!(transfer.iter().any(|t| t == id), "{id}");
    }
    for id in ["successor-changes-owner", "successor-reduces-kas-value", "hash-chain-link-mismatch"] {
        assert!(borrowed.iter().any(|t| t == id), "{id}");
    }
}

/// Shawn Pearce's first-cut file as he posted it: its encodings, tags and
/// template hash, and all eleven rejects, each for its stated rule.
#[test]
fn the_pr20_first_cut_vectors_pass() {
    let v = pearce();
    assert_eq!(v["kcc"].as_u64(), Some(20));
    let mut checks = 0;
    for row in v["state_encoding"].as_array().unwrap() {
        let input = &row["input"];
        let state = State {
            amount: input["amount"].as_i64().unwrap(),
            owner: b32(&input["owner"]),
            owner_scheme: input["owner_scheme"].as_u64().unwrap() as u8,
            borrow_scheme: input["borrow_scheme"].as_u64().unwrap() as u8,
            borrow_guard: b32(&input["borrow_guard"]),
            extension_commitment: b32(&input["extension_commitment"]),
        };
        let want = unhex(&row["encoded_state_hex"]);
        assert_eq!(want.len() as u64, row["encoded_state_len"].as_u64().unwrap());
        assert_eq!(state.encode().unwrap().as_slice(), want.as_slice(), "{}", row["name"]);
        assert_eq!(State::decode(&want), Some(state));
        checks += 1;
    }
    for row in v["dispatch_tags"].as_array().unwrap() {
        let signature = row["function_signature"].as_str().unwrap();
        assert_eq!(hex::encode(kcc1::dispatch_tag(signature)), row["dispatch_tag_hex"].as_str().unwrap());
        checks += 1;
    }
    for row in v["template_hashes"].as_array().unwrap() {
        let template = Template { prefix: unhex(&row["prefix_hex"]), suffix: unhex(&row["suffix_hex"]) };
        assert_eq!(hex::encode(template.hash()), row["template_hash_hex"].as_str().unwrap());
        checks += 1;
    }

    let rules = reference().rules;
    let bounds = reference().bounds();
    let template = template();
    let p2pk = State {
        amount: 1000,
        owner: [0xAA; 32],
        owner_scheme: 0,
        borrow_scheme: 0,
        borrow_guard: [0; 32],
        extension_commitment: [0; 32],
    };
    let sig_witness = [&[0x00][..], &[0x44; 65]].concat();
    // one leader, one successor each, judged by check_family
    let family = |inputs: Vec<State>, next: State, witness: Vec<u8>| -> Vec<String> {
        let inputs: Vec<FamilyInput> = inputs
            .into_iter()
            .enumerate()
            .map(|(i, state)| FamilyInput { input_index: i as u32, state, kas: 1 })
            .collect();
        let output = FamilyOutput { spk_version: 0, spk: template.reconstruct_spk(&next).unwrap(), kas: 1 };
        let transfer = TransferArgs { next_states: vec![next], witness };
        check_family(&inputs, &transfer, &template, &[output], &bounds).unwrap().violations
    };
    let threshold = |t: i64| {
        let mut guard = [0u8; 32];
        guard[..8].copy_from_slice(&kcc1::encode_state_int(t).unwrap());
        guard
    };
    let borrow = |leader: State, next: State, witness: &[u8], kas_in: u64, kas_out: u64| {
        verify_borrow(&rules, &leader, &next, witness, kas_in, kas_out)
    };
    for row in v["reject_vectors"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        assert_eq!(row["expected"], "reject", "{name}");
        let scenario = &row["scenario"];
        let has = |violations: Vec<String>, want: &str| {
            assert!(violations.iter().any(|v| v.contains(want)), "{name}: want {want:?}, got {violations:?}");
        };
        match name {
            "negative_amount" => {
                // a set sign bit is not a state at all
                assert_eq!(State::decode(&unhex(&row["successor_state_hex"])), None);
            }
            "unsupported_owner_scheme" => {
                let next = State::decode(&unhex(&row["successor_state_hex"])).unwrap();
                assert_eq!(next.owner_scheme, 0xff);
                has(family(vec![State { amount: next.amount, ..p2pk }], next, sig_witness.clone()), "owner scheme outside");
            }
            "invalid_witness_path_byte" => {
                let witness = [&[0x02][..], &[0x44; 65]].concat();
                has(family(vec![p2pk], p2pk, witness), "invalid path byte");
            }
            "extension_commitment_not_preserved" => {
                let input = State { extension_commitment: b32(&scenario["input_extension_commitment"]), ..p2pk };
                let next = State { extension_commitment: b32(&scenario["successor_extension_commitment"]), ..p2pk };
                has(family(vec![input], next, sig_witness.clone()), "that no input carries");
            }
            "conservation_violated" => {
                let input = State { amount: scenario["total_input_amount"].as_i64().unwrap(), ..p2pk };
                let next = State { amount: scenario["total_output_amount"].as_i64().unwrap(), ..p2pk };
                has(family(vec![input], next, sig_witness.clone()), "family total not preserved: 1000 in, 1001 out");
            }
            "exceeds_max_token_inputs" => {
                let n = scenario["token_inputs"].as_u64().unwrap() as i64;
                assert_eq!(scenario["max_token_inputs"].as_u64().unwrap() as usize, bounds.max_token_inputs);
                let each = State { amount: 250, ..p2pk };
                has(family(vec![each; n as usize], p2pk, sig_witness.clone()), "family exceeds bounds: 4 inputs");
            }
            "borrow_when_disabled" => {
                let next = State { amount: p2pk.amount + 1, ..p2pk };
                assert_eq!(borrow(p2pk, next, &[], 1, 1), BorrowVerdict::Invalid(BorrowRefusal::Disabled));
            }
            "borrow_increase_not_above_threshold" => {
                let leader = State {
                    amount: scenario["leader_amount"].as_i64().unwrap(),
                    borrow_scheme: 0x01,
                    borrow_guard: threshold(scenario["threshold"].as_i64().unwrap()),
                    ..p2pk
                };
                let next = State { amount: scenario["successor_amount"].as_i64().unwrap(), ..leader };
                assert_eq!(borrow(leader, next, &[], 1, 1), BorrowVerdict::Invalid(BorrowRefusal::NotIncreased));
            }
            "borrow_changes_owner" => {
                let leader = State {
                    amount: scenario["leader_amount"].as_i64().unwrap(),
                    owner: b32(&scenario["leader_owner"]),
                    borrow_scheme: 0x01,
                    borrow_guard: threshold(scenario["threshold"].as_i64().unwrap()),
                    ..p2pk
                };
                let next = State {
                    amount: scenario["successor_amount"].as_i64().unwrap(),
                    owner: b32(&scenario["successor_owner"]),
                    ..leader
                };
                assert_eq!(borrow(leader, next, &[], 1, 1), BorrowVerdict::Invalid(BorrowRefusal::NotPreserved));
            }
            "borrow_reduces_kas_value" => {
                let leader = State { borrow_scheme: 0x01, borrow_guard: threshold(0), ..p2pk };
                let next = State { amount: leader.amount + 1, ..leader };
                let (kas_in, kas_out) = (
                    scenario["borrowed_input_value_sompi"].as_u64().unwrap(),
                    scenario["successor_output_value_sompi"].as_u64().unwrap(),
                );
                assert_eq!(borrow(leader, next, &[], kas_in, kas_out), BorrowVerdict::Invalid(BorrowRefusal::KasReduced));
            }
            "hash_chain_guard_mismatch" => {
                let link = KeyedLink { next: [0x5A; 32], pubkey: [0xB3; 32], signature: [0x44; 65] };
                // a guard committing to another key than the one revealed
                let leader = State {
                    borrow_scheme: 0x03,
                    borrow_guard: hash_chain_link(&link.next, &[0xB4; 32]),
                    ..p2pk
                };
                let next = State { amount: leader.amount + 1, borrow_guard: link.next, ..leader };
                assert_eq!(
                    borrow(leader, next, &link.encode(), 1, 1),
                    BorrowVerdict::Invalid(BorrowRefusal::LinkMismatch)
                );
            }
            other => panic!("PR #20 reject {other} has no expectation here; read it and add one"),
        }
        checks += 1;
    }
    assert_eq!(checks, 17);
}

/// kascov's own case for #16, run in the official format against the
/// official common block.
#[test]
fn the_mixed_class_reject_passes() {
    let v = official();
    let extra = extra();
    assert_eq!(extra["spec_commit"], kcc0020::SPEC_COMMIT);
    assert_eq!(run_standard_transfer(&v, &extra), 1);
    // and the same family with one class throughout is clean
    let mut uniform = extra["standard_transfer"]["cases"][0].clone();
    uniform["delegate_extension_commitments_hex"] = serde_json::json!(["00".repeat(32)]);
    uniform["next_state_extension_commitments_hex"] = serde_json::json!(["00".repeat(32), "00".repeat(32)]);
    let violations = standard_transfer_violations(&v["standard_transfer"]["common"], &uniform, &v["transfer_arguments"]);
    assert!(violations.is_empty(), "{violations:?}");
}

/// The vectors are judged under the reference pin because it is the one
/// build that declares every rule of the Final text; that pin is found by its
/// canonical hash and by its record view alike.
#[test]
fn the_reference_pin_is_the_last_call_judge() {
    let pin = reference();
    assert_eq!(pin.rules, Rules::default());
    let view: [u8; 32] = hex::decode(pin.view_template_hash.as_deref().unwrap()).unwrap().try_into().unwrap();
    assert_eq!(pin_for(&view).unwrap().template_hash, pin.template_hash);
}
