import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

/* Search results carry two very different kinds of name. The canonical slug is
 * derived from the coin's own id, so kascov owns it. A claimed name is a string
 * the deployer wrote into a payload, and soon a string a launchpad publishes in
 * a list, and kascov owns none of it.
 *
 * The search endpoint has always distinguished the two (`matched: "claimed"`),
 * and the frontend used to throw that away and print "from the chain" over
 * every remote hit, which stated the opposite of the truth for exactly the rows
 * where it mattered. These pin the fix, the way pending.test.mjs pins the
 * mempool geometry: by reading the shipped source as text.
 *
 * Comments are STRIPPED before asserting, because the comments beside this code
 * quote the very strings the assertions forbid. */
const strip = (s) => s.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
const app = readFileSync(new URL('./app.js', import.meta.url), 'utf8');
const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
const format = readFileSync(new URL('./core/format.js', import.meta.url), 'utf8');
const main = readFileSync(new URL('../crates/kascov/src/main.rs', import.meta.url), 'utf8');

const region = (from, to) => strip(app.slice(app.indexOf(from), app.indexOf(to)));
const suggest = region('function renderSuggest(', 'function setActiveSuggest(');
const gridCards = region('function remoteGridCardsHtml(', 'function suggestionItems(');

test('a remote hit keeps the reason the server gave for it', () => {
  // The whole bug in one line: every remote row was labelled identically.
  assert.ok(
    !/why:\s*'remote'\s*,/.test(suggest),
    'renderSuggest must not hardcode why:\'remote\' — it discards `matched`',
  );
  assert.match(suggest, /why:\s*r\.matched/, 'the server\'s `matched` must reach the row');
});

test('a claimed name is never presented as coming from the chain', () => {
  const claimedBranch = suggest.match(/s\.why === 'claimed' \?([\s\S]*?):\n/);
  assert.ok(claimedBranch, 'renderSuggest needs an explicit `claimed` branch');
  assert.doesNotMatch(
    claimedBranch[1], /from the chain/,
    'the claimed branch must not borrow the chain-derived wording',
  );
  assert.match(claimedBranch[1], /claims /, 'it must say whose word the name is');

  // "from the chain" may survive ONLY as the fallback for a row with no
  // `matched` at all, never as the label for a specific match kind.
  for (const [, kind] of suggest.matchAll(/s\.why === '(\w+)' \? `<span class="suggest-kind">from the chain/g)) {
    assert.equal(kind, 'remote', `"from the chain" must not label a '${kind}' match`);
  }
});

test('the grid rescue cards label a claimed hit too', () => {
  assert.match(
    gridCards, /matched === 'claimed'/,
    'remote grid cards render the canonical slug as their title, so an ' +
    'unlabelled claim hit looks like the slug matched the query',
  );
  assert.match(gridCards, /flag-claimed/);
});

test('the claim badge keeps the one styling claims already have', () => {
  // .flag-claimed is shared with the token page's "named on chain" badge, so
  // there must be exactly one rule: a second, later definition would silently
  // win the cascade and this file would be asserting against dead CSS.
  const rules = [...css.matchAll(/^\.flag-claimed[\s{]/gm)];
  assert.equal(rules.length, 1, 'exactly one .flag-claimed rule may exist');
  const claimed = css.match(/^\.flag-claimed \{([^}]*)\}/m);
  assert.match(
    claimed[1], /border:[^;]*dashed/,
    'the dashed border is what marks a badge as somebody\'s claim rather than a derived fact',
  );
});

test('the claim tooltip says what is and is not proven', () => {
  const entry = format.match(/claimed_name:\s*'([^']*)'/);
  assert.ok(entry, 'GLOSSARY.claimed_name must exist');
  assert.match(entry[1], /never that it is true/, 'it must disclaim truth, not just cite a source');
});

test('the endpoint sends the claimed string, not only the fact of a claim', () => {
  // Without it the UI can say "claims something" but never what, because every
  // row's `name` is the canonical slug.
  assert.match(
    main, /row\["claimed"\] = serde_json::Value::String\(claim\)/,
    'the search handler must attach the claimed name to a `claimed` match',
  );
});
