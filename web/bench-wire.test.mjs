/* The wire's dangerous moments are WHEN it posts (once per ISO week, never
   twice) and WHAT it claims (only what the two verification feeds actually
   carried, absences included). All pure, all pinned here with fixed instants
   — no Date.now anywhere in the functions under test. */
import test from 'node:test';
import assert from 'node:assert/strict';

import {
  alreadyPosted, extractSummary, formatWire, isoWeekOf, mondayOf, pickSpecimen,
  postWire,
} from '../scripts/discord-bench-wire.mjs';

/* Monday 2026-08-03 09:00 UTC — the exact instant the timer fires. */
const MONDAY = Date.UTC(2026, 7, 3, 9, 0, 0);

/* ------------------------------------------------------------- week math */

test('the ISO week is stable across the whole week and flips on Monday', () => {
  assert.equal(isoWeekOf(MONDAY), '2026-W32');
  assert.equal(isoWeekOf(Date.UTC(2026, 7, 5, 12)), '2026-W32');       // Wednesday
  assert.equal(isoWeekOf(Date.UTC(2026, 7, 9, 23, 59, 59)), '2026-W32'); // Sunday night
  assert.equal(isoWeekOf(Date.UTC(2026, 7, 10, 0, 0, 0)), '2026-W33');  // next Monday
});

test('a week at the year boundary belongs to the year of its Thursday', () => {
  // 2026 starts on a Thursday, so it owns W01 and runs 53 weeks
  assert.equal(isoWeekOf(Date.UTC(2026, 0, 1)), '2026-W01');
  assert.equal(isoWeekOf(Date.UTC(2027, 0, 1)), '2026-W53'); // that Friday is still 2026's week
});

test('the header names every day of a week by the same Monday', () => {
  assert.equal(mondayOf(MONDAY), '2026-08-03');
  assert.equal(mondayOf(Date.UTC(2026, 7, 6, 3)), '2026-08-03');  // Thursday
  assert.equal(mondayOf(Date.UTC(2026, 7, 9, 23)), '2026-08-03'); // Sunday
  assert.equal(mondayOf(Date.UTC(2026, 7, 10, 1)), '2026-08-10'); // the flip
});

/* -------------------------------------------------------- same-week guard */

test('a re-run in the same ISO week posts nothing', () => {
  assert.ok(alreadyPosted({ week: '2026-W32' }, MONDAY));
  assert.ok(alreadyPosted({ week: '2026-W32' }, Date.UTC(2026, 7, 9, 23))); // late Sunday retry
});

test('a fresh week, or no state at all, posts', () => {
  assert.ok(!alreadyPosted({ week: '2026-W31' }, MONDAY));
  assert.ok(!alreadyPosted({ week: '2026-W32' }, Date.UTC(2026, 7, 10, 9))); // next Monday
  assert.ok(!alreadyPosted(null, MONDAY));   // first run ever
  assert.ok(!alreadyPosted({}, MONDAY));     // state without a week
});

/* ------------------------------------------------------------ extraction */

/* shaped exactly like /data/{network}/verification.json */
const REPORT = {
  network: 'mainnet',
  runs: [
    { outcome: 'failed', markets_matched: 0, markets_unmatched: 0, tokens_verified: 0 },
    { outcome: 'ok', markets_matched: 41, markets_unmatched: 7, tokens_verified: 12 },
  ],
  unknown_programs_total: 5,
  audit_bench: {
    families: [
      { instances: 8, trades: 15, push_count: 12, sample_covenant: 'bb'.repeat(32) },
      { instances: 3, trades: 120, push_count: 9, sample_covenant: 'aa'.repeat(32) },
    ],
  },
};

test('the summary prefers the latest COMPLETED run over a newer failed one', () => {
  const s = extractSummary('mainnet', REPORT);
  assert.deepEqual(s.run, { markets_matched: 41, markets_unmatched: 7, tokens_verified: 12 });
});

test('families are re-sorted by trades, never trusted as received', () => {
  const s = extractSummary('mainnet', REPORT);
  assert.equal(s.bench.families, 2);
  assert.deepEqual(s.bench.top.map((f) => f.trades), [120, 15]);
});

test('absences stay absences: no runs, no bench, no report', () => {
  // an old worker serves audit_bench: null; that is "no report", not zero families
  const bare = extractSummary('testnet-10', { runs: [], audit_bench: null });
  assert.equal(bare.run, null);
  assert.equal(bare.bench, null);
  const empty = extractSummary('testnet-10', undefined);
  assert.equal(empty.run, null);
  assert.equal(empty.bench, null);
  // a bench that ran and found nothing is a different fact from no bench
  const clean = extractSummary('mainnet', { runs: [], audit_bench: { families: [] } });
  assert.equal(clean.bench.families, 0);
});

/* -------------------------------------------------------- specimen picker */

const summaryWith = (network, top) => ({ network, run: null, bench: top && { families: top.length, top } });

