//! The argent detector against real chain bytes: what it must accept, what it
//! must refuse, and the two rules that look right and are not.
//!
//! Every positive fixture is a full revealed program from testnet-10, kept
//! whole because the template hash is over the whole thing — a truncated
//! program would still exercise the walk but could no longer prove a
//! commitment, and proving commitments is the point. Every negative is a
//! fixture the repo already carries for another matcher, reused rather than
//! copied so the two can never drift apart.

use kascov_decode::argent::template_hash;
use kascov_decode::argent::{
    dispatch_envelope, recognize, Authorised, Dispatch, Generation, Split, Terminator,
};

fn hex_bytes(text: &str) -> Vec<u8> {
    let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).expect("fixture is hex"))
        .collect()
}

fn hex32(s: &str) -> [u8; 32] {
    hex_bytes(s).try_into().expect("32 bytes")
}

/// Zealous Swap's token cell: 1,279 B, four entrypoints, the single largest
/// argent family on testnet-10 (265 instances over 13 lineages at the census).
/// Reveal of covenant `1feb0161…`; blake2b of these bytes is
/// `6524b071564668b4…`, which is the P2SH commitment it spent.
fn token_cell() -> Vec<u8> {
    hex_bytes(include_str!("fixtures/argent-zealous-token-cell.hex"))
}

/// A 573 B two-entrypoint Zealous build, reveal of covenant `828aad5e…`.
/// Here to keep the tests honest about entrypoint counts: a walk that stopped
/// at the first rung would still pass the four-entrypoint fixture.
fn cell_573() -> Vec<u8> {
    hex_bytes(include_str!("fixtures/argent-zealous-cell-573.hex"))
}

/// The 980 B order cell — the leaf tier — revealed by covenant `35d0f746…`,
/// the same Zealous market `argent_template.rs` reads its commitment from.
fn order_cell() -> Vec<u8> {
    hex_bytes(include_str!("fixtures/argent-zealous-order-cell.hex"))
}

#[test]
fn zealous_programs_reproduce_their_family_hashes() {
    let cases: [(Vec<u8>, &str, usize, usize, usize); 2] = [
        (
            token_cell(),
            "7d006a1f5560450d49246b555a41caf79e9e1a1d77a91d199eaa682ce2de90ca",
            77,
            1201,
            4,
        ),
        (
            cell_573(),
            "91cc441412843d685929fbf6ed6175e29c9a80ff3c9b4a4e720dd31bea0ced98",
            108,
            464,
            2,
        ),
    ];
    for (program, want_hash, state_len, code_len, entrypoints) in cases {
        let got = recognize(&program).expect("a Zealous reveal is argent-compiled");
        assert_eq!(got.split, Split::Proven, "the guard fixes the state window");
        assert_eq!(got.generation, Generation::Fork, "selector rungs date the fork");
        assert_eq!(got.terminator, Some(Terminator::OpReturn));
        // The head is the lone OP_TOALTSTACK: state is everything the guard
        // fences, code is the guard's close onward.
        assert_eq!(got.head_len, 1);
        assert_eq!(got.state_len, state_len);
        assert_eq!(got.code_len, code_len);
        assert_eq!(got.entrypoint_count(), entrypoints);
        assert_eq!(got.template_hash, hex32(want_hash));
        // The split partitions the program with nothing left over, and the
        // hash is exactly the hash of the two halves it names.
        let (head, state, code) = got.slices(&program);
        assert_eq!(head.len() + state.len() + code.len(), program.len());
        assert_eq!(template_hash(head, code), got.template_hash);
    }
}

