mod og;

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

/// Hard ceiling on one grid page — also the size of the bare (param-less)
/// response, which is a first page with a continuation cursor rather than the
/// whole table (168k rows would be tens of MB in flight).
const MAX_PAGE: u64 = 20_000;

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
    // A caller that passes `?after_daa=`/`?limit=` opts into a page window
    // ordered by `last_activity_daa DESC`, default 5000 most-recent. A bare
    // request is the same shape, just a MAX_PAGE-sized first page: small nets
    // still fit in one response, and when more rows remain `next_after_daa`/
    // `next_after_id` are set so any consumer can keep walking.
    const DEFAULT_PAGE: u64 = 5000;
    let paged = after.is_some() || limit.is_some();
    let mut next_after_daa: Option<u64> = None;
    let mut next_after_id: Option<String> = None;
    let page = if paged { limit.unwrap_or(DEFAULT_PAGE).max(1) } else { MAX_PAGE };
    // Over-fetch by one to detect whether another page exists.
    let mut covenants = store.list_page(after, page.saturating_add(1))?;
    if covenants.len() as u64 > page {
        covenants.truncate(page as usize);
        if let Some(last) = covenants.last() {
            next_after_daa = Some(last.last_activity_daa);
            next_after_id = Some(last.covenant_id.to_string());
        }
    }
    let tip = store.tip()?;
    let rows: Vec<_> = covenants
        .iter()
        .map(|c| {
            serde_json::json!({
                "covenant_id": c.covenant_id,
                "name": og::friendly_name(&c.covenant_id.to_string()),
                "status": if c.live_utxos > 0 { "active" } else { "burned" },
                "genesis_daa": c.genesis_daa,
                "lineage_complete": c.lineage_complete,
                "event_count": c.event_count,
                "last_activity_daa": c.last_activity_daa,
                "live_utxos": c.live_utxos,
                "live_value": c.live_value,
                "born_value": c.born_value,
                "template": c.template,
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
    let obj = detail.as_object_mut().context("covenant json is not an object")?;
    obj.insert("network".into(), serde_json::json!(network.to_string()));
    obj.insert(
        "name".into(),
        serde_json::json!(og::friendly_name(&summary.covenant_id.to_string())),
    );
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
    let mut event_rows = Vec::with_capacity(events.len().min(max_events as usize));
    for e in events.iter().take(max_events as usize) {
        let mut v = serde_json::to_value(e).context("event serializes")?;
        // based-app payloads can be large; the snapshot inlines small ones only
        if let Some(p) = &e.payload {
            if p.len() > 512 {
                v.as_object_mut().context("event json is not an object")?.remove("payload");
                v["payload_len"] = serde_json::json!(p.len());
            }
        }
        // multi-covenant transactions: name the other coins this tx moved
        if let Ok(others) = store.covenants_by_txid(&e.txid) {
            let with: Vec<_> =
                others.into_iter().filter(|c| c != &summary.covenant_id).take(4).collect();
            if !with.is_empty() {
                v["with_covenants"] = serde_json::json!(with);
            }
        }
        event_rows.push(v);
    }
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
        "events": event_rows,
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
            SyncUpdate::Event { covenant_id, kind, txid, accepting_daa, tx_index } => {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "type": "event", "kind": kind, "covenant_id": covenant_id,
                            "txid": txid, "accepting_daa": accepting_daa, "tx_index": tx_index,
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

/// Per-network follower liveness, shared with /healthz. Epoch ms of the last
/// successful sync pass; initialized to boot time so a fresh instance gets the
/// same 10-minute grace as a healthy one.
struct SyncHealth {
    last_sync_ok_ms: std::sync::atomic::AtomicI64,
}

struct ServeState {
    base_dir: std::path::PathBuf,
    networks: Vec<Network>,
    max_events: u64,
    /// Node url for the custodial deploy endpoint (None → public resolver).
    rpc: Option<String>,
    /// Rate limiter shared by the custodial /deploy endpoint.
    deploy_limiter: tokio::sync::Mutex<DeployLimiter>,
    /// Rate limiter shared by the compiler-adjacent endpoints
    /// (/compile, /publish, /zk-verify).
    tool_limiter: tokio::sync::Mutex<ToolLimiter>,
    /// Follower liveness per network (same Vec-not-HashMap shape as `live`).
    sync_health: Vec<(Network, std::sync::Arc<SyncHealth>)>,
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
    /// Per-network search index (friendly names + templates), keyed by the
    /// network name. `(built_at, covenant_count, index)` — the count is the
    /// cheap staleness probe (ids are append-only). A std Mutex because it's
    /// taken inside spawn_blocking; held only for map lookups, never builds.
    search_index: std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, u64, std::sync::Arc<SearchIndex>)>>,
}

/// Parse a `{network}` path segment and require it to be a network this
/// worker follows. `Err` carries the ready-made 404 response, so handlers
/// `return` it as-is.
fn resolve_network(
    state: &ServeState,
    raw: &str,
) -> std::result::Result<Network, axum::response::Response> {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    match raw.parse::<Network>() {
        Ok(network) if state.networks.contains(&network) => Ok(network),
        _ => Err((StatusCode::NOT_FOUND, "unknown network").into_response()),
    }
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
    let mut sync_health = Vec::with_capacity(networks.len());
    for &network in &networks {
        let channel = LiveChannel::new();
        let health = std::sync::Arc::new(SyncHealth {
            last_sync_ok_ms: std::sync::atomic::AtomicI64::new(now_ms() as i64),
        });
        let db = base_dir.join(format!("{network}.db"));
        // Webhook delivery rides the same event callback as SSE: the follower
        // try_sends into this queue and a per-network task does the POSTs.
        let (hook_tx, hook_rx) = tokio::sync::mpsc::channel::<HookEvent>(HOOK_QUEUE);
        tokio::spawn(webhook_delivery_forever(network, db.clone(), hook_rx));
        tokio::spawn(follow_forever(
            network,
            cli.rpc.clone(),
            db,
            channel.tx.clone(),
            hook_tx,
            health.clone(),
        ));
        live.push((network, channel));
        sync_health.push((network, health));
    }

    let state = std::sync::Arc::new(ServeState {
        base_dir,
        networks,
        max_events,
        rpc: cli.rpc.clone(),
        deploy_limiter: tokio::sync::Mutex::new(DeployLimiter::new()),
        tool_limiter: tokio::sync::Mutex::new(ToolLimiter::new()),
        sync_health,
        deploy_inflight: tokio::sync::Mutex::new(()),
        live,
        cache: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        build_locks: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        search_index: std::sync::Mutex::new(std::collections::HashMap::new()),
    });
    // Galaxy keep-warm: a build costs ~5-8s at production scale, and the
    // section reads as "broken" when a visitor pays that at the door (the
    // user-reported 10s blank canvas). Rebuild the two variants the frontend
    // actually requests (?fmt=2&tier=core for first paint, ?fmt=2 for the
    // hot-swap) every ~4min per network so the cache never goes cold — data
    // staleness ≤4min is fine for a network-wide visualization. Runs inside
    // spawn_blocking; ~5% of one core on the busiest testnet.
    {
        let state = state.clone();
        tokio::spawn(async move {
            // First tick held back 90s: a fresh instance must answer cheap
            // requests before it pays 2×networks galaxy builds (boot storm).
            let mut tick = tokio::time::interval_at(
                tokio::time::Instant::now() + std::time::Duration::from_secs(90),
                std::time::Duration::from_secs(240),
            );
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                for &network in &state.networks {
                    let db = state.base_dir.join(format!("{network}.db"));
                    if !db.exists() {
                        continue;
                    }
                    for core_only in [true, false] {
                        let fmt = GalaxyFmt { columnar: true, core_only };
                        let db = db.clone();
                        let built = tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
                            let store = kascov_core::store::Store::open(&db, network)?;
                            Ok(serde_json::to_string(&build_galaxy_fmt(&store, network, fmt)?)?)
                        })
                        .await;
                        match built {
                            Ok(Ok(json)) => {
                                let key = format!("{network}/galaxy?fmt=1&tier={}", core_only as u8);
                                state.cache.lock().await.insert(
                                    key,
                                    (std::time::Instant::now(), std::sync::Arc::new(CachedBody::new(json))),
                                );
                            }
                            Ok(Err(e)) => tracing::warn!("{network}: galaxy keep-warm build failed: {e}"),
                            Err(e) => tracing::warn!("{network}: galaxy keep-warm task failed: {e}"),
                        }
                    }
                }
            }
        });
    }
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
        // Google Front End swallows /healthz on *.run.app before it reaches
        // the container — /health is the path that actually works in prod.
        .route("/healthz", get(healthz_handler))
        .route("/health", get(healthz_handler))
        .route("/data/{network}/simulate", post(simulate_handler))
        .route("/data/{network}/zk-verify", post(zk_verify_handler))
        .route("/data/{network}/compile", post(compile_handler))
        .route("/data/{network}/deploy", post(deploy_handler))
        .route("/data/{network}/publish", post(publish_handler))
        .route("/data/{network}/verified/{hash}", get(verified_handler))
        .route("/data/{network}/subscribe", post(subscribe_handler))
        .route("/data/{network}/unsubscribe", post(unsubscribe_handler))
        .route("/data/{network}/lane/{ns}", get(lane_handler))
        .route("/data/{network}/debug/{txid}", get(debug_handler))
        // static path beats the {file} capture below (axum route priority)
        .route("/data/price.json", get(price_handler))
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
        .route("/data/{network}/tokens.json", get(tokens_handler))
        .route("/data/{network}/events", get(events_handler))
        .route("/data/{network}/coins", get(coins_handler))
        .route("/data/{network}/activity.json", get(activity_handler))
        .route("/data/{network}/addr/{address}", get(addr_handler))
        .route("/data/{network}/search", get(search_handler))
        .route("/data/{network}/stream", get(stream_handler))
        // share surface: crawler-visible per-coin pages (the SPA is
        // hash-routed, so scrapers never see #/… urls) + PNG OG cards
        // (Facebook/X reject SVG og:images) + the sitemap that feeds them.
        .route("/og/{network}/{id}", get(og_card_handler))
        .route("/share/{network}/{id}", get(share_handler))
        .route("/sitemap.xml", get(sitemap_handler))
        // compresses the small dynamic responses; the big cached bodies are
        // pre-gzipped (Content-Encoding already set, so this layer skips them)
        .layer(tower_http::compression::CompressionLayer::new())
        // browsers preflight the JSON POSTs (compile/publish/subscribe/…) with
        // OPTIONS, which a post-only route would 405. This layer answers the
        // preflight and stamps the same open policy the GETs already send by
        // hand (its header replaces, not duplicates, any manual ACAO).
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([axum::http::header::CONTENT_TYPE])
                .max_age(std::time::Duration::from_secs(3600)),
        )
        .with_state(state);

    eprintln!("kascov worker listening on {listen}");
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// How stale the newest successful sync pass may be before /healthz reports
/// "stalled" and answers 503 (the uptime check's restart signal).
const HEALTHZ_STALL_MS: i64 = 10 * 60 * 1000;

/// GET /healthz — follower liveness + index progress per network. 503 as soon
/// as ANY followed network hasn't completed a sync pass in HEALTHZ_STALL_MS.
async fn healthz_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let now = now_ms() as i64;
    let mut stalled = false;
    let mut networks = serde_json::Map::new();
    for &network in &state.networks {
        let last_ok = state
            .sync_health
            .iter()
            .find(|(n, _)| *n == network)
            .map(|(_, h)| h.last_sync_ok_ms.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(0);
        stalled |= now.saturating_sub(last_ok) > HEALTHZ_STALL_MS;
        let db = state.base_dir.join(format!("{network}.db"));
        // Nulls until the follower has created the DB; an open/read failure
        // degrades to the same nulls rather than failing the whole probe.
        let indexed = if db.exists() {
            tokio::task::spawn_blocking(move || -> Result<(Option<u64>, Option<u64>, bool)> {
                let store = kascov_core::store::Store::open(&db, network)?;
                Ok((
                    store.processed_daa()?,
                    store.tip()?.map(|t| t.0),
                    store.tx_index_backfill_done()?,
                ))
            })
            .await
            .ok()
            .and_then(|r| r.ok())
        } else {
            None
        };
        let (processed, tip, backfill_done) = indexed.unwrap_or((None, None, false));
        networks.insert(
            network.to_string(),
            serde_json::json!({
                "processed_daa": processed,
                "tip_daa": tip,
                "lag_daa": tip.zip(processed).map(|(t, p)| t.saturating_sub(p)),
                "last_sync_ok_ms": last_ok,
                "tx_index_backfill_done": backfill_done,
            }),
        );
    }
    let code = if stalled { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::OK };
    (
        code,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
        ],
        serde_json::json!({
            "status": if stalled { "stalled" } else { "ok" },
            "networks": networks,
        })
        .to_string(),
    )
        .into_response()
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

/// A pending tx_index backfill re-fetches every retained block over RPC —
/// heavy enough to starve a booting instance. Hold it back this long after
/// boot; a completed backfill (the steady state) skips the wait entirely.
const TX_BACKFILL_BOOT_DELAY: std::time::Duration = std::time::Duration::from_secs(120);

/// Follow a network's virtual chain forever, reconnecting on any failure.
async fn follow_forever(
    network: Network,
    rpc: Option<String>,
    db: std::path::PathBuf,
    live_tx: tokio::sync::broadcast::Sender<std::sync::Arc<str>>,
    hook_tx: tokio::sync::mpsc::Sender<HookEvent>,
    health: std::sync::Arc<SyncHealth>,
) {
    use kascov_core::sync::SyncUpdate;
    // This task is spawned once per network at boot, so "task start" = boot.
    let boot = tokio::time::Instant::now();
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
        // One-shot per database: stamp tx_index onto pre-capture event rows
        // still inside node retention. Best-effort — a failed walk resumes
        // next session and never blocks following the chain.
        // Boot-storm guard: when the one-shot still has work, hold it (and
        // this network's first follow) until the instance has been serving
        // for a while — requests come first, heavy background work second.
        if !store.tx_index_backfill_done().unwrap_or(true) {
            let since_boot = boot.elapsed();
            if since_boot < TX_BACKFILL_BOOT_DELAY {
                tokio::time::sleep(TX_BACKFILL_BOOT_DELAY - since_boot).await;
            }
        }
        match kascov_core::sync::backfill_tx_index(&node, &mut store).await {
            Ok(0) => {}
            Ok(n) => tracing::info!("{network}: tx_index backfill stamped {n} event rows"),
            Err(err) => tracing::warn!("{network}: tx_index backfill interrupted ({err}) — will resume next session"),
        }
        tracing::info!("{network}: following the chain");
        loop {
            let result = kascov_core::sync::sync_once(&node, &mut store, None, |update| match update {
                SyncUpdate::Event { covenant_id, kind, txid, accepting_daa, tx_index } => {
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
                            "tx_index": tx_index,
                        })
                        .to_string();
                        let _ = live_tx.send(msg.into());
                    }
                    // Webhook queue: try_send so a slow/stalled delivery task
                    // can never block the indexer — under backpressure (e.g.
                    // the initial full sync) extra events are dropped, which
                    // is fine: webhooks are hints, not a durable feed.
                    let _ = hook_tx.try_send(HookEvent {
                        covenant_id,
                        kind: kind.as_str(),
                        txid,
                        accepting_daa,
                        tx_index,
                    });
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
                    health
                        .last_sync_ok_ms
                        .store(now_ms() as i64, std::sync::atomic::Ordering::Relaxed);
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

/// Webhook delivery queue depth per network. Full queue = events dropped
/// (webhooks are best-effort hints; the polled feeds are the truth).
const HOOK_QUEUE: usize = 1024;
/// Consecutive delivery failures before a subscription is deleted.
const WEBHOOK_MAX_FAILURES: u32 = 10;

/// One covenant event bound for webhook delivery.
struct HookEvent {
    covenant_id: CovenantId,
    kind: &'static str,
    txid: TxId,
    accepting_daa: u64,
    tx_index: u32,
}

/// Is this IP off-limits for webhook POSTs? Loopback, RFC1918 private,
/// link-local (incl. the 169.254.169.254 cloud metadata endpoint), CGNAT,
/// unspecified/broadcast, IPv6 unique-local (fc00::/7) and link-local
/// (fe80::/10) — anything that would let a subscription URL reach the
/// worker's own network instead of the public internet.
fn ip_is_forbidden(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()          // 127.0.0.0/8
                || v4.is_private()    // 10/8, 172.16/12, 192.168/16
                || v4.is_link_local() // 169.254/16 (metadata service)
                || v4.is_unspecified()
                || v4.is_broadcast()
                || o[0] == 0 // 0.0.0.0/8 ("this network")
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64/10 CGNAT
                || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24 IETF
        }
        std::net::IpAddr::V6(v6) => {
            // IPv4-mapped (::ffff:a.b.c.d) inherits the V4 verdict.
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return ip_is_forbidden(std::net::IpAddr::V4(mapped));
            }
            let seg = v6.segments();
            v6.is_loopback()                  // ::1
                || v6.is_unspecified()        // ::
                || (seg[0] & 0xfe00) == 0xfc00 // fc00::/7 unique local
                || (seg[0] & 0xffc0) == 0xfe80 // fe80::/10 link local
        }
    }
}

