import assert from 'node:assert/strict';
import test from 'node:test';

import { networkRouteHash } from './core/routing.js';

test('network-scoped pages keep the selected network in their URL', () => {
  assert.equal(networkRouteHash({ view: 'dev' }, 'mainnet'), '#/mainnet/dev');
  assert.equal(networkRouteHash({ view: 'explore' }, 'mainnet'), '#/mainnet/explore');
  assert.equal(networkRouteHash({ view: 'tokens' }, 'testnet-10'), '#/testnet-10/tokens');
});

test('network switching preserves useful route parameters', () => {
  assert.equal(
    networkRouteHash({ view: 'explore', galaxy: true }, 'mainnet'),
    '#/mainnet/explore?galaxy=1',
  );
  assert.equal(
    networkRouteHash({ view: 'decode', s: 'pk(A) && older(10)' }, 'mainnet'),
    '#/mainnet/decode?s=pk%28A%29%20%26%26%20older%2810%29',
  );
});

test('coin and token switches land on a valid destination', () => {
  assert.equal(
    networkRouteHash({ view: 'detail', id: 'deadbeef' }, 'mainnet'),
    '#/mainnet/explore',
  );
  assert.equal(
    networkRouteHash({ view: 'token', id: 'deadbeef' }, 'mainnet'),
    '#/mainnet/tokens',
  );
});

test('the application resets the Galaxy when a route changes networks', async () => {
  const { readFile } = await import('node:fs/promises');
  const source = await readFile(new URL('./app.js', import.meta.url), 'utf8');
  assert.match(source, /function resetGalaxyNetworkState\(\)/);
  assert.match(source, /function selectNetwork\(network\)[\s\S]*resetGalaxyNetworkState\(\)/);
});
