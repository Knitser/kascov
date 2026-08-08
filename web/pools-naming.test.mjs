import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

/* The pools surfaces used to show the raw chain codename even for tokens the
 * chain or a launchpad had named, so one coin read as two different assets a
 * single click apart. These pin the shared precedence helper — an on-chain
 * claim outranks the listing, the listing fills the gap, the canonical name
 * never disappears — and that both pools surfaces actually use it. */

const app = readFileSync(new URL('./app.js', import.meta.url), 'utf8');

/* tokenDisplayParts only touches friendlyName, state and registryEntry, so it
   lifts cleanly out of the DOM-heavy module (guide.test.mjs pattern). */
const start = app.indexOf('function tokenDisplayParts(');
assert.ok(start > 0, 'tokenDisplayParts must exist');
const end = app.indexOf('\nfunction ', start + 10);
const factory = new Function(
  'friendlyName', 'state', 'registryEntry',
  `${app.slice(start, end)}\nreturn tokenDisplayParts;`,
);
const friendlyName = (cid) => `codename-${cid.slice(0, 4)}`;
const lift = (registryRow) => factory(
  friendlyName,
  { registry: { mainnet: registryRow ? { data: {} } : null } },
  () => registryRow || null,
);

const CID = 'abcd'.repeat(16);

test('an on-chain claim outranks the listing, field by field', () => {
  const parts = lift({ name: 'Listed Name', ticker: 'LST' })(
    'mainnet', { covenant_id: CID, claimed_name: 'Chain Name', claimed_ticker: 'CHN' });
  assert.equal(parts.display, 'Chain Name');
  assert.equal(parts.ticker, 'CHN');
  assert.equal(parts.canonical, `codename-abcd`);
});

test('the listing fills only the fields the chain left empty', () => {
  const parts = lift({ name: 'Listed Name', ticker: 'LST' })(
    'mainnet', { covenant_id: CID, claimed_ticker: 'CHN' });
  assert.equal(parts.display, 'Listed Name', 'listed name fills the gap');
  assert.equal(parts.ticker, 'CHN', 'but the claimed ticker still wins its field');
});

test('with neither source the canonical codename stands alone', () => {
  const parts = lift(null)('mainnet', { covenant_id: CID });
  assert.equal(parts.display, parts.canonical);
  assert.equal(parts.name, null);
  assert.equal(parts.ticker, null);
});

test('a ticker-only listing still beats the bare codename', () => {
  const parts = lift({ ticker: 'KRON' })('mainnet', { covenant_id: CID });
  assert.equal(parts.display, 'KRON');
});

test('an unwarmed registry never throws — it just yields the codename', () => {
  const parts = factory(friendlyName, { registry: {} }, () => { throw new Error('must not be called'); })(
    'mainnet', { covenant_id: CID });
  assert.equal(parts.display, `codename-abcd`);
});

test('the worker-derived name outranks the generated codename as canonical', () => {
  const parts = lift(null)('mainnet', { covenant_id: CID, name: 'quiet-otter' });
  assert.equal(parts.canonical, 'quiet-otter');
});

/* ---------------------------------------------- the surfaces that use it */

test('the pools list rows go through the shared precedence', () => {
  const renderPools = app.slice(app.indexOf('function renderPools()'), app.indexOf('\nfunction renderPoolPage('));
  assert.match(renderPools, /tokenDisplayParts\(network, t\)/);
  assert.doesNotMatch(renderPools, /const name = friendlyName\(/, 'raw codename rows are gone');
  /* the canonical name rides along whenever a claimed or listed name leads */
  assert.match(renderPools, /token-alias/);
});

test('the pool page title, h1 and wiring sentence all use it', () => {
  const renderPool = app.slice(app.indexOf('function renderPoolPage('), app.indexOf('\nfunction renderVerify('));
  assert.match(renderPool, /tokenDisplayParts\(network, tok\)/);
  assert.match(renderPool, /tokenDisplayParts\(network, lp\)/, 'the share-token link is named too');
  assert.doesNotMatch(renderPool, /friendlyName\(tok\.covenant_id\)/);
  assert.doesNotMatch(renderPool, /friendlyName\(lp\.covenant_id\)/);
  /* the second, slower name source is warmed exactly like the pools list */
  assert.match(renderPool, /warmRegistryOnce\(network, 'pool'/);
  /* the canonical name is demoted, never hidden */
  assert.match(renderPool, /token-alias/);
});
