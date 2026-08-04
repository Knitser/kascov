/* The thin client's one real contract is the URLs it constructs: every method
   must hit a route the worker actually registers (main.rs), and the SSE URL
   in particular has been wrong before — so its exact shape is pinned here,
   with and without the per-covenant filter. The optional lane token must ride
   on EVERY request or on none.

   Run:  node --test clients/js/kascov.test.mjs  */
import test from 'node:test';
import assert from 'node:assert/strict';

import { Kascov } from './kascov.mjs';

const COVENANT = 'a'.repeat(64);

/* a fake fetch that records every call and answers with a canned JSON body */
const fakeFetch = () => {
  const calls = [];
  const impl = async (url, opts = {}) => {
    calls.push({ url: String(url), headers: opts.headers || {} });
    return { ok: true, status: 200, json: async () => ({}), body: null };
  };
  return { impl, calls };
};

/* an SSE body: fetch-Response-shaped, replaying the given wire chunks */
const sseResponse = (...chunks) => ({
  ok: true,
  status: 200,
  body: {
    getReader() {
      let i = 0;
      return {
        read: async () => (i < chunks.length
          ? { done: false, value: new TextEncoder().encode(chunks[i++]) }
          : { done: true, value: undefined }),
      };
    },
  },
});

const withFetch = async (impl, fn) => {
  const real = globalThis.fetch;
  globalThis.fetch = impl;
  try { return await fn(); } finally { globalThis.fetch = real; }
};

test('the stream URL matches the live route: /data/{network}/stream', () => {
  // main.rs registers .route("/data/{network}/stream", get(stream_handler))
  const k = new Kascov('testnet-10');
  assert.equal(k.streamUrl(), 'https://kascov.io/data/testnet-10/stream');
});

test('a covenant narrows the stream via ?covenant=, never a path segment', () => {
  // the worker takes the filter as a query param; a path-shaped guess 404s
  const k = new Kascov('mainnet');
  assert.equal(
    k.streamUrl(COVENANT),
    `https://kascov.io/data/mainnet/stream?covenant=${COVENANT}`,
  );
});

test('a custom base keeps the same path shape, trailing slash and all', () => {
  const k = new Kascov('testnet-10', 'http://localhost:8080/');
  assert.equal(k.streamUrl(), 'http://localhost:8080/data/testnet-10/stream');
});

test('stream() actually fetches the pinned URL', async () => {
  const seen = [];
  const impl = async (url) => { seen.push(String(url)); return sseResponse(); };
  await withFetch(impl, async () => {
    const events = [];
    for await (const ev of new Kascov('testnet-10').stream({ covenant: COVENANT })) events.push(ev);
    assert.deepEqual(events, []);
  });
  assert.deepEqual(seen, [`https://kascov.io/data/testnet-10/stream?covenant=${COVENANT}`]);
});

test('SSE frames parse to events; keep-alive comments are skipped', async () => {
  const impl = async () => sseResponse(
    ': connected\n\n',
    'data: {"kind":"birth","covenant_id":"',
    `${COVENANT}"}\n\n: ka\n\n`,
    'data: {"kind":"move"}\n\n',
  );
  await withFetch(impl, async () => {
    const events = [];
    for await (const ev of new Kascov('testnet-10').stream()) events.push(ev);
    assert.deepEqual(events.map((e) => e.kind), ['birth', 'move']);
    assert.equal(events[0].covenant_id, COVENANT);
  });
});

test('no lane token configured: no lane header leaks onto any request', async () => {
  const { impl, calls } = fakeFetch();
  await withFetch(impl, () => new Kascov('mainnet').live());
  assert.equal(calls.length, 1);
  assert.ok(!('x-kascov-lane' in calls[0].headers));
});

test('a lane token rides as X-Kascov-Lane on plain GETs AND the stream', async () => {
  // the token is minted at kascov.io/lane; capacity only, never verdicts
  const calls = [];
  const impl = async (url, opts = {}) => {
    calls.push({ url: String(url), headers: opts.headers || {} });
    return String(url).includes('/stream')
      ? sseResponse()
      : { ok: true, status: 200, json: async () => ({}) };
  };
  await withFetch(impl, async () => {
    const k = new Kascov('mainnet', 'https://kascov.io', { laneToken: 'tok-123' });
    await k.live();
    for await (const _ of k.stream()) { /* drain */ }
  });
  assert.equal(calls.length, 2);
  for (const c of calls) assert.equal(c.headers['x-kascov-lane'], 'tok-123');
});

test('the polling endpoints still hit their registered routes', async () => {
  const { impl, calls } = fakeFetch();
  await withFetch(impl, async () => {
    const k = new Kascov('testnet-10');
    await k.live();
    await k.coins({ limit: 5, afterDaa: 9, afterId: 'ff' });
    await k.coin(COVENANT);
  });
  assert.deepEqual(calls.map((c) => c.url), [
    'https://kascov.io/data/testnet-10-live.json',
    'https://kascov.io/data/testnet-10.json?limit=5&after_daa=9&after_id=ff',
    `https://kascov.io/data/testnet-10/c/${COVENANT}.json`,
  ]);
});
