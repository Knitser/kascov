#!/usr/bin/env node
/* the kascov bench wire — zero dependencies.
 *
 * One Discord post every Monday morning: what the verification runs and the
 * audit bench actually recorded on mainnet and testnet-10, pulled from the
 * same public verification.json anyone can read. The wire never learns
 * anything privately — every number in the post has a public URL it was
 * copied from, so a reader can check the post against the site.
 *
 * WHAT IT NEVER DOES
 *   - post twice in one ISO week: the state file remembers which week last
 *     got its post, so a re-run (or a Persistent= catch-up firing late) is a
 *     quiet no-op.
 *   - invent a specimen: the "specimen of the week" is the unmatched family
 *     with the most recorded trades, and when no family has any trades the
 *     section is absent entirely, never a placeholder.
 *   - pass a verdict: unmatched means kascov has not audited the build. It
 *     says nothing about the build itself, in either direction.
 *
 * Setup (never commit these):
 *   KASCOV_WIRE_WEBHOOK=https://discord.com/api/webhooks/...
 *   KASCOV_WIRE_STATE=/home/kascov/kascov-wire.json   (optional)
 *
 * A missing webhook means the wire reports itself off and exits clean. A
 * weekly digest that cannot post is a feature that is off, not a failing
 * unit that pages someone every Monday.
 *
 * Run:    node scripts/discord-bench-wire.mjs
 *         node scripts/discord-bench-wire.mjs --dry-run   # print, don't post
 * Timer:  kascov-bench-wire.timer, Mondays 09:00 UTC.
 */

import { readFile, writeFile } from 'node:fs/promises';

const SITE = process.env.KASCOV_BASE || 'https://kascov.io';
const WEBHOOK = process.env.KASCOV_WIRE_WEBHOOK || '';
const STATE_PATH = process.env.KASCOV_WIRE_STATE || './.kascov-wire.json';
const DRY = process.argv.includes('--dry-run');

const NETWORKS = ['mainnet', 'testnet-10'];
const MINT = 0x70c7ba;

/* ---------------------------------------------------------------- pure bits */

const fmt = (n) => Number(n || 0).toLocaleString('en-US');

/** The ISO week an instant falls in, as "YYYY-Www", computed in UTC. ISO
 *  because its weeks never split across a year boundary ambiguously: the
 *  week belongs to the year holding its Thursday. */
export function isoWeekOf(ms) {
  const d = new Date(ms);
  const thursday = new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()));
  thursday.setUTCDate(thursday.getUTCDate() + 4 - (thursday.getUTCDay() || 7));
  const week = Math.ceil(
    ((thursday.getTime() - Date.UTC(thursday.getUTCFullYear(), 0, 1)) / 86_400_000 + 1) / 7,
  );
  return `${thursday.getUTCFullYear()}-W${String(week).padStart(2, '0')}`;
}

/** The Monday that opens an instant's ISO week, as "YYYY-MM-DD" — the date
 *  the header names, so the same week is always called by the same day. */
export function mondayOf(ms) {
  const d = new Date(ms);
  const monday = new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()));
  monday.setUTCDate(monday.getUTCDate() + 1 - (monday.getUTCDay() || 7));
  return monday.toISOString().slice(0, 10);
}

/** The same-week guard. True means this ISO week already got its post, so a
 *  re-run in the same week must post nothing. */
export function alreadyPosted(state, nowMs) {
  return Boolean(state?.week) && state.week === isoWeekOf(nowMs);
}

/**
 * What one network's verification.json actually carries, and nothing more.
 *
 * The latest COMPLETED run is preferred over a newer failed one, because a
 * failed run's counters describe the failure, not the network. Absences stay
 * absences: a null run or bench makes the post say so rather than showing a
 * zero that was never counted.
 */
export function extractSummary(network, report) {
  const runs = Array.isArray(report?.runs) ? report.runs : [];
  /* the store writes ok/degraded/failed/interrupted — prefer the newest clean
     run so a failed sync never becomes the week's headline numbers */
  const latest = runs.find((r) => r && r.outcome === 'ok') || runs[0] || null;
  const bench = report?.audit_bench && typeof report.audit_bench === 'object'
    ? report.audit_bench : null;
  const families = Array.isArray(bench?.families) ? bench.families : [];
  return {
    network,
    run: latest && {
      markets_matched: Number(latest.markets_matched) || 0,
      markets_unmatched: Number(latest.markets_unmatched) || 0,
      tokens_verified: Number(latest.tokens_verified) || 0,
    },
    bench: bench && {
      families: families.length,
      // the bench sorts by trades already, but the wire re-sorts rather than
      // trusting a file it did not write: trades first, then instances.
      top: families.slice()
        .sort((a, b) => (Number(b?.trades) || 0) - (Number(a?.trades) || 0)
          || (Number(b?.instances) || 0) - (Number(a?.instances) || 0))
        .slice(0, 3)
        .map((f) => ({
          instances: Number(f?.instances) || 0,
          trades: Number(f?.trades) || 0,
          sample_covenant: String(f?.sample_covenant || ''),
        })),
    },
  };
}

/** The specimen of the week: across every network, the unmatched family with
 *  the most recorded trades; ties go to more deployments. No family with any
 *  trades means NO specimen — null, and the section is skipped, because a
 *  placeholder specimen would be the wire inventing news. */
