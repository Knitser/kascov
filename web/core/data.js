/* kascov core/data — the fetch/load/cache layer between the worker's
   endpoints and the views: the paginated grid snapshot and its derived
   index, per-coin details, and every small per-network feed (templates,
   lanes, inscriptions, lifespans, families, activity, reorgs, digest,
   galaxy, lane pages, changelog). No DOM — everything here returns data
   and fills the caches in core/state; rendering stays in app.js. */

import { friendlyName } from './format.js';
import {
  GRID_PAGE, ACTIVITY_TTL_MS, ACTIVITY_MISS_TTL_MS,
  makeAnchor, daaToMs, state,
} from './state.js';

/* the wire says 'active'/'burned'; the UI speaks alive/retired — the one
   place that mapping happens (grid rows, detail coins, search results and
   address rows all carry the same status field) */
const isAlive = (c) => Boolean(c) && c.status === 'active';

/* Balance held right after each event daa. A spent UTXO is assumed
   consumed by the first covenant event after its creation (covenant
   outputs can only be spent by covenant events), so it counts toward
   balances in [creation event, next event). Reconciles with live_value
   at the last event for every covenant in current snapshots. */
function balancesByEventDaa(c) {
  const evDaas = [...new Set(c.events.map((e) => e.accepting_daa))].sort((a, b) => a - b);
  const out = new Map();
  for (const daa of evDaas) {
    let total = 0;
    for (const u of (c.utxos || [])) {
      if (u.created_daa > daa) continue;
      if (u.live) { total += u.value; continue; }
      const next = evDaas.find((x) => x > u.created_daa);
      if (next == null || daa < next) total += u.value;
    }
    out.set(daa, total);
  }
  return out;
}

/* Build a derived index so rendering stays cheap. The grid feed carries one
   summary row per covenant (no timelines, no scripts — those come from the
   per-coin detail endpoint), so this stays linear even at 40k+ coins. */

/* one index entry from a grid row (name collisions handled by the callers) */
function indexEntry(c, data, name) {
  /* transitions = events minus the birth (if seen) and the burn (if retired) */
  const moves = Math.max(0, c.event_count -
    (c.genesis_daa != null ? 1 : 0) -
    (isAlive(c) ? 0 : 1));
  const bornMs = c.genesis_daa != null ? daaToMs(c.genesis_daa, data)
    : (c.last_activity_daa ? daaToMs(c.last_activity_daa, data) : data.generated_at_ms);
  const lastMs = c.last_activity_daa ? daaToMs(c.last_activity_daa, data) : bornMs;
  const birthValue = c.born_value || 0;
  const blob = (name + ' ' + c.covenant_id).toLowerCase();
  return { c, name, moves, bornMs, lastMs, birthValue, blob };
}

function buildIndex(data) {
  /* friendly names can collide; count them so duplicates get a suffix */
  const nameCounts = new Map();
  for (const c of data.covenants) {
    const n = friendlyName(c.covenant_id);
    nameCounts.set(n, (nameCounts.get(n) || 0) + 1);
  }
  const covs = data.covenants.map((c) => {
    let name = friendlyName(c.covenant_id);
    if (nameCounts.get(name) > 1) name += `-${c.covenant_id.slice(0, 4)}`;
    return indexEntry(c, data, name);
  });
  covs.sort((a, b) => (b.c.last_activity_daa || 0) - (a.c.last_activity_daa || 0));
  const byId = new Map(covs.map((e) => [e.c.covenant_id, e]));
  /* nameCounts + generation persist so load-more can append incrementally
     (O(page), not a full O(n log n) rebuild) and renders can memoize.
     generation is globally unique — a rebuilt index (45s refresh) must never
     collide with the memo key of the index it replaces. */
  return { covs, byId, nameCounts, generation: ++indexSeq };
}
let indexSeq = 0;

/* Merge newly loaded (older) grid rows into an existing index in O(page).
   Cursor pages arrive in descending-activity order strictly below what's
   loaded, so appending preserves the sort; a cheap boundary check catches the
   rare out-of-order page (server refreshed mid-walk) and falls back to one
   sort. A name colliding with an already-indexed coin retro-suffixes the
   existing entry, matching what a full rebuild would have produced. */
