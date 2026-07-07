/* kascov — a camera pointed at Kaspa's smart coins.
   Pure vanilla JS, no dependencies, no build step. */
(() => {
'use strict';

/* ---------------------------------------------------------------- config */

const NETWORKS = {
  'testnet-10': {
    label: 'testnet-10',
    word: 'testnet',
    unit: 'TKAS',
    unitHint: 'TKAS = test-network KAS (play money, no real-world value)',
    txBase: 'https://tn10.kaspa.stream/transactions/',
    pulseTitle: 'life on the testnet',
  },
  'mainnet': {
    label: 'mainnet',
    word: 'mainnet',
    unit: 'KAS',
    txBase: 'https://explorer.kaspa.org/transactions/',
    pulseTitle: 'life on mainnet',
  },
};

const MS_PER_DAA = 100;   // the chain ticks ~10 DAA per second
const PAGE_SIZE = 60;
const STORY_COUNT = 15;
const TEASER_COUNT = 3;
const PULSE_BUCKETS = 12;
const ACTIVITY_RANGES = ['1h', '6h', '24h', '48h', 'all'];
const ACTIVITY_LABELS = { '1h': '1h', '6h': '6h', '24h': '24h', '48h': '2d', 'all': 'all' };
const ACTIVITY_PHRASE = { '1h': 'in the last hour', '6h': 'in the last 6 hours', '24h': 'in the last 24 hours', '48h': 'in the last 2 days' };
const ACTIVITY_TTL_MS = 30_000;          // mirrors the server ttl
const ACTIVITY_MISS_TTL_MS = 5 * 60_000; // 404 (old worker) reprobe, mirrors LIVE_REPROBE_MS
const ACTIVITY_MAX_COLS = 200;           // defensive DOM cap
const ADDR_RE = /^(kaspa|kaspatest):[a-z0-9]{20,}$/i;
const PUBKEY_RE = /^[0-9a-fA-F]{64}(?:[0-9a-fA-F]{2})?$/; // 32B x-only or 33B ECDSA

/* ------------------------------------------------------- friendly names */

const ADJECTIVES = [
  'brave','quick','silent','gentle','bold','clever','curious','dizzy',
  'eager','fierce','glad','happy','humble','jolly','keen','lively',
  'lucky','mellow','nimble','noble','patient','playful','proud','quiet',
  'rapid','restless','shy','sleepy','sly','snappy','steady','stubborn',
  'sunny','swift','tidy','tiny','vivid','wandering','wise','zesty',
];
const COLORS = [
  'teal','amber','coral','indigo','jade','crimson',
  'cobalt','olive','violet','copper','pearl','slate',
];
const ANIMALS = [
  'otter','lynx','crane','fox','owl','badger','heron','marmot',
  'falcon','tortoise','hare','raven','seal','ibis','moth','newt',
  'panda','quail','robin','stoat','tapir','urchin','vole','wren',
  'yak','zebra','gecko','dolphin','ferret','magpie','hedgehog','jackal',
  'kiwi','lemur','mole','narwhal','osprey','puffin','squid','toad',
];

function idByte(id, i) {
  const v = parseInt(id.slice(i * 2, i * 2 + 2), 16);
  return Number.isNaN(v) ? 0 : v;
}

function friendlyName(id) {
  const adj = ADJECTIVES[(idByte(id, 0) * 256 + idByte(id, 1)) % ADJECTIVES.length];
  const col = COLORS[(idByte(id, 2) * 256 + idByte(id, 3)) % COLORS.length];
  const ani = ANIMALS[(idByte(id, 4) * 256 + idByte(id, 5)) % ANIMALS.length];
  return `${adj}-${col}-${ani}`;
}

/* --------------------------------------------------------------- avatar */

function avatarSvg(id, size) {
  const b = (i) => idByte(id, i);
  const hue = (b(6) * 256 + b(7)) % 360;
  const hue2 = (hue + 60 + (b(8) % 150)) % 360;
  const bg = `hsl(${hue} 45% 17%)`;
  const ring = `hsl(${hue} 50% 42%)`;
  const count = 2 + (b(9) % 2);
  let shapes = '';
  for (let k = 0; k < count; k++) {
    const o = 10 + k * 5;                       // bytes 10.. / 15.. / 20..
    const kind = b(o) % 5;
    const ang = (b(o + 1) / 255) * Math.PI * 2;
    const dist = 3 + (b(o + 2) % 11);           // 3..13 from center
    const cx = +(32 + Math.cos(ang) * dist).toFixed(1);
    const cy = +(32 + Math.sin(ang) * dist).toFixed(1);
    const s = 8 + (b(o + 3) % 8);               // 8..15
    const rot = b(o + 4) % 90;
    const col = `hsl(${(hue2 + k * 47) % 360} ${60 + (b(o) % 25)}% ${58 + (b(o + 1) % 16)}%)`;
    if (kind === 0) {
      shapes += `<circle cx="${cx}" cy="${cy}" r="${(s * 0.72).toFixed(1)}" fill="${col}" opacity="0.92"/>`;
    } else if (kind === 1) {
      shapes += `<rect x="${(cx - s / 2).toFixed(1)}" y="${(cy - s / 2).toFixed(1)}" width="${s}" height="${s}" rx="2" fill="${col}" opacity="0.92" transform="rotate(${rot} ${cx} ${cy})"/>`;
    } else if (kind === 2) {
      const h = s * 0.9;
      const p1 = `${cx},${(cy - h).toFixed(1)}`;
      const p2 = `${(cx - h * 0.87).toFixed(1)},${(cy + h * 0.5).toFixed(1)}`;
      const p3 = `${(cx + h * 0.87).toFixed(1)},${(cy + h * 0.5).toFixed(1)}`;
      shapes += `<polygon points="${p1} ${p2} ${p3}" fill="${col}" opacity="0.92" transform="rotate(${rot} ${cx} ${cy})"/>`;
    } else if (kind === 3) {
      shapes += `<circle cx="${cx}" cy="${cy}" r="${(s * 0.65).toFixed(1)}" fill="none" stroke="${col}" stroke-width="3.5" opacity="0.92"/>`;
    } else {
      shapes += `<rect x="${(cx - s / 2).toFixed(1)}" y="${(cy - s / 2).toFixed(1)}" width="${s}" height="${s}" rx="2" fill="${col}" opacity="0.92" transform="rotate(45 ${cx} ${cy})"/>`;
    }
  }
  return `<svg class="avatar" viewBox="0 0 64 64" width="${size}" height="${size}" aria-hidden="true" focusable="false">` +
    `<circle cx="32" cy="32" r="30" fill="${bg}" stroke="${ring}" stroke-width="2.5"/>${shapes}</svg>`;
}

/* ----------------------------------------------------------------- icons */

const ICONS = {
  born: '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">' +
    '<path d="M12 21v-8"/><path d="M12 13C12 9.2 9.3 6.6 5 6.2c.4 4.4 3 7 7 6.8z"/><path d="M12 11c0-3 2-5.2 6-5.6-.3 3.4-2.4 5.4-6 5.6z"/></svg>',
  move: '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">' +
    '<path d="M20.5 3.5v5h-5"/><path d="M3.7 10a8.5 8.5 0 0 1 14.2-4l2.6 2.5"/><path d="M3.5 20.5v-5h5"/><path d="M20.3 14a8.5 8.5 0 0 1-14.2 4l-2.6-2.5"/></svg>',
  burn: '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">' +
    '<path d="M12 2.5c.6 3 2.2 4.7 4 6.5 1.8 1.8 3 3.6 3 6a7 7 0 0 1-14 0c0-1.8.7-3.4 1.8-4.6.4 1.1 1 1.9 2 2.4C8.2 9.4 9.6 5.5 12 2.5z"/><path d="M12 21.5a3.2 3.2 0 0 1-3.2-3.2c0-1.5 1.2-2.6 2-3.6.5-.6.9-1.3 1.2-2 1.3 1.6 3.2 3.4 3.2 5.6a3.2 3.2 0 0 1-3.2 3.2z"/></svg>',
};

const KIND_META = {
  genesis:    { icon: 'born', cls: 'kind-born' },
  transition: { icon: 'move', cls: 'kind-move' },
  burn:       { icon: 'burn', cls: 'kind-burn' },
};

/* plain-words explanations, surfaced as hover/long-press titles wherever
   jargon appears — the site should never assume the reader knows KIPs */
const GLOSSARY = {
  alive: 'this smart coin still has live (unspent) state on the network — its story can continue',
  retired: 'every piece of this coin\u2019s state has been spent without continuing the covenant — its story ended (recorded here forever)',
  genesis: 'born: a transaction created this coin\u2019s permanent identity (its KIP-20 covenant id)',
  transition: 'moved: the coin\u2019s state was spent and continued under the same identity — same coin, new state',
  burn: 'retired: the state was spent without continuing the covenant — the value left, the identity ended',
  'p2pk state': 'the simplest state shape: a public key + OpCheckSig — whoever holds the key controls this piece of state',
  'p2sh commitment': 'a 35-byte hash commitment (OpBlake2b <hash> OpEqual) — the actual program stays hidden until the coin is spent, then kascov captures and verifies it',
  live_states: 'state pieces (UTXOs) with this shape that are unspent right now',
  ever_seen: 'every state piece with this shape kascov has ever indexed, spent or not',
  ran_at_spend: 'revealed programs: hidden P2SH programs this shape, seen and hash-verified when their coins were spent',
  lineage: 'whether kascov saw this coin\u2019s whole life — \u201cno\u201d means it was born before we started watching, so earlier history is honestly missing',
  digest_born: 'new smart coins created in the last 24 hours',
  digest_moved: 'state transitions in the last 24 hours — coins changing hands or updating state',
  digest_retired: 'coins whose story ended in the last 24 hours',
};

/* ----------------------------------------------------------------- utils */

const $ = (sel) => document.querySelector(sel);
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => (
  { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]
));

function ordinal(n) {
  const s = ['th', 'st', 'nd', 'rd'];
  const v = n % 100;
  return n + (s[(v - 20) % 10] || s[v] || s[0]);
}

function fmtInt(n) { return Number(n).toLocaleString('en-US'); }

function fmtAmount(sompi, network) {
  const kas = sompi / 1e8;
  const unit = NETWORKS[network].unit;
  let str;
  if (kas >= 1000) str = kas.toLocaleString('en-US', { maximumFractionDigits: 0 });
  else if (kas >= 1) str = kas.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  else if (kas === 0) str = '0';
  else str = kas.toLocaleString('en-US', { maximumFractionDigits: 4 });
  return `${str} ${unit}`;
}

/* DAA → estimated wall-clock ms (chain ticks ~10 DAA/sec).

   Preferred anchor is the chain tip at export time (data.tip_daa) — exact.
   Older snapshots lack it and only give the DAA of the newest recorded
   event, which may be much older than the export moment. Anchoring on it
   directly would re-date the whole history to "just now" on every export,
   so we pin the first wall-clock time we ever saw for a given
   last-activity DAA (per network, in localStorage) and reuse it until the
   network actually moves again. */
function makeAnchor(data, network) {
  if (data.tip_daa != null) {
    /* tip_at_ms is when the indexer saw that tip — the precise pairing.
       generated_at_ms (build time) is a close second for older exports. */
    const ms = data.tip_at_ms != null ? data.tip_at_ms : data.generated_at_ms;
    return { daa: data.tip_daa, ms, exact: true };
  }
  const daa = data.stats.last_activity_daa;
  let ms = data.generated_at_ms;
  try {
    const key = `kascov-anchor-${network}`;
    const prev = JSON.parse(localStorage.getItem(key) || 'null');
    if (prev && prev.daa === daa && prev.ms < ms) ms = prev.ms;
    localStorage.setItem(key, JSON.stringify({ daa, ms }));
  } catch (e) { /* private mode */ }
  return { daa, ms, exact: false };
}

function daaToMs(daa, data) {
  const a = data.__anchor;
  return a.ms - (a.daa - daa) * MS_PER_DAA;
}

function relTime(ms) {
  const diff = Date.now() - ms;
  if (diff < 0) return 'just now';
  const sec = diff / 1000;
  if (sec < 45) return 'just now';
  if (sec < 90) return 'about a minute ago';
  const min = sec / 60;
  if (min < 45) return `about ${Math.round(min)} minutes ago`;
  if (min < 90) return 'about an hour ago';
  const hr = min / 60;
  if (hr < 22) return `about ${Math.round(hr)} hours ago`;
  const day = hr / 24;
  if (day < 1.5) return 'yesterday';
  if (day < 26) return `${Math.round(day)} days ago`;
  const mon = day / 30.4;
  if (mon < 11.5) return `${Math.round(mon)} months ago`;
  return `${Math.round(day / 365)} years ago`;
}

function relTimeShort(ms) {
  return relTime(ms)
    .replace('about ', '')
    .replace('a minute ago', '1m ago')
    .replace(/(\d+) minutes ago/, '$1m ago')
    .replace('an hour ago', '1h ago')
    .replace(/(\d+) hours ago/, '$1h ago')
    .replace('yesterday', '1d ago')
    .replace(/(\d+) days ago/, '$1d ago');
}

function fmtClock(ms, withSeconds, withDate) {
  const d = new Date(ms);
  const opt = { hour: '2-digit', minute: '2-digit', hour12: false };
  if (withSeconds) opt.second = '2-digit';
  const time = d.toLocaleTimeString([], opt);
  if (!withDate) return time;
  return `${d.toLocaleDateString([], { month: 'short', day: 'numeric' })} ${time}`;
}

function fmtSpan(ms) {
  const min = ms / 60000;
  if (min < 2) return 'about a minute';
  if (min < 90) return `${Math.round(min)} minutes`;
  const hr = min / 60;
  if (hr < 36) return `${Math.round(hr)} hour${Math.round(hr) === 1 ? '' : 's'}`;
  const day = Math.round(hr / 24);
  return `${day} day${day === 1 ? '' : 's'}`;
}

function shortHex(hex, head, tail) {
  if (!hex || hex.length <= head + tail + 1) return hex || '';
  return `${hex.slice(0, head)}…${hex.slice(-tail)}`;
}

/* exact UTC stamp for tooltips on relative times */
function utcTitle(ms) {
  return new Date(ms).toISOString().replace('T', ' ').replace(/\.\d+Z$/, ' UTC');
}

/* compact absolute time, always UTC: "Jul 5, 14:32 UTC" (year only when
   it isn't this year) — shown inline so nobody has to hover, mobile included */
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
function absShort(ms) {
  const d = new Date(ms);
  const year = d.getUTCFullYear() === new Date().getUTCFullYear() ? '' : ` ${d.getUTCFullYear()}`;
  const hh = String(d.getUTCHours()).padStart(2, '0');
  const mm = String(d.getUTCMinutes()).padStart(2, '0');
  return `${MONTHS[d.getUTCMonth()]} ${d.getUTCDate()}${year}, ${hh}:${mm} UTC`;
}

function txUrl(network, txid) {
  return NETWORKS[network].txBase + txid;
}

/* ----------------------------------------------------------------- state */

const state = {
  network: 'testnet-10',
  cache: {},          // network -> { data, index }
  filter: 'all',
  query: '',
  shown: PAGE_SIZE,
  nerd: false,
  sort: 'activity',
  live: {},           // network -> { supported, missedAt, data }
  details: {},        // network -> Map(covenant id -> merged detail entry)
  txLookup: {},       // 64-hex query -> 'pending' | 'miss' (server tx resolver)
  addrs: {},          // network -> Map(addressOrPubkey query -> /addr response)
  templates: {},      // network -> { data, at } (contract-type analytics)
  families: {},       // network -> { data, at } (multi-contract apps)
  lanes: {},          // network -> { data, at } (based-app namespaces)
  inscriptions: {},   // network -> { data, at } (decoded JSON inscriptions)
  lifespans: {},      // network -> { data, at } (retired-coin lifespans)
  digest: {},         // network -> { data, at, animated }
  activity: {},       // network -> { [range]: { data, at } } (data null = 404 miss)
  pulseRange: '24h',
  watch: new Set(),   // covenant ids starred on the current network
  watchNet: null,
};

try { state.nerd = localStorage.getItem('kascov-nerd') === '1'; } catch (e) { /* private mode */ }
try {
  const r = localStorage.getItem('kascov-pulse-range');
  if (ACTIVITY_RANGES.includes(r)) state.pulseRange = r;
} catch (e) { /* private mode */ }

/* ------------------------------------------------------------- watchlist */

function watchKey(network) { return `kascov-watch-${network}`; }

function loadWatch(network) {
  try { return new Set(JSON.parse(localStorage.getItem(watchKey(network)) || '[]')); }
  catch (e) { return new Set(); }
}

function saveWatch(network, set) {
  try { localStorage.setItem(watchKey(network), JSON.stringify([...set])); }
  catch (e) { /* private mode */ }
}

state.watch = loadWatch(state.network);
state.watchNet = state.network;

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
    /* transitions = events minus the birth (if seen) and the burn (if retired) */
    const moves = Math.max(0, c.event_count -
      (c.genesis_daa != null ? 1 : 0) -
      (c.status !== 'active' ? 1 : 0));
    const bornMs = c.genesis_daa != null ? daaToMs(c.genesis_daa, data)
      : (c.last_activity_daa ? daaToMs(c.last_activity_daa, data) : data.generated_at_ms);
    const lastMs = c.last_activity_daa ? daaToMs(c.last_activity_daa, data) : bornMs;
    const birthValue = c.born_value || 0;
    const blob = (name + ' ' + c.covenant_id).toLowerCase();
    return { c, name, moves, bornMs, lastMs, birthValue, blob };
  });
  covs.sort((a, b) => (b.c.last_activity_daa || 0) - (a.c.last_activity_daa || 0));
  const byId = new Map(covs.map((e) => [e.c.covenant_id, e]));
  return { covs, byId };
}

