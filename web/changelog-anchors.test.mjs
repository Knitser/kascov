import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

/* Changelog deep links: every rendered entry carries a stable id derived from
 * the same date|title identity as changelogStamp, so /changelog#<slug> (the
 * shape a feed link uses) and #/changelog?at=<slug> land on the entry. These
 * pin the slug rule, its collision counter, the route plumbing and the boot
 * fold — plus the token page's new share affordance and the reworded artwork
 * audit row, which shipped in the same pass. */

const app = readFileSync(new URL('./app.js', import.meta.url), 'utf8');

const lifted = new Function(
  `${app.slice(app.indexOf('const changelogStamp'), app.indexOf('/* the landing'))}
   return { changelogStamp, changelogSlug };`,
)();
const { changelogSlug, changelogStamp } = lifted;

/* parseRoute lifts the same way guide.test.mjs lifts it */
const parseRouteFactory = new Function(
  'location',
  `${app.slice(app.indexOf('function parseRoute()'), app.indexOf('\nfunction selectNetwork('))}
   return parseRoute;`,
);
const routeFor = (hash) => parseRouteFactory({ hash })();

test('the slug is the stamp, slugified the way the feed slugs titles', () => {
  const e = { date: '2026-08-01', title: 'every transaction gets a page' };
  assert.equal(changelogStamp(e), '2026-08-01|every transaction gets a page');
  assert.equal(changelogSlug(e), '2026-08-01-every-transaction-gets-a-page');
});

test('punctuation runs fold to one dash and the ends are trimmed', () => {
  assert.equal(
    changelogSlug({ date: '2026-08-08', title: '— pools & curves, priced!' }),
    '2026-08-08-pools-curves-priced',
  );
  assert.equal(changelogSlug({ date: '2026-08-08', title: '' }), '2026-08-08');
  assert.equal(changelogSlug({}), '');
});

test('#/changelog?at= carries a sanitized id and nothing else', () => {
  assert.equal(routeFor('#/changelog').at, '');
  assert.equal(routeFor('#/changelog?at=2026-08-01-a-title').at, '2026-08-01-a-title');
  /* an id is all ?at= may ever be — it feeds getElementById */
  assert.equal(routeFor('#/changelog?at=%3Cimg%20src%3Dx%3E').at, 'imgsrcx');
  assert.equal(routeFor('#/changelog?at=UPPER').at, 'upper');
  assert.equal(routeFor('#/changelog').view, 'changelog');
});

test('rendered entries carry the id, the collision counter and a self link', () => {
  const render = app.slice(app.indexOf('function renderChangelog()'), app.indexOf('\nfunction wireApiSidebar('));
  assert.match(render, /id="\$\{esc\(slug\)\}"/);
  assert.match(render, /slug = `\$\{slug\}-\$\{n\}`/, 'same-day reships stay unique, like the feed ids');
  assert.match(render, /href="#\/changelog\?at=\$\{esc\(slug\)\}"/, 'each entry links to itself');
  assert.match(render, /scroll-margin-top/);
  /* a feed link may carry the bare title slug; a dated suffix match accepts it */
  assert.match(render, /id\.endsWith\(`-\$\{route\.at\}`\)/);
  assert.match(render, /scrollIntoView/);
});

test('/changelog#<slug> is folded onto the route at boot, like old guide links', () => {
  assert.match(app, /\^\\\/changelog\\\/\?\$\/\.test\(location\.pathname\)/);
  assert.match(app, /history\.replaceState\(null, '', `\/#\/changelog\$\{at \? `\?at=/);
});

test('the token page offers the same share affordance as coin pages', () => {
  const tokenPage = app.slice(app.indexOf('function renderTokenPage('), app.indexOf('\nfunction tokenBacklinkHtml('));
  assert.match(tokenPage, /data-copy="\$\{esc\(shareUrl\(network, id\)\)\}"/);
  assert.match(tokenPage, /copy a shareable link to this token/);
});

test('the artwork audit row claims only what the payload establishes', () => {
  /* "no fetchable logo" was disprovable by anyone whose browser opened the
     url — the fetcher sends a bot UA, not a browser one. Comments are
     stripped first: the old words survive only as the comment quoting them. */
  const code = app.replace(/\/\*[\s\S]*?\*\//g, '');
  assert.doesNotMatch(code, /no fetchable logo/);
  assert.match(app, /identifies itself as kascov, not as a browser/);
  assert.match(app, /may still open in your browser/);
  /* a listing that names no artwork is a different fact than a failed fetch */
  assert.match(app, /row\.image\s*\n?\s*\?/);
  assert.match(app, /names no artwork, so there is nothing to witness/);
});
