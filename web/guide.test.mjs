import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import test from 'node:test';

import { networkRouteHash } from './core/routing.js';

const app = readFileSync(new URL('./app.js', import.meta.url), 'utf8');
const index = readFileSync(new URL('./index.html', import.meta.url), 'utf8');

/* parseRoute only touches `location` and URLSearchParams, so it can be lifted
   out of the DOM-heavy module and exercised for real. */
const parseRouteFactory = new Function(
  'location',
  `${app.slice(app.indexOf('function parseRoute()'), app.indexOf('\nfunction selectNetwork('))}
   return parseRoute;`,
);
const routeFor = (hash) => parseRouteFactory({ hash })();

test('the guide is a view in the shell, not a page of its own', () => {
  assert.ok(!existsSync(new URL('./guide.html', import.meta.url)), 'guide.html must be gone');
  const main = index.slice(index.indexOf('<main id="main"'), index.indexOf('</main>'));
  assert.match(main, /<section id="view-guide" class="view-fade guide-doc"/);
  assert.match(index, /<a class="nav-link" href="#\/guide" data-nav="guide">guide<\/a>/);
  assert.match(index, /<a class="footer-link" href="#\/guide">/);
  /* nothing may still point at the removed page — including the links app.js
     builds into rendered HTML (the preflight page cites the trap section) */
  assert.doesNotMatch(index, /(?:href|src)="[^"]*guide\.html/);
  assert.doesNotMatch(app, /(?:href|src)="[^"]*guide\.html/);
  assert.equal((app.match(/href="#\/guide\?at=trap"/g) || []).length, 2);
  /* the view is registered and dispatched like every other one */
  assert.match(app, /guide: \$\('#view-guide'\),/);
  assert.match(app, /route\.view === 'guide'\) renderGuide\(route\)/);
});

test('#/guide routes, and old /guide.html links keep working', () => {
  assert.deepEqual(routeFor('#/guide'), { view: 'guide', network: null, at: '' });
  assert.deepEqual(routeFor('#/guide/'), { view: 'guide', network: null, at: '' });
  /* a stale bookmark of the standalone page */
  assert.equal(routeFor('#/guide.html').view, 'guide');
  /* section deep links — the shape '/guide.html#trap' is folded into */
  assert.equal(routeFor('#/guide?at=trap').at, 'trap');
  assert.equal(routeFor('#/guide?at=token-name').at, 'token-name');
  /* an id is all ?at= may ever be: it is fed straight to getElementById */
  assert.equal(routeFor('#/guide?at=%3Cimg%20src%3Dx%3E').at, 'imgsrcx');
  /* the boot fixup that rewrites the pathname forms onto the route */
  assert.match(app, /\^\\\/guide\(\?:\\\.html\)\?\\\/\?\$/);
  assert.match(app, /history\.replaceState\(null, '', `\/#\/guide\$\{at \? `\?at=/);
});

test('switching networks keeps you on the guide', () => {
  assert.equal(networkRouteHash({ view: 'guide' }, 'mainnet'), '#/guide');
  assert.equal(networkRouteHash({ view: 'guide', at: 'trap' }, 'testnet-10'), '#/guide');
});

test('every sidebar entry names a section that exists, and vice versa', () => {
  const view = index.slice(index.indexOf('<section id="view-guide"'), index.indexOf('<!-- ============ CHANGELOG'));
  const toc = [...view.matchAll(/href="#\/guide\?at=([a-z0-9-]+)"/g)].map((m) => m[1]);
  const sections = [...view.matchAll(/<section class="gd-block" id="gd-([a-z0-9-]+)"/g)].map((m) => m[1]);
  assert.deepEqual(toc.slice(0, sections.length), sections);
  assert.deepEqual([...new Set(toc)].sort(), [...sections].sort());
  assert.ok(sections.includes('trap'), 'the compute-budget trap is deep-linked from the API docs');
  /* the trap section is cited from the sidebar, from step 4, and from the two
     places outside the guide that used to link /guide.html#trap: the preflight
     page and the preflight endpoint in the API docs */
  assert.equal((index.match(/href="#\/guide\?at=trap"/g) || []).length, 4);
  assert.equal((view.match(/href="#\/guide\?at=trap"/g) || []).length, 2);
});

test('command blocks stay copyable and the sidebar tracks the reader', () => {
  const wiring = app.slice(app.indexOf('function renderGuide('), app.indexOf('\n/* ---- guided visual builder'));
  assert.match(wiring, /wireGuideCopy\(\);/);
  assert.match(wiring, /wireGuideSpy\(\);/);
  /* a copy must not hand you the sample output back */
  assert.match(wiring, /clone\.querySelectorAll\('\.o, \.e'\)\.forEach\(\(n\) => n\.remove\(\)\)/);
  /* idempotent: render runs on every live refresh */
  assert.match(wiring, /if \(box\.dataset\.wired\) return;/);
  assert.match(wiring, /if \(!nav \|\| nav\.dataset\.wired\) return;/);
  /* every code block the markup ships can carry a button */
  const view = index.slice(index.indexOf('<section id="view-guide"'), index.indexOf('<!-- ============ CHANGELOG'));
  assert.equal(
    (view.match(/<div class="gd-code"><pre class="codeblock">/g) || []).length,
    (view.match(/<pre class="codeblock">/g) || []).length,
  );
});