/// The leaf tier reproduces the commitment `argent_template.rs` proves from
/// the 905 B code half alone — but from the WHOLE program, by finding that
/// boundary rather than being handed it. That is the only thing that makes
/// the cut worth anything, and it is still labelled a candidate: nothing in
/// these bytes says the cut is there, only the market's stored constant does.
#[test]
fn the_leaf_split_lands_on_the_known_boundary() {
    let program = order_cell();
    let got = recognize(&program).expect("the order cell is argent-compiled");
    assert_eq!(got.split, Split::Candidate);
    assert_eq!(got.generation, Generation::Fork, "a guardless leaf is fork era");
    assert_eq!(got.terminator, None, "a leaf has no dispatch tail to date it");
    assert_eq!(got.entrypoints[0].dispatch, Dispatch::Leaf);
    assert_eq!((got.head_len, got.state_len, got.code_len), (0, 75, 905));
    assert_eq!(got.entrypoint_count(), 1);

    let (_, _, code) = got.slices(&program);
    let known = hex_bytes(include_str!("fixtures/argent-order-cell-suffix.hex"));
    assert_eq!(code, known.as_slice(), "the cut must find the 905 B code half");
    assert_eq!(
        got.template_hash,
        hex32("49470b2c17f3cb3cf782c1dc721053ba858366c5f15361147ae884826847f85d"),
        "and reproduce the constant the Zealous market stores"
    );
}

/// CORRECTION 1. The dispatch terminator dates a compiler revision; it does
/// not identify a build. Both terminators appear on real programs that are
/// NOT argent: `75 00 69` is the older silverscript (d25bd34), `6a` is what
/// the fork the box runs and 1.0 both emit. A detector keyed on the tail
/// alone calls every KRON market argent.
#[test]
fn both_terminators_appear_on_programs_that_are_not_argent() {
    let drop_fail = include_bytes!("../fixtures/kron_curve_v1.bin").as_slice();
    let op_return = include_bytes!("../fixtures/kcm_curve_v1.bin").as_slice();

    assert_eq!(dispatch_envelope(drop_fail), Some((6, Terminator::DropFail)));
    assert_eq!(dispatch_envelope(op_return), Some((5, Terminator::OpReturn)));
    // Both clear the envelope. Neither is argent.
    assert_eq!(recognize(drop_fail), None);
    assert_eq!(recognize(op_return), None);
}

/// CORRECTION 2. The authorised-output indices are a permutation of
/// `0..N-1`, not an ascending run, and `N = 0` is a real branch. The token
/// cell proves both in one program: its second rung pins output 1 before
/// output 0, and its fourth rung authorises nothing at all. Requiring
/// ascending order and `N >= 1` would drop this entire family.
#[test]
fn the_token_cell_authorises_out_of_order_and_authorises_nothing() {
    let got = recognize(&token_cell()).expect("argent");
    let counts: Vec<Authorised> = got
        .entrypoints
        .iter()
        .map(|e| e.authorised)
        .collect();
    assert_eq!(
        counts,
        vec![Authorised::Exact(1), Authorised::Exact(2), Authorised::Exact(1), Authorised::Exact(0)]
    );
    // Selectors ascend from zero, one rung per trailing ENDIF, and a fork
    // rung carries no tag.
    let selectors: Vec<Option<u8>> = got.entrypoints.iter().map(|e| e.selector()).collect();
    assert_eq!(selectors, vec![Some(0), Some(1), Some(2), Some(3)]);
    assert!(got.entrypoints.iter().all(|e| e.tag().is_none()));
}

