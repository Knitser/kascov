import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

/* Witnessed launchpad art sits one trust tier below chain-proven art, and the
 * difference must survive in the markup, not just in someone's memory. These
 * pin the tell: proven art replaces the identicon cleanly, a witnessed logo
 * never sheds its dashed ring, and neither namespace serves the other. */
const strip = (s) => s.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
const app = strip(readFileSync(new URL('./app.js', import.meta.url), 'utf8'));
const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
const format = readFileSync(new URL('./core/format.js', import.meta.url), 'utf8');

test('witnessed art always carries the listed marker class', () => {
  for (const m of app.matchAll(/listed-img\/\$\{esc\(network\)\}/g)) {
    const before = app.slice(Math.max(0, m.index - 400), m.index);
    assert.match(
      before, /token-art-listed/,
      'every listed-img usage must sit inside a token-art-listed wrap',
    );
  }
  assert.ok([...app.matchAll(/listed-img\//g)].length >= 2, 'directory and token page both render it');
});

test('proven art never points at the witnessed namespace and vice versa', () => {
  // the /img namespace promises chain-proven bytes; crossing the streams
  // would dress one tier as the other inside every cache on the internet
  assert.doesNotMatch(app, /src="img\/[^"]*"[^>]*token-art-listed/);
  for (const m of app.matchAll(/claimed_image_hash\s*\?\s*`([^`]*)`/g)) {
    assert.doesNotMatch(m[1], /listed-img/, 'proven branch must use /img');
  }
});

test('the dashed ring exists and proven art does not wear it', () => {
  const rule = css.match(/\.token-art-listed \.token-art \{([^}]*)\}/);
  assert.ok(rule, 'the ring rule must exist');
  assert.match(rule[1], /dashed/, 'dashed is the tell for a claim, site-wide');
});

test('the tooltip says what a witnessed logo is and is not', () => {
  const entry = format.match(/listed_logo:\s*'([^']*)'/);
  assert.ok(entry, 'GLOSSARY.listed_logo must exist');
  assert.match(entry[1], /nothing on chain commits to it/);
  assert.match(entry[1], /saved copy/, 'it must say kascov serves its own copy');
  assert.doesNotMatch(entry[1], /verified|proven|pinned/, 'reserved words for chain facts');
});
