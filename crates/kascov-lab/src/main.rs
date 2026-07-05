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
    sign::sign,
    subnets::SUBNETWORK_ID_NATIVE,
    tx::{
        CovenantBinding, MutableTransaction, Transaction, TransactionInput,
        TransactionOutpoint, TransactionOutput, UtxoEntry,
    },
};
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_txscript::pay_to_address_script;
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Keygen => keygen(&cli),
        Command::Balance => balance(&cli).await,
        Command::Demo { transitions } => demo(&cli, transitions).await,
        Command::Deploy { ref program_hex, value } => deploy(&cli, program_hex, value).await,
    }
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
async fn deploy(cli: &Cli, program_hex: &str, value: u64) -> Result<()> {
    let program = hex::decode(program_hex.trim()).context("--program-hex is not valid hex")?;
    if program.is_empty() {
        bail!("empty program");
    }

    // Name what we're deploying (warn-and-proceed on unknown shapes: the
    // chain doesn't care, but the user should know kascov won't label it).
    let decoded = kascov_decode::Registry::default().decode(0, &program);
    match decoded.template {
        Some(t) => println!("program:   {t} ({} bytes)", program.len()),
        None => println!("program:   unrecognized shape ({} bytes) — deploying anyway; kascov will show it as a plain p2sh commitment", program.len()),
    }
    for f in &decoded.fields {
        println!("           {} = {}", f.name, hex::encode(&f.value));
    }

    let commitment = blake2b_simd::Params::new().hash_length(32).hash(&program);
    let mut spk_script = Vec::with_capacity(35);
    spk_script.push(0xaa); // OpBlake2b
    spk_script.push(0x20); // push 32
    spk_script.extend_from_slice(commitment.as_bytes());
    spk_script.push(0x87); // OpEqual
    let p2sh_spk = kaspa_consensus_core::tx::ScriptPublicKey::from_vec(0, spk_script);

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
    println!("           program blake2b {}", hex::encode(commitment.as_bytes()));
    println!();
    println!("watch it live (give the indexer ~a minute):");
    println!("  https://kascov-explorer.web.app/testnet-10/c/{id}");
    println!();
    println!("honest note: the coin shows as a 'p2sh commitment' until a spend");
    println!("reveals the program; spending means satisfying the contract's own");
    println!("rules — this lab doesn't do that part (yet).");
    Ok(())
}

fn build_signed(keypair: &Keypair, from: &SpendableUtxo, outputs: Vec<TransactionOutput>) -> Result<Transaction> {
    let input = TransactionInput::new(from.outpoint, vec![], 0, 1);
    let tx = Transaction::new(TX_VERSION_TOCCATA, vec![input], outputs, 0, SUBNETWORK_ID_NATIVE, 0, vec![]);
    let signable = MutableTransaction::with_entries(tx, vec![from.entry.clone()]);
    let signed = sign(signable, *keypair);
    Ok(signed.tx)
}