/// SSRF pre-flight for a webhook URL: http(s) only, and every address the
/// host resolves to must be public. Blocking (std DNS) — call it off the
/// async runtime. Best effort by nature: a DNS rebind between this check and
/// reqwest's own resolution can still slip through, so the egress network
/// policy remains the real backstop.
fn webhook_target_allowed(url: &str) -> std::result::Result<(), &'static str> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "unparseable url")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("only http(s) urls are delivered");
    }
    let host = parsed.host_str().ok_or("url has no host")?;
    let port = parsed.port_or_known_default().ok_or("url has no port")?;
    // Literal IPs (host_str keeps IPv6 brackets) skip DNS entirely.
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        return if ip_is_forbidden(ip) { Err("address is private/internal") } else { Ok(()) };
    }
    use std::net::ToSocketAddrs;
    let mut addrs = (bare, port).to_socket_addrs().map_err(|_| "host does not resolve")?.peekable();
    if addrs.peek().is_none() {
        return Err("host does not resolve");
    }
    if addrs.any(|a| ip_is_forbidden(a.ip())) {
        return Err("host resolves to a private/internal address");
    }
    Ok(())
}

/// The delivery signature: keyed BLAKE2b-256 over the exact POST body, keyed
/// with the subscription secret's ASCII bytes (the hex string as handed out
/// by /subscribe — no decoding step for the verifier to get wrong). BLAKE2's
/// keyed mode is a MAC by construction, so the blake2b already in-tree
/// covers this without an HMAC dependency.
fn webhook_signature(secret: &str, body: &str) -> String {
    hex::encode(
        blake2b_simd::Params::new()
            .hash_length(32)
            .key(secret.as_bytes())
            .hash(body.as_bytes())
            .as_bytes(),
    )
}

/// POST one event to one subscriber: SSRF pre-flight, then up to 3 attempts
/// with exponential backoff (1s, 2s between attempts). True iff a 2xx landed.
/// `body` is the pre-serialized JSON — the signature must cover the exact
/// bytes on the wire. Legacy subscriptions (no secret) are sent unsigned.
async fn deliver_webhook(
    client: &reqwest::Client,
    url: &str,
    body: &str,
    secret: Option<&str>,
) -> bool {
    // The guard does blocking DNS — keep it off the runtime workers. A
    // rejected target counts as a failure, so a private URL that slipped into
    // the store retires itself after WEBHOOK_MAX_FAILURES events.
    let check_url = url.to_string();
    let allowed = tokio::task::spawn_blocking(move || webhook_target_allowed(&check_url))
        .await
        .unwrap_or(Err("ssrf guard panicked"));
    if let Err(reason) = allowed {
        tracing::warn!("webhook {url}: rejected ({reason})");
        return false;
    }
    let signature = secret.map(|s| webhook_signature(s, body));
    for attempt in 0u32..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500u64 << attempt)).await;
        }
        let mut req = client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string());
        if let Some(sig) = &signature {
            req = req.header("X-Kascov-Signature", sig.as_str());
        }
        match req.send().await {
            Ok(resp) if resp.status().is_success() => return true,
            Ok(resp) => tracing::debug!("webhook {url}: attempt {} got {}", attempt + 1, resp.status()),
            Err(err) => tracing::debug!("webhook {url}: attempt {} failed: {err}", attempt + 1),
        }
    }
    false
}

/// Per-network webhook delivery: drain the event queue, look up matching
/// subscriptions, POST to each. Sequential by design — a per-url failure
/// counter (in memory; resets on restart) retires subscriptions that fail
/// WEBHOOK_MAX_FAILURES deliveries in a row.
async fn webhook_delivery_forever(
    network: Network,
    db: std::path::PathBuf,
    mut rx: tokio::sync::mpsc::Receiver<HookEvent>,
) {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("kascov-webhook/0.1")
        .build()
    {
        Ok(c) => c,
        Err(err) => {
            tracing::error!("{network}: webhook client unavailable ({err}) — delivery disabled");
            return;
        }
    };
    let mut failures: std::collections::HashMap<i64, u32> = std::collections::HashMap::new();
    // "Anyone subscribed at all?" probe, cached 10s, so the initial full sync
    // (hundreds of thousands of events) doesn't open the DB once per event.
    let mut subs_probe: Option<(std::time::Instant, bool)> = None;
    while let Some(ev) = rx.recv().await {
        let stale = subs_probe.is_none_or(|(at, _)| at.elapsed() > std::time::Duration::from_secs(10));
        if stale {
            let db = db.clone();
            let any = tokio::task::spawn_blocking(move || -> Result<bool> {
                let store = Store::open(&db, network)?;
                Ok(store.subscription_count()? > 0)
            })
            .await;
            subs_probe = Some((std::time::Instant::now(), matches!(any, Ok(Ok(true)))));
        }
        if !subs_probe.map(|(_, any)| any).unwrap_or(false) {
            continue;
        }
        let subs = {
            let db = db.clone();
            let cid = ev.covenant_id;
            let kind = ev.kind;
            tokio::task::spawn_blocking(move || -> Result<Vec<(i64, String, Option<String>)>> {
                let store = Store::open(&db, network)?;
                Ok(store.subscriptions_matching(cid.0.as_slice(), kind)?)
            })
            .await
        };
        let Ok(Ok(subs)) = subs else { continue };
        if subs.is_empty() {
            continue;
        }
        // Serialized once: every subscriber gets (and signs over) these bytes.
        let body = serde_json::json!({
            "network": network.to_string(),
            "covenant_id": ev.covenant_id,
            "kind": ev.kind,
            "txid": ev.txid,
            "accepting_daa": ev.accepting_daa,
            "tx_index": ev.tx_index,
        })
        .to_string();
        for (id, url, secret) in subs {
            if deliver_webhook(&client, &url, &body, secret.as_deref()).await {
                failures.remove(&id);
                continue;
            }
            let n = failures.entry(id).or_insert(0);
            *n += 1;
            if *n >= WEBHOOK_MAX_FAILURES {
                failures.remove(&id);
                let db = db.clone();
                let deleted = tokio::task::spawn_blocking(move || -> Result<bool> {
                    let store = Store::open(&db, network)?;
                    Ok(store.delete_subscription(id)?)
                })
                .await;
                tracing::warn!(
                    "{network}: webhook subscription {id} ({url}) removed after {WEBHOOK_MAX_FAILURES} consecutive failures (deleted: {})",
                    matches!(deleted, Ok(Ok(true)))
                );
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
        body = fresh_body(&*state.cache.lock().await);
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

/// How long a fetched KAS/USD price is served from the in-process cache.
const PRICE_TTL_OK: std::time::Duration = std::time::Duration::from_secs(60);
/// How long a total fetch failure short-circuits to 503 before retrying —
/// a failure must never be pinned longer than this.
const PRICE_TTL_ERR: std::time::Duration = std::time::Duration::from_secs(30);

/// The last price fetch: when it ran and the serialized response body
/// (None = every provider failed).
struct PriceState {
    fetched_at: std::time::Instant,
    body: Option<String>,
}

fn price_cache() -> &'static tokio::sync::Mutex<Option<PriceState>> {
    static CACHE: std::sync::OnceLock<tokio::sync::Mutex<Option<PriceState>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::Mutex::new(None))
}

/// Kraken public ticker: `{"error":[],"result":{"KASUSD":{"c":["0.0777",…]…}}}`
/// — the last-trade price is `c[0]`. The pair key is read from the result map
/// rather than hardcoded (Kraken is known to alias pair names).
fn parse_kraken_price(body: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    if v["error"].as_array().is_some_and(|e| !e.is_empty()) {
        return None;
    }
    let price = v["result"].as_object()?.values().next()?["c"][0].as_str()?.parse::<f64>().ok()?;
    (price.is_finite() && price > 0.0).then_some(price)
}

/// CoinGecko simple price: `{"kaspa":{"usd":0.0777}}`.
fn parse_coingecko_price(body: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let price = v["kaspa"]["usd"].as_f64()?;
    (price.is_finite() && price > 0.0).then_some(price)
}

/// KAS/USD spot from Kraken, falling back to CoinGecko. Fixed URLs only —
/// no user input reaches the fetch.
async fn fetch_price() -> Option<(f64, &'static str)> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("kascov-price/0.1")
        .build()
        .ok()?;
    let get = |url: &'static str| {
        let client = client.clone();
        async move { client.get(url).send().await.ok()?.error_for_status().ok()?.text().await.ok() }
    };
    if let Some(body) = get("https://api.kraken.com/0/public/Ticker?pair=KASUSD").await {
        if let Some(price) = parse_kraken_price(&body) {
            return Some((price, "kraken"));
        }
    }
    if let Some(body) =
        get("https://api.coingecko.com/api/v3/simple/price?ids=kaspa&vs_currencies=usd").await
    {
        if let Some(price) = parse_coingecko_price(&body) {
            return Some((price, "coingecko"));
        }
    }
    None
}

/// GET /data/price.json — network-independent KAS/USD spot for the UI.
/// serve_cached doesn't fit (its builders are blocking; this fetch is async),
/// so a single-entry cache with the same single-flight idea: the fetch runs
/// under the cache lock, so concurrent cold misses share one upstream call
/// (bounded by the client's 5s timeout).
async fn price_handler() -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let mut cache = price_cache().lock().await;
    let stale = match &*cache {
        Some(state) => {
            let ttl = if state.body.is_some() { PRICE_TTL_OK } else { PRICE_TTL_ERR };
            state.fetched_at.elapsed() >= ttl
        }
        None => true,
    };
    if stale {
        let body = fetch_price().await.map(|(price, source)| {
            serde_json::json!({
                "kas_usd": price,
                "updated_at_ms": now_ms(),
                "source": source,
            })
            .to_string()
        });
        *cache = Some(PriceState { fetched_at: std::time::Instant::now(), body });
    }
    let body = cache.as_ref().and_then(|state| state.body.clone());
    drop(cache);

    match body {
        Some(json) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/json; charset=utf-8"),
                (header::CACHE_CONTROL, "public, max-age=30, s-maxage=60"),
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            ],
            json,
        )
            .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            [
                (header::CONTENT_TYPE, "application/json; charset=utf-8"),
                // the CDN must drop a failure at least as fast as we retry it
                (header::CACHE_CONTROL, "public, max-age=15"),
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            ],
            r#"{"error":"price unavailable"}"#,
        )
            .into_response(),
    }
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
    let network = match resolve_network(&state, net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };

    let db = state.base_dir.join(format!("{network}.db"));
    let (ttl, cache_control) = if live {
        // s-maxage lets the hosting CDN absorb the polling herd; SWR keeps
        // pages responsive while the edge revalidates.
        (5, "public, max-age=5, s-maxage=10, stale-while-revalidate=30")
    } else {
        (20, "public, max-age=15, s-maxage=60, stale-while-revalidate=300")
    };
    // Grid paging: `?after_daa=` (exclusive cursor) and `?limit=` (page size,
    // capped) walk the grid newest-first. An unparseable limit is a 400 (a
    // silently ignored limit re-serves the full first page — tens of MB the
    // caller asked NOT to get); a bad after_daa still degrades to page one.
    // Params are only meaningful for the grid, and are folded into the cache
    // key so each page caches independently.
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
        let limit = match q.get("limit") {
            None => None,
            Some(s) => match s.parse::<u64>() {
                Ok(l) => Some(l.clamp(1, MAX_PAGE)),
                Err(_) => {
                    return (StatusCode::BAD_REQUEST, "limit must be a non-negative integer")
                        .into_response()
                }
            },
        };
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

