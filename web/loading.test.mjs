import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import {
  connectionAllowsHeavyData,
  galaxyPreloadPolicy,
  routeNeedsSnapshot,
} from './core/loading.js';

test('only landing and explore require the network snapshot', () => {
  assert.equal(routeNeedsSnapshot('landing'), true);
  assert.equal(routeNeedsSnapshot('explore'), true);
  for (const view of ['detail', 'address', 'lane', 'tokens', 'token', 'tx', 'decode', 'build', 'preflight', 'dev', 'guide', 'changelog']) {
    assert.equal(routeNeedsSnapshot(view), false, `${view} must not wait for the grid`);
  }
});

test('galaxy does not preload for a first-time visitor', () => {
  assert.equal(galaxyPreloadPolicy(), 'none');
  assert.equal(galaxyPreloadPolicy({ preference: 'closed' }), 'none');
});

test('an explicit galaxy deep link always loads', () => {
  assert.equal(galaxyPreloadPolicy({
    explicit: true,
    preference: 'closed',
    connection: { saveData: true, effectiveType: '2g' },
    coarsePointer: true,
  }), 'open');
});

test('a remembered open galaxy restores only on an unconstrained desktop connection', () => {
  assert.equal(galaxyPreloadPolicy({ preference: 'open' }), 'restore');
  assert.equal(galaxyPreloadPolicy({ preference: 'open', coarsePointer: true }), 'none');
  assert.equal(galaxyPreloadPolicy({ preference: 'open', connection: { saveData: true } }), 'none');
  assert.equal(galaxyPreloadPolicy({ preference: 'open', connection: { effectiveType: '2g' } }), 'none');
  assert.equal(connectionAllowsHeavyData({ effectiveType: '4g' }), true);
});

test('the application routes snapshot-free views through the shared policy', () => {
  const source = readFileSync(new URL('./app.js', import.meta.url), 'utf8');
  assert.match(source, /if \(!routeNeedsSnapshot\(route\.view\) && views\[route\.view\]\)/);
});
