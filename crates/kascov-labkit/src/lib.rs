//! The reusable half of the covenant lab: everything that builds, signs, and
//! broadcasts real covenant transactions on testnet-10. `kascov-lab` is a thin
//! CLI over these functions; the worker's custodial `/deploy` endpoint links
//! this crate directly so the browser builder can deploy without a local
//! toolchain.
//!
//! Unlike kascov-core, this crate uses the kaspa crates directly — it exists to
//! create real covenant transactions that the explorer can then index and trace.

use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use kaspa_addresses::{Address, Prefix, Version as AddrVersion};
use kaspa_consensus_core::{
    constants::TX_VERSION_TOCCATA,
    hashing::covenant_id::covenant_id,
    hashing::sighash::{calc_schnorr_signature_hash, SigHashReusedValuesUnsync},
    hashing::sighash_type::SIG_HASH_ALL,
    mass::units::ComputeBudget,
    sign::sign,
    subnets::SUBNETWORK_ID_NATIVE,
    tx::{
        ComputeCommit, CovenantBinding, MutableTransaction, ScriptPublicKey,
        Transaction, TransactionInput, TransactionOutpoint, TransactionOutput, UtxoEntry,
    },
    Hash,
};
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_txscript::{
    caches::Cache, extract_script_pub_key_address, pay_to_address_script, pay_to_script_hash_script,
    EngineCtx, EngineFlags, TxScriptEngine,
};
use kaspa_wrpc_client::{
    client::{ConnectOptions, ConnectStrategy},
    prelude::{NetworkId, NetworkType},
    KaspaRpcClient, Resolver, WrpcEncoding,
};
use secp256k1::{Keypair, SECP256K1};

pub const FEE: u64 = 500_000; // 0.005 KAS per tx — TN10's post-Toccata minimum relay fee is ~0.00166 for 1-in-1-out

/// Dry-run one input of a built transaction through the real Kaspa script
/// engine — the exact validation a node performs — WITHOUT broadcasting.
/// Returns (passed, human verdict). This is "what-if spend": test a covenant
/// spend before you send it, or simulate a spend you can't even sign.
pub fn simulate_input(mtx: &MutableTransaction<Transaction>, idx: usize) -> (bool, String) {
    let reused = SigHashReusedValuesUnsync::new();
    let vtx = mtx.as_verifiable();
    let sig_cache = Cache::new(10_000);
    let entry = mtx.entries[idx].clone().expect("entry present");
    let mut vm = TxScriptEngine::from_transaction_input(
        &vtx,
        &mtx.tx.inputs[idx],
        idx,
        &entry,
        EngineCtx::new(&sig_cache).with_reused(&reused),
        EngineFlags { covenants_enabled: true, ..Default::default() },
    );
    match vm.execute() {
        Ok(()) => (true, "the spend SATISFIES the contract — a node would accept it".to_string()),
        Err(e) => (false, format!("the contract REJECTS this spend: {e}")),
    }
}

/// Blake2b-256, the covenant P2SH commitment hash.
pub fn blake2b32(bytes: &[u8]) -> [u8; 32] {
    *blake2b_simd::Params::new().hash_length(32).hash(bytes).as_bytes().first_chunk::<32>().unwrap()
}

/// The x-only public key of a keypair, 32 bytes.
pub fn xonly(keypair: &Keypair) -> [u8; 32] {
    keypair.public_key().x_only_public_key().0.serialize()
}

/// The P2SH commitment scriptPubKey for a redeem program (OpBlake2b <h> OpEqual).
pub fn p2sh_spk(program: &[u8]) -> ScriptPublicKey {
    pay_to_script_hash_script(program)
}

/// How a recognized contract entrypoint is satisfied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntrypointPlan {
    /// Pure-signature: witness = push(pk) ++ push(sig) ++ [push(selector)] ++
    /// push(program); the contract doesn't constrain the outputs, so the
    /// spender takes the funds wherever they like. `sequence` is the value
    /// input 0 must carry when the entrypoint has an age gate
    /// (`OpCheckSequenceVerify` compares the contract's period against the
    /// input's sequence field), 0 otherwise.
    PureSig { selector: Option<i64>, signer_field: &'static str, sequence: u64 },
    /// Output-constrained: the contract introspects the outputs, so the tx is
    /// built to the contract's own math (see `build_constrained_spend`).
    /// `signer_field` is None when no signature is required at all (anyone
    /// may trigger the entrypoint).
    Constrained { kind: ConstrainedKind, selector: i64, signer_field: Option<&'static str> },
}

/// The output-constrained entrypoints the lab knows how to build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstrainedKind {
    /// Mecenas.receive (selector 0, NO signature — anyone may trigger it once
    /// `period` has elapsed): outputs[0] pays the recipient exactly `pledge`
    /// and the remainder continues the covenant at outputs[1] (the same P2SH,
    /// re-bound) — unless what would remain is ≤ pledge + 1000, in which case
    /// outputs[0] takes everything − 1000 and the covenant ends.
    MecenasReceive,
    /// LastWill.refresh (selector 2, hot key): outputs[0] = the same P2SH
    /// re-bound with value − 1000 — the "I'm alive" timer reset.
    LastWillRefresh,
}

/// For a recognized contract + entrypoint: how to build a spend of it.
/// The selector values mirror kascov-sim's `spec` and the compiled branch
/// order in the skeleton dumps (LastWill: inherit=0, cold=1, refresh=2).
pub fn entrypoint_spec(template: &str, entrypoint: &str) -> Result<EntrypointPlan> {
    let spec = match (template, entrypoint) {
        ("SilverScript · Mecenas", "reclaim") => {
            EntrypointPlan::PureSig { selector: Some(1), signer_field: "funder_hash", sequence: 0 }
        }
        ("SilverScript · LastWill", "cold") => {
            EntrypointPlan::PureSig { selector: Some(1), signer_field: "cold_hash", sequence: 0 }
        }
        // inherit is age-gated: the compiled branch runs `<180> OpCheckSequenceVerify`
        // (the contract's fixed timer), so input 0 must state sequence ≥ 180.
        ("SilverScript · LastWill", "inherit") => {
            EntrypointPlan::PureSig { selector: Some(0), signer_field: "inheritor_hash", sequence: 180 }
        }
        ("SilverScript · Mecenas", "receive") => {
            EntrypointPlan::Constrained { kind: ConstrainedKind::MecenasReceive, selector: 0, signer_field: None }
        }
        ("SilverScript · LastWill", "refresh") => {
            EntrypointPlan::Constrained { kind: ConstrainedKind::LastWillRefresh, selector: 2, signer_field: Some("hot_hash") }
        }
        ("SilverScript · Escrow", "spend") => bail!(
            "Escrow's spend entrypoint needs a --release-to party — use `settle-escrow` instead of `spend`"
        ),
        _ => bail!("don't know how to satisfy {template} . {entrypoint}"),
    };
    Ok(spec)
}

/// Decode a minimally-encoded script number (little-endian, MSB sign bit) —
/// the encoding `kascov_decode::snum` and the SilverScript compiler emit for
/// int fields like pledge/period.
fn snum_to_i64(bytes: &[u8]) -> Result<i64> {
    if bytes.len() > 8 {
        bail!("script number wider than 8 bytes");
    }
    let mut v: u64 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        v |= (b as u64) << (8 * i);
    }
    if let Some(&msb) = bytes.last() {
        if msb & 0x80 != 0 {
            let sign_bit = 0x80u64 << (8 * (bytes.len() - 1));
            return Ok(-((v & !sign_bit) as i64));
        }
    }
    i64::try_from(v).context("script number out of i64 range")
}