/// Every contract of every 1.0 artifact argent ships as an example (30 at
/// 867b080 and at e76ee07): recognised, dated 1.0, hashed under BLAKE3 to the compiler's own
/// `compiled.template_hash`, one rung per declared entry with the tags
/// matching. Reads the directory named by `KASCOV_ARGENT_EXAMPLES` (argent's
/// `examples/build`), so it is ignored by default and run by hand:
///
/// ```text
/// KASCOV_ARGENT_EXAMPLES=/path/to/argent/examples/build \
///   cargo test -p kascov-decode --test argent_detect -- --ignored
/// ```
#[test]
#[ignore]
fn every_shipped_v1_example_is_recognised() {
    let root = std::env::var("KASCOV_ARGENT_EXAMPLES").expect("KASCOV_ARGENT_EXAMPLES");
    let mut contracts = 0usize;
    let mut apps = 0usize;
    for app in std::fs::read_dir(&root).expect("examples/build") {
        let path = app.unwrap().path().join("artifact.json");
        if !path.is_file() {
            continue;
        }
        apps += 1;
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for (name, contract) in doc["sil_abi"]["contracts"].as_object().expect("1.0 contracts map") {
            let bytes = |v: &serde_json::Value| -> Vec<u8> {
                v.as_array().unwrap().iter().map(|b| b.as_u64().unwrap() as u8).collect()
            };
            let script = bytes(&contract["compiled"]["bytecode"]);
            let claimed = bytes(&contract["compiled"]["template_hash"]);
            let span = &contract["compiled"]["state_span"];
            let offset = span["offset"].as_u64().unwrap() as usize;
            let len = span["len"].as_u64().unwrap() as usize;
            let seen = recognize(&script).unwrap_or_else(|| panic!("{}: {name} not recognised", path.display()));
            assert_eq!(seen.generation, Generation::V1, "{name}");
            assert_eq!(seen.split, Split::Proven, "{name}");
            assert_eq!((seen.head_len, seen.state_len), (offset, len), "{name}");
            assert_eq!(seen.template_hash.as_slice(), claimed.as_slice(), "{name}");
            let entries = contract["entries"].as_object().unwrap();
            assert_eq!(seen.entrypoint_count(), entries.len(), "{name}");
            let mut declared: Vec<String> =
                entries.values().map(|e| e["dispatch_tag"].as_str().unwrap().to_string()).collect();
            declared.sort();
            let mut read: Vec<String> = seen.entrypoints.iter().map(|e| hex::encode(e.tag().unwrap())).collect();
            read.sort();
            assert_eq!(read, declared, "{name}");
            contracts += 1;
        }
    }
    eprintln!("{contracts} contracts across {apps} apps recognised as 1.0");
    assert!(contracts >= 30, "argent ships 30 example contracts (867b080, e76ee07), saw {contracts}");
}

/// A commitment may pin a narrower state window than the guard marks, so the
/// head is allowed to grow one state push at a time. The first candidate is
/// always the primary split; the code half never moves.
#[test]
fn candidate_splits_walk_the_head_forward() {
    let program = token_cell();
    let got = recognize(&program).expect("argent");
    let splits = got.candidate_splits(&program);
    assert_eq!(splits[0], (1, got.template_hash));
    assert!(splits.len() > 1, "the state block has pushes to absorb");
    for (boundary, hash) in &splits {
        assert!(*boundary <= got.head_len + got.state_len);
        assert_eq!(
            *hash,
            template_hash(&program[..*boundary], &program[got.head_len + got.state_len..])
        );
    }
    // Distinct boundaries are distinct templates — the LE64 length framing in
    // argent::template_hash is what guarantees it, and it is what stops a
    // narrower-window guess from colliding with the real family.
    let mut hashes: Vec<[u8; 32]> = splits.iter().map(|(_, h)| *h).collect();
    hashes.sort_unstable();
    let before = hashes.len();
    hashes.dedup();
    assert_eq!(hashes.len(), before);
}

