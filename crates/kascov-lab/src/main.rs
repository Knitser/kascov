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

const FEE: u64 = 500_000; // 0.005 KAS per tx — TN10's post-Toccata minimum relay fee is ~0.00166 for 1-in-1-out

#[derive(Parser)]
#[command(
    name = "kascov-lab",
    about = "Covenant lab: create real covenants on testnet-10",
    long_about = "Covenant lab: create real covenants on testnet-10.

NEW HERE? one command does the whole loop — deploy a contract and watch
it run itself on-chain:

    cargo run -p kascov-lab -- escrow-demo      # an escrow settles itself
    cargo run -p kascov-lab -- contract-demo    # a Mecenas reveals itself

Then open the kascov link it prints. `kascov-lab examples` shows every
copy-paste recipe; each subcommand also has its own --help."
)]
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
    /// Print every copy-paste recipe (needs no key or network).
    Examples,
    /// ★ START HERE — the whole loop in one command: emit a Mecenas
    /// reclaimable by YOUR key, deploy it, then reclaim it — you watch a coin
    /// get born and reveal itself as SilverScript · Mecenas on kascov.
    ContractDemo {
        /// Sompi the coin holds while it lives (default 10 TKAS)
        #[arg(long, default_value_t = 1_000_000_000)]
        value: u64,
    },
    /// ★ START HERE — an escrow, end to end: emit one (arbiter = you, buyer =
    /// you, seller = a throwaway), deploy it, then settle it to the buyer — a
    /// real dispute resolution playing out on testnet-10 in one command.
    EscrowDemo {
        /// Sompi held in escrow (default 10 TKAS)
        #[arg(long, default_value_t = 1_000_000_000)]
        value: u64,
    },
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
        /// Don't broadcast — run the spend through the real script engine and
        /// report whether the contract would accept it (a "what-if" test).
        #[arg(long)]
        dry_run: bool,
    },
    /// Settle a deployed Escrow ON ITS OWN TERMS: the arbiter (your key)
    /// signs, and the contract's introspection rules force output 0 to pay
    /// the buyer or the seller exactly value − 1000 sompi. A second plain
    /// input funds the real network fee, so the contract's hardcoded
    /// 1000-sompi fee and the node's compute-mass fee can both hold.
    SettleEscrow {
        /// Compiled Escrow hex (arbiter must be your keygen blake2b)
        #[arg(long)]
        program_hex: String,
        /// Who gets the funds: buyer | seller
        #[arg(long, default_value = "buyer")]
        release_to: String,
        /// Which covenant to settle, when several share this program
        #[arg(long)]
        covenant: Option<String>,
        /// Don't broadcast — simulate the settlement through the script engine.
        #[arg(long)]
        dry_run: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Keygen => keygen(&cli),
        Command::Balance => balance(&cli).await,
        Command::Examples => examples(),
        Command::Demo { transitions } => demo(&cli, transitions).await,
        Command::Deploy { ref program_hex, value } => {
            let program = hex::decode(program_hex.trim()).context("--program-hex is not valid hex")?;
            deploy(&cli, &program, value).await.map(|_| ())
        }
        Command::Spend { ref program_hex, ref entrypoint, ref covenant, ref to, compute_budget, dry_run } => {
            let program = hex::decode(program_hex.trim()).context("--program-hex is not valid hex")?;
            let target = covenant
                .as_deref()
                .map(|c| c.parse::<kaspa_consensus_core::Hash>().context("bad --covenant id"))
                .transpose()?;
            spend(&cli, &program, entrypoint, target, to.as_deref(), compute_budget, dry_run).await
        }
        Command::ContractDemo { value } => contract_demo(&cli, value).await,
        Command::SettleEscrow { ref program_hex, ref release_to, ref covenant, dry_run } => {
            let program = hex::decode(program_hex.trim()).context("--program-hex is not valid hex")?;
            let target = covenant
                .as_deref()
                .map(|c| c.parse::<kaspa_consensus_core::Hash>().context("bad --covenant id"))
                .transpose()?;
            settle_escrow(&cli, &program, release_to, target, dry_run).await
        }
        Command::EscrowDemo { value } => escrow_demo(&cli, value).await,
    }
}

