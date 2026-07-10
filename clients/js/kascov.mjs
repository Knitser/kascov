/* kascov.mjs — a tiny zero-dependency client for the kascov JSON API.
   Works in Node 18+ and the browser (native fetch). CORS is open, no keys.

     import { Kascov } from './kascov.mjs';
     const k = new Kascov('testnet-10');
     const { covenants, next_after_daa } = await k.coins({ limit: 100 });
     const coin = await k.coin(covenants[0].covenant_id);
     for await (const ev of k.stream()) console.log(ev.kind, ev.covenant_id);

   Publishing to npm is a separate decision — this file is the whole client. */

const DEFAULT_BASE = 'https://kascov.io';

export class Kascov {
  constructor(network = 'mainnet', base = DEFAULT_BASE) {
    this.network = network;
    this.base = base.replace(/\/$/, '');
  }

  async #get(path) {
    const res = await fetch(`${this.base}${path}`, { headers: { accept: 'application/json' } });
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

  /** Live events as an async iterator (SSE). Yields {covenant_id, kind,
      txid, accepting_daa}. Hints only — refetch details on receipt. */
  async *stream({ signal } = {}) {
    const res = await fetch(`${this.base}/data/${this.network}/stream`, {
      headers: { accept: 'text/event-stream' },
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

export default Kascov;
