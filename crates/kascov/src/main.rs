use std::collections::{HashSet, VecDeque};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures::stream::{FuturesUnordered, StreamExt};
use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use kascov_core::detect::{covenant_sightings, CovenantSighting};
use kascov_core::node::NodeHandle;
use kascov_core::store::Store;
use kascov_core::{BlockHash, CovenantId, Network};

#[derive(Parser)]
#[command(name = "kascov", version, about = "Kaspa covenant explorer (Toccata / KIP-20)")]
struct Cli {
    /// wRPC (borsh) node url, e.g. ws://127.0.0.1:17210. Defaults to the public resolver.
    #[arg(long, global = true)]
    rpc: Option<String>,

    /// Network: mainnet | testnet-10
    #[arg(long, global = true, default_value = "mainnet")]
    network: Network,

    /// Emit JSON instead of tables
    #[arg(long, global = true)]
    json: bool,

    /// Index database path (default: ~/.kascov/<network>.db)
    #[arg(long, global = true)]
    db: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan the most recent blocks for covenant-bound outputs (no database).
    Scan {
        /// How many recent blocks to walk (backwards from the sink)
        #[arg(long, default_value_t = 200)]
        last: usize,
    },
    /// Build or update the covenant index by following the virtual chain.
    Sync {
        /// Chain block hash to start from (fresh index only; default: current sink)
        #[arg(long)]
        from: Option<BlockHash>,
        /// Keep running, syncing continuously
        #[arg(long)]
        follow: bool,
    },
    /// List indexed covenants.
    List {
        #[arg(long, default_value_t = 50)]
        limit: u64,
    },
    /// Show one covenant: summary, live state UTXOs.
    Show {
        covenant_id: CovenantId,
        /// Disassemble the state script instead of printing raw hex
        #[arg(long)]
        decode: bool,
    },
    /// Print a covenant's full lineage (genesis → tip).
    Trace { covenant_id: CovenantId },
    /// Follow the chain live and print covenant events as they are accepted.
    Watch,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info".into()),
    ).with_writer(std::io::stderr).init();

    let cli = Cli::parse();
    match cli.command {
        Command::Scan { last } => scan(&cli, last).await,
        Command::Sync { from, follow } => sync(&cli, from, follow, false).await,
        Command::List { limit } => list(&cli, limit),
        Command::Show { covenant_id, decode } => show(&cli, covenant_id, decode),
        Command::Trace { covenant_id } => trace(&cli, covenant_id),
        Command::Watch => sync(&cli, None, true, true).await,
    }
}

fn db_path(cli: &Cli) -> std::path::PathBuf {
    cli.db.clone().unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        std::path::PathBuf::from(home).join(".kascov").join(format!("{}.db", cli.network))
    })
}

fn open_store(cli: &Cli) -> Result<Store> {
    Ok(Store::open(&db_path(cli), cli.network)?)
}

async fn sync(cli: &Cli, from: Option<BlockHash>, follow: bool, watch: bool) -> Result<()> {
    use kascov_core::sync::SyncUpdate;
    let node = NodeHandle::connect(cli.network, cli.rpc.as_deref())
        .await
        .context("failed to connect to node")?;
    let mut store = open_store(cli)?;
    let json = cli.json;
    loop {
        let stats = kascov_core::sync::sync_once(&node, &mut store, from, |update| match update {
            SyncUpdate::Progress(s) if !watch => {
                eprintln!("… {} chain blocks, {} covenant events", s.chain_blocks, s.events);
            }
            SyncUpdate::Progress(_) => {}
            SyncUpdate::Reorg { rolled_back } => {
                if json {
                    println!("{}", serde_json::json!({"type": "reorg", "rolled_back": rolled_back}));
                } else {
                    println!("REORG      rolled back {rolled_back} chain blocks");
                }
            }
            SyncUpdate::Event { covenant_id, kind, txid, accepting_daa } => {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "type": "event", "kind": kind, "covenant_id": covenant_id,
                            "txid": txid, "accepting_daa": accepting_daa,
                        })
                    );
                } else {
                    println!("{:<10} {covenant_id}  tx {txid}  @ DAA {accepting_daa}", kind.as_str().to_uppercase());
                }
            }
        })
        .await?;
        if !follow {
            eprintln!(
                "synced: {} chain blocks processed, {} covenant events{}",
                stats.chain_blocks,
                stats.events,
                if stats.reorged_out > 0 { format!(", {} reorged out", stats.reorged_out) } else { String::new() }
            );
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    Ok(())
}