function appendToIndex(index, data, newRows) {
  const covs = index.covs;
  for (const c of newRows) {
    const base = friendlyName(c.covenant_id);
    const count = (index.nameCounts.get(base) || 0) + 1;
    index.nameCounts.set(base, count);
    if (count === 2) {
      /* retro-suffix the existing holder of this name (rare; linear scan) */
      const prev = covs.find((e) => e.name === base);
      if (prev) {
        prev.name = base + `-${prev.c.covenant_id.slice(0, 4)}`;
        prev.blob = (prev.name + ' ' + prev.c.covenant_id).toLowerCase();
      }
    }
    const name = count > 1 ? base + `-${c.covenant_id.slice(0, 4)}` : base;
    const entry = indexEntry(c, data, name);
    covs.push(entry);
    index.byId.set(c.covenant_id, entry);
  }
  /* boundary guard: if the first appended row is newer than what precedes it,
     the pages interleaved — restore order with one sort */
  const at = covs.length - newRows.length;
  if (at > 0 && newRows.length &&
      (covs[at].c.last_activity_daa || 0) > (covs[at - 1].c.last_activity_daa || 0)) {
    covs.sort((a, b) => (b.c.last_activity_daa || 0) - (a.c.last_activity_daa || 0));
  }
  index.generation = ++indexSeq; /* same global sequence as buildIndex — no key collisions */
  return index;
}

/* Fetch one grid page. The compound cursor `afterDaa`+`afterId` and `limit` are
   folded into the query only when present so a plain `data/{net}.json` still
   works against older workers. */
async function fetchGridPage(network, afterDaa, afterId, limit) {
  const qs = new URLSearchParams();
  if (afterDaa != null) qs.set('after_daa', String(afterDaa));
  if (afterId != null) qs.set('after_id', String(afterId));
  if (limit != null) qs.set('limit', String(limit));
  const suffix = qs.toString();
  const res = await fetch(`data/${network}.json${suffix ? `?${suffix}` : ''}`, { cache: 'no-cache' });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.json();
}

async function loadNetwork(network) {
  if (state.cache[network]) return state.cache[network];
  /* one silent retry: a single dropped request must not paint the
     "couldn't load" card — that card should mean the worker is actually
     unreachable, not that one packet died */
  let data;
  try {
    data = await fetchGridPage(network, null, null, GRID_PAGE);
  } catch (_) {
    await new Promise((r) => setTimeout(r, 1200));
    data = await fetchGridPage(network, null, null, GRID_PAGE);
  }
  data.__anchor = makeAnchor(data, network);
  /* a cursor means the worker paginated and older rows remain; its absence
     means we already hold the full snapshot (older worker or a small net) */
  const cursor = data.next_after_daa != null ? data.next_after_daa : null;
  const entry = {
    data, index: buildIndex(data), nextAfterDaa: cursor,
    nextAfterId: data.next_after_id != null ? data.next_after_id : null,
    loadingMore: false,
  };
  state.cache[network] = entry;
  return entry;
}

/* Pull the next older grid page and merge it into the cached entry: append the
   fresh rows, advance the cursor, and rebuild the derived index so search,
   sort, filter and the watch strip all see the newly loaded coins. Returns the
   entry unchanged (no-op) when there is nothing more to load or a fetch is
   already in flight. */
async function loadMoreGrid(network) {
  const entry = state.cache[network];
  if (!entry || entry.nextAfterDaa == null || entry.loadingMore) return entry;
  entry.loadingMore = true;
  try {
    const page = await fetchGridPage(network, entry.nextAfterDaa, entry.nextAfterId, GRID_PAGE);
    const rows = Array.isArray(page.covenants) ? page.covenants : [];
    /* dedup against the index (byId covers everything loaded), then merge the
       genuinely-new rows incrementally — O(page), not a full rebuild */
    const fresh = rows.filter((c) => !entry.index.byId.has(c.covenant_id));
    entry.data.covenants.push(...fresh);
    entry.nextAfterDaa = page.next_after_daa != null ? page.next_after_daa : null;
    entry.nextAfterId = page.next_after_id != null ? page.next_after_id : null;
    appendToIndex(entry.index, entry.data, fresh);
  } finally {
    entry.loadingMore = false;
  }
  return entry;
}

