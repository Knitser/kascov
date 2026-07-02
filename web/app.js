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
  watch: new Set(),   // covenant ids starred on the current network
  watchNet: null,
};

try { state.nerd = localStorage.getItem('kascov-nerd') === '1'; } catch (e) { /* private mode */ }

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

/* Build a derived index so rendering stays cheap. */
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
    const moves = c.events.filter((e) => e.kind === 'transition').length;
    const firstEvent = c.events[0] || null;
    const lastEvent = c.events[c.events.length - 1] || null;
    const bornMs = c.genesis_daa != null ? daaToMs(c.genesis_daa, data)
      : (firstEvent ? daaToMs(firstEvent.accepting_daa, data) : data.generated_at_ms);
    const lastMs = lastEvent ? daaToMs(lastEvent.accepting_daa, data) : bornMs;
    let birthValue = 0;
    if (c.genesis_daa != null && Array.isArray(c.utxos)) {
      for (const u of c.utxos) if (u.created_daa === c.genesis_daa) birthValue += u.value;
    }
    const balances = balancesByEventDaa(c);
    const blob = (name + ' ' + c.covenant_id + ' ' + (c.genesis_txid || '') + ' ' +
      c.events.map((e) => e.txid).join(' ')).toLowerCase();
    return { c, name, moves, bornMs, lastMs, birthValue, balances, blob };
  });
  covs.sort((a, b) => (b.c.last_activity_daa || 0) - (a.c.last_activity_daa || 0));
  const byId = new Map(covs.map((e) => [e.c.covenant_id, e]));
  /* txid → covenant, so a pasted transaction id resolves straight to a coin */
  const txToCov = new Map();
  for (const e of covs) {
    if (e.c.genesis_txid && !txToCov.has(e.c.genesis_txid)) txToCov.set(e.c.genesis_txid, e);
    for (const ev of e.c.events) {
      if (!txToCov.has(ev.txid)) txToCov.set(ev.txid, e);
    }
  }
  return { covs, byId, txToCov };
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

/* DAA → ms against the live feed's own tip anchor. */
function liteMs(live, daa) {
  const aDaa = live.tip_daa != null ? live.tip_daa : live.stats.last_activity_daa;
  const aMs = live.tip_at_ms != null ? live.tip_at_ms : live.generated_at_ms;
  return aMs - (aDaa - daa) * MS_PER_DAA;
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
    `<span class="story-text">${sentence} <span class="story-when" title="${esc(utcTitle(ms))}">— ${esc(relTime(ms))}</span></span>` +
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
  const bits = ['loading the full picture…'];
  if (s.live_value > 0) bits.unshift(`together they hold ${fmtAmount(s.live_value, network)}`);
  $('#freshness').innerHTML =
    `<span class="live-badge-slot">${liveBadgeHtml(network)}</span> · ${bits.map(esc).join(' · ')}`;
  const empty = s.covenants === 0;
  $('#landing-empty').hidden = !empty;
  $('#section-teaser').hidden = empty;
  if (empty) {
    $('#landing-empty').innerHTML = emptyCardHtml(network);
    return;
  }
  $('#teaser-list').innerHTML = (live.recent_events || [])
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
    'loading the full picture…',
  ];
  $('#explore-stats').innerHTML =
    `<span class="live-badge-slot">${liveBadgeHtml(network)}</span> · ${bits.map(esc).join(' · ')}`;
  const empty = s.covenants === 0;
  $('#empty-net').hidden = !empty;
  $('#watch-strip').hidden = true;
  $('#section-records').hidden = true;
  $('#section-pulse').hidden = true;
  $('#section-stories').hidden = empty;
  $('#section-coins').hidden = empty;
  if (empty) {
    $('#empty-net').innerHTML = emptyCardHtml(network);
    return;
  }
  $('#story-list').innerHTML = (live.recent_events || [])
    .slice(0, STORY_COUNT)
    .map((ev) => liteStoryRow(ev, live, network))
    .join('');
  $('#result-count').textContent = `${fmtInt(s.covenants)} smart coins`;
  $('#coin-grid').innerHTML =
    `<div class="no-results"><p class="dim">loading all ${esc(fmtInt(s.covenants))} smart coins…</p></div>`;
  $('#grid-foot').innerHTML = '';
}

