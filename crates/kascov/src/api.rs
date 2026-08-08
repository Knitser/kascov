use super::*;
use axum::response::IntoResponse;

const PAGE_DEFAULT: u64 = 100;
const PAGE_MAX: u64 = 1000;

fn bad_request(message: &'static str) -> axum::response::Response {
    (axum::http::StatusCode::BAD_REQUEST, message).into_response()
}

fn parse_limit(
    q: &std::collections::HashMap<String, String>,
    default: u64,
    max: u64,
) -> std::result::Result<u64, axum::response::Response> {
    match q.get("limit") {
        None => Ok(default),
        Some(value) => value
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .map(|value| value.min(max))
            .ok_or_else(|| bad_request("limit must be a positive integer")),
    }
}

fn parse_id(
    raw: &str,
    label: &'static str,
) -> std::result::Result<CovenantId, axum::response::Response> {
    raw.strip_suffix(".json")
        .unwrap_or(raw)
        .parse::<CovenantId>()
        .map_err(|_| bad_request(label))
}

fn holder_json(row: &kascov_core::tokens::TokenBalanceRow, network: Network) -> serde_json::Value {
    let display = kascov_core::tokens::owner_display(&row.owner);
    let mut value = serde_json::json!({
        "owner": display,
        "balance": row.balance,
        "cells": row.cells,
    });
    if let Some(address) = owner_address(&display, network) {
        value["owner_address"] = serde_json::json!(address);
    }
    value
}

fn token_event_json(event: &kascov_core::tokens::TokenEventRow) -> serde_json::Value {
    let mut value = serde_json::json!({
        "seq": event.seq,
        "delta_idx": event.delta_idx,
        "token_kind": event.kind,
        "event_kind": event.event_kind,
        "accepting_daa": event.accepting_daa,
        "txid": event.txid,
    });
    if let Some(amount) = event.amount {
        value["amount"] = serde_json::json!(amount);
    }
    if let Some(owner) = &event.owner_from {
        value["owner_from"] = serde_json::json!(kascov_core::tokens::owner_display(owner));
    }
    if let Some(owner) = &event.owner_to {
        value["owner_to"] = serde_json::json!(kascov_core::tokens::owner_display(owner));
    }
    if let Some(index) = event.tx_index {
        value["tx_index"] = serde_json::json!(index);
    }
    value
}

/// Store pages are bounded by distinct event sequence, so this drops the
/// over-fetched final event as a whole and returns the last complete cursor.
pub(super) fn trim_complete_event_page(
    mut rows: Vec<kascov_core::tokens::TokenEventRow>,
    event_limit: u64,
) -> (Vec<kascov_core::tokens::TokenEventRow>, Option<u64>) {
    let mut sequences = Vec::new();
    for row in &rows {
        if sequences.last() != Some(&row.seq) {
            sequences.push(row.seq);
        }
    }
    if sequences.len() as u64 <= event_limit {
        return (rows, None);
    }
    let over_fetched = *sequences.last().expect("at least one over-fetched event");
    rows.retain(|row| row.seq != over_fetched);
    let next = rows.last().map(|row| row.seq);
    (rows, next)
}

pub(super) async fn token_holders_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, id)): axum::extract::Path<(String, String)>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let network = match resolve_network(&state, &net_name) {
        Ok(network) => network,
        Err(response) => return response,
    };
    let token_id = match parse_id(&id, "bad token id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let limit = match parse_limit(&q, PAGE_DEFAULT, 500) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let cursor = match (q.get("after_balance"), q.get("after_owner")) {
        (None, None) => (None, None),
        (Some(balance), Some(owner)) => {
            if owner.len() != 66
                || !owner
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return bad_request("after_owner must be the 66-hex cursor returned by the API");
            }
            let Ok(balance) = balance.parse::<i64>() else {
                return bad_request("after_balance must be an integer");
            };
            (Some(balance), Some(owner.clone()))
        }
        _ => return bad_request("after_balance and after_owner must be supplied together"),
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let key = format!(
        "{network}/token/{token_id}/holders?limit={limit}&after_balance={}&after_owner={}",
        cursor.0.map_or(String::new(), |value| value.to_string()),
        cursor.1.as_deref().unwrap_or("")
    );
    serve_cached(
        &state,
        key,
        30,
        "public, max-age=15, s-maxage=30, stale-while-revalidate=120",
        accepts_gzip(&headers),
        move || {
            let store = Store::open(&db, network)?;
            let Some(token) = store.token_row(&token_id)? else {
                return Ok(None);
            };
            let mut rows =
                store.token_balances_page(&token_id, cursor.0, cursor.1.as_deref(), limit + 1)?;
            let more = rows.len() as u64 > limit;
            rows.truncate(limit as usize);
            let mut out = serde_json::json!({
                "network": network.to_string(),
                "token_id": token_id,
                "generated_at_ms": now_ms(),
                "holders_total": token.holders,
                "holders": rows.iter().map(|row| holder_json(row, network)).collect::<Vec<_>>(),
            });
            if more {
                if let Some(last) = rows.last() {
                    out["next_after_balance"] = serde_json::json!(last.balance);
                    out["next_after_owner"] = serde_json::json!(last.owner);
                }
            }
            Ok(Some(serde_json::to_string(&out)?))
        },
    )
    .await
}

