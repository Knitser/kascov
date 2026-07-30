#!/usr/bin/env node
/* kascov changelog poster for Discord — zero dependencies.
 *
 * Watches kascov.io/changelog.json and posts anything new into #changelog, so
 * the server carries ship notes without anyone copying them across by hand.
 *
 * Reads the JSON rather than /feed.xml: the JSON is already the canonical
 * shape the site itself renders from, so there is no XML parser and no second
 * definition of what an entry is. An entry's identity is `date|title`, exactly
 * the stamp `changelogStamp()` uses in web/app.js — date alone repeats when two
 * things ship the same day, which they routinely do.
 *
 * TWO RULES THAT MATTER:
 *   - The FIRST run posts nothing. It records the newest stamp and exits.
 *     Otherwise standing this up dumps 47 entries into a fresh channel.
 *   - New entries post OLDEST first, so the channel reads in the order things
 *     actually happened rather than backwards.
 *
 * Setup (never commit these):
 *   KASCOV_CHANGELOG_WEBHOOK=https://discord.com/api/webhooks/...
 *   KASCOV_CHANGELOG_STATE=/home/kascov/kascov-changelog-bot.json  (optional)
 *
 * Run:    node scripts/discord-changelog-bot.mjs
 *         node scripts/discord-changelog-bot.mjs --dry-run   # print, don't post
 *         node scripts/discord-changelog-bot.mjs --replay=3  # post newest 3 anyway
 * Timer:  every 15 minutes is plenty; shipping is not that fast.
 */

import { readFile, writeFile } from 'node:fs/promises';

const SITE = process.env.KASCOV_BASE || 'https://kascov.io';
const WEBHOOK = process.env.KASCOV_CHANGELOG_WEBHOOK || '';
const STATE_PATH = process.env.KASCOV_CHANGELOG_STATE
  || new URL('../.kascov-changelog-bot.json', import.meta.url).pathname;
const DRY = process.argv.includes('--dry-run');
const REPLAY = Number((process.argv.find((a) => a.startsWith('--replay=')) || '').split('=')[1] || 0);

const MINT = 0x70c7ba;

/* ---------------------------------------------------------------- pure bits */

/** An entry's identity. Same rule as web/app.js: date alone is not unique. */
export function stampOf(e) {
  return `${e?.date || ''}|${e?.title || ''}`;
}

/**
 * What still needs posting, oldest first.
 *
 * `seen` is the set of stamps already posted. A first run (empty `seen`)
 * returns nothing at all — the caller records the current stamps instead, so
 * standing the bot up is silent rather than a 47-message flood.
 */
export function pendingEntries(list, seen, { firstRun = false } = {}) {
  if (!Array.isArray(list)) return [];
  if (firstRun) return [];
  const fresh = list.filter((e) => e && !seen.has(stampOf(e)));
  // changelog.json is newest-first; post in the order things happened.
  return fresh.slice().reverse();
}

/** One entry as a Discord embed. */
export function buildEntryEmbed(e) {
  const body = String(e.body || '');
  return {
    title: String(e.title || 'shipped'),
    url: `${SITE}/#/changelog`,
    color: MINT,
    description: body.length > 4000 ? `${body.slice(0, 3997)}...` : body,
    footer: { text: `kascov · ${e.date || ''}` },
  };
}

/* -------------------------------------------------------------------- runtime */

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/* One scheduled run gets no second chance, so ride out a cold cache or a worker
   restart. Retries 5xx and network errors, never 4xx (that is our bug). */
async function retrying(label, attempt, { retryNetworkErrors = true } = {}) {
  let last;
  for (let i = 1; i <= 3; i++) {
    try {
      const res = await attempt(AbortSignal.timeout(20_000));
      if (res.ok) return res;
      if (res.status < 500) throw new Error(`${label} HTTP ${res.status}`);
      last = new Error(`${label} HTTP ${res.status}`);
    } catch (err) {
      if (!retryNetworkErrors || /HTTP 4\d\d/.test(err.message)) throw err;
      last = err;
    }
    if (i < 3) {
      console.error(`${label} attempt ${i} failed (${last.message}); retrying...`);
      await sleep(i * 2000);
    }
  }
  throw last;
}

async function loadState() {
  try {
    const raw = JSON.parse(await readFile(STATE_PATH, 'utf8'));
    return { seen: new Set(raw.seen || []), known: true };
  } catch {
    return { seen: new Set(), known: false };
  }
}

async function saveState(seen) {
  // Keep the tail bounded; the changelog only ever grows.
  const list = [...seen].slice(-500);
  try {
    await writeFile(STATE_PATH, JSON.stringify({ seen: list }), 'utf8');
  } catch (e) {
    console.warn(`could not persist state to ${STATE_PATH}: ${e.message}`);
  }
}

async function main() {
  if (!WEBHOOK && !DRY) {
    console.error('KASCOV_CHANGELOG_WEBHOOK is not set — refusing to run.');
    console.error('Create a webhook on #changelog and pass it in the environment.');
    process.exit(2);
  }

  const res = await retrying('changelog.json', (signal) =>
    fetch(`${SITE}/changelog.json`, {
      headers: { 'user-agent': 'kascov-changelog-bot' },
      cache: 'no-cache',
      signal,
    }));
  const list = await res.json();
  if (!Array.isArray(list) || !list.length) throw new Error('changelog.json was empty');

  const { seen, known } = await loadState();
  const firstRun = !known;

  let toPost = pendingEntries(list, seen, { firstRun });
  if (REPLAY > 0) toPost = list.slice(0, REPLAY).reverse(); // deliberate re-post

  if (firstRun && !REPLAY) {
    for (const e of list) seen.add(stampOf(e));
    await saveState(seen);
    console.log(`first run: recorded ${list.length} existing entries, posted nothing`);
    return;
  }

  if (!toPost.length) {
    console.log('nothing new');
    return;
  }

  for (const e of toPost) {
    const embed = buildEntryEmbed(e);
    if (DRY) {
      console.log(`[dry-run] ${embed.footer.text} — ${embed.title}`);
    } else {
      // No retry on a send: a timeout leaves the outcome unknown and a repeat
      // would double-post into a channel people are reading.
      const post = await fetch(WEBHOOK, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ embeds: [embed] }),
      });
      if (!post.ok) {
        throw new Error(`discord POST ${post.status}: ${(await post.text()).slice(0, 200)}`);
      }
      // Only mark it seen once Discord actually took it.
      await sleep(1200); // stay well under the webhook rate limit
    }
    seen.add(stampOf(e));
    await saveState(seen);
  }
  console.log(`posted ${toPost.length} entr${toPost.length === 1 ? 'y' : 'ies'}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}