export function pickSpecimen(summaries) {
  let best = null;
  for (const s of summaries || []) {
    for (const f of s?.bench?.top || []) {
      if (!f || !(f.trades > 0)) continue;
      if (!best || f.trades > best.family.trades
        || (f.trades === best.family.trades && f.instances > best.family.instances)) {
        best = { network: s.network, family: f };
      }
    }
  }
  return best;
}

/** The whole post, as one embed with a fixed structure: header, a block per
 *  network, the specimen when one exists, footer links. Every line states
 *  what the feed carried, including the absences. */
export function formatWire(summaries, specimen, nowMs) {
  const lines = [];
  for (const s of summaries || []) {
    lines.push(`**${s.network}**`);
    if (s.run) {
      lines.push(`markets: **${fmt(s.run.markets_matched)}** matched to audited builds, **${fmt(s.run.markets_unmatched)}** not yet`);
      lines.push(`tokens verified: **${fmt(s.run.tokens_verified)}**`);
    } else {
      lines.push('no verification run on record yet');
    }
    if (!s.bench) {
      lines.push('bench: no report yet');
    } else if (!s.bench.families) {
      lines.push('bench: no unmatched build families');
    } else {
      lines.push(`bench: **${fmt(s.bench.families)}** unmatched build ${s.bench.families === 1 ? 'family' : 'families'}, largest first:`);
      for (const f of s.bench.top) {
        lines.push(`· ${fmt(f.instances)} deployment${f.instances === 1 ? '' : 's'}, ${fmt(f.trades)} recorded trade${f.trades === 1 ? '' : 's'}`);
      }
    }
    lines.push('');
  }
  if (specimen) {
    const f = specimen.family;
    lines.push('**specimen of the week**');
    lines.push(`The unmatched family carrying the most trades: ${fmt(f.instances)} deployment${f.instances === 1 ? '' : 's'} on ${specimen.network}, ${fmt(f.trades)} recorded trades, sample \`${f.sample_covenant.slice(0, 16)}…\`. Unmatched means unaudited — nothing about this build is proven, in either direction.`);
    lines.push('');
  }
  lines.push(`[the unknowns board](${SITE}/unknowns) · [the audit vote](${SITE}/vote)`);
  const description = lines.join('\n');
  return {
    title: `the bench wire · week of ${mondayOf(nowMs)}`,
    url: `${SITE}/unknowns`,
    color: MINT,
    // Discord caps an embed description at 4096 and rejects the whole post over it
    description: description.length > 4000 ? `${description.slice(0, 3997)}...` : description,
    footer: { text: 'kascov · every number here has a public URL it was copied from' },
  };
}

/* -------------------------------------------------------------------- runtime */

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/* One scheduled run gets no second chance until next Monday, so ride out a
   cold cache or a worker restart. Retries 5xx and network errors, never 4xx
   (that is our bug). Same shape as the changelog bot's. */
async function retrying(label, attempt) {
  let last;
  for (let i = 1; i <= 3; i++) {
    try {
      const res = await attempt(AbortSignal.timeout(20_000));
      if (res.ok) return res;
      if (res.status < 500) throw new Error(`${label} HTTP ${res.status}`);
      last = new Error(`${label} HTTP ${res.status}`);
    } catch (err) {
      if (/HTTP 4\d\d/.test(err.message)) throw err;
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
 * Send the embed, honouring Discord's rate limiter. A 429 explicitly REJECTED
 * the message, so re-sending cannot double-post; a timed-out request has an
 * unknown outcome and is never retried, because a duplicate weekly digest is
 * worse than a missing one.
 */
export async function postWire(embed, { webhook = WEBHOOK, fetchImpl = fetch, sleepImpl = sleep } = {}) {
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
    return JSON.parse(await readFile(STATE_PATH, 'utf8'));
  } catch {
    return null; // first run: no week has been posted yet
  }
}

async function saveState(next) {
  try {
    await writeFile(STATE_PATH, JSON.stringify(next), 'utf8');
  } catch (e) {
    console.warn(`could not persist state to ${STATE_PATH}: ${e.message}`);
  }
}

async function main() {
  if (!WEBHOOK && !DRY) {
    // fail closed: off is a reported state, never a crash and never a post.
    console.log('KASCOV_WIRE_WEBHOOK is not set — the bench wire is off.');
    return;
  }
  const now = Date.now();
  if (!DRY && alreadyPosted(await loadState(), now)) {
    console.log(`already posted in ${isoWeekOf(now)}; nothing to do`);
    return;
  }

  const summaries = [];
  for (const network of NETWORKS) {
    const res = await retrying(`${network} verification.json`, (signal) =>
      fetch(`${SITE}/data/${network}/verification.json`, {
        headers: { 'user-agent': 'kascov-bench-wire' },
        cache: 'no-cache',
        signal,
      }));
    summaries.push(extractSummary(network, await res.json()));
  }

  const embed = formatWire(summaries, pickSpecimen(summaries), now);
  if (DRY) {
    console.log(`[dry-run] ${embed.title}\n${embed.description}`);
    return;
  }
  await postWire(embed);
  // marked only after Discord took it: a failed post leaves the week
  // unclaimed, so a manual re-run can still deliver it.
  await saveState({ week: isoWeekOf(now), posted_ms: now });
  console.log(`posted the bench wire for ${isoWeekOf(now)}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}
