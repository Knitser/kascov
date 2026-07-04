use std::collections::{HashSet, VecDeque};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures::stream::{FuturesUnordered, StreamExt};
use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use kascov_core::detect::{covenant_sightings, CovenantSighting};
use kascov_core::node::NodeHandle;
use kascov_core::store::Store;
use kascov_core::{BlockHash, CovenantId, Network, TxId};

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
    /// Export the index as a JSON snapshot for the web dashboard.
    Export {
        /// Output file (default: web/data/<network>.json)
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// Cap on events exported per covenant
        #[arg(long, default_value_t = 500)]
        max_events: u64,
    },
    /// Run the always-on worker: follow the chain for each network and serve
    /// fresh JSON snapshots over HTTP (for Cloud Run behind a CDN).
    Serve {
        #[arg(long, default_value = "0.0.0.0:8080")]
        listen: String,
        /// Comma-separated networks to follow and serve
        #[arg(long, default_value = "testnet-10,mainnet")]
        networks: String,
        /// Directory holding <network>.db files (default: ~/.kascov)
        #[arg(long)]
        db_dir: Option<std::path::PathBuf>,
        #[arg(long, default_value_t = 500)]
        max_events: u64,
    },
    /// Write a consistent copy of the index database (safe while syncing).
    Backup {
        #[arg(long)]
        out: std::path::PathBuf,
    },
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
        Command::Export { ref out, max_events } => export(&cli, out.clone(), max_events),
        Command::Serve { ref listen, ref networks, ref db_dir, max_events } => {
            serve(&cli, listen.clone(), networks.clone(), db_dir.clone(), max_events).await
        }
        Command::Backup { ref out } => {
            let store = open_store(&cli)?;
            store.backup_to(out)?;
            eprintln!("backed up {} index to {}", cli.network, out.display());
            Ok(())
        }
    }
}

fn export(cli: &Cli, out: Option<std::path::PathBuf>, max_events: u64) -> Result<()> {
    let store = open_store(cli)?;
    let snapshot = build_snapshot(&store, cli.network, max_events)?;
    let covenants = snapshot["stats"]["covenants"].as_u64().unwrap_or(0);
    let events = snapshot["stats"]["events"].as_u64().unwrap_or(0);

    let out = out.unwrap_or_else(|| std::path::PathBuf::from(format!("web/data/{}.json", cli.network)));
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out, serde_json::to_string(&snapshot)?)?;

    let live_out = live_path(&out);
    let live = build_live_snapshot(&store, cli.network)?;
    std::fs::write(&live_out, serde_json::to_string(&live)?)?;

    eprintln!(
        "exported {covenants} covenants ({events} events) to {} (+ {})",
        out.display(),
        live_out.display()
    );
    Ok(())
}