async function loadNetwork(network) {
  if (state.cache[network]) return state.cache[network];
  const res = await fetch(`data/${network}.json`, { cache: 'no-cache' });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const data = await res.json();
  data.__anchor = makeAnchor(data, network);
  const entry = { data, index: buildIndex(data) };
  state.cache[network] = entry;
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

/* a namespace is 4 bytes — show its ASCII when printable ("KASP"), else hex */
function nsLabel(hex) {
  const bytes = hex.match(/../g) || [];
  const printable = bytes.length > 0 && bytes.every((b) => { const c = parseInt(b, 16); return c >= 0x20 && c <= 0x7e; });
  if (printable) {
    const ascii = bytes.map((b) => String.fromCharCode(parseInt(b, 16))).join('');
    return `<span class="ns-ascii">${esc(ascii)}</span> <span class="dim mono">0x${esc(hex)}</span>`;
  }
  return `<span class="mono">0x${esc(hex)}</span>`;
}

function renderLanes(network) {
  const section = $('#section-lanes');
  const host = $('#lanes-row');
  if (!section || !host) return;
  const cached = state.lanes[network];
  if (!cached) {
    loadLanes(network).then((d) => {
      if (d && state.network === network && parseRoute().view === 'explore') renderLanes(network);
    });
    section.hidden = true;
    return;
  }
  const lanes = (cached.data && cached.data.lanes) || [];
  if (!lanes.length) { section.hidden = true; return; }
  section.hidden = false;
  const cnt = $('#lanes-count');
  if (cnt) cnt.textContent = `${lanes.length} namespace${lanes.length === 1 ? '' : 's'}`;
  const max = Math.max(1, ...lanes.map((l) => l.events));
  host.innerHTML = lanes.slice(0, 14).map((l) => {
    const w = Math.max((l.events / max) * 100, 3).toFixed(1);
    const name = l.kind === 'inscription' ? 'JSON inscriptions' : esc(l.label);
    const title = l.ascii ? ` title="0x${esc(l.hex)}"` : '';
    return `<div class="lane-row"><span class="lane-ns"${title}>${name}</span>` +
      `<span class="lane-track"><span class="lane-fill" style="width:${w}%"></span></span>` +
      `<span class="lane-counts dim">${fmtInt(l.events)} tx${l.events === 1 ? '' : 's'} · ${fmtInt(l.covenants)} coin${l.covenants === 1 ? '' : 's'}</span></div>`;
  }).join('');
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

function renderInscriptions(network) {
  const section = $('#section-inscriptions');
  const host = $('#inscriptions-row');
  if (!section || !host) return;
  const cached = state.inscriptions[network];
  if (!cached) {
    loadInscriptions(network).then((d) => {
      if (d && state.network === network && parseRoute().view === 'explore') renderInscriptions(network);
    });
    section.hidden = true;
    return;
  }
  const items = (cached.data && cached.data.inscriptions) || [];
  if (!items.length) { section.hidden = true; return; }
  section.hidden = false;
  const cnt = $('#inscriptions-count');
  if (cnt) cnt.textContent = `${items.length} kind${items.length === 1 ? '' : 's'}`;
  const max = Math.max(1, ...items.map((l) => l.events));
  host.innerHTML = items.slice(0, 14).map((l) => {
    const w = Math.max((l.events / max) * 100, 3).toFixed(1);
    return `<div class="lane-row"><span class="lane-ns">${esc(l.label)}</span>` +
      `<span class="lane-track"><span class="lane-fill" style="width:${w}%"></span></span>` +
      `<span class="lane-counts dim">${fmtInt(l.events)} tx${l.events === 1 ? '' : 's'} · ${fmtInt(l.covenants)} coin${l.covenants === 1 ? '' : 's'}</span></div>`;
  }).join('');
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

function renderLifespans(network) {
  const section = $('#section-lifespans');
  const host = $('#lifespans-row');
  if (!section || !host) return;
  const cached = state.lifespans[network];
  if (!cached) {
    loadLifespans(network).then((d) => {
      if (d && state.network === network && parseRoute().view === 'explore') renderLifespans(network);
    });
    section.hidden = true;
    return;
  }
  const data = cached.data;
  const buckets = (data && data.buckets) || [];
  if (!buckets.length || !data.total) { section.hidden = true; return; }
  section.hidden = false;
  const medMin = data.median_ms / 60000;
  const medLabel = medMin >= 1 ? `${medMin.toFixed(1)} min` : `${Math.round(data.median_ms / 1000)} s`;
  const cnt = $('#lifespans-count');
  if (cnt) cnt.textContent = `median ${medLabel}`;
  const med = $('#lifespans-median');
  if (med) med.textContent = `median lifespan ${medLabel}, across ${fmtInt(data.total)} retired coins.`;
  const max = Math.max(1, ...buckets.map((b) => b.count));
  host.innerHTML = buckets.map((b) => {
    const w = Math.max((b.count / max) * 100, 2).toFixed(1);
    return `<div class="lane-row"><span class="lane-ns">${esc(b.label)}</span>` +
      `<span class="lane-track"><span class="lane-fill" style="width:${w}%"></span></span>` +
      `<span class="lane-counts dim">${fmtInt(b.count)} coin${b.count === 1 ? '' : 's'}</span></div>`;
  }).join('');
}

/* the app graph — a force-directed cluster of a family (reuses loaded
   families data; renders lazily when the section is expanded) */
let appGraphCtrl = null;
let graphIdx = 0;

function graphFamilies() {
  const c = state.families[state.network];
  return ((c && c.data && c.data.families) || []).filter((f) => f.size >= 2);
}

function renderAppGraph() {
  if (!window.kascovGraph) return;
  const fams = graphFamilies();
  const canvas = $('#appgraph-canvas');
  const label = $('#appgraph-label');
  if (!fams.length || !canvas) return;
  graphIdx = ((graphIdx % fams.length) + fams.length) % fams.length;
  const fam = fams[graphIdx];
  if (label) label.textContent = `app ${graphIdx + 1} / ${fmtInt(fams.length)} · ${fmtInt(fam.size)} coins${fam.size > 40 ? ' (showing 40)' : ''}`;
  if (appGraphCtrl) appGraphCtrl.stop();
  appGraphCtrl = window.kascovGraph.render(
    canvas,
    { members: fam.members, label: `${fam.size} coins` },
    { onPick: (n) => { location.hash = `#/${state.network}/c/${n.id}`; } }
  );
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

/* covenant apps: coins that moved together in a transaction. Renders the
   biggest few clusters; hidden when none (single-covenant networks). */
function renderFamilies(network) {
  const section = $('#section-families');
  const host = $('#families-row');
  if (!section || !host) return;
  const cached = state.families[network];
  if (!cached) {
    loadFamilies(network).then((d) => {
      if (d && state.network === network && parseRoute().view === 'explore') renderFamilies(network);
    });
    section.hidden = true;
    return;
  }
  const fams = (cached.data && cached.data.families) || [];
  if (!fams.length) { section.hidden = true; return; }
  section.hidden = false;
  const fcnt = $('#families-count');
  if (fcnt) fcnt.textContent = `${fams.length} app${fams.length === 1 ? '' : 's'}`;
  // the app-graph section shares this families data
  const gsec = $('#section-appgraph');
  if (gsec) {
    const graphable = fams.filter((f) => f.size >= 2).length;
    if (graphable) {
      gsec.hidden = false;
      const gcnt = $('#appgraph-count');
      if (gcnt) gcnt.textContent = `${fmtInt(graphable)} app${graphable === 1 ? '' : 's'}`;
      if (gsec.open) renderAppGraph();
    } else {
      gsec.hidden = true;
    }
  }
  host.innerHTML = fams.slice(0, 6).map((f) => {
    const named = f.members.filter((m) => m.template && !/^p2(pk|sh)/.test(m.template));
    const label = named.length
      ? [...new Set(named.map((m) => m.template.replace('SilverScript · ', '')))].join(' + ')
      : `${f.size} smart coins`;
    const members = f.members.slice(0, 6).map((m) =>
      `<a class="fam-member" href="#/${esc(network)}/c/${esc(m.covenant_id)}">` +
      `${avatarSvg(m.covenant_id, 26)}<span>${esc(friendlyName(m.covenant_id))}</span></a>`
    ).join('');
    return `<div class="family-card">` +
      `<div class="family-head"><span class="family-label">${esc(label)}</span>` +
      `<span class="family-sub dim">${f.size} coins moved together in a transaction</span></div>` +
      `<div class="family-members">${members}</div></div>`;
  }).join('');
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

/* DAA → ms against the live feed's own tip anchor. */
function liteMs(live, daa) {
  const aDaa = live.tip_daa != null ? live.tip_daa : live.stats.last_activity_daa;
  const aMs = live.tip_at_ms != null ? live.tip_at_ms : live.generated_at_ms;
  return aMs - (aDaa - daa) * MS_PER_DAA;
}

/* A burst of activity on one coin (10 burns in one tx wave) shouldn't fill
   the whole feed — show each coin once, newest event wins. */
function dedupByCovenant(events) {
  const out = [];
  const seen = new Set();
  for (const ev of events || []) {
    if (seen.has(ev.covenant_id)) continue;
    seen.add(ev.covenant_id);
    out.push(ev);
  }
  return out;
}

function liteStoryRow(ev, live, network) {
  const name = friendlyName(ev.covenant_id);
  const meta = KIND_META[ev.kind] || KIND_META.transition;
  const ms = liteMs(live, ev.accepting_daa);
  const sentence =
    ev.kind === 'genesis' ? `<strong>${esc(name)}</strong> was born`
    : ev.kind === 'burn' ? `<strong>${esc(name)}</strong> retired`
    : `<strong>${esc(name)}</strong> moved`;
  return `<li><a class="story ${meta.cls}" href="#/${esc(network)}/c/${esc(ev.covenant_id)}">` +
    avatarSvg(ev.covenant_id, 34) +
    `<span class="story-text">${sentence} <span class="story-when" title="${esc(utcTitle(ms))}">— ${esc(relTime(ms))} <span class="abs-t">· ${esc(absShort(ms))}</span></span></span>` +
    `<span class="story-kind" aria-hidden="true">${ICONS[meta.icon]}</span>` +
    `</a></li>`;
}

/* First-paint renderers: everything the live feed can show right away.
   The sections that need the full snapshot (grid, pulse, records, watch)
   show a lightweight loading note and fill in when it lands. */
function renderLiteLanding(live, network) {
  const net = NETWORKS[network];
  document.title = 'kascov — watch Kaspa’s smart coins live their lives';
  document.querySelectorAll('[data-net-word]').forEach((el) => { el.textContent = net.word; });
  const s = live.stats;
  $('#hero-stats').innerHTML = [
    { n: s.covenants, label: 'smart coins tracked' },
    { n: s.active, label: 'alive right now' },
    { n: s.events, label: 'life events recorded' },
  ].map((st) => `<div class="stat"><span class="stat-n">${esc(fmtInt(st.n))}</span><span class="stat-label">${esc(st.label)}</span></div>`).join('');
  const bits = [];
  if (s.live_value > 0) bits.push(`together they hold ${fmtAmount(s.live_value, network)}`);
  const scan = '<span class="radar" aria-hidden="true"></span>scanning the chain…';
  $('#freshness').innerHTML =
    `<span class="live-badge-slot">${liveBadgeHtml(network)}</span> · ${[...bits.map(esc), scan].join(' · ')}`;
  const empty = s.covenants === 0;
  $('#landing-empty').hidden = !empty;
  $('#section-teaser').hidden = empty;
  renderDigestStrip(network, empty);
  if (empty) {
    $('#landing-empty').innerHTML = emptyCardHtml(network);
    return;
  }
  $('#teaser-list').innerHTML = dedupByCovenant(live.recent_events)
    .slice(0, TEASER_COUNT)
    .map((ev) => liteStoryRow(ev, live, network))
    .join('');
}

function renderLiteExplore(live, network) {
  const net = NETWORKS[network];
  document.title = `kascov — exploring ${net.label}`;
  const s = live.stats;
  const bits = [
    `${fmtInt(s.covenants)} smart coin${s.covenants === 1 ? '' : 's'}`,
    `${fmtInt(s.active)} alive`,
    `${fmtInt(s.events)} event${s.events === 1 ? '' : 's'}`,
  ];
  const scan = '<span class="radar" aria-hidden="true"></span>scanning the chain…';
  $('#explore-stats').innerHTML =
    `<span class="live-badge-slot">${liveBadgeHtml(network)}</span> · ${[...bits.map(esc), scan].join(' · ')}`;
  const empty = s.covenants === 0;
  $('#empty-net').hidden = !empty;
  $('#watch-strip').hidden = true;
  $('#section-records').hidden = true;
  /* these fetch their own small endpoints — render even before the big
     snapshot lands so the analytics show immediately */
  renderFamilies(network);
  renderLanes(network);
  renderInscriptions(network);
  renderLifespans(network);
  const tpl = $('#section-templates');
  if (tpl) tpl.hidden = true; /* appears when the full snapshot render runs */
  $('#section-pulse').hidden = true;
  $('#section-stories').hidden = empty;
  $('#section-coins').hidden = empty;
  if (empty) {
    $('#empty-net').innerHTML = emptyCardHtml(network);
    return;
  }
  $('#story-list').innerHTML = dedupByCovenant(live.recent_events)
    .slice(0, STORY_COUNT)
    .map((ev) => liteStoryRow(ev, live, network))
    .join('');
  $('#result-count').textContent = `${fmtInt(s.covenants)} smart coins`;
  $('#coin-grid').innerHTML =
    `<div class="no-results"><p class="dim">loading all ${esc(fmtInt(s.covenants))} smart coins…</p></div>`;
  $('#grid-foot').innerHTML = '';
}

/* ------------------------------------------------------------- sentences */

/* what one event did to the coin's pieces — derived from the utxo set */
function eventShape(entry, ev) {
  const utxos = entry.c.utxos || [];
  const consumed = utxos.filter((u) => u.spent_txid === ev.txid);
  const created = utxos.filter((u) => u.outpoint.startsWith(ev.txid + ':'));
  return {
    consumedN: consumed.length,
    createdN: created.length,
    consumedValue: consumed.reduce((sum, u) => sum + u.value, 0),
  };
}

function eventSentence(entry, ev, network, withBalance) {
  const name = entry.name;
  if (ev.kind === 'genesis') {
    /* a coin can be born in several state pieces (one tx, many bound outputs) */
    const pieces = (entry.c.utxos || []).filter((u) => u.created_daa === entry.c.genesis_daa).length;
    const pieceBit = pieces > 1 ? ` in ${pieces} pieces` : '';
    return entry.birthValue > 0
      ? `<strong>${esc(name)}</strong> was born, holding ${esc(fmtAmount(entry.birthValue, network))}${esc(pieceBit)}`
      : `<strong>${esc(name)}</strong> was born`;
  }
  if (ev.kind === 'transition') {
    /* count by seq value, not array index — older events may be truncated */
    const nth = entry.c.events.filter((e) => e.kind === 'transition' && e.seq <= ev.seq).length;
    const bal = withBalance ? entry.balances.get(ev.accepting_daa) : null;
    const balBit = bal ? ` — now holding ${esc(fmtAmount(bal, network))}` : '';
    const shape = eventShape(entry, ev);
    let shapeBit = '';
    if (shape.consumedN && shape.createdN && shape.consumedN !== shape.createdN) {
      shapeBit = shape.createdN > shape.consumedN
        ? ` <span class="dim">(split ${shape.consumedN} → ${shape.createdN} pieces)</span>`
        : ` <span class="dim">(merged ${shape.consumedN} → ${shape.createdN})</span>`;
    }
    return `<strong>${esc(name)}</strong> moved <span class="dim">(${ordinal(nth)} time)</span>${shapeBit}${balBit}`;
  }
  /* burns: a multi-piece coin retires in stages. "retired" is reserved for
     the burn that actually ends the story — the coin's LAST event on a coin
     that is burned now. A burn followed by more life (the other piece kept
     moving) is "lost a piece", even when it's the only burn so far. */
  const lastEvent = entry.c.events[entry.c.events.length - 1];
  const isFinal = entry.c.status !== 'active' && lastEvent && ev.seq === lastEvent.seq;
  const gone = eventShape(entry, ev).consumedValue;
  const goneBit = gone > 0 ? ` — ${esc(fmtAmount(gone, network))} left the covenant` : '';
  if (!isFinal) {
    const bal = withBalance ? entry.balances.get(ev.accepting_daa) : null;
    const balBit = bal ? `, ${esc(fmtAmount(bal, network))} lives on` : '';
    return `<strong>${esc(name)}</strong> lost a piece${goneBit}${balBit}`;
  }
  const m = entry.moves;
  const tail = m === 0 ? 'without ever moving' : m === 1 ? 'after 1 move' : `after ${m} moves`;
  return `<strong>${esc(name)}</strong> retired ${tail}${goneBit}`;
}

function cardStory(entry, network) {
  const c = entry.c;
  const bits = [];
  bits.push(`${c.genesis_daa != null ? 'born' : 'first seen'} ${relTimeShort(entry.bornMs)}`);
  if (entry.moves > 0) bits.push(`moved ${entry.moves}×`);
  if (c.status === 'active') bits.push(`holds ${fmtAmount(c.live_value, network)}`);
  else bits.push(`retired ${relTimeShort(entry.lastMs)}`);
  return bits.join(' · ');
}

/* ------------------------------------------------------------ pulse chart */

/* what the pointer/keyboard handlers need to know about the painted chart —
   written by paintActivity, null while the fallback (or nothing) shows */
let pulseView = null;
let pulseHot = -1;

/* DAA → ms against the activity response's own tip anchor (liteMs pattern) */
function activityMs(data, daa) {
  const aDaa = data.tip_daa != null ? data.tip_daa
    : (data.buckets.length ? data.buckets[data.buckets.length - 1].daa : data.window_start_daa);
  const aMs = data.tip_at_ms != null ? data.tip_at_ms : data.generated_at_ms;
  return aMs - (aDaa - daa) * MS_PER_DAA;
}

/* build the chart scaffolding (range pills, plot, tooltip, legend) once per
   network; repaints only touch the bars inside so height transitions live */
function ensurePulseShell(network) {
  const host = $('#pulse-chart');
  if (!host) return; /* stale cached index.html */
  if (host.dataset.net === network && host.dataset.mode === 'activity') return;
  pulseView = null;
  pulseHot = -1;
  host.innerHTML =
    `<div class="pulse-head"><p class="pulse-caption" id="pulse-caption">reading the pulse…</p>` +
    `<div class="pulse-ranges" role="group" aria-label="chart time range">` +
    ACTIVITY_RANGES.map((r) =>
      `<button type="button" class="chip" data-action="pulse-range" data-range="${r}" aria-pressed="${r === state.pulseRange}">${ACTIVITY_LABELS[r]}</button>`
    ).join('') +
    `</div></div>` +
    `<div class="pulse-plot" id="pulse-plot" role="img" tabindex="0" aria-label="bar chart of smart-coin life events over time">` +
    `<div class="pulse-bars" id="pulse-bars"></div>` +
    `<p class="dim pulse-none" id="pulse-none" hidden></p>` +
    `<div class="pulse-ticks" id="pulse-ticks"></div>` +
    `<div class="pulse-tip" id="pulse-tip" hidden></div>` +
    `</div>` +
    `<div class="pulse-legend" aria-hidden="true">` +
    `<span><i class="dot dot-born"></i>born</span>` +
    `<span><i class="dot dot-move"></i>moved</span>` +
    `<span><i class="dot dot-burn"></i>retired</span></div>`;
  host.dataset.net = network;
  host.dataset.mode = 'activity';
}

/* paint one activity response into the shell — index-keyed columns so
   heights transition in place between refetches */
function paintActivity(data, network) {
  const host = $('#pulse-chart');
  const bars = $('#pulse-bars');
  const ticksHost = $('#pulse-ticks');
  const none = $('#pulse-none');
  const caption = $('#pulse-caption');
  const plot = $('#pulse-plot');
  if (!host || !bars || !ticksHost || !none || !caption || !plot) return;

  const showEmpty = (noneText, captionText) => {
    hidePulseTip();
    pulseView = null;
    bars.hidden = true;
    ticksHost.hidden = true;
    none.hidden = false;
    none.textContent = noneText;
    caption.textContent = captionText;
    plot.setAttribute('aria-label', `bar chart of smart-coin life events — ${captionText}`);
    host.dataset.range = data.range;
  };

  const width = data.bucket_daa || 1;
  const anchorDaa = data.tip_daa != null ? data.tip_daa
    : (data.buckets.length ? data.buckets[data.buckets.length - 1].daa : null);
  if (anchorDaa == null) {
    /* an index with no tip and no events — nothing to anchor a window on */
    showEmpty('no life events yet.', 'no life events yet');
    return;
  }
  const last = Math.floor(anchorDaa / width);
  let first = Math.floor((data.window_start_daa != null ? data.window_start_daa
    : (data.buckets[0] ? data.buckets[0].daa : anchorDaa)) / width);
  let n = last - first + 1;
  if (n > ACTIVITY_MAX_COLS) { first = last - ACTIVITY_MAX_COLS + 1; n = ACTIVITY_MAX_COLS; }
  if (n < 1) { first = last; n = 1; }

  /* client-side zero-fill: the endpoint omits empty buckets */
  const buckets = Array.from({ length: n }, () => ({ births: 0, moves: 0, burns: 0, total: 0 }));
  for (const b of data.buckets) {
    const i = Math.floor(b.daa / width) - first;
    if (i < 0 || i >= n) continue;
    const births = Number(b.births) || 0;
    const moves = Number(b.moves) || 0;
    const burns = Number(b.burns) || 0;
    buckets[i] = { births, moves, burns, total: births + moves + burns };
  }
  const total = buckets.reduce((sum, b) => sum + b.total, 0);
  const spanMs = n * width * MS_PER_DAA;
  const windowStartMs = activityMs(data, first * width);
  /* clock times alone mislead once the window is long or long past (the old
     chart's rule); seconds are never needed at ≥1h spans */
  const withDate = spanMs > 20 * 3600 * 1000 || (Date.now() - windowStartMs) > 20 * 3600 * 1000;
  const phrase = data.range === 'all' ? `across ${fmtSpan(spanMs)}` : ACTIVITY_PHRASE[data.range];

  if (total === 0) {
    /* pills stay so the reader can widen the range */
    showEmpty(
      data.range === 'all' ? 'no life events yet.' : `nothing happened ${phrase} yet.`,
      `no life events ${phrase}`
    );
    return;
  }
  bars.hidden = false;
  ticksHost.hidden = false;
  none.hidden = true;

  /* y auto-scale: 92% headroom leaves room for the 2px gaps and the tooltip
     lane; a 2% floor keeps a lone event visible */
  const maxTotal = Math.max(1, ...buckets.map((b) => b.total));
  const pct = (c) => (c ? Math.max((c / maxTotal) * 92, 2) : 0);

  const rebuild = bars.children.length !== n || host.dataset.range !== data.range;
  if (rebuild) {
    hidePulseTip();
    /* DOM order top→bottom burn/move/born + flex-end stacks born at the baseline */
    bars.innerHTML = Array.from({ length: n }, () =>
      '<div class="pulse-col"><div class="pulse-seg seg-burn"></div><div class="pulse-seg seg-move"></div><div class="pulse-seg seg-born"></div></div>'
    ).join('');
  }
  const setHeights = () => {
    for (let i = 0; i < n; i++) {
      const col = bars.children[i];
      if (!col) break;
      const counts = [buckets[i].burns, buckets[i].moves, buckets[i].births];
      let seen = false; /* a visible segment higher in the stack */
      for (let k = 0; k < 3; k++) {
        const el = col.children[k];
        const c = counts[k];
        el.style.height = pct(c) + '%';
        /* 2px surface gap between visible segments */
        el.style.marginTop = c && seen ? '2px' : '0px';
        /* rounded data-end on the top-most visible segment only */
        el.classList.toggle('seg-cap', Boolean(c) && !seen);
        if (c) seen = true;
      }
    }
  };
  if (rebuild) requestAnimationFrame(setHeights); /* first paint grows from 0 */
  else setHeights();

  const tickIdx = [...new Set([0, n >> 2, n >> 1, (3 * n) >> 2, n - 1])];
  ticksHost.innerHTML = tickIdx.map((i) =>
    `<span>${esc(fmtClock(activityMs(data, (first + i) * width + width / 2), false, withDate))}</span>`
  ).join('');

  let latestIdx = 0;
  for (let i = n - 1; i >= 0; i--) { if (buckets[i].total) { latestIdx = i; break; } }
  const latestMs = activityMs(data, Math.min((first + latestIdx) * width + width, anchorDaa));
  const captionText =
    `${fmtInt(total)} event${total === 1 ? '' : 's'} ${phrase} · latest ${relTime(latestMs)}`;
  caption.textContent = captionText;
  plot.setAttribute('aria-label', `bar chart of smart-coin life events — ${captionText}`);

  pulseView = {
    n, first, width, buckets, anchorDaa,
    anchorMs: data.tip_at_ms != null ? data.tip_at_ms : data.generated_at_ms,
    withDate,
    network,
    range: data.range,
  };
  host.dataset.range = data.range;
}

/* fetch (through the client TTL) then paint the current range; a known 404
   (older worker) falls back to the summaries-derived SVG chart */
async function updatePulse(network) {
  const range = state.pulseRange;
  const d = await loadActivity(network, range);
  if (state.network !== network || state.pulseRange !== range || parseRoute().view !== 'explore') return;
  if (d) {
    ensurePulseShell(network);
    paintActivity(d, network);
  } else {
    const entry = state.cache[network];
    const byRange = state.activity[network];
    const known404 = byRange && byRange[range] && byRange[range].data === null;
    if (known404 && entry) renderPulseFallback(entry);
  }
}

/* renderExplore's entry point: instant paint from cache (or the shell's
   loading note), then a TTL-guarded refetch */
function renderPulse(entry, network) {
  const hit = state.activity[network] && state.activity[network][state.pulseRange];
  if (hit && hit.data === null) {
    renderPulseFallback(entry);
  } else {
    ensurePulseShell(network);
    if (hit && hit.data) paintActivity(hit.data, network);
  }
  updatePulse(network);
}

/* pollLive-detected changes refetch the chart, debounced — pollLive's 12s
   cadence (SSE pokes fold into it) is the natural rate limit */
let pulseRefreshTimer = 0;
function schedulePulseRefresh() {
  clearTimeout(pulseRefreshTimer);
  pulseRefreshTimer = setTimeout(() => {
    const byRange = state.activity[state.network];
    if (byRange && byRange[state.pulseRange] && byRange[state.pulseRange].data !== null) {
      byRange[state.pulseRange].at = 0; /* stamp stale; keep known 404s quiet */
    }
    if (parseRoute().view === 'explore' && document.visibilityState === 'visible') {
      updatePulse(state.network);
    }
  }, 1500);
}

/* one pointer-driven tooltip for the whole plot — nearest-bucket model, so
   4px-wide phone columns never need individual hit targets */
function setPulseHot(i) {
  if (!pulseView) return;
  const bars = $('#pulse-bars');
  const tip = $('#pulse-tip');
  const plot = $('#pulse-plot');
  if (!bars || !tip || !plot || bars.hidden) return;
  i = Math.max(0, Math.min(pulseView.n - 1, i));
  const col = bars.children[i];
  if (!col) return;
  pulseHot = i;
  for (let k = 0; k < bars.children.length; k++) {
    bars.children[k].classList.toggle('is-hot', k === i);
  }
  const b = pulseView.buckets[i];
  const centerDaa = (pulseView.first + i) * pulseView.width + pulseView.width / 2;
  const centerMs = pulseView.anchorMs - (pulseView.anchorDaa - centerDaa) * MS_PER_DAA;
  const rows = [];
  if (b.births) rows.push(`<i class="dot dot-born"></i> <strong>${esc(fmtInt(b.births))}</strong> born`);
  if (b.moves) rows.push(`<i class="dot dot-move"></i> <strong>${esc(fmtInt(b.moves))}</strong> moved`);
  if (b.burns) rows.push(`<i class="dot dot-burn"></i> <strong>${esc(fmtInt(b.burns))}</strong> retired`);
  tip.innerHTML =
    `<span class="tip-when">around ${esc(fmtClock(centerMs, false, pulseView.withDate))}</span><br>` +
    (rows.length ? rows.join(' · ') : '<span class="dim">quiet</span>');
  tip.hidden = false;
  const plotRect = plot.getBoundingClientRect();
  const colRect = col.getBoundingClientRect();
  const center = colRect.left + colRect.width / 2 - plotRect.left;
  tip.style.left =
    Math.max(4, Math.min(center - tip.offsetWidth / 2, plotRect.width - tip.offsetWidth - 4)) + 'px';
}

function hidePulseTip() {
  pulseHot = -1;
  const tip = $('#pulse-tip');
  if (tip) tip.hidden = true;
  document.querySelectorAll('#pulse-bars .pulse-col.is-hot').forEach((el) => {
    el.classList.remove('is-hot');
  });
}

function onPulsePointer(e) {
  if (!pulseView) return;
  const bars = $('#pulse-bars');
  if (!bars || bars.hidden) return;
  const rect = bars.getBoundingClientRect();
  if (!rect.width || e.clientY < rect.top - 12 || e.clientY > rect.bottom + 12) {
    hidePulseTip();
    return;
  }
  setPulseHot(Math.floor((e.clientX - rect.left) / rect.width * pulseView.n));
}

/* the old summaries-based SVG chart, kept verbatim — the fallback when the
   activity endpoint 404s (older worker); ranges don't apply to grid-derived
   data, so it has no pills */
function renderPulseFallback(entry) {
  const { index } = entry;
  /* the grid feed carries one row per coin, not full timelines — the pulse
     charts births and retirements (moves stream through "latest stories") */
  const events = [];
  for (const e of index.covs) {
    if (e.c.genesis_daa != null) events.push({ kind: 'genesis', ms: e.bornMs });
    if (e.c.status !== 'active') events.push({ kind: 'burn', ms: e.lastMs });
  }
  const host = $('#pulse-chart');
  host.dataset.mode = 'fallback'; /* so ensurePulseShell rebuilds when the endpoint returns */
  delete host.dataset.range;
  pulseView = null;
  if (!events.length) { host.innerHTML = '<p class="dim">no life events yet.</p>'; return; }

  let min = Infinity, max = -Infinity;
  for (const ev of events) { if (ev.ms < min) min = ev.ms; if (ev.ms > max) max = ev.ms; }
  if (max - min < 60000) { min -= 30000; max += 30000; }
  const span = max - min;
  const bw = span / PULSE_BUCKETS;

  const buckets = Array.from({ length: PULSE_BUCKETS }, () => ({ genesis: 0, transition: 0, burn: 0 }));
  for (const ev of events) {
    let i = Math.floor((ev.ms - min) / bw);
    if (i >= PULSE_BUCKETS) i = PULSE_BUCKETS - 1;
    if (i < 0) i = 0;
    buckets[i][ev.kind] = (buckets[i][ev.kind] || 0) + 1;
  }
  const maxTotal = Math.max(1, ...buckets.map((b) => b.genesis + b.transition + b.burn));

  const W = 720, H = 190, padT = 20, padB = 26, padX = 6;
  const innerW = W - padX * 2, innerH = H - padT - padB;
  const gap = 6;
  const slot = innerW / PULSE_BUCKETS;
  const barW = slot - gap;
  const withSeconds = span < 15 * 60 * 1000;
  /* clock times alone mislead once the window is long or long past */
  const withDate = span > 20 * 3600 * 1000 || (Date.now() - min) > 20 * 3600 * 1000;

  const colors = { genesis: 'var(--born)', transition: 'var(--move)', burn: 'var(--burn)' };
  let bars = '';
  buckets.forEach((b, i) => {
    const x = padX + i * slot + gap / 2;
    const total = b.genesis + b.transition + b.burn;
    const centerMs = min + (i + 0.5) * bw;
    let y = H - padB;
    const parts = [];
    for (const kind of ['genesis', 'transition', 'burn']) {
      if (!b[kind]) continue;
      let h = (b[kind] / maxTotal) * innerH;
      if (h < 3) h = 3;
      y -= h;
      parts.push(`<rect x="${x.toFixed(1)}" y="${y.toFixed(1)}" width="${barW.toFixed(1)}" height="${h.toFixed(1)}" rx="2" fill="${colors[kind]}"/>`);
    }
    const label = [
      b.genesis ? `${b.genesis} born` : '',
      b.transition ? `${b.transition} move${b.transition > 1 ? 's' : ''}` : '',
      b.burn ? `${b.burn} retired` : '',
    ].filter(Boolean).join(', ') || 'quiet';
    /* visible count so touch users aren't dependent on hover tooltips */
    const count = total
      ? `<text x="${(x + barW / 2).toFixed(1)}" y="${Math.max(y - 4, 15).toFixed(1)}" text-anchor="middle" class="pulse-count">${total}</text>`
      : '';
    bars += `<g>${parts.join('')}${count}<rect x="${x.toFixed(1)}" y="${padT}" width="${barW.toFixed(1)}" height="${innerH}" fill="transparent">` +
      `<title>${esc(fmtClock(centerMs, withSeconds, withDate))} — ${esc(label)}${total ? ` (${total} event${total > 1 ? 's' : ''})` : ''}</title></rect></g>`;
  });

  let ticks = '';
  for (const i of [0, 3, 6, 9, 11]) {
    const centerMs = min + (i + 0.5) * bw;
    const anchor = i === 0 ? 'start' : i === PULSE_BUCKETS - 1 ? 'end' : 'middle';
    const x = anchor === 'start' ? padX + i * slot + gap / 2
      : anchor === 'end' ? padX + (i + 1) * slot - gap / 2
      : padX + i * slot + gap / 2 + barW / 2;
    ticks += `<text x="${x.toFixed(1)}" y="${H - 8}" text-anchor="${anchor}" class="pulse-tick">${esc(fmtClock(centerMs, withSeconds, withDate))}</text>`;
  }

  const caption = `${fmtInt(events.length)} birth${events.length === 1 ? '' : 's'} & retirement${events.length === 1 ? '' : 's'} ` +
    `over ${fmtSpan(span)} · the latest ${relTime(max)}`;

  host.innerHTML =
    `<p class="pulse-caption">${esc(caption)}</p>` +
    `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="Bar chart of smart-coin life events over time" preserveAspectRatio="xMidYMid meet">` +
    `<line x1="${padX}" y1="${H - padB + 0.5}" x2="${W - padX}" y2="${H - padB + 0.5}" class="pulse-axis"/>` +
    bars + ticks + `</svg>` +
    `<div class="pulse-legend" aria-hidden="true">` +
    `<span><i class="dot dot-born"></i>born</span>` +
    `<span><i class="dot dot-burn"></i>retired</span></div>`;
}

/* ---------------------------------------------------------------- stories */

/* Stories come from the live feed (the newest ~150 events across all coins);
   the grid feed itself carries no per-event data. Falls back to a synthetic
   feed from summaries when the live feed hasn't answered yet. */
function buildFeed(entry, network) {
  const live = state.live[network] && state.live[network].data;
  if (live && Array.isArray(live.recent_events)) {
    return { live, events: dedupByCovenant(live.recent_events) };
  }
  /* fallback: most recently active coins as one-line stories */
  const events = entry.index.covs.slice(0, STORY_COUNT).map((e) => ({
    covenant_id: e.c.covenant_id,
    kind: e.c.status !== 'active' ? 'burn' : (e.moves > 0 ? 'transition' : 'genesis'),
    accepting_daa: e.c.last_activity_daa,
  }));
  return { live: null, events };
}

function storyRow(ev, entry, live, network) {
  const meta = KIND_META[ev.kind] || KIND_META.transition;
  const ms = live ? liteMs(live, ev.accepting_daa) : daaToMs(ev.accepting_daa, entry.data);
  const rec = entry.index.byId.get(ev.covenant_id);
  const name = rec ? rec.name : friendlyName(ev.covenant_id);
  const sentence =
    ev.kind === 'genesis' ? `<strong>${esc(name)}</strong> was born`
    : ev.kind === 'burn' ? `<strong>${esc(name)}</strong> retired`
    : `<strong>${esc(name)}</strong> moved`;
  return `<li><a class="story ${meta.cls}" href="#/${esc(network)}/c/${esc(ev.covenant_id)}">` +
    avatarSvg(ev.covenant_id, 34) +
    `<span class="story-text">${sentence} <span class="story-when" title="${esc(utcTitle(ms))}">— ${esc(relTime(ms))} <span class="abs-t">· ${esc(absShort(ms))}</span></span></span>` +
    `<span class="story-kind" aria-hidden="true">${ICONS[meta.icon]}</span>` +
    `</a></li>`;
}

function renderStories(entry, network) {
  const { live, events } = buildFeed(entry, network);
  $('#story-list').innerHTML = events
    .slice(0, STORY_COUNT)
    .map((ev) => storyRow(ev, entry, live, network))
    .join('');
}

/* ------------------------------------------------------------------- grid */

function matchesFilter(entry) {
  if (state.filter === 'watch') {
    if (!state.watch.has(entry.c.covenant_id)) return false;
  } else if (state.filter !== 'all' && entry.c.status !== state.filter) {
    return false;
  }
  if (state.query && !entry.blob.includes(state.query)) return false;
  return true;
}

const SORTS = {
  activity: (a, b) => (b.c.last_activity_daa || 0) - (a.c.last_activity_daa || 0),
  newest: (a, b) => b.bornMs - a.bornMs,
  oldest: (a, b) => a.bornMs - b.bornMs,
  richest: (a, b) => b.c.live_value - a.c.live_value,
  moves: (a, b) => b.moves - a.moves,
};

function renderGrid(entry, network) {
  const list = entry.index.covs.filter(matchesFilter).sort(SORTS[state.sort] || SORTS.activity);
  const total = entry.index.covs.length;
  $('#result-count').textContent = (state.query || state.filter !== 'all')
    ? `${list.length} of ${total} smart coin${total === 1 ? '' : 's'}`
    : `${total} smart coin${total === 1 ? '' : 's'}`;

  const grid = $('#coin-grid');
  const foot = $('#grid-foot');
  if (!list.length) {
    if (/^[0-9a-f]{64}$/.test(state.query)) {
      if (state.txLookup[state.query] === 'pending') {
        grid.innerHTML = `<div class="no-results">` +
          `<p>checking whether <strong class="mono">${esc(shortHex(state.query, 12, 10))}</strong> touched a smart coin…</p></div>`;
      } else {
        /* a full id that resolved nowhere — answer the tester's real question */
        grid.innerHTML = `<div class="no-results">` +
          `<p>kascov hasn’t seen <strong class="mono">${esc(shortHex(state.query, 12, 10))}</strong> in any covenant on ${esc(NETWORKS[network].label)}.</p>` +
          `<p class="dim">it may be a regular (non-covenant) transaction, still unconfirmed, or on the other network — ` +
          `<a href="${esc(txUrl(network, state.query))}" target="_blank" rel="noopener noreferrer">check it on the block explorer ↗</a></p>` +
          `<p class="dim">if it’s a public key rather than a transaction, ` +
          `<a href="#/${esc(network)}/addr/${esc(state.query)}">see the smart coins it owns →</a></p></div>`;
      }
    } else if (state.filter === 'watch' && !state.watch.size) {
      grid.innerHTML = `<div class="no-results"><p>You’re not watching any coins yet.</p>` +
        `<p class="dim">tap the ★ on any coin to pin it here — it survives reloads.</p></div>`;
    } else {
      const example = entry.index.covs[0] ? entry.index.covs[0].name : 'brave-teal-otter';
      grid.innerHTML = `<div class="no-results"><p>No smart coins match${state.query ? ` <strong>“${esc(state.query)}”</strong>` : ' that filter'}.</p>` +
        `<p class="dim">Try a friendly name like <em>${esc(example)}</em>, or paste a coin’s id or a transaction id.</p></div>`;
    }
    foot.innerHTML = '';
    return;
  }
  grid.innerHTML = list.slice(0, state.shown).map((e) => {
    const alive = e.c.status === 'active';
    const watched = state.watch.has(e.c.covenant_id);
    const namedTpl = e.c.template && !/^p2(pk|sh)/.test(e.c.template) ? e.c.template : null;
    return `<article class="card">` +
      `<div class="card-head">${avatarSvg(e.c.covenant_id, 40)}` +
      `<div class="card-id"><a class="card-link" href="#/${esc(network)}/c/${esc(e.c.covenant_id)}">${esc(e.name)}</a>` +
      `<span class="pill ${alive ? 'pill-alive' : 'pill-retired'}" title="${esc(alive ? GLOSSARY.alive : GLOSSARY.retired)}">${alive ? 'alive' : 'retired'}</span>` +
      (namedTpl ? `<span class="flag flag-tpl" title="recognized contract: a compiled ${esc(namedTpl)} — constructor arguments labeled on the coin page">${esc(namedTpl)}</span>` : '') +
      `</div>` +
      `<button type="button" class="star${watched ? ' starred' : ''}" data-action="watch" data-id="${esc(e.c.covenant_id)}"` +
      ` aria-pressed="${watched}" aria-label="${watched ? 'stop watching' : 'watch'} ${esc(e.name)}">★</button></div>` +
      `<p class="card-story">${esc(cardStory(e, network))}</p>` +
      `</article>`;
  }).join('');
  foot.innerHTML = list.length > state.shown
    ? `<button type="button" class="btn" data-action="more">show more <span class="dim">(${list.length - state.shown} hidden)</span></button>`
    : '';
}

/* --------------------------------------------------- search suggestions */

const suggest = { items: [], active: -1 };

function suggestionItems(entry) {
  const q = state.query;
  if (!entry || !q || q.length < 2) return [];
  const out = [];
  const seen = new Set();
  const push = (e, why, tx, score) => {
    if (seen.has(e.c.covenant_id)) return;
    seen.add(e.c.covenant_id);
    out.push({ e, why, tx, score });
  };
  for (const e of entry.index.covs) {
    if (e.name.startsWith(q)) push(e, 'name', null, 0);
    else if (e.name.includes(q)) push(e, 'name', null, 1);
    else if (e.c.covenant_id.startsWith(q)) push(e, 'coin id', null, 2);
    if (out.length >= 24) break;
  }
  /* full transaction ids resolve through the server (the grid feed carries
     no per-event txids) — the search handler routes those directly */
  out.sort((a, b) => a.score - b.score ||
    (b.e.c.last_activity_daa || 0) - (a.e.c.last_activity_daa || 0));
  return out.slice(0, 8);
}

function markMatch(name, q) {
  const i = name.indexOf(q);
  if (i < 0) return esc(name);
  return esc(name.slice(0, i)) + '<mark>' + esc(name.slice(i, i + q.length)) + '</mark>' +
    esc(name.slice(i + q.length));
}

function closeSuggest() {
  const host = $('#search-suggest');
  if (!host) return;
  host.hidden = true;
  host.innerHTML = '';
  suggest.items = [];
  suggest.active = -1;
  const input = $('#search');
  if (input) {
    input.setAttribute('aria-expanded', 'false');
    input.removeAttribute('aria-activedescendant');
  }
}

function renderSuggest() {
  const host = $('#search-suggest');
  if (!host) return;
  suggest.items = suggestionItems(state.cache[state.network]);
  suggest.active = -1;
  if (!suggest.items.length) { closeSuggest(); return; }
  host.innerHTML = suggest.items.map((s, i) => {
    const alive = s.e.c.status === 'active';
    const href = `#/${esc(state.network)}/c/${esc(s.e.c.covenant_id)}` +
      (s.tx ? `?tx=${esc(s.tx)}` : '');
    const kind = s.why === 'name' ? '' :
      `<span class="suggest-kind">${esc(s.why)} ${esc(shortHex(s.tx || s.e.c.covenant_id, 8, 6))}</span>`;
    return `<a class="suggest-item" id="sugg-${i}" role="option" href="${href}" data-suggest="${i}">` +
      avatarSvg(s.e.c.covenant_id, 26) +
      `<span class="suggest-name">${markMatch(s.e.name, state.query)}</span>` +
      kind +
      `<span class="pill ${alive ? 'pill-alive' : 'pill-retired'}" title="${esc(alive ? GLOSSARY.alive : GLOSSARY.retired)}">${alive ? 'alive' : 'retired'}</span>` +
      `</a>`;
  }).join('');
  host.hidden = false;
  const input = $('#search');
  if (input) input.setAttribute('aria-expanded', 'true');
}

function setActiveSuggest(i) {
  suggest.active = i;
  const input = $('#search');
  document.querySelectorAll('.suggest-item').forEach((el, k) => {
    el.classList.toggle('is-active', k === i);
  });
  if (input) {
    if (i >= 0) input.setAttribute('aria-activedescendant', `sugg-${i}`);
    else input.removeAttribute('aria-activedescendant');
  }
  const el = document.getElementById(`sugg-${i}`);
  if (el) el.scrollIntoView({ block: 'nearest' });
}

function goToSuggestion(s) {
  const input = $('#search');
  if (input) input.value = '';
  state.query = '';
  closeSuggest();
  location.hash = `#/${state.network}/c/${s.e.c.covenant_id}` + (s.tx ? `?tx=${s.tx}` : '');
}

/* ------------------------------------------------- records + watch strip */

function miniCard(e, network, label, sub) {
  return `<div class="mini-card">${avatarSvg(e.c.covenant_id, 34)}` +
    `<span class="mini-body">` +
    (label ? `<span class="mini-label">${esc(label)}</span>` : '') +
    `<a class="mini-name" href="#/${esc(network)}/c/${esc(e.c.covenant_id)}">${esc(e.name)}</a>` +
    `<span class="mini-sub">${esc(sub)}</span></span></div>`;
}

function renderWatchStrip(entry, network) {
  const strip = $('#watch-strip');
  const watched = [...state.watch]
    .map((id) => entry.index.byId.get(id))
    .filter(Boolean)
    .sort((a, b) => (b.c.last_activity_daa || 0) - (a.c.last_activity_daa || 0));
  strip.hidden = watched.length === 0;
  if (!watched.length) return;
  $('#watch-row').innerHTML = watched.map((e) => {
    const alive = e.c.status === 'active';
    const sub = alive
      ? `holds ${fmtAmount(e.c.live_value, network)} · active ${relTimeShort(e.lastMs)}`
      : `retired ${relTimeShort(e.lastMs)}`;
    return miniCard(e, network, null, sub);
  }).join('');
}

function renderRecords(entry, network) {
  const covs = entry.index.covs;
  const alive = covs.filter((e) => e.c.status === 'active');
  const retired = covs.filter((e) => e.c.status !== 'active');
  const born = (list) => list.filter((e) => e.c.genesis_daa != null);
  const recs = [];
  const oldest = born(alive).sort((a, b) => a.bornMs - b.bornMs)[0];
  if (oldest) recs.push({ e: oldest, label: 'oldest alive', sub: `born ${relTimeShort(oldest.bornMs)}` });
  const traveled = [...covs].sort((a, b) => b.moves - a.moves)[0];
  if (traveled && traveled.moves > 1) recs.push({ e: traveled, label: 'most traveled', sub: `moved ${traveled.moves} times` });
  const richest = [...alive].sort((a, b) => b.c.live_value - a.c.live_value)[0];
  if (richest && richest.c.live_value > 0) recs.push({ e: richest, label: 'richest', sub: `holds ${fmtAmount(richest.c.live_value, network)}` });
  const bigBirth = [...covs].sort((a, b) => b.birthValue - a.birthValue)[0];
  if (bigBirth && bigBirth.birthValue > 0) recs.push({ e: bigBirth, label: 'biggest birth', sub: `born holding ${fmtAmount(bigBirth.birthValue, network)}` });
  const longLife = born(retired).sort((a, b) => (b.lastMs - b.bornMs) - (a.lastMs - a.bornMs))[0];
  if (longLife && longLife.lastMs > longLife.bornMs) recs.push({ e: longLife, label: 'longest life', sub: `lived ${fmtSpan(longLife.lastMs - longLife.bornMs)}` });
  /* one coin can hold several crowns; show each coin once, first crown wins */
  const seen = new Set();
  const unique = recs.filter((r) => !seen.has(r.e.c.covenant_id) && seen.add(r.e.c.covenant_id));
  $('#section-records').hidden = unique.length === 0;
  $('#records-row').innerHTML = unique.map((r) => miniCard(r.e, network, r.label, r.sub)).join('');
}

/* --------------------------------------------------------- contract types */

/* bar colors from a fixed token whitelist — never from response data — so
   the style attribute stays injection-safe alongside the esc()'d text */
function templateColor(name) {
  if (/^SilverScript/.test(name)) return 'var(--accent)';
  if (/^p2pk/.test(name)) return 'var(--born)';
  if (/^p2sh/.test(name)) return 'var(--move)';
  return 'var(--burn)';
}

/* "what's running here" — bar length ∝ live states (present tense); a
   template that only ever showed up in spend-time reveals keeps a zero bar
   and the "ran N× at spend" count carries the truth */
function renderTemplates(network) {
  const section = $('#section-templates');
  const host = $('#template-bars');
  if (!section || !host) return; /* stale cached index.html */
  const cached = state.templates[network];
  if (!cached) {
    section.hidden = false;
    host.innerHTML = '<p class="dim"><span class="radar" aria-hidden="true"></span>reading contract types…</p>';
    return;
  }
  if (!cached.data) { section.hidden = true; return; } /* older worker (404) */
  const data = cached.data;
  const rows = (data.templates || []).map((x) => ({ ...x, label: x.name, unrec: false }));
  if (data.unrecognized && data.unrecognized.ever_seen > 0) {
    rows.push({ ...data.unrecognized, label: 'unrecognized scripts', revealed_runs: 0, unrec: true });
  }
  section.hidden = false;
  if (!rows.length) {
    host.innerHTML = '<p class="dim">no contract state seen here yet.</p>';
    return;
  }
  const max = Math.max(1, ...rows.map((r) => r.live_states));
  host.innerHTML = rows.map((r) => {
    const w = r.live_states > 0 ? Math.max((r.live_states / max) * 100, 2).toFixed(1) : 0;
    const color = r.unrec ? 'var(--faint)' : templateColor(r.label);
    const bits = [
      `${fmtInt(r.live_states)} live`,
      `${fmtInt(r.ever_seen)} ever`,
      `${fmtInt(r.covenants)} coin${r.covenants === 1 ? '' : 's'}`,
    ];
    if (r.revealed_runs > 0) bits.push(`ran ${fmtInt(r.revealed_runs)}× at spend`);
    const nameTip = GLOSSARY[r.label] ||
      (r.unrec ? 'state scripts kascov doesn\u2019t recognize as a known shape yet — matching never guesses'
        : `a compiled ${r.label} contract, recognized by its instruction skeleton with constructor arguments labeled`);
    const countsTip = `live: unspent right now \u00b7 ever: all state pieces indexed with this shape \u00b7 coins: distinct smart coins` +
      (r.revealed_runs > 0 ? ' \u00b7 ran at spend: hidden programs revealed and hash-verified when spent' : '');
    return `<div class="tpl-row"><span class="tpl-name" title="${esc(nameTip)}">${esc(r.label)}</span>` +
      `<span class="tpl-track" aria-hidden="true"><span class="tpl-fill" style="width:${w}%;background:${color}"></span></span>` +
      `<span class="tpl-counts dim" title="${esc(countsTip)}">${esc(bits.join(' · '))}</span></div>`;
  }).join('');
}

/* ---------------------------------------------------------------- landing */

function emptyCardHtml(network) {
  return `<div class="empty-card">` +
    `<span class="empty-icon" aria-hidden="true">${ICONS.born}</span>` +
    `<h2>${network === 'mainnet' ? 'Mainnet’s' : 'This network’s'} first smart coin hasn’t been born yet.</h2>` +
    `<p>The moment it happens, kascov will be watching — and remembering.</p>` +
    (network === 'mainnet'
      ? `<button type="button" class="btn btn-accent" data-action="network" data-network="testnet-10">meanwhile, watch the testnet</button>`
      : '') +
    `</div>`;
}

/* ----------------------------------------------------------- daily digest */

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

/* count-up to the real number: ~700ms, ease-out cubic, landing exactly on
   the target; reduced motion gets the settled value immediately */
function animateStatN(el, target) {
  if (window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    el.textContent = fmtInt(target);
    return;
  }
  const t0 = performance.now();
  const DUR = 700;
  const tick = (now) => {
    const p = Math.min(1, (now - t0) / DUR);
    const eased = 1 - Math.pow(1 - p, 3);
    el.textContent = p < 1 ? fmtInt(Math.round(target * eased)) : fmtInt(target);
    if (p < 1) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}

function paintDigest(d, network) {
  const host = $('#section-digest');
  if (!host) return; /* stale cached index.html */
  const counts = [
    { n: Number(d.births) || 0, label: 'born', cls: 'n-born' },
    { n: Number(d.moves) || 0, label: 'moved', cls: 'n-move' },
    { n: Number(d.burns) || 0, label: 'retired', cls: 'n-burn' },
  ];
  const digestTips = { born: GLOSSARY.digest_born, moved: GLOSSARY.digest_moved, retired: GLOSSARY.digest_retired };
  $('#digest-counts').innerHTML = counts.map((st) =>
    `<div class="stat" title="${esc(digestTips[st.label] || '')}"><span class="stat-n ${st.cls}" data-n="${st.n}">0</span>` +
    `<span class="stat-label">${esc(st.label)}</span></div>`).join('');
  /* headline cards reuse the grid record when it's here; before the full
     snapshot lands the name falls back to friendlyName (same as liteStoryRow) */
  const entry = state.cache[network];
  const rec = (id) => (entry && entry.index.byId.get(id)) ||
    { c: { covenant_id: id }, name: friendlyName(id) };
  const cards = [];
  if (d.biggest_birth && d.biggest_birth.covenant_id) {
    cards.push(miniCard(rec(d.biggest_birth.covenant_id), network, 'biggest new coin',
      `born holding ${fmtAmount(d.biggest_birth.value || 0, network)}`));
  }
  if (d.busiest && d.busiest.covenant_id) {
    const n = Number(d.busiest.events) || 0;
    cards.push(miniCard(rec(d.busiest.covenant_id), network, 'busiest coin',
      `${fmtInt(n)} life event${n === 1 ? '' : 's'}`));
  }
  const cardHost = $('#digest-cards');
  cardHost.innerHTML = cards.join('');
  cardHost.hidden = cards.length === 0;
  host.hidden = false;
  host.dataset.gen = String(d.generated_at_ms);
  /* the count-up plays once per network per page load; 45s re-renders and
     later repaints set the settled numbers statically */
  const ds = state.digest[network];
  const animate = ds && !ds.animated;
  if (ds) ds.animated = true;
  document.querySelectorAll('#digest-counts .stat-n').forEach((el) => {
    const n = Number(el.dataset.n) || 0;
    if (animate) animateStatN(el, n);
    else el.textContent = fmtInt(n);
  });
}

function renderDigestStrip(network, empty) {
  const host = $('#section-digest');
  if (!host) return; /* stale cached index.html */
  if (empty) { host.hidden = true; return; }
  const ds = state.digest[network];
  if (ds && ds.data) paintDigest(ds.data, network);
  else host.hidden = true; /* nothing for this network yet — no flash */
  loadDigest(network).then((d) => {
    if (!d || state.network !== network || parseRoute().view !== 'landing') return;
    if (host.dataset.gen === String(d.generated_at_ms) && !host.hidden) return;
    paintDigest(d, network);
  });
}

function renderLanding(entry) {
  const network = state.network;
  const { data } = entry;
  const net = NETWORKS[network];

  document.title = 'kascov — watch Kaspa’s smart coins live their lives';
  document.querySelectorAll('[data-net-word]').forEach((el) => { el.textContent = net.word; });

  const s = data.stats;
  $('#hero-stats').innerHTML = [
    { n: s.covenants, label: 'smart coins tracked' },
    { n: s.active, label: 'alive right now' },
    { n: s.events, label: 'life events recorded' },
  ].map((st) => `<div class="stat"><span class="stat-n">${esc(fmtInt(st.n))}</span><span class="stat-label">${esc(st.label)}</span></div>`).join('');

  const bits = [`snapshot from ${relTime(data.generated_at_ms)}`];
  if (s.live_value > 0) {
    bits.push(`together they hold ${fmtAmount(s.live_value, network)}`);
    if (net.unitHint) bits.push(net.unitHint);
  }
  if (!data.__anchor.exact) bits.push('event times are estimates');
  $('#freshness').innerHTML =
    `<span class="live-badge-slot">${liveBadgeHtml(network)}</span> · ${bits.map(esc).join(' · ')}`;

  const empty = data.covenants.length === 0;
  $('#landing-empty').hidden = !empty;
  $('#section-teaser').hidden = empty;
  renderDigestStrip(network, empty);

  if (empty) {
    $('#landing-empty').innerHTML = emptyCardHtml(network);
    return;
  }

  const feed = buildFeed(entry, network);
  $('#teaser-list').innerHTML = feed.events
    .slice(0, TEASER_COUNT)
    .map((ev) => storyRow(ev, entry, feed.live, network))
    .join('');
}

/* --------------------------------------------------------------- explorer */

function renderExplore(entry) {
  const network = state.network;
  const { data } = entry;
  const net = NETWORKS[network];

  document.title = `kascov — exploring ${net.label}`;

  const s = data.stats;
  const bits = [
    `${fmtInt(s.covenants)} smart coin${s.covenants === 1 ? '' : 's'}`,
    `${fmtInt(s.active)} alive`,
    `${fmtInt(s.events)} event${s.events === 1 ? '' : 's'}`,
    `snapshot ${relTimeShort(data.generated_at_ms)}`,
  ];
  if (!data.__anchor.exact) bits.push('times are estimates');
  $('#explore-stats').innerHTML =
    `<span class="live-badge-slot">${liveBadgeHtml(network)}</span> · ${bits.map(esc).join(' · ')}`;

  const empty = data.covenants.length === 0;
  $('#empty-net').hidden = !empty;
  $('#section-pulse').hidden = empty;
  $('#section-stories').hidden = empty;
  $('#section-coins').hidden = empty;

  if (empty) {
    $('#watch-strip').hidden = true;
    $('#section-records').hidden = true;
    const tpl = $('#section-templates');
    if (tpl) tpl.hidden = true;
    $('#empty-net').innerHTML = emptyCardHtml(network);
    return;
  }

  $('#pulse-title').textContent = net.pulseTitle;
  renderWatchStrip(entry, network);
  renderPulse(entry, network);
  renderRecords(entry, network);
  renderTemplates(network);
  renderFamilies(network);
  renderLanes(network);
  renderInscriptions(network);
  renderLifespans(network);
  loadTemplates(network).then(() => {
    if (state.network === network && parseRoute().view === 'explore') renderTemplates(network);
  });
  renderStories(entry, network);
  const sortSel = $('#sort');
  if (sortSel && sortSel.value !== state.sort) sortSel.value = state.sort;
  renderGrid(entry, network);
}

/* ----------------------------------------------------------------- detail */

function timelineItem(entry, ev, data, network, flashTx) {
  const meta = KIND_META[ev.kind] || KIND_META.transition;
  const ms = daaToMs(ev.accepting_daa, data);
  let nerdBits = state.nerd
    ? ` · <span class="mono dim">DAA ${esc(fmtInt(ev.accepting_daa))}</span>`
    : '';
  if (state.nerd && ev.payload) {
    nerdBits += ` · <span class="dim">payload</span> <span class="mono" title="${esc(ev.payload)}">${esc(shortHex(ev.payload, 12, 8))}</span>`;
  } else if (state.nerd && ev.payload_len) {
    nerdBits += ` · <span class="dim">payload ${esc(fmtInt(ev.payload_len))}B</span>`;
  }
  /* KIP-21 user lane: 4-byte app namespace + 16 zero bytes */
  if (ev.payload && ev.payload.length >= 40 && /^0{32}$/.test(ev.payload.slice(8, 40))) {
    nerdBits += ` · <span class="flag flag-tpl" title="KIP-21 based-app lane — this transaction carries app-sequencing data">lane 0x${esc(ev.payload.slice(0, 8))}</span>`;
  }
  /* multi-covenant transactions: this tx moved other coins too */
  let withBits = '';
  if (Array.isArray(ev.with_covenants) && ev.with_covenants.length) {
    withBits = `<p class="tl-with dim">in the same transaction as ` +
      ev.with_covenants.map((id) =>
        `<a href="#/${esc(network)}/c/${esc(id)}" title="${esc(id)}">${esc(friendlyName(id))}</a>`
      ).join(', ') + `</p>`;
  }
  const flash = flashTx && ev.txid === flashTx ? ' tl-flash' : '';
  return `<li class="tl-item ${meta.cls}${flash}" data-txid="${esc(ev.txid)}">` +
    `<span class="tl-icon" title="${esc(GLOSSARY[ev.kind] || '')}">${ICONS[meta.icon]}</span>` +
    `<div class="tl-body">` +
    `<p class="tl-text">${eventSentence(entry, ev, network, true)}</p>` +
    `<p class="tl-meta"><span title="${esc(utcTitle(ms))}">${esc(relTime(ms))}</span> · <span class="abs-t" title="${esc(utcTitle(ms))}">${esc(absShort(ms))}</span> · <a href="${esc(txUrl(network, ev.txid))}" target="_blank" rel="noopener noreferrer">view transaction ↗</a>${nerdBits}</p>` +
    withBits +
    `</div></li>`;
}

const fieldRow = (f) =>
  `<span class="tpl-field"><span class="dim">${esc(f.name)}</span> ` +
  `<span class="mono" title="${esc(f.value)}">${esc(shortHex(f.value, 12, 8))}</span></span>`;
const templateLine = (name, fields) => name
  ? `<p class="tpl-line"><span class="flag flag-tpl">${esc(name)}</span>${(fields || []).map(fieldRow).join('')}</p>`
  : '';

/* client-side reveal preview: a live p2sh-commitment coin shows its
   contract the moment someone who HOLDS the program proves it (blake2b
   match) — no spend needed. Auto-runs from ?program= deep links. */
function verifyProgramAgainstUtxo(u, programHex) {
  if (!window.kascovBlake2b256) return { err: 'hash unavailable in this browser' };
  const bytes = window.kascovDisasm.parseHex(programHex);
  if (!bytes) return { err: 'that is not valid hex' };
  const committed = ((u.state_fields || []).find((f) => f.name === 'program_hash') || {}).value;
  if (!committed) return { err: 'no committed hash on this state' };
  const got = window.kascovDisasm.toHex(window.kascovBlake2b256(bytes));
  if (got !== committed) return { err: 'blake2b mismatch — this is not the committed program' };
  const dec = window.kascovDisasm.disassemble(bytes);
  const tpl = window.kascovDisasm.matchTemplates(dec.instructions, bytes);
  return { ok: true, tpl, hex: window.kascovDisasm.toHex(bytes) };
}

/* Verified Contract — the "verified source" for covenants. Given a program's
   bytes, if it matches a known SilverScript skeleton we (1) show the readable
   canonical source, (2) label the on-chain constructor args, and (3) PROVE it
   by re-emitting from those args and confirming the bytes are byte-identical
   (BLAKE2b). Not "we think this is a Mecenas" — "this provably compiles to
   exactly these bytes." Nobody has a verified-contract layer for Kaspa. */
function verifiedContractHtml(programHex) {
  const D = window.kascovDisasm, G = window.kascovGen;
  if (!D || !G || !programHex) return '';
  const bytes = D.parseHex(programHex);
  if (!bytes) return '';
  const dec = D.disassemble(bytes);
  const tpl = D.matchTemplates(dec.instructions, bytes);
  const info = tpl && D.skeletonInfo(tpl.name);
  if (!tpl || !info || !info.emitVerified || !G.SOURCES[tpl.name]) return '';
  const args = {};
  tpl.fields.forEach((f) => { args[f.name] = Array.from(D.parseHex(f.value)); });
  const emitted = D.emitFromSkeleton(tpl.name, args);
  const identical = !!emitted && D.toHex(emitted) === programHex.toLowerCase();
  const hash = window.kascovBlake2b256 ? D.toHex(window.kascovBlake2b256(bytes)) : '';
  const argRows = info.params.map((p) => {
    const f = tpl.fields.find((x) => x.name === p.name);
    if (!f) return '';
    let val;
    if (p.kind === 'amount') val = G.sompiToTkas(D.snumDecode(Array.from(D.parseHex(f.value)))) + ' TKAS';
    else if (p.kind === 'daa') val = String(D.snumDecode(Array.from(D.parseHex(f.value)))) + ' DAA';
    else val = shortHex(f.value, 8, 6);
    return `<div class="vc-arg"><span class="dim">${esc(p.source)}</span><span class="mono">${esc(val)}</span></div>`;
  }).join('');
  return `<div class="verified-contract">` +
    `<div class="vc-head"><span class="vc-badge">✓ verified contract</span>` +
    `<strong>${esc(tpl.name)}</strong>` +
    (identical ? `<span class="vc-proof" title="the published source re-emits to exactly these on-chain bytes">recompiles byte-identical${hash ? ' · blake2b ' + esc(hash.slice(0, 10)) + '…' : ''} ✓</span>` : '') +
    `</div>` +
    (argRows ? `<div class="vc-args">${argRows}</div>` : '') +
    `<details class="vc-source"><summary>readable SilverScript source</summary>` +
    `<pre class="script">${esc(G.SOURCES[tpl.name])}</pre></details>` +
    `</div>`;
}

/* ---- plain-English contract explainer — turns a recognized template's
   decoded fields into a sentence anyone can read. ---- */
function explainCovenant(tpl) {
  if (!tpl || !tpl.fields) return '';
  const D = window.kascovDisasm, G = window.kascovGen;
  const get = (n) => { const x = tpl.fields.find((f) => f.name === n); return x ? x.value : ''; };
  const shortHex = (v) => (v && v.length > 16 ? v.slice(0, 8) + '…' + v.slice(-4) : v || '?');
  const amount = (hex) => { try { return G.sompiToTkas(D.snumDecode(D.parseHex(hex))) + ' TKAS'; } catch (e) { return shortHex(hex); } };
  const daa = (hex) => { try { return D.snumDecode(D.parseHex(hex)) + ' DAA'; } catch (e) { return shortHex(hex); } };
  const c = (s) => `<code class="mono">${esc(s)}</code>`;
  switch (tpl.name) {
    case 'SilverScript · Escrow':
      return `This is an <strong>escrow</strong>. The funds stay locked until the <strong>arbiter</strong> (key ${c(shortHex(get('arbiter_hash')))}) signs a release — and the contract forces that payout to go to <em>either</em> the buyer (${c(shortHex(get('buyer')))}) or the seller (${c(shortHex(get('seller')))}), the full amount minus the fee. No third address can be paid, and neither party can move it alone. The arbiter decides the outcome but can never touch the money.`;
    case 'SilverScript · Mecenas':
      return `This is a <strong>Mecenas</strong> — a recurring on-chain allowance. The <strong>funder</strong> (key ${c(shortHex(get('funder_hash')))}) funds it; the <strong>recipient</strong> (${c(shortHex(get('recipient')))}) may withdraw up to ${c(amount(get('pledge')))} once every ${c(daa(get('period')))} window, and the funder can reclaim the rest. The coin enforces it — the recipient can’t take more than the pledge per window, and only the funder can cancel.`;
    case 'SilverScript · LastWill':
      return `This is a <strong>LastWill</strong> — a dead-man’s-switch inheritance. Day-to-day the owner spends with a <strong>hot key</strong> (${c(shortHex(get('hot_hash')))}); a <strong>cold key</strong> (${c(shortHex(get('cold_hash')))}) is the backup that can always reclaim and reset the clock. If the owner goes silent long enough, the <strong>heir</strong> (${c(shortHex(get('inheritor_hash')))}) can inherit — but the cold key overrides them, so inheritance only fires on genuine inactivity.`;
    default:
      return '';
  }
}

function explainerPanelHtml(tpl) {
  const html = explainCovenant(tpl);
  if (!html) return '';
  return `<details class="explain-panel" open><summary><span class="explain-badge">📖 in plain English</span></summary><p class="explain-body">${html}</p></details>`;
}

/* ---- covenant security lint — a static audit from the opcodes. No covenant
   linter exists anywhere; this flags the classic gaps from the disassembly. */
function lintCovenant(instructions) {
  const names = new Set(instructions.map((i) => i.name));
  const has = (...ns) => ns.some((n) => names.has(n));
  const sig = has('OpCheckSig', 'OpCheckSigVerify', 'OpCheckSigECDSA', 'OpCheckMultiSig', 'OpCheckMultiSigVerify', 'OpCheckMultiSigECDSA', 'OpCheckSigFromStack', 'OpCheckSigFromStackECDSA');
  const zk = has('OpZkPrecompile');
  const hashlock = has('OpBlake2b', 'OpBlake3', 'OpSHA256') && has('OpEqual', 'OpEqualVerify');
  const timelock = has('OpCheckSequenceVerify', 'OpCheckLockTimeVerify');
  const outSpk = has('OpTxOutputSpk', 'OpTxOutputSpkLen', 'OpTxOutputSpkSubstr');
  const outVal = has('OpTxOutputAmount');
  // covenants can also pin outputs by covenant-id / authorizing-input, not just spk/value
  const outCov = has('OpOutputCovenantId', 'OpCovOutputCount', 'OpCovOutputIdx', 'OpAuthOutputCount', 'OpAuthOutputIdx', 'OpOutputAuthorizingInput');
  const introspection = instructions.some((i) => /^Op(Tx|Cov|Outpoint|Auth)|CovenantId/.test(i.name));
  const opreturn = has('OpReturn');
  const f = [];
  if (sig) f.push(['ok', 'requires a signature', 'a valid signature from the committed key is needed to spend.']);
  else if (zk) f.push(['ok', 'requires a ZK proof', 'spending needs a valid zero-knowledge proof (KIP-16).']);
  else if (hashlock) f.push(['ok', 'gated by a hash preimage', 'spending needs a value that hashes to a committed digest.']);
  else f.push(['high', 'no authentication', 'needs no signature, hash preimage, or ZK proof — anyone who meets its other conditions can spend it.']);
  if (outSpk || outVal || outCov) f.push(['ok', 'constrains its outputs', 'it checks the output destination, amount, or covenant-id — the spender can’t freely redirect the funds.']);
  else if (introspection) f.push(['warn', 'reads the tx but doesn’t pin outputs', 'it inspects the spending transaction but never checks the output destination or amount.']);
  if (timelock) f.push(['ok', 'enforces a timelock', 'a time lock gates at least one spend path.']);
  if (opreturn) f.push(['warn', 'contains OpReturn', 'an always-fail opcode — one branch can never be spent; confirm that’s deliberate.']);
  return f.map(([sev, title, body]) => ({ sev, title, body }));
}

function lintPanelHtml(instructions) {
  if (!instructions || !instructions.length) return '';
  const f = lintCovenant(instructions);
  const highs = f.filter((x) => x.sev === 'high').length;
  const warns = f.filter((x) => x.sev === 'warn').length;
  const head = highs ? `${highs} issue${highs > 1 ? 's' : ''} found` : warns ? `${warns} thing${warns > 1 ? 's' : ''} to check` : 'looks well-formed';
  const rows = f.map((x) => `<div class="lint-row lint-${x.sev}"><span class="lint-dot"></span><div><strong>${esc(x.title)}</strong><span class="dim"> — ${esc(x.body)}</span></div></div>`).join('');
  return `<details class="lint-panel"${highs ? ' open' : ''}><summary><span class="lint-badge">🛡 security check</span> <span class="dim">${head}</span></summary><div class="lint-body">${rows}</div></details>`;
}

/* ---- SilverScript compiler playground (worker shells out to silverc) ---- */
const SILVERSCRIPT_EXAMPLE = {
  source: `pragma silverscript ^0.1.0;

contract Escrow(byte[32] arbiter, pubkey buyer, pubkey seller) {
    entrypoint function spend(pubkey pk, sig s) {
        require(blake2b(pk) == arbiter);
        require(checkSig(s, pk));

        int minerFee = 1000;
        int amount = tx.inputs[this.activeInputIndex].value - minerFee;
        require(tx.outputs[0].value == amount);

        byte[34] buyerLock = new ScriptPubKeyP2PK(buyer);
        byte[34] sellerLock = new ScriptPubKeyP2PK(seller);
        bool sendsToBuyer = tx.outputs[0].scriptPubKey == byte[](buyerLock);
        bool sendsToSeller = tx.outputs[0].scriptPubKey == byte[](sellerLock);
        require(sendsToBuyer || sendsToSeller);
    }
}`,
  args: `0x${'33'.repeat(32)}\n0x${'11'.repeat(32)}\n0x${'22'.repeat(32)}`,
};

function initCompiler() {
  const src = $('#compiler-src');
  const args = $('#compiler-args');
  if (src && !src.value) src.value = SILVERSCRIPT_EXAMPLE.source;
  if (args && !args.value) args.value = SILVERSCRIPT_EXAMPLE.args;
}

let lastCompiled = null;
function renderCompileResult(d) {
  const out = $('#compiler-result');
  if (!out) return;
  if (!d || !d.ok) {
    lastCompiled = null;
    out.innerHTML = `<div class="compile-err">✗ ${esc((d && d.error) || 'compile failed')}</div>`;
    return;
  }
  lastCompiled = d.hex;
  const D = window.kascovDisasm;
  let decoded = '';
  const bytes = D && D.parseHex(d.hex);
  if (bytes) {
    const tpl = D.matchTemplates(D.disassemble(bytes).instructions, bytes);
    if (tpl) decoded = `<div class="compile-tpl">✓ recognized as <strong>${esc(tpl.name)}</strong></div>`;
  }
  out.innerHTML = `<div class="compile-ok">✓ compiled — ${d.hex.length / 2} bytes of Kaspa script</div>` +
    `<pre class="compile-hex script" data-copy="${esc(d.hex)}">${esc(d.hex)}</pre>` +
    decoded +
    `<div class="compile-actions"><a class="btn" href="#/decode?s=${esc(d.hex)}">▶ decode / debug it</a>` +
    `<button type="button" class="chip" data-action="compile-publish">publish as verified source</button>` +
    `<span class="dim"> then deploy with <code class="mono">kascov-lab deploy --program-hex …</code></span></div>` +
    `<div id="publish-result" class="compile-publish-result"></div>`;
}

/* community verify-and-publish: if a decoded program's hash has a published
   SilverScript source, show it. Async, injected after the decode renders. */
function checkVerifiedRegistry(hex) {
  const host = document.getElementById('registry-panel');
  if (!host || !hex || !window.kascovBlake2b256) return;
  const bytes = window.kascovDisasm.parseHex(hex);
  if (!bytes) return;
  const hash = window.kascovDisasm.toHex(window.kascovBlake2b256(bytes));
  fetch(`data/${state.network}/verified/${hash}`, { cache: 'no-cache' })
    .then((r) => r.json())
    .then((d) => {
      if (!d || !d.ok || !d.source) return;
      host.innerHTML = `<details class="verified-contract registry-src"><summary><span class="vc-badge">✓ community-verified source</span>` +
        `<strong>${esc(d.template || 'published SilverScript')}</strong>` +
        `<span class="vc-proof">compiles byte-identical to this program ✓</span></summary>` +
        `<pre class="script">${esc(d.source)}</pre></details>`;
    })
    .catch(() => {});
}

/* ---- in-browser spend simulation (worker runs the real script engine) ---- */
const SIM_VALUE = 100_000_000; // simulate on a 1 TKAS coin
const SIM_SCENARIOS = {
  'SilverScript · Escrow': [
    { label: 'arbiter releases to buyer', entrypoint: 'spend', recipient: 'buyer' },
    { label: 'arbiter releases to seller', entrypoint: 'spend', recipient: 'seller' },
    { label: 'arbiter sends it elsewhere', entrypoint: 'spend', recipient: 'other' },
    { label: 'arbiter skims 5000 sompi', entrypoint: 'spend', recipient: 'buyer', amount: SIM_VALUE - 6000 },
  ],
  'SilverScript · Mecenas': [
    { label: 'funder reclaims the pledge', entrypoint: 'reclaim', recipient: 'self' },
  ],
  'SilverScript · LastWill': [
    { label: 'owner spends with the cold key', entrypoint: 'cold', recipient: 'self' },
    { label: 'heir inherits', entrypoint: 'inherit', recipient: 'self' },
  ],
};

function simulatePanelHtml(tpl, hex) {
  if (!tpl || !SIM_SCENARIOS[tpl.name] || !hex) return '';
  const chips = SIM_SCENARIOS[tpl.name]
    .map((s, i) => `<button type="button" class="sim-chip" data-action="sim-run" data-i="${i}">${esc(s.label)}</button>`)
    .join('');
  return `<details class="sim-panel" data-hex="${esc(hex)}" data-tpl="${esc(tpl.name)}">` +
    `<summary class="sim-head"><span class="sim-badge">▶ simulate</span> <strong>try a spend — without broadcasting</strong></summary>` +
    `<p class="sim-sub dim">each runs through Kaspa&rsquo;s real script engine and reports what a node would decide.</p>` +
    `<div class="sim-chips">${chips}</div>` +
    `<div class="sim-result"></div></details>`;
}

let lastSimTrace = null;
function simVerdictHtml(d) {
  if (!d || !d.ok) return `<div class="sim-verdict sim-na">can&rsquo;t simulate — ${esc((d && d.verdict) || 'unknown')}</div>`;
  const cls = d.pass ? 'sim-pass' : 'sim-fail';
  const icon = d.pass ? '✓ PASS' : '✗ FAIL';
  const rule = !d.pass && d.rule ? `<div class="sim-rule">↳ ${esc(d.rule)}</div>` : '';
  const traceBtn = d.trace && d.trace.length
    ? `<button type="button" class="btn dbg-btn sim-trace-btn" data-action="sim-trace">⧉ step through this run</button>`
    : '';
  return `<div class="sim-verdict ${cls}"><span class="sim-icon">${icon}</span><span>${esc(d.verdict)}</span></div>` +
    rule + `<p class="sim-note dim">${esc(d.note)}</p>` + traceBtn;
}

/* open the debugger on a CONCRETE engine trace (from a simulate run) — real
   stacks, real control flow, vs the symbolic decode-page trace */
function openSimTrace(trace) {
  if (!trace || !trace.length) return;
  const short = (h) => (h && h.length > 18 ? h.slice(0, 10) + '…' + h.slice(-4) : h);
  dbg = {
    concrete: true,
    i: 0,
    steps: trace.map((s, i) => ({
      offset: i,
      name: s.op.split(' ')[0],
      note: s.op.includes(' ') ? s.op.slice(s.op.indexOf(' ') + 1) : '',
      group: 'standard',
      indent: 0,
      dstack: (s.dstack || []).map(short),
      astack: (s.astack || []).map(short),
    })),
  };
  renderDebugger();
  const host = document.getElementById('dbg-panel');
  if (host) host.scrollIntoView({ block: 'start', behavior: 'smooth' });
}

/* KIP-16 ZK-app detection. A covenant that calls OpZkPrecompile verifies a
   zero-knowledge proof ON-CHAIN — Kaspa's precompile checks BOTH Groth16
   (verifier tag 0x20) and RISC Zero zkVM (tag 0x21) proofs. No explorer in
   any ecosystem surfaces on-chain ZK verification; this is the flagship. */
function zkInfo(instructions) {
  const idx = instructions.findIndex((i) => i.name === 'OpZkPrecompile');
  if (idx < 0) return null;
  // the verifier tag is a 1-byte push (0x20/0x21) among the args feeding the op
  let tag = null;
  for (let j = idx - 1; j >= 0 && j >= idx - 6; j--) {
    const d = instructions[j].data;
    if (d && d.length === 1 && (d[0] === 0x20 || d[0] === 0x21)) { tag = d[0]; break; }
  }
  const system = tag === 0x20 ? 'Groth16' : tag === 0x21 ? 'RISC Zero (zkVM)' : 'a zero-knowledge proof';
  // count how many pushes precede the op (proof / verification key / public inputs)
  const pushesBefore = instructions.slice(0, idx).filter((i) => i.group === 'push' && i.data && i.data.length > 2).length;
  return { system, tag, pushesBefore };
}

function zkPanelHtml(instructions, hex) {
  const z = zkInfo(instructions);
  if (!z) return '';
  // a self-contained proof script (public inputs + proof + vk + OpZkPrecompile,
  // no transaction introspection) can be verified live here; a covenant that
  // only *expects* a spend-time proof can't be (no proof present).
  const selfContained = hex && !instructions.some((i) => /^Op(Tx|Cov|Outpoint|Auth)|CovenantId/.test(i.name));
  const verify = selfContained
    ? `<div class="zk-verify"><button type="button" class="btn zk-verify-btn" data-action="zk-verify" data-prog="${esc(hex)}">◆ verify the proof</button><span class="zk-verify-result"></span></div>`
    : '';
  return `<div class="zk-panel">` +
    `<div class="zk-head"><span class="zk-badge">⬡ ZK ${selfContained ? 'proof' : 'covenant'}</span>` +
    `<strong>on-chain zero-knowledge verification</strong>` +
    `<span class="zk-sys">${esc(z.system)}</span></div>` +
    `<p class="zk-desc">${selfContained
      ? `A self-contained <code class="mono">${esc(z.system)}</code> proof (public inputs + proof + verifying key + <code class="mono">OpZkPrecompile</code>). Verify it below — kascov runs the <em>exact</em> verifier Kaspa's L1 uses.`
      : `This contract calls <code class="mono">OpZkPrecompile</code> (KIP-16) — Kaspa's L1 verifies a ${esc(z.system)} proof <em>inside the script</em>, so the coin only moves if a valid zero-knowledge proof is supplied. Verified computation, settled on a ~10-blocks/sec BlockDAG.`}</p>` +
    verify +
    `</div>`;
}

/* ---- the visual script debugger (symbolic stepper) ---- */
let dbg = null; // { steps, i }

function debugCtaHtml(bytes) {
  if (!window.kascovVm) return '';
  const hex = window.kascovDisasm.toHex(bytes);
  return `<div class="dbg-cta">` +
    `<button type="button" class="btn dbg-open-btn" data-action="dbg-open" data-prog="${esc(hex)}">⧉ step through the stack</button>` +
    `<span class="dim"> — watch the contract's logic run, opcode by opcode</span></div>` +
    `<div id="dbg-panel"></div>`;
}

function openDebugger(hex) {
  const D = window.kascovDisasm;
  const bytes = D.parseHex(hex);
  if (!bytes || !window.kascovVm) return;
  const dec = D.disassemble(bytes);
  dbg = { steps: window.kascovVm.symbolicTrace(dec.instructions), i: 0 };
  renderDebugger();
}

function dbgStep(delta, abs) {
  if (!dbg) return;
  dbg.i = abs != null ? abs : dbg.i + delta;
  dbg.i = Math.max(0, Math.min(dbg.steps.length - 1, dbg.i));
  renderDebugger();
}

function renderDebugger() {
  const host = document.getElementById('dbg-panel');
  if (!host || !dbg) return;
  const n = dbg.steps.length;
  const i = Math.max(0, Math.min(n - 1, dbg.i));
  const s = dbg.steps[i];
  const col = (title, arr) => `<div class="dbg-col"><div class="dbg-col-h">${title} <span class="dim">${arr.length}</span></div>` +
    (arr.length ? arr.slice().reverse().map((x, k) => `<div class="dbg-item${k === 0 ? ' dbg-top' : ''}">${esc(x)}</div>`).join('') : '<div class="dbg-empty">empty</div>') +
    `</div>`;
  const ops = dbg.steps.map((st, k) =>
    `<div class="dbg-op${k === i ? ' dbg-active' : ''}" style="padding-left:${(0.6 + st.indent * 0.9).toFixed(2)}rem" data-action="dbg-seek" data-i="${k}">` +
    `<span class="dbg-op-off">${st.offset.toString(16).padStart(4, '0')}</span>` +
    `<span class="dbg-op-name g-${esc(st.group)}">${esc(st.name)}</span></div>`).join('');
  host.innerHTML = `<div class="dbg">` +
    `<div class="dbg-controls">` +
    `<button type="button" class="btn dbg-btn" data-action="dbg-prev"${i === 0 ? ' disabled' : ''}>◀</button>` +
    `<input type="range" class="dbg-slider" min="0" max="${n - 1}" value="${i}" data-action="dbg-slider" aria-label="step">` +
    `<button type="button" class="btn dbg-btn" data-action="dbg-next"${i === n - 1 ? ' disabled' : ''}>▶</button>` +
    `<span class="dbg-count mono">step ${i + 1} / ${n}</span>` +
    `<button type="button" class="btn dbg-close" data-action="dbg-close">close</button></div>` +
    `<div class="dbg-now"><span class="dbg-now-op mono">${esc(s.name)}</span> <span class="dbg-now-note">${esc(s.note)}</span></div>` +
    `<div class="dbg-stacks">${col('data stack', s.dstack)}${col('alt stack', s.astack)}</div>` +
    `<div class="dbg-oplist">${ops}</div>` +
    `<p class="dim dbg-footnote">` +
    (dbg.concrete
      ? 'concrete trace — the real engine&rsquo;s stacks for this simulated spend, opcode by opcode, following the actual control flow'
      : 'symbolic trace — concrete for pushes &amp; stack ops, ‹symbolic› where a value only resolves against a real spend') +
    `</p>` +
    `</div>`;
  // scroll the active op into view WITHIN the op-list only — never the page
  // (scrollIntoView would bubble up and jump the whole document)
  const list = host.querySelector('.dbg-oplist');
  const act = host.querySelector('.dbg-active');
  if (list && act) list.scrollTop = act.offsetTop - list.clientHeight / 2 + act.offsetHeight / 2;
}

function revealPreviewHtml(u, program) {
  if (u.template !== 'p2sh commitment' || u.revealed_asm || !u.live) return '';
  let result = '';
  if (program) {
    const v = verifyProgramAgainstUtxo(u, program);
    if (v.ok) {
      result = `<p class="gen-verify-ok">✓ hash-verified — this coin commits to ` +
        `<strong>${esc(v.tpl ? v.tpl.name : 'an unrecognized program')}</strong></p>` +
        verifiedContractHtml(v.hex) +
        (v.tpl ? '' : templateLine(v.tpl ? v.tpl.name : '', v.tpl ? v.tpl.fields : [])) +
        `<a class="decode-open" href="#/decode?s=${esc(v.hex)}">open the program in the decoder →</a>`;
    } else if (program) {
      result = `<p class="gen-err">${esc(v.err)}</p>`;
    }
  }
  return `<div class="reveal-preview" data-outpoint="${esc(u.outpoint)}">` +
    (result || `<p class="dim reveal-hint" title="${esc(GLOSSARY['p2sh commitment'] || '')}">know the program behind this hash? paste it to preview the contract (nothing leaves your browser):</p>` +
      `<div class="reveal-row"><input type="text" class="reveal-input" placeholder="program hex…" spellcheck="false">` +
      `<button type="button" class="btn" data-action="reveal-check">verify</button></div>`) +
    `</div>`;
}

function nerdPanel(entry, network, program) {
  const c = entry.c;
  const rows = [
    ['covenant id', `<span class="mono break">${esc(c.covenant_id)}</span> <button type="button" class="copy-btn" data-action="copy" data-copy="${esc(c.covenant_id)}">copy</button>`],
    ['genesis txid', c.genesis_txid
      ? `<a class="mono break" href="${esc(txUrl(network, c.genesis_txid))}" target="_blank" rel="noopener noreferrer">${esc(c.genesis_txid)}</a> <button type="button" class="copy-btn" data-action="copy" data-copy="${esc(c.genesis_txid)}">copy</button>`
      : '<span class="dim">unknown — genesis happened before indexing began</span>'],
    ['genesis DAA', c.genesis_daa != null ? `<span class="mono">${esc(fmtInt(c.genesis_daa))}</span>` : '<span class="dim">unknown</span>'],
    ['last activity DAA', `<span class="mono">${esc(fmtInt(c.last_activity_daa))}</span>`],
    ['lineage complete', c.lineage_complete ? '<span class="flag flag-yes">yes</span>' : '<span class="flag flag-no">no — earlier history is missing</span>'],
    ['events indexed', `<span class="mono">${esc(fmtInt(c.event_count))}</span>${c.events_truncated ? ' <span class="flag flag-no">truncated</span>' : ''}`],
    ['live UTXOs', `<span class="mono">${esc(fmtInt(c.live_utxos))}</span> holding <span class="mono">${esc(fmtAmount(c.live_value, network))}</span>`],
  ];
  const allUtxos = c.utxos || [];
  const foldUtxos = allUtxos.length > UTXO_WINDOW + 4 && !state.utxoAll;
  const shownUtxos = foldUtxos ? allUtxos.slice(0, UTXO_WINDOW) : allUtxos;
  const utxoFoot = allUtxos.length > UTXO_WINDOW + 4
    ? `<button type="button" class="btn btn-expand" data-action="utxo-all">` +
      (foldUtxos ? `show all ${fmtInt(allUtxos.length)} UTXOs ↓` : 'collapse UTXOs ↑') +
      `</button>`
    : '';
  const utxos = shownUtxos.map((u) => {
    const badges = [
      u.live ? '<span class="flag flag-yes">live</span>' : '<span class="flag flag-off">spent</span>',
      u.uses_covenant_ops ? '<span class="flag flag-ops">covenant ops</span>' : '',
      u.uses_zk_ops ? '<span class="flag flag-ops">zk ops</span>' : '',
      u.revealed_uses_covenant_ops ? '<span class="flag flag-ops">ran covenant ops</span>' : '',
      u.revealed_uses_zk_ops ? '<span class="flag flag-ops">ran zk ops</span>' : '',
    ].filter(Boolean).join(' ');
    let reveal = '';
    if (u.revealed_asm) {
      reveal = `<p class="reveal-label">revealed at spend — the program this state actually ran` +
        (u.spent_txid ? ` <a href="${esc(txUrl(network, u.spent_txid))}" target="_blank" rel="noopener noreferrer">(tx ↗)</a>` : '') +
        `:</p>` +
        (verifiedContractHtml(u.revealed_hex) || templateLine(u.revealed_template, u.revealed_fields)) +
        (u.revealed_hex ? zkPanelHtml(window.kascovDisasm.disassemble(window.kascovDisasm.parseHex(u.revealed_hex) || []).instructions, u.revealed_hex) : '') +
        `<pre class="script script-reveal">${esc(u.revealed_asm.join('\n'))}</pre>` +
        (u.revealed_hex ? `<a class="decode-open" href="#/decode?s=${esc(u.revealed_hex)}">open revealed program in decoder →</a>` : '');
    } else if (u.sig_hex || u.spent_txid) {
      const bits = [];
      if (u.sig_hex) {
        bits.push(`spend signature <span class="mono">${esc(shortHex(u.sig_hex, 10, 6))}</span> (${u.sig_hex.length / 2}B)`);
      } else if (u.sig_len) {
        bits.push(`spend script: ${esc(fmtInt(u.sig_len))}B (too large to inline)`);
      }
      if (u.spent_txid) {
        bits.push(`spent by <a href="${esc(txUrl(network, u.spent_txid))}" target="_blank" rel="noopener noreferrer">tx ${esc(shortHex(u.spent_txid, 8, 6))} ↗</a>`);
      }
      if (bits.length) reveal = `<p class="spend-note dim">${bits.join(' · ')}</p>`;
    }
    return `<div class="utxo">` +
      `<div class="utxo-head"><span class="mono break">${esc(u.outpoint)}</span><span class="utxo-flags">${badges}</span></div>` +
      `<div class="utxo-meta"><span>${esc(fmtAmount(u.value, network))}</span><span class="dim">created at DAA ${esc(fmtInt(u.created_daa))}</span>` +
      (u.spent_budget != null ? `<span class="dim">spent with budget ${esc(fmtInt(u.spent_budget))}</span>` : '') +
      `</div>` +
      templateLine(u.template, u.state_fields) +
      `<pre class="script">${esc((u.script_asm || []).join('\n'))}</pre>` +
      (u.script_hex ? `<a class="decode-open" href="#/decode?s=${esc(u.script_hex)}">open in decoder →</a>` : '') +
      reveal +
      revealPreviewHtml(u, program) +
      `</div>`;
  }).join('');
  return `<dl class="nerd-rows">${rows.map(([k, v]) => `<div class="nerd-row"><dt>${esc(k)}</dt><dd>${v}</dd></div>`).join('')}</dl>` +
    `<h3 class="nerd-h">UTXOs (${allUtxos.length})</h3>` +
    (utxos || '<p class="dim">no UTXOs recorded.</p>') +
    utxoFoot;
}

/* long coins fold: show a window of events/UTXOs with expanders */
const STORY_WINDOW = 8;
const UTXO_WINDOW = 8;

function renderDetail(entry, covId, flashTx, program) {
  const network = state.network;
  const { data, index } = entry;
  const view = $('#view-detail');
  const gridRec = index.byId.get(covId);
  if (state.detailId !== covId) {
    state.detailId = covId;
    state.storyAll = false;
    state.utxoAll = false;
  }

  if (!gridRec) {
    document.title = 'smart coin not found — kascov';
    const other = network === 'mainnet' ? 'testnet-10' : 'mainnet';
    view.innerHTML = `<a class="back" href="#/explore">← all smart coins</a>` +
      `<div class="empty-card"><h2>We haven’t met this smart coin.</h2>` +
      `<p class="dim">It isn’t in the ${esc(NETWORKS[network].label)} snapshot — it may live on the other network, or the id might be mistyped.</p>` +
      `<button type="button" class="btn" data-action="network" data-network="${other}">look on ${other}</button></div>`;
    return;
  }

  /* the life story and scripts come from the per-coin detail endpoint —
     paint the header from the grid row instantly, fill in when it lands */
  const detMap = state.details[network];
  const rec = detMap && detMap.get(covId);
  if (!rec) {
    const alive0 = gridRec.c.status === 'active';
    const watched0 = state.watch.has(covId);
    document.title = `${gridRec.name} — kascov`;
    const bits = [];
    bits.push(`${gridRec.c.genesis_daa != null ? 'born' : 'first seen'} ${relTime(gridRec.bornMs)}`);
    bits.push(gridRec.moves === 0 ? 'never moved' : gridRec.moves === 1 ? 'moved once' : `moved ${gridRec.moves} times`);
    if (alive0) bits.push(`currently holds ${fmtAmount(gridRec.c.live_value, network)}`);
    else bits.push(`retired ${relTime(gridRec.lastMs)}`);
    view.innerHTML =
      `<a class="back" href="#/explore">← all smart coins</a>` +
      `<header class="detail-head">` +
      `<span role="img" aria-label="avatar of ${esc(gridRec.name)}">${avatarSvg(covId, 88)}</span>` +
      `<div class="detail-id"><h1>${esc(gridRec.name)}</h1>` +
      `<p class="detail-tags"><span class="pill ${alive0 ? 'pill-alive' : 'pill-retired'}" title="${esc(alive0 ? GLOSSARY.alive : GLOSSARY.retired)}">${alive0 ? 'alive' : 'retired'}</span>` +
      `<button type="button" class="star${watched0 ? ' starred' : ''}" data-action="watch" data-id="${esc(covId)}" aria-pressed="${watched0}" aria-label="watch this coin">★</button>` +
      `<span class="dim">smart coin on ${esc(NETWORKS[network].label)}</span></p>` +
      `<p class="id-chip"><span class="mono">${esc(shortHex(covId, 10, 8))}</span>` +
      `<button type="button" class="copy-btn" data-action="copy" data-copy="${esc(covId)}" aria-label="copy this coin’s full id">copy id</button></p>` +
      `</div></header>` +
      `<p class="detail-summary">${esc(bits.join(' · '))}.</p>` +
      `<section aria-label="Life story"><h2>life story</h2>` +
      `<p class="dim">reading this coin’s full story…</p></section>`;
    loadDetail(network, covId)
      .then(() => {
        if (state.network === network && state.detailId === covId && parseRoute().view === 'detail') {
          renderDetail(entry, covId, flashTx, program);
        }
      })
      .catch(() => {
        const story = view.querySelector('section[aria-label="Life story"]');
        if (story && state.detailId === covId) {
          story.innerHTML = `<h2>life story</h2><p class="dim">couldn’t load this coin’s story.</p>` +
            `<button type="button" class="btn" data-action="retry-detail">try again</button>`;
        }
      });
    return;
  }

  const c = rec.c;
  const alive = c.status === 'active';
  const watched = state.watch.has(c.covenant_id);
  /* surface recognized contract templates (but not the ubiquitous p2pk/p2sh shapes) */
  const namedTemplate = (c.utxos || [])
    .map((u) => u.revealed_template || u.template)
    .find((t) => t && !/^p2(pk|sh)/.test(t));
  document.title = `${rec.name} — kascov`;

  const summaryBits = [];
  summaryBits.push(`${c.genesis_daa != null ? 'born' : 'first seen'} ${relTime(rec.bornMs)} (${absShort(rec.bornMs)})`);
  summaryBits.push(rec.moves === 0 ? 'never moved' : rec.moves === 1 ? 'moved once' : `moved ${rec.moves} times`);
  if (alive) {
    summaryBits.push(`currently holds ${fmtAmount(c.live_value, network)}${c.live_utxos > 1 ? ` in ${c.live_utxos} pieces` : ''}`);
  } else {
    summaryBits.push(`retired ${relTime(rec.lastMs)} (${absShort(rec.lastMs)})`);
  }

  const preface = c.genesis_txid == null
    ? `<li class="tl-item tl-note"><span class="tl-icon" aria-hidden="true">${ICONS.move}</span><div class="tl-body">` +
      `<p class="tl-text dim">first seen mid-life — its earlier story happened before we started watching</p></div></li>`
    : '';
  const truncNote = c.events_truncated
    ? `<p class="dim trunc-note">part of this coin’s story is missing — it had more events than we keep per coin.</p>`
    : '';

  /* long life stories fold to a window; a highlighted event beyond the
     fold auto-expands so ?tx= deep links always land */
  const events = c.events;
  if (flashTx && !state.storyAll) {
    const at = events.findIndex((ev) => ev.txid === flashTx);
    if (at >= STORY_WINDOW - 1) state.storyAll = true;
  }
  const foldStory = events.length > STORY_WINDOW + 4 && !state.storyAll;
  const shownEvents = foldStory ? events.slice(0, STORY_WINDOW) : events;
  const storyFoot = events.length > STORY_WINDOW + 4
    ? `<button type="button" class="btn btn-expand" data-action="story-all">` +
      (foldStory
        ? `show all ${fmtInt(events.length)} events ↓`
        : 'collapse the story ↑') +
      `</button>`
    : '';

  view.innerHTML =
    `<a class="back" href="#/explore">← all smart coins</a>` +
    `<header class="detail-head">` +
    `<span role="img" aria-label="avatar of ${esc(rec.name)}">${avatarSvg(c.covenant_id, 88)}</span>` +
    `<div class="detail-id">` +
    `<h1>${esc(rec.name)}</h1>` +
    `<p class="detail-tags"><span class="pill ${alive ? 'pill-alive' : 'pill-retired'}">${alive ? 'alive' : 'retired'}</span>` +
    `<button type="button" class="star${watched ? ' starred' : ''}" data-action="watch" data-id="${esc(c.covenant_id)}"` +
    ` aria-pressed="${watched}" aria-label="${watched ? 'stop watching' : 'watch'} this coin">★</button>` +
    (namedTemplate ? `<span class="flag flag-tpl">${esc(namedTemplate)}</span>` : '') +
    `<span class="dim">smart coin on ${esc(NETWORKS[network].label)}</span></p>` +
    `<p class="id-chip"><span class="mono">${esc(shortHex(c.covenant_id, 10, 8))}</span>` +
    `<button type="button" class="copy-btn" data-action="copy" data-copy="${esc(c.covenant_id)}" aria-label="copy this coin’s full id">copy id</button></p>` +
    `</div></header>` +
    `<p class="detail-summary">${esc(summaryBits.join(' · '))}.</p>` +
    `<section aria-label="Life story"><h2>life story</h2>${truncNote}` +
    `<ol class="timeline">${preface}${shownEvents.map((ev) => timelineItem(rec, ev, data, network, flashTx)).join('')}</ol>${storyFoot}</section>` +
    `<section class="nerd" aria-label="Technical details">` +
    `<button type="button" class="nerd-toggle" data-action="nerd" aria-expanded="${state.nerd}">` +
    `<span class="nerd-switch" aria-hidden="true"></span><span>nerd mode</span>` +
    `<span class="dim nerd-hint">raw ids, DAA scores, UTXOs &amp; scripts</span></button>` +
    `<div id="nerd-panel" class="nerd-panel" ${state.nerd ? '' : 'hidden'}>${state.nerd ? nerdPanel(rec, network, program) : ''}</div>` +
    `</section>`;

  if (flashTx) {
    /* after render()'s scroll-to-top settles, bring the flashed event into view */
    setTimeout(() => {
      const el = view.querySelector(`.tl-item[data-txid="${flashTx}"]`);
      if (el) el.scrollIntoView({ block: 'center', behavior: 'smooth' });
    }, 120);
  }
}

/* ---------------------------------------------------------------- decoder */

/* the example gallery: protocol shapes, the three compiled SilverScript
   contracts (real silverc output), and the real mainnet ZK covenant's
   revealed program */
const DECODE_EXAMPLES = {
  p2pk: '20' + 'a3'.repeat(32) + 'ac',
  p2sh: 'aa20' + 'c5'.repeat(32) + '87',
  guard: 'b9cf20' + '11'.repeat(32) + '8851',
  groth: '20c07a65145c3cb48b6101962ea607a4dd93c753bb26975cb47feb00d3666e440420d223ffcb21c6ffcb7c8f60392ca49dde0000000000000000000000000000000020a95ac0b37bfedcd8136e6c1143086bf50000000000000000000000000000000020dbe7c0194edfcc37eb4d422a998c1f560000000000000000000000000000000020a54dc85ac99f851c92d7c96d7318af4100000000000000000000000000000000554c80570253c0c483a1b16460118e63c155f3684e784ae7d97e8fc3f544128b37fe15075eab5ac31150c8a44253d8525971241bbd7227fcefbae2db4ae71675c56a2e0eb9235136b15ab72f16e707832f3d6ae5b0ba7cca53ae17cb52b3201919eb9d908c16297abd90aa7e00267bc21a9a78116e717d4d76edd44e21cca17e3d592d4da801e2f26dbea299f5223b646cb1fb33eadb059d9407559d7441dfd902e3a79a4d2dabb73dc17fbc13021e2471e0c08bd67d8401f52b73d6d07483794cad4778180e0c06f33bbc4c79a9cadef253a68084d382f17788f885c9afd176f7cb2f036789edf692d95cbdde46ddda5ef7d422436779445c5e66006a42761e1f12efde0018c212f3aeb785e49712e7a9353349aaf1255dfb31b7bf60723a480d9293938e1933033e7fea1f40604eaacf699d4be9aacc577054a0db22d9129a1728ff85a01a1c3af829b62bf4914c0bcf2c81a4bd577190eff5f194ee9bac95faefd53cb0030600000000000000e43bdc655d0f9d730535554d9caa611ddd152c081a06a932a8e1d5dc259aac123f42a188f683d869873ccc4c119442e57b056e03e2fa92f2028c97bc20b9078747c30f85444697fdf436e348711c011115963f855197243e4b39e6cbe236ca8ba7f2042e11f9255afbb6c6e2c3accb88e401f2aac21c097c92b3fbdb99f98a9b0dcd6c075ada6ed0ddfece1d4a2d005f61a7d5df0b75c18a5b2374d64e495fab93d4c4b1200394d5253cce2f25a59b862ee8e4cd43686603faa09d5d0d3c1c8f0120a6',
  zk: '08b1762f000000000075088b1e466a00000000756320901be291efb290173ae8c021842fad986e73b878bff72d3405821b7ed0136270d0519d00796001307f20dcbe0edd8a2b405aabdead896b04ae82cd9a881df095fee9805fd5584068a9b888007900587f51080100000000000010a569007958607fb9b9c976022901947c02210194bca2690108517900587f7e0275087e517958607f7e01757eb9b9c976022001947cbc7eb9cf76d0519dd2519daa01877e02aa207c7e0200007c7e00c38800c2b9be0340420f94a269a8200f3756c052ff1749fbbe0d4b28010a42c989e227130752e7188047498ba124aa207a8f24092c34ed3eb81b3d0a0b796c588c615d3488ef9e61c21dbd1e4b83ea6e01010121a6695167b9cf76d0519d76d2519d00d376c3b9bf88c2b9be0340420f94a2695168',
};
/* the SilverScript instances come from disasm.js's embedded compiler dumps */
for (const d of (window.kascovDisasm.SS_DUMPS || [])) {
  if (d.name.includes('Mecenas')) DECODE_EXAMPLES.mecenas = d.a;
  else if (d.name.includes('Escrow')) DECODE_EXAMPLES.escrow = d.a;
  else if (d.name.includes('LastWill')) DECODE_EXAMPLES.lastwill = d.a;
}

/* long-script ergonomics: window the output, collapse the input, download */
const DECODE_WINDOW = 200;
const DECODE_COLLAPSE_INPUT = 2000; /* hex chars */
const DECODE_SHARE_MAX = 8192;
let decodeShowAll = false;
let lastDecodeKey = '';

/* ------------------------ contract generator ("make this yours") -------- */

/* open state + the user's edits; reset whenever the pasted script changes */
let genState = null;

function genCta(tpl) {
  if (!tpl || !tpl.name.startsWith('SilverScript · ')) return '';
  const info = window.kascovDisasm.skeletonInfo(tpl.name);
  if (!info || !info.emitVerified) return ''; /* generator only offers itself when emit is proven */
  const open = genState && genState.open;
  return `<p class="gen-cta-row"><button type="button" class="btn btn-accent gen-cta" data-action="gen-toggle">` +
    `${open ? 'close the generator ↑' : 'make this yours →'}</button>` +
    `<span class="dim gen-cta-hint">edit the parameters, get your own deployable contract</span></p>`;
}

function genPanelHtml(tpl, bytes) {
  if (!tpl || !genState || !genState.open) return '';
  const info = window.kascovDisasm.skeletonInfo(tpl.name);
  if (!info) return '';
  const decoded = new Map(tpl.fields.map((f) => [f.name, f.value]));
  if (!genState.values) {
    genState.values = {};
    for (const p of info.params) {
      genState.values[p.name] = window.kascovGen.prefillFor(p.kind, decoded.get(p.name) || '');
    }
    genState.coinValue = '10';
    genState.sourceHex = window.kascovDisasm.toHex(bytes);
  }
  const kindLabel = { pubkey: 'x-only pubkey · 32 bytes hex', hash32: 'blake2b-256 · 32 bytes hex', amount: 'TKAS', daa: 'DAA ticks' };
  const fields = info.params.map((p) => {
    const v = genState.values[p.name] || '';
    const check = window.kascovGen.validateField(p.kind, v);
    return `<label class="gen-field">` +
      `<span class="gen-label">${esc(p.source)} <span class="dim">(${esc(kindLabel[p.kind] || p.kind)})</span></span>` +
      `<input type="text" data-gen-field="${esc(p.name)}" value="${esc(v)}" spellcheck="false" autocomplete="off">` +
      `<span class="gen-hint dim">${esc(p.hint || '')}</span>` +
      (check.ok ? '' : `<span class="gen-err">${esc(check.err)}</span>`) +
      `</label>`;
  }).join('');
  const valueField = `<label class="gen-field">` +
    `<span class="gen-label">coin value <span class="dim">(TKAS the newborn coin holds)</span></span>` +
    `<input type="text" data-gen-field="__value" value="${esc(genState.coinValue)}" spellcheck="false" autocomplete="off">` +
    `<span class="gen-hint dim">comes from your faucet-funded lab wallet, not from thin air</span>` +
    `</label>`;
  return `<div class="gen-panel" id="gen-panel">` +
    `<p class="gen-head">your <strong>${esc(tpl.name.replace('SilverScript · ', ''))}</strong> — same contract, your parameters` +
    `<span class="gen-keyhint dim">need keys? <code>cargo run -p kascov-lab -- keygen</code> prints your address, pubkey and its blake2b</span></p>` +
    `<div class="gen-fields">${fields}${valueField}</div>` +
    `<div id="gen-out">${genOutputsHtml(tpl)}</div>` +
    `</div>`;
}

function genOutputsHtml(tpl) {
  const info = window.kascovDisasm.skeletonInfo(tpl.name);
  const args = {};
  const values = {};
  for (const p of info.params) {
    const check = window.kascovGen.validateField(p.kind, genState.values[p.name] || '');
    if (!check.ok) {
      return `<p class="gen-wait dim">fix the highlighted field${info.params.length > 1 ? 's' : ''} above to generate your contract.</p>`;
    }
    args[p.name] = check.value;
    values[p.name] = check;
  }
  const valCheck = window.kascovGen.validateField('amount', genState.coinValue || '');
  if (!valCheck.ok) return `<p class="gen-wait dim">coin value: ${esc(valCheck.err)}</p>`;

  const emitted = window.kascovDisasm.emitFromSkeleton(tpl.name, args);
  if (!emitted) return `<p class="gen-err">could not rebuild the script — this should not happen; please report it.</p>`;
  const hex = window.kascovDisasm.toHex(emitted);

  /* self-verify: the emitted bytes must decode back to exactly these args */
  const redecoded = window.kascovDisasm.disassemble(emitted);
  const back = window.kascovDisasm.matchTemplates(redecoded.instructions, emitted);
  const roundTrips = !!back && back.name === tpl.name && info.params.every((p) => {
    const got = (back.fields.find((f) => f.name === p.name) || {}).value;
    return got === window.kascovDisasm.toHex(Uint8Array.from(args[p.name]));
  });
  const identical = hex === genState.sourceHex;
  const verify = roundTrips
    ? `<p class="gen-verify-ok">re-decodes as ${esc(tpl.name)} with your args ✓${identical ? ' · byte-identical to the pasted script' : ''}</p>`
    : `<p class="gen-err">round-trip failed — not offering this script. please report it.</p>`;
  if (!roundTrips) return verify;

  const source = window.kascovGen.buildSource(tpl.name, info.params, values,
    { date: new Date().toISOString().slice(0, 10) });
  const deploy = window.kascovGen.buildDeployCommand(hex, String(valCheck.sompi));
  const block = (title, body, hint) =>
    `<div class="gen-block"><div class="gen-block-head"><span>${esc(title)}</span>` +
    (hint ? `<span class="dim">${esc(hint)}</span>` : '') +
    `<button type="button" class="copy-btn" data-action="copy-block">copy</button></div>` +
    `<pre>${esc(body)}</pre></div>`;
  const note = `<p class="gen-note dim">deploying commits your contract as a <strong>hidden p2sh state</strong> — ` +
    `the coin shows a hash until you <strong>spend</strong> it, which reveals the program on-chain and makes ` +
    `kascov name it your contract, permanently.</p>`;
  return verify + note +
    block('the contract, readable', source, 'canonical SilverScript source') +
    block('the contract, compiled', hex, `${emitted.length} bytes — paste it back into the decoder any time`) +
    block('birth it, then reveal it on testnet-10', deploy, 'copy-paste; born in ~a minute, revealed when you spend');
}

function runDecode(updateHash) {
  const raw = $('#decode-input').value;
  const out = $('#decode-out');
  const dlBtn = $('#decode-download');
  const inToggle = $('#decode-input-toggle');
  if (!raw.trim()) {
    out.innerHTML = '<p class="dim">paste a script above — the disassembly appears here.</p>';
    if (dlBtn) dlBtn.hidden = true;
    if (inToggle) inToggle.hidden = true;
    return;
  }
  const bytes = window.kascovDisasm.parseHex(raw);
  if (!bytes) {
    out.innerHTML = '<p class="decode-err">that doesn’t look like hex — expected an even number of 0-9a-f characters.</p>';
    if (dlBtn) dlBtn.hidden = true;
    return;
  }
  const cleanKey = raw.replace(/\s+/g, '');
  if (cleanKey !== lastDecodeKey) {
    lastDecodeKey = cleanKey;
    decodeShowAll = false;
    genState = null; /* a new script gets a fresh generator panel */
    /* huge paste: fold the input away so the result is what you see */
    const input = $('#decode-input');
    const big = cleanKey.length > DECODE_COLLAPSE_INPUT;
    if (inToggle) inToggle.hidden = !big;
    if (input) {
      input.classList.toggle('collapsed', big);
      if (inToggle) inToggle.textContent = big ? 'expand input ▼' : 'collapse input ▲';
    }
  }
  if (dlBtn) dlBtn.hidden = false;
  const { instructions, truncated } = window.kascovDisasm.disassemble(bytes);
  const groups = [...new Set(instructions.map((i) => i.group))];
  const tpl = !truncated && window.kascovDisasm.matchTemplates
    ? window.kascovDisasm.matchTemplates(instructions, bytes)
    : null;
  const hexAll = window.kascovDisasm.toHex(bytes);
  const statsLine =
    `<p class="decode-summary">` +
    `<span>${fmtInt(bytes.length)} byte${bytes.length === 1 ? '' : 's'} · ` +
    `${fmtInt(instructions.length)} instruction${instructions.length === 1 ? '' : 's'}</span>` +
    groups.map((g) => `<span class="op-chip op-${g}">${g}</span>`).join('') +
    (groups.includes('covenant') ? '<span class="flag flag-ops">covenant ops</span>' : '') +
    (groups.includes('zk') ? '<span class="flag flag-ops">zk ops</span>' : '') +
    (truncated ? '<span class="flag flag-no">truncated / malformed tail</span>' : '') +
    `</p>`;
  const shown = decodeShowAll ? instructions : instructions.slice(0, DECODE_WINDOW);
  const rows = shown.map((inst) => {
    const dataBit = inst.data && inst.data.length
      ? ` <span class="inst-data">0x${window.kascovDisasm.toHex(inst.data)}</span>` : '';
    return `<div class="inst g-${inst.group}">` +
      `<span class="inst-off">${inst.offset.toString(16).padStart(4, '0')}</span>` +
      `<span class="inst-hex">${inst.opcode.toString(16).padStart(2, '0')}</span>` +
      `<span class="inst-text"><span class="op-name">${esc(inst.name)}</span>${dataBit}</span>` +
      `</div>`;
  }).join('');
  const foot = instructions.length > DECODE_WINDOW
    ? `<div class="decode-foot"><button type="button" class="btn" data-action="decode-all">` +
      (decodeShowAll
        ? 'collapse to first ' + fmtInt(DECODE_WINDOW) + ' ↑'
        : `show all ${fmtInt(instructions.length)} instructions ↓`) +
      `</button></div>`
    : '';
  // TIERS — Identity (what it is) · Understand (read the logic) · Deep tools.
  const identity = statsLine + (tpl ? verifiedContractHtml(hexAll) || templateLine(tpl.name, tpl.fields) : '');
  const disasm = `<div class="inst-list">${rows}</div>` + foot;
  const understand = explainerPanelHtml(tpl) + disasm + lintPanelHtml(instructions) + '<div id="registry-panel"></div>';
  const deepTools =
    `<div class="deep-tools-head dim">deep tools</div>` +
    zkPanelHtml(instructions, hexAll) +
    simulatePanelHtml(tpl, hexAll) +
    genCta(tpl) +
    genPanelHtml(tpl, bytes) +
    debugCtaHtml(bytes);
  out.innerHTML = identity + understand + deepTools;
  checkVerifiedRegistry(hexAll);
  if (updateHash && cleanKey.length <= DECODE_SHARE_MAX) {
    /* replaceState keeps the link shareable without re-triggering render;
       megabyte URLs help nobody, so huge scripts skip it */
    history.replaceState(null, '', `#/decode?s=${encodeURIComponent(cleanKey)}`);
  }
}

function downloadDisassembly() {
  const raw = $('#decode-input').value;
  const bytes = window.kascovDisasm.parseHex(raw);
  if (!bytes) return;
  const { instructions, truncated } = window.kascovDisasm.disassemble(bytes);
  const lines = instructions.map(
    (i) => i.offset.toString(16).padStart(4, '0') + '  ' + window.kascovDisasm.toAsm(i)
  );
  if (truncated) lines.push('[truncated / malformed tail]');
  const blob = new Blob(
    [`# kascov disassembly · ${bytes.length} bytes · ${instructions.length} instructions\n` + lines.join('\n') + '\n'],
    { type: 'text/plain' }
  );
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = 'kascov-disassembly.txt';
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 5000);
}

function renderDecode(route) {
  document.title = 'script decoder — kascov';
  const input = $('#decode-input');
  if (route.s && input.value.replace(/\s+/g, '') !== route.s.replace(/\s+/g, '')) {
    input.value = route.s;
  }
  runDecode(false);
}

function renderDev() {
  document.title = 'API — kascov';
  wireApiSidebar();
}

function renderBuild() {
  document.title = 'make your own smart coin — kascov';
  initCompiler();
}

/* API docs: scroll-spy that highlights the sidebar entry for the endpoint
   currently in view. Idempotent — safe to call on every dev render. */
function wireApiSidebar() {
  const nav = document.querySelector('.api-nav');
  if (!nav || nav.dataset.wired) return;
  nav.dataset.wired = '1';
  const links = [...nav.querySelectorAll('a')];
  const byId = new Map(links.map((a) => [a.getAttribute('href').slice(1), a]));
  /* these hrefs are bare in-page anchors (#ep-live), not SPA routes — clicking
     them would set an unrecognized hash and bounce to the landing page. Scroll
     to the target instead and leave the route on #/dev. */
  links.forEach((a) => a.addEventListener('click', (e) => {
    const target = document.getElementById(a.getAttribute('href').slice(1));
    if (target) {
      e.preventDefault();
      target.scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  }));
  const spy = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (e.isIntersecting) {
        links.forEach((l) => l.removeAttribute('aria-current'));
        const a = byId.get(e.target.id);
        if (a) a.setAttribute('aria-current', 'true');
      }
    }
  }, { rootMargin: '-20% 0px -70% 0px' });
  document.querySelectorAll('.api-endpoint[id], .api-block[id]').forEach((el) => spy.observe(el));
}

/* ---------------------------------------------------------------- address */

/* which smart coins has this address/pubkey touched — renders from its own
   endpoint (two-phase like renderDetail: instant header, fill on fetch) and
   never blocks on the multi-MB grid snapshot; names upgrade to the grid's
   dedup-suffixed ones when it happens to be loaded */
function renderAddress(route) {
  const network = state.network;
  const view = $('#view-address');
  const q = route.id;
  const net = NETWORKS[network];
  document.title = `address ${shortHex(q, 10, 6)} — kascov`;
  const back = `<a class="back" href="#/${esc(network)}/explore">← all smart coins</a>`;
  const headChip = (label, value) =>
    `<p class="id-chip">${label ? `<span class="dim">${esc(label)}</span> ` : ''}<span class="mono break">${esc(value)}</span>` +
    `<button type="button" class="copy-btn" data-action="copy" data-copy="${esc(value)}">copy</button></p>`;
  if (!ADDR_RE.test(q) && !PUBKEY_RE.test(q)) {
    view.innerHTML = back + `<div class="empty-card"><h2>that doesn’t look like an address.</h2>` +
      `<p class="dim">paste a kaspa address (kaspa:… / kaspatest:…) or a 32/33-byte pubkey as hex.</p></div>`;
    return;
  }
  const map = state.addrs[network];
  const data = map && map.get(q);
  if (!data) {
    view.innerHTML = back + `<header class="page-head addr-head"><h1>address</h1>` + headChip(null, q) +
      `<p class="page-sub dim">checking which smart coins this address has touched…</p></header>`;
    loadAddress(network, q)
      .then(() => {
        const r = parseRoute();
        if (state.network === network && r.view === 'address' && r.id === q) renderAddress(r);
      })
      .catch(() => {
        const r = parseRoute();
        if (state.network === network && r.view === 'address' && r.id === q) {
          view.innerHTML = back + `<div class="empty-card"><h2>couldn’t look up this address.</h2>` +
            `<p class="dim">the lookup didn’t answer — the worker may be busy, or the address malformed.</p>` +
            `<button type="button" class="btn" data-action="retry-addr">try again</button></div>`;
        }
      });
    return;
  }
  const rows = data.covenants || [];
  /* date cards from the response's own tip anchor (liteMs pattern) */
  const aDaa = data.tip_daa != null ? data.tip_daa : (rows[0] ? rows[0].last_activity_daa : 0);
  const aMs = data.tip_at_ms != null ? data.tip_at_ms : data.generated_at_ms;
  const toMs = (daa) => aMs - (aDaa - daa) * MS_PER_DAA;
  const entry = state.cache[network];
  const controls = rows.filter((r) => r.controls_now).length;
  const bits = [`${fmtInt(data.covenants_total)} smart coin${data.covenants_total === 1 ? '' : 's'} touched`];
  if (controls) bits.push(`${fmtInt(controls)} controlled right now`);
  if (data.covenants_total > rows.length) bits.push(`showing the ${fmtInt(rows.length)} most recent`);
  const cards = rows.map((c) => {
    const gridRec = entry && entry.index.byId.get(c.covenant_id);
    const name = gridRec ? gridRec.name : friendlyName(c.covenant_id);
    const alive = c.status === 'active';
    const sb = [`${c.genesis_daa != null ? 'born' : 'first seen'} ${relTimeShort(toMs(c.genesis_daa != null ? c.genesis_daa : c.first_seen_daa))}`];
    if (alive) sb.push(`holds ${fmtAmount(c.live_value, network)}`);
    else sb.push(`retired ${relTimeShort(toMs(c.last_activity_daa))}`);
    sb.push(c.controls_now ? 'this key controls it now' : 'this key owned it earlier');
    return `<article class="card"><div class="card-head">${avatarSvg(c.covenant_id, 40)}` +
      `<div class="card-id"><a class="card-link" href="#/${esc(network)}/c/${esc(c.covenant_id)}">${esc(name)}</a>` +
      `<span class="pill ${alive ? 'pill-alive' : 'pill-retired'}" title="${esc(alive ? GLOSSARY.alive : GLOSSARY.retired)}">${alive ? 'alive' : 'retired'}</span></div></div>` +
      `<p class="card-story">${esc(sb.join(' · '))}</p></article>`;
  }).join('');
  view.innerHTML = back + `<header class="page-head addr-head"><h1>address</h1>` +
    headChip(null, data.address) +
    (data.pubkey !== q.toLowerCase() ? headChip('pubkey', data.pubkey) : '') +
    `<p class="page-sub">${bits.map(esc).join(' · ')} <span class="dim">on ${esc(net.label)}</span></p></header>` +
    (rows.length ? `<div class="coin-grid">${cards}</div>`
      : `<div class="empty-card"><h2>this address hasn’t touched any smart coins we’ve seen.</h2>` +
        `<p class="dim">kascov matches p2pk covenant states only — plain payments to this address don’t appear here, and covenants with richer scripts may not name their owner.</p></div>`);
}

/* ---------------------------------------------------------------- routing */

function parseRoute() {
  const h = location.hash || '#/';
  /* the path may carry a query ('#/decode?s=…', '#/…/c/<id>?tx=…') */
  const qIdx = h.indexOf('?');
  const path = qIdx === -1 ? h : h.slice(0, qIdx);
  const params = new URLSearchParams(qIdx === -1 ? '' : h.slice(qIdx + 1));
  /* '#/<network>/c/<id>' and bare '#/c/<id>' (keeps the current network,
     for back-compat with old links); '?tx=<txid>' highlights that event */
  let m = path.match(/^#\/(?:(testnet-10|mainnet)\/)?c\/([0-9a-fA-F]{6,64})$/);
  if (m) {
    const tx = (params.get('tx') || '').toLowerCase();
    return {
      view: 'detail',
      network: m[1] || null,
      id: m[2].toLowerCase(),
      tx: /^[0-9a-f]{64}$/.test(tx) ? tx : null,
      program: (() => {
        const pr = (params.get('program') || '').toLowerCase().replace(/^0x/, '');
        return /^[0-9a-f]+$/.test(pr) && pr.length % 2 === 0 && pr.length >= 8 ? pr : null;
      })(),
    };
  }
  /* '#/<network>/addr/<address-or-pubkey>' and bare '#/addr/…' (current network) */
  m = path.match(/^#\/(?:(testnet-10|mainnet)\/)?addr\/([a-zA-Z0-9:]{6,120})$/);
  if (m) return { view: 'address', network: m[1] || null, id: m[2] };
  /* '#/explore' and '#/<network>/explore' */
  m = path.match(/^#\/(?:(testnet-10|mainnet)\/)?explore\/?$/);
  if (m) return { view: 'explore', network: m[1] || null };
  if (/^#\/decode\/?$/.test(path)) return { view: 'decode', network: null, s: params.get('s') || '' };
  if (/^#\/playground\/?$/.test(path)) return { view: 'decode', network: null, s: params.get('s') || '' };
  if (/^#\/build\/?$/.test(path)) return { view: 'build', network: null };
  if (/^#\/dev\/?$/.test(path)) return { view: 'dev', network: null };
  /* old home links '#/<network>' were data views — send them to the explorer */
  m = path.match(/^#\/(testnet-10|mainnet)\/?$/);
  if (m) return { view: 'explore', network: m[1] };
  return { view: 'landing', network: null };
}

function routeHash(view, id) {
  if (view === 'detail') return `#/${state.network}/c/${id}`;
  /* pubkeys are network-independent — an address page survives a network switch */
  if (view === 'address') return `#/${state.network}/addr/${id}`;
  if (view === 'explore') return `#/${state.network}/explore`;
  /* decode/dev are network-free — switching networks keeps the page (and its query) */
  if (view === 'decode' || view === 'dev' || view === 'build') return location.hash || `#/${view}`;
  return '#/';
}

/* Fade a view in without ever risking it staying invisible: the resting
   state is opacity 1; the transient .is-entering class (opacity 0) is
   removed on the next frame so the CSS transition carries it to 1, with a
   timeout as a belt-and-braces fallback and a reduced-motion override in
   the CSS pinning entering views to opacity 1. */
function fadeIn(el) {
  el.classList.remove('is-entering');
  if (window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  void el.offsetWidth;                 /* flush styles so the transition replays */
  el.classList.add('is-entering');
  const settle = () => el.classList.remove('is-entering');
  requestAnimationFrame(() => requestAnimationFrame(settle));
  setTimeout(settle, 400);
}

let renderToken = 0;
let lastView = null;

async function render() {
  const token = ++renderToken;
  const route = parseRoute();
  if (route.network && NETWORKS[route.network] && route.network !== state.network) {
    state.network = route.network;
    state.shown = PAGE_SIZE;
    closeSuggest();
  }
  if (state.watchNet !== state.network) {
    state.watch = loadWatch(state.network);
    state.watchNet = state.network;
  }
  syncStream();
  const panel = $('#panel');
  const views = {
    landing: $('#view-landing'),
    explore: $('#view-explore'),
    detail: $('#view-detail'),
    address: $('#view-address'),
    decode: $('#view-decode'),
    build: $('#view-build'),
    dev: $('#view-dev'),
  };
  /* a stale cached index.html may predate newer views — never crash on them */
  for (const k of Object.keys(views)) if (!views[k]) delete views[k];
  if (!views[route.view]) route.view = 'landing';

  document.querySelectorAll('.network-tab').forEach((b) => {
    b.setAttribute('aria-pressed', String(b.dataset.network === state.network));
  });
  /* decode + build are the two modes of the unified "playground" nav entry */
  const navFor = route.view === 'decode' || route.view === 'build' ? 'playground' : route.view;
  document.querySelectorAll('.nav-link').forEach((a) => {
    if (a.dataset.nav === navFor) a.setAttribute('aria-current', 'page');
    else a.removeAttribute('aria-current');
  });
  $('#header-search').hidden = route.view !== 'explore';

  /* the decoder, dev docs, and address pages never need a snapshot — don't
     block them on data (address pages fetch from their own endpoint) */
  if ((route.view === 'decode' || route.view === 'dev' || route.view === 'build' || route.view === 'address') && views[route.view]) {
    panel.hidden = true;
    for (const [name, el] of Object.entries(views)) el.hidden = name !== route.view;
    views.detail.innerHTML = '';
    if (route.view === 'decode') renderDecode(route);
    else if (route.view === 'address') renderAddress(route);
    else if (route.view === 'build') renderBuild();
    else renderDev();
    fadeIn(views[route.view]);
    if (route.view !== lastView) {
      window.scrollTo({ top: 0, behavior: 'instant' });
      lastView = route.view;
    }
    return;
  }

  let entry = state.cache[state.network];
  if (!entry) {
    const network = state.network;
    /* start the heavyweight snapshot immediately; re-render when it lands */
    const fullPromise = loadNetwork(network)
      .then((e) => {
        if (state.network === network && !document.hidden) render();
        return e;
      })
      .catch(() => null);

    /* deep links straight to a coin: warm its detail while the grid loads */
    if (route.view === 'detail' && route.id) {
      loadDetail(network, route.id).catch(() => { /* handled on render */ });
    }

    /* instant first paint from the tiny live feed (landing/explorer only —
       a coin page needs the full snapshot) */
    if (route.view !== 'detail') {
      const live = (state.live[network] && state.live[network].data) || (await loadLite(network));
      if (token !== renderToken) return;
      if (live) {
        panel.hidden = true;
        for (const [name, el] of Object.entries(views)) el.hidden = name !== route.view;
        views.detail.innerHTML = '';
        if (route.view === 'explore') renderLiteExplore(live, network);
        else renderLiteLanding(live, network);
        fadeIn(views[route.view]);
        if (route.view !== lastView) {
          window.scrollTo({ top: 0, behavior: 'instant' });
          lastView = route.view;
        }
        return; /* the fullPromise re-render completes the page */
      }
    }

    panel.hidden = false;
    panel.className = 'panel';
    panel.innerHTML = `<p>pointing the camera at ${esc(NETWORKS[state.network].label)}…</p>`;
    for (const el of Object.values(views)) el.hidden = true;
    entry = await fullPromise;
    if (token !== renderToken) return;
    if (!entry) {
      panel.hidden = false;
      panel.className = 'panel panel-error';
      panel.innerHTML = `<p>Couldn’t load the ${esc(NETWORKS[state.network].label)} snapshot.</p>` +
        `<button type="button" class="btn" data-action="retry">try again</button>`;
      return;
    }
  }

  panel.hidden = true;

  for (const [name, el] of Object.entries(views)) el.hidden = name !== route.view;

  if (route.view === 'detail') {
    renderDetail(entry, route.id, route.tx, route.program);
  } else {
    views.detail.innerHTML = '';
    if (route.view === 'explore') renderExplore(entry);
    else renderLanding(entry);
  }
  fadeIn(views[route.view]);

  if (route.view !== lastView) {
    /* jump like a page navigation — CSS smooth-scroll is for anchors only */
    window.scrollTo({ top: 0, behavior: 'instant' });
    lastView = route.view;
  }
}

/* Live refresh: refetch the current network's snapshot periodically (only
   while the tab is visible) and re-render in place when it actually changed.
   The detail view is left alone mid-visit so open sections don't collapse;
   its cache still updates for the next navigation. */
const REFRESH_MS = 45_000;
async function refreshSnapshot() {
  if (document.visibilityState !== 'visible') return;
  const network = state.network;
  try {
    const res = await fetch(`data/${network}.json`, { cache: 'no-cache' });
    if (!res.ok) return;
    const data = await res.json();
    const old = state.cache[network];
    if (old && data.generated_at_ms === old.data.generated_at_ms) return;
    data.__anchor = makeAnchor(data, network);
    state.cache[network] = { data, index: buildIndex(data) };
    /* cached coin details are now stale — drop all but the one on screen
       (yanking the open coin's story mid-read would collapse its sections) */
    const dm = state.details[network];
    if (dm) for (const k of [...dm.keys()]) { if (k !== state.detailId) dm.delete(k); }
    if (network === state.network && parseRoute().view !== 'detail') render();
  } catch (e) {
    /* transient — the next tick retries */
  }
}
setInterval(refreshSnapshot, REFRESH_MS);

/* ------------------------------------------------------------- live feed */

/* The tiny <network>-live.json is polled often; the heavyweight snapshot is
   only refetched when its stats say something actually changed. Servers
   without the endpoint 404 — the poller then backs off and the 45s full
   refresh above stays the safety net. */
const LIVE_MS = 12_000;
const LIVE_REPROBE_MS = 5 * 60_000;
const LIVE_FRESH_MS = 3 * 60_000;
const LAG_LIVE_DAA = 3000; /* < ~5 min behind the node's tip still reads as live */

function syncLag(network) {
  /* node tip DAA minus the last DAA the indexer actually applied.
     Old workers don't send processed_daa — null means unknown, and the
     badge falls back to its old behavior. Clamped at 0: a testnet reset
     can briefly leave processed_daa ahead of the new chain's tip. */
  const ls = state.live[network];
  const entry = state.cache[network];
  const src = [ls && ls.data, entry && entry.data].find(
    (d) => d && d.tip_daa != null && d.processed_daa != null);
  if (!src) return null;
  return Math.max(0, src.tip_daa - src.processed_daa);
}

function liveBadgeHtml(network) {
  const ls = state.live[network];
  const entry = state.cache[network];
  const tipAt = ls && ls.data && ls.data.tip_at_ms != null ? ls.data.tip_at_ms
    : entry && entry.data.tip_at_ms != null ? entry.data.tip_at_ms : null;
  if (tipAt != null) {
    if (Date.now() - tipAt < LIVE_FRESH_MS) {
      const lag = syncLag(network);
      if (lag != null && lag >= LAG_LIVE_DAA) {
        /* keep the visible text short — the full story lives in the tooltip */
        const mins = Math.round((lag * MS_PER_DAA) / 60000);
        const span = mins >= 90 ? `${Math.floor(mins / 60)}h ${mins % 60}m` : `${mins}m`;
        return `<span class="live-badge live-lag" title="the indexer is replaying ${esc(fmtSpan(lag * MS_PER_DAA))} of chain, every block in order — nothing is skipped">` +
          `<i class="live-dot" aria-hidden="true"></i>` +
          `catching up · ${esc(span)} behind</span>`;
      }
      return '<span class="live-badge live-on"><i class="live-dot" aria-hidden="true"></i>watching live</span>';
    }
    return `<span class="live-badge live-stale"><i class="live-dot" aria-hidden="true"></i>` +
      `sync catching up — last saw the chain ${esc(relTimeShort(tipAt))}</span>`;
  }
  return '<span class="live-badge live-off"><i class="live-dot" aria-hidden="true"></i>snapshot mode</span>';
}

function updateLiveBadge() {
  const html = liveBadgeHtml(state.network);
  document.querySelectorAll('.live-badge-slot').forEach((el) => { el.innerHTML = html; });
}

async function pollLive() {
  if (document.visibilityState !== 'visible') return;
  const network = state.network;
  const ls = state.live[network] || (state.live[network] = { supported: null, missedAt: 0, data: null });
  if (ls.supported === false && Date.now() - ls.missedAt < LIVE_REPROBE_MS) return;
  try {
    const res = await fetch(`data/${network}-live.json`, { cache: 'no-cache' });
    if (res.status === 404) {
      ls.supported = false;
      ls.missedAt = Date.now();
      updateLiveBadge();
      return;
    }
    if (!res.ok) return;
    const live = await res.json();
    ls.supported = true;
    ls.data = live;
    updateLiveBadge();
    const cached = state.cache[network];
    if (cached && live.stats && (
      live.stats.events !== cached.data.stats.events ||
      live.stats.covenants !== cached.data.stats.covenants
    )) {
      refreshSnapshot();
      schedulePulseRefresh();
    }
  } catch (e) {
    /* transient — the next tick retries */
  }
}
setInterval(pollLive, LIVE_MS);

/* ------------------------------------------------------------ live stream */

/* Server-sent events push each covenant event the moment the indexer sees
   it. Strictly an optimization over the polling above: a message only
   schedules an immediate pollLive() (debounced, so bursts fold into one),
   and any error backs off and leaves polling untouched. Some CDNs buffer
   rewrites — if the stream never delivers, nothing is lost. */
const STREAM_RETRY_BASE_MS = 5_000;
const STREAM_RETRY_MAX_MS = 60_000;
const STREAM_POKE_MS = 500;

const stream = { es: null, network: null, retryMs: STREAM_RETRY_BASE_MS, retryTimer: 0, pokeTimer: 0 };

function streamWanted() {
  const view = parseRoute().view;
  return document.visibilityState === 'visible' && (view === 'landing' || view === 'explore');
}

function closeStream() {
  if (stream.es) { stream.es.close(); stream.es = null; }
  stream.network = null;
  clearTimeout(stream.retryTimer);
  clearTimeout(stream.pokeTimer);
  stream.retryTimer = 0;
  stream.pokeTimer = 0;
}

function flashLiveBadge() {
  document.querySelectorAll('.live-badge').forEach((el) => {
    el.classList.remove('live-flash');
    void el.offsetWidth; /* restart the animation */
    el.classList.add('live-flash');
  });
}

/* Open/close/retarget the stream to match the current view + network.
   Idempotent — called from render() and visibilitychange. */
function syncStream() {
  if (typeof EventSource === 'undefined') return;
  if (!streamWanted()) { closeStream(); return; }
  if (stream.es && stream.network === state.network) return;
  closeStream();
  const network = state.network;
  const es = new EventSource(`data/${network}/stream`);
  stream.es = es;
  stream.network = network;
  es.onopen = () => { stream.retryMs = STREAM_RETRY_BASE_MS; };
  es.onmessage = () => {
    flashLiveBadge();
    clearTimeout(stream.pokeTimer);
    stream.pokeTimer = setTimeout(pollLive, STREAM_POKE_MS);
  };
  es.onerror = () => {
    if (stream.es !== es) return;
    closeStream();
    stream.retryTimer = setTimeout(syncStream, stream.retryMs);
    stream.retryMs = Math.min(stream.retryMs * 2, STREAM_RETRY_MAX_MS);
  };
}

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') {
    refreshSnapshot();
    pollLive();
  }
  syncStream();
});

/* ----------------------------------------------------------------- events */

async function copyToClipboard(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch (e) {
    try {
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.setAttribute('readonly', '');
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand('copy');
      ta.remove();
      return ok;
    } catch (e2) {
      return false;
    }
  }
}

document.addEventListener('click', (e) => {
  const el = e.target.closest('[data-action]');
  if (!el) return;
  const action = el.dataset.action;

  if (action === 'network') {
    const net = el.dataset.network;
    if (!NETWORKS[net] || net === state.network) return;
    state.network = net;
    state.shown = PAGE_SIZE;
    /* encode the network in the hash so the choice survives reloads and
       shared links land on the right network. A specific coin can't exist on
       the other network, so switching from a detail page lands on that
       network's explorer overview rather than a guaranteed "not found". */
    const route = parseRoute();
    const view = route.view === 'detail' ? 'explore' : route.view;
    const target = routeHash(view, route.id);
    if (location.hash === target) render();
    else location.hash = target;
  } else if (action === 'filter') {
    state.filter = el.dataset.filter;
    state.shown = PAGE_SIZE;
    /* scope to the filter chips — the pulse range pills are .chip too */
    document.querySelectorAll('[data-action="filter"]').forEach((c) => {
      c.setAttribute('aria-pressed', String(c.dataset.filter === state.filter));
    });
    const entry = state.cache[state.network];
    if (entry) renderGrid(entry, state.network);
  } else if (action === 'pulse-range') {
    const r = el.dataset.range;
    if (!ACTIVITY_RANGES.includes(r) || r === state.pulseRange) return;
    state.pulseRange = r;
    try { localStorage.setItem('kascov-pulse-range', r); } catch (err) { /* private mode */ }
    document.querySelectorAll('[data-action="pulse-range"]').forEach((b) => {
      b.setAttribute('aria-pressed', String(b.dataset.range === r));
    });
    hidePulseTip();
    updatePulse(state.network);
  } else if (action === 'more') {
    state.shown += PAGE_SIZE;
    const entry = state.cache[state.network];
    if (entry) renderGrid(entry, state.network);
  } else if (action === 'nerd') {
    state.nerd = !state.nerd;
    try { localStorage.setItem('kascov-nerd', state.nerd ? '1' : '0'); } catch (err) { /* ignore */ }
    const route = parseRoute();
    const entry = state.cache[state.network];
    if (route.view === 'detail' && entry) {
      const y = window.scrollY;
      renderDetail(entry, route.id, route.tx, route.program);
      window.scrollTo({ top: y, behavior: 'instant' });
    }
  } else if (action === 'watch') {
    const id = el.dataset.id;
    if (!id) return;
    if (state.watch.has(id)) state.watch.delete(id);
    else state.watch.add(id);
    saveWatch(state.network, state.watch);
    const route = parseRoute();
    const entry = state.cache[state.network];
    if (!entry) return;
    if (route.view === 'explore') {
      renderWatchStrip(entry, state.network);
      renderGrid(entry, state.network);
    } else if (route.view === 'detail') {
      const y = window.scrollY;
      renderDetail(entry, route.id);
      window.scrollTo({ top: y, behavior: 'instant' });
    }
  } else if (action === 'decode') {
    runDecode(true);
  } else if (action === 'decode-load') {
    const hex = DECODE_EXAMPLES[el.dataset.example];
    if (hex) {
      $('#decode-input').value = hex;
      runDecode(true);
    }
  } else if (action === 'decode-all') {
    decodeShowAll = !decodeShowAll;
    runDecode(false);
  } else if (action === 'gen-example') {
    const input = $('#decode-input');
    const which = el.dataset.example || 'mecenas';
    if (input && DECODE_EXAMPLES[which]) {
      input.value = DECODE_EXAMPLES[which];
      /* decode first (a fresh script resets genState), THEN open the panel */
      runDecode(true);
      genState = { open: true };
      runDecode(false);
      setTimeout(() => {
        const panel = $('#gen-panel');
        if (panel) panel.scrollIntoView({ behavior: 'smooth', block: 'start' });
      }, 120);
    }
  } else if (action === 'gen-toggle') {
    genState = genState && genState.open ? null : { open: true };
    runDecode(false);
    if (genState) {
      const panel = $('#gen-panel');
      if (panel) panel.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  } else if (action === 'copy-block') {
    const pre = el.closest('.gen-block');
    const text = pre && pre.querySelector('pre') ? pre.querySelector('pre').textContent : '';
    copyToClipboard(text).then((ok) => {
      const orig = el.textContent;
      el.textContent = ok ? 'copied!' : 'copy failed';
      el.classList.add('copied');
      setTimeout(() => { el.textContent = orig; el.classList.remove('copied'); }, 1400);
    });
  } else if (action === 'decode-download') {
    downloadDisassembly();
  } else if (action === 'decode-input-toggle') {
    const input = $('#decode-input');
    const collapsed = input.classList.toggle('collapsed');
    el.textContent = collapsed ? 'expand input ▼' : 'collapse input ▲';
  } else if (action === 'story-all') {
    state.storyAll = !state.storyAll;
    const entry = state.cache[state.network];
    const route = parseRoute();
    if (entry && route.view === 'detail') {
      const y = window.scrollY;
      renderDetail(entry, route.id);
      window.scrollTo({ top: y, behavior: 'instant' });
    }
  } else if (action === 'utxo-all') {
    state.utxoAll = !state.utxoAll;
    const entry = state.cache[state.network];
    const route = parseRoute();
    if (entry && route.view === 'detail') {
      const y = window.scrollY;
      renderDetail(entry, route.id);
      window.scrollTo({ top: y, behavior: 'instant' });
    }
  } else if (action === 'decode-share') {
    runDecode(true);
    copyToClipboard(location.href).then((ok) => {
      const original = el.dataset.label || el.textContent;
      el.dataset.label = original;
      el.textContent = ok ? 'link copied!' : 'copy failed';
      setTimeout(() => { el.textContent = el.dataset.label; }, 1400);
    });
  } else if (action === 'copy') {
    copyToClipboard(el.dataset.copy || '').then((ok) => {
      const original = el.dataset.label || el.textContent;
      el.dataset.label = original;
      el.textContent = ok ? 'copied!' : 'copy failed';
      el.classList.add('copied');
      setTimeout(() => {
        el.textContent = el.dataset.label;
        el.classList.remove('copied');
      }, 1400);
    });
  } else if (action === 'retry') {
    delete state.cache[state.network];
    render();
  } else if (action === 'reveal-check') {
    const box = el.closest('.reveal-preview');
    const val = box && box.querySelector('.reveal-input') ? box.querySelector('.reveal-input').value.trim().replace(/^0x/, '') : '';
    if (val) {
      const route = parseRoute();
      if (route.view === 'detail') {
        location.hash = `#/${state.network}/c/${route.id}?program=${val}`;
      }
    }
  } else if (action === 'compile-run') {
    const src = $('#compiler-src');
    const argsEl = $('#compiler-args');
    const out = $('#compiler-result');
    if (!src || !out) return;
    out.innerHTML = '<span class="dim">compiling through silverc…</span>';
    const args = (argsEl ? argsEl.value : '').split('\n').map((s) => s.trim()).filter(Boolean);
    fetch(`data/${state.network}/compile`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ source: src.value, args }) })
      .then((r) => r.json())
      .then(renderCompileResult)
      .catch(() => { out.innerHTML = '<div class="compile-err">the compiler isn’t available right now</div>'; });
  } else if (action === 'compile-example') {
    const src = $('#compiler-src');
    const args = $('#compiler-args');
    if (src) src.value = SILVERSCRIPT_EXAMPLE.source;
    if (args) args.value = SILVERSCRIPT_EXAMPLE.args;
    const out = $('#compiler-result');
    if (out) out.innerHTML = '';
  } else if (action === 'compile-publish') {
    const src = $('#compiler-src');
    const argsEl = $('#compiler-args');
    const out = $('#publish-result');
    if (!lastCompiled || !src || !out) return;
    out.innerHTML = '<span class="dim">publishing…</span>';
    const args = (argsEl ? argsEl.value : '').split('\n').map((s) => s.trim()).filter(Boolean);
    fetch(`data/${state.network}/publish`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ source: src.value, args }) })
      .then((r) => r.json())
      .then((d) => {
        out.innerHTML = d.ok
          ? `<div class="compile-ok">✓ published — any coin whose program hashes <code class="mono">${esc(d.hash.slice(0, 12))}…</code> now shows this source on its decode page</div>`
          : `<div class="compile-err">${esc(d.error || 'publish failed')}</div>`;
      })
      .catch(() => { out.innerHTML = '<div class="compile-err">publish unavailable</div>'; });
  } else if (action === 'zk-verify') {
    const result = el.parentElement.querySelector('.zk-verify-result');
    const prog = el.dataset.prog;
    if (!prog || !result) return;
    result.innerHTML = ' <span class="dim">running the verifier…</span>';
    fetch(`data/${state.network}/zk-verify`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ program_hex: prog }) })
      .then((r) => r.json())
      .then((d) => {
        result.innerHTML = d.valid
          ? ` <span class="zk-valid">✓ ${esc(d.reason)}</span>`
          : ` <span class="zk-invalid">✗ ${esc(d.reason || 'invalid')}</span>`;
      })
      .catch(() => { result.innerHTML = ' <span class="dim">verifier unavailable</span>'; });
  } else if (action === 'graph-prev') {
    graphIdx -= 1;
    renderAppGraph();
  } else if (action === 'graph-next') {
    graphIdx += 1;
    renderAppGraph();
  } else if (action === 'sim-run') {
    const panel = el.closest('.sim-panel');
    const scenario = (SIM_SCENARIOS[panel && panel.dataset.tpl] || [])[parseInt(el.dataset.i, 10)];
    const out = panel && panel.querySelector('.sim-result');
    if (!panel || !scenario || !out) return;
    panel.querySelectorAll('.sim-chip').forEach((c) => c.classList.remove('sim-active'));
    el.classList.add('sim-active');
    out.innerHTML = '<span class="dim">running through the script engine…</span>';
    const body = { program_hex: panel.dataset.hex, entrypoint: scenario.entrypoint, recipient: scenario.recipient, value: SIM_VALUE, trace: true };
    if (scenario.amount != null) body.amount = scenario.amount;
    fetch(`data/${state.network}/simulate`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) })
      .then((r) => r.json())
      .then((d) => { lastSimTrace = d.trace || null; out.innerHTML = simVerdictHtml(d); })
      .catch(() => { out.innerHTML = '<div class="sim-verdict sim-na">simulation unavailable</div>'; });
  } else if (action === 'sim-trace') {
    openSimTrace(lastSimTrace);
  } else if (action === 'dbg-open') {
    openDebugger(el.dataset.prog || '');
  } else if (action === 'dbg-prev') {
    dbgStep(-1);
  } else if (action === 'dbg-next') {
    dbgStep(1);
  } else if (action === 'dbg-seek') {
    dbgStep(0, parseInt(el.dataset.i, 10));
  } else if (action === 'dbg-close') {
    dbg = null;
    const host = document.getElementById('dbg-panel');
    if (host) host.innerHTML = '';
  } else if (action === 'retry-detail') {
    render(); /* the detail map has no entry for a failed fetch — this refetches */
  } else if (action === 'retry-addr') {
    render(); /* failed lookups are never cached — this refetches */
  }
});