/// Load a 32-byte hex secret key from disk (creating a fresh one if `create`).
pub fn load_or_create_key(path: &Path, create: bool) -> Result<Keypair> {
    if path.exists() {
        let hex_key = std::fs::read_to_string(path)?.trim().to_string();
        let bytes = hex::decode(&hex_key)?;
        Ok(Keypair::from_seckey_slice(SECP256K1, &bytes)?)
    } else if create {
        let keypair = Keypair::new(SECP256K1, &mut secp256k1::rand::thread_rng());
        std::fs::write(path, hex::encode(keypair.secret_bytes()))?;
        Ok(keypair)
    } else {
        bail!("no key at {} — run `kascov-lab keygen` first", path.display());
    }
}

/// Build a keypair from a bare 32-byte hex secret (no file). The worker's
/// custodial deploy key arrives this way, via the KASCOV_DEPLOY_KEY env var.
pub fn keypair_from_hex(hex_key: &str) -> Result<Keypair> {
    let bytes = hex::decode(hex_key.trim()).context("deploy key is not valid hex")?;
    Keypair::from_seckey_slice(SECP256K1, &bytes).context("deploy key is not a valid secret")
}

pub fn address_of(keypair: &Keypair) -> Address {
    let (xonly, _) = keypair.public_key().x_only_public_key();
    Address::new(Prefix::Testnet, AddrVersion::PubKey, &xonly.serialize())
}

/// Generate a keypair (if none exists) and print the TN10 address + key fields.
pub fn keygen(path: &Path) -> Result<()> {
    let keypair = load_or_create_key(path, true)?;
    let (xonly, _) = keypair.public_key().x_only_public_key();
    let pk = xonly.serialize();
    let pk_hash = blake2b_simd::Params::new().hash_length(32).hash(&pk);
    println!("key file:        {}", path.display());
    println!("address:         {}", address_of(&keypair));
    println!("pubkey (x-only): {}", hex::encode(pk));
    println!("blake2b(pubkey): {}", hex::encode(pk_hash.as_bytes()));
    println!();
    println!("the pubkey and its blake2b fill the generator's key fields on");
    println!("kascov.io/decode — fund the address at");
    println!("https://faucet-testnet.kaspanet.io before deploying.");
    Ok(())
}

/// Connect a borsh wRPC client to testnet-10. `rpc` is an explicit node url;
/// `None` uses the public resolver.
pub async fn connect(rpc: Option<&str>) -> Result<KaspaRpcClient> {
    let network_id = NetworkId::with_suffix(NetworkType::Testnet, 10);
    let resolver = rpc.is_none().then(Resolver::default);
    let client = KaspaRpcClient::new(WrpcEncoding::Borsh, rpc, resolver, Some(network_id), None)?;
    client
        .connect(Some(ConnectOptions {
            block_async_connect: true,
            connect_timeout: Some(Duration::from_millis(15_000)),
            strategy: ConnectStrategy::Fallback,
            ..Default::default()
        }))
        .await?;
    Ok(client)
}

/// Show the address and its current UTXO balance.
pub async fn balance(client: &KaspaRpcClient, keypair: &Keypair) -> Result<()> {
    let address = address_of(keypair);
    let utxos = client.get_utxos_by_addresses(vec![address.clone().into()]).await?;
    let total: u64 = utxos.iter().map(|u| u.utxo_entry.amount).sum();
    println!("address: {address}");
    println!("utxos:   {}", utxos.len());
    println!("balance: {:.8} TKAS", total as f64 / 100_000_000.0);
    Ok(())
}

/// A UTXO by outpoint + entry — enough to build and sign a spend offline.
pub struct SpendableUtxo {
    pub outpoint: TransactionOutpoint,
    pub entry: UtxoEntry,
}

/// The largest plain (non-covenant) UTXO on the key's address, in sompi —
/// the same funding selection `deploy` makes. Lets a server pre-flight
/// affordability (value + FEE) without attempting a build, so a drained
/// faucet can be answered cheaply and without leaking wallet details.
pub async fn spendable_balance(client: &KaspaRpcClient, keypair: &Keypair) -> Result<u64> {
    let address = address_of(keypair);
    let utxos = client.get_utxos_by_addresses(vec![address.into()]).await?;
    Ok(utxos
        .iter()
        .filter(|u| u.utxo_entry.covenant_id.is_none())
        .map(|u| u.utxo_entry.amount)
        .max()
        .unwrap_or(0))
}

pub async fn submit(client: &KaspaRpcClient, tx: &Transaction) -> Result<String> {
    let rpc_tx: kaspa_rpc_core::RpcTransaction = tx.into();
    let id = client.submit_transaction(rpc_tx, false).await.context("submit failed")?;
    Ok(id.to_string())
}

/// Run the full covenant lifecycle: genesis → N transitions → burn.
pub async fn demo(client: &KaspaRpcClient, keypair: &Keypair, transitions: u32) -> Result<()> {
    let address = address_of(keypair);
    let spk = pay_to_address_script(&address);

    // Funding UTXO: largest non-covenant UTXO we own.
    let utxos = client.get_utxos_by_addresses(vec![address.clone().into()]).await?;
    let funding = utxos
        .iter()
        .filter(|u| u.utxo_entry.covenant_id.is_none())
        .max_by_key(|u| u.utxo_entry.amount)
        .with_context(|| format!("no spendable UTXOs on {address} — fund it via the faucet first"))?;
    let needed = FEE * (transitions as u64 + 2) + 100_000;
    if funding.utxo_entry.amount < needed {
        bail!(
            "largest UTXO holds {:.8} TKAS, need at least {:.8}",
            funding.utxo_entry.amount as f64 / 1e8,
            needed as f64 / 1e8
        );
    }
    let mut current = SpendableUtxo {
        outpoint: TransactionOutpoint::new(funding.outpoint.transaction_id, funding.outpoint.index),
        entry: UtxoEntry::new(
            funding.utxo_entry.amount,
            funding.utxo_entry.script_public_key.clone(),
            funding.utxo_entry.block_daa_score,
            funding.utxo_entry.is_coinbase,
            None,
        ),
    };
    println!("funding UTXO {}:{} ({:.8} TKAS)", current.outpoint.transaction_id, current.outpoint.index, current.entry.amount as f64 / 1e8);

    // ── Genesis ──────────────────────────────────────────────────────────
    let value = current.entry.amount - FEE;
    let plain_output = TransactionOutput::new(value, spk.clone());
    let id = covenant_id(current.outpoint, std::iter::once((0u32, &plain_output)));
    let genesis_output = TransactionOutput::with_covenant(value, spk.clone(), Some(CovenantBinding::new(0, id)));
    let tx = build_signed(keypair, &current, vec![genesis_output])?;
    let txid = submit(client, &tx).await?;
    println!("GENESIS    covenant {id}");
    println!("           tx {txid}");
    current = SpendableUtxo {
        outpoint: TransactionOutpoint::new(tx.id(), 0),
        entry: UtxoEntry::new(value, spk.clone(), 0, false, Some(id)),
    };

    // ── Transitions ──────────────────────────────────────────────────────
    for n in 1..=transitions {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let value = current.entry.amount - FEE;
        let output = TransactionOutput::with_covenant(value, spk.clone(), Some(CovenantBinding::new(0, id)));
        let tx = build_signed(keypair, &current, vec![output])?;
        let txid = submit(client, &tx).await?;
        println!("TRANSITION #{n} tx {txid}");
        current = SpendableUtxo {
            outpoint: TransactionOutpoint::new(tx.id(), 0),
            entry: UtxoEntry::new(value, spk.clone(), 0, false, Some(id)),
        };
    }

    // ── Burn ─────────────────────────────────────────────────────────────
    tokio::time::sleep(Duration::from_secs(3)).await;
    let value = current.entry.amount - FEE;
    let output = TransactionOutput::new(value, spk.clone());
    let tx = build_signed(keypair, &current, vec![output])?;
    let txid = submit(client, &tx).await?;
    println!("BURN       tx {txid}");
    println!();
    println!("covenant lifecycle complete — trace it with:");
    println!("  kascov --network testnet-10 trace {id}");
    Ok(())
}

