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
    /// Fetch a transaction from the node (via its accepting block, known to
    /// the index) and print its full covenant anatomy — bindings, budgets,
    /// payload lanes. The truth tool for classification disputes.
    InspectTx { txid: TxId },
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
        Command::InspectTx { txid } => inspect_tx(&cli, txid).await,
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

/// Row cap for the address endpoint — a TN10 faucet key can plausibly own
/// thousands of covenants; covenants_total still reports the true count.
const ADDR_MAX_COVENANTS: usize = 1000;

fn build_live_snapshot(store: &Store, network: kascov_core::Network) -> Result<serde_json::Value> {
    let tip = store.tip()?;
    Ok(serde_json::json!({
        "network": network.to_string(),
        "generated_at_ms": now_ms(),
        "tip_daa": tip.map(|t| t.0),
        "tip_at_ms": tip.map(|t| t.1),
        "processed_daa": store.processed_daa()?,
        "stats": stats_json(store)?,
        "recent_events": store.recent_events(LIVE_EVENTS)?,
    }))
}

/// "Today on the testnet": the last 24 hours in one small JSON — counts,
/// headline coins, and the tip anchor. Pure SQL over the index.
const DIGEST_WINDOW_HOURS: u64 = 24;
const DIGEST_WINDOW_DAA: u64 = DIGEST_WINDOW_HOURS * 3600 * 10; // DAA ticks ~10/s

fn build_digest(store: &Store, network: kascov_core::Network) -> Result<serde_json::Value> {
    let tip = store.tip()?;
    let d = store.digest(DIGEST_WINDOW_DAA)?;
    Ok(serde_json::json!({
        "network": network.to_string(),
        "window_hours": DIGEST_WINDOW_HOURS,
        "generated_at_ms": now_ms(),
        "tip_daa": tip.map(|t| t.0),
        "tip_at_ms": tip.map(|t| t.1),
        "births": d.births,
        "moves": d.moves,
        "burns": d.burns,
        "value_born": d.value_born,
        "active_now": d.active_now,
        "busiest": d.busiest.map(|(id, n)| serde_json::json!({ "covenant_id": id, "events": n })),
        "biggest_birth": d.biggest_birth.map(|(id, v)| serde_json::json!({ "covenant_id": id, "value": v })),
    }))
}

/// (range, total window in DAA, bucket width in DAA) — ~60 buckets each
/// (DAA ticks ~10/s). "all" derives its width from the index's own bounds.
const ACTIVITY_RANGES: [(&str, u64, u64); 4] = [
    ("1h", 36_000, 600),
    ("6h", 216_000, 3_600),
    ("24h", 864_000, 14_400),
    ("48h", 1_728_000, 28_800),
];

/// Kind counts per DAA bucket for the activity chart. Bucket edges are
/// absolute multiples of the width and the cutoff is aligned down to one,
/// so consecutive rebuilds agree bucket-for-bucket (the CDN and the client
/// can diff by `daa`). Empty buckets are omitted; the client zero-fills.
fn build_activity_snapshot(
    store: &Store,
    network: kascov_core::Network,
    range: &'static str,
) -> Result<serde_json::Value> {
    let tip = store.tip()?;
    let bounds = store.event_daa_bounds()?;
    // window anchor: the recorded tip, else the newest event (pre-tip DBs)
    let anchor = tip.map(|t| t.0).or(bounds.map(|b| b.1)).unwrap_or(0);
    let (bucket_daa, cutoff) = if range == "all" {
        let min = bounds.map(|b| b.0).unwrap_or(anchor);
        let width = (anchor.saturating_sub(min) / 64).max(1);
        (width, (min / width) * width)
    } else {
        let &(_, total, width) =
            ACTIVITY_RANGES.iter().find(|(r, ..)| *r == range).expect("range is whitelisted");
        (width, (anchor.saturating_sub(total) / width) * width)
    };
    Ok(serde_json::json!({
        "network": network.to_string(),
        "range": range,
        "bucket_daa": bucket_daa,
        "window_start_daa": cutoff,
        "generated_at_ms": now_ms(),
        "tip_daa": tip.map(|t| t.0),
        "tip_at_ms": tip.map(|t| t.1),
        "buckets": store.activity(bucket_daa, cutoff)?,
    }))
}

/// The explorer grid: stats + one summary row per covenant, no timelines and
/// no scripts. This is what the web app loads up front; per-coin detail comes
/// from `/data/{network}/c/{id}.json` on demand. At 42k covenants the old
/// all-in-one snapshot passed 1 GiB in flight — this stays a few MB.
fn build_grid_snapshot(
    store: &Store,
    network: kascov_core::Network,
    after: Option<(u64, [u8; 32])>,
    limit: Option<u64>,
) -> Result<serde_json::Value> {
    // Back-compat: with no paging params we still serialize every covenant
    // (`store.list(u64::MAX)`). A caller that passes `?after_daa=`/`?limit=`
    // opts into a page window ordered by `last_activity_daa DESC`, default
    // 5000 most-recent; `next_after_daa` appears when more rows remain.
    const DEFAULT_PAGE: u64 = 5000;
    let paged = after.is_some() || limit.is_some();
    let mut next_after_daa: Option<u64> = None;
    let mut next_after_id: Option<String> = None;
    let covenants = if paged {
        let page = limit.unwrap_or(DEFAULT_PAGE).max(1);
        // Over-fetch by one to detect whether another page exists.
        let mut rows = store.list_page(after, page.saturating_add(1))?;
        if rows.len() as u64 > page {
            rows.truncate(page as usize);
            if let Some(last) = rows.last() {
                next_after_daa = Some(last.last_activity_daa);
                next_after_id = Some(last.covenant_id.to_string());
            }
        }
        rows
    } else {
        store.list(u64::MAX)?
    };
    let born = store.born_values()?;
    let templates = store.covenant_templates()?;
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
                "template": templates.get(&c.covenant_id),
            })
        })
        .collect();
    let mut snapshot = serde_json::json!({
        "network": network.to_string(),
        "grid": true,
        "generated_at_ms": now_ms(),
        "tip_daa": tip.map(|t| t.0),
        "tip_at_ms": tip.map(|t| t.1),
        "processed_daa": store.processed_daa()?,
        "stats": stats_json(store)?,
        "covenants": rows,
    });
    if let (Some(daa), Some(id)) = (next_after_daa, next_after_id) {
        snapshot["next_after_daa"] = serde_json::json!(daa);
        snapshot["next_after_id"] = serde_json::json!(id);
    }
    Ok(snapshot)
}

