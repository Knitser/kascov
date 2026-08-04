/* kascov.mjs — a tiny zero-dependency client for the kascov JSON API.
   Works in Node 18+ and the browser (native fetch). CORS is open, no keys.

     import { Kascov } from './kascov.mjs';
     const k = new Kascov('testnet-10');
     const { covenants, next_after_daa } = await k.coins({ limit: 100 });
     const coin = await k.coin(covenants[0].covenant_id);
     for await (const ev of k.stream()) console.log(ev.kind, ev.covenant_id);
     for await (const ev of k.stream({ covenant: id })) ...  // one coin only

   The API needs no token. An optional lane token (minted at kascov.io/lane)
   rides along as an X-Kascov-Lane header on every request; it buys extra
   capacity on the holder lane and nothing else — the anonymous tier keeps
   working without it:

     const k = new Kascov('mainnet', 'https://kascov.io', { laneToken: '...' });

   Passport badges verify locally — no request, no trust in the server:

     import { verifyBadge } from './kascov.mjs';
     verifyBadge(claim, proof, root);  // true only if the claim is in the tree

   Publishing to npm is a separate decision — this file is the whole client. */

const DEFAULT_BASE = 'https://kascov.io';

export class Kascov {
  constructor(network = 'mainnet', base = DEFAULT_BASE, { laneToken = '' } = {}) {
    this.network = network;
    this.base = base.replace(/\/$/, '');
    this.laneToken = laneToken;
  }