/// Birth a compiled contract: the coin's state is the P2SH commitment of the
/// program (OpBlake2b <blake2b-256> OpEqual — the exact shape the explorer
/// recognizes and, at spend time, verifies against the revealed program).
/// Returns the newborn coin's covenant id.
pub async fn deploy(client: &KaspaRpcClient, keypair: &Keypair, program: &[u8], value: u64) -> Result<Hash> {
    if program.is_empty() {
        bail!("empty program");
    }

    // Name what we're deploying (warn-and-proceed on unknown shapes: the
    // chain doesn't care, but the user should know kascov won't label it).
    let decoded = kascov_decode::Registry::default().decode(0, program);
    match decoded.template {
        Some(t) => println!("program:   {t} ({} bytes)", program.len()),
        None => println!("program:   unrecognized shape ({} bytes) — deploying anyway; kascov will show it as a plain p2sh commitment", program.len()),
    }
    for f in &decoded.fields {
        println!("           {} = {}", f.name, hex::encode(&f.value));
    }

    let commitment = blake2b32(program);
    let p2sh_spk = p2sh_spk(program);

    let address = address_of(keypair);
    let plain_spk = pay_to_address_script(&address);

    let utxos = client.get_utxos_by_addresses(vec![address.clone().into()]).await?;
    let funding = utxos
        .iter()
        .filter(|u| u.utxo_entry.covenant_id.is_none())
        .max_by_key(|u| u.utxo_entry.amount)
        .with_context(|| format!("no spendable UTXOs on {address} — fund it via the faucet first"))?;
    let needed = value + FEE;
    if funding.utxo_entry.amount < needed {
        bail!(
            "largest UTXO holds {:.8} TKAS, need at least {:.8} (value + fee)",
            funding.utxo_entry.amount as f64 / 1e8,
            needed as f64 / 1e8
        );
    }
    let funding_utxo = SpendableUtxo {
        outpoint: TransactionOutpoint::new(funding.outpoint.transaction_id, funding.outpoint.index),
        entry: UtxoEntry::new(
            funding.utxo_entry.amount,
            funding.utxo_entry.script_public_key.clone(),
            funding.utxo_entry.block_daa_score,
            funding.utxo_entry.is_coinbase,
            None,
        ),
    };

    // The covenant id commits to the funding outpoint + the authorized
    // output (index 0 only — change at index 1 stays unbound).
    let bound_plain = TransactionOutput::new(value, p2sh_spk.clone());
    let id = covenant_id(funding_utxo.outpoint, std::iter::once((0u32, &bound_plain)));
    let mut outputs = vec![TransactionOutput::with_covenant(
        value,
        p2sh_spk,
        Some(CovenantBinding::new(0, id)),
    )];
    let change = funding_utxo.entry.amount - value - FEE;
    if change >= 100_000 {
        outputs.push(TransactionOutput::new(change, plain_spk));
    } else if change > 0 {
        bail!("change of {change} sompi is dust — pick a coin value that leaves ≥ 0.001 TKAS or exactly 0");
    }

    let tx = build_signed(keypair, &funding_utxo, outputs)?;
    let txid = submit(client, &tx).await?;
    println!();
    println!("BIRTH      covenant {id}");
    println!("           tx {txid}");
    println!("           program blake2b {}", hex::encode(commitment));
    println!();
    println!("watch it live (give the indexer ~a minute) — this link proves the");
    println!("commitment in the browser, no spend needed:");
    println!("  https://kascov.io/testnet-10/c/{id}?program={}", hex::encode(program));
    println!();
    println!("the coin shows as a 'p2sh commitment' (the program is hidden) until");
    println!("you SPEND it — that reveals the program on-chain and kascov names it:");
    println!("  kascov-lab spend --program-hex <the same hex> --entrypoint reclaim");
    println!("(reclaim needs the coin's funder_hash to be your key's blake2b —");
    println!(" `kascov-lab keygen` prints it. Or just run `kascov-lab contract-demo`.)");
    Ok(id)
}

/// Settle an Escrow: input 0 = the covenant state (witness satisfies
/// `spend(pk, s)` with the ARBITER key), input 1 = a plain UTXO paying the
/// network fee. The contract forces outputs[0] = P2PK(buyer|seller) with
/// exactly state.value − 1000; change from the fee input rides at index 1.
pub async fn settle_escrow(
    client: &KaspaRpcClient,
    keypair: &Keypair,
    program: &[u8],
    release_to: &str,
    target_covenant: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let target_covenant = parse_covenant(target_covenant)?;
    let decoded = kascov_decode::Registry::default().decode(0, program);
    let template = decoded.template.context("not a recognized contract")?;
    if template != "SilverScript · Escrow" {
        bail!("settle-escrow works on Escrow programs; this is {template}");
    }
    let field = |n: &str| {
        decoded.fields.iter().find(|f| f.name == n).map(|f| f.value.clone())
            .with_context(|| format!("missing {n}"))
    };
    let arbiter_hash = field("arbiter_hash")?;
    let buyer_pk = field("buyer")?;
    let seller_pk = field("seller")?;

    let pk = xonly(keypair);
    if blake2b32(&pk).to_vec() != arbiter_hash {
        bail!(
            "the arbiter of this escrow is {}, not your key ({}) — only the arbiter can settle it",
            hex::encode(&arbiter_hash),
            hex::encode(blake2b32(&pk))
        );
    }
    let recipient_pk = match release_to {
        "buyer" => buyer_pk,
        "seller" => seller_pk,
        other => bail!("--release-to must be buyer or seller, not {other}"),
    };
    let recipient_addr = Address::new(Prefix::Testnet, AddrVersion::PubKey, &recipient_pk);
    let recipient_spk = pay_to_address_script(&recipient_addr);

    // The escrow state UTXO…
    let spk = p2sh_spk(program);
    let p2sh_addr = extract_script_pub_key_address(&spk, Prefix::Testnet)
        .map_err(|e| anyhow::anyhow!("cannot derive P2SH address: {e:?}"))?;
    let states = client.get_utxos_by_addresses(vec![p2sh_addr.clone().into()]).await?;
    let state = match target_covenant {
        Some(t) => states.iter().find(|u| u.utxo_entry.covenant_id == Some(t)),
        None => states.iter().find(|u| u.utxo_entry.covenant_id.is_some()).or_else(|| states.first()),
    }
    .with_context(|| format!("no live escrow state at {p2sh_addr}"))?;

    // …and a plain UTXO of ours to pay the real network fee.
    let my_addr = address_of(keypair);
    let mine = client.get_utxos_by_addresses(vec![my_addr.clone().into()]).await?;
    // escrow input ≈ 100k script units (2 P2PK rebuilds + introspection);
    // the fee input runs its own p2pk checksig (~100k) — both need real budget.
    let budget: u16 = 40;
    let fee = 100 * (2 * budget as u64 * 100 + 5000) + 200_000;
    let funding = mine
        .iter()
        .filter(|u| u.utxo_entry.covenant_id.is_none() && u.utxo_entry.amount > fee + 100_000)
        .max_by_key(|u| u.utxo_entry.amount)
        .with_context(|| format!("no fee-funding UTXO on {my_addr} — faucet it first"))?;

    // Contract math: outputs[0].value == state.value − 1000 (its hardcoded fee).
    let state_value = state.utxo_entry.amount;
    let out0 = TransactionOutput::new(state_value - 1000, recipient_spk);
    // change: everything from the fee input minus the real network fee − the 1000
    // the contract already "spent" (the tx must balance: in − out == network fee).
    let change_value = funding.utxo_entry.amount + 1000 - fee;
    let out1 = TransactionOutput::new(change_value, pay_to_address_script(&my_addr));

    let covenant_input = TransactionInput::new_with_mass(
        TransactionOutpoint::new(state.outpoint.transaction_id, state.outpoint.index),
        vec![],
        0,
        ComputeCommit::ComputeBudget(ComputeBudget(budget)),
    );
    let fee_input = TransactionInput::new_with_mass(
        TransactionOutpoint::new(funding.outpoint.transaction_id, funding.outpoint.index),
        vec![],
        0,
        ComputeCommit::ComputeBudget(ComputeBudget(budget)),
    );
    let tx = Transaction::new(
        TX_VERSION_TOCCATA,
        vec![covenant_input, fee_input],
        vec![out0, out1],
        0,
        SUBNETWORK_ID_NATIVE,
        0,
        vec![],
    );
    let entries = vec![
        UtxoEntry::new(
            state_value,
            state.utxo_entry.script_public_key.clone(),
            state.utxo_entry.block_daa_score,
            state.utxo_entry.is_coinbase,
            state.utxo_entry.covenant_id,
        ),
        UtxoEntry::new(
            funding.utxo_entry.amount,
            funding.utxo_entry.script_public_key.clone(),
            funding.utxo_entry.block_daa_score,
            funding.utxo_entry.is_coinbase,
            None,
        ),
    ];
    let mut mtx = MutableTransaction::with_entries(tx, entries);

    let reused = SigHashReusedValuesUnsync::new();
    // input 0: the arbiter satisfies Escrow.spend(pk, s) + reveals the program
    let h0 = calc_schnorr_signature_hash(&mtx.as_verifiable(), 0, SIG_HASH_ALL, &reused);
    let sig0 = keypair.sign_schnorr(secp256k1::Message::from_digest_slice(h0.as_bytes().as_slice())?);
    let mut sig0_arg = sig0.as_ref().to_vec();
    sig0_arg.push(SIG_HASH_ALL.to_u8());
    let mut witness = Vec::new();
    witness.extend_from_slice(&kascov_decode::encode_push(&pk));
    witness.extend_from_slice(&kascov_decode::encode_push(&sig0_arg));
    // Escrow has a single entrypoint — no selector.
    witness.extend_from_slice(&kascov_decode::encode_push(program));
    mtx.tx.inputs[0].signature_script = witness;
    // input 1: plain p2pk spend of our fee UTXO
    let h1 = calc_schnorr_signature_hash(&mtx.as_verifiable(), 1, SIG_HASH_ALL, &reused);
    let sig1 = keypair.sign_schnorr(secp256k1::Message::from_digest_slice(h1.as_bytes().as_slice())?);
    let mut sig1_full = sig1.as_ref().to_vec();
    sig1_full.push(SIG_HASH_ALL.to_u8());
    mtx.tx.inputs[1].signature_script = kascov_decode::encode_push(&sig1_full);

    let covenant_id_opt = state.utxo_entry.covenant_id;
    if dry_run {
        let (pass, verdict) = simulate_input(&mtx, 0);
        println!("SIMULATE   Escrow → {release_to}  (not broadcast)");
        println!("           {}  {verdict}", if pass { "✓ PASS —" } else { "✗ FAIL —" });
        println!("           would release {:.8} TKAS to the {release_to}", (state_value - 1000) as f64 / 1e8);
        return Ok(());
    }
    let txid = submit(client, &mtx.tx).await?;
    println!("SETTLED    Escrow → {release_to} ({:.8} TKAS released)", (state_value - 1000) as f64 / 1e8);
    println!("           tx {txid}");
    if let Some(id) = covenant_id_opt {
        println!();
        println!("the escrow revealed itself on-chain. watch the story:");
        println!("  https://kascov.io/testnet-10/c/{id}");
    }
    Ok(())
}