/// Contract-type analytics: which script templates run on this network,
/// aggregated over every state UTXO ever indexed (recognition is stamped at
/// write time — this is two GROUP BYs, no decoding). Reveal counts ride
/// along because compiled contracts (Mecenas, Escrow, LastWill) live behind
/// p2sh commitments and only show themselves at spend time.
fn build_templates_snapshot(store: &Store, network: kascov_core::Network) -> Result<serde_json::Value> {
    #[derive(Default)]
    struct Row {
        live_states: u64,
        live_value: u64,
        ever_seen: u64,
        covenants: u64,
        revealed_runs: u64,
    }
    let mut named: std::collections::BTreeMap<String, Row> = Default::default();
    let mut unrecognized = Row::default();
    for s in store.template_stats()? {
        let row = Row {
            live_states: s.live_states,
            live_value: s.live_value,
            ever_seen: s.ever_seen,
            covenants: s.covenants,
            revealed_runs: 0,
        };
        match s.template {
            Some(name) => {
                named.insert(name, row);
            }
            None => unrecognized = row,
        }
    }
    // A template can exist through reveals alone — no live state carries it.
    for (name, runs) in store.revealed_template_counts()? {
        named.entry(name).or_default().revealed_runs = runs;
    }
    let mut rows: Vec<(String, Row)> = named.into_iter().collect();
    rows.sort_by(|a, b| {
        (b.1.ever_seen + b.1.revealed_runs)
            .cmp(&(a.1.ever_seen + a.1.revealed_runs))
            .then_with(|| a.0.cmp(&b.0))
    });
    let tip = store.tip()?;
    Ok(serde_json::json!({
        "network": network.to_string(),
        "generated_at_ms": now_ms(),
        "tip_daa": tip.map(|t| t.0),
        "tip_at_ms": tip.map(|t| t.1),
        "templates": rows.iter().map(|(name, r)| serde_json::json!({
            "name": name,
            "live_states": r.live_states,
            "live_value": r.live_value,
            "ever_seen": r.ever_seen,
            "covenants": r.covenants,
            "revealed_runs": r.revealed_runs,
        })).collect::<Vec<_>>(),
        "unrecognized": {
            "live_states": unrecognized.live_states,
            "live_value": unrecognized.live_value,
            "ever_seen": unrecognized.ever_seen,
            "covenants": unrecognized.covenants,
        },
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
    // Per-coin holders: the p2pk-state owners of THIS covenant (inverse of
    // covenants_by_pubkey). Cheap single query, capped at 100 recent owners.
    let holders = store.holders_of_covenant(&summary.covenant_id, 100)?;
    obj.insert("holders".into(), serde_json::json!(holders));
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
            if decoded.uses_zk_ops {
                json["zk_system"] = serde_json::json!(decoded.zk_system);
            }
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
                    if d.uses_zk_ops {
                        json["revealed_zk_system"] = serde_json::json!(d.zk_system);
                    }
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
            // multi-covenant transactions: name the other coins this tx moved
            if let Ok(others) = store.covenants_by_txid(&e.txid) {
                let with: Vec<_> = others.into_iter().filter(|c| c != &summary.covenant_id).take(4).collect();
                if !with.is_empty() {
                    v["with_covenants"] = serde_json::json!(with);
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

/// Ground truth for one transaction: bindings, budgets, payload/lane.
async fn inspect_tx(cli: &Cli, txid: TxId) -> Result<()> {
    let store = open_store(cli)?;
    let Some(block) = store.accepting_block_of(&txid)? else {
        anyhow::bail!("{txid} is not in this index — kascov only knows blocks it has walked");
    };
    let node = NodeHandle::connect(cli.network, cli.rpc.as_deref()).await?;
    let accepting = node.block_with_txs(block).await.context("accepting block no longer on the node (pruned?)")?;
    // the accepting chain block ACCEPTS the tx; its body lives in the
    // accepting block itself or one of its mergeset blocks (same walk the
    // sync engine does)
    let mut found = accepting.transactions.iter().find(|t| t.txid == txid).cloned();
    if found.is_none() {
        for &hash in &accepting.mergeset {
            if let Ok(b) = node.block_with_txs(hash).await {
                if let Some(t) = b.transactions.iter().find(|t| t.txid == txid) {
                    found = Some(t.clone());
                    break;
                }
            }
        }
    }
    let Some(tx) = found else {
        anyhow::bail!("tx not found in accepting block or its mergeset (pruned or reorged since indexing)");
    };
    let tx = &tx;

    println!("tx {txid}");
    if !tx.payload.is_empty() {
        // KIP-21 user lanes: 4-byte namespace + 16 zero bytes prefix
        let lane = tx.payload.len() >= 20 && tx.payload[4..20].iter().all(|&b| b == 0);
        let lane_note = if lane {
            format!("  (KIP-21 lane, namespace 0x{})", hex::encode(&tx.payload[..4]))
        } else {
            String::new()
        };
        println!("payload: {} bytes{lane_note}", tx.payload.len());
    }
    println!("inputs:");
    for (i, input) in tx.inputs.iter().enumerate() {
        let known = store
            .utxo_covenant(&input.previous_outpoint)?
            .map(|c| format!("  <- state of covenant {c}"))
            .unwrap_or_default();
        println!(
            "  #{i} spends {} (budget {}){known}",
            input.previous_outpoint, input.compute_budget
        );
    }
    println!("outputs:");
    for (i, o) in tx.outputs.iter().enumerate() {
        let bind = o
            .covenant
            .map(|b| format!("  BOUND to {} (authorizing input #{})", b.covenant_id, b.authorizing_input))
            .unwrap_or_default();
        println!(
            "  #{i} value {} script {}…{bind}",
            o.value,
            hex::encode(&o.spk_script[..o.spk_script.len().min(12)])
        );
    }
    Ok(())
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

/// Cap on concurrent SSE subscribers per network — extras are rejected with
/// 503 and stay on the polling path.
const MAX_STREAM_SUBSCRIBERS: usize = 512;
/// Broadcast buffer per network. A receiver that falls behind gets
/// `RecvError::Lagged`, skips ahead, and the client resyncs via the poll.
const STREAM_BUFFER: usize = 256;

/// One network's live event fan-out: the chain follower broadcasts each
/// covenant event as pre-serialized JSON; every open SSE connection holds a
/// receiver. Messages are hints — clients confirm through the polled feeds.
struct LiveChannel {
    tx: tokio::sync::broadcast::Sender<std::sync::Arc<str>>,
    subscribers: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl LiveChannel {
    fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(STREAM_BUFFER);
        Self { tx, subscribers: Default::default() }
    }
}

/// Frees a subscriber slot when its SSE stream is dropped (client gone,
/// keep-alive write failed, or the connection timed out).
struct SubscriberSlot(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl Drop for SubscriberSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

struct ServeState {
    base_dir: std::path::PathBuf,
    networks: Vec<Network>,
    max_events: u64,
    /// Node url for the custodial deploy endpoint (None → public resolver).
    rpc: Option<String>,
    /// Rate limiter shared by the custodial /deploy endpoint.
    deploy_limiter: tokio::sync::Mutex<DeployLimiter>,
    /// Serializes custodial deploys: they all spend from one funding wallet, so
    /// concurrent builds would pick the same UTXO and double-spend. One in flight.
    deploy_inflight: tokio::sync::Mutex<()>,
    /// Per-network live event broadcast (SSE). A Vec, not a HashMap:
    /// `Network` has no `Hash` impl and there are at most a couple entries.
    live: Vec<(Network, LiveChannel)>,
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
    use axum::routing::{get, post};

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

    let mut live = Vec::with_capacity(networks.len());
    for &network in &networks {
        let channel = LiveChannel::new();
        let db = base_dir.join(format!("{network}.db"));
        tokio::spawn(follow_forever(network, cli.rpc.clone(), db, channel.tx.clone()));
        live.push((network, channel));
    }

    let state = std::sync::Arc::new(ServeState {
        base_dir,
        networks,
        max_events,
        rpc: cli.rpc.clone(),
        deploy_limiter: tokio::sync::Mutex::new(DeployLimiter::new()),
        deploy_inflight: tokio::sync::Mutex::new(()),
        live,
        cache: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        build_locks: tokio::sync::Mutex::new(std::collections::HashMap::new()),
    });
    // Periodic cache sweep: the insert-time eviction only fires past 2048
    // entries, so expired multi-MB bodies (galaxy, grid pages) could otherwise
    // linger indefinitely on a quiet keyspace. Sweep every 60s; drop bodies
    // older than 300s and build locks nobody holds.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                state
                    .cache
                    .lock()
                    .await
                    .retain(|_, (at, _)| at.elapsed() < std::time::Duration::from_secs(300));
                state
                    .build_locks
                    .lock()
                    .await
                    .retain(|_, l| std::sync::Arc::strong_count(l) > 1);
            }
        });
    }
    let app = axum::Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/data/{network}/simulate", post(simulate_handler))
        .route("/data/{network}/zk-verify", post(zk_verify_handler))
        .route("/data/{network}/compile", post(compile_handler))
        .route("/data/{network}/deploy", post(deploy_handler))
        .route("/data/{network}/publish", post(publish_handler))
        .route("/data/{network}/verified/{hash}", get(verified_handler))
        .route("/data/{network}/subscribe", post(subscribe_handler))
        .route("/data/{file}", get(data_handler))
        .route("/data/{network}/c/{id}", get(detail_handler))
        .route("/data/{network}/tx/{txid}", get(tx_handler))
        .route("/data/{network}/families.json", get(families_handler))
        .route("/data/{network}/reorgs.json", get(reorgs_handler))
        .route("/data/{network}/galaxy.json", get(galaxy_handler))
        .route("/data/{network}/lanes.json", get(lanes_handler))
        .route("/data/{network}/inscriptions.json", get(inscriptions_handler))
        .route("/data/{network}/lifespans.json", get(lifespans_handler))
        .route("/data/{network}/digest.json", get(digest_handler))
        .route("/data/{network}/templates.json", get(templates_handler))
        .route("/data/{network}/activity.json", get(activity_handler))
        .route("/data/{network}/addr/{address}", get(addr_handler))
        .route("/data/{network}/stream", get(stream_handler))
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
async fn follow_forever(
    network: Network,
    rpc: Option<String>,
    db: std::path::PathBuf,
    live_tx: tokio::sync::broadcast::Sender<std::sync::Arc<str>>,
) {
    use kascov_core::sync::SyncUpdate;
    // Lives across reconnects: every sync failure breaks to a fresh session,
    // so a per-session counter would reset before ever reaching the
    // testnet-reset recovery threshold below.
    let mut consecutive_errors = 0u32;
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
        loop {
            let result = kascov_core::sync::sync_once(&node, &mut store, None, |update| match update {
                SyncUpdate::Event { covenant_id, kind, txid, accepting_daa } => {
                    tracing::info!("{network}: {} covenant {covenant_id}", kind.as_str());
                    // Fan out to any open SSE streams; serialization is skipped
                    // entirely when nobody is listening, and send() failing
                    // (zero receivers) is fine.
                    if live_tx.receiver_count() > 0 {
                        let msg = serde_json::json!({
                            "covenant_id": covenant_id,
                            "kind": kind.as_str(),
                            "txid": txid,
                            "accepting_daa": accepting_daa,
                        })
                        .to_string();
                        let _ = live_tx.send(msg.into());
                    }
                }
                SyncUpdate::Reorg { rolled_back } => {
                    tracing::info!("{network}: reorg — rolled back {rolled_back} chain blocks");
                    // Same fire-and-forget fan-out as events; the "kind":"reorg"
                    // tag lets subscribers distinguish it from covenant activity.
                    if live_tx.receiver_count() > 0 {
                        let msg = serde_json::json!({
                            "kind": "reorg",
                            "rolled_back": rolled_back,
                        })
                        .to_string();
                        let _ = live_tx.send(msg.into());
                    }
                }
                SyncUpdate::Progress(_) => {}
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
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
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
    // Grid paging: `?after_daa=` (exclusive cursor) and `?limit=` (page size,
    // capped) walk the grid newest-first. Invalid numbers are ignored so a bad
    // param degrades to the full snapshot rather than erroring. Params are only
    // meaningful for the grid, and are folded into the cache key so each page
    // caches independently.
    const MAX_PAGE: u64 = 20_000;
    let (after, limit) = if live {
        (None, None)
    } else {
        // Compound cursor `(after_daa, after_id)`. A caller sending only
        // `after_daa` (older client) gets id = 0xFF..FF, which re-includes the
        // whole boundary DAA — the client dedups by id, so nothing is skipped.
        let after = q.get("after_daa").and_then(|s| s.parse::<u64>().ok()).map(|daa| {
            let id = q
                .get("after_id")
                .and_then(|s| {
                    let mut b = [0u8; 32];
                    hex::decode_to_slice(s.trim(), &mut b).ok().map(|_| b)
                })
                .unwrap_or([0xFF; 32]);
            (daa, id)
        });
        let limit = q.get("limit").and_then(|s| s.parse::<u64>().ok()).map(|l| l.clamp(1, MAX_PAGE));
        (after, limit)
    };
    let key = match (after, limit) {
        (None, None) => name.to_string(),
        (a, l) => format!(
            "{name}?after_daa={}&after_id={}&limit={}",
            a.map_or(0, |v| v.0),
            a.map_or_else(String::new, |v| hex::encode(v.1)),
            l.map_or(0, |v| v)
        ),
    };
    serve_cached(&state, key, ttl, cache_control, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        let snapshot = if live {
            build_live_snapshot(&store, network)?
        } else {
            build_grid_snapshot(&store, network, after, limit)?
        };
        Ok(Some(serde_json::to_string(&snapshot)?))
    })
    .await
}

/// Cluster covenants that moved together into "apps" (multi-contract flows):
/// union-find over transactions that touched more than one covenant.
fn build_families(store: &Store, network: kascov_core::Network) -> Result<serde_json::Value> {
    let edges = store.multi_covenant_txs()?;
    let templates = store.covenant_templates()?;

    // union-find over covenant ids
    let mut parent: std::collections::HashMap<kascov_core::CovenantId, kascov_core::CovenantId> =
        std::collections::HashMap::new();
    fn find(
        parent: &mut std::collections::HashMap<kascov_core::CovenantId, kascov_core::CovenantId>,
        x: kascov_core::CovenantId,
    ) -> kascov_core::CovenantId {
        let p = *parent.get(&x).unwrap_or(&x);
        if p == x {
            return x;
        }
        let root = find(parent, p);
        parent.insert(x, root);
        root
    }
    let mut shared_txs: std::collections::HashMap<kascov_core::CovenantId, u64> =
        std::collections::HashMap::new();
    for (_txid, covs) in &edges {
        for c in covs {
            parent.entry(*c).or_insert(*c);
            *shared_txs.entry(*c).or_insert(0) += 1;
        }
        // union all covenants in this tx to the first
        if let Some(first) = covs.first() {
            for c in &covs[1..] {
                let (ra, rb) = (find(&mut parent, *first), find(&mut parent, *c));
                if ra != rb {
                    parent.insert(ra, rb);
                }
            }
        }
    }

    // gather members per cluster root
    let members: Vec<kascov_core::CovenantId> = parent.keys().copied().collect();
    let mut clusters: std::collections::HashMap<kascov_core::CovenantId, Vec<kascov_core::CovenantId>> =
        std::collections::HashMap::new();
    for m in members {
        let root = find(&mut parent, m);
        clusters.entry(root).or_default().push(m);
    }

    let mut out: Vec<serde_json::Value> = clusters
        .into_values()
        .filter(|c| c.len() >= 2)
        .map(|mut covs| {
            covs.sort_by(|a, b| a.0.cmp(&b.0));
            let members: Vec<_> = covs
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "covenant_id": c,
                        "template": templates.get(c),
                        "shared_txs": shared_txs.get(c).copied().unwrap_or(0),
                    })
                })
                .collect();
            serde_json::json!({ "size": covs.len(), "members": members })
        })
        .collect();
    // biggest apps first
    out.sort_by(|a, b| b["size"].as_u64().cmp(&a["size"].as_u64()));

    let tip = store.tip()?;
    Ok(serde_json::json!({
        "network": network.to_string(),
        "generated_at_ms": now_ms(),
        "tip_daa": tip.map(|t| t.0),
        "tip_at_ms": tip.map(|t| t.1),
        "families": out,
    }))
}

/// Build the whole-network "galaxy": the same union-find clusters as
/// `build_families`, but with everything a zoomable node-link map needs and
/// `families.json` lacks — precomputed 2D node positions (so the browser never
/// runs a force sim), weighted pairwise edges (how many txs each pair shared),
/// and per-node template + alive/burned status. Positions come from a
/// cumulative-area sunflower packing: big apps near the galactic core, size-2
/// dust at the rim. Coordinates are centered on the origin and quantized to
/// integers to keep the payload small. See docs plan Wave 1.
fn build_galaxy(store: &Store, network: kascov_core::Network) -> Result<serde_json::Value> {
    use kascov_core::CovenantId;
    use std::collections::HashMap;

    let edges_raw = store.multi_covenant_txs()?;
    let templates = store.covenant_templates()?;

    // alive/burned per covenant — one grouped pass; same semantics as the
    // grid's live_utxos > 0 (missing entries read as inactive below).
    let active = store.active_flags()?;

    // union-find over covenant ids (mirrors build_families)
    let mut parent: HashMap<CovenantId, CovenantId> = HashMap::new();
    fn find(parent: &mut HashMap<CovenantId, CovenantId>, x: CovenantId) -> CovenantId {
        let p = *parent.get(&x).unwrap_or(&x);
        if p == x {
            return x;
        }
        let root = find(parent, p);
        parent.insert(x, root);
        root
    }
    let mut degree: HashMap<CovenantId, u32> = HashMap::new();
    for (_txid, covs) in &edges_raw {
        for c in covs {
            parent.entry(*c).or_insert(*c);
            *degree.entry(*c).or_insert(0) += 1;
        }
        if let Some(first) = covs.first() {
            for c in &covs[1..] {
                let (ra, rb) = (find(&mut parent, *first), find(&mut parent, *c));
                if ra != rb {
                    parent.insert(ra, rb);
                }
            }
        }
    }

    // gather clusters (root -> members), keep size >= 2
    let all: Vec<CovenantId> = parent.keys().copied().collect();
    let mut clusters: HashMap<CovenantId, Vec<CovenantId>> = HashMap::new();
    for m in all {
        let root = find(&mut parent, m);
        clusters.entry(root).or_default().push(m);
    }
    let mut cluster_list: Vec<Vec<CovenantId>> =
        clusters.into_values().filter(|c| c.len() >= 2).collect();
    // biggest first (core), deterministic tiebreak by smallest member id
    cluster_list.sort_by(|a, b| {
        b.len()
            .cmp(&a.len())
            .then_with(|| a.iter().map(|c| c.0).min().cmp(&b.iter().map(|c| c.0).min()))
    });
    for c in &mut cluster_list {
        c.sort_by(|a, b| a.0.cmp(&b.0));
    }

    // intern template names once; -1 == unrecognized
    let mut tpl_names: Vec<String> = Vec::new();
    let mut tpl_index: HashMap<&str, i64> = HashMap::new();
    for name in templates.values() {
        if !tpl_index.contains_key(name.as_str()) {
            tpl_index.insert(name.as_str(), tpl_names.len() as i64);
            tpl_names.push(name.clone());
        }
    }
    let tpl_of = |id: &CovenantId| -> i64 {
        templates
            .get(id)
            .and_then(|n| tpl_index.get(n.as_str()).copied())
            .unwrap_or(-1)
    };

    // ---- layout: cumulative-area sunflower ----
    const GOLDEN_ANGLE: f64 = 2.399_963_229_728_653; // 137.5° in radians
    const TAU: f64 = std::f64::consts::TAU;
    const SPACING: f64 = 0.62; // ~ disk area == total cluster area
    let ring_radius = |size: usize| -> f64 { 14.0 + 10.0 * (size as f64).sqrt() };

    let mut nodes: Vec<serde_json::Value> = Vec::new();
    let mut apps: Vec<serde_json::Value> = Vec::new();
    let mut node_index: HashMap<CovenantId, usize> = HashMap::new();
    let (mut min_x, mut min_y, mut max_x, mut max_y) =
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);

    let mut cum_area = 0.0_f64;
    for (i, cluster) in cluster_list.iter().enumerate() {
        let size = cluster.len();
        let cr = ring_radius(size);
        cum_area += std::f64::consts::PI * (cr + 6.0) * (cr + 6.0);
        let spiral_r = SPACING * cum_area.sqrt();
        let theta = i as f64 * GOLDEN_ANGLE;
        let (cx, cy) = (spiral_r * theta.cos(), spiral_r * theta.sin());

        // dominant template of the cluster = most common member template
        let mut counts: HashMap<i64, usize> = HashMap::new();
        for m in cluster {
            *counts.entry(tpl_of(m)).or_insert(0) += 1;
        }
        let dom_t = counts
            .iter()
            .filter(|(t, _)| **t >= 0)
            .max_by_key(|(_, c)| **c)
            .map(|(t, _)| *t)
            .unwrap_or(-1);

        apps.push(serde_json::json!({
            "cx": cx.round() as i64,
            "cy": cy.round() as i64,
            "r": cr.round() as i64,
            "size": size,
            "t": dom_t,
        }));

        for (mi, m) in cluster.iter().enumerate() {
            let a = (mi as f64 / size as f64) * TAU;
            let (x, y) = (cx + cr * a.cos(), cy + cr * a.sin());
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            let nr = 3 + degree.get(m).copied().unwrap_or(1).min(6);
            node_index.insert(*m, nodes.len());
            nodes.push(serde_json::json!({
                "id": m,
                "t": tpl_of(m),
                "s": if *active.get(m).unwrap_or(&false) { 1 } else { 0 },
                "x": x.round() as i64,
                "y": y.round() as i64,
                "r": nr,
                "a": i,
            }));
        }
    }

    // ---- pairwise weighted edges (cap clique-explosion) ----
    let mut edge_w: HashMap<(usize, usize), u32> = HashMap::new();
    let bump = |a: usize, b: usize, edge_w: &mut HashMap<(usize, usize), u32>| {
        let key = if a < b { (a, b) } else { (b, a) };
        *edge_w.entry(key).or_insert(0) += 1;
    };
    for (_txid, covs) in &edges_raw {
        let idxs: Vec<usize> = covs.iter().filter_map(|c| node_index.get(c).copied()).collect();
        if idxs.len() < 2 {
            continue;
        }
        if idxs.len() <= 8 {
            for i in 0..idxs.len() {
                for j in (i + 1)..idxs.len() {
                    bump(idxs[i], idxs[j], &mut edge_w);
                }
            }
        } else {
            // a single high-degree tx would emit O(k^2) edges; star it instead
            let hub = idxs[0];
            for &other in &idxs[1..] {
                bump(hub, other, &mut edge_w);
            }
        }
    }
    const MAX_EDGES: usize = 80_000;
    let mut edges: Vec<(usize, usize, u32)> =
        edge_w.into_iter().map(|((a, b), w)| (a, b, w)).collect();
    let edge_total = edges.len();
    if edges.len() > MAX_EDGES {
        edges.sort_by(|a, b| b.2.cmp(&a.2)); // keep the heaviest links
        edges.truncate(MAX_EDGES);
    }
    let edges_json: Vec<serde_json::Value> = edges
        .iter()
        .map(|(a, b, w)| serde_json::json!([a, b, w]))
        .collect();

    if !min_x.is_finite() {
        min_x = 0.0;
        min_y = 0.0;
        max_x = 0.0;
        max_y = 0.0;
    }
    let tip = store.tip()?;
    Ok(serde_json::json!({
        "network": network.to_string(),
        "generated_at_ms": now_ms(),
        "tip_daa": tip.map(|t| t.0),
        "tip_at_ms": tip.map(|t| t.1),
        "bounds": {
            "minx": min_x.floor() as i64,
            "miny": min_y.floor() as i64,
            "w": (max_x - min_x).ceil() as i64,
            "h": (max_y - min_y).ceil() as i64,
        },
        "templates": tpl_names,
        "apps": apps,
        "nodes": nodes,
        "edges": edges_json,
        "edges_total": edge_total,
    }))
}

/// POST /data/{network}/compile — compile SilverScript source + constructor
/// args to script hex by shelling out to the `silverc` binary (path in the
/// SILVERC_BIN env var). Powers verify-and-publish and the no-code builder.
#[derive(serde::Deserialize)]
struct CompileReq {
    source: String,
    #[serde(default)]
    args: Vec<String>,
}

fn json_resp(v: serde_json::Value) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;
    (StatusCode::OK, [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], v.to_string()).into_response()
}

fn blake2b32(bytes: &[u8]) -> [u8; 32] {
    *blake2b_simd::Params::new().hash_length(32).hash(bytes).as_bytes().first_chunk::<32>().unwrap()
}

/// Compile SilverScript source + args to script hex via the `silverc` binary
/// (SILVERC_BIN). Ok(hex) or Err(message).
async fn run_silverc(source: String, args: Vec<String>) -> Result<String, String> {
    let bin = std::env::var("SILVERC_BIN").unwrap_or_default();
    if bin.is_empty() {
        return Err("the SilverScript compiler isn't available on this server".into());
    }
    let out = tokio::task::spawn_blocking(move || {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new(&bin)
            .arg("-")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child.stdin.take().unwrap().write_all(source.as_bytes())?;
        let o = child.wait_with_output()?;
        std::io::Result::Ok((
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).trim().to_string(),
            String::from_utf8_lossy(&o.stderr).trim().to_string(),
        ))
    })
    .await;
    match out {
        Ok(Ok((true, hex, _))) => Ok(hex),
        Ok(Ok((false, _, err))) => Err(err),
        _ => Err("compiler failed to run".into()),
    }
}