pub(super) async fn token_events_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, id)): axum::extract::Path<(String, String)>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let network = match resolve_network(&state, &net_name) {
        Ok(network) => network,
        Err(response) => return response,
    };
    let token_id = match parse_id(&id, "bad token id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let limit = match parse_limit(&q, 200, 1000) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let parse_cursor = |name: &str| -> std::result::Result<Option<u64>, axum::response::Response> {
        q.get(name)
            .map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|_| bad_request("event cursor must be a non-negative integer"))
            })
            .transpose()
    };
    let after_seq = match parse_cursor("after_seq") {
        Ok(cursor) => cursor,
        Err(response) => return response,
    };
    let before_seq = match parse_cursor("before_seq") {
        Ok(cursor) => cursor,
        Err(response) => return response,
    };
    if after_seq.is_some() && before_seq.is_some() {
        return bad_request("after_seq and before_seq are mutually exclusive");
    }
    let newest_first = before_seq.is_some()
        || q.get("order")
            .is_some_and(|order| order.eq_ignore_ascii_case("desc"));
    if q.get("order").is_some_and(|order| {
        !order.eq_ignore_ascii_case("asc") && !order.eq_ignore_ascii_case("desc")
    }) {
        return bad_request("order must be asc or desc");
    }
    let db = state.base_dir.join(format!("{network}.db"));
    let key = format!(
        "{network}/token/{token_id}/events?limit={limit}&after={}&before={}&desc={newest_first}",
        after_seq.map_or(String::new(), |value| value.to_string()),
        before_seq.map_or(String::new(), |value| value.to_string())
    );
    serve_cached(
        &state,
        key,
        30,
        "public, max-age=15, s-maxage=30, stale-while-revalidate=120",
        accepts_gzip(&headers),
        move || {
            let store = Store::open(&db, network)?;
            if store.token_row(&token_id)?.is_none() {
                return Ok(None);
            }
            let rows = if newest_first {
                store.token_events_page_before(&token_id, before_seq, limit + 1)?
            } else {
                store.token_events_page(&token_id, after_seq, limit + 1)?
            };
            let (rows, next) = trim_complete_event_page(rows, limit);
            let mut out = serde_json::json!({
                "network": network.to_string(),
                "token_id": token_id,
                "generated_at_ms": now_ms(),
                "events_total": store.token_event_count(&token_id)?,
                "order": if newest_first { "desc" } else { "asc" },
                "events": rows.iter().map(token_event_json).collect::<Vec<_>>(),
            });
            if let Some(next) = next {
                out[if newest_first {
                    "next_before_seq"
                } else {
                    "next_after_seq"
                }] = serde_json::json!(next);
            }
            Ok(Some(serde_json::to_string(&out)?))
        },
    )
    .await
}