/// Feed page ceiling and the size of a bare (param-less) request.
const EVENTS_MAX_PAGE: u64 = 1000;
const EVENTS_DEFAULT_PAGE: u64 = 200;

/// GET /data/{network}/events?after_daa=&after_seq=&limit= — the chain-wide
/// event feed, canonical event objects in their canonical deterministic order
/// (accepting_daa, tx_index NULLS LAST, txid), oldest first. Cursor mirrors
/// the grid's conventions: when more rows remain the response carries
/// `next_after_daa`/`next_after_seq` — feed them back verbatim to keep
/// walking. `after_seq` counts events already consumed inside the `after_daa`
/// group (see Store::events_after for why that offset is stable).
async fn events_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    // Same contract as the grid: a bad cursor degrades to the stream start,
    // an unparseable limit is a 400 (a silently ignored limit would serve a
    // page size the caller asked not to get).
    let after_daa = q.get("after_daa").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let after_seq = q.get("after_seq").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let limit = match q.get("limit") {
        None => EVENTS_DEFAULT_PAGE,
        Some(s) => match s.parse::<u64>() {
            Ok(l) => l.clamp(1, EVENTS_MAX_PAGE),
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "limit must be a non-negative integer")
                    .into_response()
            }
        },
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let key = format!("{network}/events?after_daa={after_daa}&after_seq={after_seq}&limit={limit}");
    let cc = "public, max-age=10, s-maxage=15, stale-while-revalidate=60";
    serve_cached(&state, key, 15, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        // Over-fetch by one to learn whether another page exists.
        let mut events = store.events_after(after_daa, after_seq, limit + 1)?;
        let more = events.len() as u64 > limit;
        if more {
            events.truncate(limit as usize);
        }
        let next = events.last().filter(|_| more).map(|last| {
            let in_group =
                events.iter().filter(|e| e.accepting_daa == last.accepting_daa).count() as u64;
            (
                last.accepting_daa,
                if last.accepting_daa == after_daa { after_seq + in_group } else { in_group },
            )
        });
        let tip = store.tip()?;
        let mut out = serde_json::json!({
            "network": network.to_string(),
            "generated_at_ms": now_ms(),
            "tip_daa": tip.map(|t| t.0),
            "tip_at_ms": tip.map(|t| t.1),
            "events": events,
        });
        if let Some((daa, seq)) = next {
            out["next_after_daa"] = serde_json::json!(daa);
            out["next_after_seq"] = serde_json::json!(seq);
        }
        Ok(Some(serde_json::to_string(&out)?))
    })
    .await
}

/// Ceiling on one batch-summary request.
const COINS_MAX_IDS: usize = 50;

/// Parse the `ids` batch param: comma-separated 64-hex ids, at most
/// COINS_MAX_IDS of them. Any malformed id fails the whole request — a
/// silently dropped id would read as "coin unknown" to the caller.
fn parse_coin_ids(raw: &str) -> std::result::Result<Vec<[u8; 32]>, &'static str> {
    let mut ids = Vec::new();
    for part in raw.split(',') {
        let mut b = [0u8; 32];
        if hex::decode_to_slice(part.trim(), &mut b).is_err() {
            return Err("ids must be comma-separated 64-hex covenant ids");
        }
        ids.push(b);
    }
    if ids.len() > COINS_MAX_IDS {
        return Err("at most 50 ids per request");
    }
    Ok(ids)
}