test('the specimen is the family with the most trades, across networks', () => {
  const pick = pickSpecimen([
    summaryWith('mainnet', [{ instances: 3, trades: 120, sample_covenant: 'aa' }]),
    summaryWith('testnet-10', [{ instances: 9, trades: 400, sample_covenant: 'bb' }]),
  ]);
  assert.equal(pick.network, 'testnet-10');
  assert.equal(pick.family.trades, 400);
});

test('a trades tie goes to the family with more deployments', () => {
  const pick = pickSpecimen([
    summaryWith('mainnet', [{ instances: 3, trades: 100, sample_covenant: 'aa' }]),
    summaryWith('testnet-10', [{ instances: 9, trades: 100, sample_covenant: 'bb' }]),
  ]);
  assert.equal(pick.family.instances, 9);
});

test('no family with any trades means NO specimen, never a placeholder', () => {
  assert.equal(pickSpecimen([summaryWith('mainnet', [{ instances: 5, trades: 0, sample_covenant: 'aa' }])]), null);
  assert.equal(pickSpecimen([summaryWith('mainnet', null)]), null); // no bench at all
  assert.equal(pickSpecimen([]), null);
  assert.equal(pickSpecimen(undefined), null);
});

/* ------------------------------------------------------------- formatter */

const SUMMARIES = [
  extractSummary('mainnet', REPORT),
  extractSummary('testnet-10', { runs: [], audit_bench: null }),
];

test('the post has its fixed structure: header, both networks, footer links', () => {
  const embed = formatWire(SUMMARIES, pickSpecimen(SUMMARIES), MONDAY);
  assert.equal(embed.title, 'the bench wire · week of 2026-08-03');
  assert.match(embed.description, /\*\*mainnet\*\*/);
  assert.match(embed.description, /\*\*testnet-10\*\*/);
  // mainnet block carries the run's numbers
  assert.match(embed.description, /\*\*41\*\* matched/);
  assert.match(embed.description, /\*\*7\*\* not yet/);
  assert.match(embed.description, /tokens verified: \*\*12\*\*/);
  // the testnet block states its absences instead of inventing zeros
  assert.match(embed.description, /no verification run on record yet/);
  assert.match(embed.description, /bench: no report yet/);
  // footer links, both of them
  assert.match(embed.description, /kascov\.io\/unknowns/);
  assert.match(embed.description, /kascov\.io\/vote/);
});

test('the specimen section appears only when a specimen exists', () => {
  const withOne = formatWire(SUMMARIES, pickSpecimen(SUMMARIES), MONDAY);
  assert.match(withOne.description, /specimen of the week/);
  assert.match(withOne.description, /120 recorded trades/);
  // the honesty line rides with it: a specimen is unaudited, not endorsed
  assert.match(withOne.description, /nothing about this build is proven/);
  // and with no specimen the section is ABSENT — not a placeholder, not "none"
  const without = formatWire(SUMMARIES, null, MONDAY);
  assert.ok(!without.description.includes('specimen'));
});

test('a mainnet block with families lists the top ones by size', () => {
  const embed = formatWire(SUMMARIES, null, MONDAY);
  assert.match(embed.description, /\*\*2\*\* unmatched build families/);
  assert.match(embed.description, /3 deployments, 120 recorded trades/);
  assert.match(embed.description, /8 deployments, 15 recorded trades/);
});

test('an oversize description is truncated rather than rejected by Discord', () => {
  const wide = Array.from({ length: 200 }, (_, i) =>
    summaryWith(`net-${i}-${'x'.repeat(30)}`, null));
  const embed = formatWire(wide, null, MONDAY);
  assert.ok(embed.description.length <= 4000);
  assert.match(embed.description, /\.\.\.$/);
});

/* ------------------------------------------------------------- the sender */

const fakeDiscord = (...statuses) => {
  const calls = [];
  const impl = async (url, opts) => {
    calls.push({ url, body: opts.body });
    const s = statuses[calls.length - 1] ?? 200;
    if (s === 429) {
      return { ok: false, status: 429, json: async () => ({ retry_after: 0.4 }), text: async () => '' };
    }
    return { ok: s < 300, status: s, json: async () => ({}), text: async () => 'boom' };
  };
  return { impl, calls };
};

test('a rate limit is waited out and the SAME post resent, not dropped', async () => {
  const { impl, calls } = fakeDiscord(429, 200);
  const waited = [];
  await postWire({ title: 't' }, {
    webhook: 'https://example.invalid/w',
    fetchImpl: impl,
    sleepImpl: async (ms) => { waited.push(ms); },
  });
  assert.equal(calls.length, 2);
  assert.equal(calls[0].body, calls[1].body);
  assert.ok(waited[0] >= 400);
});

test('a real error is thrown, never retried into a double post', async () => {
  const { impl, calls } = fakeDiscord(400);
  await assert.rejects(
    postWire({ title: 't' }, {
      webhook: 'https://example.invalid/w', fetchImpl: impl, sleepImpl: async () => {},
    }),
    /discord POST 400/,
  );
  assert.equal(calls.length, 1);
});