/* One coin's full story, fetched on demand and merged over its grid row so
   the detail renderer sees the same shape the all-in-one snapshot used to
   provide. */
async function loadDetail(network, covId) {
  const map = state.details[network] || (state.details[network] = new Map());
  const hit = map.get(covId);
  if (hit) return hit;
  const res = await fetch(`data/${network}/c/${covId}.json`, { cache: 'no-cache' });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const detail = await res.json();
  const entry = state.cache[network];
  const gridRec = entry && entry.index.byId.get(covId);
  const c = Object.assign({}, gridRec ? gridRec.c : {}, detail);
  const data = entry ? entry.data : detail; /* anchor for daa→ms */
  const name = gridRec ? gridRec.name : friendlyName(covId);
  const moves = c.events.filter((e) => e.kind === 'transition').length;
  const firstEvent = c.events[0] || null;
  const lastEvent = c.events[c.events.length - 1] || null;
  const bornMs = c.genesis_daa != null ? daaToMs(c.genesis_daa, data)
    : (firstEvent ? daaToMs(firstEvent.accepting_daa, data) : data.generated_at_ms);
  const lastMs = lastEvent ? daaToMs(lastEvent.accepting_daa, data) : bornMs;
  let birthValue = c.born_value || 0;
  if (c.genesis_daa != null && Array.isArray(c.utxos)) {
    let v = 0;
    for (const u of c.utxos) if (u.created_daa === c.genesis_daa) v += u.value;
    if (v > 0) birthValue = v;
  }
  const rec = { c, name, moves, bornMs, lastMs, birthValue, balances: balancesByEventDaa(c) };
  map.set(covId, rec);
  return rec;
}

/* which smart coins has this address/pubkey touched — fetched on demand */
async function loadAddress(network, q) {
  const map = state.addrs[network] || (state.addrs[network] = new Map());
  const hit = map.get(q);
  if (hit) return hit;
  const res = await fetch(`data/${network}/addr/${encodeURIComponent(q)}.json`, { cache: 'no-cache' });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const data = await res.json();
  map.set(q, data);
  return data;
}

/* The tiny live feed, for instant first paint while the full snapshot
   (multi-MB on a busy testnet) is still downloading. */
async function loadLite(network) {
  const ls = state.live[network] || (state.live[network] = { supported: null, missedAt: 0, data: null });
  if (ls.data) return ls.data;
  try {
    const res = await fetch(`data/${network}-live.json`, { cache: 'no-cache' });
    if (!res.ok) {
      if (res.status === 404) {
        ls.supported = false;
        ls.missedAt = Date.now();
      }
      return null;
    }
    ls.data = await res.json();
    ls.supported = true;
    return ls.data;
  } catch (e) {
    return null;
  }
}

/* Contract-type analytics — what runs on this network, by recognized script
   template. Cached per network for a minute (mirrors the server ttl); a 404
   from an older worker is remembered (data: null) and reprobed after the
   ttl, so the section hides instead of pinning a dead panel. */
const TEMPLATES_TTL_MS = 60_000;
/* the pending feed is real-time — a short TTL so the seed snapshot is fresh,
   though the SSE stream is what actually keeps it live between polls. */
const PENDING_TTL_MS = 5_000;

async function loadTemplates(network) {
  const t = state.templates[network];
  if (t && Date.now() - t.at < TEMPLATES_TTL_MS) return t.data;
  try {
    const res = await fetch(`data/${network}/templates.json`, { cache: 'no-cache' });
    if (!res.ok) {
      state.templates[network] = { data: null, at: Date.now() };
      return null;
    }
    const data = await res.json();
    state.templates[network] = { data, at: Date.now() };
    return data;
  } catch (e) {
    return t ? t.data : null;
  }
}

async function loadLanes(network) {
  const t = state.lanes[network];
  if (t && Date.now() - t.at < TEMPLATES_TTL_MS) return t.data;
  try {
    const res = await fetch(`data/${network}/lanes.json`, { cache: 'no-cache' });
    if (!res.ok) { state.lanes[network] = { data: null, at: Date.now() }; return null; }
    const data = await res.json();
    state.lanes[network] = { data, at: Date.now() };
    return data;
  } catch (e) {
    return t ? t.data : null;
  }
}

