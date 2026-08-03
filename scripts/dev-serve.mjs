#!/usr/bin/env node
/* kascov local verification server.
 *
 * Serves web/ the way production does (the routing here mirrors
 * scripts/kascov.Caddyfile) so the SPA behaves like kascov.io on localhost:
 * real files first, then {path}.html, then the index shell for hash routes.
 * Worker routes (/data, /share, /og, /badge, /img, /sitemap.xml, /feed.xml,
 * /health) are reverse-proxied to the live origin, which is what makes the
 * page boot with real snapshots instead of a wall of 404s.
 *
 * It also carries a synthetic mempool, because the pending section
 * feature-hides itself when /pending 404s and testnet-10's real mempool is
 * usually one tx or none — so neither a bare static server nor the live proxy
 * can show a burst, a scroll, or a confirm animation on demand.
 *
 * The synthetic feed is PER NETWORK and, by default, only testnet-10. Mainnet
 * keeps proxying to the real worker, so selecting mainnet shows mainnet's
 * actual mempool and never testnet's invented rows. Set DEMO_NETWORKS to
 * change which segments are faked.
 *
 *   node scripts/dev-serve.mjs                          # testnet-10 synthetic, mainnet real
 *   DEMO_NETWORKS=testnet-10,mainnet node scripts/...    # fake both
 *   DEMO=0 node scripts/dev-serve.mjs                    # fake nothing
 *   PORT=9000 UPSTREAM=https://kascov.io node scripts/dev-serve.mjs
 *
 * Controls, from a second terminal (or just visit them in a tab). They act on
 * the demo networks only, and accept ?net= to single one out:
 *   curl -s localhost:8787/__mempool/storm   # 16 at once: scrolling + row cap
 *   curl -s localhost:8787/__mempool/solo    # exactly one row: the height test
 *   curl -s localhost:8787/__mempool/calm    # confirm everything at once
 *   curl -s localhost:8787/__mempool/drain   # empty it: the empty-state height
 *   curl -s localhost:8787/__mempool/off     # switch to the live mempool
 */

import { randomBytes } from 'node:crypto';
import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { request as httpsRequest } from 'node:https';
import { extname, join, normalize, resolve as resolvePath } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolvePath(fileURLToPath(new URL('../web', import.meta.url)));
const PORT = Number(process.env.PORT || 8787);
const UPSTREAM = process.env.UPSTREAM || 'https://kascov.io';
const UPSTREAM_HOST = new URL(UPSTREAM).hostname;

const NETWORKS = ['mainnet', 'testnet-10'];
/* Only these segments get an invented mempool. Anything else proxies, so the
   network selector keeps meaning what it says. */
const DEMO_NETS = new Set(
  (process.env.DEMO_NETWORKS || 'testnet-10')
    .split(',')
    .map((s) => s.trim())
    .filter((s) => NETWORKS.includes(s)),
);

/* ---------------------------------------------------------------- statics */

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
  '.xml': 'application/xml; charset=utf-8',
  '.txt': 'text/plain; charset=utf-8',
  '.map': 'application/json; charset=utf-8',
};

/* Everything is no-store: this server exists so a reload always shows the
   file that is on disk right now, including mid-edit. */
function sendFile(res, file) {
  const type = MIME[extname(file).toLowerCase()] || 'application/octet-stream';
  res.writeHead(200, { 'content-type': type, 'cache-control': 'no-store' });
  createReadStream(file).pipe(res);
}

function serveStatic(req, res, pathname) {
  const rel = normalize(decodeURIComponent(pathname)).replace(/^(\.\.[/\\])+/, '');
  const target = join(ROOT, rel);
  if (!target.startsWith(ROOT)) { res.writeHead(403).end('forbidden'); return; }
  for (const candidate of [target, `${target}.html`, join(target, 'index.html')]) {
    if (existsSync(candidate) && statSync(candidate).isFile()) { sendFile(res, candidate); return; }
  }
  sendFile(res, join(ROOT, 'index.html')); // SPA shell — hash routes live here
}

/* ------------------------------------------------------- upstream proxy */

const WORKER_PREFIX = /^\/(data|share|og|badge|img|listed-img)\//;
const WORKER_EXACT = new Set(['/sitemap.xml', '/feed.xml', '/health', '/healthz']);
const isWorkerRoute = (p) => WORKER_PREFIX.test(p) || WORKER_EXACT.has(p);