/// GET /data/{network}/coins?ids=&fields=summary — batch compact summaries.
/// Unknown ids are simply omitted; malformed input is a 400. Deliberately NOT
/// behind serve_cached: `ids` is an unbounded keyspace (the /search
/// reasoning), and each id is one indexed lookup.
async fn coins_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    match params.get("fields").map(String::as_str) {
        None | Some("summary") => {}
        Some(_) => return (StatusCode::BAD_REQUEST, "fields must be 'summary'").into_response(),
    }
    let Some(raw) = params.get("ids") else {
        return (StatusCode::BAD_REQUEST, "ids is required").into_response();
    };
    let ids = match parse_coin_ids(raw) {
        Ok(ids) => ids,
        Err(msg) => return (StatusCode::BAD_REQUEST, msg).into_response(),
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let built = tokio::task::spawn_blocking(move || -> Result<String> {
        let store = kascov_core::store::Store::open(&db, network)?;
        let mut coins = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(s) = store.summary(&kascov_core::CovenantId(id))? {
                let id_hex = s.covenant_id.to_string();
                coins.push(serde_json::json!({
                    "id": id_hex,
                    "name": og::friendly_name(&id_hex),
                    "template": s.template,
                    "status": if s.live_utxos > 0 { "active" } else { "burned" },
                    "live_value": s.live_value,
                    "last_activity_daa": s.last_activity_daa,
                }));
            }
        }
        Ok(serde_json::to_string(&serde_json::json!({
            "network": network.to_string(),
            "generated_at_ms": now_ms(),
            "coins": coins,
        }))?)
    })
    .await;
    match built {
        Ok(Ok(json)) => (
            [
                (header::CONTENT_TYPE, "application/json; charset=utf-8"),
                (header::CACHE_CONTROL, "public, max-age=15, s-maxage=30"),
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            ],
            json,
        )
            .into_response(),
        Ok(Err(err)) => {
            tracing::error!("{network}: coins batch failed: {err}");
            (StatusCode::SERVICE_UNAVAILABLE, "snapshot unavailable").into_response()
        }
        Err(err) => {
            tracing::error!("{network}: coins batch task panicked: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

/// Templates that put a coin in the tokens directory: the KCC20 skeletons the
/// decode registry learned from on-chain reveals.
const KCC20_TEMPLATES: [&str; 2] = ["KCC20 token", "KCC20 minter"];

/// The newest Registry decode of a token covenant's state, as a
/// label → hex-value map. Live KCC20 state hides behind a P2SH commitment,
/// so the newest decodable state is usually a spend-time reveal (i.e. the
/// state as of the last spend); a live state script that decodes wins when
/// one exists. None when nothing decodes to a KCC20 template.
fn token_fields(
    store: &Store,
    registry: &kascov_decode::Registry,
    id: &CovenantId,
) -> Result<Option<serde_json::Value>> {
    let utxos = store.utxos(id, false)?; // created_daa ascending
    for utxo in utxos.iter().rev() {
        let mut d = registry.decode(utxo.spk_version, &utxo.spk_script);
        if !d.template.is_some_and(|t| t.starts_with("KCC20")) {
            let Some(redeem) = utxo
                .spent_sig
                .as_deref()
                .and_then(|sig| kascov_decode::p2sh_reveal(&utxo.spk_script, sig))
            else {
                continue;
            };
            d = registry.decode(utxo.spk_version, &redeem);
            if !d.template.is_some_and(|t| t.starts_with("KCC20")) {
                continue;
            }
        }
        if d.fields.is_empty() {
            continue;
        }
        let mut m = serde_json::Map::new();
        for f in d.fields {
            m.insert(f.name.to_string(), serde_json::json!(hex::encode(&f.value)));
        }
        return Ok(Some(serde_json::Value::Object(m)));
    }
    Ok(None)
}

/// GET /data/{network}/tokens.json — every covenant the decode registry
/// recognizes as a KCC20 token/minter build, with its latest Registry-labeled
/// state when derivable. Decoded from chain state only — nothing here is
/// validated against KCC20's token rules.
async fn tokens_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let key = format!("{network}/tokens");
    let cc = "public, max-age=30, s-maxage=60, stale-while-revalidate=300";
    serve_cached(&state, key, 60, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        let registry = kascov_decode::Registry::default();
        let mut tokens = Vec::new();
        for s in store.covenants_with_templates(&KCC20_TEMPLATES)? {
            let id_hex = s.covenant_id.to_string();
            let mut row = serde_json::json!({
                "covenant_id": id_hex,
                "name": og::friendly_name(&id_hex),
                "template": s.template,
                "status": if s.live_utxos > 0 { "active" } else { "burned" },
                "live_value": s.live_value,
                "last_activity_daa": s.last_activity_daa,
            });
            if let Some(fields) = token_fields(&store, &registry, &s.covenant_id)? {
                row["fields"] = fields;
            }
            tokens.push(row);
        }
        Ok(Some(serde_json::to_string(&serde_json::json!({
            "network": network.to_string(),
            "generated_at_ms": now_ms(),
            "tokens": tokens,
            "note": "decoded from chain state — not validated against token rules",
        }))?))
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
/// Payload variants for `galaxy.json`, selected by query params (the bare
/// request is the legacy shape forever):
///   `?fmt=2`    → `columnar`: the per-node objects are replaced by parallel
///                 arrays `ids`/`nx`/`ny`/`nr`/`nt`/`ns`/`na` (same order and
///                 index-aligned with the legacy `nodes[]`; `ids[i]` is the
///                 64-hex covenant id, the rest mirror node fields x/y/r/t/s/a),
///                 and the per-app objects by `acx`/`acy`/`ar`/`asz`/`at`
///                 (index-aligned with the legacy `apps[]`, mirroring
///                 cx/cy/r/size/t). `edges`, `bounds`, … are unchanged.
///   `?tier=core`→ `core_only`: `apps[]` in full, but nodes/edges only for
///                 clusters of size >= GALAXY_CORE_MIN_SIZE. The layout always
///                 runs over the FULL cluster set first, so node positions and
///                 `bounds` are identical across tiers — a client can hot-swap
///                 the full set in without anything moving. The payload gains
///                 `"tier":"core"` and `"nodes_total"` (full node count).
/// The two compose; `edges_total` always counts the full pre-cap edge set.
#[derive(Clone, Copy, Default)]
struct GalaxyFmt {
    columnar: bool,
    core_only: bool,
}

/// `?tier=core` keeps only clusters at least this big.
const GALAXY_CORE_MIN_SIZE: usize = 8;

/// The bare (legacy) shape — kept as the named entrypoint the tests pin.
#[cfg_attr(not(test), allow(dead_code))]
fn build_galaxy(store: &Store, network: kascov_core::Network) -> Result<serde_json::Value> {
    build_galaxy_fmt(store, network, GalaxyFmt::default())
}

fn build_galaxy_fmt(
    store: &Store,
    network: kascov_core::Network,
    fmt: GalaxyFmt,
) -> Result<serde_json::Value> {
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

    // intermediate node records — layout ALWAYS covers the full cluster set;
    // tier filtering happens at emission time only (position stability).
    struct NodeRec {
        id: CovenantId,
        t: i64,
        s: u8,
        x: i64,
        y: i64,
        r: i64,
        app: usize,
    }
    struct AppRec {
        cx: i64,
        cy: i64,
        r: i64,
        size: usize,
        t: i64,
    }
    let mut recs: Vec<NodeRec> = Vec::new();
    let mut apps: Vec<AppRec> = Vec::new();
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

        apps.push(AppRec {
            cx: cx.round() as i64,
            cy: cy.round() as i64,
            r: cr.round() as i64,
            size,
            t: dom_t,
        });

        for (mi, m) in cluster.iter().enumerate() {
            let a = (mi as f64 / size as f64) * TAU;
            let (x, y) = (cx + cr * a.cos(), cy + cr * a.sin());
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            let nr = 3 + degree.get(m).copied().unwrap_or(1).min(6);
            node_index.insert(*m, recs.len());
            recs.push(NodeRec {
                id: *m,
                t: tpl_of(m),
                s: if *active.get(m).unwrap_or(&false) { 1 } else { 0 },
                x: x.round() as i64,
                y: y.round() as i64,
                r: nr as i64,
                app: i,
            });
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
    // deterministic order (HashMap iteration isn't) — makes the emitted body
    // stable across rebuilds and lets the tiers compare edge-for-edge
    edges.sort_unstable();

    // tier filter — decided AFTER the full layout and the (capped) full edge
    // set, so core-tier positions/edges are an exact subset of the full tier.
    // Clusters are sorted biggest-first, so the core set happens to be a
    // prefix of the node list; the remap stays general anyway.
    let keep: Vec<bool> = recs
        .iter()
        .map(|r| !fmt.core_only || cluster_list[r.app].len() >= GALAXY_CORE_MIN_SIZE)
        .collect();
    let mut remap: Vec<usize> = vec![usize::MAX; recs.len()];
    let mut kept = 0usize;
    for (i, k) in keep.iter().enumerate() {
        if *k {
            remap[i] = kept;
            kept += 1;
        }
    }
    let edges_json: Vec<serde_json::Value> = edges
        .iter()
        .filter(|(a, b, _)| keep[*a] && keep[*b])
        .map(|(a, b, w)| serde_json::json!([remap[*a], remap[*b], w]))
        .collect();

    if !min_x.is_finite() {
        min_x = 0.0;
        min_y = 0.0;
        max_x = 0.0;
        max_y = 0.0;
    }
    let tip = store.tip()?;
    let mut out = serde_json::json!({
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
        "edges": edges_json,
        "edges_total": edge_total,
    });
    let obj = out.as_object_mut().expect("galaxy payload is an object");
    let sel = || recs.iter().zip(&keep).filter(|(_, k)| **k).map(|(r, _)| r);
    if fmt.columnar {
        // ?fmt=2 — parallel arrays; index-aligned with the legacy nodes[]
        obj.insert("ids".into(), sel().map(|r| serde_json::json!(r.id)).collect::<Vec<_>>().into());
        obj.insert("nx".into(), sel().map(|r| r.x.into()).collect::<Vec<serde_json::Value>>().into());
        obj.insert("ny".into(), sel().map(|r| r.y.into()).collect::<Vec<serde_json::Value>>().into());
        obj.insert("nr".into(), sel().map(|r| r.r.into()).collect::<Vec<serde_json::Value>>().into());
        obj.insert("nt".into(), sel().map(|r| r.t.into()).collect::<Vec<serde_json::Value>>().into());
        obj.insert("ns".into(), sel().map(|r| r.s.into()).collect::<Vec<serde_json::Value>>().into());
        obj.insert("na".into(), sel().map(|r| r.app.into()).collect::<Vec<serde_json::Value>>().into());
        // …and the apps, index-aligned with the legacy apps[] (still ALL
        // clusters, in both tiers — the far-zoom LOD must look complete)
        obj.insert("acx".into(), apps.iter().map(|a| a.cx.into()).collect::<Vec<serde_json::Value>>().into());
        obj.insert("acy".into(), apps.iter().map(|a| a.cy.into()).collect::<Vec<serde_json::Value>>().into());
        obj.insert("ar".into(), apps.iter().map(|a| a.r.into()).collect::<Vec<serde_json::Value>>().into());
        obj.insert("asz".into(), apps.iter().map(|a| a.size.into()).collect::<Vec<serde_json::Value>>().into());
        obj.insert("at".into(), apps.iter().map(|a| a.t.into()).collect::<Vec<serde_json::Value>>().into());
    } else {
        let nodes: Vec<serde_json::Value> = sel()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "t": r.t,
                    "s": r.s,
                    "x": r.x,
                    "y": r.y,
                    "r": r.r,
                    "a": r.app,
                })
            })
            .collect();
        obj.insert("nodes".into(), nodes.into());
        let apps_json: Vec<serde_json::Value> = apps
            .iter()
            .map(|a| {
                serde_json::json!({
                    "cx": a.cx,
                    "cy": a.cy,
                    "r": a.r,
                    "size": a.size,
                    "t": a.t,
                })
            })
            .collect();
        obj.insert("apps".into(), apps_json.into());
    }
    if fmt.core_only {
        obj.insert("tier".into(), "core".into());
        obj.insert("nodes_total".into(), (recs.len() as u64).into());
    }
    Ok(out)
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

/// json_resp with an explicit non-200 status (client errors that must be
/// visible as such, not `ok:false` inside a 200).
fn json_error(status: axum::http::StatusCode, v: serde_json::Value) -> axum::response::Response {
    use axum::http::header;
    use axum::response::IntoResponse;
    (status, [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], v.to_string()).into_response()
}

fn blake2b32(bytes: &[u8]) -> [u8; 32] {
    *blake2b_simd::Params::new().hash_length(32).hash(bytes).as_bytes().first_chunk::<32>().unwrap()
}

/// Wall-clock ceiling on one silverc run; at the deadline the child is killed.
const SILVERC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Cap on captured stdout/stderr — a runaway compiler can't balloon memory.
const SILVERC_OUTPUT_CAP: usize = 256 * 1024;

/// Compile SilverScript source + args to script hex via the `silverc` binary
/// (SILVERC_BIN). Ok(hex) or Err(message).
async fn run_silverc(source: String, args: Vec<String>) -> Result<String, String> {
    let bin = std::env::var("SILVERC_BIN").unwrap_or_default();
    if bin.is_empty() {
        return Err("the SilverScript compiler isn't available on this server".into());
    }
    let out = tokio::task::spawn_blocking(move || {
        use std::io::{Read, Write};
        use std::process::{Command, Stdio};
        let mut child = Command::new(&bin)
            .arg("-")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        // Source is bounded (≤40KB) and fits a pipe buffer, so this can't wedge.
        child.stdin.take().unwrap().write_all(source.as_bytes())?;
        // Drain each pipe on its own thread, keeping only the first
        // SILVERC_OUTPUT_CAP bytes — draining must continue past the cap or a
        // chatty child blocks on a full pipe and never exits.
        fn capped_drain(mut r: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
            std::thread::spawn(move || {
                let mut kept = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    match r.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let room = SILVERC_OUTPUT_CAP.saturating_sub(kept.len());
                            kept.extend_from_slice(&chunk[..n.min(room)]);
                        }
                    }
                }
                String::from_utf8_lossy(&kept).trim().to_string()
            })
        }
        let stdout = capped_drain(child.stdout.take().unwrap());
        let stderr = capped_drain(child.stderr.take().unwrap());
        let deadline = std::time::Instant::now() + SILVERC_TIMEOUT;
        loop {
            match child.try_wait()? {
                Some(status) => {
                    return std::io::Result::Ok((
                        status.success(),
                        stdout.join().unwrap_or_default(),
                        stderr.join().unwrap_or_default(),
                    ));
                }
                None if std::time::Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait(); // reap; also unblocks the drain threads
                    return Ok((false, String::new(), "compiler timed out".to_string()));
                }
                None => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        }
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
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(_net): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<ZkReq>,
) -> axum::response::Response {
    if req.program_hex.len() > 8_000 {
        return json_resp(serde_json::json!({ "ok": false, "error": "program too large" }));
    }
    if let Err(reason) = state.tool_limiter.lock().await.try_take(&client_ip(&headers)) {
        return too_many(reason);
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
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(_net): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<CompileReq>,
) -> axum::response::Response {
    if req.source.len() > 40_000 || req.args.len() > 16 || req.args.iter().any(|a| a.len() > 200) {
        return json_resp(serde_json::json!({ "ok": false, "error": "input too large" }));
    }
    if let Err(reason) = state.tool_limiter.lock().await.try_take(&client_ip(&headers)) {
        return too_many(reason);
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

/// Token bucket + per-IP hourly counter shared by the compiler-adjacent
/// endpoints (/compile, /publish, /zk-verify). Same trust model as
/// DeployLimiter: the global bucket is the only sound bound (X-Forwarded-For
/// is spoofable), the per-IP cap is best-effort. Generous — these endpoints
/// burn CPU, not faucet funds.
struct ToolLimiter {
    tokens: f64,
    last_refill: std::time::Instant,
    per_ip: std::collections::HashMap<String, (u64, u32)>, // ip -> (hour, count)
}

/// Global ceiling: 500 runs/hour, burstable to the full hour's budget.
const TOOL_BUCKET_CAP: f64 = 500.0;
const TOOL_REFILL_PER_SEC: f64 = 500.0 / 3600.0;
/// Each client IP gets this many runs per clock hour (UTC) — best-effort.
const TOOL_PER_IP_PER_HOUR: u32 = 30;

impl ToolLimiter {
    fn new() -> Self {
        Self { tokens: TOOL_BUCKET_CAP, last_refill: std::time::Instant::now(), per_ip: Default::default() }
    }

    /// Charge one run to `ip`. Ok on success; Err(reason) when throttled.
    fn try_take(&mut self, ip: &str) -> std::result::Result<(), &'static str> {
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;
        self.tokens = (self.tokens + dt * TOOL_REFILL_PER_SEC).min(TOOL_BUCKET_CAP);
        // Global bucket FIRST, so a throttled flood never allocates per-IP rows.
        if self.tokens < 1.0 {
            return Err("compiler rate limit — try again in a few minutes");
        }
        let hour = now_ms() / 3_600_000;
        // Same hard bound as DeployLimiter: evict stale hours, then if a
        // same-hour spoofed-XFF flood still overflows, drop the map entirely.
        if self.per_ip.len() > DEPLOY_IP_MAP_MAX {
            self.per_ip.retain(|_, (h, _)| *h == hour);
            if self.per_ip.len() > DEPLOY_IP_MAP_MAX {
                self.per_ip.clear();
            }
        }
        let entry = self.per_ip.entry(ip.to_string()).or_insert((hour, 0));
        if entry.0 != hour {
            *entry = (hour, 0);
        }
        if entry.1 >= TOOL_PER_IP_PER_HOUR {
            return Err("hourly compiler limit reached for your address — try again later");
        }
        self.tokens -= 1.0;
        entry.1 += 1;
        Ok(())
    }
}

/// The 429 the tool limiter hands back — JSON like the endpoints it guards.
fn too_many(reason: &'static str) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")],
        serde_json::json!({ "ok": false, "error": reason }).to_string(),
    )
        .into_response()
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

    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    // Gated OFF by default: the route only exists when armed for testnet-10.
    let deploy_key = std::env::var("KASCOV_DEPLOY_KEY").unwrap_or_default();
    if deploy_key.trim().is_empty() || network != Network::Testnet(10) {
        return (StatusCode::NOT_FOUND, "not found").into_response();
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
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<CompileReq>,
) -> axum::response::Response {
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    if req.source.len() > 40_000 {
        return json_resp(serde_json::json!({ "ok": false, "error": "bad request" }));
    }
    if let Err(reason) = state.tool_limiter.lock().await.try_take(&client_ip(&headers)) {
        return too_many(reason);
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
    let network = match resolve_network(&state, &net) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
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
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    if req.url.len() > 500 || !req.url.starts_with("http") {
        return json_resp(serde_json::json!({ "ok": false, "error": "a valid http(s) url is required" }));
    }
    // A kind filter must be a real event kind — anything else would register
    // a subscription that can never fire.
    if let Some(kind) = req.kind.as_deref() {
        if !matches!(kind, "genesis" | "transition" | "burn") {
            return json_error(
                axum::http::StatusCode::BAD_REQUEST,
                serde_json::json!({ "ok": false, "error": "kind must be genesis, transition or burn (or omitted for all kinds)" }),
            );
        }
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
    // 128-bit CSPRNG secret, hex. Signs every delivery (X-Kascov-Signature)
    // and gates unsubscribe; shown once, never readable back.
    let secret = {
        use secp256k1::rand::RngCore;
        let mut buf = [0u8; 16];
        secp256k1::rand::rngs::OsRng.fill_bytes(&mut buf);
        hex::encode(buf)
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let (kind, url, stored_secret) = (req.kind, req.url, secret.clone());
    let added = tokio::task::spawn_blocking(move || -> anyhow::Result<i64> {
        let store = kascov_core::store::Store::open(&db, network)?;
        Ok(store.add_subscription(cid.as_deref(), kind.as_deref(), &url, Some(&stored_secret), now_ms())?)
    })
    .await;
    match added {
        Ok(Ok(id)) => json_resp(serde_json::json!({ "ok": true, "id": id, "secret": secret })),
        _ => json_resp(serde_json::json!({ "ok": false, "error": "couldn't subscribe" })),
    }
}

/// POST /data/{network}/unsubscribe — remove a webhook subscription by the
/// {id, secret} /subscribe returned. Legacy rows (created before secrets)
/// still delete by id alone.
#[derive(serde::Deserialize)]
struct UnsubscribeReq {
    id: i64,
    #[serde(default)]
    secret: Option<String>,
}

async fn unsubscribe_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    axum::Json(req): axum::Json<UnsubscribeReq>,
) -> axum::response::Response {
    use kascov_core::store::UnsubscribeOutcome;
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let deleted = tokio::task::spawn_blocking(move || -> Result<UnsubscribeOutcome> {
        let store = kascov_core::store::Store::open(&db, network)?;
        Ok(store.delete_subscription_secured(req.id, req.secret.as_deref())?)
    })
    .await;
    match deleted {
        Ok(Ok(UnsubscribeOutcome::Deleted)) => json_resp(serde_json::json!({ "ok": true, "deleted": true })),
        Ok(Ok(UnsubscribeOutcome::NotFound)) => json_resp(serde_json::json!({ "ok": true, "deleted": false })),
        Ok(Ok(UnsubscribeOutcome::WrongSecret)) => json_error(
            axum::http::StatusCode::FORBIDDEN,
            serde_json::json!({ "ok": false, "error": "secret does not match" }),
        ),
        _ => json_resp(serde_json::json!({ "ok": false, "error": "couldn't unsubscribe" })),
    }
}

/// GET /data/{network}/lane/{ns} — one KIP-21 lane namespace's dashboard:
/// headline counts, the newest events, and a bucketed activity series.
async fn lane_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, ns)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    // Namespaces are the 4-byte app tag as 8 lowercase hex chars — anything
    // else is a client error (and never reaches the cache/DB).
    let ns = ns.strip_suffix(".json").unwrap_or(&ns).to_ascii_lowercase();
    if ns.len() != 8 || !ns.bytes().all(|b| b.is_ascii_hexdigit()) {
        return (StatusCode::BAD_REQUEST, "namespace must be 8 hex characters").into_response();
    }
    // 36_000 DAA ≈ 1 hour at 10 blocks/s — hour buckets over the lane's life.
    const LANE_BUCKET_DAA: u64 = 36_000;
    let db = state.base_dir.join(format!("{network}.db"));
    let key = format!("{network}/lane/{ns}");
    let cc = "public, max-age=30, s-maxage=60, stale-while-revalidate=300";
    serve_cached(&state, key, 60, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        let (events, covenants) = store.lane_stats(&ns)?;
        let recent: Vec<_> = store
            .lane_recent(&ns, 50)?
            .into_iter()
            .map(|e| {
                serde_json::json!({
                    "covenant_id": e.covenant_id,
                    "txid": e.txid,
                    "accepting_daa": e.accepting_daa,
                    "kind": e.kind,
                })
            })
            .collect();
        let activity: Vec<_> = store
            .lane_activity(&ns, LANE_BUCKET_DAA)?
            .into_iter()
            .map(|(daa, count)| serde_json::json!({ "daa": daa, "count": count }))
            .collect();
        Ok(Some(serde_json::to_string(&serde_json::json!({
            "network": network.to_string(),
            "namespace": ns,
            "generated_at_ms": now_ms(),
            "events": events,
            "covenants": covenants,
            "recent": recent,
            "activity": activity,
            "bucket_daa": LANE_BUCKET_DAA,
        }))?))
    })
    .await
}

/// GET /data/{network}/debug/{txid} — replay a REAL on-chain covenant spend:
/// find the state UTXO this txid spent, take its locking script and the
/// captured witness, and run them through the actual TxScriptEngine with a
/// per-opcode trace. The tx context is fabricated (see kascov_sim::
/// debug_witness), so signature/introspection checks may diverge from the
/// original — the response says so.
async fn debug_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, txid)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    let tx_hex = txid.strip_suffix(".json").unwrap_or(&txid);
    let Ok(txid) = tx_hex.parse::<TxId>() else {
        return (StatusCode::BAD_REQUEST, "bad txid").into_response();
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let key = format!("{network}/debug/{txid}");
    // The result is immutable once the spend is indexed — cache hard.
    let cc = "public, max-age=300, s-maxage=3600, stale-while-revalidate=3600";
    serve_cached(&state, key, 3600, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        let spent = store.spent_by_txid(&txid)?;
        // Prefer an input whose witness was captured (P2SH reveals).
        let Some(row) = spent.iter().find(|r| r.spent_sig.as_ref().is_some_and(|s| !s.is_empty()))
        else {
            let reason = if spent.is_empty() {
                "this txid didn't spend any covenant state we track"
            } else {
                "no unlocking script was captured for this spend"
            };
            return Ok(Some(serde_json::to_string(&serde_json::json!({
                "ok": false,
                "reason": reason,
            }))?));
        };
        let sig = row.spent_sig.as_deref().unwrap_or_default();
        let result = kascov_sim::debug_witness(
            row.spk_version,
            &row.spk_script,
            sig,
            row.value,
            row.spent_budget,
            Some(row.covenant_id.0),
        );
        // Bound the body: pathological programs could log tens of thousands
        // of opcodes; the debugger UI walks far fewer.
        let mut trace = result.trace;
        let truncated = trace.len() > 2000;
        trace.truncate(2000);
        Ok(Some(serde_json::to_string(&serde_json::json!({
            "ok": result.ok,
            "pass": result.pass,
            "verdict": result.verdict,
            "covenant_id": row.covenant_id,
            "outpoint": { "txid": row.outpoint.txid, "index": row.outpoint.index },
            "value": row.value,
            "trace": trace,
            "trace_truncated": truncated,
            "note": result.note,
        }))?))
    })
    .await
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
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
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
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
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
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
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
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
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
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
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
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    // Opt-in payload variants (see GalaxyFmt). Unknown params and unknown
    // values degrade to the legacy shape, so old and new clients both work.
    let fmt = GalaxyFmt {
        columnar: q.get("fmt").is_some_and(|v| v == "2"),
        core_only: q.get("tier").is_some_and(|v| v == "core"),
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let cc = "public, max-age=30, s-maxage=120, stale-while-revalidate=600";
    // fold the (parsed, hence bounded: 4 combos) variant into the cache key;
    // the bare request keeps its historical key.
    let key = if fmt.columnar || fmt.core_only {
        format!("{network}/galaxy?fmt={}&tier={}", fmt.columnar as u8, fmt.core_only as u8)
    } else {
        format!("{network}/galaxy")
    };
    // TTL 300s (not the usual 60): a galaxy build costs ~5-8s at 168k
    // covenants, and the keep-warm task in serve() re-inserts the frontend's
    // two variants every ~240s — so requests always land inside the fresh
    // window instead of paying a cold rebuild at the door.
    serve_cached(&state, key, 300, cc, accepts_gzip(&headers), move || {
        let store = kascov_core::store::Store::open(&db, network)?;
        Ok(Some(serde_json::to_string(&build_galaxy_fmt(&store, network, fmt)?)?))
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

    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    let id_hex = id.strip_suffix(".json").unwrap_or(&id);
    let Ok(covenant_id) = id_hex.parse::<kascov_core::CovenantId>() else {
        return (StatusCode::BAD_REQUEST, "bad covenant id").into_response();
    };

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

/// Presentation bits shared by the /og card and the /share shell — computed
/// from the same `CovenantSummary` the detail endpoint serves.
struct ShareInfo {
    name: String,
    alive: bool,
    balance_line: String,
    born_line: String,
    description: String,
}

fn share_info(
    store: &kascov_core::store::Store,
    summary: &kascov_core::store::CovenantSummary,
    network: Network,
) -> Result<ShareInfo> {
    let name = og::friendly_name(&summary.covenant_id.to_string());
    let alive = summary.live_utxos > 0;
    let unit = match network {
        Network::Mainnet => "KAS",
        Network::Testnet(_) => "TKAS",
    };
    let balance_line = if alive {
        format!("{} live on chain", og::fmt_amount(summary.live_value, unit))
    } else {
        format!("{} at birth · story ended", og::fmt_amount(summary.born_value, unit))
    };
    // DAA -> wall clock, anchored on the indexer's tip (~10 DAA per second;
    // same estimate the frontend makes in daaToMs).
    let born_date = match (store.tip()?, summary.genesis_daa) {
        (Some((tip_daa, tip_ms)), Some(genesis_daa)) => {
            Some(og::fmt_date(tip_ms.saturating_sub(tip_daa.saturating_sub(genesis_daa) * 100)))
        }
        _ => None,
    };
    let events = format!(
        "{} event{}",
        summary.event_count,
        if summary.event_count == 1 { "" } else { "s" }
    );
    let born_line = match &born_date {
        Some(date) => format!("born {date} · {events}"),
        None => format!("adopted mid-life · {events}"),
    };
    let mut description = format!(
        "{} smart coin on Kaspa {network} — {balance_line} · {born_line}",
        if alive { "A living" } else { "A retired" },
    );
    if let Some(t) = summary.template.as_deref().filter(|t| !t.is_empty()) {
        description.push_str(&format!(" · {t}"));
    }
    Ok(ShareInfo { name, alive, balance_line, born_line, description })
}

/// GET /og/{network}/{id}.png — the 1200x630 Open Graph card. Rendered on
/// demand (SVG -> resvg -> PNG, embedded fonts); the CDN holds it for a week,
/// so no in-process cache (serve_cached stores strings, this is bytes).
async fn og_card_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, id)): axum::extract::Path<(String, String)>,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    let Some(id_hex) = id.strip_suffix(".png") else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let Ok(covenant_id) = id_hex.parse::<kascov_core::CovenantId>() else {
        return (StatusCode::BAD_REQUEST, "bad covenant id").into_response();
    };

    let db = state.base_dir.join(format!("{network}.db"));
    let result = tokio::task::spawn_blocking(move || -> Result<Option<Vec<u8>>> {
        let store = kascov_core::store::Store::open(&db, network)?;
        let Some(summary) = store.summary(&covenant_id)? else { return Ok(None) };
        let info = share_info(&store, &summary, network)?;
        let card = og::CardData {
            id: covenant_id.to_string(),
            name: info.name,
            alive: info.alive,
            balance_line: info.balance_line,
            born_line: info.born_line,
            network: network.to_string(),
        };
        let started = std::time::Instant::now();
        let png = og::render_png(&og::card_svg(&card))?;
        tracing::info!(
            "og card {network}/{covenant_id}: {} bytes in {}ms",
            png.len(),
            started.elapsed().as_millis()
        );
        Ok(Some(png))
    })
    .await;
    match result {
        Ok(Ok(Some(png))) => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, "public, max-age=86400, s-maxage=604800"),
            ],
            png,
        )
            .into_response(),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "unknown covenant").into_response(),
        Ok(Err(err)) => {
            tracing::error!("{network}: og card failed: {err}");
            (StatusCode::SERVICE_UNAVAILABLE, "card unavailable").into_response()
        }
        Err(err) => {
            tracing::error!("{network}: og card panicked: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

/// GET /share/{network}/{id} — a ~1KB crawler-visible shell: OG/Twitter meta
/// tags pointing at the PNG card, a canonical url, a visible fallback link,
/// and a JS redirect into the hash-routed SPA for humans.
async fn share_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, id)): axum::extract::Path<(String, String)>,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    let Ok(covenant_id) = id.parse::<kascov_core::CovenantId>() else {
        return (StatusCode::BAD_REQUEST, "bad covenant id").into_response();
    };

    let db = state.base_dir.join(format!("{network}.db"));
    let result = tokio::task::spawn_blocking(move || -> Result<Option<String>> {
        let store = kascov_core::store::Store::open(&db, network)?;
        let Some(summary) = store.summary(&covenant_id)? else { return Ok(None) };
        let info = share_info(&store, &summary, network)?;
        // id is validated hex and the name comes from fixed word lists, but
        // everything interpolated is escaped anyway — belt and braces.
        let id = og::esc(&covenant_id.to_string());
        let net = og::esc(&network.to_string());
        let status = if info.alive { "alive" } else { "retired" };
        let title = og::esc(&format!("{} ({status})", info.name));
        let desc = og::esc(&info.description);
        let page = og::esc(&format!("https://kascov.io/share/{network}/{covenant_id}"));
        let image = og::esc(&format!("https://kascov.io/og/{network}/{covenant_id}.png"));
        let app = format!("/#/{net}/c/{id}");
        Ok(Some(format!(
            r#"<!doctype html>
<html lang="en"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} — kascov</title>
<meta name="description" content="{desc}">
<link rel="canonical" href="{page}">
<meta property="og:type" content="website">
<meta property="og:site_name" content="kascov">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{desc}">
<meta property="og:url" content="{page}">
<meta property="og:image" content="{image}">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{title}">
<meta name="twitter:description" content="{desc}">
<meta name="twitter:image" content="{image}">
</head><body style="background:#0a100f;color:#e9f1ef;font-family:system-ui,sans-serif;padding:2rem">
<p>{title} — {desc}. <a href="{app}" style="color:#70c7ba">Open in the kascov explorer</a></p>
<script>location.replace('{app}');</script>
</body></html>
"#
        )))
    })
    .await;
    match result {
        Ok(Ok(Some(html))) => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "public, max-age=300, s-maxage=3600"),
            ],
            html,
        )
            .into_response(),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "unknown covenant").into_response(),
        Ok(Err(err)) => {
            tracing::error!("{network}: share page failed: {err}");
            (StatusCode::SERVICE_UNAVAILABLE, "share page unavailable").into_response()
        }
        Err(err) => {
            tracing::error!("{network}: share page panicked: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

/// GET /sitemap.xml — the root plus the newest 5000 MAINNET coins as /share
/// urls. Testnets are excluded on purpose: resets would churn the sitemap.
async fn sitemap_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::header;

    let include_mainnet = state.networks.contains(&Network::Mainnet);
    let db = state.base_dir.join("mainnet.db");
    let mut resp = serve_cached(
        &state,
        "sitemap".to_string(),
        600,
        "public, max-age=600, s-maxage=3600",
        accepts_gzip(&headers),
        move || {
            let mut xml = String::from(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n\
                 <url><loc>https://kascov.io/</loc></url>\n",
            );
            if include_mainnet {
                let store = kascov_core::store::Store::open(&db, Network::Mainnet)?;
                for c in store.list_page(None, 5000)? {
                    xml.push_str(&format!(
                        "<url><loc>https://kascov.io/share/mainnet/{}</loc></url>\n",
                        c.covenant_id
                    ));
                }
            }
            xml.push_str("</urlset>\n");
            Ok(Some(xml))
        },
    )
    .await;
    // serve_cached stamps application/json on everything it serves; the body
    // here is XML, so correct the label (success path only — error bodies are
    // plain text and never cached).
    if resp.status().is_success() {
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/xml; charset=utf-8"),
        );
    }
    resp
}