fn list(cli: &Cli, limit: u64) -> Result<()> {
    let store = open_store(cli)?;
    let covenants = store.list(limit)?;
    if cli.json {
        for c in &covenants {
            println!("{}", serde_json::to_string(c)?);
        }
        return Ok(());
    }
    if covenants.is_empty() {
        println!("no covenants indexed yet — run `kascov sync` first");
        return Ok(());
    }
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED).set_header([
        "COVENANT ID", "STATUS", "EVENTS", "LIVE UTXOS", "VALUE (KAS)", "LAST DAA", "LINEAGE",
    ]);
    for c in &covenants {
        table.add_row([
            abbrev(&c.covenant_id.to_string()),
            if c.live_utxos > 0 { "active" } else { "burned" }.to_string(),
            c.event_count.to_string(),
            c.live_utxos.to_string(),
            format!("{:.8}", c.live_value as f64 / 100_000_000.0),
            c.last_activity_daa.to_string(),
            if c.lineage_complete { "complete" } else { "truncated" }.to_string(),
        ]);
    }
    println!("{table}");
    println!("{} covenants", covenants.len());
    Ok(())
}

fn show(cli: &Cli, covenant_id: CovenantId, decode: bool) -> Result<()> {
    let store = open_store(cli)?;
    let Some(summary) = store.summary(&covenant_id)? else {
        anyhow::bail!("covenant {covenant_id} not in index");
    };
    let utxos = store.utxos(&covenant_id, true)?;
    let registry = kascov_decode::Registry::default();
    if cli.json {
        let decoded: Vec<_> = decode
            .then(|| utxos.iter().map(|u| registry.decode(u.spk_version, &u.spk_script)).collect())
            .unwrap_or_default();
        println!(
            "{}",
            serde_json::json!({ "summary": summary, "live_utxos": utxos, "decoded": decoded })
        );
        return Ok(());
    }
    println!("Covenant  {}", summary.covenant_id);
    println!(
        "Status    {} ({} events, lineage {})",
        if summary.live_utxos > 0 { "active" } else { "burned" },
        summary.event_count,
        if summary.lineage_complete { "complete" } else { "truncated — first seen mid-life" },
    );
    if let (Some(txid), Some(daa)) = (summary.genesis_txid, summary.genesis_daa) {
        println!("Genesis   tx {txid} @ DAA {daa}");
    }
    for utxo in &utxos {
        println!(
            "State     {} — {:.8} KAS (spk v{}, {} bytes) @ DAA {}",
            utxo.outpoint,
            utxo.value as f64 / 100_000_000.0,
            utxo.spk_version,
            utxo.spk_script.len(),
            utxo.created_daa,
        );
        if decode {
            let decoded = registry.decode(utxo.spk_version, &utxo.spk_script);
            for instruction in &decoded.instructions {
                println!("    {:>4}  {}", format!("{:04x}", instruction.offset), instruction);
            }
            if decoded.truncated {
                println!("    [script truncated / malformed tail]");
            }
            if decoded.uses_covenant_ops || decoded.uses_zk_ops {
                println!(
                    "    uses: {}{}",
                    if decoded.uses_covenant_ops { "covenant-ops " } else { "" },
                    if decoded.uses_zk_ops { "zk-ops" } else { "" },
                );
            }
        } else {
            println!("  script  {}", hex::encode(&utxo.spk_script));
        }
    }
    Ok(())
}

