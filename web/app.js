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
    explorer: 'https://explorer-tn10.kaspa.org',
    pulseTitle: 'life on the testnet',
  },
  'mainnet': {
    label: 'mainnet',
    word: 'mainnet',
    unit: 'KAS',
    explorer: 'https://explorer.kaspa.org',
    pulseTitle: 'life on mainnet',
  },
};

const MS_PER_DAA = 100;   // the chain ticks ~10 DAA per second
const PAGE_SIZE = 60;
const STORY_COUNT = 15;
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
    return { daa: data.tip_daa, ms: data.generated_at_ms, exact: true };
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

function txUrl(network, txid) {
  return `${NETWORKS[network].explorer}/txs/${txid}`;
}

/* ----------------------------------------------------------------- state */

const state = {
  network: 'testnet-10',
  cache: {},          // network -> { data, index }
  filter: 'all',
  query: '',
  shown: PAGE_SIZE,
  nerd: false,
};

try { state.nerd = localStorage.getItem('kascov-nerd') === '1'; } catch (e) { /* private mode */ }

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

function renderStories(entry, network) {
  const { data, index } = entry;
  const feed = [];
  for (const e of index.covs) {
    for (const ev of e.c.events) feed.push({ entry: e, ev, daa: ev.accepting_daa });
  }
  feed.sort((a, b) => b.daa - a.daa);
  $('#story-list').innerHTML = feed.slice(0, STORY_COUNT).map(({ entry: e, ev }) => {
    const meta = KIND_META[ev.kind] || KIND_META.transition;
    const when = relTime(daaToMs(ev.accepting_daa, data));
    return `<li><a class="story ${meta.cls}" href="#/${esc(network)}/c/${esc(e.c.covenant_id)}">` +
      avatarSvg(e.c.covenant_id, 34) +
      `<span class="story-text">${eventSentence(e, ev, network)} <span class="story-when">— ${esc(when)}</span></span>` +
      `<span class="story-kind" aria-hidden="true">${ICONS[meta.icon]}</span>` +
      `</a></li>`;
  }).join('');
}

/* ------------------------------------------------------------------- grid */

function matchesFilter(entry) {
  if (state.filter !== 'all' && entry.c.status !== state.filter) return false;
  if (state.query && !entry.blob.includes(state.query)) return false;
  return true;
}

function renderGrid(entry, network) {
  const list = entry.index.covs.filter(matchesFilter);
  const total = entry.index.covs.length;
  $('#result-count').textContent = (state.query || state.filter !== 'all')
    ? `${list.length} of ${total} smart coin${total === 1 ? '' : 's'}`
    : `${total} smart coin${total === 1 ? '' : 's'}`;

  const grid = $('#coin-grid');
  const foot = $('#grid-foot');
  if (!list.length) {
    const example = entry.index.covs[0] ? entry.index.covs[0].name : 'brave-teal-otter';
    grid.innerHTML = `<div class="no-results"><p>No smart coins match${state.query ? ` <strong>“${esc(state.query)}”</strong>` : ' that filter'}.</p>` +
      `<p class="dim">Try a friendly name like <em>${esc(example)}</em>, or paste a coin’s id or a transaction id.</p></div>`;
    foot.innerHTML = '';
    return;
  }
  grid.innerHTML = list.slice(0, state.shown).map((e) => {
    const alive = e.c.status === 'active';
    return `<a class="card" href="#/${esc(network)}/c/${esc(e.c.covenant_id)}">` +
      `<div class="card-head">${avatarSvg(e.c.covenant_id, 40)}` +
      `<div class="card-id"><span class="card-name">${esc(e.name)}</span>` +
      `<span class="pill ${alive ? 'pill-alive' : 'pill-retired'}">${alive ? 'alive' : 'retired'}</span></div></div>` +
      `<p class="card-story">${esc(cardStory(e, network))}</p>` +
      `</a>`;
  }).join('');
  foot.innerHTML = list.length > state.shown
    ? `<button type="button" class="btn" data-action="more">show more <span class="dim">(${list.length - state.shown} hidden)</span></button>`
    : '';
}

/* ------------------------------------------------------------------- home */