function proxy(req, res, pathname, search) {
  const headers = {
    host: UPSTREAM_HOST,
    accept: req.headers.accept || '*/*',
    'user-agent': 'kascov-dev-serve',
    /* identity, so an SSE body is never handed to us gzip-buffered */
    'accept-encoding': 'identity',
  };
  if (req.headers['content-type']) headers['content-type'] = req.headers['content-type'];
  const up = httpsRequest(
    { hostname: UPSTREAM_HOST, port: 443, path: pathname + search, method: req.method, headers },
    (upRes) => {
      const out = { ...upRes.headers };
      delete out['content-encoding'];
      delete out['content-length'];
      out['cache-control'] = 'no-store';
      res.writeHead(upRes.statusCode || 502, out);
      upRes.pipe(res);
    },
  );
  up.on('error', (err) => {
    if (!res.headersSent) res.writeHead(502, { 'content-type': 'text/plain' });
    res.end(`upstream ${UPSTREAM} unreachable: ${err.message}`);
  });
  req.pipe(up);
}

/* ----------------------------------------------------- synthetic mempool */

const KINDS = ['genesis', 'transition', 'burn'];
const hex64 = () => randomBytes(32).toString('hex');
const pick = (list) => list[Math.floor(Math.random() * list.length)];
const between = (lo, hi) => lo + Math.random() * (hi - lo);

/* One independent feed per faked network. Nothing is shared, so a burst on
   testnet-10 can never appear under mainnet. */
const feeds = new Map(
  [...DEMO_NETS].map((net) => [net, {
    entries: new Map(),
    clients: new Set(),
    revision: 1,
    /* reused covenant ids, so the same coins recur the way real traffic does
       and friendlyName() produces stable names. Distinct per network. */
    coins: Array.from({ length: 14 }, hex64),
  }]),
);
let demoOn = process.env.DEMO !== '0';
const fakes = (net) => demoOn && feeds.has(net);

function broadcast(net, message) {
  const feed = feeds.get(net);
  if (!feed) return;
  feed.revision += 1;
  const frame = `data: ${JSON.stringify({ ...message, revision: feed.revision })}\n\n`;
  for (const client of feed.clients) client.write(frame);
}

function spawn(net, lifetimeMs) {
  const feed = feeds.get(net);
  if (!feed) return null;
  const txid = hex64();
  const extra = Math.random() < 0.16 ? 1 + Math.floor(Math.random() * 2) : 0;
  const events = Array.from({ length: 1 + extra }, () => ({
    covenant_id: pick(feed.coins),
    tx_kind: pick(KINDS),
  }));
  const entry = {
    txid,
    tx_kind: events[0].tx_kind,
    events,
    bornAt: Date.now(),
    resolveAt: Date.now() + (lifetimeMs ?? between(2800, 7600)),
    resolution: Math.random() < 0.84 ? 'confirmed' : 'dropped',
  };
  feed.entries.set(txid, entry);
  broadcast(net, {
    kind: 'pending',
    txid,
    tx_kind: entry.tx_kind,
    covenant_id: events[0].covenant_id,
    events,
  });
  return entry;
}

function resolveEntry(net, entry, resolution) {
  const feed = feeds.get(net);
  if (!feed) return;
  feed.entries.delete(entry.txid);
  broadcast(net, {
    kind: 'pending_resolved',
    txid: entry.txid,
    resolution: resolution || entry.resolution,
  });
}

/* One clock drives arrivals and resolutions for every faked network. It idles
   toward ~6 live rows, busy enough to exercise the feed without becoming a
   stress test. */
const RESTING = 6;
setInterval(() => {
  if (!demoOn) return;
  const now = Date.now();
  for (const [net, feed] of feeds) {
    for (const entry of [...feed.entries.values()]) {
      if (entry.resolveAt <= now) resolveEntry(net, entry);
    }
    const size = feed.entries.size;
    const chance = size < RESTING ? 0.7 : size < 12 ? 0.3 : 0.08;
    if (Math.random() < chance) spawn(net);
  }
}, 750).unref?.();

function pendingSnapshot(net) {
  const feed = feeds.get(net);
  const now = Date.now();
  return {
    status: 'live',
    last_poll_ms: now,
    generated_at_ms: now,
    revision: feed.revision,
    pending: [...feed.entries.values()]
      .sort((a, b) => a.bornAt - b.bornAt)
      .map((entry) => ({
        txid: entry.txid,
        covenant_id: entry.events[0].covenant_id,
        tx_kind: entry.tx_kind,
        age_ms: now - entry.bornAt,
        events: entry.events.map((event) => ({ ...event })),
      })),
  };
}

function openStream(req, res, net) {
  const feed = feeds.get(net);
  res.writeHead(200, {
    'content-type': 'text/event-stream; charset=utf-8',
    'cache-control': 'no-store',
    connection: 'keep-alive',
    'x-accel-buffering': 'no',
  });
  res.write(': connected\n\n');
  feed.clients.add(res);
  const ka = setInterval(() => res.write(': ka\n\n'), 20_000);
  req.on('close', () => { clearInterval(ka); feed.clients.delete(res); });
}