/* the debugger scrubber (range input) — separate from click delegation */
document.addEventListener('input', (e) => {
  const el = e.target.closest('[data-action="dbg-slider"]');
  if (el) dbgStep(0, parseInt(el.value, 10));
});

/* render the app-graph lazily when its section is expanded (toggle doesn't
   bubble, so capture) */
document.addEventListener('toggle', (e) => {
  if (e.target && e.target.id === 'section-appgraph' && e.target.open) renderAppGraph();
}, true);

$('#search').addEventListener('input', (e) => {
  const hadQuery = Boolean(state.query);
  const raw = e.target.value.trim();
  /* a kaspa address or a 66-hex pubkey can't be a coin or tx id — go straight
     to its address page (64-hex stays coin/tx-first; the miss card offers the
     pubkey interpretation) */
  if (ADDR_RE.test(raw) || /^[0-9a-fA-F]{66}$/.test(raw)) {
    e.target.value = '';
    state.query = '';
    closeSuggest();
    location.hash = `#/${state.network}/addr/${raw.toLowerCase()}`;
    return;
  }
  /* forgive pasted outpoints ('txid:0') and spaced names ('proud olive stoat') */
  state.query = e.target.value.trim().toLowerCase()
    .replace(/:\d+$/, '')
    .replace(/\s+/g, '-');
  state.shown = PAGE_SIZE;
  /* results live below the fold — bring them into view when a search starts */
  if (state.query && !hadQuery) {
    const coins = $('#section-coins');
    if (coins && !coins.hidden && coins.getBoundingClientRect().top > window.innerHeight * 0.55) {
      coins.scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  }
  const entry = state.cache[state.network];
  if (!entry) return;
  /* a pasted 64-hex id resolves instantly — coin id straight away, txid via
     the server's indexed lookup (the grid feed carries no per-event txids) */
  if (/^[0-9a-f]{64}$/.test(state.query)) {
    const byId = entry.index.byId.get(state.query);
    if (byId) {
      e.target.value = '';
      state.query = '';
      closeSuggest();
      location.hash = `#/${state.network}/c/${byId.c.covenant_id}`;
      return;
    }
    resolveTxQuery(state.network, state.query, e.target);
  }
  renderGrid(entry, state.network);
  renderSuggest();
});

/* Ask the worker which covenant a transaction touched; navigate on a hit,
   remember the miss so the grid can answer honestly. */
async function resolveTxQuery(network, txid, input) {
  if (state.txLookup[txid]) return; /* already pending or answered */
  state.txLookup[txid] = 'pending';
  try {
    const res = await fetch(`data/${network}/tx/${txid}.json`, { cache: 'no-cache' });
    if (res.ok) {
      const { covenant_id } = await res.json();
      delete state.txLookup[txid];
      if (state.network === network && state.query === txid) {
        if (input) input.value = '';
        state.query = '';
        closeSuggest();
        location.hash = `#/${network}/c/${covenant_id}?tx=${txid}`;
      }
      return;
    }
    state.txLookup[txid] = 'miss';
  } catch (err) {
    state.txLookup[txid] = 'miss';
  }
  if (state.network === network && state.query === txid) {
    const entry = state.cache[network];
    if (entry) renderGrid(entry, network);
  }
}

/* generator fields: live output refresh while typing (caret-safe — only
   #gen-out re-renders), full panel refresh on blur so error hints settle */
document.addEventListener('input', (e) => {
  const field = e.target.closest && e.target.closest('#gen-panel') ? e.target : null;
  if (!field || !genState) return;
  const key = field.dataset.genField;
  if (!key) return;
  if (key === '__value') genState.coinValue = field.value;
  else genState.values[key] = field.value;
  const out = $('#gen-out');
  const raw = $('#decode-input') ? $('#decode-input').value : '';
  const bytes = window.kascovDisasm.parseHex(raw);
  if (out && bytes) {
    const { instructions } = window.kascovDisasm.disassemble(bytes);
    const tpl = window.kascovDisasm.matchTemplates(instructions, bytes);
    if (tpl) out.innerHTML = genOutputsHtml(tpl);
  }
});
document.addEventListener('change', (e) => {
  if (e.target.closest && e.target.closest('#gen-panel') && genState) runDecode(false);
});

$('#search').addEventListener('keydown', (e) => {
  const open = suggest.items.length > 0 && !$('#search-suggest').hidden;
  if (e.key === 'ArrowDown' && open) {
    e.preventDefault();
    setActiveSuggest(Math.min(suggest.active + 1, suggest.items.length - 1));
  } else if (e.key === 'ArrowUp' && open) {
    e.preventDefault();
    setActiveSuggest(Math.max(suggest.active - 1, -1));
  } else if (e.key === 'Enter' && open) {
    e.preventDefault();
    const pick = suggest.items[suggest.active >= 0 ? suggest.active : 0];
    if (pick) goToSuggestion(pick);
  } else if (e.key === 'Escape') {
    closeSuggest();
  }
});

$('#search').addEventListener('blur', () => {
  /* let a click on a suggestion land first */
  setTimeout(closeSuggest, 150);
});

const suggestHost = $('#search-suggest');
if (suggestHost) {
  /* keep focus in the input so blur doesn't eat the click */
  suggestHost.addEventListener('mousedown', (e) => e.preventDefault());
  suggestHost.addEventListener('click', (e) => {
    const a = e.target.closest('[data-suggest]');
    if (!a) return;
    e.preventDefault();
    const s = suggest.items[Number(a.dataset.suggest)];
    if (s) goToSuggestion(s);
  });
}

const sortSelect = $('#sort');
if (sortSelect) {
  sortSelect.addEventListener('change', (e) => {
    state.sort = e.target.value;
    state.shown = PAGE_SIZE;
    const entry = state.cache[state.network];
    if (entry) renderGrid(entry, state.network);
  });
}

/* the pulse chart's pointer/keyboard layer attaches once to the static
   #pulse-chart host (index.html — never replaced; its content is). arrow
   keys give keyboard users tooltip parity without 61 tab stops. */
const pulseHost = $('#pulse-chart');
if (pulseHost) {
  pulseHost.addEventListener('pointermove', onPulsePointer);
  pulseHost.addEventListener('pointerdown', onPulsePointer);
  pulseHost.addEventListener('pointerleave', hidePulseTip);
  pulseHost.addEventListener('pointercancel', hidePulseTip);
  pulseHost.addEventListener('keydown', (e) => {
    if (!pulseView) return;
    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      e.preventDefault();
      setPulseHot((pulseHot < 0 ? pulseView.n - 1 : pulseHot) + (e.key === 'ArrowRight' ? 1 : -1));
    } else if (e.key === 'Escape') {
      hidePulseTip();
    }
  });
  pulseHost.addEventListener('focusout', hidePulseTip);
}

