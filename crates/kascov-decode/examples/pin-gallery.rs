//! Dump what the decoder makes of every pinned fixture, as JSON.
//!
//! The fixtures are verbatim revealed programs from real covenants, so this
//! is the same path the indexer runs on a live spend — not a mock. Used to
//! preview the families on localhost before a worker deploy:
//!
//!   cargo run -p kascov-decode --example pin-gallery > scripts/pins-dev-fixture.json

use kascov_decode::Registry;

macro_rules! fixture {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!("../fixtures/", $name, ".bin")).as_slice(),
        )
    };
}

fn main() {
    let reg = Registry::default();
    let fixtures = [
        fixture!("g0_root_a"),
        fixture!("g0_slot_mint_413_a"),
        fixture!("g0_slot_mint_421_b"),
        fixture!("g0_slot_mint_520_a"),
        fixture!("g0_slot_mint_565_b"),
        fixture!("g0_buy_v3_a"),
        fixture!("g0_list_v3_b"),
        fixture!("g0_style_list_a"),
        fixture!("g0_dutch_list_b"),
        fixture!("g0_auction_a"),
        fixture!("g0_krc_gate_boot_b"),
        fixture!("g0_krc_gate_active_a"),
        fixture!("kw_vault_schnorr_a"),
        fixture!("kw_vault_pqcold_b"),
        fixture!("kw_vault_htlc_a"),
        fixture!("kw_vault_dms_b"),
        fixture!("kcom_token_b"),
        fixture!("zealous_launch_a_a"),
        fixture!("zealous_launch_b_b"),
        // already-pinned families, for context
        fixture!("g0_list_v1_a"),
        fixture!("g0_buy_v1_a"),
        fixture!("g0_col_a"),
        fixture!("pure_a"),
    ];

    let mut out = Vec::new();
    for (name, program) in fixtures {
        let d = reg.decode(0, program);
        let fields: Vec<_> = d
            .fields
            .iter()
            .map(|f| {
                serde_json::json!({ "name": f.name, "value": hex::encode(&f.value) })
            })
            .collect();
        out.push(serde_json::json!({
            "fixture": name,
            "family": d.template,
            "bytes": program.len(),
            "instructions": d.instructions.len(),
            "uses_covenant_ops": d.uses_covenant_ops,
            "fields": fields,
        }));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "network": "testnet-10",
            "generated_by": "cargo run -p kascov-decode --example pin-gallery",
            "note": "Every program here is a verbatim spend-time reveal whose blake2b \
                     matched its committed P2SH hash before capture; the fields are what \
                     kascov's own decoder labels them.",
            "pins": out,
        }))
        .unwrap()
    );
}
