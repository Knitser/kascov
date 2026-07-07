//! kascov-sim — run a *hypothetical* covenant spend through Kaspa's real
//! `TxScriptEngine`, off-chain, with no node and no private keys.
//!
//! A live coin commits to someone else's key, so a browser can't produce that
//! signature. Instead we re-instantiate the same contract with a fresh
//! STAND-IN signer (swap the authorized-party hash for `blake2b(stand-in pk)`),
//! fabricate the state UTXO, build the spend the caller describes, sign it with
//! the stand-in key, and execute the exact node-side script validation. So the
//! signature rule always resolves, and the *other* rules — amount, destination,
//! timelock, introspection — are tested for real against the scenario.

use blake2b_simd::Params as Blake2bParams;
use kaspa_addresses::{Address, Prefix, Version as AddrVersion};
use kaspa_consensus_core::{
    constants::TX_VERSION_TOCCATA,
    hashing::sighash::{calc_schnorr_signature_hash, SigHashReusedValuesUnsync},
    hashing::sighash_type::SIG_HASH_ALL,
    mass::units::ComputeBudget,
    subnets::SUBNETWORK_ID_NATIVE,
    tx::{
        ComputeCommit, MutableTransaction, ScriptPublicKey, Transaction, TransactionInput,
        TransactionOutpoint, TransactionOutput, UtxoEntry,
    },
};
use kaspa_txscript::{
    caches::Cache, pay_to_address_script, pay_to_script_hash_script, EngineCtx, EngineFlags,
    TxScriptEngine,
};
use secp256k1::{Keypair, SECP256K1};
use serde::{Deserialize, Serialize};

/// What the caller wants to try.
#[derive(Debug, Clone, Deserialize)]
pub struct SimRequest {
    /// The coin's compiled program (hex).
    pub program_hex: String,
    /// Which entrypoint to satisfy: `spend` (Escrow) | `reclaim` | `cold` | `inherit`.
    pub entrypoint: String,
    /// Where the funds go: `buyer` | `seller` | `other` | `self`.
    #[serde(default = "default_recipient")]
    pub recipient: String,
    /// The state coin's value, in sompi.
    #[serde(default = "default_value")]
    pub value: u64,
    /// Output-0 value in sompi; default = `value − 1000` (the contract's fee).
    #[serde(default)]
    pub amount: Option<u64>,
}

fn default_recipient() -> String {
    "self".into()
}
fn default_value() -> u64 {
    100_000_000
}

/// The verdict.
#[derive(Debug, Clone, Serialize)]
pub struct SimResult {
    /// Was the request runnable (recognized template + known entrypoint)?
    pub ok: bool,
    /// Did the spend satisfy the contract — what a node would decide.
    pub pass: bool,
    /// Human-readable verdict (the engine's reason on failure).
    pub verdict: String,
    pub template: String,
    pub entrypoint: String,
    pub recipient: String,
    /// The output value used (sompi).
    pub output_value: u64,
    /// On failure: the specific contract rule the spend violates (plain English).
    #[serde(default)]
    pub rule: String,
    /// Honest framing shown in the UI.
    pub note: String,
}

impl SimResult {
    fn err(entrypoint: &str, msg: impl Into<String>) -> Self {
        SimResult {
            ok: false,
            pass: false,
            verdict: msg.into(),
            template: String::new(),
            entrypoint: entrypoint.to_string(),
            recipient: String::new(),
            output_value: 0,
            rule: String::new(),
            note: String::new(),
        }
    }
}

