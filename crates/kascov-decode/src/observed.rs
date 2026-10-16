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
        &[
            "state_hash_a",
            "state_hash_b",
            "state_hash_c",
            "min_amount",
            "next_state_template",
        ],
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

    /* ---------------------------------------------------------------------
    KaspaCom's genesis0 launchpad and KasWare's vaults, decoded from TN10
    spend reveals 2026-08-20. Every fixture below is a verbatim revealed
    program whose blake2b matched its committed P2SH hash before capture,
    and every pair was checked to derive exactly the labels it is given —
    a pair that agrees on a real argument would silently fold it into a
    constant, so the pairs are chosen to differ at EVERY slot.
    --------------------------------------------------------------------- */

    // genesis0 · root — the 56-byte boot cell that opens a collection
    // lineage, fixing the covenant id before the real program exists.
    // Payload spellings vary across generations ("genesis0-root",
    // "genesis0-deploy", {"note":"root"}) but the bytes are one program.
    add(Skeleton::derive_observed(
        "genesis0 · root",
        &[fixture!("g0_root_a"), fixture!("g0_root_b")],
        &["owner_pubkey_hash"],
    ));

    // Two more slot-mint builds, and the supply meter is the interesting
    // part: the covenant re-creates itself at output 0 with the same id and
    // script but exactly one sompi less, so its remaining BALANCE is the
    // collection's remaining supply. These builds pin the literal string
    // {"t":"genesis0-slot-mint","v":3, in their own bytes, which is why they
    // carry the family name rather than a counter name of our invention.
    // They differ from each other only in the output-count guard (4 vs 3);
    // the 4-output build carries its payout as arguments.
    add(Skeleton::derive_observed(
        "genesis0 · slot-mint",
        &[fixture!("g0_slot_mint_565_a"), fixture!("g0_slot_mint_565_b")],
        &[
            "self_covenant_id",
            "payload_hash_a",
            "payload_hash_b",
            "item_code_hash",
            "payout_amount",
            "payout_spk_hash",
            "instance_salt",
        ],
    ));
    add(Skeleton::derive_observed(
        "genesis0 · slot-mint",
        &[fixture!("g0_slot_mint_520_a"), fixture!("g0_slot_mint_520_b")],
        &[
            "self_covenant_id",
            "payload_hash_a",
            "payload_hash_b",
            "item_code_hash",
            "instance_salt",
        ],
    ));

    // genesis0 · slot-mint, two later builds of the family the repeat
    // skeleton below already names. Both pin their own covenant id and
    // require OpAuthOutputCount == 1, which the v2 build did not, so the
    // repeat skeleton misses them and 226 covenants stayed unnamed.
    // NOTE on the 421-byte pair: its second fixture is NOT one of the eight
    // sampled programs. All eight happen to pay the same 2 KAS, which would
    // have frozen a real per-deployment argument into a constant; the pair
    // used here takes a genuine differing instance (covenant 1efdbbf7…,
    // whose utxo commits to exactly these bytes) so payout_amount stays a
    // slot and the skeleton matches gates deployed at any price.
    add(Skeleton::derive_observed(
        "genesis0 · slot-mint",
        &[fixture!("g0_slot_mint_413_a"), fixture!("g0_slot_mint_413_b")],
        &[
            "self_covenant_id",
            "payload_hash_a",
            "payload_hash_b",
            "payout_amount",
            "payout_spk_hash",
            "instance_salt",
        ],
    ));
    add(Skeleton::derive_observed(
        "genesis0 · slot-mint",
        &[fixture!("g0_slot_mint_421_a"), fixture!("g0_slot_mint_421_b")],
        &[
            "self_covenant_id",
            "payload_hash_a",
            "payload_hash_b",
            "output_amount",
            "output_spk_hash",
            "instance_salt",
        ],
    ));

    // A later generation of the genesis0 marketplace (first activity DAA
    // ~530.3M,
    // against ~506.9M for the pinned v1 buy and ~508.7M for its sibling).
    // The buy half differs from v1 by one opcode — the retained-value floor
    // now reads this input's amount instead of input 0's — plus new floors
    // and a new platform script.
    add(Skeleton::derive_observed(
        "genesis0 · buy",
        &[fixture!("g0_buy_v3_a"), fixture!("g0_buy_v3_b")],
        &["price_sompi", "payout_spk_hash", "next_state_code_hash"],
    ));
    // The item-state half that mints the listing. Its fixture pair spans the
    // two body generations (334-byte and 396-byte listing bodies) so both
    // the body and its padding stay slots.
    add(Skeleton::derive_observed(
        "genesis0 · list",
        &[fixture!("g0_list_v3_a"), fixture!("g0_list_v3_b")],
        &[
            "owner_pubkey_hash",
            "instance_salt",
            "self_code_hash",
            "listing_program_body",
            "size_padding",
        ],
    ));

    // Two sale styles the seller arms at spend time. style-list commits a
    // depth-2 merkle root over the available sale tails; dutch-list arms a
    // declining-price sale (its creating payload reads kind:"dutchB").
    add(Skeleton::derive_observed(
        "genesis0 · style-list",
        &[fixture!("g0_style_list_a"), fixture!("g0_style_list_b")],
        &[
            "seller_pubkey_hash",
            "instance_salt",
            "code_commit",
            "list_style_root",
        ],
    ));
    add(Skeleton::derive_observed(
        "genesis0 · dutch-list",
        &[fixture!("g0_dutch_list_a"), fixture!("g0_dutch_list_b")],
        &["instance_salt", "code_commit", "next_state_template"],
    ));

    // genesis0 · auction — an item covenant holding its own 1.1 KAS carry
    // and escrowing the standing highest bid on top of it. The reserve,
    // minimum increment and fee schedule are identical in every observed
    // build, so they derive as constants; only the parties, the standing
    // bid and the committed programs vary.
    add(Skeleton::derive_observed(
        "genesis0 · auction",
        &[fixture!("g0_auction_a"), fixture!("g0_auction_b")],
        &[
            "seller_pubkey",
            "expiry_daa",
            "top_bidder_pubkey",
            "top_bid_amount",
            "top_bid_item_salt",
            "item_program_hash",
            "auction_program_hash",
            "pct_fee_spk_hash",
            "flat_fee_spk_hash",
        ],
    ));

    // genesis0 KRC claim gate: the booted state and the leaf it activates
    // into. The boot script commits to the leaf's script pubkey by hash.
    add(Skeleton::derive_observed(
        "genesis0 · krc-gate-boot",
        &[fixture!("g0_krc_gate_boot_a"), fixture!("g0_krc_gate_boot_b")],
        &["output_spk_hash", "owner_pubkey_hash"],
    ));

    // KasWare vaults. One six-arm branch selector compiled once per armed
    // arm: the unarmed arms all carry the same all-zero key and fold into
    // constants, so each armed arm is its own shape. Their kw-vault-create
    // v2 payloads publish the literal redeem hex and p2sh, and blake2b of
    // each fixture re-derives that exact published address.
    add(Skeleton::derive_observed(
        "KasWare · vault-schnorr",
        &[fixture!("kw_vault_schnorr_a"), fixture!("kw_vault_schnorr_b")],
        &["schnorr_pubkey", "vault_id"],
    ));
    add(Skeleton::derive_observed(
        "KasWare · vault-pqcold",
        &[fixture!("kw_vault_pqcold_a"), fixture!("kw_vault_pqcold_b")],
        &["pq_merkle_root", "output_spk_hash", "vault_id"],
    ));
    add(Skeleton::derive_observed(
        "KasWare · vault-htlc",
        &[fixture!("kw_vault_htlc_a"), fixture!("kw_vault_htlc_b")],
        &[
            "output_spk_hash",
            "preimage_hash",
            "claim_pubkey",
            "vault_salt",
        ],
    ));
    // Their unlock payloads call this arm "dms"; the name follows their slug
    // like its siblings. The arm ends in OpTrue with no signature check: after
    // lock_daa the sweep is permissionless, bounded only by the committed
    // destination and the value floor. Say that plainly anywhere it shows.
    add(Skeleton::derive_observed(
        "KasWare · vault-dms",
        &[fixture!("kw_vault_dms_a"), fixture!("kw_vault_dms_b")],
        &["lock_daa", "output_spk_hash", "vault_salt"],
    ));

    // vprogs settlement covenant, the tic-tac-toe build: the first vprog
    // (verifiable program) running on testnet-10. The game's own server
    // (vprogs-tt.izio.fr/api/config) names covenant 85165a6f…3fb097 as the one
    // it settles, and each settlement spend proves a RISC Zero receipt on L1
    // (OpZkPrecompile) and checks OpChainblockSeqCommit. The 99-byte prefix is
    // vprogs' own build_redeem_script layout (kaspanet/vprogs,
    // zk/backend/risc0/covenant/src/script.rs): OpData32 prev_lane_tip,
    // OpData32 prev_state, OpData32 lane_key. The two fixtures are consecutive
    // settlements and differ in exactly the first two; lane_key, the image ids
    // and every other pin are constants of this build, so another vprog app
    // is a different build. Fixture a hashes (blake2b-256) to the P2SH
    // commitment its settlement output locks, 929ea610…fdba0125.
    add(Skeleton::derive_observed(
        "vprogs · tic-tac-toe settlement",
        &[fixture!("vprogs_ttt_settlement_a"), fixture!("vprogs_ttt_settlement_b")],
        &["lane_tip", "state"],
    ));

    // vprogs exit permission, for an exit tree of depth 1 (one or two exits):
    // the output a settlement opens when its batch lets funds leave the lane,
    // spent later by each exit claim. OpData32 exit_root, then OpData8 the
    // number of exits not yet claimed (u64 LE); the 447 bytes after them are
    // the same in every copy. Rebuilt byte for byte from vprogs'
    // build_permission_redeem_script(root, count, 1) as the tic-tac-toe game
    // builds it (kaspanet/vprogs branch fix/settlement-watch-wedge,
    // zk/backend/risc0/api/src/permission_script.rs). The script names no
    // app, lane or image, so the name is generic; deeper trees are longer
    // scripts and would be pins of their own. Fixture a (2 unclaimed) was
    // revealed by d9a42cb5…, fixture b (1 unclaimed) by 3ae86145….
    add(Skeleton::derive_observed(
        "vprogs · exit permission (depth 1)",
        &[fixture!("vprogs_exit_perm_d1_a"), fixture!("vprogs_exit_perm_d1_b")],
        &["exit_root", "unclaimed_exits"],
    ));

    // KaspaCom's current token program: 8,076 bytes, 6,476 instructions, one
    // single shape across every instance found. This is the family behind the
    // tokens their testnet UI lists (WBLF, KOIN, KPOP, KARBON, FINAL6 …),
    // none of which the KCC20 skeletons or the structural state-block
    // fallback could name, so they all sat as anonymous p2sh commitments.
    //
    // The prize is the prologue: ticker and name are fixed-width padded ASCII
    // IN THE PROGRAM, so a token's identity is readable from chain with no
    // registry, no payload JSON and no trust in the deployer. Cross-checked
    // against KaspaCom's own UI: mint_price_sompi 500,000 = the 0.005 KAS it
    // shows for KOIN and 100,000 = 0.001 KAS for KPOP, and remaining_supply
    // over decimals_scale reproduces each listed supply.
    //
    // remaining_supply is LIVE state, not a constant: successive reveals of
    // one covenant count it down (8000, 7000, … 1000 on the M8 instance).
    // mint_mode reads 2 on every token their UI lists as a public mint and 0
    // on the ones it does not. `flags` takes 0 or 2 and its meaning is NOT
    // established — it is labeled, not interpreted.
    //
    // The fixture pair is deliberate: `flags` and the salt pair differ in
    // exactly ONE observed instance, so any pair without it folds three real
    // arguments into constants and silently narrows the pin.
    add(Skeleton::derive_observed(
        "KaspaCom · token",
        &[fixture!("kcom_token_a"), fixture!("kcom_token_b")],
        &[
            "owner_identifier",
            "identifier_type",
            "amount",
            "mint_mode",
            "mint_price_sompi",
            "token_id",
            "ticker",
            "token_name",
            "decimals_scale",
        ],
    ));

    // Zealous Swap's token cells. Attribution is by their platform fee key
    // d4ae9d3d7ea2…, which one fixture below carries as its own owner.
    //
    // These are KCC20-SHAPED but not KCC20 as kascov models it. The state is
    // owner(32) || identifier_type(1) || amount(8) || token_id(32), where
    // kascov's block ends in a ONE-byte is_minter flag instead of a 32-byte
    // id. That single difference is why `locate_state_block` never found
    // them and why all 60 of their launches sat unnamed.
    //
    // They are therefore NOT registered as "KCC20 token" on purpose. Doing so
    // would make `decode_token_state` match the template, look for a field
    // named is_minter, fail to find it, return None — and SKIP the structural
    // fallback that would otherwise have had a chance. A wrong name here is
    // worse than no name. Under their own name the KCC20 path is untouched.
    //
    // Unlike KaspaCom's token program, nothing here carries a ticker: 400
    // Zealous programs were checked and none contains one. Their ticker and
    // name live only in the launch payload (KCC20M1 || len || TICKER ||
    // len || NAME), so that identity is CLAIMED by the deployer, never
    // committed by the covenant. Any surface showing it must say so.
    for f in [
        [fixture!("zealous_launch_a_a"), fixture!("zealous_launch_a_b")],
        [fixture!("zealous_launch_b_a"), fixture!("zealous_launch_b_b")],
    ] {
        add(Skeleton::derive_observed(
            "Zealous · token cell",
            &f,
            &["owner_identifier", "identifier_type", "amount", "token_id"],
        ));
    }


    // ZealousSwap (kaspa-tn10.zealousswap.com), decoded 2026-09-04/05 from
    // TN10 reveals against the venue's own launch pages and API: a launch is
    // a curve covenant, a token cell and, once 270k tKAS is raised, a pool.
    // Five generations circulate; the payload prefix moved KCC20M1 -> KCC20M3
    // and the platform fee key d4ae9d3d… -> 01150ea3… along the way. "curve"
    // and "pool" are the venue's words; "position cell", "liquidity position"
    // and "factory" are kascov's structural names (their bundle has none).
    // One name per role, one skeleton per build, all under the "Zealous"
    // prefix the token cell already carries so a search for "zealous" lists
    // every family. Position and liquidity-position cells carry no
    // identifier_type byte and are deliberately NOT cell families: their
    // amounts never enter the token index. Every skeleton below was checked
    // against every cached reveal of its shape (2,542 programs, all match).
    //
    // Current generation (WBLF + ROCKET fixtures): the curve is a
    // virtual-reserve constant product (X = 1.35e12 sompi, Y = 1.125e17
    // raw tokens, 993/1000 fee split 7/10, graduation at 2.7e12 sompi); the
    // pool a 997/1000 constant product. The 2,590 B token cell of this
    // generation is the KCC-0020 State layout (owner scheme 0x04) and is NOT
    // registered here: under the legacy scheme table it would be named but
    // never decoded as a token (see kcc20::CELL_FAMILIES), so it waits for
    // the KCC-0020 route.
    add(Skeleton::derive_observed(
        "Zealous · curve",
        &[fixture!("zealous_curve_10663_a"), fixture!("zealous_curve_10663_b")],
        &[
            "creator",
            "meta_digest",
            "phase",
            "kas_raised",
            "tokens_sold",
            "creator_fee_acc",
            "protocol_fee_acc",
            "claim_pot",
            "token_covenant_id",
            "pool_covenant_id",
        ],
    ));
    // Pool accumulators: lp_supply is proven by the liquidity-position cell
    // deltas; the fee/reserve accumulators follow the swap replay.
    add(Skeleton::derive_observed(
        "Zealous · pool",
        &[fixture!("zealous_pool_18346_a"), fixture!("zealous_pool_18346_b")],
        &[
            "lp_supply",
            "protocol_kas_fee_acc",
            "protocol_token_fee_acc",
            "token_reserve_acc",
            "token_covenant_id",
            "creator",
            "creator_kas_fee_acc",
            "creator_token_fee_acc",
        ],
    ));
    // Revealed by the curve covenants themselves (11 covenants / 116 reveals).
    add(Skeleton::derive_observed(
        "Zealous · position cell",
        &[fixture!("zealous_position_1019_a"), fixture!("zealous_position_1019_b")],
        &["owner_identifier", "amount"],
    ));
    // Pool-side LP share cell: amounts equal the pool's lp_supply deltas.
    add(Skeleton::derive_observed(
        "Zealous · liquidity position",
        &[fixture!("zealous_lp_906_a"), fixture!("zealous_lp_906_b")],
        &["owner_identifier", "amount"],
    ));

    // Retired generations, same roles. Where a pair comes from ONE covenant
    // the per-launch arguments fold into constants and the skeleton names
    // exactly that build's reveals (curve 8,643 B and 8,959 B, factory 561 B
    // and 573 B, token cell 1,324 B); every other pair spans two launches.
    // The 15,022 B pool needs three instances: any two agree on two fields.
    add(Skeleton::derive_observed(
        "Zealous · curve",
        &[fixture!("zealous_curve_8970_a"), fixture!("zealous_curve_8970_b")],
        &[
            "own_template_hash",
            "token_id",
            "phase",
            "kas_raised",
            "tokens_sold",
            "fee_acc_a",
            "fee_acc_b",
            "redemption_pot",
            "token_covenant_id",
            "pool_covenant_id",
            "pool_template_commit",
        ],
    ));
    add(Skeleton::derive_observed(
        "Zealous · curve",
        &[fixture!("zealous_curve_8802_a"), fixture!("zealous_curve_8802_b")],
        &[
            "launcher_key_hash",
            "token_id",
            "phase",
            "kas_raised",
            "tokens_sold",
            "fee_acc_a",
            "fee_acc_b",
            "redemption_pot",
            "token_covenant_id",
            "pool_covenant_id",
        ],
    ));
    add(Skeleton::derive_observed(
        "Zealous · curve",
        &[fixture!("zealous_curve_8959_a"), fixture!("zealous_curve_8959_b")],
        &[
            "phase",
            "kas_raised",
            "tokens_sold",
            "fee_acc_a",
            "fee_acc_b",
            "redemption_pot",
            "token_covenant_id",
            "pool_covenant_id",
        ],
    ));
    add(Skeleton::derive_observed(
        "Zealous · curve",
        &[fixture!("zealous_curve_8643_a"), fixture!("zealous_curve_8643_b")],
        &[
            "phase",
            "kas_raised",
            "tokens_sold",
            "fee_acc_a",
            "fee_acc_b",
            "redemption_pot",
            "token_covenant_id",
            "pool_covenant_id",
        ],
    ));
    add(Skeleton::derive_observed(
        "Zealous · pool",
        &[
            fixture!("zealous_pool_15022_a"),
            fixture!("zealous_pool_15022_b"),
            fixture!("zealous_pool_15022_c"),
        ],
        &[
            "kas_fee_acc",
            "token_fee_acc",
            "token_reserve_acc",
            "token_covenant_id",
            "creator_kas_fee_acc",
            "creator_token_fee_acc",
        ],
    ));
    add(Skeleton::derive_observed(
        "Zealous · pool",
        &[fixture!("zealous_pool_13478_a"), fixture!("zealous_pool_13478_b")],
        &[
            "lp_supply",
            "kas_fee_acc",
            "token_fee_acc",
            "token_reserve_acc",
            "token_covenant_id",
            "launcher_key_hash",
            "creator_kas_fee_acc",
            "creator_token_fee_acc",
        ],
    ));
    add(Skeleton::derive_observed(
        "Zealous · pool",
        &[fixture!("zealous_pool_12568_a"), fixture!("zealous_pool_12568_b")],
        &[
            "token_template_hash",
            "lp_supply",
            "fee_acc_a",
            "fee_acc_b",
            "token_reserve",
            "token_covenant_id",
        ],
    ));
    add(Skeleton::derive_observed(
        "Zealous · pool",
        &[fixture!("zealous_pool_12152_a"), fixture!("zealous_pool_12152_b")],
        &[
            "lp_supply",
            "kas_fee_acc",
            "token_fee_acc",
            "token_reserve_acc",
            "token_covenant_id",
        ],
    ));
    add(Skeleton::derive_observed(
        "Zealous · pool",
        &[fixture!("zealous_pool_12046_a"), fixture!("zealous_pool_12046_b")],
        &[
            "lp_supply",
            "kas_fee_acc",
            "token_fee_acc",
            "token_reserve_acc",
            "token_covenant_id",
        ],
    ));
    add(Skeleton::derive_observed(
        "Zealous · pool",
        &[fixture!("zealous_pool_8622_a"), fixture!("zealous_pool_8622_b")],
        &["lp_supply", "fee_acc_a", "fee_acc_b", "token_reserve", "token_covenant_id"],
    ));
    add(Skeleton::derive_observed(
        "Zealous · pool",
        &[fixture!("zealous_pool_8451_a"), fixture!("zealous_pool_8451_b")],
        &["lp_supply", "fee_acc_a", "fee_acc_b", "token_covenant_id"],
    ));
    add(Skeleton::derive_observed(
        "Zealous · pool",
        &[fixture!("zealous_pool_8403_a"), fixture!("zealous_pool_8403_b")],
        &["aux_template_hash", "lp_supply", "fee_acc_a", "fee_acc_b", "token_covenant_id"],
    ));
    add(Skeleton::derive_observed(
        "Zealous · pool",
        &[fixture!("zealous_pool_8205_a"), fixture!("zealous_pool_8205_b")],
        &["lp_supply", "fee_acc_a", "token_reserve", "token_covenant_id"],
    ));
    add(Skeleton::derive_observed(
        "Zealous · position cell",
        &[fixture!("zealous_position_1040_a"), fixture!("zealous_position_1040_b")],
        &["curve_template_hash", "amount"],
    ));
    add(Skeleton::derive_observed(
        "Zealous · position cell",
        &[fixture!("zealous_position_980_a"), fixture!("zealous_position_980_b")],
        &["curve_template_hash", "owner_identifier", "amount"],
    ));
    add(Skeleton::derive_observed(
        "Zealous · liquidity position",
        &[fixture!("zealous_lp_764_a"), fixture!("zealous_lp_764_b")],
        &["owner_identifier", "amount"],
    ));
    add(Skeleton::derive_observed(
        "Zealous · liquidity position",
        &[fixture!("zealous_lp_687_a"), fixture!("zealous_lp_687_b")],
        &["amount"],
    ));
    add(Skeleton::derive_observed(
        "Zealous · factory",
        &[fixture!("zealous_factory_620_a"), fixture!("zealous_factory_620_b")],
        &["curve_template_hash", "launch_counter"],
    ));
    add(Skeleton::derive_observed(
        "Zealous · factory",
        &[fixture!("zealous_factory_573_a"), fixture!("zealous_factory_573_b")],
        &["launch_counter"],
    ));
    add(Skeleton::derive_observed(
        "Zealous · factory",
        &[fixture!("zealous_factory_561_a"), fixture!("zealous_factory_561_b")],
        &["launch_counter"],
    ));
    // Retired token cells register AFTER the shipped 1,279/1,363 B entries
    // above: same "Zealous · token cell" name, same three token fields, so
    // kcc20::CELL_FAMILIES decodes them and their launches enter the index.
    add(Skeleton::derive_observed(
        "Zealous · token cell",
        &[fixture!("zealous_token_1811_a"), fixture!("zealous_token_1811_b")],
        &["owner_identifier", "identifier_type", "amount"],
    ));
    add(Skeleton::derive_observed(
        "Zealous · token cell",
        &[fixture!("zealous_token_1324_a"), fixture!("zealous_token_1324_b")],
        &["owner_identifier", "identifier_type", "amount"],
    ));
    add(Skeleton::derive_observed(
        "Zealous · token cell",
        &[fixture!("zealous_token_1198_a"), fixture!("zealous_token_1198_b")],
        &["owner_identifier", "identifier_type", "amount"],
    ));

    // KaspaRocket's unnamed launches are a venue cutover, not a reader gap.
    // Proven Sep 2026 by walking every event of every token AND market
    // covenant (not just genesis): launches at genesis_daa >= 521,232,027
    // carry a KCC-0021 genesis payload, launches at or below 521,229,482
    // carry none — on the token, on the market, or on any event of their
    // lives — and the LP_AMM program bytes hold no ASCII identity. The 154
    // launches before that cutover therefore have no name anywhere on chain,
    // and no future decoding pass will find one.
    // KaspaRocket (kasparocket.fun): one LP_AMM contract — their bundle's
    // own word — in three compiles, plus the order escrow behind their book.
    // The market cell itself is a constant 2 KAS: reserves ride the
    // market-owned KCC20 cell, which is why kascov records no trades for
    // these launches yet (a walker is separate work). Graduation is
    // tokens_sold >= 650,000,000 of 1e9; the 22,861 B build keeps its
    // reserves as state pushes, the 18,062 B build keeps none. Mixed fixture
    // pairs (two launches, one non-genesis reveal) keep the state pushes as
    // slots. The single-covenant 75,133 B compile stays unnamed until a
    // second deployment exists (a one-instance pair freezes its arguments).
    add(Skeleton::derive_observed(
        "KaspaRocket · lp-amm",
        &[fixture!("kr_lpamm_22861_a"), fixture!("kr_lpamm_22861_b")],
        &["phase", "tokens_sold", "token_reserve", "kas_reserve", "token_covenant_id"],
    ));
    add(Skeleton::derive_observed(
        "KaspaRocket · lp-amm",
        &[fixture!("kr_lpamm_18062_a"), fixture!("kr_lpamm_18062_b")],
        &["phase", "tokens_sold", "token_covenant_id"],
    ));
    add(Skeleton::derive_observed(
        "KaspaRocket · lp-amm",
        &[fixture!("kr_lpamm_18060_a"), fixture!("kr_lpamm_18060_b")],
        &["curve_denominator", "graduation_cap", "token_covenant_id"],
    ));
    add(Skeleton::derive_observed(
        "KaspaRocket · order-escrow",
        &[fixture!("kr_order_escrow_65541_a"), fixture!("kr_order_escrow_65541_b")],
        &["owner", "owner_spk", "price_sompi", "tokens", "token_covenant_id"],
    ));
    add(Skeleton::derive_observed(
        "KaspaRocket · order-escrow",
        &[fixture!("kr_order_escrow_43641_a"), fixture!("kr_order_escrow_43641_b")],
        &["owner", "price_sompi", "tokens", "token_covenant_id"],
    ));


    // KaspaKaha (tn10.kaspakaha.com): a KAS pool with an LP-share token,
    // state [kas_reserve, token_reserve, lp_shares, token, lp_token, fee_bps,
    // param_7], compiled seven times while its own dev tokens (KRKT, KZEN,
    // KAHA) were iterated on. Attribution is by ownership: every deployment
    // of every build is owned or operated by the identifier 72af7605… that
    // holds most of those tokens; the venue publishes no product slug, so the
    // role word "pool" is used (the "KRON pool" convention). The Aug 2026
    // dossier's "LP_AMM deployer" line points at KaspaRocket's key, but none
    // of KaspaRocket's fingerprints (81bf9ff0… P2PK fee output, no LP token)
    // occur in these bytes; if the builders say otherwise the name follows
    // them. fee_bps folds into a constant (20) on every cached instance of
    // the largest build: a deployment with another fee would stay unnamed
    // until a third fixture lands. Ordered by reveal count, frozen-state last.
    add(Skeleton::derive_observed(
        "KaspaKaha · pool",
        &[fixture!("kaha_pool_102564_a"), fixture!("kaha_pool_102564_b")],
        &[
            "kas_reserve_units",
            "token_reserve",
            "lp_shares",
            "token_covenant_id",
            "lp_token_covenant_id",
            "param_7",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaKaha · pool",
        &[fixture!("kaha_pool_102467_a"), fixture!("kaha_pool_102467_b")],
        &[
            "kas_reserve_units",
            "token_reserve",
            "token_covenant_id",
            "lp_token_covenant_id",
            "fee_bps",
            "param_7",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaKaha · pool",
        &[fixture!("kaha_pool_108722_a"), fixture!("kaha_pool_108722_b")],
        &[
            "kas_reserve_units",
            "token_reserve",
            "lp_shares",
            "token_covenant_id",
            "lp_token_covenant_id",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaKaha · pool",
        &[fixture!("kaha_pool_108623_a"), fixture!("kaha_pool_108623_b")],
        &[
            "kas_reserve_units",
            "token_reserve",
            "lp_shares",
            "token_covenant_id",
            "lp_token_covenant_id",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaKaha · pool",
        &[fixture!("kaha_pool_75692_a"), fixture!("kaha_pool_75692_b")],
        &["kas_reserve_units", "token_reserve", "lp_shares"],
    ));
    add(Skeleton::derive_observed(
        "KaspaKaha · pool",
        &[fixture!("kaha_pool_92140_a"), fixture!("kaha_pool_92140_b")],
        &["kas_reserve_units", "token_reserve", "lp_shares"],
    ));
    add(Skeleton::derive_observed(
        "KaspaKaha · pool",
        &[fixture!("kaha_pool_108714_a"), fixture!("kaha_pool_108714_b")],
        &["token_covenant_id", "lp_token_covenant_id"],
    ));

    // Three more KaspaKaha compiles, attributed by the bytes themselves:
    // every one inlines the same 469ea253… constant the seven builds above
    // carry (14 and 25 sites) and the 93,943 B build inlines the owner
    // identity 72af7605… four times. The 93,943 B build is an earlier
    // pool WITHOUT an LP-share token, one pool per liquidity provider (a
    // token trades across several of them): state is just the two reserves
    // and the token, 21 deployments, 208 trades, 40 distinct reveals all
    // match the two-fixture skeleton. Its trades replay as constant product at
    // 25 bps on 19 of the 21 markets; the other two grow faster than any
    // fee, which is a liquidity add this build cannot flag (no LP covenant
    // to co-move). The market layer replays it as "KaspaKaha pool tn-h"
    // with exactly that rule: unflagged growth is skipped, never a
    // conviction. The two 68 KB builds are one deployment
    // each (55 and 29 reveals), so their token and LP ids fold into
    // constants and a second deployment of either would stay unnamed until
    // a differing fixture lands.
    add(Skeleton::derive_observed(
        "KaspaKaha · pool",
        &[fixture!("kaha_pool_93943_a"), fixture!("kaha_pool_93943_b")],
        &["kas_reserve_units", "token_reserve", "token_covenant_id"],
    ));
    add(Skeleton::derive_observed(
        "KaspaKaha · pool",
        &[fixture!("kaha_pool_68525_a"), fixture!("kaha_pool_68525_b")],
        &["kas_reserve_units", "token_reserve", "lp_shares"],
    ));
    add(Skeleton::derive_observed(
        "KaspaKaha · pool",
        &[fixture!("kaha_pool_68511_a"), fixture!("kaha_pool_68511_b")],
        &["kas_reserve_units", "token_reserve", "lp_shares"],
    ));

    // KaspaCom on TN10, older and newer than the 8,076 B token build pinned
    // above: a 2,671 B token generation (same fee key ce2100e6… and 200 bps
    // constants, padded owner scheme, ticker in the prologue; ticker and
    // token_name are ONE push in every instance, so the slot is labelled
    // ticker) — it carries the three cell fields, so it is safe under the
    // cell-family name. The 4,001 B generation is deliberately NOT pinned:
    // every observed cell has the same owner, so a pair would freeze the
    // owner and the cell-family lookup would then fail closed.
    add(Skeleton::derive_observed(
        "KaspaCom · token",
        &[fixture!("kcom_token_2671_a"), fixture!("kcom_token_2671_b")],
        &[
            "owner_identifier",
            "identifier_type",
            "amount",
            "mint_mode",
            "mint_price_sompi",
            "treasury",
            "ticker",
        ],
    ));
    // Their KCC20V2 wrappers: a wrapper root (`tmpl` KCC20V2V3Wrapper in the
    // payload; state [canonical token id, 0x01]) and the wrapped cells the
    // order book trades (createAsk / createBid payloads). Neither is a cell
    // family: their state carries no scheme byte, so the structural reader
    // keeps deciding whether a cell is a token, exactly as today.
    add(Skeleton::derive_observed(
        "KaspaCom · wrapped token",
        &[fixture!("kcom_wrapped_25496_a"), fixture!("kcom_wrapped_25496_b")],
        &[
            "canonical_token_id",
            "owner_identifier",
            "amount",
            "mode",
            "unit_price_sompi",
            "fee_ticket_id",
            "price_scale",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaCom · wrapped token",
        &[fixture!("kcom_wrapped_22775_a"), fixture!("kcom_wrapped_22775_b")],
        &[
            "canonical_token_id",
            "owner_identifier",
            "amount",
            "mode",
            "unit_price_sompi",
            "price_scale",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaCom · wrapped token",
        &[fixture!("kcom_wrapped_16984_a"), fixture!("kcom_wrapped_16984_b")],
        &[
            "canonical_token_id",
            "owner_identifier",
            "amount",
            "mode",
            "unit_price_sompi",
            "fee_ticket_id",
            "price_scale",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaCom · wrapped token",
        &[fixture!("kcom_wrapped_14273_a"), fixture!("kcom_wrapped_14273_b")],
        &[
            "canonical_token_id",
            "owner_identifier",
            "amount",
            "mode",
            "unit_price_sompi",
            "fee_ticket_id",
            "price_scale",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaCom · wrapped token",
        &[fixture!("kcom_wrapped_14258_a"), fixture!("kcom_wrapped_14258_b")],
        &[
            "canonical_token_id",
            "owner_identifier",
            "amount",
            "mode",
            "unit_price_sompi",
            "fee_ticket_id",
            "price_scale",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaCom · wrapped token",
        &[fixture!("kcom_wrapped_10094_a"), fixture!("kcom_wrapped_10094_b")],
        &["canonical_token_id", "amount", "mode", "unit_price_sompi"],
    ));
    add(Skeleton::derive_observed(
        "KaspaCom · wrapped token",
        &[fixture!("kcom_wrapped_9889_a"), fixture!("kcom_wrapped_9889_b")],
        &[
            "canonical_token_id",
            "owner_identifier",
            "amount",
            "mode",
            "unit_price_sompi",
            "fee_ticket_id",
            "price_scale",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaCom · wrapped token",
        &[fixture!("kcom_wrapped_7273_a"), fixture!("kcom_wrapped_7273_b")],
        &[
            "canonical_token_id",
            "owner_identifier",
            "amount",
            "mode",
            "unit_price_sompi",
            "price_scale",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaCom · wrapper",
        &[fixture!("kcom_wrapper_3895_a"), fixture!("kcom_wrapper_3895_b")],
        &[
            "canonical_token_id",
            "price_scale",
            "wrap_selector",
            "holder_template_digest",
            "unwrap_selector",
            "holder_template_len",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaCom · wrapper",
        &[fixture!("kcom_wrapper_3796_a"), fixture!("kcom_wrapper_3796_b")],
        &[
            "canonical_token_id",
            "price_scale",
            "template_len",
            "template_digest",
            "self_len",
            "sig_offset",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaCom · wrapper",
        &[fixture!("kcom_wrapper_3807_a"), fixture!("kcom_wrapper_3807_b")],
        &[
            "canonical_token_id",
            "price_scale",
            "v2_template_len",
            "v2_template_digest",
            "holder_template_digest",
            "self_len",
            "holder_template_len",
        ],
    ));
    add(Skeleton::derive_observed(
        "KaspaCom · wrapper",
        &[fixture!("kcom_wrapper_3776_a"), fixture!("kcom_wrapper_3776_b")],
        &["canonical_token_id", "holder_template_digest", "holder_template_len"],
    ));

    // genesis0 · mint: the same 206-instruction stream as the 565 B slot-mint
    // build with a different tag constant ({"t":"genesis0-mint","v":3,) and
    // substr length, which is exactly why the slot-mint skeleton refuses it
    // (constants are enforced) and why it needs its own entry. Registered
    // AFTER slot-mint so the richer, earlier family keeps first claim; the
    // two skeletons refuse each other's fixtures. payout_amount is 1 KAS in
    // every sampled collection and folds into a constant.
    add(Skeleton::derive_observed(
        "genesis0 · mint",
        &[fixture!("g0_mint_560_a"), fixture!("g0_mint_560_b")],
        &[
            "self_covenant_id",
            "payload_hash_a",
            "payload_hash_b",
            "item_code_hash",
            "payout_spk_hash",
            "instance_salt",
        ],
    ));


    // A KRON launch carries NO identity on chain. Proven Sep 2026 against
    // KRON's own shipped sources (their WASM compiler and covenant source,
    // which kascov has matched to chain bytes byte-for-byte): the kcc20
    // constructor takes six arguments and not one is a ticker, name or
    // decimals; the curve and pool constructors carry only keys, fees,
    // reserves and template splices; and the only transaction payload their
    // builder ever writes is a hex referral tag. That holds on both networks
    // (91/91 mainnet, 24/24 testnet-10 launches unnamed). So there is nothing
    // here for program_token_meta() to read: do NOT add a KRON entry to
    // template_carries_identity or a KRON parse arm to the reader it guards —
    // both would decode a program per directory row to learn nothing. The only
    // name these launches have is the one on KRON's published list, which
    // kascov cross-checks against chain and renders in the weaker listed tier.
    // KRON on testnet-10 (kron.technology): the curve and pool generations
    // their testnet ran, named at the covenant level so the pages read
    // "KRON · curve" / "KRON · pool". The market layer keeps its own tagged
    // skeletons for pricing; these names never touch that allowlist. The
    // 185,912 B curve is mainnet curve v4 and the 59,183 B pool mainnet
    // pool v3 outside constructor arguments and the TN10 platform key; the
    // others are the earlier testnet generations. Labels follow KRON's own
    // source (CurveCp / AmmPoolCpV3: tokenReserve, kasReserve, totalShares,
    // the embedded pool and LP templates with their hashes, vKas,
    // graduationKas). The leading state pushes are slots because the
    // fixture sets mix genesis and later reveals; self_len is the program's
    // own length and lp_template_len the embedded LP program's. Each set
    // was checked against every cached reveal of its shape (all match).
    add(Skeleton::derive_observed(
        "KRON · curve",
        &[
            fixture!("kron_curve_185912_a"),
            fixture!("kron_curve_185912_b"),
            fixture!("kron_curve_185912_c"),
        ],
        &[
            "token_covenant",
            "token_reserve",
            "creator_fee_owner",
            "self_len",
            "v_kas_units",
            "graduation_kas_sompi",
            "pool_template",
            "pool_template_hash",
        ],
    ));
    add(Skeleton::derive_observed(
        "KRON · curve",
        &[
            fixture!("kron_curve_171749_a"),
            fixture!("kron_curve_171749_b"),
            fixture!("kron_curve_171749_c"),
            fixture!("kron_curve_171749_d"),
        ],
        &[
            "token_covenant",
            "token_reserve",
            "creator_fee_owner",
            "self_len",
            "v_kas_units",
            "graduation_kas_sompi",
            "pool_template",
            "pool_template_hash",
        ],
    ));
    add(Skeleton::derive_observed(
        "KRON · curve",
        &[
            fixture!("kron_curve_153859_a"),
            fixture!("kron_curve_153859_b"),
            fixture!("kron_curve_153859_c"),
        ],
        &[
            "token_covenant",
            "token_reserve",
            "creator_fee_owner",
            "self_len",
            "v_kas_units",
            "graduation_kas_sompi",
            "pool_template",
            "pool_template_hash",
        ],
    ));
    add(Skeleton::derive_observed(
        "KRON · curve",
        &[
            fixture!("kron_curve_144959_a"),
            fixture!("kron_curve_144959_b"),
            fixture!("kron_curve_144959_c"),
            fixture!("kron_curve_144959_d"),
        ],
        &[
            "token_covenant",
            "token_reserve",
            "creator_fee_owner",
            "lp_template",
            "lp_template_hash",
            "self_len",
            "v_kas_units",
            "lp_template_len",
            "graduation_kas_sompi",
            "pool_template",
            "pool_template_hash",
        ],
    ));
    add(Skeleton::derive_observed(
        "KRON · pool",
        &[
            fixture!("kron_pool_59183_a"),
            fixture!("kron_pool_59183_b"),
            fixture!("kron_pool_59183_c"),
            fixture!("kron_pool_59183_d"),
        ],
        &[
            "kas_reserve_units",
            "token_reserve",
            "token_covenant",
            "shares",
            "lp_token_covenant",
            "creator_fee_owner",
        ],
    ));
    add(Skeleton::derive_observed(
        "KRON · pool",
        &[
            fixture!("kron_pool_52677_a"),
            fixture!("kron_pool_52677_b"),
            fixture!("kron_pool_52677_c"),
        ],
        &[
            "kas_reserve_units",
            "token_reserve",
            "token_covenant",
            "shares",
            "lp_token_covenant",
        ],
    ));
    add(Skeleton::derive_observed(
        "KRON · pool",
        &[
            fixture!("kron_pool_50880a_a"),
            fixture!("kron_pool_50880a_b"),
            fixture!("kron_pool_50880a_c"),
            fixture!("kron_pool_50880a_d"),
        ],
        &[
            "kas_reserve_units",
            "token_reserve",
            "token_covenant",
            "shares",
            "lp_token_covenant",
            "self_len",
            "lp_template",
            "lp_template_hash",
        ],
    ));
    // The three known deployments share one token, so token_covenant folded
    // into a constant: a fourth launch on this build would stay unnamed until
    // a differing fixture lands.
    add(Skeleton::derive_observed(
        "KRON · pool",
        &[
            fixture!("kron_pool_50880b_a"),
            fixture!("kron_pool_50880b_b"),
            fixture!("kron_pool_50880b_c"),
        ],
        &["kas_reserve_units", "token_reserve", "shares", "lp_token_covenant"],
    ));

    // One operator's soak harness, 2026-07-12 to 2026-09-03: 34,497
    // covenants between two tiny builds, four fifths of everything
    // testnet-10 had left unnamed. Builder unknown and left unnamed on
    // purpose: the only public traces of these keys lead to a person's
    // handles, not a product. The two share a tag because every allowance
    // withdrawal in the record is a log deposit and the same two keys sign
    // both. Proven on 26 and 7 distinct hash-verified reveals: every one
    // matches the skeleton its two fixtures derive, and neither skeleton
    // matches a program of the other.
    //
    // The allowance is the Silverscript "Mecenas" shape with a witness-
    // supplied payee: three arms — pay up to the cap to a P2PK the witness
    // names with the continuation at output 0, the same with the
    // continuation at output 1 so a peer covenant can claim output 0, and
    // a sweep by one fixed key present in every deployment. The cap is a
    // per-deployment argument (1 or 2 KAS); the 0.05 KAS is the tx fee the
    // covenant funds itself, the 0.2 KAS is the floor a swept cell freezes
    // at. `owner_pubkey` and `max_withdrawal` are each inlined twice; the
    // derivation folds each pair into one slot.
    add(Skeleton::derive_observed(
        "allowance tn-f",
        &[fixture!("allowance_tn_f_a"), fixture!("allowance_tn_f_b")],
        &["balance", "owner_pubkey", "max_withdrawal"],
    ));
    // The log is a forward blake2b hash chain, head' = blake2b256(head ||
    // note), with a settle arm whose key is the trivially derivable test key
    // (0x53 repeated), so anyone could spend it: a soak deployment, not a
    // product. No per-deployment argument exists; both slots are state. The
    // settled flag is 0x00 in every hash-verified reveal seen and so folds
    // into a constant today; a reveal of a settled cell would make it a
    // third slot and needs a differing fixture then.
    add(Skeleton::derive_observed(
        "log tn-f",
        &[fixture!("log_tn_f_a"), fixture!("log_tn_f_b")],
        &["log_head", "accrued"],
    ));

    // LAST on purpose. The activated KRC gate state is only nine
    // instructions with four non-push opcodes (drop, drop, covenant-id,
    // equal-verify), so an unrelated "pin a covenant id, carry two dropped
    // commitments" leaf would match it too. Registered after every richer
    // family so a more specific skeleton always wins the name.
    add(Skeleton::derive_observed(
        "genesis0 · krc-gate-active",
        &[fixture!("g0_krc_gate_active_a"), fixture!("g0_krc_gate_active_b")],
        &["used_root", "gate_commit", "controller_covenant"],
    ));

    // KForge (kforge.xyz, API api-test.kforge.xyz): a fair-launch launchpad
    // whose whole product is three programs, each compiled once. A launch is
    // a CURVE ("rung") covenant, a token cell, and — once the curve sells
    // out — a POOL the curve creates in its own final spend. Read from chain
    // bytes on 2026-09-08 against all 11 launches the venue lists (4
    // graduated), every reveal of every launch matching the shape below.
    //
    // The curve sells 800 rungs of 1,000,000 token units each at
    // `base + slope x rungs_sold` sompi a rung; it keeps 99% of what a buy
    // pays and 99% of a sell's refund leaves it, the other 1% being the
    // venue's fee output. It carries the pool program it will create as a
    // state push, so the launch names its own future pool before the pool
    // exists. The fixture pair is deliberate: one launch takes the venue's
    // default terms and the other prices its rungs its own way, so the two
    // terms stay slots instead of folding into constants that would refuse
    // every launch priced differently.
    //
    // The token cell is the base four-field layout, so it also gets a
    // `kcc20::CellFamily` entry; the pool is a two-reserve constant product
    // whose KAS reserve equals the cell's own value.
    add(Skeleton::derive_observed(
        "KForge · curve",
        &[fixture!("kforge_curve_5900_a"), fixture!("kforge_curve_5900_b")],
        &["rungs_sold", "curve_base_sompi", "curve_slope_sompi", "pool_program", "launch_key"],
    ));
    add(Skeleton::derive_observed(
        "KForge · token cell",
        &[fixture!("kforge_token_1785_a"), fixture!("kforge_token_1785_b")],
        &["owner_identifier", "identifier_type", "amount", "is_minter"],
    ));
    add(Skeleton::derive_observed(
        "KForge · pool",
        &[fixture!("kforge_pool_1370_a"), fixture!("kforge_pool_1370_b")],
        &["kas_reserve", "token_reserve", "creator_fee_spk"],
    ));

    out
}

/// Whether a named family's build carries the token's own identity as
/// constructor arguments — a `ticker` beside a `token_name` — so a reader
/// knows to look there instead of at a payload. Derived from the registered
/// skeletons themselves, so a new build that carries them is included the
/// day it is registered and no venue is named here.
pub fn template_carries_identity(template: &str) -> bool {
    // Derived ONCE: `observed_skeletons` rebuilds every skeleton from its
    // fixtures on each call, which is far too expensive to run per token
    // row (it made the venue pages hang when this was first shipped).
    static CARRIERS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    CARRIERS
        .get_or_init(|| {
            observed_skeletons()
                .iter()
                .filter(|s| s.params().contains(&"ticker") && s.params().contains(&"token_name"))
                .map(|s| s.name)
                .collect()
        })
        .contains(&template)
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
        &[
            "output_index",
            "output_amount",
            "output_index",
            "output_spk_hash",
        ],
    ) {
        out.push(s);
    }

    out
}

#[cfg(test)]
mod kaspacom_tn10_tests {
    use super::*;
    use crate::Registry;

    /// Every family added from the 2026-08-20 TN10 decode must actually
    /// derive. `derive_observed` returns None when the label count does not
    /// match the slots the fixture pair reveals, and the registration site
    /// swallows that None — so without this test a mislabeled pin fails
    /// silently and the family just stays unnamed on the site.
    #[test]
    fn every_kaspacom_and_kasware_family_derives() {
        let names: Vec<&str> = observed_skeletons().iter().map(|s| s.name).collect();
        for expected in [
            "genesis0 · root",
            "genesis0 · slot-mint",
            "genesis0 · buy",
            "genesis0 · list",
            "genesis0 · style-list",
            "genesis0 · dutch-list",
            "genesis0 · auction",
            "genesis0 · krc-gate-boot",
            "genesis0 · krc-gate-active",
            "KaspaCom · token",
            "Zealous · token cell",
            "Zealous · curve",
            "Zealous · pool",
            "Zealous · position cell",
            "Zealous · liquidity position",
            "Zealous · factory",
            "KaspaRocket · lp-amm",
            "KaspaRocket · order-escrow",
            "KaspaKaha · pool",
            "KaspaCom · wrapped token",
            "KaspaCom · wrapper",
            "genesis0 · mint",
            "KRON · curve",
            "KRON · pool",
            "KasWare · vault-schnorr",
            "KasWare · vault-pqcold",
            "KasWare · vault-htlc",
            "KasWare · vault-dms",
        ] {
            assert!(
                names.contains(&expected),
                "{expected} did not derive — its label count no longer matches its fixtures"
            );
        }
        // All four newly decoded slot-mint builds register under the ONE
        // family name, per this file's convention. If a future build is
        // added under an invented name instead, this count catches it.
        assert_eq!(
            names.iter().filter(|n| **n == "genesis0 · slot-mint").count(),
            4,
            "every decoded slot-mint build must register under the family name"
        );
    }

    /// The point of a pin: real chain bytes get the right name back. Each
    /// fixture is a verbatim revealed program, so decoding it exercises the
    /// same path the indexer runs on a live spend.
    #[test]
    fn each_fixture_decodes_to_its_own_family() {
        let reg = Registry::default();
        for (name, program) in [
            ("genesis0 · root", fixture!("g0_root_a")),
            ("genesis0 · slot-mint", fixture!("g0_slot_mint_565_b")),
            ("genesis0 · slot-mint", fixture!("g0_slot_mint_520_a")),
            ("genesis0 · slot-mint", fixture!("g0_slot_mint_413_b")),
            ("genesis0 · slot-mint", fixture!("g0_slot_mint_421_b")),
            ("genesis0 · buy", fixture!("g0_buy_v3_a")),
            ("genesis0 · list", fixture!("g0_list_v3_b")),
            ("genesis0 · style-list", fixture!("g0_style_list_a")),
            ("genesis0 · dutch-list", fixture!("g0_dutch_list_b")),
            ("genesis0 · auction", fixture!("g0_auction_a")),
            ("genesis0 · krc-gate-boot", fixture!("g0_krc_gate_boot_b")),
            ("genesis0 · krc-gate-active", fixture!("g0_krc_gate_active_a")),
            ("KaspaCom · token", fixture!("kcom_token_b")),
            ("Zealous · token cell", fixture!("zealous_launch_a_a")),
            ("Zealous · token cell", fixture!("zealous_launch_b_b")),
            ("Zealous · curve", fixture!("zealous_curve_10663_a")),
            ("Zealous · pool", fixture!("zealous_pool_18346_a")),
            ("Zealous · position cell", fixture!("zealous_position_1019_a")),
            ("Zealous · liquidity position", fixture!("zealous_lp_906_a")),
            ("Zealous · curve", fixture!("zealous_curve_8970_a")),
            ("Zealous · curve", fixture!("zealous_curve_8802_a")),
            ("Zealous · curve", fixture!("zealous_curve_8959_a")),
            ("Zealous · curve", fixture!("zealous_curve_8643_a")),
            ("Zealous · pool", fixture!("zealous_pool_15022_a")),
            ("Zealous · pool", fixture!("zealous_pool_13478_a")),
            ("Zealous · pool", fixture!("zealous_pool_12568_a")),
            ("Zealous · pool", fixture!("zealous_pool_12152_a")),
            ("Zealous · pool", fixture!("zealous_pool_12046_a")),
            ("Zealous · pool", fixture!("zealous_pool_8622_a")),
            ("Zealous · pool", fixture!("zealous_pool_8451_a")),
            ("Zealous · pool", fixture!("zealous_pool_8403_a")),
            ("Zealous · pool", fixture!("zealous_pool_8205_a")),
            ("Zealous · position cell", fixture!("zealous_position_1040_a")),
            ("Zealous · position cell", fixture!("zealous_position_980_a")),
            ("Zealous · liquidity position", fixture!("zealous_lp_764_a")),
            ("Zealous · liquidity position", fixture!("zealous_lp_687_a")),
            ("Zealous · factory", fixture!("zealous_factory_620_a")),
            ("Zealous · factory", fixture!("zealous_factory_573_a")),
            ("Zealous · factory", fixture!("zealous_factory_561_a")),
            ("Zealous · token cell", fixture!("zealous_token_1811_a")),
            ("Zealous · token cell", fixture!("zealous_token_1324_a")),
            ("Zealous · token cell", fixture!("zealous_token_1198_a")),
            ("KaspaRocket · lp-amm", fixture!("kr_lpamm_22861_a")),
            ("KaspaRocket · lp-amm", fixture!("kr_lpamm_18062_a")),
            ("KaspaRocket · lp-amm", fixture!("kr_lpamm_18060_a")),
            ("KaspaRocket · order-escrow", fixture!("kr_order_escrow_65541_a")),
            ("KaspaRocket · order-escrow", fixture!("kr_order_escrow_43641_a")),
            ("KaspaKaha · pool", fixture!("kaha_pool_102564_a")),
            ("KaspaKaha · pool", fixture!("kaha_pool_102467_a")),
            ("KaspaKaha · pool", fixture!("kaha_pool_108722_a")),
            ("KaspaKaha · pool", fixture!("kaha_pool_108623_a")),
            ("KaspaKaha · pool", fixture!("kaha_pool_75692_a")),
            ("KaspaKaha · pool", fixture!("kaha_pool_92140_a")),
            ("KaspaKaha · pool", fixture!("kaha_pool_108714_a")),
            ("KaspaCom · token", fixture!("kcom_token_2671_a")),
            ("KaspaCom · wrapped token", fixture!("kcom_wrapped_25496_a")),
            ("KaspaCom · wrapped token", fixture!("kcom_wrapped_22775_a")),
            ("KaspaCom · wrapped token", fixture!("kcom_wrapped_16984_a")),
            ("KaspaCom · wrapped token", fixture!("kcom_wrapped_14273_a")),
            ("KaspaCom · wrapped token", fixture!("kcom_wrapped_14258_a")),
            ("KaspaCom · wrapped token", fixture!("kcom_wrapped_10094_a")),
            ("KaspaCom · wrapped token", fixture!("kcom_wrapped_9889_a")),
            ("KaspaCom · wrapped token", fixture!("kcom_wrapped_7273_a")),
            ("KaspaCom · wrapper", fixture!("kcom_wrapper_3895_a")),
            ("KaspaCom · wrapper", fixture!("kcom_wrapper_3796_a")),
            ("KaspaCom · wrapper", fixture!("kcom_wrapper_3807_a")),
            ("KaspaCom · wrapper", fixture!("kcom_wrapper_3776_a")),
            ("genesis0 · mint", fixture!("g0_mint_560_a")),
            ("KRON · curve", fixture!("kron_curve_185912_a")),
            ("KRON · curve", fixture!("kron_curve_171749_a")),
            ("KRON · curve", fixture!("kron_curve_153859_a")),
            ("KRON · curve", fixture!("kron_curve_144959_a")),
            ("KRON · pool", fixture!("kron_pool_59183_a")),
            ("KRON · pool", fixture!("kron_pool_52677_a")),
            ("KRON · pool", fixture!("kron_pool_50880a_a")),
            ("KRON · pool", fixture!("kron_pool_50880b_a")),
            ("KasWare · vault-schnorr", fixture!("kw_vault_schnorr_a")),
            ("KasWare · vault-pqcold", fixture!("kw_vault_pqcold_b")),
            ("KasWare · vault-htlc", fixture!("kw_vault_htlc_a")),
            ("KasWare · vault-dms", fixture!("kw_vault_dms_b")),
        ] {
            let d = reg.decode(0, program);
            assert_eq!(d.template, Some(name), "{name} fixture decoded as {:?}", d.template);
            assert!(!d.fields.is_empty(), "{name} named no fields");
        }
    }

    /// The 421-byte slot-mint pair is the one that needed a fixture from
    /// outside the sampled set: all eight sampled gates paid the same 2 KAS,
    /// which would have frozen a real per-deployment argument into a
    /// constant. This pins the reason — if payout_amount ever stops being a
    /// slot, the skeleton silently narrows and gates priced differently go
    /// unnamed again. (The label is output_amount here, matching what the
    /// build actually constrains: output 3's amount.)
    #[test]
    fn slot_mint_421_keeps_payout_amount_variable() {
        let a = fixture!("g0_slot_mint_421_a");
        let b = fixture!("g0_slot_mint_421_b");
        assert_ne!(a, b, "the pair must be two distinct deployments");
        // 0x0bebc200 = 200_000_000 sompi (2 KAS), present in the sampled
        // build and absent from the differing one — the proof the amount is
        // per-deployment and not a build constant.
        let two_kas = 200_000_000u32.to_le_bytes();
        assert!(a.windows(4).any(|w| w == two_kas), "fixture a should carry the 2 KAS payout");
        assert!(!b.windows(4).any(|w| w == two_kas), "fixture b must differ on the payout");

        let reg = Registry::default();
        let d = reg.decode(0, b);
        assert_eq!(d.template, Some("genesis0 · slot-mint"));
        let amount = d.fields.iter().find(|f| f.name == "output_amount");
        assert!(amount.is_some(), "output_amount must be a labeled slot, not a baked constant");
    }
}

#[cfg(test)]
mod kaspacom_token_tests {
    use crate::Registry;

    /// The whole point: a real KaspaCom token's revealed program must come
    /// back named, with its ticker and display name read straight out of the
    /// bytes. These are verbatim reveals from live testnet-10 covenants.
    #[test]
    fn a_live_token_yields_its_ticker_and_name_from_chain() {
        let reg = Registry::default();
        let d = reg.decode(0, fixture!("kcom_token_b"));
        assert_eq!(d.template, Some("KaspaCom · token"));

        let text = |name: &str| -> String {
            let f = d
                .fields
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{name} missing"));
            String::from_utf8_lossy(&f.value)
                .trim_end_matches('\0')
                .to_string()
        };
        let num = |name: &str| -> u64 {
            let f = d.fields.iter().find(|f| f.name == name).unwrap();
            let mut b = [0u8; 8];
            b.copy_from_slice(&f.value[..8]);
            u64::from_le_bytes(b)
        };

        // KPOP / "Kaspa Pop": ticker and name are padded ASCII in the program
        assert_eq!(text("ticker"), "KPOP");
        assert_eq!(text("token_name"), "Kaspa Pop");
        // 0.001 KAS, exactly what KaspaCom's own mint page shows for it
        assert_eq!(num("mint_price_sompi"), 100_000);
        assert_eq!(num("decimals_scale"), 100_000_000);
        // this particular cell is the treasury holding nearly the whole
        // supply, which is why it reads close to the figure the UI lists
        assert_eq!(num("amount") / num("decimals_scale"), 900_000_000);
        // their UI lists KPOP as a public mint
        assert_eq!(num("mint_mode"), 2);
        // the four fields a token index needs are all present and labelled
        for f in ["owner_identifier", "identifier_type", "amount", "token_id"] {
            assert!(d.fields.iter().any(|x| x.name == f), "{f} missing");
        }
    }

    /// The other fixture is a different token entirely, which is what makes
    /// the pair legal: it differs at every one of the nine arguments, so none
    /// of them can fold into a constant.
    #[test]
    fn the_fixture_pair_disagrees_on_every_argument() {
        let reg = Registry::default();
        let a = reg.decode(0, fixture!("kcom_token_a"));
        let b = reg.decode(0, fixture!("kcom_token_b"));
        assert_eq!(a.template, Some("KaspaCom · token"));
        assert_eq!(a.fields.len(), 9);
        assert_eq!(b.fields.len(), 9);
        for (x, y) in a.fields.iter().zip(&b.fields) {
            assert_eq!(x.name, y.name);
            assert_ne!(
                x.value, y.value,
                "{} agrees across the pair and would derive as a constant",
                x.name
            );
        }
        assert_eq!(
            String::from_utf8_lossy(&a.fields[6].value).trim_end_matches('\0'),
            "BATCH29"
        );
    }
}

#[cfg(test)]
mod venue_tn10_tests {
    use super::*;
    use crate::Registry;

    /// Every build registers under its family name: the count per name is
    /// the number of compiles decoded on 2026-09-04/05, so an invented
    /// version suffix or a silently dropped pin both fail here.
    #[test]
    fn every_venue_build_registers_under_its_family_name() {
        let names: Vec<&str> = observed_skeletons().iter().map(|s| s.name).collect();
        for (name, builds) in [
            ("Zealous · curve", 5),
            ("Zealous · pool", 10),
            ("Zealous · position cell", 3),
            ("Zealous · liquidity position", 3),
            ("Zealous · factory", 3),
            ("Zealous · token cell", 5),
            ("KaspaRocket · lp-amm", 3),
            ("KaspaRocket · order-escrow", 2),
            ("KaspaKaha · pool", 10),
            ("KaspaCom · token", 2),
            ("KaspaCom · wrapped token", 8),
            ("KaspaCom · wrapper", 4),
            ("genesis0 · mint", 1),
            ("KRON · curve", 4),
            ("KRON · pool", 4),
            ("KForge · curve", 1),
            ("KForge · token cell", 1),
            ("KForge · pool", 1),
        ] {
            assert_eq!(
                names.iter().filter(|n| **n == name).count(),
                builds,
                "{name}: every decoded build must register under the family name"
            );
        }
    }

    /// The two-launch pairs disagree on every labelled field, so no real
    /// argument folded into a constant (the 421-byte slot-mint lesson).
    #[test]
    fn two_launch_pairs_disagree_on_every_argument() {
        let reg = Registry::default();
        for (name, a, b) in [
            ("Zealous · curve", fixture!("zealous_curve_10663_a"), fixture!("zealous_curve_10663_b")),
            ("Zealous · pool", fixture!("zealous_pool_18346_a"), fixture!("zealous_pool_18346_b")),
            ("Zealous · position cell", fixture!("zealous_position_1019_a"), fixture!("zealous_position_1019_b")),
            ("Zealous · token cell", fixture!("zealous_token_1811_a"), fixture!("zealous_token_1811_b")),
            ("KaspaRocket · lp-amm", fixture!("kr_lpamm_22861_a"), fixture!("kr_lpamm_22861_b")),
            ("KaspaRocket · lp-amm", fixture!("kr_lpamm_18062_a"), fixture!("kr_lpamm_18062_b")),
            ("KaspaRocket · order-escrow", fixture!("kr_order_escrow_65541_a"), fixture!("kr_order_escrow_65541_b")),
            ("KaspaKaha · pool", fixture!("kaha_pool_102564_a"), fixture!("kaha_pool_102564_b")),
            ("KaspaCom · token", fixture!("kcom_token_2671_a"), fixture!("kcom_token_2671_b")),
            ("KaspaCom · wrapped token", fixture!("kcom_wrapped_16984_a"), fixture!("kcom_wrapped_16984_b")),
        ] {
            let x = reg.decode(0, a);
            let y = reg.decode(0, b);
            assert_eq!(x.template, Some(name));
            assert_eq!(y.template, Some(name));
            assert_eq!(x.fields.len(), y.fields.len());
            for (f, g) in x.fields.iter().zip(&y.fields) {
                assert_eq!(f.name, g.name);
                assert_ne!(f.value, g.value, "{name}: {} agrees across the pair", f.name);
            }
        }
    }

    /// The soak pair: two fixtures disagree on every labelled field, a
    /// third real reveal of each shape takes the same name, and neither
    /// shape ever takes the other's name (589 B against 576 B, one opcode
    /// stream each, proven on 26 and 7 wide reveals before the pin).
    #[test]
    fn the_soak_pair_generalizes_and_never_cross_matches() {
        let reg = Registry::default();
        for (name, a, b, c, other) in [
            (
                "allowance tn-f",
                fixture!("allowance_tn_f_a"),
                fixture!("allowance_tn_f_b"),
                fixture!("allowance_tn_f_c"),
                "log tn-f",
            ),
            (
                "log tn-f",
                fixture!("log_tn_f_a"),
                fixture!("log_tn_f_b"),
                fixture!("log_tn_f_c"),
                "allowance tn-f",
            ),
        ] {
            let x = reg.decode(0, a);
            let y = reg.decode(0, b);
            let z = reg.decode(0, c);
            assert_eq!(x.template, Some(name));
            assert_eq!(y.template, Some(name));
            assert_eq!(z.template, Some(name), "a third real reveal takes the name");
            assert_ne!(z.template, Some(other));
            assert_eq!(x.fields.len(), y.fields.len());
            for (f, g) in x.fields.iter().zip(&y.fields) {
                assert_eq!(f.name, g.name);
                assert_ne!(f.value, g.value, "{name}: {} agrees across the pair", f.name);
            }
        }
        let a = reg.decode(0, fixture!("allowance_tn_f_a"));
        assert_eq!(
            a.fields.iter().map(|f| f.name).collect::<Vec<_>>(),
            ["balance", "owner_pubkey", "max_withdrawal"]
        );
        let l = reg.decode(0, fixture!("log_tn_f_a"));
        assert_eq!(l.fields.iter().map(|f| f.name).collect::<Vec<_>>(), ["log_head", "accrued"]);
    }

    /// The 15,022 B retired pool is the one that needs three instances: any
    /// two of its fixtures agree on two real arguments, which would have
    /// frozen them. Pin the reason so the third fixture is never "cleaned up".
    #[test]
    fn retired_pool_15022_needs_all_three_fixtures() {
        let a = fixture!("zealous_pool_15022_a");
        let b = fixture!("zealous_pool_15022_b");
        let c = fixture!("zealous_pool_15022_c");
        let labels = ["1", "2", "3", "4", "5", "6"];
        let two = Skeleton::derive_observed("x", &[a, b], &labels);
        assert!(two.is_none(), "a two-fixture pair folds two arguments and must not derive six slots");
        let three = Skeleton::derive_observed("x", &[a, b, c], &labels);
        assert!(three.is_some());
    }
}

#[cfg(test)]
mod genesis0_mint_tests {
    use crate::Registry;

    /// Same instruction stream as the 565 B slot-mint build, different tag
    /// constant: each skeleton must refuse the other's fixtures, and the
    /// registry must hand each program its own name.
    #[test]
    fn mint_and_slot_mint_refuse_each_other() {
        let reg = Registry::default();
        assert_eq!(reg.decode(0, fixture!("g0_mint_560_a")).template, Some("genesis0 · mint"));
        assert_eq!(reg.decode(0, fixture!("g0_mint_560_b")).template, Some("genesis0 · mint"));
        assert_eq!(reg.decode(0, fixture!("g0_slot_mint_565_b")).template, Some("genesis0 · slot-mint"));
    }
}