fn trace(cli: &Cli, covenant_id: CovenantId) -> Result<()> {
    let store = open_store(cli)?;
    let events = store.events(&covenant_id)?;
    if events.is_empty() {
        anyhow::bail!("covenant {covenant_id} not in index");
    }
    if cli.json {
        for event in &events {
            println!("{}", serde_json::to_string(event)?);
        }
        return Ok(());
    }
    let truncated = store.summary(&covenant_id)?.map(|s| !s.lineage_complete).unwrap_or(false);
    if truncated {
        println!("[history truncated — covenant first seen mid-life]");
    }
    for event in &events {
        println!(
            "#{:03} {:<10} tx {}  @ DAA {}  (chain block {})",
            event.seq,
            event.kind,
            event.txid,
            event.accepting_daa,
            abbrev(&event.accepting_block.to_string()),
        );
    }
    Ok(())
}

async fn scan(cli: &Cli, last: usize) -> Result<()> {
    let node = NodeHandle::connect(cli.network, cli.rpc.as_deref())
        .await
        .context("failed to connect to node")?;
    let info = node.server_info().await?;
    eprintln!("connected: kaspad {} on {} (synced: {})", info.version, info.network, info.is_synced);

    let dag = node.dag_info().await?;
    eprintln!("sink {} @ DAA {} — walking {} blocks backwards", dag.sink, dag.virtual_daa_score, last);

    // BFS backwards over direct parents from the sink until `last` blocks seen,
    // fetching blocks concurrently.
    const CONCURRENCY: usize = 24;
    let node = &node;
    let mut queue = VecDeque::from([dag.sink]);
    let mut seen: HashSet<_> = [dag.sink].into();
    let mut in_flight = FuturesUnordered::new();
    let mut visited = 0usize;
    let mut sightings: Vec<CovenantSighting> = Vec::new();

    loop {
        while in_flight.len() < CONCURRENCY && visited + in_flight.len() < last {
            let Some(hash) = queue.pop_front() else { break };
            in_flight.push(async move { (hash, node.block_with_txs(hash).await) });
        }
        let Some((hash, result)) = in_flight.next().await else { break };
        let block = match result {
            Ok(block) => block,
            // Parents below the pruning point (or not yet synced) are simply skipped.
            Err(err) => {
                tracing::debug!("skipping block {hash}: {err}");
                continue;
            }
        };
        visited += 1;
        if visited % 1000 == 0 {
            eprintln!("… {visited}/{last} blocks scanned, {} covenant outputs so far", sightings.len());
        }
        sightings.extend(covenant_sightings(&block));
        for parent in block.parents {
            if seen.insert(parent) {
                queue.push_back(parent);
            }
        }
    }

    sightings.sort_by(|a, b| b.daa_score.cmp(&a.daa_score));

    if cli.json {
        for sighting in &sightings {
            println!("{}", serde_json::to_string(sighting)?);
        }
    } else if sightings.is_empty() {
        println!("no covenant outputs found in the last {visited} blocks");
    } else {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL_CONDENSED).set_header([
            "COVENANT ID", "OUTPOINT", "VALUE (KAS)", "AUTH INPUT", "DAA",
        ]);
        for s in &sightings {
            table.add_row([
                abbrev(&s.covenant_id.to_string()),
                abbrev(&s.outpoint.to_string()),
                format!("{:.8}", s.value as f64 / 100_000_000.0),
                s.authorizing_input.to_string(),
                s.daa_score.to_string(),
            ]);
        }
        println!("{table}");
        let unique: HashSet<_> = sightings.iter().map(|s| s.covenant_id).collect();
        println!(
            "{} covenant outputs across {} distinct covenants (scanned {visited} blocks)",
            sightings.len(),
            unique.len()
        );
    }
    Ok(())
}

fn abbrev(s: &str) -> String {
    if s.len() > 20 {
        format!("{}…{}", &s[..8], &s[s.len() - 8..])
    } else {
        s.to_string()
    }
}