function control(res, action, only) {
  const targets = only && feeds.has(only) ? [only] : [...feeds.keys()];
  const json = (body) => {
    res.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' });
    res.end(JSON.stringify(body, null, 2));
  };
  const live = () => Object.fromEntries([...feeds].map(([n, f]) => [n, f.entries.size]));
  const clearAll = (net, how) => {
    for (const entry of [...feeds.get(net).entries.values()]) resolveEntry(net, entry, how);
  };

  if (action === 'storm') {
    for (const net of targets) for (let i = 0; i < 16; i++) spawn(net, between(6000, 14_000));
    return json({ ok: true, did: 'spawned 16', on: targets, live: live() });
  }
  if (action === 'flood') {
    for (const net of targets) for (let i = 0; i < 40; i++) spawn(net, between(9000, 20_000));
    return json({ ok: true, did: 'spawned 40 (past the 24-row DOM cap)', on: targets, live: live() });
  }
  if (action === 'calm') {
    for (const net of targets) clearAll(net, 'confirmed');
    return json({ ok: true, did: 'confirmed everything', on: targets, live: live() });
  }
  if (action === 'drop') {
    for (const net of targets) clearAll(net, 'dropped');
    return json({ ok: true, did: 'dropped everything', on: targets, live: live() });
  }
  if (action === 'solo') {
    for (const net of targets) { clearAll(net, 'confirmed'); spawn(net, 600_000); }
    return json({ ok: true, did: 'one long-lived row', on: targets, live: live() });
  }
  if (action === 'drain') {
    demoOn = false;
    for (const net of targets) clearAll(net, 'confirmed');
    return json({ ok: true, did: 'emptied and paused arrivals', demo: false });
  }
  if (action === 'on') { demoOn = true; return json({ ok: true, demo: true, faking: [...feeds.keys()] }); }
  if (action === 'off') {
    demoOn = false;
    for (const net of targets) clearAll(net, 'confirmed');
    return json({ ok: true, demo: false, note: `mempool routes now proxy to ${UPSTREAM}` });
  }
  return json({
    demo: demoOn,
    faking: [...feeds.keys()],
    real: NETWORKS.filter((n) => !feeds.has(n)),
    live: live(),
    streams: Object.fromEntries([...feeds].map(([n, f]) => [n, f.clients.size])),
    upstream: UPSTREAM,
    actions: ['storm', 'flood', 'calm', 'drop', 'solo', 'drain', 'on', 'off', 'status'],
    hint: 'add ?net=testnet-10 to act on one network',
  });
}

/* ------------------------------------------------------------------ serve */

const MEMPOOL_ROUTE = /^\/data\/(mainnet|testnet-10)\/(pending|stream)$/;

const server = createServer((req, res) => {
  const url = new URL(req.url, `http://localhost:${PORT}`);
  const { pathname } = url;

  if (pathname.startsWith('/__mempool')) {
    return control(res, pathname.split('/')[2] || 'status', url.searchParams.get('net'));
  }

  const mempool = MEMPOOL_ROUTE.exec(pathname);
  if (mempool && fakes(mempool[1])) {
    if (mempool[2] === 'stream') return openStream(req, res, mempool[1]);
    res.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' });
    return res.end(JSON.stringify(pendingSnapshot(mempool[1])));
  }

  if (isWorkerRoute(pathname)) return proxy(req, res, pathname, url.search);
  if (req.method !== 'GET' && req.method !== 'HEAD') { res.writeHead(405).end(); return; }
  return serveStatic(req, res, pathname);
});

server.on('error', (err) => {
  if (err.code === 'EADDRINUSE') {
    console.error(`port ${PORT} is taken — rerun with PORT=<free port>`);
    process.exit(1);
  }
  throw err;
});

server.listen(PORT, () => {
  const base = `http://localhost:${PORT}`;
  const faked = [...feeds.keys()];
  console.log(`kascov dev server on ${base}`);
  console.log(`  static   ${ROOT}`);
  console.log(`  worker   ${UPSTREAM} (proxied)`);
  console.log(`  mempool  synthetic: ${faked.length ? faked.join(', ') : 'none'}`);
  console.log(`           real (proxied): ${NETWORKS.filter((n) => !feeds.has(n)).join(', ') || 'none'}`);
  console.log('');
  console.log(`  explorer     ${base}/#/testnet-10/explore`);
  console.log(`  mainnet      ${base}/#/mainnet/explore   (real data)`);
  console.log(`  storm        ${base}/__mempool/storm`);
  console.log(`  one row      ${base}/__mempool/solo`);
  console.log(`  confirm all  ${base}/__mempool/calm`);
  console.log(`  empty        ${base}/__mempool/drain`);
  console.log(`  real mempool ${base}/__mempool/off`);
});