/// POST /data/{network}/zk-verify — run a self-contained ZK verification
/// script through the real engine (Kaspa's ark_groth16 / RISC-Zero verifier).
#[derive(serde::Deserialize)]
struct ZkReq {
    program_hex: String,
}

async fn zk_verify_handler(
    axum::extract::Path(_net): axum::extract::Path<String>,
    axum::Json(req): axum::Json<ZkReq>,
) -> axum::response::Response {
    if req.program_hex.len() > 8_000 {
        return json_resp(serde_json::json!({ "ok": false, "error": "program too large" }));
    }
    let Ok(bytes) = hex::decode(req.program_hex.trim().trim_start_matches("0x")) else {
        return json_resp(serde_json::json!({ "ok": false, "error": "not valid hex" }));
    };
    let (valid, reason) = tokio::task::spawn_blocking(move || kascov_sim::verify_zk_script(&bytes))
        .await
        .unwrap_or((false, "verifier failed to run".into()));
    json_resp(serde_json::json!({ "ok": true, "valid": valid, "reason": reason }))
}

async fn compile_handler(
    axum::extract::Path(_net): axum::extract::Path<String>,
    axum::Json(req): axum::Json<CompileReq>,
) -> axum::response::Response {
    if req.source.len() > 40_000 || req.args.len() > 16 || req.args.iter().any(|a| a.len() > 200) {
        return json_resp(serde_json::json!({ "ok": false, "error": "input too large" }));
    }
    match run_silverc(req.source, req.args).await {
        Ok(hex) => json_resp(serde_json::json!({ "ok": true, "hex": hex })),
        Err(e) => json_resp(serde_json::json!({ "ok": false, "error": e })),
    }
}