pub(super) async fn trades_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let network = match resolve_network(&state, &net_name) {
        Ok(network) => network,
        Err(response) => return response,
    };
    let limit = match parse_limit(&q, PAGE_DEFAULT, PAGE_MAX) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let parse_optional_id = |name: &str| {
        q.get(name)
            .map(|value| parse_id(value, "trade filter must be a 64-character covenant id"))
            .transpose()
    };
    let token_id = match parse_optional_id("token_id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let market_id = match parse_optional_id("market_id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let side = match q.get("side").map(String::as_str) {
        None => None,
        Some("buy" | "sell") => q.get("side").cloned(),
        Some(_) => return bad_request("side must be buy or sell"),
    };
    let cursor = match (
        q.get("before_daa"),
        q.get("before_token"),
        q.get("before_seq"),
    ) {
        (None, None, None) => None,
        (Some(daa), Some(token), Some(seq)) => {
            let Ok(daa) = daa.parse::<u64>() else {
                return bad_request("before_daa must be a non-negative integer");
            };
            let token = match parse_id(token, "before_token must be a covenant id") {
                Ok(token) => token,
                Err(response) => return response,
            };
            let Ok(seq) = seq.parse::<u64>() else {
                return bad_request("before_seq must be a non-negative integer");
            };
            Some((daa, token, seq))
        }
        _ => {
            return bad_request(
                "before_daa, before_token, and before_seq must be supplied together",
            )
        }
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let key = format!(
        "{network}/trades?limit={limit}&token={}&market={}&side={}&cursor={:?}",
        token_id.map_or(String::new(), |id| id.to_string()),
        market_id.map_or(String::new(), |id| id.to_string()),
        side.as_deref().unwrap_or(""),
        cursor
            .as_ref()
            .map(|(daa, token, seq)| (*daa, token.to_string(), *seq)),
    );
    serve_cached(
        &state,
        key,
        30,
        "public, max-age=15, s-maxage=30, stale-while-revalidate=120",
        accepts_gzip(&headers),
        move || {
            let store = Store::open(&db, network)?;
            let mut rows = store.global_token_trades_page(
                token_id.as_ref(),
                market_id.as_ref(),
                side.as_deref(),
                cursor.as_ref().map(|(daa, token, seq)| (*daa, token, *seq)),
                limit + 1,
            )?;
            let more = rows.len() as u64 > limit;
            rows.truncate(limit as usize);
            let trades = rows
                .iter()
                .map(|row| {
                    let mut value = trade_json(&row.trade, network)?;
                    value["token_id"] = serde_json::json!(row.token_id);
                    Ok(value)
                })
                .collect::<std::result::Result<Vec<_>, serde_json::Error>>()?;
            let mut out = serde_json::json!({
                "network": network.to_string(),
                "generated_at_ms": now_ms(),
                "trades": trades,
            });
            if more {
                if let Some(last) = rows.last() {
                    out["next_before_daa"] = serde_json::json!(last.trade.accepting_daa);
                    out["next_before_token"] = serde_json::json!(last.token_id);
                    out["next_before_seq"] = serde_json::json!(last.trade.seq);
                }
            }
            Ok(Some(serde_json::to_string(&out)?))
        },
    )
    .await
}

fn market_entry(
    store: &Store,
    token: &kascov_core::tokens::TokenDirRow,
    network: Network,
) -> Result<Option<serde_json::Value>> {
    if token.validation != "verified" {
        return Ok(None);
    }
    let summary = store.token_market_summary(token, true)?;
    if !matches!(summary.phase.as_deref(), Some("bonding" | "graduated")) {
        return Ok(None);
    }
    let Some(program) = summary.program.as_ref() else {
        return Ok(None);
    };
    Ok(Some(serde_json::json!({
        "market_id": program.covenant_id,
        "token": token_row_json(token, store.claimed_token_meta(&token.token_id)?.as_ref()),
        "market": summary,
        "trades_total": store.token_trades_count(&token.token_id)?,
        "market_url": format!("/data/{network}/market/{}", program.covenant_id),
        "trades_url": format!("/data/{network}/trades?market_id={}", program.covenant_id),
    })))
}

fn page_market_entries(
    mut rows: Vec<serde_json::Value>,
    after_id: Option<&str>,
    limit: u64,
) -> (usize, Vec<serde_json::Value>, Option<String>) {
    rows.sort_by(|a, b| b["market_id"].as_str().cmp(&a["market_id"].as_str()));
    let total = rows.len();
    if let Some(after_id) = after_id {
        rows.retain(|row| row["market_id"].as_str().is_some_and(|id| id < after_id));
    }
    let more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let next = more
        .then(|| {
            rows.last()
                .and_then(|row| row["market_id"].as_str())
                .map(str::to_string)
        })
        .flatten();
    (total, rows, next)
}

fn market_detail_value(
    store: &Store,
    market_id: &CovenantId,
    network: Network,
) -> Result<Option<serde_json::Value>> {
    let Some(program) = store.market_program(market_id)? else {
        return Ok(None);
    };
    let tokens = store.tokens_for_market(market_id)?;
    let base = tokens.iter().find(|token| {
        token.validation == "verified"
            && (program.token_covenant_id.as_ref() == Some(&token.token_id)
                || token.market_covenant_id.as_ref() == Some(market_id))
    });
    let Some(base) = base else {
        return Ok(None);
    };
    let summary = store.token_market_summary(base, true)?;
    if summary.program.as_ref().map(|p| &p.covenant_id) != Some(market_id)
        || !matches!(summary.phase.as_deref(), Some("bonding" | "graduated"))
    {
        return Ok(None);
    }
    let token_values = tokens
        .iter()
        .map(|token| {
            let meta = store.claimed_token_meta(&token.token_id)?;
            Ok(token_row_json(token, meta.as_ref()))
        })
        .collect::<Result<Vec<_>>>()?;
    let recent = store
        .token_trades_page_before(&base.token_id, None, 100)?
        .iter()
        .map(|trade| trade_json(trade, network))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    // The live cell a trade must spend, from kascov's own index — so a trade
    // page reads the outpoint, value and committed script same-origin instead
    // of scraping a public node. The program BYTES are not embedded here (172
    // KB); a page reconstructs them from the newest trade's reveal and checks
    // the blake2b against this script. `null` when nothing live is indexed.
    let live_curve = store
        .live_market_utxo(market_id)?
        .map(|u| {
            serde_json::json!({
                "outpoint": format!("{}:{}", hex::encode(u.txid), u.index),
                "txid": hex::encode(u.txid),
                "index": u.index,
                "value_sompi": u.value,
                "script_hex": hex::encode(&u.spk_script),
                "live_count": u.live_count,
            })
        });
    Ok(Some(serde_json::json!({
        "network": network.to_string(),
        "generated_at_ms": now_ms(),
        "market_id": market_id,
        "program": program,
        "tokens": token_values,
        "market": summary,
        "live_curve": live_curve,
        "trades_total": store.token_trades_count(&base.token_id)?,
        "recent_trades": recent,
    })))
}

async fn market_directory_handler(
    state: std::sync::Arc<ServeState>,
    net_name: String,
    q: std::collections::HashMap<String, String>,
    headers: axum::http::HeaderMap,
    pools_only: bool,
) -> axum::response::Response {
    let network = match resolve_network(&state, &net_name) {
        Ok(network) => network,
        Err(response) => return response,
    };
    let limit = match parse_limit(&q, PAGE_DEFAULT, 500) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let phase = q.get("phase").cloned();
    if phase
        .as_deref()
        .is_some_and(|phase| !matches!(phase, "bonding" | "graduated"))
    {
        return bad_request("phase must be bonding or graduated");
    }
    let priced = match q.get("priced").map(String::as_str) {
        None => None,
        Some("true") => Some(true),
        Some("false") => Some(false),
        Some(_) => return bad_request("priced must be true or false"),
    };
    let after_id = match q.get("after_id") {
        Some(value) => match parse_id(value, "after_id must be a covenant id") {
            Ok(id) => Some(id.to_string()),
            Err(response) => return response,
        },
        None => None,
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let label = if pools_only { "pools" } else { "markets" };
    let key = format!(
        "{network}/{label}?limit={limit}&phase={phase:?}&priced={priced:?}&after={after_id:?}"
    );
    serve_cached(
        &state,
        key,
        30,
        "public, max-age=15, s-maxage=30, stale-while-revalidate=120",
        accepts_gzip(&headers),
        move || {
            let store = Store::open(&db, network)?;
            let mut rows = Vec::new();
            for token in store.token_directory()? {
                let Some(entry) = market_entry(&store, &token, network)? else {
                    continue;
                };
                let row_phase = entry["market"]["phase"].as_str();
                if pools_only && row_phase != Some("graduated") {
                    continue;
                }
                if phase
                    .as_deref()
                    .is_some_and(|phase| row_phase != Some(phase))
                {
                    continue;
                }
                let is_priced = entry["market"].get("last_quote_sompi").is_some();
                if priced.is_some_and(|wanted| wanted != is_priced) {
                    continue;
                }
                rows.push(entry);
            }
            let (total, rows, next) = page_market_entries(rows, after_id.as_deref(), limit);
            let mut out = serde_json::json!({
                "network": network.to_string(),
                "generated_at_ms": now_ms(),
            });
            out[format!("{label}_total")] = serde_json::json!(total);
            if let Some(next) = next {
                out["next_after_id"] = serde_json::json!(next);
            }
            out[label] = serde_json::json!(rows);
            Ok(Some(serde_json::to_string(&out)?))
        },
    )
    .await
}

pub(super) async fn markets_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    market_directory_handler(state, net_name, q, headers, false).await
}

pub(super) async fn pools_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    market_directory_handler(state, net_name, q, headers, true).await
}

