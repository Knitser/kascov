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

async function main() {
  const res = await fetch(`${BASE}/data/${NETWORK}/digest.json`, { headers: { 'user-agent': 'kascov-digest-bot' } });
  if (!res.ok) throw new Error(`digest.json HTTP ${res.status}`);
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
  const tg = await fetch(`https://api.telegram.org/bot${token}/sendMessage`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ chat_id: chat, text, disable_web_page_preview: false }),
  });
  const out = await tg.json();
  if (!out.ok) throw new Error(`telegram: ${JSON.stringify(out)}`);
  console.log(`posted digest to ${chat} (message ${out.result.message_id})`);
}

main().catch((e) => { console.error(e.message || e); process.exit(1); });
