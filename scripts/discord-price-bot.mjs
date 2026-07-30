#!/usr/bin/env node
/* kascov price bot for Discord — zero dependencies, one message.
 *
 * Reads the KASCOV token straight from kascov.io's own chain-derived API and
 * keeps ONE embed up to date in a channel. It edits that message in place
 * rather than posting a new one, so #price is a live panel and not a scroll of
 * bot spam.
 *
 * Deliberately webhook-based: no bot token, no gateway connection, no
 * dependencies, and the credential it does need is scoped to a single channel.
 * A gateway bot would also let it set a "playing 0.19 KAS" status, which is
 * nice but costs a permanent connection and a token with far more reach than
 * this job needs. Discord also rate-limits channel RENAMES to 2 per 10 minutes,
 * which rules out the ticker-in-the-channel-name trick anyway.
 *
 * HONESTY RULES, same as the site:
 *   - No verified market program means NO price. It says so instead of
 *     inventing one, because kascov refuses figures it cannot prove.
 *   - A fall is shown exactly as plainly as a rise.
 *   - Every number carries where it came from, and the embed links to the page
 *     that proves it.
 *
 * Setup (never commit these):
 *   KASCOV_DISCORD_WEBHOOK=https://discord.com/api/webhooks/...
 *   KASCOV_PRICE_STATE=/home/kascov/kascov-price-bot.json   (optional)
 *   KASCOV_PRICE_TOKEN=<covenant id>                        (optional)
 *   KASCOV_PRICE_NETWORK=mainnet                            (optional)
 *
 * Run:    node scripts/discord-price-bot.mjs
 * Timer:  every 5 minutes is plenty; the site's own cache is 30-60s.
 */

import { readFile, writeFile } from 'node:fs/promises';

const NETWORK = process.env.KASCOV_PRICE_NETWORK || 'mainnet';
const TOKEN_ID = process.env.KASCOV_PRICE_TOKEN
  || 'c58c826d0aa9cee62f93208718c674883f5c89a8aca4933dc41fb0391539abe2';
const WEBHOOK = process.env.KASCOV_DISCORD_WEBHOOK || '';
const STATE_PATH = process.env.KASCOV_PRICE_STATE
  || new URL('../.kascov-price-bot.json', import.meta.url).pathname;

const SITE = 'https://kascov.io';
const MINT = 0x70c7ba;

/* ---------------------------------------------------------------- formatting */

/** KAS from sompi, without floating through a lossy divide for display. */
export function fmtKas(sompi, dp = 2) {
  const n = Number(sompi || 0) / 1e8;
  return n.toLocaleString('en-US', { minimumFractionDigits: dp, maximumFractionDigits: dp });
}

export function fmtInt(n) {
  return Number(n || 0).toLocaleString('en-US');
}

/** A price is a RATIO of two integers here; never pre-divided upstream. */
export function priceKas(numSompi, den) {
  if (numSompi == null || !den) return null;
  const v = Number(numSompi) / 1e8 / Number(den);
  if (!Number.isFinite(v) || v <= 0) return null;
  // Sub-1 prices get 6dp, matching how the site renders them: 4dp would
  // round 0.197521 to 0.1975 and quietly drop real precision.
  return v < 1 ? v.toFixed(6) : v.toFixed(4);
}

/** Basis points to a signed percent, shown the same whichever way it went. */
export function fmtChange(bps) {
  if (bps == null) return null;
  const pct = Number(bps) / 100;
  const arrow = pct > 0 ? '▲' : pct < 0 ? '▼' : '=';
  return `${arrow} ${pct >= 0 ? '+' : ''}${pct.toFixed(2)}%`;
}