// ── Custodial deploy (SAFE, gated OFF by default) ─────────────────────────
// POST /data/{network}/deploy births a covenant with the server's own faucet
// key, so the browser builder can deploy without a local toolchain. It is
// ACTIVE ONLY when KASCOV_DEPLOY_KEY is set AND network == testnet-10 —
// otherwise the route answers 404, as if it didn't exist. Both a global token
// bucket and a per-IP/day cap throttle it (the custodial key is spendable, so
// abuse just drains testnet coins, never mainnet, but we still gate hard).

/// Global token bucket + per-IP daily counter for the deploy endpoint.
struct DeployLimiter {
    tokens: f64,
    last_refill: std::time::Instant,
    per_ip: std::collections::HashMap<String, (u64, u32)>, // ip -> (day, count)
}

// The GLOBAL token bucket is the only sound bound on faucet drain (X-Forwarded-For
// is client-spoofable, so the per-IP cap is best-effort — meaningful only behind a
// trusted proxy). Bucket holds 5 deploys, refilling 1 per 10 min (~144/day). With
// the 10 TKAS value ceiling below that caps drain at ~1,440 TKAS/day — fund the
// custodial key accordingly.
const DEPLOY_BUCKET_CAP: f64 = 5.0;
const DEPLOY_REFILL_PER_SEC: f64 = 1.0 / 600.0;
/// Each client IP may deploy this many coins per calendar day (UTC) — best-effort.
const DEPLOY_PER_IP_PER_DAY: u32 = 20;
/// Hard ceiling on the per-IP map size so a spoofed-XFF flood can't OOM us.
const DEPLOY_IP_MAP_MAX: usize = 50_000;

