//! Lab tooling for exercising covenants on testnet. A thin CLI over
//! `kascov-labkit`, which holds all the tx-building, signing, and broadcast
//! logic — it exists to create real covenant transactions that the explorer
//! can then index and trace.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

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
        Command::Keygen => kascov_labkit::keygen(&cli.key),
        Command::Balance => {
            let keypair = kascov_labkit::load_or_create_key(&cli.key, false)?;
            let client = kascov_labkit::connect(cli.rpc.as_deref()).await?;
            kascov_labkit::balance(&client, &keypair).await
        }
        Command::Examples => examples(),
        Command::Demo { transitions } => {
            let keypair = kascov_labkit::load_or_create_key(&cli.key, false)?;
            let client = kascov_labkit::connect(cli.rpc.as_deref()).await?;
            kascov_labkit::demo(&client, &keypair, transitions).await
        }
        Command::Deploy { ref program_hex, value } => {
            let program = hex::decode(program_hex.trim()).context("--program-hex is not valid hex")?;
            let keypair = kascov_labkit::load_or_create_key(&cli.key, false)?;
            let client = kascov_labkit::connect(cli.rpc.as_deref()).await?;
            kascov_labkit::deploy(&client, &keypair, &program, value).await.map(|_| ())
        }
        Command::Spend { ref program_hex, ref entrypoint, ref covenant, ref to, compute_budget, dry_run } => {
            let program = hex::decode(program_hex.trim()).context("--program-hex is not valid hex")?;
            let keypair = kascov_labkit::load_or_create_key(&cli.key, false)?;
            let client = kascov_labkit::connect(cli.rpc.as_deref()).await?;
            kascov_labkit::spend(
                &client, &keypair, &program, entrypoint, covenant.as_deref(), to.as_deref(), compute_budget, dry_run,
            )
            .await
        }
        Command::ContractDemo { value } => {
            let keypair = kascov_labkit::load_or_create_key(&cli.key, true)?;
            let client = kascov_labkit::connect(cli.rpc.as_deref()).await?;
            kascov_labkit::contract_demo(&client, &keypair, &cli.key, value).await
        }
        Command::SettleEscrow { ref program_hex, ref release_to, ref covenant, dry_run } => {
            let program = hex::decode(program_hex.trim()).context("--program-hex is not valid hex")?;
            let keypair = kascov_labkit::load_or_create_key(&cli.key, false)?;
            let client = kascov_labkit::connect(cli.rpc.as_deref()).await?;
            kascov_labkit::settle_escrow(&client, &keypair, &program, release_to, covenant.as_deref(), dry_run).await
        }
        Command::EscrowDemo { value } => {
            let keypair = kascov_labkit::load_or_create_key(&cli.key, true)?;
            let client = kascov_labkit::connect(cli.rpc.as_deref()).await?;
            kascov_labkit::escrow_demo(&client, &keypair, value).await
        }
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
