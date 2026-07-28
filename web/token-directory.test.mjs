import assert from 'node:assert/strict';
import test from 'node:test';

import {
  selectTokens,
  tokenLifecycle,
  tokenValidation,
} from './core/token-directory.js';

const rows = [
  { covenant_id: 'aa', claimed_ticker: 'ALFA', status: 'verified', alive: true, holders: 3, supply: 100, live_value: 9, last_activity_daa: 2 },
  { covenant_id: 'bb', claimed_name: 'Beta Coin', status: 'invalid', alive: false, holders: 9, supply: 50, live_value: 2, last_activity_daa: 4 },
  { covenant_id: 'cc', name: 'canonical-gamma', status: 'active', holders: 1, supply: 200, live_value: 12, last_activity_daa: 3 },
];

test('validation and lifecycle remain separate facts', () => {
  assert.equal(tokenValidation(rows[0]), 'verified');
  assert.equal(tokenLifecycle(rows[0]), 'alive');
  assert.equal(tokenValidation(rows[1]), 'invalid');
  assert.equal(tokenLifecycle(rows[1]), 'retired');
  assert.equal(tokenValidation(rows[2]), 'unknown');
  assert.equal(tokenLifecycle(rows[2]), 'alive');
});

test('directory searches claimed and canonical identity', () => {
  assert.deepEqual(selectTokens(rows, { query: 'alfa' }).map((r) => r.covenant_id), ['aa']);
  assert.deepEqual(selectTokens(rows, { query: 'gamma' }).map((r) => r.covenant_id), ['cc']);
});

/* A row that DISPLAYS a launchpad's name has to be findable by typing it.
   These tokens carry no name on chain at all, so without this the directory
   shows KRON and then finds nothing when you search KRON. */
test('directory finds a token by the name a launchpad lists it under', () => {
  const listed = [
    { covenant_id: 'dd', listed_ticker: 'KRON', listed_name: 'Kron Token', status: 'verified', alive: true },
    { covenant_id: 'ee', status: 'verified', alive: true },
  ];
  assert.deepEqual(selectTokens(listed, { query: 'kron' }).map((r) => r.covenant_id), ['dd']);
  assert.deepEqual(selectTokens(listed, { query: 'kron token' }).map((r) => r.covenant_id), ['dd']);
});

test('directory filters and sorts without mutating the response', () => {
  const original = rows.map((r) => r.covenant_id);
  assert.deepEqual(selectTokens(rows, { validation: 'verified' }).map((r) => r.covenant_id), ['aa']);
  assert.deepEqual(selectTokens(rows, { lifecycle: 'retired' }).map((r) => r.covenant_id), ['bb']);
  assert.deepEqual(selectTokens(rows, { sort: 'supply' }).map((r) => r.covenant_id), ['cc', 'aa', 'bb']);
  assert.deepEqual(rows.map((r) => r.covenant_id), original);
});
