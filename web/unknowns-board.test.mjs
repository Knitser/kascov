import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

/* The unknowns board's pinned ledger: stamps come only from the verification
 * payload, never from markup. The old board hardcoded 'first pinned by —'
 * under every family — decoration nothing backed. These pin the replacement:
 * an old payload (no `pinned` array) renders NO pin line anywhere, a new
 * payload renders the open slot on open families and a stamp per pinned one,
 * and half-filled or hostile entries degrade honestly. */

const html = readFileSync(new URL('./unknowns.html', import.meta.url), 'utf8');
const script = html.match(/<script>([\s\S]*?)<\/script>/)[1];

/* the helpers are deliberately kept together and pure so they can be lifted
   without running the fetch wiring below them */
const body = script.slice(script.indexOf('function esc('), script.indexOf('/* ------------------------------- wiring'));
const { benchState, boardHtml, pinnedOf, rowHtml, rowOf } = new Function(
  `${body}\nreturn { esc, fmtInt, isHex64, rowOf, pinnedOf, benchState, rowHtml, pinnedHtml, boardHtml };`,
)();

const HASH = 'ab'.repeat(32);
const family = { sample_program_hash: HASH, sample_covenant: 'cd'.repeat(32), instances: 4, push_count: 12, trades: 7 };
const PIN = { skeleton: 'KRON pool v3', first_pinned_by: 'izio', date: '2026-08-01', commit: 'f'.repeat(40), instances: 9 };

test('an old payload renders no pin line at all — not even the dash', () => {
  const out = boardHtml('mainnet', benchState({
    audit_bench: { unmatched_covenants: 5, recovered: 4, unrecoverable: 1, families: [family] },
  }));
  assert.doesNotMatch(out, /first pinned by/);
  assert.doesNotMatch(out, /pinned families/);
});

test('a payload carrying the ledger shows the open slot on open families', () => {
  const out = boardHtml('mainnet', benchState({
    audit_bench: { unmatched_covenants: 5, recovered: 4, unrecoverable: 1, families: [family], pinned: [] },
  }));
  assert.match(out, /first pinned by &mdash;/);
  assert.doesNotMatch(out, /pinned families/, 'no stamps yet, no ledger section');
});

test('a pinned family renders its stamp: family, credit, date, commit, spread', () => {
  const out = boardHtml('mainnet', benchState({
    audit_bench: { unmatched_covenants: 1, recovered: 1, unrecoverable: 0, families: [family], pinned: [PIN] },
  }));
  assert.match(out, /pinned families/);
  assert.match(out, /KRON pool v3/);
  assert.match(out, /first pinned by <strong>izio<\/strong>/);
  assert.match(out, /2026-08-01/);
  assert.match(out, new RegExp(`commit ${'f'.repeat(12)}`), 'commit shown as its 12-char prefix');
  assert.match(out, /9 instances/);
});

test('the ledger also renders when the payload puts it at the root', () => {
  const rows = pinnedOf({ pinned: [PIN] });
  assert.equal(rows.length, 1);
  assert.equal(rows[0].by, 'izio');
  /* even a network whose bench block is absent keeps its earned stamps */
  const out = boardHtml('mainnet', benchState({ pinned: [PIN] }));
  assert.match(out, /has not published a report/);
  assert.match(out, /first pinned by <strong>izio<\/strong>/);
});

test('a stamp without a family or a name is not a stamp', () => {
  const rows = pinnedOf({
    pinned: [
      PIN,
      { skeleton: 'orphan build' },            /* nobody credited */
      { first_pinned_by: 'ghost' },            /* nothing named */
      { skeleton: '', first_pinned_by: '' },
      null,
    ],
  });
  assert.equal(rows.length, 1);
});

test('optional stamp fields degrade to absence, never to garbage', () => {
  const out = boardHtml('mainnet', benchState({
    audit_bench: { families: [], pinned: [{ skeleton: 'lean build', first_pinned_by: 'sutton', instances: 'many' }] },
  }));
  assert.match(out, /first pinned by <strong>sutton<\/strong>/);
  assert.doesNotMatch(out, /commit [0-9a-f]/, 'no commit prefix invented for an absent commit');
  assert.doesNotMatch(out, /NaN/);
});

test('a hostile pinned entry cannot inject markup', () => {
  const out = boardHtml('mainnet', benchState({
    pinned: [{ skeleton: '<img src=x onerror=1>', first_pinned_by: '<script>x</script>' }],
    audit_bench: { families: [] },
  }));
  assert.doesNotMatch(out, /<img src=x/);
  assert.doesNotMatch(out, /<script>x/);
  assert.match(out, /&lt;script&gt;/);
});

test('the open-family row itself is unchanged apart from the slot', () => {
  const r = rowOf(family);
  const withLedger = rowHtml('mainnet', r, true);
  const withoutLedger = rowHtml('mainnet', r, false);
  assert.match(withLedger, /4 instances · 12 pushes · 7 trades ride on it/);
  assert.equal(withoutLedger, withLedger.replace(/<p class="fam-pin">[^<]*<\/p>/, ''));
});