const decodeInput = $('#decode-input');
let decodeTimer = 0;
if (decodeInput) {
  decodeInput.addEventListener('input', () => {
    clearTimeout(decodeTimer);
    decodeTimer = setTimeout(() => runDecode(true), 250);
  });
}

window.addEventListener('hashchange', render);

/* fill static guide icons */
document.querySelectorAll('.guide-icon').forEach((el) => {
  el.innerHTML = ICONS[el.dataset.icon] || '';
});

/* ------------------------------------------------ first-visit story tour */

/* Six steps over the LIVE page: watch → understand → touch. Vanilla,
   dismissible everywhere, never nags twice (localStorage flag), replay via
   ?tour=1 or the landing's "take the tour" link. */
const tour = { step: -1, el: null };

const TOUR_STEPS = [
  {
    target: () => document.querySelector('.live-badge'),
    text: 'kascov is watching the Kaspa chain <strong>right now</strong> — green means it saw the tip seconds ago.',
  },
  {
    target: () => document.querySelector('#story-list .story'),
    text: 'smart coins are <strong>born</strong>, they <strong>move</strong>, they <strong>retire</strong>. these are real events, moments old.',
  },
  {
    target: () => document.querySelector('#story-list .story'),
    text: 'let’s follow this one…',
    enter: (el) => { if (el) location.hash = el.getAttribute('href'); },
    autoAdvanceMs: 1100,
  },
  {
    target: () => document.querySelector('#view-detail .timeline'),
    text: 'this is its <strong>life story</strong> — complete, permanent, in plain words. every line is a real transaction you can verify on chain.',
  },
  {
    target: () => document.querySelector('.nerd-toggle'),
    text: 'the raw scripts live under <strong>nerd mode</strong> — decoded, labeled, and hash-verified when a spend reveals a hidden program.',
  },
  {
    target: () => document.querySelector('.nav-link[data-nav="decode"]'),
    text: 'the decoder reads <strong>any</strong> script — and if it’s a known contract, you can <strong>make your own from it</strong> and birth it on the testnet. enjoy the telescope 🔭',
    last: true,
  },
];

