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
    /// kascov.io/decode ("make this yours").
    Deploy {
        /// Compiled contract hex (the generator's "compiled" block)
        #[arg(long)]
        program_hex: String,
        /// Sompi the newborn coin holds (default 10 TKAS)
        #[arg(long, default_value_t = 1_000_000_000)]
        value: u64,
    },
    /// Spend a deployed contract coin — reveals the program on-chain, so
    /// kascov shows it as your named contract, permanently. Pure-signature
    /// entrypoints (Mecenas.reclaim, LastWill.cold/inherit) need only a
    /// signature from the matching key and send the funds to --to.
    /// Output-constrained entrypoints (Mecenas.receive, LastWill.refresh)
    /// build the outputs the contract's introspection demands — the pledge
    /// to the recipient / the timer reset — with a second plain input
    /// paying the real network fee (like settle-escrow).
    Spend {
        /// Compiled contract hex of the deployed coin
        #[arg(long)]
        program_hex: String,
        /// Which entrypoint to satisfy
        /// (reclaim | cold | inherit | receive | refresh)
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
    /// One-shot index surgery: merge the CANONICAL covenant history of a DAA
    /// window the production index skipped (deep-reorg wedge → sink reset)
    /// into an offline COPY of the database, by walking the node's virtual
    /// chain from its own pruning point. Never point this at the live DB —
    /// run it on a copy, verify, then upload/restore. Idempotent: a second
    /// run over the same window is a no-op.
    RecoverGap {
        /// Directory holding <network>.db — a COPY of production, never live
        #[arg(long)]
        db_dir: std::path::PathBuf,
        /// Network whose index to heal
        #[arg(long, default_value = "testnet-10")]
        network: String,
        /// Override the gap's lower bound (the highest indexed accepting DAA
        /// below the gap). Default: auto-detected from the DAA discontinuity.
        #[arg(long, requires = "to_daa")]
        from_daa: Option<u64>,
        /// Override the gap's upper bound (the lowest indexed accepting DAA
        /// above the gap). Given together with --from-daa.
        #[arg(long, requires = "from_daa")]
        to_daa: Option<u64>,
        /// Smallest DAA discontinuity auto-detection may call a gap
        #[arg(long, default_value_t = 100_000)]
        min_gap_daa: u64,
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
        Command::RecoverGap { ref db_dir, ref network, from_daa, to_daa, min_gap_daa } => {
            recover_gap(cli.rpc.as_deref(), db_dir, network, from_daa, to_daa, min_gap_daa).await
        }
    }
}

/// Drive kascov-core's gap recovery against an offline DB copy. All the real
/// logic lives in `kascov_core::sync::recover_gap` (walk + capture +
/// reconcile) and `Store::finalize_gap_recovery` (re-sequence + summaries +
/// token re-derivation + the honest meta note).
async fn recover_gap(
    rpc: Option<&str>,
    db_dir: &std::path::Path,
    network: &str,
    from_daa: Option<u64>,
    to_daa: Option<u64>,
    min_gap_daa: u64,
) -> Result<()> {
    use kascov_core::sync::{recover_gap, GapRecoveryOptions};

    let network: kascov_core::Network =
        network.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
    let db = db_dir.join(format!("{network}.db"));
    anyhow::ensure!(db.exists(), "no index at {} — copy the production DB there first", db.display());
    let mut store = kascov_core::store::Store::open(&db, network)
        .with_context(|| format!("open {}", db.display()))?;

    let opts = GapRecoveryOptions { from_daa, to_daa, min_gap_daa };
    // The walk is ~1000 batched RPC calls over ~15 min; public resolver nodes
    // routinely drop a long-lived WebSocket. recover_gap persists a walk cursor
    // and dedups every merge, so a dropped connection just means: reconnect
    // (fresh node) and resume where we left off. Retry generously.
    const MAX_ATTEMPTS: u32 = 40;
    let report = {
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let node = match kascov_core::node::NodeHandle::connect(network, rpc).await {
                Ok(n) => n,
                Err(e) if attempt < MAX_ATTEMPTS => {
                    eprintln!("recover-gap: node connect failed (attempt {attempt}): {e} — reconnecting");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
                Err(e) => return Err(anyhow::anyhow!(e)).context("node connect"),
            };
            match recover_gap(&node, &mut store, &opts, |line| eprintln!("recover-gap: {line}")).await {
                Ok(r) => break r,
                Err(e) if attempt < MAX_ATTEMPTS => {
                    eprintln!("recover-gap: pass failed (attempt {attempt}): {e} — reconnecting and resuming from saved cursor");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
                Err(e) => return Err(anyhow::anyhow!(e)).context("gap recovery (all attempts exhausted)"),
            }
        }
    };

    if report.already_recovered {
        println!(
            "no-op: gap [{}, {}] already recovered (meta gap_recoveries)",
            report.gap_lo, report.gap_hi
        );
        return Ok(());
    }
    println!("recovered gap [{}, {}] on {network}:", report.gap_lo, report.gap_hi);
    println!("  chain blocks walked      {}", report.chain_blocks_walked);
    println!("  blocks captured (in-gap) {}", report.blocks_captured);
    println!("  events merged            {}", report.events_added);
    println!("  state cells merged       {}", report.utxos_added);
    println!("  spends repaired          {}", report.spends_repaired);
    println!("  covenants refreshed      {}", report.covenants_refreshed);
    println!("  covenants re-sequenced   {}", report.covenants_resequenced);
    println!("  tokens re-derived        {}", report.tokens_rederived);
    if report.residual_txs > 0 {
        println!(
            "  RESIDUAL (unrecoverable) {} txs / {} blocks, DAA {}–{} — bodies pruned from this node",
            report.residual_txs, report.residual_blocks, report.residual_daa_lo, report.residual_daa_hi
        );
        println!("  → re-run recover-gap (lands on another node; merges dedup) to shrink the residual");
    }
    println!("verify, then hand {} to ops for upload/restore", db.display());
    Ok(())
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
    1. https://kascov.io/decode  → "make a Mecenas / Escrow / LastWill"
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