async fn market_detail_handler(
    state: std::sync::Arc<ServeState>,
    net_name: String,
    id: String,
    headers: axum::http::HeaderMap,
    pool_only: bool,
) -> axum::response::Response {
    let network = match resolve_network(&state, &net_name) {
        Ok(network) => network,
        Err(response) => return response,
    };
    let market_id = match parse_id(&id, "bad market id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let key = format!("{network}/market/{market_id}?pool_only={pool_only}");
    serve_cached(
        &state,
        key,
        30,
        "public, max-age=15, s-maxage=30, stale-while-revalidate=120",
        accepts_gzip(&headers),
        move || {
            let store = Store::open(&db, network)?;
            if pool_only
                && !store
                    .market_program(&market_id)?
                    .is_some_and(|program| program.skeleton.contains("pool"))
            {
                return Ok(None);
            }
            Ok(market_detail_value(&store, &market_id, network)?
                .map(|value| serde_json::to_string(&value))
                .transpose()?)
        },
    )
    .await
}

pub(super) async fn market_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, id)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    market_detail_handler(state, net_name, id, headers, false).await
}

pub(super) async fn pool_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, id)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    market_detail_handler(state, net_name, id, headers, true).await
}

pub(super) async fn token_market_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, id)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let network = match resolve_network(&state, &net_name) {
        Ok(network) => network,
        Err(response) => return response,
    };
    let token_id = match parse_id(&id, "bad token id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let key = format!("{network}/token/{token_id}/market");
    serve_cached(
        &state,
        key,
        30,
        "public, max-age=15, s-maxage=30, stale-while-revalidate=120",
        accepts_gzip(&headers),
        move || {
            let store = Store::open(&db, network)?;
            let Some(token) = store.token_row(&token_id)? else {
                return Ok(None);
            };
            let summary = store.token_market_summary(&token, true)?;
            let market_id = summary
                .program
                .as_ref()
                .map(|program| program.covenant_id)
                .or(summary.lp_of_pool);
            let Some(market_id) = market_id else {
                return Ok(None);
            };
            Ok(market_detail_value(&store, &market_id, network)?
                .map(|value| serde_json::to_string(&value))
                .transpose()?)
        },
    )
    .await
}

async fn refresh_vesting_proofs(state: &ServeState, network: Network) {
    let Some(body) = registry_list_cached().await else {
        return;
    };
    let Ok(entries) = registry::parse_list(&body, &network.to_string()) else {
        return;
    };
    let db = state.base_dir.join(format!("{network}.db"));
    let result = tokio::task::spawn_blocking(move || -> Result<()> {
        let store = Store::open(&db, network)?;
        prove_listed_vesting_schedules(&store, &entries)
    })
    .await;
    if let Ok(Err(error)) = result {
        tracing::warn!("{network}: vesting proof refresh failed: {error}");
    }
}

fn vesting_value(
    store: &Store,
    schedule: &kascov_core::store::VestingScheduleRow,
) -> Result<serde_json::Value> {
    let states = store.vesting_states(schedule)?;
    let live: Vec<_> = states.iter().filter(|state| state.live).collect();
    let current_claimed = match live.as_slice() {
        [one] => Some(one.claimed),
        [] if states
            .last()
            .is_some_and(|state| state.claimed == schedule.total) =>
        {
            Some(schedule.total)
        }
        _ => None,
    };
    let lineage_complete = states
        .last()
        .is_some_and(|state| state.live || state.claimed == schedule.total);
    let tip_daa = store.tip()?.map(|tip| tip.0);
    let vested_at_tip = tip_daa.map(|tip| {
        let elapsed = tip
            .saturating_sub(schedule.start_score)
            .min(schedule.duration_score);
        ((schedule.total as u128 * elapsed as u128) / schedule.duration_score as u128) as u64
    });
    let status = match tip_daa {
        Some(tip) if tip < schedule.start_score => "not_started",
        Some(tip) if tip < schedule.start_score.saturating_add(schedule.duration_score) => {
            "vesting"
        }
        Some(_) => "vested",
        None => "unknown",
    };
    let claimable = vested_at_tip
        .zip(current_claimed)
        .map(|(vested, claimed)| vested.saturating_sub(claimed));
    Ok(serde_json::json!({
        "schedule": schedule,
        "status": status,
        "tip_daa": tip_daa,
        "vested_at_tip": vested_at_tip,
        "current_claimed": current_claimed,
        "claimable_at_tip": claimable,
        "current_state_proven": current_claimed.is_some(),
        "lineage_complete": lineage_complete,
        "states_proven": states.len(),
        "claims_proven": states.iter().filter(|state| state.claimed_delta > 0).count(),
        "proof_note": "schedule and states are published only after reproducing their on-chain P2SH commitments; DAA scores are not timestamps",
    }))
}