/* Live pending (mempool) covenant snapshot. Short TTL — this is a real-time
   feed. A 404 (poller disabled for a no-mempool node, or an old worker) caches
   a null so renderPending feature-hides the section instead of retrying hard. */
async function loadPending(network) {
  const t = state.pending[network];
  if (t && Date.now() - t.at < PENDING_TTL_MS) return t.data;
  try {
    const res = await fetch(`data/${network}/pending`, { cache: 'no-cache' });
    if (!res.ok) { state.pending[network] = { data: null, at: Date.now() }; return null; }
    const data = await res.json();
    state.pending[network] = { data, at: Date.now() };
    return data;
  } catch (e) {
    return t ? t.data : null;
  }
}

async function loadInscriptions(network) {
  const t = state.inscriptions[network];
  if (t && Date.now() - t.at < TEMPLATES_TTL_MS) return t.data;
  try {
    const res = await fetch(`data/${network}/inscriptions.json`, { cache: 'no-cache' });
    if (!res.ok) { state.inscriptions[network] = { data: null, at: Date.now() }; return null; }
    const data = await res.json();
    state.inscriptions[network] = { data, at: Date.now() };
    return data;
  } catch (e) {
    return t ? t.data : null;
  }
}

async function loadLifespans(network) {
  const t = state.lifespans[network];
  if (t && Date.now() - t.at < TEMPLATES_TTL_MS) return t.data;
  try {
    const res = await fetch(`data/${network}/lifespans.json`, { cache: 'no-cache' });
    if (!res.ok) { state.lifespans[network] = { data: null, at: Date.now() }; return null; }
    const data = await res.json();
    state.lifespans[network] = { data, at: Date.now() };
    return data;
  } catch (e) {
    return t ? t.data : null;
  }
}

async function loadFamilies(network) {
  const t = state.families[network];
  if (t && Date.now() - t.at < TEMPLATES_TTL_MS) return t.data;
  try {
    const res = await fetch(`data/${network}/families.json`, { cache: 'no-cache' });
    if (!res.ok) { state.families[network] = { data: null, at: Date.now() }; return null; }
    const data = await res.json();
    state.families[network] = { data, at: Date.now() };
    return data;
  } catch (e) {
    return t ? t.data : null;
  }
}

/* the activity histogram behind the pulse chart — per (network, range)
   cache mirroring the server ttl; a 404 from an older worker is remembered
   (data: null) and reprobed after ACTIVITY_MISS_TTL_MS */
async function loadActivity(network, range) {
  const byRange = state.activity[network] || (state.activity[network] = {});
  const hit = byRange[range];
  const ttl = hit && hit.data === null ? ACTIVITY_MISS_TTL_MS : ACTIVITY_TTL_MS;
  if (hit && Date.now() - hit.at < ttl) return hit.data;
  try {
    const res = await fetch(`data/${network}/activity.json?range=${range}`, { cache: 'no-cache' });
    if (!res.ok) {
      if (res.status === 404) byRange[range] = { data: null, at: Date.now() };
      return hit ? hit.data : null;
    }
    const data = await res.json();
    byRange[range] = { data, at: Date.now() };
    return data;
  } catch (e) {
    return hit ? hit.data : null;   /* stale-ok; fallback only on a real 404 */
  }
}

/* the reorg log — the append-only ledger of virtual-chain reorgs the indexer
   applied. Optional: an older worker without the endpoint 404s, which we
   remember (data: null) and reprobe after the ttl, so the panel hides
   instead of pinning a dead request. */
async function loadReorgs(network) {
  const t = state.reorgs[network];
  if (t && Date.now() - t.at < TEMPLATES_TTL_MS) return t.data;
  try {
    const res = await fetch(`data/${network}/reorgs.json`, { cache: 'no-cache' });
    if (!res.ok) { state.reorgs[network] = { data: null, at: Date.now() }; return null; }
    const data = await res.json();
    state.reorgs[network] = { data, at: Date.now() };
    return data;
  } catch (e) {
    return t ? t.data : null;
  }
}