function endTour(finished) {
  if (tour.el) tour.el.remove();
  tour.el = null;
  tour.step = -1;
  try { localStorage.setItem('kascov-tour', 'done'); } catch (e) { /* private mode */ }
  if (finished) location.hash = '#/decode?s=' + (DECODE_EXAMPLES.mecenas || '');
}

function showTourStep(i) {
  const step = TOUR_STEPS[i];
  if (!step) { endTour(false); return; }
  const target = step.target();
  if (!target) {
    /* element not on screen (data still loading, view changed) — try the
       next step rather than stranding the visitor */
    if (i + 1 < TOUR_STEPS.length) showTourStep(i + 1);
    else endTour(false);
    return;
  }
  tour.step = i;
  if (!tour.el) {
    tour.el = document.createElement('div');
    tour.el.id = 'tour-root';
    document.body.appendChild(tour.el);
  }
  target.scrollIntoView({ block: 'center', behavior: 'instant' });
  const r = target.getBoundingClientRect();
  const below = r.bottom + 200 < window.innerHeight;
  const cardTop = (below ? r.bottom + 14 : Math.max(12, r.top - 160)) + window.scrollY;
  tour.el.innerHTML =
    `<div class="tour-spot" style="top:${r.top + window.scrollY - 6}px;left:${Math.max(0, r.left - 6)}px;width:${Math.min(r.width + 12, window.innerWidth)}px;height:${r.height + 12}px"></div>` +
    `<div class="tour-card" style="top:${cardTop}px;left:${Math.max(12, Math.min(r.left, window.innerWidth - 348))}px">` +
    `<p class="tour-text">${step.text}</p>` +
    `<div class="tour-nav">` +
    `<span class="dim tour-count">${i + 1}/${TOUR_STEPS.length}</span>` +
    `<button type="button" class="btn tour-skip" data-tour="skip">skip</button>` +
    `<button type="button" class="btn btn-accent" data-tour="${step.last ? 'finish' : 'next'}">${step.last ? 'try the decoder →' : 'next →'}</button>` +
    `</div></div>`;
  if (step.enter) {
    step.enter(target);
    if (step.autoAdvanceMs) setTimeout(() => { if (tour.step === i) showTourStep(i + 1); }, step.autoAdvanceMs);
  }
}