pub(super) async fn vesting_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path(net_name): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let network = match resolve_network(&state, &net_name) {
        Ok(network) => network,
        Err(response) => return response,
    };
    let limit = match parse_limit(&q, PAGE_DEFAULT, 500) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let after_id = match q.get("after_id") {
        Some(value) => match parse_id(value, "after_id must be a token covenant id") {
            Ok(id) => Some(id.to_string()),
            Err(response) => return response,
        },
        None => None,
    };
    refresh_vesting_proofs(&state, network).await;
    let db = state.base_dir.join(format!("{network}.db"));
    serve_cached(
        &state,
        format!("{network}/vesting?limit={limit}&after={after_id:?}"),
        30,
        "public, max-age=15, s-maxage=30, stale-while-revalidate=120",
        accepts_gzip(&headers),
        move || {
            let store = Store::open(&db, network)?;
            let mut rows = store.vesting_schedules()?;
            rows.sort_by(|a, b| b.token_id.0.cmp(&a.token_id.0));
            let total = rows.len();
            if let Some(after_id) = after_id.as_deref() {
                rows.retain(|schedule| schedule.token_id.to_string().as_str() < after_id);
            }
            let more = rows.len() as u64 > limit;
            rows.truncate(limit as usize);
            let next = more.then(|| rows.last().map(|row| row.token_id)).flatten();
            let schedules = rows
                .iter()
                .map(|schedule| vesting_value(&store, schedule))
                .collect::<Result<Vec<_>>>()?;
            let mut out = serde_json::json!({
                "network": network.to_string(),
                "generated_at_ms": now_ms(),
                "vesting_total": total,
                "vesting": schedules,
            });
            if let Some(next) = next {
                out["next_after_id"] = serde_json::json!(next);
            }
            Ok(Some(serde_json::to_string(&out)?))
        },
    )
    .await
}

async fn vesting_by_id_handler(
    state: std::sync::Arc<ServeState>,
    net_name: String,
    id: String,
    headers: axum::http::HeaderMap,
    claims_only: bool,
) -> axum::response::Response {
    let network = match resolve_network(&state, &net_name) {
        Ok(network) => network,
        Err(response) => return response,
    };
    let id = match parse_id(&id, "bad token or vesting covenant id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    refresh_vesting_proofs(&state, network).await;
    let db = state.base_dir.join(format!("{network}.db"));
    serve_cached(
        &state,
        format!("{network}/vesting/{id}?claims={claims_only}"),
        30,
        "public, max-age=15, s-maxage=30, stale-while-revalidate=120",
        accepts_gzip(&headers),
        move || {
            let store = Store::open(&db, network)?;
            let Some(schedule) = store.vesting_schedule(&id)? else {
                return Ok(None);
            };
            let states = store.vesting_states(&schedule)?;
            let value = if claims_only {
                serde_json::json!({
                    "network": network.to_string(),
                    "generated_at_ms": now_ms(),
                    "token_id": schedule.token_id,
                    "lock_covenant_id": schedule.lock_covenant_id,
                    "claims": states.iter().filter(|state| state.claimed_delta > 0).collect::<Vec<_>>(),
                    "states_proven": states.len(),
                    "lineage_complete": states.last().is_some_and(|state| state.live || state.claimed == schedule.total),
                })
            } else {
                let mut value = vesting_value(&store, &schedule)?;
                value["network"] = serde_json::json!(network.to_string());
                value["generated_at_ms"] = serde_json::json!(now_ms());
                value["states"] = serde_json::to_value(states)?;
                value
            };
            Ok(Some(serde_json::to_string(&value)?))
        },
    )
    .await
}

pub(super) async fn vesting_detail_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, id)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    vesting_by_id_handler(state, net_name, id, headers, false).await
}

pub(super) async fn vesting_claims_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServeState>>,
    axum::extract::Path((net_name, id)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    vesting_by_id_handler(state, net_name, id, headers, true).await
}