/// Escrow end-to-end: emit (arbiter = you, buyer = you, seller = throwaway),
/// deploy, settle to the buyer.
pub async fn escrow_demo(client: &KaspaRpcClient, keypair: &Keypair, value: u64) -> Result<()> {
    let pk = xonly(keypair);
    let pk_hash = blake2b32(&pk);
    let seller = [0x5eu8; 32]; // a throwaway "seller" — the demo releases to the buyer (you)
    let skels = kascov_decode::silverscript_skeletons();
    let escrow = skels.iter().find(|s| s.name == "SilverScript · Escrow").context("no Escrow skeleton")?;
    let args: Vec<(&str, &[u8])> = vec![
        ("arbiter_hash", &pk_hash),
        ("buyer", &pk),
        ("seller", &seller),
    ];
    let program = escrow.emit(&args).context("emit failed")?;

    println!("=== escrow-demo: deploy → arbiter settles → buyer paid ===");
    println!("[1/2] deploying the escrow…");
    let id = deploy(client, keypair, &program, value).await?;
    println!();
    println!("[2/2] waiting ~15s, then settling to the buyer…");
    tokio::time::sleep(Duration::from_secs(15)).await;
    settle_escrow(client, keypair, &program, "buyer", Some(&id.to_string()), false).await?;
    println!();
    println!("done — a real escrow lived and settled by its own rules on testnet-10.");
    Ok(())
}

fn build_signed(keypair: &Keypair, from: &SpendableUtxo, outputs: Vec<TransactionOutput>) -> Result<Transaction> {
    let input = TransactionInput::new(from.outpoint, vec![], 0, 1);
    let tx = Transaction::new(TX_VERSION_TOCCATA, vec![input], outputs, 0, SUBNETWORK_ID_NATIVE, 0, vec![]);
    let signable = MutableTransaction::with_entries(tx, vec![from.entry.clone()]);
    let signed = sign(signable, *keypair);
    Ok(signed.tx)
}

/// Parse an optional covenant-id string into a Hash.
fn parse_covenant(s: Option<&str>) -> Result<Option<Hash>> {
    s.map(|c| c.parse::<Hash>().context("bad covenant id")).transpose()
}