async fn tx_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, txid)): axum::extract::Path<(String, String)>,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    let tx_hex = txid.strip_suffix(".json").unwrap_or(&txid);
    let Ok(txid) = tx_hex.parse::<TxId>() else {
        return (StatusCode::BAD_REQUEST, "bad txid").into_response();
    };

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
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };

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
    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };

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

    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
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

    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
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
                "born_value": c.born_value,
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

/* --------------------------------------------------------------- search */

/// In-memory search index for one network. Names sit in a Vec sorted by
/// (name, id) so a prefix query is a binary search + forward walk; templates
/// are the distinct recognized names, each with a capped sample of covenant
/// ids (search shows "a few of this template", not all of them).
struct SearchIndex {
    names: Vec<(String, [u8; 32])>,
    /// The non-leading tokens of every generated name ("slate"/"tapir" of
    /// quiet-slate-tapir), same sorted shape — so a query on any word of a
    /// name matches, not just its first. Leading tokens are covered by the
    /// full-name walk over `names`.
    name_tokens: Vec<(String, [u8; 32])>,
    templates: Vec<(String, Vec<[u8; 32]>)>,
}

/// Build the token index `SearchIndex::name_tokens` out of the (name, id)
/// pairs — split on the generated names' '-' separator, skip the leading
/// token, sort for the binary-search walk.
fn name_token_index(names: &[(String, [u8; 32])]) -> Vec<(String, [u8; 32])> {
    let mut tokens: Vec<(String, [u8; 32])> = names
        .iter()
        .flat_map(|(name, id)| name.split('-').skip(1).map(move |t| (t.to_string(), *id)))
        .collect();
    tokens.sort_unstable();
    tokens
}