impl DeployLimiter {
    fn new() -> Self {
        Self { tokens: DEPLOY_BUCKET_CAP, last_refill: std::time::Instant::now(), per_ip: Default::default() }
    }

    /// Charge one deploy to `ip`. Ok on success; Err(reason) when throttled.
    fn try_take(&mut self, ip: &str) -> Result<(), &'static str> {
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;
        self.tokens = (self.tokens + dt * DEPLOY_REFILL_PER_SEC).min(DEPLOY_BUCKET_CAP);

        // Check the global bucket FIRST, before touching per_ip — so a flood of
        // throttled (or spoofed-IP) requests never allocates a per-IP row.
        if self.tokens < 1.0 {
            return Err("deploy rate limit — try again in a few minutes");
        }

        let day = now_ms() / 86_400_000;
        // Bound the map hard, regardless of day: evict stale days first, and if
        // that isn't enough (a same-day spoofed-XFF flood), drop it entirely.
        if self.per_ip.len() > DEPLOY_IP_MAP_MAX {
            self.per_ip.retain(|_, (d, _)| *d == day);
            if self.per_ip.len() > DEPLOY_IP_MAP_MAX {
                self.per_ip.clear();
            }
        }
        let entry = self.per_ip.entry(ip.to_string()).or_insert((day, 0));
        if entry.0 != day {
            *entry = (day, 0);
        }
        if entry.1 >= DEPLOY_PER_IP_PER_DAY {
            return Err("daily deploy limit reached for your address — try again tomorrow");
        }
        self.tokens -= 1.0;
        entry.1 += 1;
        Ok(())
    }

    /// Give back a token charged by `try_take` — used when a deploy is aborted
    /// for a reason that isn't the caller's fault (e.g. the faucet ran dry),
    /// so a doomed request doesn't burn the day's budget.
    fn refund(&mut self, ip: &str) {
        self.tokens = (self.tokens + 1.0).min(DEPLOY_BUCKET_CAP);
        if let Some(entry) = self.per_ip.get_mut(ip) {
            entry.1 = entry.1.saturating_sub(1);
        }
    }
}

/// Best-effort client IP: the first hop in X-Forwarded-For (set by the CDN /
/// Cloud Run front end), else X-Real-IP, else a shared bucket key.
fn client_ip(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| headers.get("x-real-ip").and_then(|v| v.to_str().ok()).map(|s| s.trim().to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

#[derive(serde::Deserialize)]
struct DeployReq {
    program_hex: String,
    #[serde(default)]
    value: u64,
}

/// POST /data/{network}/deploy — see the section comment above. Body is
/// `{program_hex, value}`; on success returns `{ok, covenant_id, network}`.
async fn deploy_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<DeployReq>,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let not_found = || (StatusCode::NOT_FOUND, "not found").into_response();
    let net = net_name.strip_suffix("/deploy").unwrap_or(&net_name);
    let Ok(network) = net.parse::<Network>() else { return not_found() };
    // Gated OFF by default: the route only exists when armed for testnet-10.
    let deploy_key = std::env::var("KASCOV_DEPLOY_KEY").unwrap_or_default();
    if deploy_key.trim().is_empty()
        || network != Network::Testnet(10)
        || !state.networks.contains(&network)
    {
        return not_found();
    }

    // Validate the request body.
    if req.program_hex.len() > 20_000 {
        return json_resp(serde_json::json!({ "ok": false, "error": "program too large" }));
    }
    let Ok(program) = hex::decode(req.program_hex.trim().trim_start_matches("0x")) else {
        return json_resp(serde_json::json!({ "ok": false, "error": "program_hex is not valid hex" }));
    };
    if program.is_empty() {
        return json_resp(serde_json::json!({ "ok": false, "error": "empty program" }));
    }
    // Value bounds: 1 .. 10 TKAS, in sompi. Keeps a runaway request from
    // draining the faucet balance into one coin (drain ceiling = global
    // refill/day × this max — see DeployLimiter).
    if req.value < 100_000_000 || req.value > 1_000_000_000 {
        return json_resp(serde_json::json!({
            "ok": false,
            "error": "value must be between 1 and 10 TKAS (given in sompi)"
        }));
    }

    // Rate limit before we touch the node.
    let ip = client_ip(&headers);
    if let Err(reason) = state.deploy_limiter.lock().await.try_take(&ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")],
            serde_json::json!({ "ok": false, "error": reason }).to_string(),
        )
            .into_response();
    }

    let keypair = match kascov_labkit::keypair_from_hex(deploy_key.trim()) {
        Ok(k) => k,
        Err(_) => return json_resp(serde_json::json!({ "ok": false, "error": "server deploy key misconfigured" })),
    };
    // Only one custodial deploy in flight — they share one funding wallet, so
    // parallel builds would select the same UTXO and collide as double-spends.
    // Error detail (labkit's rich messages embed the faucet address/balance and
    // the RPC url) is logged server-side only; clients get a fixed message.
    const DEPLOY_UNAVAILABLE: &str =
        "deploy is temporarily unavailable — the lab faucet may be low; try again in a few minutes";
    let _inflight = state.deploy_inflight.lock().await;
    let client = match kascov_labkit::connect(state.rpc.as_deref()).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("deploy: node connect failed: {e}");
            state.deploy_limiter.lock().await.refund(&ip);
            return json_resp(serde_json::json!({ "ok": false, "error": DEPLOY_UNAVAILABLE }));
        }
    };
    // Pre-flight: a drained faucet answers cheaply, reveals nothing, and
    // refunds the rate-limit token (not the caller's fault).
    match kascov_labkit::spendable_balance(&client, &keypair).await {
        Ok(available) if available < req.value + kascov_labkit::FEE => {
            tracing::warn!(
                "deploy: faucet low ({available} sompi available, {} requested)",
                req.value
            );
            state.deploy_limiter.lock().await.refund(&ip);
            return json_resp(serde_json::json!({ "ok": false, "error": DEPLOY_UNAVAILABLE }));
        }
        Err(e) => {
            tracing::warn!("deploy: balance preflight failed: {e}");
            state.deploy_limiter.lock().await.refund(&ip);
            return json_resp(serde_json::json!({ "ok": false, "error": DEPLOY_UNAVAILABLE }));
        }
        Ok(_) => {}
    }
    match kascov_labkit::deploy(&client, &keypair, &program, req.value).await {
        Ok(id) => json_resp(serde_json::json!({
            "ok": true,
            "covenant_id": id.to_string(),
            "network": network.to_string(),
        })),
        Err(e) => {
            tracing::warn!("deploy failed: {e:#}");
            json_resp(serde_json::json!({ "ok": false, "error": "deploy failed — try again in a few minutes" }))
        }
    }
}