pub(super) fn openapi_document() -> serde_json::Value {
    let responses = serde_json::json!({
        "200": { "description": "Successful JSON response", "content": {
            "application/json": { "schema": { "type": "object" } }
        }},
        "400": { "description": "Malformed path or query input" },
        "404": { "description": "Resource not found" },
        "503": { "description": "Index temporarily unavailable" }
    });
    let network = || serde_json::json!({ "$ref": "#/components/parameters/Network" });
    let id = || serde_json::json!({ "$ref": "#/components/parameters/CovenantId" });
    let path_string = |name: &str, description: &str| {
        serde_json::json!({
            "name": name, "in": "path", "required": true,
            "description": description, "schema": { "type": "string" }
        })
    };
    let query = |name: &str, description: &str, schema: serde_json::Value| {
        serde_json::json!({
            "name": name, "in": "query", "required": false,
            "description": description, "schema": schema
        })
    };
    let get = |summary: &str, operation_id: &str, parameters: Vec<serde_json::Value>| {
        serde_json::json!({
            "get": {
                "summary": summary, "operationId": operation_id,
                "parameters": parameters, "responses": responses.clone()
            }
        })
    };
    let post = |summary: &str, operation_id: &str, parameters: Vec<serde_json::Value>| {
        serde_json::json!({
            "post": {
                "summary": summary, "operationId": operation_id,
                "parameters": parameters,
                "requestBody": { "required": true, "content": {
                    "application/json": { "schema": { "type": "object" } }
                }},
                "responses": responses.clone()
            }
        })
    };
    let sse = |summary: &str, operation_id: &str, parameters: Vec<serde_json::Value>| {
        serde_json::json!({
            "get": {
                "summary": summary, "operationId": operation_id,
                "parameters": parameters,
                "responses": {
                    "200": { "description": "Live server-sent event stream", "content": {
                        "text/event-stream": { "schema": { "type": "string" } }
                    }},
                    "400": { "description": "Malformed filter" },
                    "404": { "description": "Unknown network" }
                }
            }
        })
    };
    let positive_limit = serde_json::json!({ "type": "integer", "minimum": 1 });
    let daa = serde_json::json!({ "$ref": "#/components/schemas/DaaScore" });
    let uint = serde_json::json!({ "type": "integer", "minimum": 0 });
    let bool_schema = serde_json::json!({ "type": "boolean" });
    serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "KasCov API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Open, keyless Kaspa covenant and verified token-market data. Exact monetary values are JSON integers; prices are numerator/denominator pairs. Vesting scores are DAA scores, not timestamps."
        },
        "servers": [{ "url": "https://kascov.io" }],
        "paths": {
            "/health": get("Check indexer health", "health", vec![]),
            "/openapi.json": get("Read this OpenAPI document", "openapi", vec![]),
            "/data/price.json": get("Read the KAS reference price feed", "price", vec![]),
            "/data/{network}.json": get("Page through covenant summaries", "coins", vec![
                network(), query("limit", "Maximum rows", positive_limit.clone()),
                query("after_daa", "Exclusive compound cursor DAA", daa.clone()),
                query("after_id", "Exclusive compound cursor covenant id", serde_json::json!({ "$ref": "#/components/schemas/CovenantId" }))
            ]),
            "/data/{network}-live.json": get("Read the small live network snapshot", "live", vec![network()]),
            "/data/{network}/events": get("Page through chain-wide covenant events", "events", vec![
                network(), query("limit", "Maximum events", positive_limit.clone()),
                query("after_daa", "Cursor DAA", daa.clone()), query("after_seq", "Rows consumed at the cursor DAA", uint.clone())
            ]),
            "/data/{network}/c/{id}.json": get("Get one covenant's full indexed story", "coin", vec![network(), id()]),
            "/data/{network}/coins": get("Get a batch of covenant summaries", "coinsBatch", vec![
                network(), query("ids", "Comma-separated covenant ids", serde_json::json!({ "type": "string" }))
            ]),
            "/data/{network}/tx/{txid}.json": get("Get covenants moved by one transaction", "transaction", vec![network(), path_string("txid", "Transaction id")]),
            "/data/{network}/template/{hash}.json": get("Get one verified KCC-1 template", "template", vec![network(), path_string("hash", "Template hash")]),
            "/data/{network}/verified/{hash}": get("Get byte-identical verified source", "verifiedSource", vec![network(), path_string("hash", "Program hash")]),
            "/data/{network}/families.json": get("List covenant families", "families", vec![network()]),
            "/data/{network}/reorgs.json": get("List handled chain reorgs", "reorgs", vec![network()]),
            "/data/{network}/galaxy.json": get("Read the whole-network covenant graph", "galaxy", vec![network()]),
            "/data/{network}/lanes.json": get("Read lane analytics", "lanes", vec![network()]),
            "/data/{network}/inscriptions.json": get("List inscriptions", "inscriptions", vec![network()]),
            "/data/{network}/lifespans.json": get("Read covenant lifespan analytics", "lifespans", vec![network()]),
            "/data/{network}/digest.json": get("Read the recent network digest", "digest", vec![network()]),
            "/data/{network}/templates.json": get("List template analytics", "templates", vec![network()]),
            "/data/{network}/verification.json": get("Read derivation and audit status", "verification", vec![network()]),
            "/data/{network}/consistency.json": get("Read independent consistency checks", "consistency", vec![network()]),
            "/data/{network}/activity.json": get("Read activity buckets", "activity", vec![
                network(), query("range", "1h, 6h, 24h, 48h, or all", serde_json::json!({ "type": "string", "enum": ["1h", "6h", "24h", "48h", "all"] }))
            ]),
            "/data/{network}/addr/{address}.json": get("Get covenants related to an address or public key", "address", vec![network(), path_string("address", "Kaspa address or public key")]),
            "/data/{network}/search": get("Search indexed covenants", "search", vec![
                network(), query("q", "Search text", serde_json::json!({ "type": "string", "maxLength": 128 }))
            ]),
            "/data/{network}/stream": sse("Stream live covenant events", "stream", vec![
                network(), query("covenant", "Optional exact covenant-id filter", serde_json::json!({ "$ref": "#/components/schemas/CovenantId" }))
            ]),
            "/data/{network}/pending": get("Read pending indexing work", "pending", vec![network()]),
            "/data/{network}/lane": get("Read holder-lane policy", "lanePolicy", vec![network()]),
            "/data/{network}/lane/{ns}": get("Read one payload lane", "lane", vec![network(), path_string("ns", "Eight-hex lane namespace")]),
            "/data/{network}/debug/{txid}": get("Replay and explain one covenant transaction", "debug", vec![network(), path_string("txid", "Transaction id")]),
            "/data/{network}/registry.json": get("Read chain-checked launchpad registry claims", "registry", vec![network()]),
            "/data/{network}/tokens.json": get("List derived KCC20 tokens and minters", "tokens", vec![
                network(), query("limit", "Maximum rows; any query opts into pagination", positive_limit.clone()),
                query("after_daa", "Exclusive compound cursor DAA", daa.clone()),
                query("after_id", "Exclusive compound cursor covenant id", serde_json::json!({ "$ref": "#/components/schemas/CovenantId" })),
                query("status", "Validation or liveness status", serde_json::json!({ "type": "string", "enum": ["verified", "invalid", "unvalidated", "active", "burned"] })),
                query("phase", "Verified market phase", serde_json::json!({ "type": "string", "enum": ["bonding", "graduated"] })),
                query("kind", "Directory row kind", serde_json::json!({ "type": "string", "enum": ["token", "minter"] })),
                query("q", "Case-insensitive id, name, ticker, or template search", serde_json::json!({ "type": "string", "minLength": 1, "maxLength": 128 }))
            ]),
            "/data/{network}/token/{id}": get("Get one token with validation, holders, events, and market", "token", vec![
                network(), id(), query("limit", "Embedded holder limit", positive_limit.clone()),
                query("events_limit", "Embedded event limit", positive_limit.clone()),
                query("after_seq", "Oldest-first event cursor", uint.clone()),
                query("before_seq", "Newest-first event cursor", uint.clone()),
                query("order", "Event order", serde_json::json!({ "type": "string", "enum": ["asc", "desc"] }))
            ]),
            "/data/{network}/token/{id}/holders": get("Page through hash-proven token holders", "tokenHolders", vec![
                network(), id(), query("limit", "Maximum holders", positive_limit.clone()),
                query("after_balance", "Exclusive compound cursor balance", serde_json::json!({ "type": "integer" })),
                query("after_owner", "Opaque exclusive compound cursor owner returned by the previous page", serde_json::json!({ "type": "string", "pattern": "^[0-9a-f]{66}$" }))
            ]),
            "/data/{network}/token/{id}/events": get("Page through classified token events", "tokenEvents", vec![
                network(), id(), query("limit", "Maximum event deltas", positive_limit.clone()),
                query("after_seq", "Oldest-first event cursor", uint.clone()),
                query("before_seq", "Newest-first event cursor", uint.clone()),
                query("order", "Event order", serde_json::json!({ "type": "string", "enum": ["asc", "desc"] }))
            ]),
            "/data/{network}/token/{id}/trades": get("Page through one token's admitted trades", "tokenTrades", vec![
                network(), id(), query("limit", "Maximum trades", positive_limit.clone()),
                query("before_seq", "Exclusive newest-first trade cursor", uint.clone())
            ]),
            "/data/{network}/token/{id}/trades.json": get("Read or page through one token's admitted trades", "tokenTradesJson", vec![
                network(), id(), query("limit", "Maximum trades", positive_limit.clone()),
                query("before_seq", "Exclusive newest-first trade cursor", uint.clone())
            ]),
            "/data/{network}/token/{id}/market": get("Resolve a token to its verified market", "tokenMarket", vec![network(), id()]),
            "/data/{network}/trades": get("Page through admitted trades across all tokens", "trades", vec![
                network(), query("limit", "Maximum trades", positive_limit.clone()),
                query("token_id", "Filter by token", serde_json::json!({ "$ref": "#/components/schemas/CovenantId" })),
                query("market_id", "Filter by market", serde_json::json!({ "$ref": "#/components/schemas/CovenantId" })),
                query("side", "Filter by side", serde_json::json!({ "type": "string", "enum": ["buy", "sell"] })),
                query("before_daa", "Compound cursor DAA", daa.clone()),
                query("before_token", "Compound cursor token", serde_json::json!({ "$ref": "#/components/schemas/CovenantId" })),
                query("before_seq", "Compound cursor token-local trade sequence", uint.clone())
            ]),
            "/data/{network}/markets": get("List verified bonding curves and graduated pools", "markets", vec![
                network(), query("limit", "Maximum markets", positive_limit.clone()),
                query("after_id", "Exclusive market-id cursor", serde_json::json!({ "$ref": "#/components/schemas/CovenantId" })),
                query("phase", "Market phase", serde_json::json!({ "type": "string", "enum": ["bonding", "graduated"] })),
                query("priced", "Require or exclude an admitted price", bool_schema.clone())
            ]),
            "/data/{network}/market/{id}": get("Get one verified market", "market", vec![network(), id()]),
            "/data/{network}/pools": get("List verified graduated pools", "pools", vec![
                network(), query("limit", "Maximum pools", positive_limit.clone()),
                query("after_id", "Exclusive pool-id cursor", serde_json::json!({ "$ref": "#/components/schemas/CovenantId" })),
                query("priced", "Require or exclude an admitted price", bool_schema.clone())
            ]),
            "/data/{network}/pool/{id}": get("Get one verified pool", "pool", vec![network(), id()]),
            "/data/{network}/vesting": get("List commitment-proven vesting schedules", "vesting", vec![
                network(), query("limit", "Maximum schedules", positive_limit.clone()),
                query("after_id", "Exclusive token-id cursor", serde_json::json!({ "$ref": "#/components/schemas/CovenantId" }))
            ]),
            "/data/{network}/vesting/{id}": get("Get a proven vesting schedule and states", "vestingDetail", vec![network(), id()]),
            "/data/{network}/vesting/{id}/claims": get("List commitment-proven vesting claims", "vestingClaims", vec![network(), id()]),
            "/data/{network}/token/{id}/candles": get("Get OHLC candles built only from replay-verified trades; each bucket carries txids so every candle is replayable", "tokenCandles", vec![
                network(), id(),
                query("bucket", "Candle duration", serde_json::json!({ "type": "string", "enum": ["1h", "4h", "1d"] }))
            ]),
            "/data/{network}/token/{id}/book": get("List open resting orders, served as decoded facts with an explicit not-verified provenance note", "tokenBook", vec![network(), id()]),
            "/data/{network}/index.json": get("Discover the machine-readable API surface", "index", vec![network()]),
            "/data/{network}/simulate": post("Simulate a covenant action", "simulate", vec![network()]),
            "/data/{network}/preflight": post("Preflight a transaction", "preflight", vec![network()]),
            "/data/{network}/zk-verify": post("Verify a supported zero-knowledge proof", "zkVerify", vec![network()]),
            "/data/{network}/compile": post("Compile a supported covenant template", "compile", vec![network()]),
            "/data/{network}/deploy": post("Build a deployment transaction", "deploy", vec![network()]),
            "/data/{network}/publish": post("Publish supported metadata", "publish", vec![network()]),
            "/data/{network}/lane/mint": post("Mint a capacity lane token", "laneMint", vec![network()]),
            "/data/{network}/subscribe": post("Create a subscription", "subscribe", vec![network()]),
            "/data/{network}/unsubscribe": post("Remove a subscription", "unsubscribe", vec![network()]),
            "/data/{network}/prove-holding": post("Build a privacy-preserving holding proof", "proveHolding", vec![network()])
        },
        "components": {
            "parameters": {
                "Network": {
                    "name": "network", "in": "path", "required": true,
                    "description": "Indexed Kaspa network",
                    "schema": { "type": "string", "enum": ["mainnet", "testnet-10"] }
                },
                "CovenantId": {
                    "name": "id", "in": "path", "required": true,
                    "description": "Lowercase 32-byte covenant id",
                    "schema": { "$ref": "#/components/schemas/CovenantId" }
                }
            },
            "schemas": {
                "CovenantId": { "type": "string", "pattern": "^[0-9a-f]{64}$" },
                "ExactPrice": { "type": "object", "required": ["quote_sompi", "base_amount"], "properties": {
                    "quote_sompi": { "type": "integer", "format": "int64" },
                    "base_amount": { "type": "integer", "format": "int64" }
                }},
                "DaaScore": { "type": "integer", "format": "int64", "minimum": 0 },
                "ExactAmount": {
                    "type": "integer", "format": "int64",
                    "description": "Exact on-chain integer amount; never a floating-point display value"
                }
            }
        }
    })
}