/// On a failed spend, name the specific rule the scenario violates. The engine
/// only reports "verification failed" at the OpVerify, but for a known template
/// + scenario the offending require is deterministic (and matches the source
/// order — Escrow checks the amount before the destination).
fn failing_rule(template: &str, recipient: &str, value: u64, output_value: u64) -> String {
    match template {
        "SilverScript · Escrow" => {
            if output_value != value.saturating_sub(1000) {
                "the output must equal the escrowed value minus the contract's 1000-sompi fee".into()
            } else if !matches!(recipient, "buyer" | "seller") {
                "the escrow can only pay the committed buyer or seller — no third address".into()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

fn blake2b32(bytes: &[u8]) -> [u8; 32] {
    *Blake2bParams::new().hash_length(32).hash(bytes).as_bytes().first_chunk::<32>().unwrap()
}

fn xonly(kp: &Keypair) -> [u8; 32] {
    kp.public_key().x_only_public_key().0.serialize()
}

fn p2pk_spk(xonly_pk: &[u8]) -> Option<ScriptPublicKey> {
    let addr = Address::new(Prefix::Testnet, AddrVersion::PubKey, xonly_pk);
    Some(pay_to_address_script(&addr))
}

/// (selector to push after pk+sig, the committed hash field the signer must
/// match). Mirrors kascov-lab's `entrypoint_spec`, plus Escrow.
fn spec(template: &str, entrypoint: &str) -> Option<(Option<i64>, &'static str)> {
    match (template, entrypoint) {
        ("SilverScript · Escrow", "spend") => Some((None, "arbiter_hash")),
        ("SilverScript · Mecenas", "reclaim") => Some((Some(1), "funder_hash")),
        ("SilverScript · LastWill", "cold") => Some((Some(1), "cold_hash")),
        ("SilverScript · LastWill", "inherit") => Some((Some(0), "inheritor_hash")),
        _ => None,
    }
}

/// Replace the 32-byte `old` subsequence in `program` with `new` (both 32B).
/// The authorized-party field is a blake2b hash — unique in the script.
fn splice_field(program: &[u8], old: &[u8], new: &[u8; 32]) -> Option<Vec<u8>> {
    if old.len() != 32 {
        return None;
    }
    let pos = program.windows(32).position(|w| w == old)?;
    let mut out = program.to_vec();
    out[pos..pos + 32].copy_from_slice(new);
    Some(out)
}

pub fn simulate(req: &SimRequest) -> SimResult {
    let program = match hex::decode(req.program_hex.trim().trim_start_matches("0x")) {
        Ok(p) if !p.is_empty() => p,
        _ => return SimResult::err(&req.entrypoint, "program isn't valid hex"),
    };
    let decoded = kascov_decode::Registry::default().decode(0, &program);
    let Some(template) = decoded.template else {
        return SimResult::err(&req.entrypoint, "not a recognized SilverScript contract");
    };
    let Some((selector, signer_field)) = spec(template, &req.entrypoint) else {
        return SimResult::err(
            &req.entrypoint,
            format!("simulation doesn't support {template} · {}", req.entrypoint),
        );
    };
    let field = |name: &str| decoded.fields.iter().find(|f| f.name == name).map(|f| f.value.clone());
    let Some(committed) = field(signer_field) else {
        return SimResult::err(&req.entrypoint, format!("{template} has no {signer_field}"));
    };

    // A stand-in signer, and the contract re-instantiated to trust it.
    let stand_in = Keypair::new(SECP256K1, &mut secp256k1::rand::thread_rng());
    let pk = xonly(&stand_in);
    let Some(program2) = splice_field(&program, &committed, &blake2b32(&pk)) else {
        return SimResult::err(&req.entrypoint, "couldn't re-instantiate the contract");
    };

    // Where the money goes.
    let other = Keypair::new(SECP256K1, &mut secp256k1::rand::thread_rng());
    let recipient_pk: Vec<u8> = match req.recipient.as_str() {
        "buyer" => match field("buyer") {
            Some(v) => v,
            None => return SimResult::err(&req.entrypoint, "this contract has no buyer"),
        },
        "seller" => match field("seller") {
            Some(v) => v,
            None => return SimResult::err(&req.entrypoint, "this contract has no seller"),
        },
        "other" => xonly(&other).to_vec(),
        _ => pk.to_vec(), // "self"
    };
    let Some(dest_spk) = p2pk_spk(&recipient_pk) else {
        return SimResult::err(&req.entrypoint, "bad recipient key");
    };

    let value = req.value.max(1_000_000);
    let output_value = req.amount.unwrap_or(value.saturating_sub(1000));

    // One input (the fabricated covenant state), one output.
    let state_spk = pay_to_script_hash_script(&program2);
    let outpoint = TransactionOutpoint::new(kaspa_consensus_core::Hash::from_bytes([0x11; 32]), 0);
    let input = TransactionInput::new_with_mass(
        outpoint,
        vec![],
        0,
        ComputeCommit::ComputeBudget(ComputeBudget(60)),
    );
    let output = TransactionOutput::new(output_value, dest_spk);
    let tx = Transaction::new(TX_VERSION_TOCCATA, vec![input], vec![output], 0, SUBNETWORK_ID_NATIVE, 0, vec![]);
    // block_daa_score = 0 → the coin reads as maximally old, so relative
    // timelocks (reclaim) are satisfied and don't mask the rule under test.
    let entry = UtxoEntry::new(value, state_spk, 0, false, None);
    let mut mtx = MutableTransaction::with_entries(tx, vec![entry]);

    // Sign the schnorr sighash over the P2SH UTXO with the stand-in key.
    let reused = SigHashReusedValuesUnsync::new();
    let sig_hash = calc_schnorr_signature_hash(&mtx.as_verifiable(), 0, SIG_HASH_ALL, &reused);
    let msg = match secp256k1::Message::from_digest_slice(sig_hash.as_bytes().as_slice()) {
        Ok(m) => m,
        Err(_) => return SimResult::err(&req.entrypoint, "sighash error"),
    };
    let sig = stand_in.sign_schnorr(msg);
    let mut sig_arg = sig.as_ref().to_vec();
    sig_arg.push(SIG_HASH_ALL.to_u8());

    let mut witness = Vec::new();
    witness.extend_from_slice(&kascov_decode::encode_push(&pk));
    witness.extend_from_slice(&kascov_decode::encode_push(&sig_arg));
    if let Some(sel) = selector {
        witness.extend_from_slice(&kascov_decode::encode_push(&kascov_decode::snum(sel)));
    }
    witness.extend_from_slice(&kascov_decode::encode_push(&program2));
    mtx.tx.inputs[0].signature_script = witness;

    let (pass, verdict) = run_engine(&mtx);
    let rule = if pass { String::new() } else { failing_rule(template, &req.recipient, value, output_value) };
    SimResult {
        ok: true,
        pass,
        verdict,
        template: template.to_string(),
        entrypoint: req.entrypoint.clone(),
        recipient: req.recipient.clone(),
        output_value,
        rule,
        note: "simulated with a stand-in signer — the signature rule always resolves so the amount, destination & timelock rules are what's tested".into(),
    }
}

fn run_engine(mtx: &MutableTransaction<Transaction>) -> (bool, String) {
    let reused = SigHashReusedValuesUnsync::new();
    let vtx = mtx.as_verifiable();
    let sig_cache = Cache::new(10_000);
    let entry = mtx.entries[0].clone().expect("entry present");
    let mut vm = TxScriptEngine::from_transaction_input(
        &vtx,
        &mtx.tx.inputs[0],
        0,
        &entry,
        EngineCtx::new(&sig_cache).with_reused(&reused),
        EngineFlags { covenants_enabled: true, ..Default::default() },
    );
    match vm.execute() {
        Ok(()) => (true, "the spend satisfies the contract — a node would accept it".to_string()),
        Err(e) => (false, format!("{e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ESCROW: &str = "78aa2033333333333333333333333333333333333333333333333333333333333333338769765279ac6900c2b9be02e803949c6900c3201111111111111111111111111111111111111111111111111111111111111111030000207c7e01ac7e8700c3202222222222222222222222222222222222222222222222222222222222222222030000207c7e01ac7e879b69757551";

    fn sim(recipient: &str, amount: Option<u64>) -> SimResult {
        simulate(&SimRequest {
            program_hex: ESCROW.into(),
            entrypoint: "spend".into(),
            recipient: recipient.into(),
            value: 100_000_000,
            amount,
        })
    }

    #[test]
    fn arbiter_releases_to_buyer_passes() {
        let r = sim("buyer", None);
        assert!(r.ok, "runnable: {}", r.verdict);
        assert!(r.pass, "buyer release should pass, got: {}", r.verdict);
    }

    #[test]
    fn arbiter_releases_to_seller_passes() {
        let r = sim("seller", None);
        assert!(r.pass, "seller release should pass, got: {}", r.verdict);
    }

    #[test]
    fn release_to_third_party_fails() {
        let r = sim("other", None);
        assert!(r.ok);
        assert!(!r.pass, "releasing to a third address must fail");
    }

    #[test]
    fn skimming_the_amount_fails() {
        // send 2000 less than value-1000 → outputs[0].value != value-1000
        let r = sim("buyer", Some(100_000_000 - 3000));
        assert!(!r.pass, "skimming must fail the amount rule");
    }

    #[test]
    fn unknown_template_is_not_runnable() {
        let r = simulate(&SimRequest {
            program_hex: "76a914deadbeef88ac".into(),
            entrypoint: "spend".into(),
            recipient: "self".into(),
            value: 1_000_000,
            amount: None,
        });
        assert!(!r.ok);
    }
}
