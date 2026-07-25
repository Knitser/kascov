import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
const index = readFileSync(new URL('./index.html', import.meta.url), 'utf8');
const guide = readFileSync(new URL('./guide.html', import.meta.url), 'utf8');

test('phone header and navigation remain reachable without page overflow', () => {
  assert.match(css, /@media \(max-width: 720px\)[\s\S]*?\.header-inner\s*\{[\s\S]*?display:\s*grid/);
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

test('standalone guide has a phone-safe and keyboard-visible shell', () => {
  assert.match(guide, /viewport-fit=cover/);
  assert.match(guide, /<a class="skip-link" href="#main">/);
  assert.match(guide, /<main id="main"/);
  assert.match(guide, /:focus-visible/);
  assert.match(guide, /@media \(pointer: coarse\)/);
  assert.match(guide, /class="table-scroll" tabindex="0"/);
});