/* "the last 24 hours" — one tiny object per network, cached for a minute
   (mirrors the server ttl). A failed or 404 fetch marks it missing and
   returns whatever we already had, so the strip degrades to hidden instead
   of flashing errors under the hero. */
const DIGEST_TTL_MS = 60_000;

async function loadDigest(network) {
  const ds = state.digest[network] ||
    (state.digest[network] = { data: null, at: 0, animated: false });
  if (Date.now() - ds.at < DIGEST_TTL_MS) return ds.data;
  try {
    const res = await fetch(`data/${network}/digest.json`, { cache: 'no-cache' });
    if (!res.ok) {
      /* misses respect the TTL too — an old worker without the endpoint
         shouldn't be re-asked on every landing render */
      ds.at = Date.now();
      return ds.data; /* stale-ok */
    }
    ds.data = await res.json();
    ds.at = Date.now();
    return ds.data;
  } catch (e) {
    return ds.data;
  }
}

/* the galaxy payload — positions + weighted edges precomputed by the worker.
   First paint asks for the compact columnar core tier (?fmt=2&tier=core:
   big clusters only, parallel arrays). An older worker ignores the params
   and answers with the full legacy shape — galaxy.js feature-detects
   either. app.js upgrades a core-tier reply to the full set in place. */
const galaxyCache = {};        // network -> { data, at }

async function loadGalaxy(network) {
  const t = galaxyCache[network];
  if (t && Date.now() - t.at < TEMPLATES_TTL_MS) return t.data;
  try {
    const res = await fetch(`data/${network}/galaxy.json?fmt=2&tier=core`, { cache: 'no-cache' });
    if (!res.ok) { galaxyCache[network] = { data: null, at: Date.now() }; return null; }
    const data = await res.json();
    galaxyCache[network] = { data, at: Date.now() };
    return data;
  } catch (e) {
    return t ? t.data : null;
  }
}

/* one KIP-21 lane namespace's dashboard — served by its own worker endpoint
   (feature-detected: an older worker / static hosting 404s → graceful note) */
const LANE_PAGE_TTL_MS = 60_000;
const lanePages = new Map(); // `${network}/${ns}` -> { data|missing, at }

async function loadLanePage(network, ns) {
  const key = `${network}/${ns}`;
  const t = lanePages.get(key);
  if (t && Date.now() - t.at < LANE_PAGE_TTL_MS) return t;
  const res = await fetch(`data/${network}/lane/${ns}`, { cache: 'no-cache' });
  if (res.status === 404 || res.status === 400) {
    /* a worker that serves lanes always answers a well-formed namespace with
       200 (even for an empty lane) — a 404 means the route doesn't exist */
    const rec = { missing: true, at: Date.now() };
    lanePages.set(key, rec);
    return rec;
  }
  if (!res.ok) throw new Error(`lane ${res.status}`);
  const rec = { data: await res.json(), at: Date.now() };
  lanePages.set(key, rec);
  return rec;
}

/* the decoded covenant-token directory (KCC20 and friends) — served by its
   own worker endpoint. Feature-detected like lane pages: a 404 means an
   older worker without the route (remembered as missing, reprobed after the
   ttl) so the view can say "needs a newer worker" honestly. */
const TOKENS_TTL_MS = 60_000;
const tokenPages = new Map(); // network -> { data|missing, at }

async function loadTokens(network) {
  const t = tokenPages.get(network);
  if (t && Date.now() - t.at < TOKENS_TTL_MS) return t;
  const res = await fetch(`data/${network}/tokens.json`, { cache: 'no-cache' });
  if (res.status === 404) {
    const rec = { missing: true, at: Date.now() };
    tokenPages.set(network, rec);
    return rec;
  }
  if (!res.ok) throw new Error(`tokens ${res.status}`);
  const rec = { data: await res.json(), at: Date.now() };
  tokenPages.set(network, rec);
  return rec;
}

/* one decoded token's page — its directory row, top balances, classified
   mint/transfer/burn events and the validation verdict, served by its own
   worker endpoint. Feature-detected like the directory: a 404 (older worker
   without the route, or an id it doesn't know as a token) is remembered as
   missing and reprobed after the ttl. */