/// Build (entirely offline) an output-constrained entrypoint spend, the
/// settle_escrow pattern generalized: input 0 = the covenant state (P2SH
/// reveal witness), input 1 = a plain fee UTXO owned by `keypair`, outputs
/// shaped to satisfy the contract's own introspection math, and the covenant
/// binding re-bound wherever the state continues. Pure function of its
/// inputs — no network; the result is ready for `simulate_input` or submit.
pub fn build_constrained_spend(
    keypair: &Keypair,
    program: &[u8],
    entrypoint: &str,
    state: &SpendableUtxo,
    funding: &SpendableUtxo,
    compute_budget: u16,
) -> Result<MutableTransaction<Transaction>> {
    let decoded = kascov_decode::Registry::default().decode(0, program);
    let template = decoded.template.context("not a recognized contract")?;
    let EntrypointPlan::Constrained { kind, selector, signer_field } = entrypoint_spec(template, entrypoint)? else {
        bail!("{template} . {entrypoint} is a pure-signature entrypoint — `spend` handles it directly");
    };
    let field = |n: &str| {
        decoded
            .fields
            .iter()
            .find(|f| f.name == n)
            .map(|f| f.value.clone())
            .with_context(|| format!("{template} has no {n} field"))
    };

    // The signature gate, when the entrypoint demands one.
    let pk = xonly(keypair);
    if let Some(sf) = signer_field {
        let committed = field(sf)?;
        if committed != blake2b32(&pk).to_vec() {
            bail!(
                "this coin's {sf} is {}, but your key's blake2b is {} — you can't {entrypoint} it",
                hex::encode(&committed),
                hex::encode(blake2b32(&pk))
            );
        }
    }

    let state_value = state.entry.amount;
    let state_spk = state.entry.script_public_key.clone();
    if state_spk != p2sh_spk(program) {
        bail!("the state UTXO's scriptPubKey isn't this program's P2SH commitment");
    }
    if state_value <= 1000 {
        bail!("coin holds {state_value} sompi — not even the contract's own 1000-sompi fee");
    }
    // Continuation binding: where the state lives on, the covenant id rides
    // along, authorized by input 0. (A stateless P2SH coin continues unbound.)
    let binding = state.entry.covenant_id.map(|id| CovenantBinding::new(0, id));

    // Fee-input math, same shape as settle_escrow: both inputs commit
    // `compute_budget`; the plain change absorbs the 1000 sompi the contract's
    // internal math already "spent", so the tx balances at exactly `fee`.
    let fee = 100 * (2 * compute_budget as u64 * 100 + 5000) + 200_000;
    if funding.entry.amount <= fee + 100_000 {
        bail!(
            "fee UTXO holds {} sompi — need more than {} (network fee + dust floor)",
            funding.entry.amount,
            fee + 100_000
        );
    }
    let my_spk = pay_to_address_script(&address_of(keypair));
    let fee_change = TransactionOutput::new(funding.entry.amount + 1000 - fee, my_spk);

    // Outputs + input-0 sequence, per the contract's own math.
    let mut sequence0 = 0u64;
    let mut outputs: Vec<TransactionOutput> = Vec::new();
    match kind {
        ConstrainedKind::MecenasReceive => {
            let recipient = field("recipient")?;
            let pledge = snum_to_i64(&field("pledge")?)?;
            let period = snum_to_i64(&field("period")?)?;
            if pledge <= 0 || period < 0 {
                bail!("nonsensical pledge ({pledge}) / period ({period})");
            }
            // `this.age >= period` compiled to OpCheckSequenceVerify: the tx
            // states the coin's age via input 0's sequence field, and
            // consensus holds the tx until the UTXO really is that old.
            sequence0 = period as u64;
            let recipient_addr = Address::new(Prefix::Testnet, AddrVersion::PubKey, &recipient);
            let recipient_spk = pay_to_address_script(&recipient_addr);
            // Contract math (all in its own units; the real network fee rides
            // on input 1): change = value − pledge − 1000.
            let change = state_value as i128 - pledge as i128 - 1000;
            if change <= pledge as i128 + 1000 {
                // Terminal payout: everything − 1000 to the recipient, the
                // covenant ends.
                outputs.push(TransactionOutput::new(state_value - 1000, recipient_spk));
            } else {
                // Pledge out, remainder continues the covenant at the same P2SH.
                outputs.push(TransactionOutput::new(pledge as u64, recipient_spk));
                outputs.push(TransactionOutput::with_covenant(change as u64, state_spk.clone(), binding));
            }
        }
        ConstrainedKind::LastWillRefresh => {
            // The whole coin (− the contract's 1000) stays at the same P2SH:
            // a keep-alive that resets the inheritance timer.
            outputs.push(TransactionOutput::with_covenant(state_value - 1000, state_spk.clone(), binding));
        }
    }
    outputs.push(fee_change);

    let covenant_input = TransactionInput::new_with_mass(
        state.outpoint,
        vec![],
        sequence0,
        ComputeCommit::ComputeBudget(ComputeBudget(compute_budget)),
    );
    let fee_input = TransactionInput::new_with_mass(
        funding.outpoint,
        vec![],
        0,
        ComputeCommit::ComputeBudget(ComputeBudget(compute_budget)),
    );
    let tx = Transaction::new(
        TX_VERSION_TOCCATA,
        vec![covenant_input, fee_input],
        outputs,
        0,
        SUBNETWORK_ID_NATIVE,
        0,
        vec![],
    );
    let entries = vec![state.entry.clone(), funding.entry.clone()];
    let mut mtx = MutableTransaction::with_entries(tx, entries);

    let reused = SigHashReusedValuesUnsync::new();
    // input 0: reveal witness — [push(pk) push(sig)] ++ push(selector) ++ push(program).
    // Mecenas.receive takes no arguments at all (its branch only consumes the
    // selector), so the witness is just selector + program.
    let mut witness = Vec::new();
    if signer_field.is_some() {
        let h0 = calc_schnorr_signature_hash(&mtx.as_verifiable(), 0, SIG_HASH_ALL, &reused);
        let sig0 = keypair.sign_schnorr(secp256k1::Message::from_digest_slice(h0.as_bytes().as_slice())?);
        let mut sig0_arg = sig0.as_ref().to_vec();
        sig0_arg.push(SIG_HASH_ALL.to_u8());
        witness.extend_from_slice(&kascov_decode::encode_push(&pk));
        witness.extend_from_slice(&kascov_decode::encode_push(&sig0_arg));
    }
    witness.extend_from_slice(&kascov_decode::encode_push(&kascov_decode::snum(selector)));
    witness.extend_from_slice(&kascov_decode::encode_push(program));
    mtx.tx.inputs[0].signature_script = witness;
    // input 1: plain p2pk spend of the fee UTXO.
    let h1 = calc_schnorr_signature_hash(&mtx.as_verifiable(), 1, SIG_HASH_ALL, &reused);
    let sig1 = keypair.sign_schnorr(secp256k1::Message::from_digest_slice(h1.as_bytes().as_slice())?);
    let mut sig1_full = sig1.as_ref().to_vec();
    sig1_full.push(SIG_HASH_ALL.to_u8());
    mtx.tx.inputs[1].signature_script = kascov_decode::encode_push(&sig1_full);

    Ok(mtx)
}

/// Network half of an output-constrained spend: locate the live state UTXO
/// and a plain fee UTXO, build via `build_constrained_spend`, then simulate
/// or broadcast — `spend` dispatches here for receive/refresh.
#[allow(clippy::too_many_arguments)]
async fn spend_constrained(
    client: &KaspaRpcClient,
    keypair: &Keypair,
    program: &[u8],
    entrypoint: &str,
    template: &str,
    target_covenant: Option<Hash>,
    to: Option<&str>,
    compute_budget: u16,
    dry_run: bool,
) -> Result<()> {
    if to.is_some() {
        bail!("--to doesn't apply to {entrypoint} — the contract itself dictates where the funds go");
    }

    // The coin's live state UTXO, via its P2SH address.
    let spk = p2sh_spk(program);
    let p2sh_addr = extract_script_pub_key_address(&spk, Prefix::Testnet)
        .map_err(|e| anyhow::anyhow!("cannot derive P2SH address: {e:?}"))?;
    let states = client.get_utxos_by_addresses(vec![p2sh_addr.clone().into()]).await?;
    let state = match target_covenant {
        Some(t) => states
            .iter()
            .find(|u| u.utxo_entry.covenant_id == Some(t))
            .with_context(|| format!("covenant {t} has no live UTXO at {p2sh_addr} (spent already?)"))?,
        None => states
            .iter()
            .find(|u| u.utxo_entry.covenant_id.is_some())
            .or_else(|| states.first())
            .with_context(|| format!("no live state UTXO at {p2sh_addr} — is the coin deployed and unspent?"))?,
    };
    let state_utxo = SpendableUtxo {
        outpoint: TransactionOutpoint::new(state.outpoint.transaction_id, state.outpoint.index),
        entry: UtxoEntry::new(
            state.utxo_entry.amount,
            state.utxo_entry.script_public_key.clone(),
            state.utxo_entry.block_daa_score,
            state.utxo_entry.is_coinbase,
            state.utxo_entry.covenant_id,
        ),
    };

    // …and a plain UTXO of ours to pay the real network fee.
    let my_addr = address_of(keypair);
    let mine = client.get_utxos_by_addresses(vec![my_addr.clone().into()]).await?;
    let fee = 100 * (2 * compute_budget as u64 * 100 + 5000) + 200_000;
    let funding = mine
        .iter()
        .filter(|u| u.utxo_entry.covenant_id.is_none() && u.utxo_entry.amount > fee + 100_000)
        .max_by_key(|u| u.utxo_entry.amount)
        .with_context(|| format!("no fee-funding UTXO on {my_addr} — faucet it first"))?;
    let funding_utxo = SpendableUtxo {
        outpoint: TransactionOutpoint::new(funding.outpoint.transaction_id, funding.outpoint.index),
        entry: UtxoEntry::new(
            funding.utxo_entry.amount,
            funding.utxo_entry.script_public_key.clone(),
            funding.utxo_entry.block_daa_score,
            funding.utxo_entry.is_coinbase,
            None,
        ),
    };

    let mtx = build_constrained_spend(keypair, program, entrypoint, &state_utxo, &funding_utxo, compute_budget)?;
    let covenant_id_opt = state_utxo.entry.covenant_id;

    if dry_run {
        let (pass, verdict) = simulate_input(&mtx, 0);
        println!("SIMULATE   {template} . {entrypoint}  (not broadcast)");
        println!("           {}  {verdict}", if pass { "✓ PASS —" } else { "✗ FAIL —" });
        for (i, out) in mtx.tx.outputs.iter().enumerate() {
            let tag = if out.covenant.is_some() { " [covenant continues]" } else { "" };
            println!("           output[{i}] {:.8} TKAS{tag}", out.value as f64 / 1e8);
        }
        return Ok(());
    }
    let txid = submit(client, &mtx.tx).await?;
    println!("SPEND      {template} . {entrypoint}");
    println!("           tx {txid}");
    if let Some(id) = covenant_id_opt {
        println!();
        println!("the program is now revealed on-chain. give the indexer ~a minute, then:");
        println!("  https://kascov.io/testnet-10/c/{id}");
    }
    Ok(())
}