/* ------------------------------------------------------------- sentences */

function eventSentence(entry, ev, network, withBalance) {
  const name = entry.name;
  if (ev.kind === 'genesis') {
    return entry.birthValue > 0
      ? `<strong>${esc(name)}</strong> was born, holding ${esc(fmtAmount(entry.birthValue, network))}`
      : `<strong>${esc(name)}</strong> was born`;
  }
  if (ev.kind === 'transition') {
    /* count by seq value, not array index — older events may be truncated */
    const nth = entry.c.events.filter((e) => e.kind === 'transition' && e.seq <= ev.seq).length;
    const bal = withBalance ? entry.balances.get(ev.accepting_daa) : null;
    const balBit = bal ? ` — now holding ${esc(fmtAmount(bal, network))}` : '';
    return `<strong>${esc(name)}</strong> moved <span class="dim">(${ordinal(nth)} time)</span>${balBit}`;
  }
  const m = entry.moves;
  const tail = m === 0 ? 'without ever moving' : m === 1 ? 'after 1 move' : `after ${m} moves`;
  return `<strong>${esc(name)}</strong> retired ${tail}`;
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

function renderPulse(entry) {
  const { data, index } = entry;
  const events = [];
  for (const e of index.covs) {
    for (const ev of e.c.events) events.push({ kind: ev.kind, ms: daaToMs(ev.accepting_daa, data) });
  }
  const host = $('#pulse-chart');
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

  const caption = `${fmtInt(events.length)} life event${events.length === 1 ? '' : 's'} ` +
    `over ${fmtSpan(span)} · the latest ${relTime(max)}`;

  host.innerHTML =
    `<p class="pulse-caption">${esc(caption)}</p>` +
    `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="Bar chart of smart-coin life events over time" preserveAspectRatio="xMidYMid meet">` +
    `<line x1="${padX}" y1="${H - padB + 0.5}" x2="${W - padX}" y2="${H - padB + 0.5}" class="pulse-axis"/>` +
    bars + ticks + `</svg>` +
    `<div class="pulse-legend" aria-hidden="true">` +
    `<span><i class="dot dot-born"></i>born</span>` +
    `<span><i class="dot dot-move"></i>moved</span>` +
    `<span><i class="dot dot-burn"></i>retired</span></div>`;
}

/* ---------------------------------------------------------------- stories */

function buildFeed(entry) {
  const feed = [];
  for (const e of entry.index.covs) {
    for (const ev of e.c.events) feed.push({ entry: e, ev, daa: ev.accepting_daa });
  }
  feed.sort((a, b) => b.daa - a.daa);
  return feed;
}

function storyRow({ entry: e, ev }, data, network) {
  const meta = KIND_META[ev.kind] || KIND_META.transition;
  const ms = daaToMs(ev.accepting_daa, data);
  return `<li><a class="story ${meta.cls}" href="#/${esc(network)}/c/${esc(e.c.covenant_id)}">` +
    avatarSvg(e.c.covenant_id, 34) +
    `<span class="story-text">${eventSentence(e, ev, network)} <span class="story-when" title="${esc(utcTitle(ms))}">— ${esc(relTime(ms))}</span></span>` +
    `<span class="story-kind" aria-hidden="true">${ICONS[meta.icon]}</span>` +
    `</a></li>`;
}

function renderStories(entry, network) {
  $('#story-list').innerHTML = buildFeed(entry)
    .slice(0, STORY_COUNT)
    .map((f) => storyRow(f, entry.data, network))
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
      /* a full id that resolved nowhere — answer the tester's real question */
      grid.innerHTML = `<div class="no-results">` +
        `<p>kascov hasn’t seen <strong class="mono">${esc(shortHex(state.query, 12, 10))}</strong> in any covenant on ${esc(NETWORKS[network].label)}.</p>` +
        `<p class="dim">it may be a regular (non-covenant) transaction, still unconfirmed, or on the other network — ` +
        `<a href="${esc(txUrl(network, state.query))}" target="_blank" rel="noopener noreferrer">check it on the block explorer ↗</a></p></div>`;
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
    return `<article class="card">` +
      `<div class="card-head">${avatarSvg(e.c.covenant_id, 40)}` +
      `<div class="card-id"><a class="card-link" href="#/${esc(network)}/c/${esc(e.c.covenant_id)}">${esc(e.name)}</a>` +
      `<span class="pill ${alive ? 'pill-alive' : 'pill-retired'}">${alive ? 'alive' : 'retired'}</span></div>` +
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
  /* hex-looking input also suggests transaction matches (deep-linked) */
  if (/^[0-9a-f]{4,}$/.test(q)) {
    for (const e of entry.index.covs) {
      if (seen.has(e.c.covenant_id)) continue;
      if (e.c.genesis_txid && e.c.genesis_txid.startsWith(q)) {
        push(e, 'genesis tx', e.c.genesis_txid, 3);
      } else {
        const ev = e.c.events.find((x) => x.txid.startsWith(q));
        if (ev) push(e, 'transaction', ev.txid, 3);
      }
      if (out.length >= 32) break;
    }
  }
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
      `<span class="pill ${alive ? 'pill-alive' : 'pill-retired'}">${alive ? 'alive' : 'retired'}</span>` +
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

  if (empty) {
    $('#landing-empty').innerHTML = emptyCardHtml(network);
    return;
  }

  $('#teaser-list').innerHTML = buildFeed(entry)
    .slice(0, TEASER_COUNT)
    .map((f) => storyRow(f, data, network))
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
    $('#empty-net').innerHTML = emptyCardHtml(network);
    return;
  }

  $('#pulse-title').textContent = net.pulseTitle;
  renderWatchStrip(entry, network);
  renderPulse(entry);
  renderRecords(entry, network);
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
  const flash = flashTx && ev.txid === flashTx ? ' tl-flash' : '';
  return `<li class="tl-item ${meta.cls}${flash}" data-txid="${esc(ev.txid)}">` +
    `<span class="tl-icon" aria-hidden="true">${ICONS[meta.icon]}</span>` +
    `<div class="tl-body">` +
    `<p class="tl-text">${eventSentence(entry, ev, network, true)}</p>` +
    `<p class="tl-meta"><span title="${esc(utcTitle(ms))}">${esc(relTime(ms))}</span> · <a href="${esc(txUrl(network, ev.txid))}" target="_blank" rel="noopener noreferrer">view transaction ↗</a>${nerdBits}</p>` +
    `</div></li>`;
}