  /* every request goes through here so the lane header can never be forgotten
     on one endpoint and sent on another */
  #headers(accept) {
    const h = { accept };
    if (this.laneToken) h['x-kascov-lane'] = this.laneToken;
    return h;
  }

  async #get(path) {
    const res = await fetch(`${this.base}${path}`, { headers: this.#headers('application/json') });
    if (!res.ok) throw new Error(`kascov: ${path} → HTTP ${res.status}`);
    return res.json();
  }

  /** Small fast feed: stats + chain tip + newest ~150 events. Poll this. */
  live() { return this.#get(`/data/${this.network}-live.json`); }

  /** One page of coin summaries, newest activity first.
      opts: { limit, afterDaa, afterId } — pass the previous page's
      next_after_daa / next_after_id to walk older coins. */
  coins(opts = {}) {
    const q = new URLSearchParams();
    if (opts.limit != null) q.set('limit', opts.limit);
    if (opts.afterDaa != null) q.set('after_daa', opts.afterDaa);
    if (opts.afterId != null) q.set('after_id', opts.afterId);
    const s = q.toString();
    return this.#get(`/data/${this.network}.json${s ? `?${s}` : ''}`);
  }

  /** One coin's full story: events (payloads, moved-with), UTXOs (scripts,
      reveals, budgets), holders. */
  coin(covenantId) { return this.#get(`/data/${this.network}/c/${covenantId}.json`); }

  /** Which covenant(s) did this transaction move? */
  tx(txid) { return this.#get(`/data/${this.network}/tx/${txid}.json`); }

  /** Smart coins an address/pubkey funded, received, or controls. */
  address(addrOrPubkey) { return this.#get(`/data/${this.network}/addr/${encodeURIComponent(addrOrPubkey)}.json`); }

  /** Last-24h digest: births/moves/burns, value born, headliner coins. */
  digest() { return this.#get(`/data/${this.network}/digest.json`); }

  /** The whole-network app graph (positions + weighted edges). */
  galaxy() { return this.#get(`/data/${this.network}/galaxy.json`); }

  /** Recent chain reorgs the indexer rolled back through. */
  reorgs() { return this.#get(`/data/${this.network}/reorgs.json`); }

  /** Contract-type analytics (what's running on this network). */
  templates() { return this.#get(`/data/${this.network}/templates.json`); }

  /** Births/moves/burns per DAA bucket. range: 1h|6h|24h|48h|all */
  activity(range = '24h') { return this.#get(`/data/${this.network}/activity.json?range=${range}`); }

  /** The SSE URL the worker actually serves: /data/{network}/stream, with an
      optional ?covenant=<64 hex> filter narrowing it to one coin. Extracted so
      tests pin the shape against the route registered in main.rs. */
  streamUrl(covenant) {
    const q = covenant ? `?covenant=${encodeURIComponent(covenant)}` : '';
    return `${this.base}/data/${this.network}/stream${q}`;
  }

  /** Live events as an async iterator (SSE). Yields {covenant_id, kind,
      txid, accepting_daa}. Hints only — refetch details on receipt.
      opts.covenant: exactly 64 hex chars follows ONE coin; anything else is
      a 400 from the server, never a silent firehose. */
  async *stream({ covenant, signal } = {}) {
    const res = await fetch(this.streamUrl(covenant), {
      headers: this.#headers('text/event-stream'),
      signal,
    });
    if (!res.ok || !res.body) throw new Error(`kascov: stream → HTTP ${res.status}`);
    const reader = res.body.getReader();
    const dec = new TextDecoder();
    let buf = '';
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return;
      buf += dec.decode(value, { stream: true });
      let i;
      while ((i = buf.indexOf('\n\n')) >= 0) {
        const chunk = buf.slice(0, i);
        buf = buf.slice(i + 2);
        const data = chunk.split('\n').filter((l) => l.startsWith('data:')).map((l) => l.slice(5).trim()).join('');
        if (!data) continue;
        try { yield JSON.parse(data); } catch { /* keepalive/comment */ }
      }
    }
  }
}

/* ------------------------------------------------------------------------
   Passport badge verification — pure and local. The scheme (which the bot's
   merkle publisher in scripts/discord-holder-bot.mjs must match exactly —
   the publisher was not yet written when this landed, so this file is the
   spec both sides pin):

     leaf  = sha256(canonical JSON of the claim)   // keys sorted, no spaces
     node  = sha256(lo ++ hi)                      // the PAIR sorted as bytes
     root  = the published 32-byte hex string

   Claims stick to strings, integers, booleans and null — floats serialize
   differently across languages and would fork the leaf hash. */

/** Canonical JSON: recursively sorted keys, no whitespace, raw unicode.
    Matches python's json.dumps(sort_keys=True, separators=(",", ":"),
    ensure_ascii=False) for claims built from the types listed above. */
export function canonicalJson(value) {
  if (value === null || typeof value !== 'object') return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
  return `{${Object.keys(value).sort()
    .map((k) => `${JSON.stringify(k)}:${canonicalJson(value[k])}`).join(',')}}`;
}

// sha256 in plain js so the module keeps its browser promise (a top-level
// node:crypto import would break a native browser import) and verification
// stays synchronous. cross-checked against node:crypto in the tests.
const K = [
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];
const rotr = (x, n) => ((x >>> n) | (x << (32 - n))) >>> 0;

/** sha256 as lowercase hex. Accepts a string (UTF-8 encoded) or Uint8Array. */
export function sha256Hex(input) {
  const bytes = typeof input === 'string' ? new TextEncoder().encode(input) : input;
  // pad to a 64-byte multiple: 0x80, zeros, 64-bit big-endian bit length
  const bitLen = bytes.length * 8;
  const padded = new Uint8Array((((bytes.length + 8) >> 6) << 6) + 64);
  padded.set(bytes);
  padded[bytes.length] = 0x80;
  const dv = new DataView(padded.buffer);
  dv.setUint32(padded.length - 8, Math.floor(bitLen / 4294967296));
  dv.setUint32(padded.length - 4, bitLen >>> 0);
  const h = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
  const w = new Uint32Array(64);
  for (let off = 0; off < padded.length; off += 64) {
    for (let i = 0; i < 16; i++) w[i] = dv.getUint32(off + i * 4);
    for (let i = 16; i < 64; i++) {
      const s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >>> 3);
      const s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) >>> 0;
    }
    let [a, b, c, d, e, f, g, hh] = h;
    for (let i = 0; i < 64; i++) {
      const t1 = (hh + (rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25)) + ((e & f) ^ (~e & g)) + K[i] + w[i]) >>> 0;
      const t2 = ((rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22)) + ((a & b) ^ (a & c) ^ (b & c))) >>> 0;
      hh = g; g = f; f = e; e = (d + t1) >>> 0; d = c; c = b; b = a; a = (t1 + t2) >>> 0;
    }
    [a, b, c, d, e, f, g, hh].forEach((v, i) => { h[i] = (h[i] + v) >>> 0; });
  }
  return h.map((v) => v.toString(16).padStart(8, '0')).join('');
}

const HEX64 = /^[0-9a-f]{64}$/;
const hexToBytes = (hex) => {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
};

/** Verify a passport badge against a published merkle root, entirely locally.
    claim: the claim object as published; proof: sibling hashes leaf→root
    (64-char hex each); root: the published root (64-char hex). An empty proof
    means the claim IS the whole tree. Malformed input returns false — the
    verifier fails closed, it never throws. */
export function verifyBadge(claim, proof, root) {
  if (!Array.isArray(proof) || typeof root !== 'string') return false;
  const want = root.toLowerCase();
  if (!HEX64.test(want)) return false;
  let cur = sha256Hex(canonicalJson(claim));
  for (const step of proof) {
    if (typeof step !== 'string') return false;
    const sib = step.toLowerCase();
    if (!HEX64.test(sib)) return false;
    // pair-sorted concatenation: lexicographic hex order == byte order here
    const [lo, hi] = cur <= sib ? [cur, sib] : [sib, cur];
    cur = sha256Hex(hexToBytes(lo + hi));
  }
  return cur === want;
}

export default Kascov;
