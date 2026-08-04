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
 *   KASCOV_DISCORD_WEBHOOK_HOLDERS=https://discord.com/api/webhooks/...  (optional)
 *   KASCOV_HOLDER_LEAD_SECONDS=0  (optional; only read when the holder webhook is set)
 *
 * The holder webhook is an early-access channel for ship notes about TOOLS —
 * per the /bot doctrine it never carries findings about live mainnet tokens,
 * and the public channel always gets every entry. When it is set, each run
 * posts new entries to the holder channel first, then to the public one after
 * KASCOV_HOLDER_LEAD_SECONDS (default 0 = simultaneous; the operator sets the
 * lead per release). Unset means exactly the old single-channel behavior.
 *
 * Run:    node scripts/discord-changelog-bot.mjs
 *         node scripts/discord-changelog-bot.mjs --dry-run   # print, don't post
 *         node scripts/discord-changelog-bot.mjs --replay=3  # post newest 3 anyway
 * Timer:  every 15 minutes is plenty; shipping is not that fast.
 */

import { readFile, writeFile } from 'node:fs/promises';

const SITE = process.env.KASCOV_BASE || 'https://kascov.io';
const WEBHOOK = process.env.KASCOV_CHANGELOG_WEBHOOK || '';
/* Optional second channel. Empty means the feature is off and the bot behaves
   exactly as it always has — no crash, no half-open gate. */
const HOLDER_WEBHOOK = process.env.KASCOV_DISCORD_WEBHOOK_HOLDERS || '';
/* A malformed or negative lead degrades to 0 (simultaneous), never to a wait. */
const HOLDER_LEAD_MS = Math.max(0, Number(process.env.KASCOV_HOLDER_LEAD_SECONDS) || 0) * 1000;
const STATE_PATH = process.env.KASCOV_CHANGELOG_STATE
  || new URL('../.kascov-changelog-bot.json', import.meta.url).pathname;
const DRY = process.argv.includes('--dry-run');
const REPLAY = Number((process.argv.find((a) => a.startsWith('--replay=')) || '').split('=')[1] || 0);
/* Discord allows roughly 30 webhook messages a minute per channel. 2.5s is 24
   a minute, which leaves headroom and matters only when back-filling history:
   a normal run posts nothing or one thing, so a slower default costs nothing. */
const DELAY_MS = Number(process.env.KASCOV_CHANGELOG_DELAY_MS || 2500);

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

/**
 * The channels one run must serve, in send order.
 *
 * Each channel keeps its OWN seen-set and its own first-run silence, so
 * standing up the holder webhook months after the public one floods nothing.
 * The holder channel, when configured, always comes first — the caller sleeps
 * the published lead between the two passes. Without a holder webhook the
 * plan is exactly the old single-channel behavior.
 */
export function deliveryPlan(list, {
  seen, firstRun = false,
  holdersEnabled = false, holdersSeen = new Set(), holdersFirstRun = false,
} = {}) {
  const plan = [];
  if (holdersEnabled) {
    plan.push({
      channel: 'holders',
      entries: pendingEntries(list, holdersSeen, { firstRun: holdersFirstRun }),
    });
  }
  plan.push({ channel: 'public', entries: pendingEntries(list, seen, { firstRun }) });
  return plan;
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

/**
 * Send one embed, honouring Discord's rate limiter.
 *
 * A 429 is not a failure here, it is the server stating the pace. Retrying it
 * is safe in a way that retrying a timeout is not: Discord explicitly REJECTED
 * the message, so re-sending cannot double-post. A timed-out request has an
 * unknown outcome, which is why those are still never retried.
 */
export async function postEmbed(embed, { webhook = WEBHOOK, fetchImpl = fetch, sleepImpl = sleep } = {}) {
  const body = JSON.stringify({ embeds: [embed] });
  for (let attempt = 1; attempt <= 4; attempt++) {
    const res = await fetchImpl(webhook, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body,
    });
    if (res.ok) return;
    if (res.status === 429) {
      let wait = 5;
      try {
        wait = Number((await res.json())?.retry_after) || 5; /* seconds, may be fractional */
      } catch { /* a non-JSON 429 body; the default stands */ }
      console.error(`rate limited, waiting ${wait}s`);
      await sleepImpl(Math.min(wait * 1000 + 250, 60_000));
      continue;
    }
    throw new Error(`discord POST ${res.status}: ${String(await res.text()).slice(0, 200)}`);
  }
  throw new Error('discord kept rate limiting after 4 attempts');
}

