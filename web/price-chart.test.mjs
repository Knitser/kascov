import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

/* The price chart's whole claim is the click-through: every point must be an
 * anchor to the replayable transaction that produced its price. These pin
 * that, plus the honesty gates — under three priced trades nothing renders,
 * co-covenant trades never chart, and both surfaces (token page, pool page)
 * read the same verified feed as their trade tables. */

const app = readFileSync(new URL('./app.js', import.meta.url), 'utf8');

/* priceChartSvg is pure once its formatters are supplied; lift it with the
   REAL fmtPriceKas so a display regression in either shows up here. */
const lifted = (name) => {
  const start = app.indexOf(`function ${name}(`);
  assert.ok(start > 0, `${name} must exist`);
  return app.slice(start, app.indexOf('\nfunction ', start + 10));
};
const factory = new Function(
  'esc', 'fmtInt', 'relTimeShort', 'absShort',
  `${lifted('fmtPriceKas')}\n${lifted('priceChartSvg')}\nreturn priceChartSvg;`,
);
const priceChartSvg = factory(
  (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])),
  (n) => String(n),
  () => 'recently',
  () => 'Aug 8',
);

const tx = (n) => n.toString(16).padStart(64, '0');
const trade = (seq, quote, base, extra = {}) => ({
  seq, txid: tx(seq), side: seq % 2 ? 'buy' : 'sell',
  quote_sompi: quote, base_amount: base,
  accepting_time_ms: 1754600000000 + seq * 60000, accepting_daa: 1000 + seq,
  co_covenants: 0, ...extra,
});

test('under three priced trades, nothing renders', () => {
  assert.equal(priceChartSvg([], 'mainnet', () => null), '');
  assert.equal(priceChartSvg([trade(1, 100, 10), trade(2, 200, 10)], 'mainnet', () => null), '');
});

test('every point is an anchor to its own transaction', () => {
  const svg = priceChartSvg([trade(3, 300, 10), trade(1, 100, 10), trade(2, 200, 10)], 'mainnet', () => null);
  const anchors = [...svg.matchAll(/<a href="#\/mainnet\/tx\/([0-9a-f]{64})"/g)].map((m) => m[1]);
  assert.equal(anchors.length, 3, 'one anchor per priced trade');
  /* payload order is newest-first; the chart must follow seq, the chain's order */
  assert.deepEqual(anchors, [tx(1), tx(2), tx(3)]);
  assert.equal((svg.match(/<circle /g) || []).length, 3);
  assert.match(svg, /<title>/, 'each point says its price and side');
});

test('the line is a step function, not an interpolation', () => {
  const svg = priceChartSvg([trade(1, 100, 10), trade(2, 200, 10), trade(3, 300, 10)], 'mainnet', () => null);
  const d = svg.match(/<path d="([^"]+)"/)[1];
  assert.match(d, /^M[\d. ]+( H[\d.]+ V[\d.]+){2}$/, 'M then H/V pairs only — price holds until the next trade');
});

test('unpriced and co-covenant trades never chart', () => {
  const rows = [
    trade(1, 100, 10), trade(2, 200, 10), trade(3, 300, 10),
    trade(4, 400, 10, { co_covenants: 2 }),   /* several covenants moved: not a publishable price */
    trade(5, 0, 10),                          /* no quote */
    trade(6, 100, 0),                         /* no base */
    trade(7, 100, 10, { txid: null }),        /* nothing to link to */
  ];
  const svg = priceChartSvg(rows, 'mainnet', () => null);
  assert.equal((svg.match(/<a href=/g) || []).length, 3);
  assert.doesNotMatch(svg, new RegExp(tx(4)));
});

test('excluded trades do not help a thin series over the gate', () => {
  const rows = [trade(1, 100, 10), trade(2, 200, 10), trade(3, 300, 10, { co_covenants: 1 })];
  assert.equal(priceChartSvg(rows, 'mainnet', () => null), '');
});