/// Spend a deployed contract coin by satisfying one of its entrypoints.
/// Pure-signature entrypoints (reclaim/cold/inherit) are handled inline: the
/// unlocking script is the revealed contract program,
///   push(pubkey) ++ push(sig) ++ [push(selector)] ++ push(program)
/// and the funds go wherever you point --to. Output-constrained entrypoints
/// (Mecenas.receive, LastWill.refresh) dispatch to `spend_constrained`, which
/// builds the outputs the contract's introspection demands. Either way the
/// spend reveals the program on-chain; kascov's indexer then shows the coin
/// as its named contract for everyone, permanently.
pub async fn spend(
    client: &KaspaRpcClient,
    keypair: &Keypair,
    program: &[u8],
    entrypoint: &str,
    target_covenant: Option<&str>,
    to: Option<&str>,
    compute_budget: u16,
    dry_run: bool,
) -> Result<()> {
    let target_covenant = parse_covenant(target_covenant)?;
    if program.is_empty() {
        bail!("empty program");
    }
    let decoded = kascov_decode::Registry::default().decode(0, program);
    let template = decoded.template.context(
        "this program isn't a recognized SilverScript contract — the lab only knows how to spend Mecenas/Escrow/LastWill",
    )?;
    let (selector, signer_field, sequence) = match entrypoint_spec(template, entrypoint)? {
        EntrypointPlan::Constrained { .. } => {
            return spend_constrained(
                client, keypair, program, entrypoint, template, target_covenant, to, compute_budget, dry_run,
            )
            .await;
        }
        EntrypointPlan::PureSig { selector, signer_field, sequence } => (selector, signer_field, sequence),
    };

    // The key that signs must be the one the contract checks for this entrypoint.
    let pk = xonly(keypair);
    let pk_hash = blake2b32(&pk);
    let committed = decoded
        .fields
        .iter()
        .find(|f| f.name == signer_field)
        .map(|f| f.value.clone())
        .with_context(|| format!("{template} has no {signer_field} field"))?;
    if committed != pk_hash {
        if dry_run {
            // simulate anyway — the whole point is to see the rejection honestly
            println!(
                "note: your key's blake2b ({}) isn't this coin's {signer_field} ({}) —\n\
                 the checksig will fail; simulating so you can see exactly where.\n",
                hex::encode(pk_hash),
                hex::encode(&committed)
            );
        } else {
            bail!(
                "this coin's {signer_field} is {}, but your key's blake2b is {} — you can't {entrypoint} it.\n\
                 deploy a coin whose {signer_field} = your `kascov-lab keygen` blake2b, then spend that one.",
                hex::encode(&committed),
                hex::encode(pk_hash)
            );
        }
    }

    // Find the coin's live state UTXO from the node, via its P2SH address.
    let spk = p2sh_spk(program);
    let p2sh_addr = extract_script_pub_key_address(&spk, Prefix::Testnet)
        .map_err(|e| anyhow::anyhow!("cannot derive P2SH address: {e:?}"))?;
    let utxos = client.get_utxos_by_addresses(vec![p2sh_addr.clone().into()]).await?;
    let state = match target_covenant {
        Some(t) => utxos
            .iter()
            .find(|u| u.utxo_entry.covenant_id == Some(t))
            .with_context(|| format!("covenant {t} has no live UTXO at {p2sh_addr} (spent already?)"))?,
        None => utxos
            .iter()
            .find(|u| u.utxo_entry.covenant_id.is_some())
            .or_else(|| utxos.first())
            .with_context(|| format!("no live state UTXO at {p2sh_addr} — is the coin deployed and unspent?"))?,
    };
    let value = state.utxo_entry.amount;
    // Required fee = 100 sompi × compute_mass; compute_mass ≈ committed grams
    // (budget × 100) + a small size term. Cover it with headroom so bumping
    // the budget never hits the fee wall.
    let fee = 100 * (compute_budget as u64 * 100 + 3000) + 100_000;
    if value <= fee {
        bail!("coin holds {value} sompi, less than the {fee} fee for this spend");
    }

    let dest = match to {
        Some(a) => Address::try_from(a).map_err(|e| anyhow::anyhow!("bad --to address: {e}"))?,
        None => address_of(keypair),
    };
    let dest_spk = pay_to_address_script(&dest);

    // Assemble the spending tx: one input (the covenant state), one output
    // (the reclaimed funds) — a burn that reveals the program.
    let outpoint = TransactionOutpoint::new(state.outpoint.transaction_id, state.outpoint.index);
    let input = TransactionInput::new_with_mass(
        outpoint,
        vec![],
        sequence, // age-gated entrypoints (inherit) state the coin's age here
        ComputeCommit::ComputeBudget(ComputeBudget(compute_budget)),
    );
    let output = TransactionOutput::new(value - fee, dest_spk);
    let tx = Transaction::new(TX_VERSION_TOCCATA, vec![input], vec![output], 0, SUBNETWORK_ID_NATIVE, 0, vec![]);
    let entry = UtxoEntry::new(
        value,
        state.utxo_entry.script_public_key.clone(),
        state.utxo_entry.block_daa_score,
        state.utxo_entry.is_coinbase,
        state.utxo_entry.covenant_id,
    );
    let mut mtx = MutableTransaction::with_entries(tx, vec![entry]);

    // Sign the schnorr sighash over the P2SH UTXO — NOT the p2pk `sign()`,
    // which would overwrite our witness.
    let reused = SigHashReusedValuesUnsync::new();
    let sig_hash = calc_schnorr_signature_hash(&mtx.as_verifiable(), 0, SIG_HASH_ALL, &reused);
    let msg = secp256k1::Message::from_digest_slice(sig_hash.as_bytes().as_slice())?;
    let sig = keypair.sign_schnorr(msg);
    let mut sig_arg = sig.as_ref().to_vec(); // 64-byte schnorr
    sig_arg.push(SIG_HASH_ALL.to_u8()); // + hashtype = 65

    // Witness = args (pubkey, sig) ++ selector ++ redeem program.
    let mut witness = Vec::new();
    witness.extend_from_slice(&kascov_decode::encode_push(&pk));
    witness.extend_from_slice(&kascov_decode::encode_push(&sig_arg));
    if let Some(sel) = selector {
        witness.extend_from_slice(&kascov_decode::encode_push(&kascov_decode::snum(sel)));
    }
    witness.extend_from_slice(&kascov_decode::encode_push(program));
    mtx.tx.inputs[0].signature_script = witness;

    let covenant_id = state.utxo_entry.covenant_id;
    if dry_run {
        let (pass, verdict) = simulate_input(&mtx, 0);
        println!("SIMULATE   {template} . {entrypoint}  (not broadcast)");
        println!("           {}  {verdict}", if pass { "✓ PASS —" } else { "✗ FAIL —" });
        println!();
        println!("this ran the exact spend through Kaspa's real script engine — the same");
        println!("validation a node performs — without sending anything on-chain.");
        return Ok(());
    }
    let txid = submit(client, &mtx.tx).await?;
    println!("SPEND      {template} . {entrypoint}");
    println!("           tx {txid}");
    if let Some(id) = covenant_id {
        println!();
        println!("the program is now revealed on-chain. give the indexer ~a minute, then:");
        println!("  https://kascov.io/testnet-10/c/{id}");
        println!("nerd mode shows \"revealed at spend — {template}\" with your args.");
    }
    Ok(())
}