function maybeStartTour() {
  let seen = 'done';
  try { seen = localStorage.getItem('kascov-tour') || ''; } catch (e) { /* private mode: never nag */ }
  const forced = /[?&]tour=1/.test(location.hash) || /[?&]tour=1/.test(location.search);
  if (seen === 'done' && !forced) return;
  const view = parseRoute().view;
  if (view !== 'explore' && view !== 'landing') return;
  if (view === 'landing') location.hash = `#/${state.network}/explore`;
  const tryStart = (attempt) => {
    if (tour.step >= 0) return;
    if (document.querySelector('#story-list .story') && document.querySelector('.live-badge')) {
      showTourStep(0);
    } else if (attempt < 40) {
      setTimeout(() => tryStart(attempt + 1), 250);
    }
  };
  tryStart(0);
}

document.addEventListener('click', (e) => {
  const b = e.target.closest('[data-tour]');
  if (!b) return;
  const act = b.dataset.tour;
  if (act === 'skip') endTour(false);
  else if (act === 'finish') endTour(true);
  else showTourStep(tour.step + 1);
});
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && tour.step >= 0) endTour(false);
});
window.addEventListener('resize', () => { if (tour.step >= 0) showTourStep(tour.step); });
/* the "take the tour" link changes the hash on an already-loaded page —
   arm the tour then too, not just at boot */