/// POST /data/{network}/publish — compile submitted source, and if it compiles,
/// record it as a community-verified source keyed by the program's blake2b hash.
/// A coin whose revealed program hashes the same now shows the published source.
async fn publish_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    axum::Json(req): axum::Json<CompileReq>,
) -> axum::response::Response {
    let net = net_name.strip_suffix("/publish").unwrap_or(&net_name);
    let Ok(network) = net.parse::<Network>() else { return json_resp(serde_json::json!({ "ok": false, "error": "unknown network" })) };
    if !state.networks.contains(&network) || req.source.len() > 40_000 {
        return json_resp(serde_json::json!({ "ok": false, "error": "bad request" }));
    }
    let hex = match run_silverc(req.source.clone(), req.args.clone()).await {
        Ok(h) => h,
        Err(e) => return json_resp(serde_json::json!({ "ok": false, "error": e })),
    };
    let Ok(bytes) = hex::decode(&hex) else { return json_resp(serde_json::json!({ "ok": false, "error": "compiler output wasn't hex" })) };
    let hash = hex::encode(blake2b32(&bytes));
    let decoded = kascov_decode::Registry::default().decode(0, &bytes);
    let template = decoded.template.map(|t| t.to_string());
    let db = state.base_dir.join(format!("{network}.db"));
    let (source, args) = (req.source, req.args.join("\n"));
    let (hash2, tmpl2) = (hash.clone(), template.clone());
    let stored = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let store = kascov_core::store::Store::open(&db, network)?;
        store.put_verified_source(&hash2, &hex, &source, &args, tmpl2.as_deref(), now_ms())?;
        Ok(())
    })
    .await;
    match stored {
        Ok(Ok(())) => json_resp(serde_json::json!({ "ok": true, "hash": hash, "template": template })),
        _ => json_resp(serde_json::json!({ "ok": false, "error": "couldn't store the source" })),
    }
}

/// GET /data/{network}/verified/{hash} — the published source for a program hash.
async fn verified_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net, hash)): axum::extract::Path<(String, String)>,
) -> axum::response::Response {
    let Ok(network) = net.parse::<Network>() else { return json_resp(serde_json::json!({ "ok": false })) };
    if !state.networks.contains(&network) {
        return json_resp(serde_json::json!({ "ok": false }));
    }
    let db = state.base_dir.join(format!("{network}.db"));
    let hash = hash.trim_end_matches(".json").to_lowercase();
    let got = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<(String, String, Option<String>, u64)>> {
        Ok(kascov_core::store::Store::open(&db, network)?.get_verified_source(&hash)?)
    })
    .await;
    match got {
        Ok(Ok(Some((source, args, template, at)))) => json_resp(serde_json::json!({ "ok": true, "source": source, "args": args, "template": template, "verified_at": at })),
        Ok(Ok(None)) => json_resp(serde_json::json!({ "ok": false })),
        _ => json_resp(serde_json::json!({ "ok": false })),
    }
}

/// POST /data/{network}/subscribe — register a webhook for covenant events.
#[derive(serde::Deserialize)]
struct SubscribeReq {
    #[serde(default)]
    covenant_id: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    url: String,
}

async fn subscribe_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    axum::Json(req): axum::Json<SubscribeReq>,
) -> axum::response::Response {
    let net = net_name.strip_suffix("/subscribe").unwrap_or(&net_name);
    let Ok(network) = net.parse::<Network>() else { return json_resp(serde_json::json!({ "ok": false, "error": "unknown network" })) };
    if !state.networks.contains(&network) || req.url.len() > 500 || !req.url.starts_with("http") {
        return json_resp(serde_json::json!({ "ok": false, "error": "a valid http(s) url is required" }));
    }
    // A covenant filter must be exactly 64 hex chars. Anything else is a
    // client error — silently mapping bad hex to None would register an
    // accidental wildcard (all-events) subscription.
    let cid = match req.covenant_id.as_deref() {
        None => None,
        Some(s) => {
            let s = s.trim();
            let mut bytes = [0u8; 32];
            if hex::decode_to_slice(s, &mut bytes).is_err() {
                return json_resp(serde_json::json!({
                    "ok": false,
                    "error": "covenant_id must be 64 hex characters (or omitted for all events)"
                }));
            }
            Some(bytes.to_vec())
        }
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let (kind, url) = (req.kind, req.url);
    let added = tokio::task::spawn_blocking(move || -> anyhow::Result<i64> {
        let store = kascov_core::store::Store::open(&db, network)?;
        Ok(store.add_subscription(cid.as_deref(), kind.as_deref(), &url, now_ms())?)
    })
    .await;
    match added {
        Ok(Ok(id)) => json_resp(serde_json::json!({ "ok": true, "id": id })),
        _ => json_resp(serde_json::json!({ "ok": false, "error": "couldn't subscribe" })),
    }
}

/// POST /data/{network}/simulate — run a hypothetical covenant spend through
/// the real script engine (kascov-sim), off-chain. Network-agnostic (pure
/// computation); the {network} segment just keeps it under the /data rewrite.
async fn simulate_handler(
    axum::extract::Path(_net): axum::extract::Path<String>,
    axum::Json(req): axum::Json<kascov_sim::SimRequest>,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;
    if req.program_hex.len() > 20_000 {
        return (StatusCode::BAD_REQUEST, "program too large").into_response();
    }
    match tokio::task::spawn_blocking(move || kascov_sim::simulate(&req)).await {
        Ok(r) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")],
            serde_json::to_string(&r).unwrap_or_else(|_| "{}".into()),
        )
            .into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "simulation failed").into_response(),
    }
}

async fn lifespans_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let net = net_name.strip_suffix("/lifespans.json").unwrap_or(&net_name);
    let Ok(network) = net.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    if !state.networks.contains(&network) {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    }
    let db = state.base_dir.join(format!("{network}.db"));
    let cc = "public, max-age=120, s-maxage=300, stale-while-revalidate=900";
    serve_cached(&state, format!("{network}/lifespans"), 180, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        let (buckets, median_daa, total) = store.lifespan_stats()?;
        let items: Vec<_> = buckets
            .into_iter()
            .map(|(label, count)| serde_json::json!({ "label": label, "count": count }))
            .collect();
        Ok(Some(serde_json::to_string(&serde_json::json!({
            "network": network.to_string(),
            "generated_at_ms": now_ms(),
            "buckets": items,
            "median_daa": median_daa,
            "median_ms": median_daa * 100, // 10 DAA ≈ 1 s
            "total": total,
        }))?))
    })
    .await
}

async fn inscriptions_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let net = net_name.strip_suffix("/inscriptions.json").unwrap_or(&net_name);
    let Ok(network) = net.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    if !state.networks.contains(&network) {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    }
    let db = state.base_dir.join(format!("{network}.db"));
    let cc = "public, max-age=60, s-maxage=180, stale-while-revalidate=600";
    serve_cached(&state, format!("{network}/inscriptions"), 90, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        let items: Vec<_> = store
            .inscription_breakdown()?
            .into_iter()
            .map(|(label, events, coins)| serde_json::json!({ "label": label, "events": events, "covenants": coins }))
            .collect();
        Ok(Some(serde_json::to_string(&serde_json::json!({
            "network": network.to_string(),
            "generated_at_ms": now_ms(),
            "inscriptions": items,
        }))?))
    })
    .await
}