/// Every build fixture the repo carries for another matcher: KRON's curve and
/// pool families, the third-party curve and order builds, the pair pool, the
/// KCC20 cells, a genesis0 shape. None of them is argent. Several clear the
/// dispatch envelope and are refused at the leader preamble, which is the
/// whole reason the preamble clause is in the rule.
#[test]
fn no_repo_fixture_is_argent() {
    let fixtures: [(&str, &[u8]); 22] = [
        ("kron_curve_v1", include_bytes!("../fixtures/kron_curve_v1.bin")),
        ("kron_curve_v2", include_bytes!("../fixtures/kron_curve_v2.bin")),
        ("kron_curve_v3", include_bytes!("../fixtures/kron_curve_v3.bin")),
        ("kron_pool_v1", include_bytes!("../fixtures/kron_pool_v1.bin")),
        ("kron_pool_v2", include_bytes!("../fixtures/kron_pool_v2.bin")),
        ("kron_pool_v3", include_bytes!("../fixtures/kron_pool_v3.bin")),
        ("kron_pool_tn_a", include_bytes!("../fixtures/kron_pool_tn_a.bin")),
        ("kcm_curve_v1", include_bytes!("../fixtures/kcm_curve_v1.bin")),
        ("kcm_curve_live_tn10", include_bytes!("../fixtures/kcm_curve_live_tn10.bin")),
        ("kcm_order_v1", include_bytes!("../fixtures/kcm_order_v1.bin")),
        ("kcm_order_live_tn10", include_bytes!("../fixtures/kcm_order_live_tn10.bin")),
        ("pair_pool_tn_c", include_bytes!("../fixtures/pair_pool_tn_c.bin")),
        ("kcc20_a_a", include_bytes!("../fixtures/kcc20_a_a.bin")),
        ("kcc20_b_a", include_bytes!("../fixtures/kcc20_b_a.bin")),
        ("kcc20_c_a", include_bytes!("../fixtures/kcc20_c_a.bin")),
        ("kcc20_minter_a", include_bytes!("../fixtures/kcc20_minter_a.bin")),
        ("kcc20_unguarded_kron", include_bytes!("../fixtures/kcc20_unguarded_kron.bin")),
        ("vesting_kron_suffix", include_bytes!("../fixtures/vesting_kron_suffix.bin")),
        ("g0_list_v1_a", include_bytes!("../fixtures/g0_list_v1_a.bin")),
        ("g0_buy_v1_a", include_bytes!("../fixtures/g0_buy_v1_a.bin")),
        ("pure_a", include_bytes!("../fixtures/pure_a.bin")),
        ("slot_mint_di4m_a", include_bytes!("../fixtures/slot_mint_di4m_a.bin")),
    ];
    for (name, program) in fixtures {
        assert_eq!(recognize(program), None, "{name} must not read as argent");
    }
    // Named so the negative is specific rather than incidental: these three
    // carry silverscript's dispatch envelope and fail only on the preamble.
    for name in ["kron_curve_v1", "kcm_curve_v1", "kcc20_a_a"] {
        let program = fixtures
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, p)| *p)
            .expect("listed above");
        assert!(
            dispatch_envelope(program).is_some(),
            "{name} clears the envelope"
        );
    }
}

/// Mutation is the cheapest way to show the clauses are load-bearing rather
/// than decorative: each edit below breaks exactly one of them on a program
/// that is otherwise a real, accepted argent reveal.
#[test]
fn each_clause_is_load_bearing() {
    let good = token_cell();
    assert!(recognize(&good).is_some());

    // The guard byte. Without it there is no proven state window, and the
    // program is not a leaf either (its body is a dispatch chain).
    let mut no_guard = good.clone();
    no_guard[0] = 0x75;
    assert_eq!(recognize(&no_guard), None);

    // The tail. Drop one ENDIF and the rung count no longer matches the chain.
    assert_eq!(recognize(&good[..good.len() - 1]), None);

    // The leader preamble. OpAuthOutputCount becomes an unrelated opcode, so
    // the branch asserts nothing about which outputs it authorises — this is
    // the clause that separates argent from every KRON build.
    let count_site = good
        .windows(2)
        .position(|w| w == [0xb9, 0xcb])
        .expect("the token cell has a count site");
    let mut no_preamble = good.clone();
    no_preamble[count_site + 1] = 0x76;
    assert_eq!(recognize(&no_preamble), None);

    // The permutation. Repoint the one site that pins output 1 at output 0,
    // leaving `[0, 0]` where `[1, 0]` was: two pins on one output, which is
    // exactly the hand-written-covenant shape the rule exists to exclude.
    let idx_site = good
        .windows(3)
        .position(|w| w == [0xb9, 0x51, 0xcc])
        .expect("the token cell pins output 1 exactly once");
    let mut duplicated = good.clone();
    duplicated[idx_site + 1] = 0x00;
    assert_eq!(recognize(&duplicated), None);
}