function nerdPanel(entry, network) {
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
  const fieldRow = (f) =>
    `<span class="tpl-field"><span class="dim">${esc(f.name)}</span> ` +
    `<span class="mono" title="${esc(f.value)}">${esc(shortHex(f.value, 12, 8))}</span></span>`;
  const templateLine = (name, fields) => name
    ? `<p class="tpl-line"><span class="flag flag-tpl">${esc(name)}</span>${(fields || []).map(fieldRow).join('')}</p>`
    : '';
  const utxos = (c.utxos || []).map((u) => {
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
        templateLine(u.revealed_template, u.revealed_fields) +
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
      `</div>`;
  }).join('');
  return `<dl class="nerd-rows">${rows.map(([k, v]) => `<div class="nerd-row"><dt>${esc(k)}</dt><dd>${v}</dd></div>`).join('')}</dl>` +
    `<h3 class="nerd-h">UTXOs (${(c.utxos || []).length})</h3>` +
    (utxos || '<p class="dim">no UTXOs recorded.</p>');
}

function renderDetail(entry, covId, flashTx) {
  const network = state.network;
  const { data, index } = entry;
  const view = $('#view-detail');
  const rec = index.byId.get(covId);

  if (!rec) {
    document.title = 'smart coin not found — kascov';
    const other = network === 'mainnet' ? 'testnet-10' : 'mainnet';
    view.innerHTML = `<a class="back" href="#/explore">← all smart coins</a>` +
      `<div class="empty-card"><h2>We haven’t met this smart coin.</h2>` +
      `<p class="dim">It isn’t in the ${esc(NETWORKS[network].label)} snapshot — it may live on the other network, or the id might be mistyped.</p>` +
      `<button type="button" class="btn" data-action="network" data-network="${other}">look on ${other}</button></div>`;
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
  summaryBits.push(`${c.genesis_daa != null ? 'born' : 'first seen'} ${relTime(rec.bornMs)}`);
  summaryBits.push(rec.moves === 0 ? 'never moved' : rec.moves === 1 ? 'moved once' : `moved ${rec.moves} times`);
  if (alive) {
    summaryBits.push(`currently holds ${fmtAmount(c.live_value, network)}${c.live_utxos > 1 ? ` in ${c.live_utxos} pieces` : ''}`);
  } else {
    summaryBits.push(`retired ${relTime(rec.lastMs)}`);
  }

  const preface = c.genesis_txid == null
    ? `<li class="tl-item tl-note"><span class="tl-icon" aria-hidden="true">${ICONS.move}</span><div class="tl-body">` +
      `<p class="tl-text dim">first seen mid-life — its earlier story happened before we started watching</p></div></li>`
    : '';
  const truncNote = c.events_truncated
    ? `<p class="dim trunc-note">part of this coin’s story is missing — it had more events than we keep per coin.</p>`
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
    `<ol class="timeline">${preface}${c.events.map((ev) => timelineItem(rec, ev, data, network, flashTx)).join('')}</ol></section>` +
    `<section class="nerd" aria-label="Technical details">` +
    `<button type="button" class="nerd-toggle" data-action="nerd" aria-expanded="${state.nerd}">` +
    `<span class="nerd-switch" aria-hidden="true"></span><span>nerd mode</span>` +
    `<span class="dim nerd-hint">raw ids, DAA scores, UTXOs &amp; scripts</span></button>` +
    `<div id="nerd-panel" class="nerd-panel" ${state.nerd ? '' : 'hidden'}>${state.nerd ? nerdPanel(rec, network) : ''}</div>` +
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

/* a KIP-20-style guard: input introspection asserting the covenant id —
   the same script kascov's own test suite decodes */
const DECODE_EXAMPLE = 'b9cf20' + '11'.repeat(32) + '8851';

function runDecode(updateHash) {
  const raw = $('#decode-input').value;
  const out = $('#decode-out');
  if (!raw.trim()) {
    out.innerHTML = '<p class="dim">paste a script above — the disassembly appears here.</p>';
    return;
  }
  const bytes = window.kascovDisasm.parseHex(raw);
  if (!bytes) {
    out.innerHTML = '<p class="decode-err">that doesn’t look like hex — expected an even number of 0-9a-f characters.</p>';
    return;
  }
  const { instructions, truncated } = window.kascovDisasm.disassemble(bytes);
  const groups = [...new Set(instructions.map((i) => i.group))];
  const summary =
    `<p class="decode-summary">` +
    `<span>${fmtInt(bytes.length)} byte${bytes.length === 1 ? '' : 's'} · ` +
    `${fmtInt(instructions.length)} instruction${instructions.length === 1 ? '' : 's'}</span>` +
    groups.map((g) => `<span class="op-chip op-${g}">${g}</span>`).join('') +
    (groups.includes('covenant') ? '<span class="flag flag-ops">covenant ops</span>' : '') +
    (groups.includes('zk') ? '<span class="flag flag-ops">zk ops</span>' : '') +
    (truncated ? '<span class="flag flag-no">truncated / malformed tail</span>' : '') +
    `</p>`;
  const rows = instructions.map((inst) => {
    const dataBit = inst.data && inst.data.length
      ? ` <span class="inst-data">0x${window.kascovDisasm.toHex(inst.data)}</span>` : '';
    return `<div class="inst g-${inst.group}">` +
      `<span class="inst-off">${inst.offset.toString(16).padStart(4, '0')}</span>` +
      `<span class="inst-hex">${inst.opcode.toString(16).padStart(2, '0')}</span>` +
      `<span class="inst-text"><span class="op-name">${esc(inst.name)}</span>${dataBit}</span>` +
      `</div>`;
  }).join('');
  out.innerHTML = summary + `<div class="inst-list">${rows}</div>`;
  if (updateHash) {
    /* replaceState keeps the link shareable without re-triggering render */
    const clean = raw.replace(/\s+/g, '');
    history.replaceState(null, '', `#/decode?s=${encodeURIComponent(clean)}`);
  }
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
  document.title = 'for developers — kascov';
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
    };
  }
  /* '#/explore' and '#/<network>/explore' */
  m = path.match(/^#\/(?:(testnet-10|mainnet)\/)?explore\/?$/);
  if (m) return { view: 'explore', network: m[1] || null };
  if (/^#\/decode\/?$/.test(path)) return { view: 'decode', network: null, s: params.get('s') || '' };
  if (/^#\/dev\/?$/.test(path)) return { view: 'dev', network: null };
  /* old home links '#/<network>' were data views — send them to the explorer */
  m = path.match(/^#\/(testnet-10|mainnet)\/?$/);
  if (m) return { view: 'explore', network: m[1] };
  return { view: 'landing', network: null };
}

function routeHash(view, id) {
  if (view === 'detail') return `#/${state.network}/c/${id}`;
  if (view === 'explore') return `#/${state.network}/explore`;
  /* decode/dev are network-free — switching networks keeps the page (and its query) */
  if (view === 'decode' || view === 'dev') return location.hash || `#/${view}`;
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
  const panel = $('#panel');
  const views = {
    landing: $('#view-landing'),
    explore: $('#view-explore'),
    detail: $('#view-detail'),
    decode: $('#view-decode'),
    dev: $('#view-dev'),
  };
  /* a stale cached index.html may predate newer views — never crash on them */
  for (const k of Object.keys(views)) if (!views[k]) delete views[k];
  if (!views[route.view]) route.view = 'landing';

  document.querySelectorAll('.network-tab').forEach((b) => {
    b.setAttribute('aria-pressed', String(b.dataset.network === state.network));
  });
  document.querySelectorAll('.nav-link').forEach((a) => {
    if (a.dataset.nav === route.view) a.setAttribute('aria-current', 'page');
    else a.removeAttribute('aria-current');
  });
  $('#header-search').hidden = route.view !== 'explore';

  /* the decoder and dev docs never need a snapshot — don't block them on data */
  if ((route.view === 'decode' || route.view === 'dev') && views[route.view]) {
    panel.hidden = true;
    for (const [name, el] of Object.entries(views)) el.hidden = name !== route.view;
    views.detail.innerHTML = '';
    if (route.view === 'decode') renderDecode(route);
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
    renderDetail(entry, route.id, route.tx);
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

function liveBadgeHtml(network) {
  const ls = state.live[network];
  const entry = state.cache[network];
  const tipAt = ls && ls.data && ls.data.tip_at_ms != null ? ls.data.tip_at_ms
    : entry && entry.data.tip_at_ms != null ? entry.data.tip_at_ms : null;
  if (tipAt != null) {
    if (Date.now() - tipAt < LIVE_FRESH_MS) {
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
    }
  } catch (e) {
    /* transient — the next tick retries */
  }
}
setInterval(pollLive, LIVE_MS);

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') {
    refreshSnapshot();
    pollLive();
  }
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
    document.querySelectorAll('.chip').forEach((c) => {
      c.setAttribute('aria-pressed', String(c.dataset.filter === state.filter));
    });
    const entry = state.cache[state.network];
    if (entry) renderGrid(entry, state.network);
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
      renderDetail(entry, route.id);
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
  } else if (action === 'decode-example') {
    $('#decode-input').value = DECODE_EXAMPLE;
    runDecode(true);
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
  }
});

$('#search').addEventListener('input', (e) => {
  const hadQuery = Boolean(state.query);
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
  /* a pasted 64-hex id resolves instantly — coin id or txid, straight to the coin */
  if (/^[0-9a-f]{64}$/.test(state.query)) {
    const byId = entry.index.byId.get(state.query);
    const byTx = byId ? null : entry.index.txToCov.get(state.query);
    if (byId || byTx) {
      const target = byId || byTx;
      const tx = byTx ? `?tx=${state.query}` : '';
      e.target.value = '';
      state.query = '';
      closeSuggest();
      location.hash = `#/${state.network}/c/${target.c.covenant_id}${tx}`;
      return;
    }
  }
  renderGrid(entry, state.network);
  renderSuggest();
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

/* pasted clean URLs (hosting rewrites everything to this page):
   /explore, /decode?s=…, /testnet-10/c/<id> → the same hash routes */
if (location.pathname !== '/' && location.pathname !== '/index.html' && !location.hash) {
  const path = location.pathname.replace(/^\/+|\/+$/g, '');
  history.replaceState(null, '', `/#/${path}${location.search}`);
}

render();
pollLive();

})();