/// Ids a single template contributes to the index — search returns at most
/// `SEARCH_MAX_LIMIT` rows total, so a handful per template is plenty.
const SEARCH_TEMPLATE_IDS: usize = 32;
const SEARCH_MAX_LIMIT: usize = 20;
/// How long a cached index is trusted without even re-checking the covenant
/// count. Past this we probe COUNT(*) and rebuild only if it moved.
const SEARCH_INDEX_FRESH: std::time::Duration = std::time::Duration::from_secs(60);

fn build_search_index(store: &kascov_core::store::Store) -> Result<SearchIndex> {
    let ids = store.covenant_ids()?;
    // friendly_name only reads the first 6 bytes; feeding it the full hex id
    // keeps byte-parity with the frontend obvious.
    let mut names: Vec<(String, [u8; 32])> = ids
        .into_iter()
        .map(|id| (og::friendly_name(&hex::encode(id)), id))
        .collect();
    names.sort_unstable();
    let name_tokens = name_token_index(&names);
    let mut by_template: std::collections::HashMap<String, Vec<[u8; 32]>> =
        std::collections::HashMap::new();
    for (id, template) in store.covenant_templates()? {
        let slot = by_template.entry(template.to_lowercase()).or_default();
        if slot.len() < SEARCH_TEMPLATE_IDS {
            slot.push(id.0);
        }
    }
    let mut templates: Vec<(String, Vec<[u8; 32]>)> = by_template.into_iter().collect();
    for (_, ids) in &mut templates {
        ids.sort_unstable();
    }
    templates.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    Ok(SearchIndex { names, name_tokens, templates })
}

/// The current index for `network`, rebuilding at most when the covenant set
/// actually grew. Runs on a blocking thread (SQLite + a ~168k-row sort).
/// Two racing cold requests may both build; the loser's work is discarded —
/// harmless, and it keeps the lock scope to plain map lookups.
fn search_index_for(
    state: &ServeState,
    network: Network,
    store: &kascov_core::store::Store,
) -> Result<std::sync::Arc<SearchIndex>> {
    let key = network.to_string();
    if let Some((at, _, idx)) = state.search_index.lock().unwrap().get(&key) {
        if at.elapsed() < SEARCH_INDEX_FRESH {
            return Ok(idx.clone());
        }
    }
    let count = store.covenant_count()?;
    {
        let mut cache = state.search_index.lock().unwrap();
        if let Some(entry) = cache.get_mut(&key) {
            if entry.1 == count {
                entry.0 = std::time::Instant::now();
                return Ok(entry.2.clone());
            }
        }
    }
    let built = std::sync::Arc::new(build_search_index(store)?);
    state
        .search_index
        .lock()
        .unwrap()
        .insert(key, (std::time::Instant::now(), count, built.clone()));
    Ok(built)
}

/// A hex prefix (even or odd nibble count) → the inclusive `[lo, hi]` 32-byte
/// range it covers on the BLOB primary key. Even pairs pin whole bytes; an odd
/// trailing nibble pins the high half of its byte (`lo = p·0`, `hi = p·f`).
/// None when `q` isn't plausible hex or is longer than a full id.
fn hex_prefix_range(q: &str) -> Option<([u8; 32], [u8; 32])> {
    if q.is_empty() || q.len() > 64 || !q.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let nib = |b: u8| (b as char).to_digit(16).expect("hexdigit checked") as u8;
    let bytes = q.as_bytes();
    let mut lo = [0u8; 32];
    let mut hi = [0xffu8; 32];
    for i in 0..q.len() / 2 {
        let v = (nib(bytes[2 * i]) << 4) | nib(bytes[2 * i + 1]);
        lo[i] = v;
        hi[i] = v;
    }
    if q.len() % 2 == 1 {
        let i = q.len() / 2;
        let v = nib(bytes[q.len() - 1]);
        lo[i] = v << 4;
        hi[i] = (v << 4) | 0x0f;
    }
    Some((lo, hi))
}

/// Ids whose friendly name starts with `q`, in name order.
fn name_prefix_matches(names: &[(String, [u8; 32])], q: &str, limit: usize) -> Vec<[u8; 32]> {
    let start = names.partition_point(|(n, _)| n.as_str() < q);
    names[start..]
        .iter()
        .take_while(|(n, _)| n.starts_with(q))
        .take(limit)
        .map(|(_, id)| *id)
        .collect()
}