/// The whole loop in one command: emit a Mecenas reclaimable by the lab key,
/// deploy it, wait for confirmation, then reclaim it. `key_path` is only used
/// for the informational banner.
pub async fn contract_demo(client: &KaspaRpcClient, keypair: &Keypair, key_path: &Path, value: u64) -> Result<()> {
    let pk = xonly(keypair);
    let pk_hash = blake2b32(&pk);
    // Mecenas with recipient = your pubkey, funder = your blake2b (so you can
    // reclaim), a small pledge and a short period.
    let skels = kascov_decode::silverscript_skeletons();
    let mecenas = skels
        .iter()
        .find(|s| s.name == "SilverScript · Mecenas")
        .context("Mecenas skeleton unavailable")?;
    let pledge = kascov_decode::snum(100_000_000); // 1 TKAS
    let period = kascov_decode::snum(1000);
    let args: Vec<(&str, &[u8])> = vec![
        ("recipient", &pk),
        ("funder_hash", &pk_hash),
        ("pledge", &pledge),
        ("period", &period),
    ];
    let program = mecenas.emit(&args).context("failed to emit Mecenas program")?;

    println!("=== contract-demo: born → revealed, one loop ===");
    println!("key: {}", key_path.display());
    println!("address: {}", address_of(keypair));
    println!();
    println!("[1/2] deploying your Mecenas…");
    let id = deploy(client, keypair, &program, value).await?;

    // Wait for the genesis to be accepted before spending its output.
    println!();
    println!("[2/2] waiting ~15s for confirmation, then reclaiming…");
    tokio::time::sleep(Duration::from_secs(15)).await;
    spend(client, keypair, &program, "reclaim", Some(&id.to_string()), None, 20, false).await?;
    println!();
    println!("done — the coin was born as a p2sh commitment and revealed itself as");
    println!("SilverScript · Mecenas when you reclaimed it. watch its story on kascov.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: u16 = 40;
    // Must match build_constrained_spend's fee formula.
    const NET_FEE: u64 = 100 * (2 * BUDGET as u64 * 100 + 5000) + 200_000;

    fn kp(byte: u8) -> Keypair {
        Keypair::from_seckey_slice(SECP256K1, &[byte; 32]).unwrap()
    }

    fn state_utxo(program: &[u8], value: u64, covenant: Option<Hash>) -> SpendableUtxo {
        SpendableUtxo {
            outpoint: TransactionOutpoint::new(Hash::from_bytes([0x11; 32]), 0),
            entry: UtxoEntry::new(value, p2sh_spk(program), 0, false, covenant),
        }
    }

    fn fee_utxo(keypair: &Keypair, value: u64) -> SpendableUtxo {
        SpendableUtxo {
            outpoint: TransactionOutpoint::new(Hash::from_bytes([0x22; 32]), 1),
            entry: UtxoEntry::new(value, pay_to_address_script(&address_of(keypair)), 0, false, None),
        }
    }

    fn mecenas(recipient: &[u8; 32], funder_hash: &[u8; 32], pledge: i64, period: i64) -> Vec<u8> {
        let skels = kascov_decode::silverscript_skeletons();
        let skel = skels.iter().find(|s| s.name == "SilverScript · Mecenas").unwrap();
        let pledge = kascov_decode::snum(pledge);
        let period = kascov_decode::snum(period);
        let args: Vec<(&str, &[u8])> = vec![
            ("recipient", recipient),
            ("funder_hash", funder_hash),
            ("pledge", &pledge),
            ("period", &period),
        ];
        skel.emit(&args).unwrap()
    }

    fn lastwill(inheritor_hash: &[u8; 32], cold_hash: &[u8; 32], hot_hash: &[u8; 32]) -> Vec<u8> {
        let skels = kascov_decode::silverscript_skeletons();
        let skel = skels.iter().find(|s| s.name == "SilverScript · LastWill").unwrap();
        let args: Vec<(&str, &[u8])> = vec![
            ("inheritor_hash", inheritor_hash),
            ("cold_hash", cold_hash),
            ("hot_hash", hot_hash),
        ];
        skel.emit(&args).unwrap()
    }

    #[test]
    fn entrypoint_spec_maps_all_known_entrypoints() {
        assert_eq!(
            entrypoint_spec("SilverScript · Mecenas", "receive").unwrap(),
            EntrypointPlan::Constrained { kind: ConstrainedKind::MecenasReceive, selector: 0, signer_field: None }
        );
        assert_eq!(
            entrypoint_spec("SilverScript · LastWill", "refresh").unwrap(),
            EntrypointPlan::Constrained {
                kind: ConstrainedKind::LastWillRefresh,
                selector: 2,
                signer_field: Some("hot_hash")
            }
        );
        assert_eq!(
            entrypoint_spec("SilverScript · Mecenas", "reclaim").unwrap(),
            EntrypointPlan::PureSig { selector: Some(1), signer_field: "funder_hash", sequence: 0 }
        );
        assert_eq!(
            entrypoint_spec("SilverScript · LastWill", "inherit").unwrap(),
            EntrypointPlan::PureSig { selector: Some(0), signer_field: "inheritor_hash", sequence: 180 }
        );
        assert!(entrypoint_spec("SilverScript · Escrow", "spend").unwrap_err().to_string().contains("settle-escrow"));
        assert!(entrypoint_spec("SilverScript · Mecenas", "yolo").is_err());
    }

    #[test]
    fn snum_round_trips() {
        for v in [0i64, 1, 6, 180, 1000, 100_000_000, 250_000_000, i64::from(u32::MAX)] {
            assert_eq!(snum_to_i64(&kascov_decode::snum(v)).unwrap(), v, "snum({v})");
        }
    }

    #[test]
    fn mecenas_receive_continuation_shape_and_engine_pass() {
        let key = kp(7);
        let recipient = xonly(&kp(9)); // some other party
        let funder = [0xf0u8; 32];
        let pledge: u64 = 100_000_000;
        let period: u64 = 1000;
        let program = mecenas(&recipient, &funder, pledge as i64, period as i64);
        let id = Hash::from_bytes([0x42; 32]);

        let cv: u64 = 1_000_000_000; // change = cv − pledge − 1000 > pledge + 1000 → covenant continues
        let state = state_utxo(&program, cv, Some(id));
        let funding = fee_utxo(&key, 50_000_000);
        let mtx = build_constrained_spend(&key, &program, "receive", &state, &funding, BUDGET).unwrap();

        // exact tx shape
        assert_eq!(mtx.tx.inputs.len(), 2);
        assert_eq!(mtx.tx.outputs.len(), 3);
        // outputs[0]: exactly the pledge, P2PK(recipient), NOT covenant-bound
        let recipient_spk =
            pay_to_address_script(&Address::new(Prefix::Testnet, AddrVersion::PubKey, &recipient));
        assert_eq!(mtx.tx.outputs[0].value, pledge);
        assert_eq!(mtx.tx.outputs[0].script_public_key, recipient_spk);
        assert!(mtx.tx.outputs[0].covenant.is_none());
        // outputs[1]: the continuation — same P2SH, cv − pledge − 1000, re-bound
        assert_eq!(mtx.tx.outputs[1].value, cv - pledge - 1000);
        assert_eq!(mtx.tx.outputs[1].script_public_key, p2sh_spk(&program));
        assert_eq!(mtx.tx.outputs[1].covenant, Some(CovenantBinding::new(0, id)));
        // outputs[2]: fee change back to us; tx balances at exactly NET_FEE
        assert_eq!(mtx.tx.outputs[2].value, 50_000_000 + 1000 - NET_FEE);
        assert_eq!(mtx.tx.outputs[2].script_public_key, pay_to_address_script(&address_of(&key)));
        let in_sum = cv + 50_000_000;
        let out_sum: u64 = mtx.tx.outputs.iter().map(|o| o.value).sum();
        assert_eq!(in_sum - out_sum, NET_FEE);
        // input 0 states the coin's age (the compiled CSV gate) via sequence
        assert_eq!(mtx.tx.inputs[0].sequence, period);
        // receive takes no pk/sig: witness = push(selector 0) ++ push(program)
        let mut expect = kascov_decode::encode_push(&kascov_decode::snum(0));
        expect.extend_from_slice(&kascov_decode::encode_push(&program));
        assert_eq!(mtx.tx.inputs[0].signature_script, expect);

        // the real script engine accepts both inputs
        let (pass0, verdict0) = simulate_input(&mtx, 0);
        assert!(pass0, "covenant input rejected: {verdict0}");
        let (pass1, verdict1) = simulate_input(&mtx, 1);
        assert!(pass1, "fee input rejected: {verdict1}");
    }

    #[test]
    fn mecenas_receive_terminal_pays_everything_minus_1000() {
        let key = kp(7);
        let recipient = xonly(&kp(9));
        let pledge: u64 = 100_000_000;
        let program = mecenas(&recipient, &[0xf0; 32], pledge as i64, 1000);

        let cv = pledge + 1500; // change = 500 ≤ pledge + 1000 → terminal payout
        let state = state_utxo(&program, cv, Some(Hash::from_bytes([0x42; 32])));
        let funding = fee_utxo(&key, 50_000_000);
        let mtx = build_constrained_spend(&key, &program, "receive", &state, &funding, BUDGET).unwrap();

        assert_eq!(mtx.tx.outputs.len(), 2, "terminal receive: recipient payout + fee change only");
        assert_eq!(mtx.tx.outputs[0].value, cv - 1000);
        assert_eq!(
            mtx.tx.outputs[0].script_public_key,
            pay_to_address_script(&Address::new(Prefix::Testnet, AddrVersion::PubKey, &recipient))
        );
        assert!(mtx.tx.outputs.iter().all(|o| o.covenant.is_none()), "covenant must end here");
        let (pass, verdict) = simulate_input(&mtx, 0);
        assert!(pass, "terminal receive rejected: {verdict}");
    }

    #[test]
    fn mecenas_receive_age_gate_is_modeled() {
        let key = kp(7);
        let recipient = xonly(&kp(9));
        let program = mecenas(&recipient, &[0xf0; 32], 100_000_000, 1000);
        let state = state_utxo(&program, 1_000_000_000, Some(Hash::from_bytes([0x42; 32])));
        let funding = fee_utxo(&key, 50_000_000);
        let mut mtx = build_constrained_spend(&key, &program, "receive", &state, &funding, BUDGET).unwrap();

        // pretend the coin is younger than the period: CSV must reject
        mtx.tx.inputs[0].sequence = 0;
        let (pass, verdict) = simulate_input(&mtx, 0);
        assert!(!pass, "engine accepted an under-age receive");
        assert!(verdict.to_lowercase().contains("lock"), "unexpected rejection reason: {verdict}");
    }

    #[test]
    fn mecenas_receive_wrong_payout_is_rejected_by_the_contract() {
        let key = kp(7);
        let recipient = xonly(&kp(9));
        let program = mecenas(&recipient, &[0xf0; 32], 100_000_000, 1000);
        let state = state_utxo(&program, 1_000_000_000, Some(Hash::from_bytes([0x42; 32])));
        let funding = fee_utxo(&key, 50_000_000);
        let mut mtx = build_constrained_spend(&key, &program, "receive", &state, &funding, BUDGET).unwrap();

        // skim one sompi off the recipient — introspection must catch it
        // (input 0 carries no signature, so this is purely the contract's check)
        mtx.tx.outputs[0].value -= 1;
        let (pass, _) = simulate_input(&mtx, 0);
        assert!(!pass, "engine accepted a wrong pledge payout");
    }

    #[test]
    fn lastwill_refresh_shape_and_engine_pass() {
        let key = kp(7); // the hot key
        let hot_hash = blake2b32(&xonly(&key));
        let program = lastwill(&[0xaa; 32], &[0xbb; 32], &hot_hash);
        let id = Hash::from_bytes([0x43; 32]);

        let cv: u64 = 500_000_000;
        let state = state_utxo(&program, cv, Some(id));
        let funding = fee_utxo(&key, 50_000_000);
        let mtx = build_constrained_spend(&key, &program, "refresh", &state, &funding, BUDGET).unwrap();

        assert_eq!(mtx.tx.inputs.len(), 2);
        assert_eq!(mtx.tx.outputs.len(), 2);
        // outputs[0]: the same P2SH, cv − 1000, covenant re-bound
        assert_eq!(mtx.tx.outputs[0].value, cv - 1000);
        assert_eq!(mtx.tx.outputs[0].script_public_key, p2sh_spk(&program));
        assert_eq!(mtx.tx.outputs[0].covenant, Some(CovenantBinding::new(0, id)));
        // outputs[1]: fee change; balances at exactly NET_FEE
        assert_eq!(mtx.tx.outputs[1].value, 50_000_000 + 1000 - NET_FEE);
        let out_sum: u64 = mtx.tx.outputs.iter().map(|o| o.value).sum();
        assert_eq!(cv + 50_000_000 - out_sum, NET_FEE);
        // witness ends with push(selector 2) ++ push(program), after pk+sig
        let mut suffix = kascov_decode::encode_push(&kascov_decode::snum(2));
        suffix.extend_from_slice(&kascov_decode::encode_push(&program));
        assert!(mtx.tx.inputs[0].signature_script.ends_with(&suffix));
        assert_eq!(mtx.tx.inputs[0].signature_script[0], 0x20, "witness starts with the 32-byte pubkey push");

        let (pass0, verdict0) = simulate_input(&mtx, 0);
        assert!(pass0, "refresh rejected: {verdict0}");
        let (pass1, verdict1) = simulate_input(&mtx, 1);
        assert!(pass1, "fee input rejected: {verdict1}");
    }

    #[test]
    fn lastwill_refresh_wrong_key_bails() {
        let key = kp(7);
        let program = lastwill(&[0xaa; 32], &[0xbb; 32], &[0xcc; 32]); // hot ≠ our key
        let state = state_utxo(&program, 500_000_000, None);
        let funding = fee_utxo(&key, 50_000_000);
        let err = build_constrained_spend(&key, &program, "refresh", &state, &funding, BUDGET).unwrap_err();
        assert!(err.to_string().contains("hot_hash"), "unexpected error: {err}");
    }

    #[test]
    fn constrained_builder_rejects_pure_sig_entrypoints_and_bad_state() {
        let key = kp(7);
        let recipient = xonly(&kp(9));
        let program = mecenas(&recipient, &[0xf0; 32], 100_000_000, 1000);
        let state = state_utxo(&program, 1_000_000_000, None);
        let funding = fee_utxo(&key, 50_000_000);
        // reclaim is pure-signature — this builder must refuse it
        assert!(build_constrained_spend(&key, &program, "reclaim", &state, &funding, BUDGET).is_err());
        // state spk that isn't the program's P2SH must refuse
        let wrong = SpendableUtxo {
            outpoint: state.outpoint,
            entry: UtxoEntry::new(1_000_000_000, pay_to_address_script(&address_of(&key)), 0, false, None),
        };
        assert!(build_constrained_spend(&key, &program, "receive", &wrong, &funding, BUDGET).is_err());
        // fee UTXO too small must refuse
        let broke = fee_utxo(&key, 100_000);
        assert!(build_constrained_spend(&key, &program, "receive", &state, &broke, BUDGET).is_err());
    }
}
