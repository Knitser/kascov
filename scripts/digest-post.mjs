#!/usr/bin/env node
/* kascov daily digest poster — fetches /digest.json and posts a short
   "today on Kaspa smart coins" update to a Telegram channel.

   Usage:
     node scripts/digest-post.mjs --dry-run              # print, don't post
     TELEGRAM_BOT_TOKEN=… TELEGRAM_CHAT_ID=… node scripts/digest-post.mjs

   Runs from .github/workflows/digest.yml on a cron; secrets live in GitHub
   repo secrets, never in this repo. X/Twitter posting can be added later —
   the formatted text below is already tweet-sized. */

const BASE = process.env.KASCOV_BASE || 'https://kascov.io';
const NETWORK = process.env.KASCOV_NETWORK || 'mainnet';
const DRY = process.argv.includes('--dry-run');

const fmt = (n) => Number(n || 0).toLocaleString('en-US');
const tkas = (sompi, net) =>
  `${(Number(sompi || 0) / 1e8).toLocaleString('en-US', { maximumFractionDigits: 2 })} ${net === 'mainnet' ? 'KAS' : 'TKAS'}`;

/* One cron run a day gets no second chance, so ride out a cold cache or a
   worker restart rather than failing the workflow: three tries, 20s ceiling
   each, backing off 2s → 6s. Retries 5xx and network errors, not 4xx. */
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function retrying(label, attempt, { retryNetworkErrors = true } = {}) {
  let last;
  for (let i = 1; i <= 3; i++) {
    try {
      const res = await attempt(AbortSignal.timeout(20_000));
      if (res.ok) return res;
      if (res.status < 500) throw new Error(`${label} HTTP ${res.status}`); // our bug, retrying won't help
      last = new Error(`${label} HTTP ${res.status}`);
    } catch (e) {
      // A timeout/socket error leaves the outcome unknown. Safe to repeat for a
      // GET; for a send it could double-post, so those callers opt out.
      if (!retryNetworkErrors || /HTTP 4\d\d/.test(e.message)) throw e;
      last = e;
    }
    if (i < 3) {
      console.error(`${label} attempt ${i} failed (${last.message}); retrying…`);
      await sleep(i * 2000);
    }
  }
  throw last;
}

async function main() {
  const res = await retrying('digest.json', (signal) =>
    fetch(`${BASE}/data/${NETWORK}/digest.json`, { headers: { 'user-agent': 'kascov-digest-bot' }, signal }));
  const d = await res.json();

  const quiet = !d.births && !d.moves && !d.burns;
  const lines = [
    `🔭 today on Kaspa smart coins (${NETWORK})`,
    '',
    quiet
      ? 'a quiet day — no covenant activity in the last 24h.'
      : `🌱 ${fmt(d.births)} born · ⇄ ${fmt(d.moves)} moves · 🔥 ${fmt(d.burns)} retired`,
  ];
  if (d.value_born > 0) lines.push(`💰 ${tkas(d.value_born, NETWORK)} wrapped into new coins`);
  if (d.active_now != null) lines.push(`● ${fmt(d.active_now)} coins alive right now`);
  if (d.busiest && d.busiest.covenant_id) {
    lines.push('', `busiest coin: ${BASE}/#/${NETWORK}/c/${d.busiest.covenant_id} (${fmt(d.busiest.events)} events)`);
  }
  lines.push('', `${BASE}`);
  const text = lines.join('\n');

  if (DRY) {
    console.log('--- dry run — would post: ---');
    console.log(text);
    return;
  }

  const token = process.env.TELEGRAM_BOT_TOKEN;
  const chat = process.env.TELEGRAM_CHAT_ID;
  if (!token || !chat) throw new Error('TELEGRAM_BOT_TOKEN / TELEGRAM_CHAT_ID not set (use --dry-run to preview)');
  const tg = await retrying('telegram', (signal) =>
    fetch(`https://api.telegram.org/bot${token}/sendMessage`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ chat_id: chat, text, disable_web_page_preview: false }),
      signal,
    }), { retryNetworkErrors: false });
  const out = await tg.json();
  if (!out.ok) throw new Error(`telegram: ${JSON.stringify(out)}`);
  console.log(`posted digest to ${chat} (message ${out.result.message_id})`);
}

main().catch((e) => { console.error(e.message || e); process.exit(1); });
