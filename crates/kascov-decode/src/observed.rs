//! Skeletons derived from real revealed programs observed on chain.
//!
//! Unlike [`crate::silverscript_skeletons`] (compiler dumps built with
//! sentinel arguments), these families were learned from spend-time P2SH
//! reveals in the TN10 index: each fixture pair is two distinct on-chain
//! instances of the same compiled contract, and the derivation marks the
//! positions where real deployments disagree as labeled slots
//! ([`Skeleton::derive_observed`]; [`RepeatSkeleton::derive`] additionally
//! takes a second arity so the repeated per-output block is matched as a
//! group). Names follow the protocol tags the covenants themselves put in
//! their accepted-transaction payloads — the evidence is cited per family.
//!
//! Fixture bytes are verbatim reveal programs (`p2sh_reveal` verified them
//! against the committed state hash before they were captured), stored under
//! `fixtures/` and embedded at compile time.

use crate::{RepeatSkeleton, Skeleton};

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!("../fixtures/", $name, ".bin")).as_slice()
    };
}

/// Fixed-shape families seen on TN10. Every skeleton derives from two
/// distinct real instances; a family compiled at several arities/branches
/// registers one skeleton per observed build, all under the same name.
pub fn observed_skeletons() -> Vec<Skeleton> {
    let mut out = Vec::new();
    let mut add = |s: Option<Skeleton>| out.extend(s);

    // PURE: 14 covenants / ~1.4k spends+burns whose event payloads all read
    // "PURE\0…". One inlined argument: the key that OpCheckSigFromStack
    // verifies right after the leading Dup·SHA256 of the witness message.
    add(Skeleton::derive_observed(
        "PURE",
        &[fixture!("pure_a"), fixture!("pure_b")],
        &["signer_pubkey"],
    ));

    // genesis0 marketplace listings. A listing covenant is spent twice, and
    // the accepted-tx payload names the program that ran each time:
    // {"t":"genesis0-list","v":1,…} for the first spend and
    // {"t":"genesis0-buy","v":1,…} (or …-delist) for the second — 993+982
    // covenants of the larger build, 205+188 of the smaller. The "list"
    // program embeds the byte template of the follow-up "buy" state
    // (`next_state_template` below literally starts with the buy program's
    // post-state bytes), which is how the two stages were tied together.
    add(Skeleton::derive_observed(
        "genesis0 · list",
        &[fixture!("g0_list_v1_a"), fixture!("g0_list_v1_b")],
        &["state_hash_a", "state_hash_b", "state_hash_c", "min_amount", "next_state_template"],
    ));
    add(Skeleton::derive_observed(
        "genesis0 · buy",
        &[fixture!("g0_buy_v1_a"), fixture!("g0_buy_v1_b")],
        &[
            "state_amount",
            "state_hash_a",
            "state_hash_b",
            "min_amount",
            "output_spk_hash_a",
            "output_amount",
            "output_spk_hash_b",
        ],
    ));
    add(Skeleton::derive_observed(
        "genesis0 · list",
        &[fixture!("g0_list_v2_a"), fixture!("g0_list_v2_b")],
        &["witness_hash", "output_spk_hash", "salt_a", "salt_b"],
    ));
    add(Skeleton::derive_observed(
        "genesis0 · buy",
        &[fixture!("g0_buy_v2_a"), fixture!("g0_buy_v2_b")],
        &["output_spk_hash", "price", "witness_hash", "salt"],
    ));

    // genesis0 collection registry: 41 covenants whose spends carry
    // {"t":"genesis0","v":1,"col":…}. One inlined argument — the amount the
    // covenant sheds per mint (input amount minus `amount_step` must equal
    // output 0's amount).
    add(Skeleton::derive_observed(
        "genesis0 · collection",
        &[fixture!("g0_col_a"), fixture!("g0_col_b")],
        &["amount_step"],
    ));

    // KCC20 token (kcc20.sil): state rides as the leading
    // OpToAltStack-guarded pushes and matches the contract's field order —
    // byte[32] ownerIdentifier, byte identifierType (0x00 pubkey / 0x01
    // script hash / 0x02 covenant id), int amount, bool isMinter. Three
    // builds circulate on TN10 (~200 covenants): the compiler unrolls
    // `maxCovIns`/`maxCovOuts` loops and constant-folds the isMinter branch,
    // so each build gets its own skeleton under the one name.
    for f in [
        [fixture!("kcc20_a_a"), fixture!("kcc20_a_b")],
        [fixture!("kcc20_b_a"), fixture!("kcc20_b_b")],
        [fixture!("kcc20_c_a"), fixture!("kcc20_c_b")],
    ] {
        add(Skeleton::derive_observed(
            "KCC20 token",
            &f,
            &["owner_identifier", "identifier_type", "amount", "is_minter"],
        ));
    }

    // KCC20 minter/controller: pins two covenant ids with OpInputCovenantId
    // + OpOutputCovenantId (each id is required on the way in *and* out, so
    // the two uses fold into one slot each) and embeds the KCC20 token
    // template bytes three times to validate the governed token states it
    // mints into. Both pinned ids resolve to live "KCC20 token" covenants in
    // the TN10 index.
    add(Skeleton::derive_observed(
        "KCC20 minter",
        &[fixture!("kcc20_minter_a"), fixture!("kcc20_minter_b")],
        &["kcc20_covenant_a", "kcc20_covenant_b"],
    ));

    out
}

/// Variable-arity families: one skeleton matches every repeat count.
pub fn observed_repeat_skeletons() -> Vec<RepeatSkeleton> {
    let mut out = Vec::new();

    // genesis0 slot-mint — the DI4M/GZ4M lanes' mint contract and by far the
    // busiest program on TN10 (~8.5k spends, ~40% of all P2SH reveal
    // traffic). Every spend's payload opens with the 5-byte lane tag
    // ("DI4M2"/"GZ4M1") followed by {"t":"genesis0-slot-mint","v":2,…}. The
    // build repeats one `OpTxOutputAmount…OpTxOutputSpk` check per
    // collection output, so the two observed arities (two checks for DI4M2,
    // one for GZ4M1) derive a repeat group; arity-dependent constants like
    // the minimum output count become slots automatically.
    if let Some(s) = RepeatSkeleton::derive(
        "genesis0 · slot-mint",
        &[fixture!("slot_mint_di4m_a"), fixture!("slot_mint_di4m_b")],
        &[fixture!("slot_mint_gz4m_a"), fixture!("slot_mint_gz4m_b")],
        &[
            "min_outputs",
            "lane_tag",
            "payload_hash_a",
            "payload_hash_b",
            "payload_len_a",
            "payload_len_b",
            "instance_salt",
        ],
        &["output_index", "output_amount", "output_index", "output_spk_hash"],
    ) {
        out.push(s);
    }

    out
}