/// Copy-paste cheat sheet — the fastest way to see everything the lab does.
fn examples() -> Result<()> {
    println!(
        r#"kascov-lab — make real smart coins on Kaspa testnet-10
======================================================

★ FASTEST: one command deploys a contract AND runs it on-chain.
  Open the kascov link it prints (flip on "nerd mode") to see the coin
  reveal itself as its named contract, for everyone, forever.

    cargo run -p kascov-lab -- escrow-demo
        an escrow is deployed, then the arbiter settles it — the contract
        itself forces the payout to the buyer.

    cargo run -p kascov-lab -- contract-demo
        a Mecenas is deployed, then reclaimed — it reveals itself on-chain.

SETUP (only once):
    cargo run -p kascov-lab -- keygen          # makes a key, prints your address
    # fund that address at https://faucet-testnet.kaspanet.io (open in a browser)
    cargo run -p kascov-lab -- balance         # check it arrived

MAKE YOUR OWN (choose the parameters yourself):
    1. https://kascov-explorer.web.app/decode  → "make a Mecenas / Escrow / LastWill"
       edit the fields (use your keygen pubkey / blake2b), copy the compiled hex.
    2. deploy it (born as a hidden p2sh commitment):
       cargo run -p kascov-lab -- deploy --program-hex <hex> --value 1000000000
    3. reveal it by spending it (names the coin on kascov, permanently):
       cargo run -p kascov-lab -- spend --program-hex <hex> --entrypoint reclaim
       # escrow instead? settle it to a party:
       cargo run -p kascov-lab -- settle-escrow --program-hex <hex> --release-to buyer

Every command has its own help, e.g.:  cargo run -p kascov-lab -- spend --help
Full guide: docs/Covenant Lab.md
"#
    );
    Ok(())
}

/// Dry-run one input of a built transaction through the real Kaspa script
/// engine — the exact validation a node performs — WITHOUT broadcasting.
/// Returns (passed, human verdict). This is "what-if spend": test a covenant
/// spend before you send it, or simulate a spend you can't even sign.
fn simulate_input(mtx: &MutableTransaction<Transaction>, idx: usize) -> (bool, String) {
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
    println!("watch it live (give the indexer ~a minute) — this link proves the");
    println!("commitment in the browser, no spend needed:");
    println!("  https://kascov-explorer.web.app/testnet-10/c/{id}?program={}", hex::encode(program));
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
async fn settle_escrow(
    cli: &Cli,
    program: &[u8],
    release_to: &str,
    target_covenant: Option<kaspa_consensus_core::Hash>,
    dry_run: bool,
) -> Result<()> {
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

    let keypair = load_or_create_key(&cli.key, false)?;
    let pk = xonly(&keypair);
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
    let client = connect(cli).await?;
    let states = client.get_utxos_by_addresses(vec![p2sh_addr.clone().into()]).await?;
    let state = match target_covenant {
        Some(t) => states.iter().find(|u| u.utxo_entry.covenant_id == Some(t)),
        None => states.iter().find(|u| u.utxo_entry.covenant_id.is_some()).or_else(|| states.first()),
    }
    .with_context(|| format!("no live escrow state at {p2sh_addr}"))?;

    // …and a plain UTXO of ours to pay the real network fee.
    let my_addr = address_of(&keypair);
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
    let txid = submit(&client, &mtx.tx).await?;
    println!("SETTLED    Escrow → {release_to} ({:.8} TKAS released)", (state_value - 1000) as f64 / 1e8);
    println!("           tx {txid}");
    if let Some(id) = covenant_id_opt {
        println!();
        println!("the escrow revealed itself on-chain. watch the story:");
        println!("  https://kascov-explorer.web.app/testnet-10/c/{id}");
    }
    Ok(())
}

/// Escrow end-to-end: emit (arbiter = you, buyer = you, seller = throwaway),
/// deploy, settle to the buyer.
async fn escrow_demo(cli: &Cli, value: u64) -> Result<()> {
    let keypair = load_or_create_key(&cli.key, true)?;
    let pk = xonly(&keypair);
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
    let id = deploy(cli, &program, value).await?;
    println!();
    println!("[2/2] waiting ~15s, then settling to the buyer…");
    tokio::time::sleep(Duration::from_secs(15)).await;
    settle_escrow(cli, &program, "buyer", Some(id), false).await?;
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
    dry_run: bool,
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
    if dry_run {
        let (pass, verdict) = simulate_input(&mtx, 0);
        println!("SIMULATE   {template} . {entrypoint}  (not broadcast)");
        println!("           {}  {verdict}", if pass { "✓ PASS —" } else { "✗ FAIL —" });
        println!();
        println!("this ran the exact spend through Kaspa's real script engine — the same");
        println!("validation a node performs — without sending anything on-chain.");
        return Ok(());
    }
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
    spend(cli, &program, "reclaim", Some(id), None, 20, false).await?;
    println!();
    println!("done — the coin was born as a p2sh commitment and revealed itself as");
    println!("SilverScript · Mecenas when you reclaimed it. watch its story on kascov.");
    Ok(())
}