function renderHome(entry) {
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
  $('#freshness').textContent = bits.join(' · ');

  const empty = data.covenants.length === 0;
  $('#empty-net').hidden = !empty;
  $('#section-pulse').hidden = empty;
  $('#section-stories').hidden = empty;
  $('#section-coins').hidden = empty;

  if (empty) {
    $('#empty-net').innerHTML =
      `<div class="empty-card">` +
      `<span class="empty-icon" aria-hidden="true">${ICONS.born}</span>` +
      `<h2>${network === 'mainnet' ? 'Mainnet’s' : 'This network’s'} first smart coin hasn’t been born yet.</h2>` +
      `<p>The moment it happens, kascov will be watching — and remembering.</p>` +
      (network === 'mainnet'
        ? `<button type="button" class="btn btn-accent" data-action="network" data-network="testnet-10">meanwhile, watch the testnet</button>`
        : '') +
      `</div>`;
    return;
  }

  $('#pulse-title').textContent = net.pulseTitle;
  renderPulse(entry);
  renderStories(entry, network);
  renderGrid(entry, network);
}

/* ----------------------------------------------------------------- detail */

function timelineItem(entry, ev, data, network) {
  const meta = KIND_META[ev.kind] || KIND_META.transition;
  const when = relTime(daaToMs(ev.accepting_daa, data));
  const nerdBits = state.nerd
    ? ` · <span class="mono dim">DAA ${esc(fmtInt(ev.accepting_daa))}</span>`
    : '';
  return `<li class="tl-item ${meta.cls}">` +
    `<span class="tl-icon" aria-hidden="true">${ICONS[meta.icon]}</span>` +
    `<div class="tl-body">` +
    `<p class="tl-text">${eventSentence(entry, ev, network, true)}</p>` +
    `<p class="tl-meta">${esc(when)} · <a href="${esc(txUrl(network, ev.txid))}" target="_blank" rel="noopener noreferrer">view transaction ↗</a>${nerdBits}</p>` +
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
  const utxos = (c.utxos || []).map((u) => {
    const badges = [
      u.live ? '<span class="flag flag-yes">live</span>' : '<span class="flag flag-off">spent</span>',
      u.uses_covenant_ops ? '<span class="flag flag-ops">covenant ops</span>' : '',
      u.uses_zk_ops ? '<span class="flag flag-ops">zk ops</span>' : '',
    ].filter(Boolean).join(' ');
    return `<div class="utxo">` +
      `<div class="utxo-head"><span class="mono break">${esc(u.outpoint)}</span><span class="utxo-flags">${badges}</span></div>` +
      `<div class="utxo-meta"><span>${esc(fmtAmount(u.value, network))}</span><span class="dim">created at DAA ${esc(fmtInt(u.created_daa))}</span></div>` +
      `<pre class="script">${esc((u.script_asm || []).join('\n'))}</pre>` +
      `</div>`;
  }).join('');
  return `<dl class="nerd-rows">${rows.map(([k, v]) => `<div class="nerd-row"><dt>${esc(k)}</dt><dd>${v}</dd></div>`).join('')}</dl>` +
    `<h3 class="nerd-h">UTXOs (${(c.utxos || []).length})</h3>` +
    (utxos || '<p class="dim">no UTXOs recorded.</p>');
}

function renderDetail(entry, covId) {
  const network = state.network;
  const { data, index } = entry;
  const view = $('#view-detail');
  const rec = index.byId.get(covId);

  if (!rec) {
    document.title = 'smart coin not found — kascov';
    const other = network === 'mainnet' ? 'testnet-10' : 'mainnet';
    view.innerHTML = `<a class="back" href="#/${esc(network)}">← all smart coins</a>` +
      `<div class="empty-card"><h2>We haven’t met this smart coin.</h2>` +
      `<p class="dim">It isn’t in the ${esc(NETWORKS[network].label)} snapshot — it may live on the other network, or the id might be mistyped.</p>` +
      `<button type="button" class="btn" data-action="network" data-network="${other}">look on ${other}</button></div>`;
    return;
  }

  const c = rec.c;
  const alive = c.status === 'active';
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
    `<a class="back" href="#/${esc(network)}">← all smart coins</a>` +
    `<header class="detail-head">` +
    `<span role="img" aria-label="avatar of ${esc(rec.name)}">${avatarSvg(c.covenant_id, 88)}</span>` +
    `<div class="detail-id">` +
    `<h1>${esc(rec.name)}</h1>` +
    `<p class="detail-tags"><span class="pill ${alive ? 'pill-alive' : 'pill-retired'}">${alive ? 'alive' : 'retired'}</span>` +
    `<span class="dim">smart coin on ${esc(NETWORKS[network].label)}</span></p>` +
    `<p class="id-chip"><span class="mono">${esc(shortHex(c.covenant_id, 10, 8))}</span>` +
    `<button type="button" class="copy-btn" data-action="copy" data-copy="${esc(c.covenant_id)}" aria-label="copy this coin’s full id">copy id</button></p>` +
    `</div></header>` +
    `<p class="detail-summary">${esc(summaryBits.join(' · '))}.</p>` +
    `<section aria-label="Life story"><h2>life story</h2>${truncNote}` +
    `<ol class="timeline">${preface}${c.events.map((ev) => timelineItem(rec, ev, data, network)).join('')}</ol></section>` +
    `<section class="nerd" aria-label="Technical details">` +
    `<button type="button" class="nerd-toggle" data-action="nerd" aria-expanded="${state.nerd}">` +
    `<span class="nerd-switch" aria-hidden="true"></span><span>nerd mode</span>` +
    `<span class="dim nerd-hint">raw ids, DAA scores, UTXOs &amp; scripts</span></button>` +
    `<div id="nerd-panel" class="nerd-panel" ${state.nerd ? '' : 'hidden'}>${state.nerd ? nerdPanel(rec, network) : ''}</div>` +
    `</section>`;
}