const tokenDetails = new Map(); // `${network}/${id}` -> { data|missing, at }

async function loadTokenDetail(network, id) {
  const key = `${network}/${id}`;
  const t = tokenDetails.get(key);
  if (t && Date.now() - t.at < TOKENS_TTL_MS) return t;
  const res = await fetch(`data/${network}/token/${id}`, { cache: 'no-cache' });
  if (res.status === 404) {
    const rec = { missing: true, at: Date.now() };
    tokenDetails.set(key, rec);
    return rec;
  }
  if (!res.ok) throw new Error(`token ${res.status}`);
  const rec = { data: await res.json(), at: Date.now() };
  tokenDetails.set(key, rec);
  return rec;
}

/* one transaction's covenant footprint — the same route the search resolver
   asks. Newer workers enrich it additively (events, created/spent cells,
   token actions); older workers answer only { txid?, covenant_id,
   covenant_ids? } and the page feature-detects. A 404 means kascov never saw
   this tx touch a smart coin — remembered and reprobed after the ttl, since
   a fresh tx can confirm late. */
const txDetails = new Map(); // `${network}/${txid}` -> { data|missing, at }

async function loadTxDetail(network, txid) {
  const key = `${network}/${txid}`;
  const t = txDetails.get(key);
  if (t && Date.now() - t.at < TOKENS_TTL_MS) return t;
  const res = await fetch(`data/${network}/tx/${txid}.json`, { cache: 'no-cache' });
  if (res.status === 404) {
    const rec = { missing: true, at: Date.now() };
    txDetails.set(key, rec);
    return rec;
  }
  if (!res.ok) throw new Error(`tx ${res.status}`);
  const rec = { data: await res.json(), at: Date.now() };
  txDetails.set(key, rec);
  return rec;
}

/* web/changelog.json — a static file shipped with the frontend: an array of
   { date, title, body }, newest first. Cached for the session; a missing
   file (older deploy) hides everything that depends on it. */
let changelogCache;
async function loadChangelog() {
  if (changelogCache !== undefined) return changelogCache;
  try {
    const res = await fetch('changelog.json', { cache: 'no-cache' });
    const data = res.ok ? await res.json() : null;
    changelogCache = Array.isArray(data) && data.length ? data : null;
  } catch (e) {
    changelogCache = null;
  }
  return changelogCache;
}

/* web/community.json — curated "built with covenants" showcase entries,
   committed with the frontend: an array of { name, by, blurb,
   links:{site?, repo?, example?}, date }, newest first. Same graceful-null
   contract as the changelog: a missing or empty file (older deploy) hides
   the landing section entirely. */
/* curated launchpad registry: identifies a token's launch platform from the
   image host its genesis payload committed — an on-chain fact, not an API
   handshake. missing/empty file simply means no trade buttons. */
let launchpadsCache;
async function loadLaunchpads() {
  if (launchpadsCache !== undefined) return launchpadsCache;
  try {
    const res = await fetch('launchpads.json', { cache: 'no-cache' });
    const data = res.ok ? await res.json() : null;
    launchpadsCache = Array.isArray(data) && data.length ? data : null;
  } catch (e) {
    launchpadsCache = null;
  }
  return launchpadsCache;
}

let communityCache;
async function loadCommunity() {
  if (communityCache !== undefined) return communityCache;
  try {
    const res = await fetch('community.json', { cache: 'no-cache' });
    const data = res.ok ? await res.json() : null;
    communityCache = Array.isArray(data) && data.length ? data : null;
  } catch (e) {
    communityCache = null;
  }
  return communityCache;
}

export {
  isAlive,
  buildIndex, fetchGridPage, loadNetwork, loadMoreGrid,
  loadDetail, loadAddress, loadLite,
  loadTemplates, loadLanes, loadPending, loadInscriptions, loadLifespans, loadFamilies,
  loadActivity, loadReorgs, loadDigest,
  galaxyCache, loadGalaxy,
  LANE_PAGE_TTL_MS, lanePages, loadLanePage,
  TOKENS_TTL_MS, tokenPages, loadTokens,
  tokenDetails, loadTokenDetail,
  txDetails, loadTxDetail,
  loadChangelog,
  loadCommunity,
  loadLaunchpads,
};