async fn lanes_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let net = net_name.strip_suffix("/lanes.json").unwrap_or(&net_name);
    let Ok(network) = net.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    if !state.networks.contains(&network) {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    }
    let db = state.base_dir.join(format!("{network}.db"));
    let cc = "public, max-age=30, s-maxage=120, stale-while-revalidate=600";
    serve_cached(&state, format!("{network}/lanes"), 60, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        let mut json_events = 0u64;
        let mut json_coins = 0u64;
        let mut lanes: Vec<serde_json::Value> = Vec::new();
        // KIP-21 user lanes: payloads shaped <4-byte namespace><16 zero bytes>,
        // stamped with their namespace at write time. Strict complement of the
        // generic tag buckets below, so no event is counted twice. (Zero rows
        // today — detection scaffolding that lights up when lane traffic lands.)
        for (hex, events, coins) in store.lane_namespaces()? {
            let bytes = hex::decode(&hex).unwrap_or_default();
            let printable = !bytes.is_empty() && bytes.iter().all(|&b| (0x20..=0x7e).contains(&b));
            let label = if printable { String::from_utf8_lossy(&bytes).into_owned() } else { format!("0x{hex}") };
            lanes.push(serde_json::json!({
                "label": label,
                "hex": hex,
                "ascii": printable,
                "kind": "lane",
                "events": events,
                "covenants": coins,
            }));
        }
        for (key, events, coins) in store.based_app_namespaces()? {
            if key == "json" || key == "jsonhex" {
                json_events += events;
                json_coins += coins;
                continue;
            }
            // key = "tag:<hex>" — a 4-byte app tag; decode printable ASCII
            let hex = key.strip_prefix("tag:").unwrap_or(&key);
            let bytes = hex::decode(hex).unwrap_or_default();
            let printable = !bytes.is_empty() && bytes.iter().all(|&b| (0x20..=0x7e).contains(&b));
            let label = if printable { String::from_utf8_lossy(&bytes).into_owned() } else { format!("0x{hex}") };
            lanes.push(serde_json::json!({
                "label": label,
                "hex": hex,
                "ascii": printable,
                "kind": "tag",
                "events": events,
                "covenants": coins,
            }));
        }
        if json_events > 0 {
            lanes.push(serde_json::json!({
                "label": "JSON inscriptions",
                "hex": serde_json::Value::Null,
                "ascii": false,
                "kind": "inscription",
                "events": json_events,
                "covenants": json_coins,
            }));
        }
        lanes.sort_by(|a, b| b["events"].as_u64().cmp(&a["events"].as_u64()));
        let tip = store.tip()?;
        Ok(Some(serde_json::to_string(&serde_json::json!({
            "network": network.to_string(),
            "generated_at_ms": now_ms(),
            "tip_daa": tip.map(|t| t.0),
            "lanes": lanes,
        }))?))
    })
    .await
}

async fn families_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let net = net_name.strip_suffix("/families.json").unwrap_or(&net_name);
    let Ok(network) = net.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    if !state.networks.contains(&network) {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    }
    let db = state.base_dir.join(format!("{network}.db"));
    let cc = "public, max-age=30, s-maxage=120, stale-while-revalidate=600";
    serve_cached(&state, format!("{network}/families"), 60, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        Ok(Some(serde_json::to_string(&build_families(&store, network)?)?))
    })
    .await
}

/// GET /data/{network}/reorgs.json — the applied virtual-chain reorg feed,
/// newest first. Reorgs are rare, so this is cached like families.
async fn reorgs_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let net = net_name.strip_suffix("/reorgs.json").unwrap_or(&net_name);
    let Ok(network) = net.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    if !state.networks.contains(&network) {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    }
    let db = state.base_dir.join(format!("{network}.db"));
    let cc = "public, max-age=30, s-maxage=120, stale-while-revalidate=600";
    serve_cached(&state, format!("{network}/reorgs"), 60, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        let reorgs = store.reorg_log(500)?;
        let out = serde_json::json!({
            "network": network.to_string(),
            "generated_at_ms": now_ms(),
            "reorgs": reorgs,
        });
        Ok(Some(serde_json::to_string(&out)?))
    })
    .await
}

/// GET /data/{network}/galaxy.json — the whole-network App Graph (precomputed
/// positions + weighted edges + status). Cached like families; independent of
/// first paint (the explorer never blocks on it).
async fn galaxy_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let net = net_name.strip_suffix("/galaxy.json").unwrap_or(&net_name);
    let Ok(network) = net.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    if !state.networks.contains(&network) {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    }
    let db = state.base_dir.join(format!("{network}.db"));
    let cc = "public, max-age=30, s-maxage=120, stale-while-revalidate=600";
    serve_cached(&state, format!("{network}/galaxy"), 60, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        Ok(Some(serde_json::to_string(&build_galaxy(&store, network)?)?))
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
    let result = tokio::task::spawn_blocking(move || -> Result<Vec<kascov_core::CovenantId>> {
        let store = kascov_core::store::Store::open(&db, network)?;
        Ok(store.covenants_by_txid(&txid)?)
    })
    .await;
    let ok_headers = [
        (header::CONTENT_TYPE, "application/json; charset=utf-8"),
        (header::CACHE_CONTROL, "public, max-age=60, s-maxage=300"),
        (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
    ];
    match result {
        Ok(Ok(ids)) if !ids.is_empty() => (
            ok_headers,
            // covenant_id stays for existing consumers; covenant_ids is the full set
            serde_json::json!({ "txid": tx_hex, "covenant_id": ids[0], "covenant_ids": ids }).to_string(),
        )
            .into_response(),
        Ok(Ok(_)) => (
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

/// The last 24 hours as one small object — counts, value born, and the
/// headline coins. A daily summary moves slowly; the CDN absorbs the herd.
async fn digest_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    let Ok(network) = net_name.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    if !state.networks.contains(&network) {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    }

    let db = state.base_dir.join(format!("{network}.db"));
    let key = format!("{network}/digest");
    let cc = "public, max-age=60, s-maxage=300, stale-while-revalidate=600";
    serve_cached(&state, key, 60, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        Ok(Some(serde_json::to_string(&build_digest(&store, network)?)?))
    })
    .await
}

/// Contract-type analytics: what runs on this network, by recognized
/// script template. Slow-moving and cheap to rebuild (two GROUP BYs), so
/// the CDN absorbs essentially all traffic.
async fn templates_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    let Ok(network) = net_name.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    if !state.networks.contains(&network) {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    }

    let db = state.base_dir.join(format!("{network}.db"));
    let key = format!("{network}/templates");
    let cc = "public, max-age=30, s-maxage=60, stale-while-revalidate=300";
    serve_cached(&state, key, 60, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        Ok(Some(serde_json::to_string(&build_templates_snapshot(&store, network)?)?))
    })
    .await
}

/// Kind counts per DAA bucket for the interactive activity chart.
/// ?range= is whitelisted; unknown values are a 400, absent means 24h.
async fn activity_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let Ok(network) = net_name.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    // whitelist → &'static str, so the closure needs no owned copy
    let range: &'static str = match q.get("range").map(String::as_str) {
        None | Some("24h") => "24h",
        Some("1h") => "1h",
        Some("6h") => "6h",
        Some("48h") => "48h",
        Some("all") => "all",
        Some(_) => {
            return (
                StatusCode::BAD_REQUEST,
                [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
                "unknown range — use 1h | 6h | 24h | 48h | all",
            )
                .into_response()
        }
    };
    if !state.networks.contains(&network) {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    }

    let db = state.base_dir.join(format!("{network}.db"));
    let key = format!("{network}/activity/{range}");
    let cc = "public, max-age=15, s-maxage=60, stale-while-revalidate=300";
    serve_cached(&state, key, 30, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        Ok(Some(serde_json::to_string(&build_activity_snapshot(&store, network, range)?)?))
    })
    .await
}

fn addr_prefix(network: Network) -> kaspa_addresses::Prefix {
    match network {
        Network::Mainnet => kaspa_addresses::Prefix::Mainnet,
        Network::Testnet(_) => kaspa_addresses::Prefix::Testnet,
    }
}

/// `kaspa:…`/`kaspatest:…` (any known prefix — pubkeys are network-independent)
/// or raw 32/33-byte pubkey hex. Returns (canonical address re-encoded for the
/// queried network, pubkey bytes). Script-hash addresses carry no pubkey -> None.
fn parse_addr_or_pubkey(raw: &str, network: Network) -> Option<(String, Vec<u8>)> {
    use kaspa_addresses::{Address, Version};
    let (version, pubkey) = if raw.contains(':') {
        let addr = Address::try_from(raw).ok()?; // validates charset + checksum
        if !matches!(addr.version, Version::PubKey | Version::PubKeyECDSA) {
            return None;
        }
        (addr.version, addr.payload.to_vec())
    } else {
        let bytes = hex::decode(raw).ok()?;
        let version = match bytes.len() {
            32 => Version::PubKey,
            33 => Version::PubKeyECDSA,
            _ => return None,
        };
        (version, bytes)
    };
    if pubkey.len() != version.public_key_len() {
        return None;
    }
    Some((Address::new(addr_prefix(network), version, &pubkey).to_string(), pubkey))
}