/// GET /data/{network}/search?q=&limit= — find covenants by id hex prefix,
/// friendly-name prefix, or template substring. Deliberately NOT behind
/// serve_cached: `q` is an unbounded keyspace, so caching bodies per query
/// would let strangers grow the cache without limit. Every path is either a
/// bounded PK range scan or an in-memory probe, cheap enough to serve raw.
async fn search_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    let q = params
        .get("q")
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    if q.is_empty() || q.len() > 64 {
        return (StatusCode::BAD_REQUEST, "q must be 1..=64 characters").into_response();
    }
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(10)
        .clamp(1, SEARCH_MAX_LIMIT);

    let db = state.base_dir.join(format!("{network}.db"));
    let state2 = state.clone();
    let built = tokio::task::spawn_blocking(move || -> Result<String> {
        use kascov_core::store::CovenantSummary;
        let store = kascov_core::store::Store::open(&db, network)?;
        let mut seen: std::collections::HashSet<[u8; 32]> = std::collections::HashSet::new();
        let mut rows: Vec<serde_json::Value> = Vec::new();
        let push = |s: &CovenantSummary, matched: &str, rows: &mut Vec<serde_json::Value>| {
            let id_hex = s.covenant_id.to_string();
            rows.push(serde_json::json!({
                "id": id_hex,
                "name": og::friendly_name(&id_hex),
                "template": s.template,
                "status": if s.live_utxos > 0 { "active" } else { "burned" },
                "matched": matched,
            }));
        };

        // (a) id hex prefix — a bounded range scan on the PK.
        if q.len() >= 4 {
            if let Some((lo, hi)) = hex_prefix_range(&q) {
                for s in store.covenants_by_id_range(&lo, &hi, limit as u64)? {
                    if seen.insert(s.covenant_id.0) {
                        push(&s, "id", &mut rows);
                    }
                }
            }
        }
        // (b) friendly-name prefix, (c) template substring — via the index.
        if rows.len() < limit {
            let idx = search_index_for(&state2, network, &store)?;
            for id in name_prefix_matches(&idx.names, &q, limit - rows.len()) {
                if !seen.contains(&id) {
                    if let Some(s) = store.summary(&kascov_core::CovenantId(id))? {
                        seen.insert(id);
                        push(&s, "name", &mut rows);
                    }
                }
            }
            // Token prefix: "tapir" finds quiet-slate-tapir. Still a name
            // hit as far as the caller cares, so `matched` stays "name".
            for id in name_prefix_matches(&idx.name_tokens, &q, limit - rows.len()) {
                if !seen.contains(&id) {
                    if let Some(s) = store.summary(&kascov_core::CovenantId(id))? {
                        seen.insert(id);
                        push(&s, "name", &mut rows);
                    }
                }
            }
            'templates: for (template, ids) in &idx.templates {
                if !template.contains(&q) {
                    continue;
                }
                for id in ids {
                    if rows.len() >= limit {
                        break 'templates;
                    }
                    if !seen.contains(id) {
                        if let Some(s) = store.summary(&kascov_core::CovenantId(*id))? {
                            seen.insert(*id);
                            push(&s, "template", &mut rows);
                        }
                    }
                }
            }
        }
        let out = serde_json::json!({
            "network": network.to_string(),
            "query": q,
            "results": rows,
        });
        Ok(serde_json::to_string(&out)?)
    })
    .await;

    match built {
        Ok(Ok(json)) => (
            [
                (header::CONTENT_TYPE, "application/json; charset=utf-8"),
                // short shared TTL: repeated keystrokes hit the CDN, but a
                // hostile keyspace ages out fast
                (header::CACHE_CONTROL, "public, max-age=15, s-maxage=60"),
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            ],
            json,
        )
            .into_response(),
        Ok(Err(err)) => {
            tracing::error!("{network}: search failed: {err}");
            (StatusCode::SERVICE_UNAVAILABLE, "search unavailable").into_response()
        }
        Err(err) => {
            tracing::error!("{network}: search task panicked: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

/* --------------------------------------------------------------- stream */

/// Parses the optional `?covenant=` SSE filter. `Ok(None)` when absent; the
/// substring needle to probe fan-out messages with when it's a well-formed
/// 64-hex id; `Err` on anything else (a typo'd filter must fail loudly, not
/// silently stream the whole firehose).
fn covenant_filter(param: Option<&str>) -> std::result::Result<Option<String>, ()> {
    let Some(raw) = param else { return Ok(None) };
    let id = raw.trim().to_ascii_lowercase();
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(());
    }
    Ok(Some(format!("\"covenant_id\":\"{id}\"")))
}

/// Substring probe, no JSON parse: fan-out messages are compact serde_json
/// strings, so a covenant event embeds `"covenant_id":"<hex>"` verbatim.
/// Non-covenant messages (reorg notices) don't match a filtered stream.
fn sse_event_matches(msg: &str, needle: Option<&str>) -> bool {
    needle.map_or(true, |n| msg.contains(n))
}

/// Push covenant events over SSE the moment the follower indexes them.
/// Hints only — no replay, no backlog, lagged subscribers skip ahead;
/// consumers confirm state through the polled feeds.
async fn stream_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    use axum::http::{header, HeaderName, HeaderValue, StatusCode};
    use axum::response::sse::{Event, KeepAlive, Sse};
    use axum::response::IntoResponse;
    use std::sync::atomic::Ordering;

    let network = match resolve_network(&state, &net_name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    // Optional ?covenant=<64 hex>: narrow the fan-out to one coin's events.
    let Ok(needle) = covenant_filter(params.get("covenant").map(String::as_str)) else {
        return (StatusCode::BAD_REQUEST, "bad covenant filter (want 64 hex chars)").into_response();
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
    let stream = futures::stream::unfold((rx, slot, needle), move |(mut rx, slot, needle)| async move {
        loop {
            match tokio::time::timeout_at(deadline, rx.recv()).await {
                Ok(Ok(msg)) => {
                    // Filtered streams drop non-matching events pre-emit; the
                    // keep-alive layer still shows the client a live socket.
                    if !sse_event_matches(&msg, needle.as_deref()) {
                        continue;
                    }
                    let event = Event::default().data(&*msg);
                    return Some((Ok::<_, std::convert::Infallible>(event), (rx, slot, needle)));
                }
                // Fell behind the buffer: skip ahead — clients resync by polling.
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return None,
                // Lifetime reached — recycle the slot.
                Err(_) => return None,
            }
        }
    });
    // Lead with a comment so headers and first bytes flush at accept time —
    // clients see the connection is live and buffering proxies commit to the
    // stream instead of holding a byteless response open.
    let stream = futures::stream::once(async {
        Ok::<_, std::convert::Infallible>(Event::default().comment("connected"))
    })
    .chain(stream);

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
mod search_tests {
    use super::*;

    #[test]
    fn hex_prefix_range_even_and_odd() {
        // even prefix pins whole bytes
        let (lo, hi) = hex_prefix_range("a1b2").unwrap();
        assert_eq!(&lo[..2], &[0xa1, 0xb2]);
        assert_eq!(&hi[..2], &[0xa1, 0xb2]);
        assert!(lo[2..].iter().all(|&b| b == 0x00));
        assert!(hi[2..].iter().all(|&b| b == 0xff));
        // odd trailing nibble pins the high half of its byte
        let (lo, hi) = hex_prefix_range("a1b").unwrap();
        assert_eq!(&lo[..2], &[0xa1, 0xb0]);
        assert_eq!(&hi[..2], &[0xa1, 0xbf]);
        // a full 64-char id degenerates to a point range
        let full = "ff".repeat(32);
        let (lo, hi) = hex_prefix_range(&full).unwrap();
        assert_eq!(lo, [0xff; 32]);
        assert_eq!(hi, [0xff; 32]);
        // junk is rejected
        assert!(hex_prefix_range("").is_none());
        assert!(hex_prefix_range("xyz1").is_none());
        assert!(hex_prefix_range("brave-teal").is_none());
        assert!(hex_prefix_range(&"a".repeat(65)).is_none());
    }

    #[test]
    fn name_prefix_binary_search() {
        let names = vec![
            ("brave-teal-otter".to_string(), [1u8; 32]),
            ("brave-teal-owl".to_string(), [2u8; 32]),
            ("quiet-slate-tapir".to_string(), [3u8; 32]),
        ];
        assert_eq!(name_prefix_matches(&names, "brave-te", 10).len(), 2);
        assert_eq!(name_prefix_matches(&names, "brave-te", 1).len(), 1);
        assert_eq!(name_prefix_matches(&names, "quiet", 10), vec![[3u8; 32]]);
        assert!(name_prefix_matches(&names, "zesty", 10).is_empty());
        // prefix past the last entry must not walk off the slice
        assert!(name_prefix_matches(&names, "quiet-slate-tapirx", 10).is_empty());
    }

    #[test]
    fn covenant_filter_parses_and_rejects() {
        assert_eq!(covenant_filter(None), Ok(None));
        let id = "ab".repeat(32);
        assert_eq!(
            covenant_filter(Some(&id)),
            Ok(Some(format!("\"covenant_id\":\"{id}\"")))
        );
        // uppercase input normalizes to the lowercase hex the follower emits
        assert_eq!(
            covenant_filter(Some(&"AB".repeat(32))),
            Ok(Some(format!("\"covenant_id\":\"{id}\"")))
        );
        assert_eq!(covenant_filter(Some("abcd")), Err(())); // too short
        assert_eq!(covenant_filter(Some(&"zz".repeat(32))), Err(())); // not hex
    }

    /// The filter must match exactly the JSON the follower's fan-out builds
    /// (same serde_json compact encoding, same field name).
    #[test]
    fn sse_filter_matches_fanout_shape() {
        let id = kascov_core::CovenantId([0xab; 32]);
        let other = kascov_core::CovenantId([0xcd; 32]);
        let msg = serde_json::json!({
            "covenant_id": id,
            "kind": "genesis",
            "txid": kascov_core::TxId([1; 32]),
            "accepting_daa": 12345,
        })
        .to_string();
        let reorg = serde_json::json!({ "kind": "reorg", "rolled_back": 2 }).to_string();

        let needle = covenant_filter(Some(&id.to_string())).unwrap();
        let wrong = covenant_filter(Some(&other.to_string())).unwrap();
        assert!(sse_event_matches(&msg, needle.as_deref()));
        assert!(!sse_event_matches(&msg, wrong.as_deref()));
        // reorg notices don't match a filtered stream
        assert!(!sse_event_matches(&reorg, needle.as_deref()));
        // unfiltered streams pass everything through
        assert!(sse_event_matches(&msg, None));
        assert!(sse_event_matches(&reorg, None));
    }
}

#[cfg(test)]
mod galaxy_tests {
    use super::*;
    use kascov_core::store::{BlockEvents, EventKind, NewEvent, NewUtxo, Store};
    use kascov_core::{BlockHash, CovenantId, Network, Outpoint, TxId};

    fn ev(cov: u8, kind: EventKind, tx: u8) -> NewEvent {
        NewEvent { covenant_id: CovenantId([cov; 32]), kind, txid: TxId([tx; 32]), tx_index: tx as u32, payload: None, lane_namespace: None }
    }

    // A synthetic index with two "apps": {A1,B2} share tx 0x10, and
    // {C3,D4,E5} share tx 0x20; a lone F6 is a size-1 cluster (excluded).
    // A1 gets a live utxo so it reads as active. Extra events extend it.
    fn galaxy_store(tag: &str, extra: Vec<NewEvent>) -> Store {
        let path = std::env::temp_dir()
            .join(format!("kascov-galaxy-{tag}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut store = Store::open(&path, Network::Testnet(10)).unwrap();
        let mut events = vec![
            ev(0xA1, EventKind::Genesis, 0x10),
            ev(0xB2, EventKind::Genesis, 0x10),
            ev(0xC3, EventKind::Genesis, 0x20),
            ev(0xD4, EventKind::Genesis, 0x20),
            ev(0xE5, EventKind::Genesis, 0x20),
            ev(0xF6, EventKind::Genesis, 0x30),
        ];
        events.extend(extra);
        let block = BlockEvents {
            accepting_block: BlockHash([1; 32]),
            accepting_daa: 100,
            accepting_time_ms: 100_000,
            accepting_blue_score: 100,
            events,
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
        store
    }

    #[test]
    fn galaxy_clusters_nodes_and_edges() {
        let store = galaxy_store("legacy", vec![]);
        let g = build_galaxy(&store, Network::Testnet(10)).unwrap();
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

    // ?fmt=2 — the parallel arrays must be index-aligned with legacy nodes[]
    // and everything else identical.
    #[test]
    fn galaxy_fmt2_columnar_is_index_aligned_with_legacy() {
        let store = galaxy_store("fmt2", vec![]);
        let net = Network::Testnet(10);
        let legacy = build_galaxy(&store, net).unwrap();
        let col =
            build_galaxy_fmt(&store, net, GalaxyFmt { columnar: true, core_only: false }).unwrap();

        assert!(col.get("nodes").is_none(), "fmt=2 must not carry nodes[]");
        assert!(col.get("tier").is_none(), "full tier must not be tagged");
        let nodes = legacy["nodes"].as_array().unwrap();
        assert_eq!(col["ids"].as_array().unwrap().len(), nodes.len());
        for (i, n) in nodes.iter().enumerate() {
            assert_eq!(col["ids"][i], n["id"], "ids[{i}]");
            assert_eq!(col["nx"][i], n["x"], "nx[{i}]");
            assert_eq!(col["ny"][i], n["y"], "ny[{i}]");
            assert_eq!(col["nr"][i], n["r"], "nr[{i}]");
            assert_eq!(col["nt"][i], n["t"], "nt[{i}]");
            assert_eq!(col["ns"][i], n["s"], "ns[{i}]");
            assert_eq!(col["na"][i], n["a"], "na[{i}]");
        }
        for k in ["edges", "edges_total", "bounds", "templates"] {
            assert_eq!(col[k], legacy[k], "{k} must be unchanged under fmt=2");
        }
        // apps go columnar too, index-aligned with the legacy apps[]
        assert!(col.get("apps").is_none(), "fmt=2 must not carry apps[]");
        let apps = legacy["apps"].as_array().unwrap();
        assert_eq!(col["acx"].as_array().unwrap().len(), apps.len());
        for (i, a) in apps.iter().enumerate() {
            assert_eq!(col["acx"][i], a["cx"], "acx[{i}]");
            assert_eq!(col["acy"][i], a["cy"], "acy[{i}]");
            assert_eq!(col["ar"][i], a["r"], "ar[{i}]");
            assert_eq!(col["asz"][i], a["size"], "asz[{i}]");
            assert_eq!(col["at"][i], a["t"], "at[{i}]");
        }
    }

    // ?tier=core — layout runs over the full set, so every core node's
    // position is byte-identical to its full-tier twin; apps/bounds unchanged.
    #[test]
    fn galaxy_core_tier_positions_match_full_tier() {
        // add a 9-member cluster (all sharing tx 0x40) so one cluster crosses
        // GALAXY_CORE_MIN_SIZE while {A1,B2} and {C3,D4,E5} stay below it
        let extra: Vec<NewEvent> =
            (0x60..0x69).map(|c| ev(c, EventKind::Genesis, 0x40)).collect();
        let store = galaxy_store("core", extra);
        let net = Network::Testnet(10);
        let full = build_galaxy(&store, net).unwrap();
        let core =
            build_galaxy_fmt(&store, net, GalaxyFmt { columnar: false, core_only: true }).unwrap();

        assert_eq!(core["tier"], "core");
        let full_nodes = full["nodes"].as_array().unwrap();
        let core_nodes = core["nodes"].as_array().unwrap();
        assert_eq!(full_nodes.len(), 14); // 9 + 3 + 2
        assert_eq!(core_nodes.len(), 9); // only the big cluster survives
        assert_eq!(core["nodes_total"].as_u64().unwrap(), full_nodes.len() as u64);

        // apps + bounds emitted in full — the client viewport must not shift
        assert_eq!(core["apps"], full["apps"]);
        assert_eq!(core["bounds"], full["bounds"]);

        // every core node equals its full-tier twin, matched by covenant id
        let full_by_id: std::collections::HashMap<&str, &serde_json::Value> =
            full_nodes.iter().map(|n| (n["id"].as_str().unwrap(), n)).collect();
        for n in core_nodes {
            let twin = full_by_id[n["id"].as_str().unwrap()];
            assert_eq!(n, twin, "core node must be byte-identical to its full twin");
        }

        // core edges are the full edges restricted to core nodes, re-indexed:
        // resolve both sides to id pairs and compare as sets
        let pairs = |g: &serde_json::Value, nodes: &[serde_json::Value]| {
            g["edges"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    let (a, b) = (e[0].as_u64().unwrap() as usize, e[1].as_u64().unwrap() as usize);
                    let (ia, ib) =
                        (nodes[a]["id"].as_str().unwrap(), nodes[b]["id"].as_str().unwrap());
                    let (lo, hi) = if ia < ib { (ia, ib) } else { (ib, ia) };
                    (lo.to_string(), hi.to_string(), e[2].as_u64().unwrap())
                })
                .collect::<std::collections::BTreeSet<_>>()
        };
        let core_pairs = pairs(&core, core_nodes);
        let full_pairs = pairs(&full, full_nodes);
        assert!(!core_pairs.is_empty());
        assert!(core_pairs.is_subset(&full_pairs), "core edges must be a subset of full edges");
        // and exactly the full edges whose two ends are both core members
        let expected = full_pairs
            .iter()
            .filter(|(a, b, _)| {
                let is_core = |id: &str| core_nodes.iter().any(|n| n["id"] == *id);
                is_core(a) && is_core(b)
            })
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(core_pairs, expected);

        // composed: fmt=2 + tier=core keeps the same filtered set, columnar
        let both =
            build_galaxy_fmt(&store, net, GalaxyFmt { columnar: true, core_only: true }).unwrap();
        assert_eq!(both["tier"], "core");
        assert_eq!(both["ids"].as_array().unwrap().len(), core_nodes.len());
        for (i, n) in core_nodes.iter().enumerate() {
            assert_eq!(both["ids"][i], n["id"]);
            assert_eq!(both["nx"][i], n["x"]);
            assert_eq!(both["ny"][i], n["y"]);
        }
        assert_eq!(both["edges"], core["edges"]);
    }
}

#[cfg(test)]
mod api_growth_tests {
    use super::*;

    /// The X-Kascov-Signature construction, pinned against an independent
    /// implementation (python hashlib.blake2b, key = the secret's ASCII
    /// bytes, digest_size=32).
    #[test]
    fn webhook_signature_vector() {
        assert_eq!(
            webhook_signature("00112233445566778899aabbccddeeff", "{\"kind\":\"genesis\"}"),
            "d255c6775ad244870d5ddfd7b79bbc232a7764df408e07c59441d3703dfbff59"
        );
        assert_eq!(
            webhook_signature("aa", ""),
            "75e3638c6c3f6a10429cadf5630f0cb0c0b9575b6cfd7893b4a14c795ea0c544"
        );
        // Different secrets must not collide on the same body.
        assert_ne!(
            webhook_signature("aa", "{\"kind\":\"genesis\"}"),
            webhook_signature("bb", "{\"kind\":\"genesis\"}")
        );
    }

    #[test]
    fn coin_ids_parse_and_clamp() {
        let a = "11".repeat(32);
        let b = "22".repeat(32);
        assert_eq!(parse_coin_ids(&a).unwrap(), vec![[0x11u8; 32]]);
        assert_eq!(parse_coin_ids(&format!("{a},{b}")).unwrap(), vec![[0x11u8; 32], [0x22u8; 32]]);
        // whitespace around ids is tolerated
        assert_eq!(parse_coin_ids(&format!(" {a} , {b}")).unwrap().len(), 2);
        // malformed: empty, short, non-hex, trailing comma
        assert!(parse_coin_ids("").is_err());
        assert!(parse_coin_ids("11").is_err());
        assert!(parse_coin_ids(&"zz".repeat(32)).is_err());
        assert!(parse_coin_ids(&format!("{a},")).is_err());
        // the batch ceiling: 50 ok, 51 rejected
        let max = vec![a.as_str(); COINS_MAX_IDS].join(",");
        assert_eq!(parse_coin_ids(&max).unwrap().len(), COINS_MAX_IDS);
        let over = vec![a.as_str(); COINS_MAX_IDS + 1].join(",");
        assert!(parse_coin_ids(&over).is_err());
    }

    /// Token-prefix search: any non-leading word of a generated name matches,
    /// leading words stay with the full-name walk.
    #[test]
    fn name_tokens_match_inner_words() {
        let names = vec![
            ("eager-copper-yak".to_string(), [1u8; 32]),
            ("quiet-slate-tapir".to_string(), [2u8; 32]),
            ("stubborn-violet-moth".to_string(), [3u8; 32]),
        ];
        let tokens = name_token_index(&names);
        assert_eq!(name_prefix_matches(&tokens, "tapir", 10), vec![[2u8; 32]]);
        assert_eq!(name_prefix_matches(&tokens, "sla", 10), vec![[2u8; 32]]);
        assert_eq!(name_prefix_matches(&tokens, "violet", 10), vec![[3u8; 32]]);
        assert_eq!(name_prefix_matches(&tokens, "copper", 10), vec![[1u8; 32]]);
        // leading tokens are the full-name walk's job, not the token index's
        assert!(name_prefix_matches(&tokens, "quiet", 10).is_empty());
        assert!(name_prefix_matches(&tokens, "zzz", 10).is_empty());
        // the walk honors its limit
        assert_eq!(name_prefix_matches(&tokens, "", 2).len(), 2);
    }
}

#[cfg(test)]
mod webhook_guard_tests {
    use super::*;
    use std::net::IpAddr;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn private_and_internal_ips_are_forbidden() {
        for s in [
            "127.0.0.1",
            "127.8.8.8",
            "10.0.0.1",
            "10.255.255.255",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.1.1",
            "169.254.169.254", // cloud metadata
            "169.254.0.1",
            "0.0.0.0",
            "0.1.2.3",
            "255.255.255.255",
            "100.64.0.1", // CGNAT
            "100.127.255.254",
            "192.0.0.1",
            "::1",
            "::",
            "fc00::1",
            "fdab::2", // unique local
            "fe80::1", // link local
            "::ffff:10.0.0.1", // v4-mapped private
            "::ffff:127.0.0.1",
        ] {
            assert!(ip_is_forbidden(ip(s)), "{s} must be forbidden");
        }
    }

    #[test]
    fn public_ips_are_allowed() {
        for s in [
            "8.8.8.8",
            "1.1.1.1",
            "93.184.216.34",
            "172.15.0.1",  // just below 172.16/12
            "172.32.0.1",  // just above 172.16/12
            "100.63.0.1",  // just below CGNAT
            "100.128.0.1", // just above CGNAT
            "11.0.0.1",
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8", // v4-mapped public
        ] {
            assert!(!ip_is_forbidden(ip(s)), "{s} must be allowed");
        }
    }

    #[test]
    fn url_guard_rejects_internal_targets() {
        // Literal IPs — no DNS involved, deterministic in CI.
        for url in [
            "http://127.0.0.1:8080/hook",
            "http://10.1.2.3/x",
            "https://192.168.0.10/x",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]:9999/hook",
            "http://[fe80::1]/x",
            "http://[fc00::2]/x",
            "http://0.0.0.0/x",
        ] {
            assert!(webhook_target_allowed(url).is_err(), "{url} must be rejected");
        }
    }

    #[test]
    fn url_guard_rejects_non_http_and_garbage() {
        assert!(webhook_target_allowed("ftp://example.com/x").is_err());
        assert!(webhook_target_allowed("file:///etc/passwd").is_err());
        assert!(webhook_target_allowed("not a url").is_err());
        assert!(webhook_target_allowed("http://").is_err());
    }

    #[test]
    fn url_guard_allows_public_literal_ips() {
        assert!(webhook_target_allowed("http://8.8.8.8/hook").is_ok());
        assert!(webhook_target_allowed("https://93.184.216.34:8443/hook").is_ok());
        assert!(webhook_target_allowed("http://[2606:4700:4700::1111]/hook").is_ok());
    }
}

#[cfg(test)]
mod price_tests {
    use super::*;

    #[test]
    fn kraken_ticker_shape_parses() {
        // Trimmed from a real Kraken /0/public/Ticker?pair=KASUSD response.
        let body = r#"{"error":[],"result":{"KASUSD":{
            "a":["0.077710","24896","24896.000"],
            "b":["0.077630","1553","1553.000"],
            "c":["0.077650","310.27216455"],
            "v":["4381437.63177596","10023973.86077098"],
            "p":["0.077034","0.077416"],
            "t":[382,1290],
            "l":["0.076250","0.076250"],
            "h":["0.077810","0.078710"],
            "o":"0.076850"}}}"#;
        assert_eq!(parse_kraken_price(body), Some(0.077650));
        // an unexpected pair alias still parses (key read from the map)
        let aliased = r#"{"error":[],"result":{"KASZUSD":{"c":["1.25","10"]}}}"#;
        assert_eq!(parse_kraken_price(aliased), Some(1.25));
    }

    #[test]
    fn kraken_errors_and_junk_are_rejected() {
        // Kraken signals failure via a non-empty error array, HTTP 200.
        assert_eq!(
            parse_kraken_price(r#"{"error":["EQuery:Unknown asset pair"]}"#),
            None
        );
        assert_eq!(parse_kraken_price(r#"{"error":[],"result":{}}"#), None);
        assert_eq!(
            parse_kraken_price(r#"{"error":[],"result":{"KASUSD":{"c":["nope","1"]}}}"#),
            None
        );
        assert_eq!(
            parse_kraken_price(r#"{"error":[],"result":{"KASUSD":{"c":["-1.0","1"]}}}"#),
            None
        );
        assert_eq!(parse_kraken_price("not json"), None);
    }

    #[test]
    fn coingecko_shape_parses_and_rejects_junk() {
        assert_eq!(parse_coingecko_price(r#"{"kaspa":{"usd":0.077612}}"#), Some(0.077612));
        assert_eq!(parse_coingecko_price(r#"{}"#), None);
        assert_eq!(parse_coingecko_price(r#"{"kaspa":{}}"#), None);
        assert_eq!(parse_coingecko_price(r#"{"kaspa":{"usd":"0.07"}}"#), None); // string, not number
        assert_eq!(parse_coingecko_price(r#"{"kaspa":{"usd":0}}"#), None);
        assert_eq!(parse_coingecko_price("not json"), None);
    }
}