async function loadState() {
  try {
    const raw = JSON.parse(await readFile(STATE_PATH, 'utf8'));
    return {
      seen: new Set(raw.seen || []),
      known: true,
      // the holder channel has its own baseline: a state file written before
      // the holder webhook existed has no `holders` key, and that absence is
      // what makes the holder channel's first run silent.
      holdersSeen: new Set(raw.holders || []),
      holdersKnown: Array.isArray(raw.holders),
    };
  } catch {
    return { seen: new Set(), known: false, holdersSeen: new Set(), holdersKnown: false };
  }
}

async function saveState(seen, holdersSeen = null) {
  // Keep the tail bounded; the changelog only ever grows.
  const state = { seen: [...seen].slice(-500) };
  // only write the holders key once that channel has a baseline — writing an
  // empty one early would silently skip its first-run silence later.
  if (holdersSeen) state.holders = [...holdersSeen].slice(-500);
  try {
    await writeFile(STATE_PATH, JSON.stringify(state), 'utf8');
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

  const { seen, known, holdersSeen, holdersKnown } = await loadState();
  const firstRun = !known;
  const holdersEnabled = Boolean(HOLDER_WEBHOOK);
  const holdersFirstRun = holdersEnabled && !holdersKnown;
  // Once the holder channel has a baseline it keeps being persisted even with
  // the webhook unset for a release — re-enabling must not replay history.
  const holdersState = () => (holdersEnabled || holdersKnown ? holdersSeen : null);

  let plan = deliveryPlan(list, { seen, firstRun, holdersEnabled, holdersSeen, holdersFirstRun });
  if (REPLAY > 0) {
    const replayed = list.slice(0, REPLAY).reverse(); // deliberate re-post
    plan = plan.map((pass) => ({ ...pass, entries: replayed }));
  }

  // Each channel's FIRST sighting is silent: record its baseline, post nothing
  // there. The two baselines are independent, so wiring up the holder webhook
  // months after the public one floods neither channel.
  if (holdersFirstRun && !REPLAY) {
    for (const e of list) holdersSeen.add(stampOf(e));
    console.log(`holder channel first run: recorded ${list.length} existing entries, posting nothing there`);
  }
  if (firstRun && !REPLAY) {
    for (const e of list) seen.add(stampOf(e));
    await saveState(seen, holdersState());
    console.log(`first run: recorded ${list.length} existing entries, posted nothing`);
    return;
  }
  if (holdersFirstRun && !REPLAY) await saveState(seen, holdersState());

  if (!plan.some((pass) => pass.entries.length)) {
    console.log('nothing new');
    return;
  }

  let posted = 0;
  for (const [i, pass] of plan.entries()) {
    // The published lead separates the holder pass from the public one, and
    // only when this run actually posted holders first — after a crash mid-lead
    // the next run delivers the public backlog immediately, no second wait.
    if (i > 0 && posted > 0 && pass.entries.length && HOLDER_LEAD_MS > 0 && !DRY) {
      console.log(`holder lead: waiting ${HOLDER_LEAD_MS / 1000}s before the public pass`);
      await sleep(HOLDER_LEAD_MS);
    }
    const webhook = pass.channel === 'holders' ? HOLDER_WEBHOOK : WEBHOOK;
    const channelSeen = pass.channel === 'holders' ? holdersSeen : seen;
    for (const e of pass.entries) {
      const embed = buildEntryEmbed(e);
      if (DRY) {
        console.log(`[dry-run] [${pass.channel}] ${embed.footer.text} — ${embed.title}`);
      } else {
        // A rejected send is retried; a timed-out one never is, because its
        // outcome is unknown and a repeat lands twice in a channel people read.
        await postEmbed(embed, { webhook });
        await sleep(DELAY_MS);
      }
      // Only marked seen once Discord actually took it — per channel.
      channelSeen.add(stampOf(e));
      await saveState(seen, holdersState());
      posted += 1;
    }
  }
  const channels = plan.filter((pass) => pass.entries.length).length;
  console.log(`posted ${posted} message${posted === 1 ? '' : 's'} across ${channels} channel${channels === 1 ? '' : 's'}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}
