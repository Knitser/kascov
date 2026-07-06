//! Lab tooling for exercising covenants on testnet. Unlike kascov-core, this
//! binary uses the kaspa crates directly — it exists to create real covenant
//! transactions that the explorer can then index and trace.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
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
};
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_txscript::{extract_script_pub_key_address, pay_to_address_script, pay_to_script_hash_script};
use kaspa_wrpc_client::{
    client::{ConnectOptions, ConnectStrategy},
    prelude::{NetworkId, NetworkType},
    KaspaRpcClient, Resolver, WrpcEncoding,
};
use secp256k1::{Keypair, SECP256K1};

const FEE: u64 = 500_000; // 0.005 KAS per tx — TN10's post-Toccata minimum relay fee is ~0.00166 for 1-in-1-out

#[derive(Parser)]
#[command(name = "kascov-lab", about = "Covenant lab: create real covenants on testnet-10")]
struct Cli {
    /// wRPC (borsh) url; defaults to the public resolver
    #[arg(long)]
    rpc: Option<String>,

    /// Key file (hex-encoded 32-byte secret)
    #[arg(long, default_value = "/tmp/kascov-lab-key.hex")]
    key: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a keypair (if none exists) and print the TN10 address.
    Keygen,
    /// Show the address and its current UTXO balance.
    Balance,
    /// Run the full covenant lifecycle: genesis → N transitions → burn.
    Demo {
        #[arg(long, default_value_t = 2)]
        transitions: u32,
    },
    /// Birth a compiled contract as a real covenant: its P2SH commitment
    /// becomes the coin's state. Pairs with the generator on
    /// kascov-explorer.web.app/decode ("make this yours").
    Deploy {
        /// Compiled contract hex (the generator's "compiled" block)
        #[arg(long)]
        program_hex: String,
        /// Sompi the newborn coin holds (default 10 TKAS)
        #[arg(long, default_value_t = 1_000_000_000)]
        value: u64,
    },
    /// Spend a deployed contract coin — reveals the program on-chain, so
    /// kascov shows it as your named contract, permanently. v1 supports the
    /// pure-signature entrypoints (Mecenas.reclaim, LastWill.cold/inherit):
    /// they need only a signature from the matching key.
    Spend {
        /// Compiled contract hex of the deployed coin
        #[arg(long)]
        program_hex: String,
        /// Which entrypoint to satisfy (reclaim | cold | inherit)
        #[arg(long, default_value = "reclaim")]
        entrypoint: String,
        /// Which covenant to spend, when several coins share this program
        #[arg(long)]
        covenant: Option<String>,
        /// Where the reclaimed funds go (default: your own address)
        #[arg(long)]
        to: Option<String>,
        /// Per-input compute budget to commit. 1 unit = 10 000 script units;
        /// a signature spend needs only a handful. Fee scales with it.
        #[arg(long, default_value_t = 20)]
        compute_budget: u16,
    },
    /// The whole loop in one command: emit a Mecenas reclaimable by YOUR key,
    /// deploy it, then reclaim it — so you watch a coin get born and reveal
    /// itself as SilverScript · Mecenas on kascov.
    ContractDemo {
        /// Sompi the coin holds while it lives (default 10 TKAS)
        #[arg(long, default_value_t = 1_000_000_000)]
        value: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Keygen => keygen(&cli),
        Command::Balance => balance(&cli).await,
        Command::Demo { transitions } => demo(&cli, transitions).await,
        Command::Deploy { ref program_hex, value } => {
            let program = hex::decode(program_hex.trim()).context("--program-hex is not valid hex")?;
            deploy(&cli, &program, value).await.map(|_| ())
        }
        Command::Spend { ref program_hex, ref entrypoint, ref covenant, ref to, compute_budget } => {
            let program = hex::decode(program_hex.trim()).context("--program-hex is not valid hex")?;
            let target = covenant
                .as_deref()
                .map(|c| c.parse::<kaspa_consensus_core::Hash>().context("bad --covenant id"))
                .transpose()?;
            spend(&cli, &program, entrypoint, target, to.as_deref(), compute_budget).await
        }
        Command::ContractDemo { value } => contract_demo(&cli, value).await,
    }
}

/// Blake2b-256, the covenant P2SH commitment hash.
fn blake2b32(bytes: &[u8]) -> [u8; 32] {
    *blake2b_simd::Params::new().hash_length(32).hash(bytes).as_bytes().first_chunk::<32>().unwrap()
}

/// The x-only public key of a keypair, 32 bytes.
fn xonly(keypair: &Keypair) -> [u8; 32] {
    keypair.public_key().x_only_public_key().0.serialize()
}

/// The P2SH commitment scriptPubKey for a redeem program (OpBlake2b <h> OpEqual).
fn p2sh_spk(program: &[u8]) -> ScriptPublicKey {
    pay_to_script_hash_script(program)
}