window.addEventListener('hashchange', () => {
  if (tour.step < 0 && /[?&]tour=1/.test(location.hash)) setTimeout(maybeStartTour, 400);
});

/* pasted clean URLs (hosting rewrites everything to this page):
   /explore, /decode?s=…, /testnet-10/c/<id> → the same hash routes */
if (location.pathname !== '/' && location.pathname !== '/index.html' && !location.hash) {
  const path = location.pathname.replace(/^\/+|\/+$/g, '');
  history.replaceState(null, '', `/#/${path}${location.search}`);
}

/* cmd-K / ctrl-K focuses the search — the search IS the command palette */
document.addEventListener('keydown', (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault();
    const s2 = document.querySelector('#search');
    if (s2) { s2.focus(); s2.select(); s2.closest('.search-wrap').classList.add('palette-flash');
      setTimeout(() => s2.closest('.search-wrap').classList.remove('palette-flash'), 700); }
  }
});
(() => { const s2 = document.querySelector('#search'); if (s2 && navigator.platform.includes('Mac')) s2.placeholder += '  ⌘K'; })();

/* click any [data-copy] (build-page commands) to copy it */
document.addEventListener('click', (e) => {
  const el = e.target.closest('[data-copy]');
  if (!el) return;
  const text = el.dataset.copy;
  navigator.clipboard?.writeText(text).then(() => {
    el.classList.add('copied');
    setTimeout(() => el.classList.remove('copied'), 1100);
  }).catch(() => {});
});

render();
pollLive();
setTimeout(maybeStartTour, 900);

})();
