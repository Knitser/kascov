import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
const index = readFileSync(new URL('./index.html', import.meta.url), 'utf8');

test('phone header and navigation remain reachable without page overflow', () => {
  assert.match(css, /@media \(max-width: 840px\)[\s\S]*?\.header-inner\s*\{[\s\S]*?display:\s*grid/);
  assert.match(css, /\.site-nav\s*\{[\s\S]*?overflow-x:\s*auto/);
  assert.match(css, /\.nav-link\s*\{[\s\S]*?min-height:\s*44px/);
});

test('explorer section jumps clear sticky chrome and become a wide-screen side rail', () => {
  assert.match(
    css,
    /#view-explore\s*>\s*:is\(section,\s*details\)\[id\]\s*\{[^}]*scroll-margin-top:\s*140px/,
  );
  assert.match(
    css,
    /@media \(min-width: 1360px\)[\s\S]*?\.explore-jump\s*\{[^}]*position:\s*fixed[^}]*flex-direction:\s*column/,
  );
  assert.match(
    css,
    /\.stats-strip\s*\{[^}]*padding:\s*14px[^}]*margin:\s*-14px/,
  );
  assert.match(
    css,
    /\.explore-jump button\s*\{[^}]*border-radius:\s*7px[^}]*text-align:\s*center/,
  );
  assert.match(css, /\.explore-jump button:active\s*\{[^}]*border-color:\s*var\(--accent\)/);
  const jumpNav = index.match(/<nav class="explore-jump"[\s\S]*?<\/nav>/)?.[0] || '';
  assert.deepEqual(
    [...jumpNav.matchAll(/data-target="([^"]+)"/g)].map((match) => match[1]),
    ['section-pulse', 'section-galaxy', 'section-analytics', 'section-stories', 'section-coins'],
  );
});

test('coarse pointers get accessible controls and a scroll-releasing galaxy', () => {
  assert.match(css, /@media \(pointer: coarse\)/);
  assert.match(css, /min-height:\s*44px/);
  assert.match(css, /\.galaxy-camera button\s*\{[^}]*min-width:\s*44px;[^}]*height:\s*44px/);
  assert.match(css, /\.galaxy-canvas\s*\{\s*touch-action:\s*pan-y/);
  assert.match(css, /@media \(pointer: coarse\) and \(max-height: 500px\)/);
});

