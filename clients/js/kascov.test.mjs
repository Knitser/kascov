/* The thin client's one real contract is the URLs it constructs: every method
   must hit a route the worker actually registers (main.rs), and the SSE URL
   in particular has been wrong before — so its exact shape is pinned here,
   with and without the per-covenant filter. The optional lane token must ride
   on EVERY request or on none.

   Run:  node --test clients/js/kascov.test.mjs  */
import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';

import { Kascov, canonicalJson, sha256Hex, verifyBadge } from './kascov.mjs';

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

test('token, trade, market, pool, vesting, index, and OpenAPI methods pin the public routes', async () => {
  const { impl, calls } = fakeFetch();
  await withFetch(impl, async () => {
    const k = new Kascov('testnet-10');
    await k.tokens({ limit: 5, status: 'verified', q: 'tree' });
    await k.token(COVENANT, { eventsLimit: 7, order: 'desc' });
    await k.tokenHolders(COVENANT, { limit: 3, afterBalance: 9, afterOwner: '02aa' });
    await k.tokenEvents(COVENANT, { limit: 4, beforeSeq: 8 });
    await k.tokenTrades(COVENANT, { limit: 6, beforeSeq: 7 });
    await k.trades({ limit: 2, tokenId: COVENANT, side: 'buy' });
    await k.markets({ phase: 'bonding', priced: true });
    await k.market(COVENANT);
    await k.tokenMarket(COVENANT);
    await k.pools({ priced: false });
    await k.pool(COVENANT);
    await k.vesting();
    await k.vestingDetail(COVENANT);
    await k.vestingClaims(COVENANT);
    await k.index();
    await k.openapi();
  });
  const n = 'https://kascov.io/data/testnet-10';
  assert.deepEqual(calls.map((call) => call.url), [
    `${n}/tokens.json?limit=5&status=verified&q=tree`,
    `${n}/token/${COVENANT}?events_limit=7&order=desc`,
    `${n}/token/${COVENANT}/holders?limit=3&after_balance=9&after_owner=02aa`,
    `${n}/token/${COVENANT}/events?limit=4&before_seq=8`,
    `${n}/token/${COVENANT}/trades?limit=6&before_seq=7`,
    `${n}/trades?limit=2&token_id=${COVENANT}&side=buy`,
    `${n}/markets?phase=bonding&priced=true`,
    `${n}/market/${COVENANT}`,
    `${n}/token/${COVENANT}/market`,
    `${n}/pools?priced=false`,
    `${n}/pool/${COVENANT}`,
    `${n}/vesting`,
    `${n}/vesting/${COVENANT}`,
    `${n}/vesting/${COVENANT}/claims`,
    `${n}/index.json`,
    'https://kascov.io/openapi.json',
  ]);
});

/* ---- passport badge verification -------------------------------------
   The scheme here is the spec the bot's merkle publisher
   (scripts/discord-holder-bot.mjs) must match: sha256 over canonical
   sorted-key JSON for the leaf, pair-sorted concatenation up the tree.
   The tree in these tests is built with node:crypto, independently of the
   client's plain-js sha256 — so a hash bug cannot cancel itself out. */

const nodeSha = (buf) => createHash('sha256').update(buf).digest('hex');
const nodePair = (a, b) => {
  const [lo, hi] = a <= b ? [a, b] : [b, a];
  return nodeSha(Buffer.from(lo + hi, 'hex'));
};

const CLAIMS = [
  { address: 'kaspa:qq0badge0', role: 'verified-holder', since: 1 },
  { address: 'kaspa:qq1badge1', role: 'auditor', since: 2 },
  { address: 'kaspa:qq2badge2', role: 'voter', since: 3 },
  { address: 'kaspa:qq3badge3', role: 'watchtower', since: 4 },
];
/* the same tree the fixture below pins, rebuilt from scratch each run */
const buildTree = () => {
  const leaves = CLAIMS.map((c) => nodeSha(Buffer.from(canonicalJson(c), 'utf8')));
  const n01 = nodePair(leaves[0], leaves[1]);
  const n23 = nodePair(leaves[2], leaves[3]);
  return { leaves, n01, n23, root: nodePair(n01, n23) };
};

