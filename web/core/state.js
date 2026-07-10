/* kascov core/state — network config, tunables, the single mutable app
   state object, and its localStorage persistence (nerd flag, pulse range,
   watchlist), plus the network-aware helpers (fmtAmount, DAA→time anchor).
   Imports nothing; everything downstream imports from here. */

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
/* Rows pulled from the grid feed per network round-trip. The worker honours
   ?limit=/?after_daa= and hands back "next_after_daa" while older rows remain;
   older workers ignore the params and return the whole snapshot in one shot
   (no cursor), so this degrades to the original full load automatically. */
const GRID_PAGE = 2000;
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
  reorgs: {},         // network -> { data, at } (virtual-chain reorg log; data null = 404 miss)
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

export {
  NETWORKS, MS_PER_DAA, PAGE_SIZE, GRID_PAGE, STORY_COUNT, TEASER_COUNT,
  PULSE_BUCKETS, ACTIVITY_RANGES, ACTIVITY_LABELS, ACTIVITY_PHRASE,
  ACTIVITY_TTL_MS, ACTIVITY_MISS_TTL_MS, ACTIVITY_MAX_COLS, ADDR_RE, PUBKEY_RE,
  fmtAmount, makeAnchor, daaToMs, txUrl,
  state, watchKey, loadWatch, saveWatch,
};