test('responsive CSS uses current visibility, wrapping and scrolling primitives', () => {
  assert.doesNotMatch(css, /clip:\s*rect\(/);
  assert.match(css, /clip-path:\s*inset\(50%\)/);
  assert.doesNotMatch(css, /word-break:\s*break-word/);
  assert.match(css, /overflow-wrap:\s*anywhere/);
  assert.doesNotMatch(css, /-webkit-overflow-scrolling/);
});

test('phone forms, playground modes, transaction cells and API tables have responsive contracts', () => {
  assert.match(css, /input,\s*textarea,\s*select,[\s\S]*?font-size:\s*16px/);
  assert.match(css, /\.pg-modes\s*\{[\s\S]*?grid-template-columns:\s*repeat\(3,\s*minmax\(0,\s*1fr\)\)/);
  assert.match(css, /\.tx-cells\s*\{\s*grid-template-columns:\s*minmax\(0,\s*1fr\)/);
  assert.match(css, /\.api-nav\s*\{[\s\S]*?flex-flow:\s*row nowrap[\s\S]*?overflow-x:\s*auto/);
  assert.match(css, /\.ep-fields,\s*\.ep-params\s*\{[\s\S]*?overflow-x:\s*auto/);
});

test('playground markup exposes navigation and associated form labels', () => {
  assert.equal((index.match(/<nav class="pg-modes"/g) || []).length, 3);
  assert.doesNotMatch(index, /<div class="pg-modes"/);
  assert.match(index, /<label class="sr-only" for="decode-input">/);
  assert.match(index, /<label class="compiler-label" for="compiler-src">/);
  assert.match(index, /<label class="compiler-label" for="compiler-args">/);
  assert.match(index, /<label class="sr-only" for="preflight-input">/);
});

test('retention copy consistently uses the current roughly-30-hour window', () => {
  assert.doesNotMatch(index, /after 3(?:&nbsp;|\s)days/i);
  assert.doesNotMatch(index, /story within days/i);
  assert.match(index, /roughly 30(?:&nbsp;|\s)hours/i);
});

test('the builder guide is phone-safe inside the shell it now shares', () => {
  /* it lives in index.html, so it inherits the site's viewport, skip link and
     focus styles — what stays guide-specific is the two-column collapse, the
     scrollable wide tables, and touch-sized controls */
  assert.doesNotMatch(index, /(?:href|src)="[^"]*guide\.html/);
  assert.match(css, /@media \(max-width: 800px\)[\s\S]*?\.guide-layout\s*\{[^}]*grid-template-columns:\s*1fr/);
  assert.match(css, /@media \(max-width: 800px\)[\s\S]*?\.guide-toc\s*\{[^}]*position:\s*static/);
  assert.match(css, /\.gd-scroll\s*\{[^}]*overflow-x:\s*auto/);
  assert.match(css, /@media \(pointer: coarse\)[\s\S]*?\.guide-toc a\s*\{[^}]*min-height:\s*44px/);
  assert.equal((index.match(/class="gd-scroll" tabindex="0"/g) || []).length, 2);
});

test('the mempool feed keeps a whole number of rows on phones and coarse pointers', () => {
  /* 2.75rem = 44px, the tap-target floor this file already asserts, and the
     rows are links — so the slot height and the tap target are one number */
  assert.match(css, /--pending-row-h:\s*2\.75rem/);
  const at = css.indexOf('--pending-row-h: 2.9rem');
  const phone = css.slice(css.lastIndexOf('@media (max-width: 640px)', at), css.indexOf('\n}\n', at) + 3);
  assert.match(phone, /\.pending \{ --pending-row-h: 2\.9rem; --pending-rows: 5; \}/);
  /* the expanded value MUST be re-declared inside every query that changes the
     row count: .pending[data-expanded="true"] is (0,2,0) and would otherwise
     leak the desktop 12 onto a phone */
  assert.match(phone, /\.pending\[data-expanded="true"\] \{ --pending-rows: 9; \}/);
  assert.match(phone, /\.pending-row \{ grid-template-columns: 5\.2rem minmax\(0, 1fr\)/);
  /* the same leak, one media block down: a landscape phone caps at 4 slots, so
     its expanded value has to be capped too or the frame is taller than the screen */
  assert.match(
    css,
    /@media \(pointer: coarse\) and \(max-height: 500px\) \{[\s\S]*?\.pending \{ --pending-rows: 4; \}\s*\.pending\[data-expanded="true"\] \{ --pending-rows: 6; \}/,
  );
  /* the header's height must depend on the viewport and NOTHING else: the words
     are their own element (an anonymous text flex item wraps at min-content) and
     the status pill has a fixed width (a max-width tracked the count string, so
     9 -> 10 pending txs re-wrapped the heading and moved the page ~18px) */
  assert.match(css, /\.pending h2 \{ flex-wrap: nowrap; \}/);
  assert.match(css, /\.pending h2 > \* \{ white-space: nowrap; \}/);
  assert.match(css, /\.pending-title \{[^}]*white-space: nowrap[^}]*text-overflow: ellipsis/);
  /* comments stripped: this block's comment names the mistakes it fixed */
  const pill = css.match(/\.pending-status \{([\s\S]*?)\n\}/)[1].replace(/\/\*[\s\S]*?\*\//g, '');
  assert.match(pill, /width:\s*11rem/);
  assert.doesNotMatch(pill, /max-width/);
  assert.doesNotMatch(pill, /overflow:\s*hidden/);   /* it clipped the LED's pulse ring */
  assert.match(css, /@media \(max-width: 460px\)[\s\S]*?\.pending h2 \{ flex-wrap: wrap;/);
  assert.match(index, /id="pending-row"[^>]*tabindex="0"/);
});