test('the plain-js sha256 agrees with node:crypto, block edges included', () => {
  // 55/56/64 straddle the padding boundaries where hand-rolled sha256 breaks
  for (const n of [0, 3, 55, 56, 63, 64, 65, 1000]) {
    const bytes = Uint8Array.from({ length: n }, (_, i) => i % 251);
    assert.equal(sha256Hex(bytes), nodeSha(Buffer.from(bytes)), `length ${n}`);
  }
  assert.equal(sha256Hex('abc'), nodeSha(Buffer.from('abc')));
});

test('canonical JSON sorts keys recursively and pins the exact wire bytes', () => {
  assert.equal(
    canonicalJson(CLAIMS[0]),
    '{"address":"kaspa:qq0badge0","role":"verified-holder","since":1}',
  );
  // key order in the source object must not matter
  assert.equal(
    canonicalJson({ since: 1, role: 'verified-holder', address: 'kaspa:qq0badge0' }),
    canonicalJson(CLAIMS[0]),
  );
  assert.equal(canonicalJson({ b: [1, { z: null, a: true }] }), '{"b":[1,{"a":true,"z":null}]}');
});

test('a known-good badge round-trips for every leaf of the tree', () => {
  const { leaves, n01, n23, root } = buildTree();
  const proofs = [
    [leaves[1], n23],
    [leaves[0], n23],
    [leaves[3], n01],
    [leaves[2], n01],
  ];
  CLAIMS.forEach((claim, i) => {
    assert.equal(verifyBadge(claim, proofs[i], root), true, `leaf ${i}`);
  });
});

test('an empty proof means the claim is the whole tree', () => {
  const root = nodeSha(Buffer.from(canonicalJson(CLAIMS[0]), 'utf8'));
  assert.equal(verifyBadge(CLAIMS[0], [], root), true);
  assert.equal(verifyBadge(CLAIMS[1], [], root), false);
});

test('tampering rejects: claim, proof, root, and a dropped step', () => {
  const { leaves, n23, root } = buildTree();
  const proof = [leaves[1], n23];
  assert.equal(verifyBadge(CLAIMS[0], proof, root), true, 'baseline');
  assert.equal(verifyBadge({ ...CLAIMS[0], role: 'auditor' }, proof, root), false);
  assert.equal(verifyBadge(CLAIMS[0], [leaves[2], n23], root), false);
  assert.equal(verifyBadge(CLAIMS[0], proof, root.replace(/^./, root[0] === '0' ? '1' : '0')), false);
  assert.equal(verifyBadge(CLAIMS[0], proof.slice(0, 1), root), false);
});

test('malformed input fails closed — false, never a throw', () => {
  const { root } = buildTree();
  assert.equal(verifyBadge(CLAIMS[0], 'not-an-array', root), false);
  assert.equal(verifyBadge(CLAIMS[0], [], 'too-short'), false);
  assert.equal(verifyBadge(CLAIMS[0], [42], root), false);
  assert.equal(verifyBadge(CLAIMS[0], ['zz'.repeat(32)], root), false);
  assert.equal(verifyBadge(CLAIMS[0], [], null), false);
});

test('the cross-client fixture verifies (the same literals live in test_kascov.py)', () => {
  // computed once with node:crypto; both clients pin these exact values so
  // the js and python implementations cannot drift apart
  const proof = [
    '66d4d1b4cf99b94342e50ad8f7703a045562bf8b129e7e7a1a9ecffa0f4e9a04',
    '8825df4c9aac329f511ffd1f81a0934f1524667fe0d5f02ef8ff974cfb579e7d',
  ];
  const root = '064e13348496f570aa3b2d0a9fa33b25f7be66842bef99c7dbba9a4a65e95d4b';
  assert.equal(verifyBadge(CLAIMS[0], proof, root), true);
  assert.equal(buildTree().root, root, 'the rebuilt tree matches the pinned root');
  // uppercase hex is accepted — the wire may shout, the bytes are the same
  assert.equal(verifyBadge(CLAIMS[0], proof.map((h) => h.toUpperCase()), root.toUpperCase()), true);
});