/// `web/data/testnet-10.json` → `web/data/testnet-10-live.json`
fn live_path(out: &std::path::Path) -> std::path::PathBuf {
    let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("snapshot");
    out.with_file_name(format!("{stem}-live.json"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Whole-index stats straight from SQL aggregates — the old path materialized
/// every covenant summary (40k+ rows with correlated subqueries) just to
/// count them, every few seconds, which is what OOM-looped the worker.
fn stats_json(store: &Store) -> Result<serde_json::Value> {
    let s = store.stats()?;
    Ok(serde_json::json!({
        "covenants": s.covenants,
        "active": s.active,
        "burned": s.burned,
        "events": s.total_events,
        "live_value": s.live_value,
        "last_activity_daa": s.last_activity_daa,
    }))
}

/// The small fast-changing feed the web app polls: stats + tip + newest
/// events. Cheap to build and to fetch; the full snapshot is only refetched
/// when this reports a change.
const LIVE_EVENTS: u64 = 150;

fn build_live_snapshot(store: &Store, network: kascov_core::Network) -> Result<serde_json::Value> {
    let tip = store.tip()?;
    Ok(serde_json::json!({
        "network": network.to_string(),
        "generated_at_ms": now_ms(),
        "tip_daa": tip.map(|t| t.0),
        "tip_at_ms": tip.map(|t| t.1),
        "stats": stats_json(store)?,
        "recent_events": store.recent_events(LIVE_EVENTS)?,
    }))
}

/// The explorer grid: stats + one summary row per covenant, no timelines and
/// no scripts. This is what the web app loads up front; per-coin detail comes
/// from `/data/{network}/c/{id}.json` on demand. At 42k covenants the old
/// all-in-one snapshot passed 1 GiB in flight — this stays a few MB.
fn build_grid_snapshot(store: &Store, network: kascov_core::Network) -> Result<serde_json::Value> {
    let covenants = store.list(u64::MAX)?;
    let born = store.born_values()?;
    let tip = store.tip()?;
    let rows: Vec<_> = covenants
        .iter()
        .map(|c| {
            serde_json::json!({
                "covenant_id": c.covenant_id,
                "status": if c.live_utxos > 0 { "active" } else { "burned" },
                "genesis_daa": c.genesis_daa,
                "lineage_complete": c.lineage_complete,
                "event_count": c.event_count,
                "last_activity_daa": c.last_activity_daa,
                "live_utxos": c.live_utxos,
                "live_value": c.live_value,
                "born_value": born.get(&c.covenant_id).copied().unwrap_or(0),
            })
        })
        .collect();
    Ok(serde_json::json!({
        "network": network.to_string(),
        "grid": true,
        "generated_at_ms": now_ms(),
        "tip_daa": tip.map(|t| t.0),
        "tip_at_ms": tip.map(|t| t.1),
        "stats": stats_json(store)?,
        "covenants": rows,
    }))
}

/// One covenant's full story: every event and every UTXO, scripts decoded,
/// spend-time reveals verified. Small (one coin), built on demand.
fn build_covenant_detail(
    store: &Store,
    registry: &kascov_decode::Registry,
    network: kascov_core::Network,
    summary: &kascov_core::store::CovenantSummary,
    max_events: u64,
) -> Result<serde_json::Value> {
    let mut detail = covenant_json(store, registry, summary, max_events)?;
    let tip = store.tip()?;
    let obj = detail.as_object_mut().expect("covenant json is an object");
    obj.insert("network".into(), serde_json::json!(network.to_string()));
    obj.insert("generated_at_ms".into(), serde_json::json!(now_ms()));
    obj.insert("tip_daa".into(), serde_json::json!(tip.map(|t| t.0)));
    obj.insert("tip_at_ms".into(), serde_json::json!(tip.map(|t| t.1)));
    Ok(detail)
}

/// One covenant as JSON: summary fields + timeline + UTXOs with decoded
/// scripts and spend-time reveals. Shared by the full export and the
/// on-demand detail endpoint.
fn covenant_json(
    store: &Store,
    registry: &kascov_decode::Registry,
    summary: &kascov_core::store::CovenantSummary,
    max_events: u64,
) -> Result<serde_json::Value> {
    let events = store.events(&summary.covenant_id)?;
    let truncated_events = events.len() as u64 > max_events;
    let utxos: Vec<_> = store
        .utxos(&summary.covenant_id, false)?
        .into_iter()
        .map(|utxo| {
            let decoded = registry.decode(utxo.spk_version, &utxo.spk_script);
            let mut json = serde_json::json!({
                "outpoint": utxo.outpoint.to_string(),
                "value": utxo.value,
                "created_daa": utxo.created_daa,
                "live": utxo.live,
                "script_hex": hex::encode(&utxo.spk_script),
                "script_asm": decoded.instructions.iter().map(|i| i.to_string()).collect::<Vec<_>>(),
                "uses_covenant_ops": decoded.uses_covenant_ops,
                "uses_zk_ops": decoded.uses_zk_ops,
            });
            if let Some(template) = decoded.template {
                json["template"] = serde_json::json!(template);
                json["state_fields"] = serde_json::json!(decoded.fields);
            }
            if let Some(spent_txid) = utxo.spent_txid {
                json["spent_txid"] = serde_json::json!(spent_txid);
            }
            if let Some(budget) = utxo.spent_budget {
                json["spent_budget"] = serde_json::json!(budget);
            }
            // Spend-time decoding: a P2SH spend reveals the program that ran.
            if let Some(sig) = &utxo.spent_sig {
                if let Some(redeem) = kascov_decode::p2sh_reveal(&utxo.spk_script, sig) {
                    let d = registry.decode(utxo.spk_version, &redeem);
                    json["revealed_hex"] = serde_json::json!(hex::encode(&redeem));
                    json["revealed_asm"] = serde_json::json!(
                        d.instructions.iter().map(|i| i.to_string()).collect::<Vec<_>>()
                    );
                    json["revealed_uses_covenant_ops"] = serde_json::json!(d.uses_covenant_ops);
                    json["revealed_uses_zk_ops"] = serde_json::json!(d.uses_zk_ops);
                    if let Some(template) = d.template {
                        json["revealed_template"] = serde_json::json!(template);
                        json["revealed_fields"] = serde_json::json!(d.fields);
                    }
                } else if sig.len() <= 520 {
                    json["sig_hex"] = serde_json::json!(hex::encode(sig));
                } else {
                    json["sig_len"] = serde_json::json!(sig.len());
                }
            }
            json
        })
        .collect();
    Ok(serde_json::json!({
        "covenant_id": summary.covenant_id,
        "status": if summary.live_utxos > 0 { "active" } else { "burned" },
        "genesis_txid": summary.genesis_txid,
        "genesis_daa": summary.genesis_daa,
        "lineage_complete": summary.lineage_complete,
        "event_count": summary.event_count,
        "last_activity_daa": summary.last_activity_daa,
        "live_utxos": summary.live_utxos,
        "live_value": summary.live_value,
        "events": events.iter().take(max_events as usize).map(|e| {
            let mut v = serde_json::to_value(e).expect("event serializes");
            // based-app payloads can be large; the snapshot inlines small ones only
            if let Some(p) = &e.payload {
                if p.len() > 512 {
                    v.as_object_mut().expect("event object").remove("payload");
                    v["payload_len"] = serde_json::json!(p.len());
                }
            }
            v
        }).collect::<Vec<_>>(),
        "events_truncated": truncated_events,
        "utxos": utxos,
    }))
}

fn build_snapshot(store: &Store, network: kascov_core::Network, max_events: u64) -> Result<serde_json::Value> {
    let registry = kascov_decode::Registry::default();
    let covenants = store.list(u64::MAX)?;

    let mut exported = Vec::with_capacity(covenants.len());
    for summary in &covenants {
        exported.push(covenant_json(store, &registry, summary, max_events)?);
    }

    let tip = store.tip()?;
    let snapshot = serde_json::json!({
        "network": network.to_string(),
        "generated_at_ms": now_ms(),
        "tip_daa": tip.map(|t| t.0),
        "tip_at_ms": tip.map(|t| t.1),
        "stats": stats_json(store)?,
        "covenants": exported,
    });
    Ok(snapshot)
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
    let mut store = open_store(cli)?;
    loop {
        let node = match NodeHandle::connect(cli.network, cli.rpc.as_deref()).await {
            Ok(node) => node,
            Err(err) if follow => {
                eprintln!("connect failed ({err}), retrying in 10s…");
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                continue;
            }
            Err(err) => return Err(err).context("failed to connect to node"),
        };
        match sync_session(cli, &node, &mut store, from, follow, watch).await {
            Ok(()) => return Ok(()),
            Err(err) if follow => {
                eprintln!("sync interrupted ({err}), reconnecting in 5s…");
                if recover_wedged_cursor(&node, &mut store, cli.network).await {
                    eprintln!("cursor restarted at the current sink (testnet reset recovery)");
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            Err(err) => return Err(err),
        }
    }
}

async fn sync_session(
    cli: &Cli,
    node: &NodeHandle,
    store: &mut kascov_core::store::Store,
    from: Option<BlockHash>,
    follow: bool,
    watch: bool,
) -> Result<()> {
    use kascov_core::sync::SyncUpdate;
    let json = cli.json;
    loop {
        let stats = kascov_core::sync::sync_once(node, store, from, |update| match update {
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
    // --decode includes spent states: that's where the revealed programs live
    let utxos = store.utxos(&covenant_id, !decode)?;
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
            "{}     {} — {:.8} KAS (spk v{}, {} bytes) @ DAA {}{}",
            if utxo.live { "State" } else { "Spent" },
            utxo.outpoint,
            utxo.value as f64 / 100_000_000.0,
            utxo.spk_version,
            utxo.spk_script.len(),
            utxo.created_daa,
            utxo.spent_budget.map(|b| format!("  [spent with budget {b}]")).unwrap_or_default(),
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
            if let Some(sig) = &utxo.spent_sig {
                if let Some(redeem) = kascov_decode::p2sh_reveal(&utxo.spk_script, sig) {
                    println!("    revealed at spend (tx {}):", utxo.spent_txid.map(|t| t.to_string()).unwrap_or_default());
                    let d = registry.decode(utxo.spk_version, &redeem);
                    for instruction in &d.instructions {
                        println!("      {:>4}  {}", format!("{:04x}", instruction.offset), instruction);
                    }
                }
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

    // Spend-time reveals, keyed by the spending tx: the data pushes of the
    // revealed P2SH program are the covenant's state payload.
    let mut reveal_by_tx: std::collections::HashMap<TxId, Vec<Vec<u8>>> = Default::default();
    for utxo in store.utxos(&covenant_id, false)? {
        let (Some(spent_txid), Some(sig)) = (utxo.spent_txid, &utxo.spent_sig) else { continue };
        let Some(redeem) = kascov_decode::p2sh_reveal(&utxo.spk_script, sig) else { continue };
        let (instructions, _) = kascov_decode::disasm::disassemble(&redeem);
        let pushes: Vec<Vec<u8>> = instructions.into_iter().filter_map(|i| i.data).collect();
        reveal_by_tx.entry(spent_txid).or_insert(pushes);
    }

    let fmt_push = |bytes: &[u8]| {
        let hex = hex::encode(bytes);
        if hex.len() > 40 { format!("{}…{} ({}B)", &hex[..16], &hex[hex.len() - 8..], bytes.len()) } else { hex }
    };
    let mut prev_payload: Option<Vec<Vec<u8>>> = None;
    for event in &events {
        println!(
            "#{:03} {:<10} tx {}  @ DAA {}  (chain block {})",
            event.seq,
            event.kind,
            event.txid,
            event.accepting_daa,
            abbrev(&event.accepting_block.to_string()),
        );
        if let Some(p) = &event.payload {
            println!("      tx payload {}", fmt_push(p));
        }
        if let Some(payload) = reveal_by_tx.get(&event.txid) {
            match &prev_payload {
                Some(prev) if prev.len() == payload.len() => {
                    for (i, (a, b)) in prev.iter().zip(payload).enumerate() {
                        if a != b {
                            println!("      payload[{i}] Δ {} → {}", fmt_push(a), fmt_push(b));
                        }
                    }
                    if prev == payload {
                        println!("      payload unchanged ({} pushes)", payload.len());
                    }
                }
                _ => {
                    for (i, p) in payload.iter().enumerate() {
                        println!("      payload[{i}] = {}", fmt_push(p));
                    }
                }
            }
            prev_payload = Some(payload.clone());
        }
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

/// A cached response body, pre-compressed once at build time so a popular
/// endpoint never gzips the same megabytes per request.
struct CachedBody {
    raw: bytes::Bytes,
    gzip: bytes::Bytes,
}

impl CachedBody {
    fn new(json: String) -> Self {
        use flate2::{write::GzEncoder, Compression};
        use std::io::Write;
        let raw = bytes::Bytes::from(json);
        let mut enc = GzEncoder::new(Vec::with_capacity(raw.len() / 4), Compression::new(6));
        // write_all + finish on a Vec cannot fail
        let _ = enc.write_all(&raw);
        let gzip = bytes::Bytes::from(enc.finish().unwrap_or_default());
        Self { raw, gzip }
    }
}

struct ServeState {
    base_dir: std::path::PathBuf,
    networks: Vec<Network>,
    max_events: u64,
    cache: tokio::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, std::sync::Arc<CachedBody>)>>,
    /// Per-key build locks: concurrent cold misses on the SAME key share one
    /// rebuild instead of stampeding (at 42k covenants, N parallel grid
    /// builds OOM-killed the container). Different keys still build in
    /// parallel, so one slow network can't starve the others.
    build_locks: tokio::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>,
}

async fn serve(
    cli: &Cli,
    listen: String,
    networks: String,
    db_dir: Option<std::path::PathBuf>,
    max_events: u64,
) -> Result<()> {
    use axum::routing::get;

    let networks: Vec<Network> = networks
        .split(',')
        .map(|s| s.trim().parse())
        .collect::<std::result::Result<_, _>>()
        .map_err(|e: kascov_core::Error| anyhow::anyhow!("{e}"))?;
    let base_dir = db_dir.unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        std::path::PathBuf::from(home).join(".kascov")
    });
    std::fs::create_dir_all(&base_dir)?;

    for &network in &networks {
        let db = base_dir.join(format!("{network}.db"));
        let rpc = cli.rpc.clone();
        tokio::spawn(follow_forever(network, rpc, db));
    }

    let state = std::sync::Arc::new(ServeState {
        base_dir,
        networks,
        max_events,
        cache: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        build_locks: tokio::sync::Mutex::new(std::collections::HashMap::new()),
    });
    let app = axum::Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/data/{file}", get(data_handler))
        .route("/data/{network}/c/{id}", get(detail_handler))
        .route("/data/{network}/tx/{txid}", get(tx_handler))
        // compresses the small dynamic responses; the big cached bodies are
        // pre-gzipped (Content-Encoding already set, so this layer skips them)
        .layer(tower_http::compression::CompressionLayer::new())
        .with_state(state);

    eprintln!("kascov worker listening on {listen}");
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// After repeated sync failures, check for the testnet-reset signature: the
/// node answers fine but our stored cursor block no longer exists there.
/// Recovery restarts the cursor at the current sink — indexed history stays,
/// and the gap is real (the old chain is gone), not an artifact.
async fn recover_wedged_cursor(node: &NodeHandle, store: &mut Store, network: Network) -> bool {
    let Ok(Some(cursor)) = store.cursor() else { return false };
    let Ok(dag) = node.dag_info().await else { return false };
    if node.block_with_txs(cursor).await.is_ok() {
        return false; // cursor exists — the failures are something else
    }
    tracing::error!(
        "{network}: cursor {cursor} is unknown to a healthy node (testnet reset?) — restarting from sink {}",
        dag.sink
    );
    store.reset_cursor(dag.sink).is_ok()
}

/// Follow a network's virtual chain forever, reconnecting on any failure.
async fn follow_forever(network: Network, rpc: Option<String>, db: std::path::PathBuf) {
    use kascov_core::sync::SyncUpdate;
    loop {
        let mut store = match kascov_core::store::Store::open(&db, network) {
            Ok(store) => store,
            Err(err) => {
                tracing::error!("{network}: cannot open store: {err}");
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                continue;
            }
        };
        let node = match NodeHandle::connect(network, rpc.as_deref()).await {
            Ok(node) => node,
            Err(err) => {
                tracing::warn!("{network}: connect failed ({err}), retrying in 10s");
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                continue;
            }
        };
        tracing::info!("{network}: following the chain");
        let mut consecutive_errors = 0u32;
        loop {
            let result = kascov_core::sync::sync_once(&node, &mut store, None, |update| {
                if let SyncUpdate::Event { covenant_id, kind, .. } = update {
                    tracing::info!("{network}: {} covenant {covenant_id}", kind.as_str());
                }
            })
            .await;
            match result {
                Ok(_) => {
                    consecutive_errors = 0;
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                Err(err) => {
                    consecutive_errors += 1;
                    tracing::warn!("{network}: sync interrupted ({err}), attempt {consecutive_errors}");
                    if consecutive_errors >= 3
                        && recover_wedged_cursor(&node, &mut store, network).await
                    {
                        consecutive_errors = 0;
                        continue;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    break;
                }
            }
        }
    }
}

/// Serve a cached JSON body, building it (single-flight per key) when stale.
/// `build` runs on the blocking pool against a fresh read-only store handle.
async fn serve_cached(
    state: &ServeState,
    key: String,
    ttl_secs: u64,
    cache_control: &'static str,
    gzip_ok: bool,
    build: impl FnOnce() -> Result<Option<String>> + Send + 'static,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let fresh_body = |cache: &std::collections::HashMap<String, (std::time::Instant, std::sync::Arc<CachedBody>)>| {
        cache
            .get(&key)
            .filter(|(at, _)| at.elapsed() < std::time::Duration::from_secs(ttl_secs))
            .map(|(_, body)| body.clone())
    };

    let mut body = { fresh_body(&*state.cache.lock().await) };
    if body.is_none() {
        // Single-flight: one build per key; latecomers wait, then re-check.
        let key_lock = {
            let mut locks = state.build_locks.lock().await;
            locks.entry(key.clone()).or_default().clone()
        };
        let _building = key_lock.lock().await;
        body = { fresh_body(&*state.cache.lock().await) };
        if body.is_none() {
            match tokio::task::spawn_blocking(build).await {
                Ok(Ok(Some(json))) => {
                    let built = std::sync::Arc::new(CachedBody::new(json));
                    let mut cache = state.cache.lock().await;
                    // Detail keys accumulate — drop expired entries before they
                    // become a slow leak (grid/live keys are refreshed in place).
                    if cache.len() > 2048 {
                        cache.retain(|_, (at, _)| at.elapsed() < std::time::Duration::from_secs(300));
                    }
                    cache.insert(key.clone(), (std::time::Instant::now(), built.clone()));
                    drop(cache);
                    let mut locks = state.build_locks.lock().await;
                    if locks.len() > 2048 {
                        locks.retain(|_, l| std::sync::Arc::strong_count(l) > 1);
                    }
                    body = Some(built);
                }
                Ok(Ok(None)) => {
                    return (StatusCode::NOT_FOUND, "not found").into_response();
                }
                Ok(Err(err)) => {
                    tracing::error!("{key}: build failed: {err}");
                    return (StatusCode::SERVICE_UNAVAILABLE, "snapshot unavailable").into_response();
                }
                Err(err) => {
                    tracing::error!("{key}: build task panicked: {err}");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
                }
            }
        }
    }
    let body = body.expect("cache hit or fresh build");

    let gzipped = gzip_ok && !body.gzip.is_empty();
    let bytes = if gzipped { body.gzip.clone() } else { body.raw.clone() };
    let mut resp = (
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, cache_control),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            (header::VARY, "Accept-Encoding"),
        ],
        bytes,
    )
        .into_response();
    if gzipped {
        resp.headers_mut().insert(header::CONTENT_ENCODING, axum::http::HeaderValue::from_static("gzip"));
    }
    resp
}

fn accepts_gzip(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(axum::http::header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("gzip"))
}

async fn data_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(file): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    let not_found = || (StatusCode::NOT_FOUND, "unknown network").into_response();
    let Some(name) = file.strip_suffix(".json") else { return not_found() };
    // '<network>.json' is the explorer grid (summaries only), and
    // '<network>-live.json' the small fast-changing feed. Full timelines live
    // at /data/<network>/c/<id>.json, one covenant at a time.
    let (net_name, live) = match name.strip_suffix("-live") {
        Some(base) => (base, true),
        None => (name, false),
    };
    let Ok(network) = net_name.parse::<Network>() else { return not_found() };
    if !state.networks.contains(&network) {
        return not_found();
    }

    let db = state.base_dir.join(format!("{network}.db"));
    let (ttl, cache_control) = if live {
        // s-maxage lets the hosting CDN absorb the polling herd; SWR keeps
        // pages responsive while the edge revalidates.
        (5, "public, max-age=5, s-maxage=10, stale-while-revalidate=30")
    } else {
        (20, "public, max-age=15, s-maxage=60, stale-while-revalidate=300")
    };
    serve_cached(&state, name.to_string(), ttl, cache_control, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        let snapshot = if live {
            build_live_snapshot(&store, network)?
        } else {
            build_grid_snapshot(&store, network)?
        };
        Ok(Some(serde_json::to_string(&snapshot)?))
    })
    .await
}

async fn detail_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, id)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    let Ok(network) = net_name.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    let id_hex = id.strip_suffix(".json").unwrap_or(&id);
    let Ok(covenant_id) = id_hex.parse::<kascov_core::CovenantId>() else {
        return (StatusCode::BAD_REQUEST, "bad covenant id").into_response();
    };
    if !state.networks.contains(&network) {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    }

    let db = state.base_dir.join(format!("{network}.db"));
    let max_events = state.max_events;
    let key = format!("{network}/c/{covenant_id}");
    let cc = "public, max-age=10, s-maxage=30, stale-while-revalidate=120";
    serve_cached(&state, key, 10, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        let registry = kascov_decode::Registry::default();
        match store.summary(&covenant_id)? {
            Some(summary) => Ok(Some(serde_json::to_string(&build_covenant_detail(
                &store, &registry, network, &summary, max_events,
            )?)?)),
            None => Ok(None),
        }
    })
    .await
}

async fn tx_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, txid)): axum::extract::Path<(String, String)>,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let Ok(network) = net_name.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    let tx_hex = txid.strip_suffix(".json").unwrap_or(&txid);
    let Ok(txid) = tx_hex.parse::<TxId>() else {
        return (StatusCode::BAD_REQUEST, "bad txid").into_response();
    };
    if !state.networks.contains(&network) {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    }

    // A point lookup on an indexed column — cheap enough to skip the cache.
    let db = state.base_dir.join(format!("{network}.db"));
    let result = tokio::task::spawn_blocking(move || -> Result<Option<kascov_core::CovenantId>> {
        let store = kascov_core::store::Store::open(&db, network)?;
        Ok(store.covenant_by_txid(&txid)?)
    })
    .await;
    let ok_headers = [
        (header::CONTENT_TYPE, "application/json; charset=utf-8"),
        (header::CACHE_CONTROL, "public, max-age=60, s-maxage=300"),
        (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
    ];
    match result {
        Ok(Ok(Some(covenant_id))) => (
            ok_headers,
            serde_json::json!({ "txid": tx_hex, "covenant_id": covenant_id }).to_string(),
        )
            .into_response(),
        Ok(Ok(None)) => (
            StatusCode::NOT_FOUND,
            [
                (header::CACHE_CONTROL, "public, max-age=10, s-maxage=10"),
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            ],
            "transaction not seen by kascov",
        )
            .into_response(),
        Ok(Err(err)) => {
            tracing::error!("{network}: tx lookup failed: {err}");
            (StatusCode::SERVICE_UNAVAILABLE, "lookup unavailable").into_response()
        }
        Err(err) => {
            tracing::error!("{network}: tx lookup panicked: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}