test('a flat price series still renders inside a visible band', () => {
  const svg = priceChartSvg([trade(1, 100, 10), trade(2, 100, 10), trade(3, 100, 10)], 'mainnet', () => null);
  assert.match(svg, /<path d="/);
  assert.doesNotMatch(svg, /NaN|Infinity/);
});

test('timestamps missing on any trade → even spacing, labelled as order not time', () => {
  const rows = [trade(1, 100, 10), trade(2, 200, 10), trade(3, 300, 10)]
    .map((t) => ({ ...t, accepting_time_ms: null }));
  const svg = priceChartSvg(rows, 'mainnet', () => null);
  assert.match(svg, />oldest</);
  assert.match(svg, />newest</);
  assert.doesNotMatch(svg, /NaN/);
});

test('the axis speaks the house price format and the palette is the house palette', () => {
  const svg = priceChartSvg([trade(1, 1e8, 1), trade(2, 2e8, 1), trade(3, 3e8, 1)], 'mainnet', () => null);
  assert.match(svg, /3 KAS</, 'hi label is a real KAS price');
  assert.match(svg, /1 KAS</, 'lo label is a real KAS price');
  for (const v of ['--accent', '--born', '--burn', '--border', '--faint']) {
    assert.match(svg, new RegExp(`var\\(${v}`), `house variable ${v}`);
  }
  assert.doesNotMatch(svg, /#[0-9a-fA-F]{3,6}[;"']/, 'no hardcoded colors');
});

/* ------------------------------------------------------------ the wiring */

test('the token page chart reads the exact list the trades table reads', () => {
  const market = app.slice(app.indexOf('function marketSectionHtml('), app.indexOf('\nfunction poolWiringSvg('));
  /* priceViewHtml is the candle wrapper; it feeds the SAME source to both
     the candle series and the per-trade SVG, so the contract survives it */
  assert.match(market, /priceViewHtml\(source, network, toMs\)/);
  const viewFn = lifted('priceViewHtml');
  assert.match(viewFn, /priceChartSvg\(source, network, toMs\)/);
  assert.match(viewFn, /bucketTrades\(source, /);
});

test('the pool page fetches the token trade feed once and charts it', () => {
  const pool = app.slice(app.indexOf('function renderPoolPage('), app.indexOf('\nfunction renderVerify('));
  assert.match(pool, /loadAllTrades\(network, tok\.covenant_id\)/);
  assert.match(pool, /priceViewHtml\(tlist\.data\.trades, network, toMs\)/);
  /* refetch is guarded on the cache slot, so a failed fetch cannot loop */
  assert.match(pool, /if \(!tlist\) \{/);
  assert.match(pool, /catch\(\(\) => \{/);
});

test('the caption states the provenance and the click-through', () => {
  assert.match(app, /every point is a verified trade at its executed price — click one to open the transaction/);
});

/* ---- candle bucketing: the same admission gates, plus the clock rule ---- */

const bucketFactory = new Function(
  `${lifted('bucketTrades')}\nreturn bucketTrades;`,
);
const bucketTrades = bucketFactory();

test('candles group by bucket with correct OHLC, volume and last txid', () => {
  const trades = [
    trade(1, 100_000_000, 10), // 0.01 sompi/unit bucket A
    trade(2, 400_000_000, 10), // high of bucket A
    trade(3, 200_000_000, 10), // close of bucket A (minutes 1-3 < 15m)
    trade(20, 300_000_000, 10, { accepting_time_ms: 1754600000000 + 20 * 60000 }), // bucket B
  ];
  const candles = bucketTrades(trades, 900000, () => null);
  assert.equal(candles.length, 2);
  const [a, b] = candles;
  assert.equal(a.open, 10_000_000);
  assert.equal(a.high, 40_000_000);
  assert.equal(a.close, 20_000_000);
  assert.equal(a.lastTxid, tx(3), 'the candle closes on its last trade');
  assert.equal(a.volume_kas, 7, 'volume sums the quotes, in KAS');
  assert.equal(b.open, 30_000_000);
  assert.equal(b.lastTxid, tx(20));
});

test('a single trade without a clock refuses the whole candle view', () => {
  const trades = [trade(1, 100, 10), trade(2, 200, 10),
    trade(3, 300, 10, { accepting_time_ms: null })];
  assert.equal(bucketTrades(trades, 900000, () => null), null,
    'candles claim durations; DAA guesses may not chart');
});

test('co-covenant and unpriced trades never reach a candle', () => {
  const trades = [trade(1, 100, 10), trade(2, 200, 10), trade(3, 300, 10),
    trade(4, 999_000, 10, { co_covenants: 2 }), trade(5, 0, 10)];
  const candles = bucketTrades(trades, 900000, () => null);
  const vol = candles.reduce((s, c) => s + c.volume_kas, 0);
  assert.equal(vol, (100 + 200 + 300) / 1e8, 'only the three verified trades count');
});

test('under three admitted trades there are no candles', () => {
  assert.equal(bucketTrades([trade(1, 100, 10), trade(2, 200, 10)], 900000, () => null), null);
});

test('candles read the full verified history, not the visible table page', () => {
  const wire = lifted('wirePriceCandles');
  /* prefer the cached complete list, and fetch it when absent — the candle
     view summarises the token's whole life from its first trade */
  assert.match(wire, /tradeLists\.get\(`\$\{network\}\/\$\{tokenId\}`\)/);
  assert.match(wire, /loadAllTrades\(network, tokenId\)/);
  /* both page call sites hand the token id over */
  assert.match(app, /wirePriceCandles\(view, marketChartSource, network, toMs, route\.id\)/);
  assert.match(app, /wirePriceCandles\(view, tlist\.data\.trades, network, toMs, tok\.covenant_id\)/);
});