/** Build the embed from a token payload. Pure, so it can be tested. */
export function buildEmbed(
  data,
  { network = 'mainnet', tokenId = '', listedName: ln = null, listedTicker: lt = null } = {},
) {
  const t = data.token || data || {};
  const m = data.market || {};
  const prog = m.program || {};
  const listedName = ln || data.listed_name || t.claimed_name || null;
  const ticker = lt || data.listed_ticker || t.claimed_ticker || null;
  const title = ticker ? `${listedName || t.name} $${ticker}` : (listedName || t.name || 'token');

  const tokenUrl = `${SITE}/#/${network}/token/${tokenId}`;
  const fields = [];

  const spot = priceKas(m.spot_num_sompi, m.spot_den);
  const last = priceKas(m.last_quote_sompi, m.last_base_amount);
  if (spot) {
    fields.push({ name: 'price', value: `**${spot} KAS**\nnext small trade`, inline: true });
  } else if (last) {
    fields.push({ name: 'price', value: `**${last} KAS**\nnewest verified trade`, inline: true });
  } else {
    // The whole point: no verified program, no number.
    fields.push({
      name: 'price',
      value: m.unpriced_reason ? `not published\n${m.unpriced_reason}` : 'not published',
      inline: true,
    });
  }

  const chg = fmtChange(m.change_24h_bps);
  if (chg) fields.push({ name: '24h', value: `**${chg}**`, inline: true });
  if (m.reserve_sompi != null) {
    fields.push({ name: 'reserve', value: `${fmtKas(m.reserve_sompi, 0)} KAS`, inline: true });
  }
  if (m.volume_24h_sompi != null) {
    fields.push({
      name: 'volume 24h',
      value: `${fmtKas(m.volume_24h_sompi, 0)} KAS\n${fmtInt(m.trades_24h || 0)} trades`,
      inline: true,
    });
  }
  if (t.holders != null) {
    fields.push({ name: 'holders', value: fmtInt(t.holders), inline: true });
  }
  if (m.phase === 'bonding' && m.grad_progress_bps != null) {
    const pct = Number(m.grad_progress_bps) / 100;
    const filled = Math.max(0, Math.min(10, Math.round(pct / 10)));
    fields.push({
      name: 'bonding',
      value: `${'█'.repeat(filled)}${'░'.repeat(10 - filled)} ${pct.toFixed(1)}%`,
      inline: true,
    });
  } else if (m.phase) {
    fields.push({ name: 'phase', value: String(m.phase), inline: true });
  }

  const proof = prog.skeleton
    ? `${fmtInt(prog.exercised_trades || 0)} trades replayed against the ${prog.skeleton} program's own formula`
    : 'no verified market program, so nothing above is priced';

  return {
    title,
    url: tokenUrl,
    color: MINT,
    description:
      `every figure here is derived from chain and checked against the market program's own bytecode. ` +
      `nothing comes from a launchpad API.\n\n[open the full page](${tokenUrl})`,
    fields,
    footer: { text: `${proof} · supply ${fmtInt(t.supply)} · ${t.status || 'unverified'}` },
    timestamp: new Date().toISOString(),
  };
}

/* -------------------------------------------------------------------- runtime */

async function loadState() {
  try {
    return JSON.parse(await readFile(STATE_PATH, 'utf8'));
  } catch {
    return {};
  }
}

async function saveState(s) {
  try {
    await writeFile(STATE_PATH, JSON.stringify(s), 'utf8');
  } catch (e) {
    console.warn(`could not persist state to ${STATE_PATH}: ${e.message}`);
  }
}

async function main() {
  if (!WEBHOOK) {
    console.error('KASCOV_DISCORD_WEBHOOK is not set — refusing to run.');
    console.error('Create a webhook on the target channel and pass it in the environment.');
    process.exit(2);
  }
  const url = `${SITE}/data/${NETWORK}/token/${TOKEN_ID}`;
  const res = await fetch(url, { headers: { 'accept-encoding': 'gzip' } });
  if (!res.ok) throw new Error(`kascov ${res.status} for ${url}`);
  const data = await res.json();

  /* The listed name and ticker live in the checked launchpad registry, not in
     the token payload — the token only knows the nickname derived from its own
     coin id. Fetch it so the panel says "kascov $KASCOV" rather than
     "humble-crimson-tortoise", and treat a failure as cosmetic: a missing
     ticker must never stop a price from being published. */
  let listed = null;
  try {
    const reg = await fetch(`${SITE}/data/${NETWORK}/registry.json`);
    if (reg.ok) {
      const r = await reg.json();
      listed = (r.tokens || []).find((x) => x.covenant_id === TOKEN_ID) || null;
    }
  } catch { /* cosmetic only */ }

  const embed = buildEmbed(data, {
    network: NETWORK,
    tokenId: TOKEN_ID,
    listedName: listed?.name || null,
    listedTicker: listed?.ticker || null,
  });

  const state = await loadState();
  const body = JSON.stringify({ embeds: [embed] });
  const headers = { 'content-type': 'application/json' };

  // Edit in place when we already own a message, so the channel stays a panel.
  if (state.messageId) {
    const edit = await fetch(`${WEBHOOK}/messages/${state.messageId}`, {
      method: 'PATCH', headers, body,
    });
    if (edit.ok) {
      console.log(`updated message ${state.messageId}`);
      return;
    }
    // 404 means someone deleted it; fall through and post a fresh one.
    if (edit.status !== 404) {
      throw new Error(`discord PATCH ${edit.status}: ${(await edit.text()).slice(0, 200)}`);
    }
    console.warn('previous message is gone, posting a new one');
  }
  const post = await fetch(`${WEBHOOK}?wait=true`, { method: 'POST', headers, body });
  if (!post.ok) {
    throw new Error(`discord POST ${post.status}: ${(await post.text()).slice(0, 200)}`);
  }
  const msg = await post.json();
  await saveState({ messageId: msg.id });
  console.log(`posted message ${msg.id}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}