/// Which smart coins has this address/pubkey touched (as a p2pk-state owner)?
async fn addr_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, address)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let Ok(network) = net_name.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    let raw = address.strip_suffix(".json").unwrap_or(&address);
    let Some((canonical, pubkey)) = parse_addr_or_pubkey(raw, network) else {
        return (
            StatusCode::BAD_REQUEST,
            [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
            "expected a kaspa address or 32/33-byte pubkey hex",
        )
            .into_response();
    };
    if !state.networks.contains(&network) {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    }

    let db = state.base_dir.join(format!("{network}.db"));
    // pubkey hex normalizes the cache key: address form and hex form share one entry
    let key = format!("{network}/addr/{}", hex::encode(&pubkey));
    let cc = "public, max-age=10, s-maxage=30, stale-while-revalidate=120";
    serve_cached(&state, key, 20, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        let rows = store.covenants_by_pubkey(&pubkey)?;
        let total = rows.len();
        let tip = store.tip()?;
        let mut covenants = Vec::with_capacity(rows.len().min(ADDR_MAX_COVENANTS));
        for r in rows.iter().take(ADDR_MAX_COVENANTS) {
            let Some(c) = store.summary(&r.covenant_id)? else { continue };
            covenants.push(serde_json::json!({
                // grid-row shape — keep in sync with build_grid_snapshot
                "covenant_id": c.covenant_id,
                "status": if c.live_utxos > 0 { "active" } else { "burned" },
                "genesis_daa": c.genesis_daa,
                "lineage_complete": c.lineage_complete,
                "event_count": c.event_count,
                "last_activity_daa": c.last_activity_daa,
                "live_utxos": c.live_utxos,
                "live_value": c.live_value,
                "born_value": store.born_value(&c.covenant_id)?,
                // …plus this key's role in it
                "controls_now": r.controls_now,
                "states_seen": r.states_seen,
                "first_seen_daa": r.first_seen_daa,
                "last_seen_daa": r.last_seen_daa,
            }));
        }
        Ok(Some(serde_json::to_string(&serde_json::json!({
            "network": network.to_string(),
            "generated_at_ms": now_ms(),
            "tip_daa": tip.map(|t| t.0),
            "tip_at_ms": tip.map(|t| t.1),
            "address": canonical,
            "pubkey": hex::encode(&pubkey),
            "covenants_total": total,
            "covenants": covenants,
        }))?))
    })
    .await
}

/// Push covenant events over SSE the moment the follower indexes them.
/// Hints only — no replay, no backlog, lagged subscribers skip ahead;
/// consumers confirm state through the polled feeds.
async fn stream_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::http::{header, HeaderName, HeaderValue, StatusCode};
    use axum::response::sse::{Event, KeepAlive, Sse};
    use axum::response::IntoResponse;
    use std::sync::atomic::Ordering;

    let Ok(network) = net_name.parse::<Network>() else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    let Some((_, channel)) = state.live.iter().find(|(n, _)| *n == network) else {
        return (StatusCode::NOT_FOUND, "unknown network").into_response();
    };
    // Reserve a subscriber slot; back out over the cap.
    if channel.subscribers.fetch_add(1, Ordering::AcqRel) >= MAX_STREAM_SUBSCRIBERS {
        channel.subscribers.fetch_sub(1, Ordering::AcqRel);
        return (StatusCode::SERVICE_UNAVAILABLE, "stream full — use the polling feeds").into_response();
    }
    let slot = SubscriberSlot(channel.subscribers.clone());
    let rx = channel.tx.subscribe();

    // broadcast::Receiver is not a Stream; unfold avoids a tokio-stream dep.
    // The slot rides in the state so disconnects free it via Drop. Streams
    // also carry a hard lifetime: a client that connects and never reads
    // would otherwise pin a subscriber slot forever (keep-alives sink into
    // TCP buffers without erroring) — after the deadline the stream ends
    // cleanly and well-behaved clients (EventSource) reconnect on their own.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15 * 60);
    let stream = futures::stream::unfold((rx, slot), move |(mut rx, slot)| async move {
        loop {
            match tokio::time::timeout_at(deadline, rx.recv()).await {
                Ok(Ok(msg)) => {
                    let event = Event::default().data(&*msg);
                    return Some((Ok::<_, std::convert::Infallible>(event), (rx, slot)));
                }
                // Fell behind the buffer: skip ahead — clients resync by polling.
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return None,
                // Lifetime reached — recycle the slot.
                Err(_) => return None,
            }
        }
    });

    let mut resp = Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(25)).text("ka"))
        .into_response();
    let headers = resp.headers_mut();
    // no-store beats axum's default no-cache: the CDN must never coalesce a stream
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    // ask proxies not to buffer (nginx-style hint; Firebase may ignore it)
    headers.insert(HeaderName::from_static("x-accel-buffering"), HeaderValue::from_static("no"));
    resp
}

#[cfg(test)]
mod galaxy_tests {
    use super::*;
    use kascov_core::store::{BlockEvents, EventKind, NewEvent, NewUtxo, Store};
    use kascov_core::{BlockHash, CovenantId, Network, Outpoint, TxId};

    fn ev(cov: u8, kind: EventKind, tx: u8) -> NewEvent {
        NewEvent { covenant_id: CovenantId([cov; 32]), kind, txid: TxId([tx; 32]), payload: None, lane_namespace: None }
    }

    // A synthetic index with two "apps": {A1,B2} share tx 0x10, and
    // {C3,D4,E5} share tx 0x20; a lone F6 is a size-1 cluster (excluded).
    // A1 gets a live utxo so it reads as active.
    fn galaxy_store() -> serde_json::Value {
        let path = std::env::temp_dir().join(format!("kascov-galaxy-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut store = Store::open(&path, Network::Testnet(10)).unwrap();
        let block = BlockEvents {
            accepting_block: BlockHash([1; 32]),
            accepting_daa: 100,
            events: vec![
                ev(0xA1, EventKind::Genesis, 0x10),
                ev(0xB2, EventKind::Genesis, 0x10),
                ev(0xC3, EventKind::Genesis, 0x20),
                ev(0xD4, EventKind::Genesis, 0x20),
                ev(0xE5, EventKind::Genesis, 0x20),
                ev(0xF6, EventKind::Genesis, 0x30),
            ],
            created_utxos: vec![NewUtxo {
                outpoint: Outpoint { txid: TxId([0x10; 32]), index: 0 },
                covenant_id: CovenantId([0xA1; 32]),
                value: 1_000_000_000,
                spk_version: 0,
                spk_script: vec![],
            }],
            spent_utxos: vec![],
        };
        store.apply(&block, BlockHash([1; 32])).unwrap();
        build_galaxy(&store, Network::Testnet(10)).unwrap()
    }

    #[test]
    fn galaxy_clusters_nodes_and_edges() {
        let g = galaxy_store();
        // two apps (size>=2), five member nodes (F6 excluded)
        assert_eq!(g["apps"].as_array().unwrap().len(), 2);
        assert_eq!(g["nodes"].as_array().unwrap().len(), 5);
        // edges: {A1,B2}=1 pair, {C3,D4,E5}=3 pairs -> 4 weighted edges
        assert_eq!(g["edges"].as_array().unwrap().len(), 4);
        assert_eq!(g["edges_total"].as_u64().unwrap(), 4);

        // node shape + status wiring: exactly one node is active (A1's utxo)
        let nodes = g["nodes"].as_array().unwrap();
        let active = nodes.iter().filter(|n| n["s"].as_i64() == Some(1)).count();
        assert_eq!(active, 1);
        for n in nodes {
            assert_eq!(n["id"].as_str().unwrap().len(), 64); // hex covenant id
            for k in ["t", "s", "x", "y", "r", "a"] {
                assert!(n.get(k).is_some(), "node missing {k}");
            }
        }
        // apps sorted biggest-first; each edge references valid node indices
        assert_eq!(g["apps"][0]["size"].as_u64().unwrap(), 3);
        for e in g["edges"].as_array().unwrap() {
            let (a, b) = (e[0].as_u64().unwrap(), e[1].as_u64().unwrap());
            assert!((a as usize) < nodes.len() && (b as usize) < nodes.len());
            assert!(e[2].as_u64().unwrap() >= 1); // weight
        }
        // bounds present and finite
        for k in ["minx", "miny", "w", "h"] {
            assert!(g["bounds"].get(k).is_some(), "bounds missing {k}");
        }
    }
}