pub(super) async fn openapi_handler() -> axum::response::Response {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/json; charset=utf-8",
            ),
            (axum::http::header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        openapi_document().to_string(),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_is_standard_and_names_every_new_endpoint() {
        let doc = openapi_document();
        assert_eq!(doc["openapi"], "3.1.0");
        assert_eq!(doc["info"]["title"], "KasCov API");
        let paths = doc["paths"].as_object().unwrap();
        for path in [
            "/data/{network}/tokens.json",
            "/data/{network}/token/{id}/holders",
            "/data/{network}/token/{id}/events",
            "/data/{network}/token/{id}/trades",
            "/data/{network}/trades",
            "/data/{network}/markets",
            "/data/{network}/pools",
            "/data/{network}/vesting",
            "/data/{network}/vesting/{id}/claims",
            "/data/{network}/token/{id}/candles",
            "/data/{network}/token/{id}/book",
        ] {
            assert!(paths.contains_key(path), "missing {path}");
        }
        let serialized = doc.to_string().to_ascii_lowercase();
        assert!(!serialized.contains("openai"));
        assert!(!serialized.contains("codex"));
    }

    #[test]
    fn openapi_operations_have_unique_ids_and_bound_path_parameters() {
        let doc = openapi_document();
        let mut operation_ids = std::collections::BTreeSet::new();
        for (path, item) in doc["paths"].as_object().unwrap() {
            let operation = item
                .get("get")
                .or_else(|| item.get("post"))
                .expect("every path has an operation");
            let operation_id = operation["operationId"].as_str().unwrap();
            assert!(
                operation_ids.insert(operation_id),
                "duplicate {operation_id}"
            );
            let parameter_names = operation["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|parameter| {
                    parameter["name"]
                        .as_str()
                        .or_else(|| match parameter["$ref"].as_str() {
                            Some("#/components/parameters/Network") => Some("network"),
                            Some("#/components/parameters/CovenantId") => Some("id"),
                            _ => None,
                        })
                })
                .collect::<std::collections::BTreeSet<_>>();
            for piece in path.split('{').skip(1) {
                let name = piece.split('}').next().unwrap();
                assert!(
                    parameter_names.contains(name),
                    "{path} does not bind path parameter {name}"
                );
            }
            assert!(operation["responses"].get("200").is_some());
        }
    }

    #[test]
    fn event_page_trimming_never_splits_one_events_deltas() {
        let row = |seq, delta_idx| kascov_core::tokens::TokenEventRow {
            seq,
            delta_idx,
            kind: "transfer".into(),
            amount: Some(1),
            owner_from: None,
            owner_to: None,
            accepting_daa: 10 + seq,
            tx_index: None,
            txid: TxId([seq as u8; 32]),
            event_kind: "transition".into(),
        };
        let rows = vec![row(1, 0), row(1, 1), row(2, 0), row(2, 1)];
        let (page, next) = trim_complete_event_page(rows, 1);
        assert_eq!(page.len(), 2);
        assert!(page.iter().all(|row| row.seq == 1));
        assert_eq!(next, Some(1));
    }

    #[test]
    fn market_pages_use_a_stable_exclusive_id_cursor() {
        let row = |byte: u8| serde_json::json!({ "market_id": format!("{byte:064x}") });
        let rows = vec![row(1), row(3), row(2)];
        let (total, first, next) = page_market_entries(rows.clone(), None, 2);
        assert_eq!(total, 3);
        assert_eq!(first[0]["market_id"], format!("{:064x}", 3));
        assert_eq!(next, Some(format!("{:064x}", 2)));
        let (_, second, next) =
            page_market_entries(rows, first.last().unwrap()["market_id"].as_str(), 2);
        assert_eq!(second, vec![row(1)]);
        assert_eq!(next, None);
    }
}