/* ---------------------------------------------------------------- routing */

function parseRoute() {
  const h = location.hash || '#/';
  /* '#/<network>/c/<id>' and '#/<network>'; bare '#/c/<id>' and '#/' keep
     the current network for back-compat with old links */
  let m = h.match(/^#\/(?:(testnet-10|mainnet)\/)?c\/([0-9a-fA-F]{6,64})$/);
  if (m) return { view: 'detail', network: m[1] || null, id: m[2].toLowerCase() };
  m = h.match(/^#\/(testnet-10|mainnet)\/?$/);
  if (m) return { view: 'home', network: m[1] };
  return { view: 'home', network: null };
}

function routeHash(view, id) {
  return view === 'detail' ? `#/${state.network}/c/${id}` : `#/${state.network}`;
}

let renderToken = 0;

async function render() {
  const token = ++renderToken;
  const route = parseRoute();
  if (route.network && NETWORKS[route.network] && route.network !== state.network) {
    state.network = route.network;
    state.shown = PAGE_SIZE;
  }
  const panel = $('#panel');
  const home = $('#view-home');
  const detail = $('#view-detail');

  document.querySelectorAll('.network-tab').forEach((b) => {
    b.setAttribute('aria-pressed', String(b.dataset.network === state.network));
  });

  let entry = state.cache[state.network];
  if (!entry) {
    panel.hidden = false;
    panel.className = 'panel';
    panel.innerHTML = `<p>pointing the camera at ${esc(NETWORKS[state.network].label)}…</p>`;
    home.hidden = true;
    detail.hidden = true;
    try {
      entry = await loadNetwork(state.network);
    } catch (err) {
      if (token !== renderToken) return;
      panel.hidden = false;
      panel.className = 'panel panel-error';
      panel.innerHTML = `<p>Couldn’t load the ${esc(NETWORKS[state.network].label)} snapshot (${esc(err.message)}).</p>` +
        `<button type="button" class="btn" data-action="retry">try again</button>`;
      return;
    }
    if (token !== renderToken) return;
  }

  panel.hidden = true;

  if (route.view === 'detail') {
    home.hidden = true;
    detail.hidden = false;
    renderDetail(entry, route.id);
    detail.classList.remove('fade-in');
    void detail.offsetWidth;
    detail.classList.add('fade-in');
    /* jump like a page navigation — CSS smooth-scroll is for anchors only */
    window.scrollTo({ top: 0, behavior: 'instant' });
  } else {
    detail.hidden = true;
    detail.innerHTML = '';
    home.hidden = false;
    renderHome(entry);
    home.classList.remove('fade-in');
    void home.offsetWidth;
    home.classList.add('fade-in');
  }
}

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
    /* encode the network in the hash so the choice survives reloads
       and shared links land on the right network */
    const route = parseRoute();
    const target = routeHash(route.view, route.id);
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
  /* forgive pasted outpoints ('txid:0') and spaced names ('proud olive stoat') */
  state.query = e.target.value.trim().toLowerCase()
    .replace(/:\d+$/, '')
    .replace(/\s+/g, '-');
  state.shown = PAGE_SIZE;
  const entry = state.cache[state.network];
  if (entry) renderGrid(entry, state.network);
});

window.addEventListener('hashchange', render);

/* fill static guide icons */
document.querySelectorAll('.guide-icon').forEach((el) => {
  el.innerHTML = ICONS[el.dataset.icon] || '';
});

render();

})();
