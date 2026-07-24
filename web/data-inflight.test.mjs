import assert from 'node:assert/strict';
import test from 'node:test';

import { state } from './core/state.js';
import { loadActivity, loadLifespans, loadNetwork, loadTemplates } from './core/data.js';

function response(data) {
  return { ok: true, status: 200, json: async () => data };
}

test('parallel cold data loads share one request per resource', async () => {
  const network = 'inflight-test';
  const originalFetch = globalThis.fetch;
  const calls = new Map();
  globalThis.fetch = async (url) => {
    calls.set(url, (calls.get(url) || 0) + 1);
    await new Promise((resolve) => setTimeout(resolve, 10));
    if (url.includes('/templates.json')) return response({ templates: [] });
    if (url.includes('/activity.json')) return response({ buckets: [] });
    if (url.includes('/lifespans.json')) return response({ buckets: [] });
    return response({
      generated_at_ms: 1,
      tip_daa: 1,
      tip_at_ms: 1,
      stats: { events: 0, covenants: 0, last_activity_daa: 0 },
      covenants: [],
    });
  };

  try {
    await Promise.all([loadNetwork(network), loadNetwork(network), loadNetwork(network)]);
    await Promise.all([loadTemplates(network), loadTemplates(network)]);
    await Promise.all([loadActivity(network, '24h'), loadActivity(network, '24h')]);
    await Promise.all([loadLifespans(network), loadLifespans(network), loadLifespans(network)]);
    assert.equal(calls.get(`data/${network}.json?limit=2000`), 1);
    assert.equal(calls.get(`data/${network}/templates.json`), 1);
    assert.equal(calls.get(`data/${network}/activity.json?range=24h`), 1);
    assert.equal(calls.get(`data/${network}/lifespans.json`), 1);
  } finally {
    delete state.cache[network];
    delete state.templates[network];
    delete state.activity[network];
    delete state.lifespans[network];
    globalThis.fetch = originalFetch;
  }
});
