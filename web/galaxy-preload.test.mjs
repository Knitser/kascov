import assert from 'node:assert/strict';
import test from 'node:test';

import { galaxyCache, loadGalaxy } from './core/data.js';

test('parallel galaxy preload and render share one network request', async () => {
  const network = 'preload-test';
  const originalFetch = globalThis.fetch;
  let calls = 0;
  globalThis.fetch = async () => {
    calls += 1;
    await new Promise((resolve) => setTimeout(resolve, 10));
    return {
      ok: true,
      json: async () => ({ tier: 'core', nodes_total: 12, asz: [8, 4] }),
    };
  };

  try {
    const preload = loadGalaxy(network);
    const render = loadGalaxy(network);
    assert.equal(calls, 1);
    assert.deepEqual(await Promise.all([preload, render]), [
      { tier: 'core', nodes_total: 12, asz: [8, 4] },
      { tier: 'core', nodes_total: 12, asz: [8, 4] },
    ]);
  } finally {
    delete galaxyCache[network];
    globalThis.fetch = originalFetch;
  }
});