/// For a recognized contract + entrypoint: (selector to push, the committed
/// hash field the signer must match). v1 = pure-signature entrypoints only.
fn entrypoint_spec(template: &str, entrypoint: &str) -> Result<(Option<i64>, &'static str)> {
    let spec = match (template, entrypoint) {
        ("SilverScript · Mecenas", "reclaim") => (Some(1), "funder_hash"),
        ("SilverScript · LastWill", "cold") => (Some(1), "cold_hash"),
        ("SilverScript · LastWill", "inherit") => (Some(0), "inheritor_hash"),
        (_, "receive" | "refresh" | "spend") => bail!(
            "entrypoint '{entrypoint}' constrains the transaction outputs (introspection) — \
             not supported by this lab yet; use a pure-signature entrypoint (reclaim/cold/inherit)"
        ),
        _ => bail!("don't know how to satisfy {template} . {entrypoint}"),
    };
    Ok(spec)
}

fn load_or_create_key(path: &PathBuf, create: bool) -> Result<Keypair> {
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

fn address_of(keypair: &Keypair) -> Address {
    let (xonly, _) = keypair.public_key().x_only_public_key();
    Address::new(Prefix::Testnet, AddrVersion::PubKey, &xonly.serialize())
}

fn keygen(cli: &Cli) -> Result<()> {
    let keypair = load_or_create_key(&cli.key, true)?;
    let (xonly, _) = keypair.public_key().x_only_public_key();
    let pk = xonly.serialize();
    let pk_hash = blake2b_simd::Params::new().hash_length(32).hash(&pk);
    println!("key file:        {}", cli.key.display());
    println!("address:         {}", address_of(&keypair));
    println!("pubkey (x-only): {}", hex::encode(pk));
    println!("blake2b(pubkey): {}", hex::encode(pk_hash.as_bytes()));
    println!();
    println!("the pubkey and its blake2b fill the generator's key fields on");
    println!("kascov-explorer.web.app/decode — fund the address at");
    println!("https://faucet-testnet.kaspanet.io before deploying.");
    Ok(())
}

async fn connect(cli: &Cli) -> Result<KaspaRpcClient> {
    let network_id = NetworkId::with_suffix(NetworkType::Testnet, 10);
    let resolver = cli.rpc.is_none().then(Resolver::default);
    let client = KaspaRpcClient::new(WrpcEncoding::Borsh, cli.rpc.as_deref(), resolver, Some(network_id), None)?;
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

async fn balance(cli: &Cli) -> Result<()> {
    let keypair = load_or_create_key(&cli.key, false)?;
    let address = address_of(&keypair);
    let client = connect(cli).await?;
    let utxos = client.get_utxos_by_addresses(vec![address.clone().into()]).await?;
    let total: u64 = utxos.iter().map(|u| u.utxo_entry.amount).sum();
    println!("address: {address}");
    println!("utxos:   {}", utxos.len());
    println!("balance: {:.8} TKAS", total as f64 / 100_000_000.0);
    Ok(())
}

struct SpendableUtxo {
    outpoint: TransactionOutpoint,
    entry: UtxoEntry,
}

async fn submit(client: &KaspaRpcClient, tx: &Transaction) -> Result<String> {
    let rpc_tx: kaspa_rpc_core::RpcTransaction = tx.into();
    let id = client.submit_transaction(rpc_tx, false).await.context("submit failed")?;
    Ok(id.to_string())
}

async fn demo(cli: &Cli, transitions: u32) -> Result<()> {
    let keypair = load_or_create_key(&cli.key, false)?;
    let address = address_of(&keypair);
    let spk = pay_to_address_script(&address);
    let client = connect(cli).await?;

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
    let tx = build_signed(&keypair, &current, vec![genesis_output])?;
    let txid = submit(&client, &tx).await?;
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
        let tx = build_signed(&keypair, &current, vec![output])?;
        let txid = submit(&client, &tx).await?;
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
    let tx = build_signed(&keypair, &current, vec![output])?;
    let txid = submit(&client, &tx).await?;
    println!("BURN       tx {txid}");
    println!();
    println!("covenant lifecycle complete — trace it with:");
    println!("  kascov --network testnet-10 trace {id}");
    Ok(())
}

/// Birth a compiled contract: the coin's state is the P2SH commitment of the
/// program (OpBlake2b <blake2b-256> OpEqual — the exact shape the explorer
/// recognizes and, at spend time, verifies against the revealed program).
async fn deploy(cli: &Cli, program: &[u8], value: u64) -> Result<kaspa_consensus_core::Hash> {
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

    let keypair = load_or_create_key(&cli.key, false)?;
    let address = address_of(&keypair);
    let plain_spk = pay_to_address_script(&address);
    let client = connect(cli).await?;

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

    let tx = build_signed(&keypair, &funding_utxo, outputs)?;
    let txid = submit(&client, &tx).await?;
    println!();
    println!("BIRTH      covenant {id}");
    println!("           tx {txid}");
    println!("           program blake2b {}", hex::encode(commitment));
    println!();
    println!("watch it live (give the indexer ~a minute):");
    println!("  https://kascov-explorer.web.app/testnet-10/c/{id}");
    println!();
    println!("the coin shows as a 'p2sh commitment' (the program is hidden) until");
    println!("you SPEND it — that reveals the program on-chain and kascov names it:");
    println!("  kascov-lab spend --program-hex <the same hex> --entrypoint reclaim");
    println!("(reclaim needs the coin's funder_hash to be your key's blake2b —");
    println!(" `kascov-lab keygen` prints it. Or just run `kascov-lab contract-demo`.)");
    Ok(id)
}

fn build_signed(keypair: &Keypair, from: &SpendableUtxo, outputs: Vec<TransactionOutput>) -> Result<Transaction> {
    let input = TransactionInput::new(from.outpoint, vec![], 0, 1);
    let tx = Transaction::new(TX_VERSION_TOCCATA, vec![input], outputs, 0, SUBNETWORK_ID_NATIVE, 0, vec![]);
    let signable = MutableTransaction::with_entries(tx, vec![from.entry.clone()]);
    let signed = sign(signable, *keypair);
    Ok(signed.tx)
}

/// Spend a deployed contract coin by satisfying one of its pure-signature
/// entrypoints. The unlocking script is the revealed contract program:
///   push(pubkey) ++ push(sig) ++ [push(selector)] ++ push(program)
/// The spend reveals the program on-chain; kascov's indexer then shows the
/// coin as its named contract for everyone, permanently.
async fn spend(
    cli: &Cli,
    program: &[u8],
    entrypoint: &str,
    target_covenant: Option<kaspa_consensus_core::Hash>,
    to: Option<&str>,
    compute_budget: u16,
) -> Result<()> {
    if program.is_empty() {
        bail!("empty program");
    }
    let decoded = kascov_decode::Registry::default().decode(0, program);
    let template = decoded.template.context(
        "this program isn't a recognized SilverScript contract — the lab only knows how to spend Mecenas/Escrow/LastWill",
    )?;
    let (selector, signer_field) = entrypoint_spec(template, entrypoint)?;

    // The key that signs must be the one the contract checks for this entrypoint.
    let keypair = load_or_create_key(&cli.key, false)?;
    let pk = xonly(&keypair);
    let pk_hash = blake2b32(&pk);
    let committed = decoded
        .fields
        .iter()
        .find(|f| f.name == signer_field)
        .map(|f| f.value.clone())
        .with_context(|| format!("{template} has no {signer_field} field"))?;
    if committed != pk_hash {
        bail!(
            "this coin's {signer_field} is {}, but your key's blake2b is {} — you can't {entrypoint} it.\n\
             deploy a coin whose {signer_field} = your `kascov-lab keygen` blake2b, then spend that one.",
            hex::encode(&committed),
            hex::encode(pk_hash)
        );
    }

    // Find the coin's live state UTXO from the node, via its P2SH address.
    let spk = p2sh_spk(program);
    let p2sh_addr = extract_script_pub_key_address(&spk, Prefix::Testnet)
        .map_err(|e| anyhow::anyhow!("cannot derive P2SH address: {e:?}"))?;
    let client = connect(cli).await?;
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
        None => address_of(&keypair),
    };
    let dest_spk = pay_to_address_script(&dest);

    // Assemble the spending tx: one input (the covenant state), one output
    // (the reclaimed funds) — a burn that reveals the program.
    let outpoint = TransactionOutpoint::new(state.outpoint.transaction_id, state.outpoint.index);
    let input = TransactionInput::new_with_mass(
        outpoint,
        vec![],
        0,
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
    let txid = submit(&client, &mtx.tx).await?;
    println!("SPEND      {template} . {entrypoint}");
    println!("           tx {txid}");
    if let Some(id) = covenant_id {
        println!();
        println!("the program is now revealed on-chain. give the indexer ~a minute, then:");
        println!("  https://kascov-explorer.web.app/testnet-10/c/{id}");
        println!("nerd mode shows \"revealed at spend — {template}\" with your args.");
    }
    Ok(())
}

/// The whole loop in one command: emit a Mecenas reclaimable by the lab key,
/// deploy it, wait for confirmation, then reclaim it.
async fn contract_demo(cli: &Cli, value: u64) -> Result<()> {
    let keypair = load_or_create_key(&cli.key, true)?;
    let pk = xonly(&keypair);
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
    println!("key: {}", cli.key.display());
    println!("address: {}", address_of(&keypair));
    println!();
    println!("[1/2] deploying your Mecenas…");
    let id = deploy(cli, &program, value).await?;

    // Wait for the genesis to be accepted before spending its output.
    println!();
    println!("[2/2] waiting ~15s for confirmation, then reclaiming…");
    tokio::time::sleep(Duration::from_secs(15)).await;
    spend(cli, &program, "reclaim", Some(id), None, 20).await?;
    println!();
    println!("done — the coin was born as a p2sh commitment and revealed itself as");
    println!("SilverScript · Mecenas when you reclaimed it. watch its story on kascov.");
    Ok(())
}
