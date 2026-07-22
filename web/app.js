/* kascov — a camera pointed at Kaspa's smart coins.
   Pure vanilla JS, no dependencies, no build step.
   Loaded as an ES module: module scope replaces the old IIFE (same isolation,
   strict mode by default), and imports let the file shed pieces into
   web/core/* modules without a bundler. */

import {
  idByte, friendlyName, semanticTemplate, avatarSvg,
  ICONS, KIND_META, GLOSSARY,
  esc, ordinal, fmtInt,
  relTime, relTimeShort, fmtClock, fmtSpan, shortHex, leAmount,
  lineageBadge, payloadPeek, utcTitle, absShort,
} from './core/format.js';
import {
  NETWORKS, MS_PER_DAA, PAGE_SIZE, GRID_PAGE, STORY_COUNT, TEASER_COUNT,
  PULSE_BUCKETS, ACTIVITY_RANGES, ACTIVITY_LABELS, ACTIVITY_PHRASE,
  ACTIVITY_MAX_COLS, ADDR_RE, PUBKEY_RE,
  fmtAmount, makeAnchor, daaToMs, txUrl,
  state, loadWatch, saveWatch,
} from './core/state.js';
import { loadPrice, amountWithUsd, usdToggleHtml, toggleUsd } from './core/price.js';
import {
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
} from './core/data.js';




/* ----------------------------------------------------------------- utils */

const $ = (sel) => document.querySelector(sel);




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
  /* raw 0x tags are honest data but noisy chrome: fold them into one row
     so named namespaces surface — the fold states its own count. */
  const hexTags = lanes.filter((l) => l.kind !== 'inscription' && String(l.label || '').startsWith('0x'));
  const namedLanes = lanes.filter((l) => !hexTags.includes(l));
  const hexRow = hexTags.length
    ? `<div class="lane-row"><span class="lane-ns dim" title="4-byte tags with no printable label — real payload namespaces, folded so the named ones stay readable">unlabeled binary tags (${fmtInt(hexTags.length)})</span>` +
      `<span class="lane-track"></span>` +
      `<span class="lane-counts dim">${fmtInt(hexTags.reduce((n, l) => n + l.events, 0))} txs · ${fmtInt(hexTags.reduce((n, l) => n + l.covenants, 0))} coins</span></div>`
    : '';
  const max = Math.max(1, ...namedLanes.map((l) => l.events), 1);
  host.innerHTML = namedLanes.slice(0, 14).map((l) => {
    const w = Math.max((l.events / max) * 100, 3).toFixed(1);
    const name = l.kind === 'inscription' ? 'JSON inscriptions' : esc(l.label);
    const title = l.ascii ? ` title="0x${esc(l.hex)}"` : '';
    /* KIP-21 lanes with a namespace hex get their own dashboard page (tag /
       inscription buckets don't — the lane endpoint only tracks true lanes) */
    if (l.kind === 'lane' && /^[0-9a-f]{8}$/i.test(l.hex || '')) {
      return `<div class="lane-row"><a class="lane-ns lane-ns-link" href="#/${esc(network)}/lane/${esc(l.hex.toLowerCase())}"${title}>${name} <span class="lane-ns-arrow">→</span></a>` +
        `<span class="lane-track"><span class="lane-fill" style="width:${w}%"></span></span>` +
        `<span class="lane-counts dim">${fmtInt(l.events)} tx${l.events === 1 ? '' : 's'} · ${fmtInt(l.covenants)} coin${l.covenants === 1 ? '' : 's'}</span></div>`;
    }
    return `<div class="lane-row"><span class="lane-ns"${title}>${name}</span>` +
      `<span class="lane-track"><span class="lane-fill" style="width:${w}%"></span></span>` +
      `<span class="lane-counts dim">${fmtInt(l.events)} tx${l.events === 1 ? '' : 's'} · ${fmtInt(l.covenants)} coin${l.covenants === 1 ? '' : 's'}</span></div>`;
  }).join('') + hexRow;
}

/* live pending (mempool) covenant feed — rows appear as covenant txs enter the
   node's mempool and clear as a block confirms them (or the pool drops them).
   pendingLive[network] is a Map<txid, { covenant_id, tx_kind, ts, resolution? }>
   in insertion order, seeded once from the /pending snapshot and kept live by
   the SSE stream. Kept separate from state.pending (the fetch cache) so the
   stream can mutate it between snapshot reloads. */
const pendingLive = {};
const PENDING_ROWS_MAX = 24;   // DOM cap — mempool is a ticker, not an archive
const PENDING_LIVE_MAX = 128;  // client-side entry cap (server caps its snapshot)

function renderPending(network) {
  if (state.network !== network || parseRoute().view !== 'explore') return;
  const section = $('#section-pending');
  const host = $('#pending-row');
  if (!section || !host) return;
  const map = pendingLive[network];
  if (!map) {
    /* lazy seed from the snapshot; a 404 (no-mempool node / old worker) seeds
       an empty map so the section simply stays hidden — feature-detected. */
    loadPending(network).then((d) => {
      if (!pendingLive[network]) pendingLive[network] = new Map();
      if (d && Array.isArray(d.pending)) {
        for (const p of d.pending) {
          if (p && p.txid) {
            pendingLive[network].set(p.txid, { covenant_id: p.covenant_id, tx_kind: p.tx_kind, ts: Date.now() });
          }
        }
      }
      if (state.network === network && parseRoute().view === 'explore') renderPending(network);
    });
    section.hidden = true;
    return;
  }
  /* feature detection: hide the whole section ONLY when the worker doesn't
     serve /pending (404 leaves state.pending[network].data null). Once the
     feature is supported the section STAYS in place, so a tx arriving or
     clearing fills or empties it without ever shoving the page up or down. */
  const probed = state.pending[network];
  if (probed && probed.data === null) { section.hidden = true; return; }
  section.hidden = false;
  const rows = [...map.entries()];
  const cnt = $('#pending-count');
  if (cnt) cnt.textContent = rows.length ? String(rows.length) : '';
  if (!rows.length) {
    host.innerHTML = '<div class="pending-empty dim">nothing pending right now, covenant transactions appear here the moment the node sees them</div>';
    return;
  }
  /* newest at the bottom: as fresh txs arrive the window scrolls up and the
     oldest slides off the top, so the locked-height box never grows the page. */
  host.innerHTML = rows.slice(-PENDING_ROWS_MAX).map(([txid, p]) => {
    const meta = KIND_META[p.tx_kind] || KIND_META.transition;
    const name = p.covenant_id ? friendlyName(p.covenant_id) : shortHex(txid, 8, 6);
    const res = p.resolution === 'confirmed' ? ' pending-confirmed'
      : (p.resolution === 'dropped' ? ' pending-dropped' : '');
    return `<div class="pending-row ${esc(meta.cls)}${res}" data-txid="${esc(txid)}">` +
      `<span class="pending-kind">${esc(p.tx_kind || 'pending')}</span>` +
      `<span class="lane-ns pending-name">${esc(name)}</span>` +
      `<span class="lane-counts dim mono pending-tx">${esc(shortHex(txid, 8, 6))}` +
      `<span class="pending-spin" aria-hidden="true">⏳</span></span></div>`;
  }).join('');
}

/* record a freshly-seen pending covenant tx from the stream, evicting the
   oldest if the client map is over its cap. */
function notePending(network, txid, covenant_id, tx_kind) {
  if (!pendingLive[network]) pendingLive[network] = new Map();
  const map = pendingLive[network];
  map.delete(txid); // re-insert moves it to newest
  map.set(txid, { covenant_id, tx_kind, ts: Date.now() });
  while (map.size > PENDING_LIVE_MAX) map.delete(map.keys().next().value);
}

/* resolve a tracked pending tx: mark it (confirmed flash / dropped fade), then
   evict it after the animation and re-render. */
function resolvePending(network, txid, resolution) {
  const map = pendingLive[network];
  if (!map || !map.has(txid)) return;
  map.get(txid).resolution = resolution;
  renderPending(network);
  setTimeout(() => {
    const m = pendingLive[network];
    if (m) m.delete(txid);
    renderPending(network);
  }, resolution === 'confirmed' ? 900 : 700);
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

/* Load a script on demand, once. galaxy.js is the largest optional lib and
   most visits never open the galaxy section — so it stays out of index.html
   and arrives only when actually needed. */
const scriptPromises = {};
function ensureScript(src) {
  if (!scriptPromises[src]) {
    scriptPromises[src] = new Promise((resolve, reject) => {
      const s = document.createElement('script');
      s.src = src;
      s.onload = resolve;
      s.onerror = () => { delete scriptPromises[src]; reject(new Error(`failed to load ${src}`)); };
      document.head.appendChild(s);
    });
  }
  return scriptPromises[src];
}

/* the galaxy — the whole-network map: every app at once, positions +
   weighted edges precomputed by the worker (data/<net>/galaxy.json). Rendered
   lazily by web/galaxy.js when the section is expanded. */
let galaxyCtrl = null;
let galaxyMounted = null;      // which network the live controller shows
let galaxyLinkDone = false;    // '?galaxy=1' deep link honored once per load

/* Collapsible-section promotion: default-open ONCE per user, then respect
   whatever they last chose (the toggle listener persists open/closed under
   the same key). Values: unset → first visit, 'open' / 'closed' after. */
function promoteSection(sec, key) {
  try {
    const pref = localStorage.getItem(key);
    if (pref === null) { sec.open = true; localStorage.setItem(key, 'open'); }
    else if (pref === 'open') sec.open = true;
    else if (pref === 'closed') sec.open = false;
  } catch (e) { /* private mode — leave the default (closed) */ }
}

/* core-tier first paint → fetch the full node set (?fmt=2) in the background
   and hot-swap it into the live controller. Positions and bounds are
   identical across tiers (worker invariant), so nothing jumps and the
   current pan/zoom is preserved. */
const galaxyUpgrading = {};
function upgradeGalaxy(network) {
  if (galaxyUpgrading[network]) return;
  galaxyUpgrading[network] = true;
  fetch(`data/${network}/galaxy.json?fmt=2`, { cache: 'no-cache' })
    .then((res) => (res.ok ? res.json() : null))
    .then((full) => {
      delete galaxyUpgrading[network];
      const count = full && (((full.ids && full.ids.length) || (full.nodes && full.nodes.length)) || 0);
      if (!count) return;
      if (galaxyCtrl && galaxyMounted === network) {
        galaxyCtrl.load(full, { preserveView: true });
      }
      /* the cache entry keeps only the light fields (heavy arrays live in the
         controller's typed arrays now) — record that it's the full set */
      const entry = galaxyCache[network];
      if (entry && entry.data) { entry.data.nodeCount = count; delete entry.data.tier; }
    })
    .catch(() => { delete galaxyUpgrading[network]; });
}

function renderGalaxyLegend(data) {
  const host = $('#galaxy-legend');
  if (!host || !galaxyCtrl) return;
  const swatch = (label, color) =>
    `<span class="lg-item"><span class="lg-dot" style="background:${esc(color)}"></span>${esc(label)}</span>`;
  const parts = (data.templates || []).slice(0, 8).map(
    (n, i) => swatch(n.replace('SilverScript · ', ''), galaxyCtrl.colorForTemplate(i))
  );
  parts.push(swatch('other', 'rgba(150,160,180,0.85)'));
  host.innerHTML = parts.join('');
}

function renderGalaxy() {
  const gsec = $('#section-galaxy');
  const canvas = $('#galaxy-canvas');
  if (!gsec || !gsec.open || !canvas) return;
  if (!window.kascovGalaxy) {
    /* first open: pull the renderer in, then come back */
    ensureScript('/galaxy.js').then(() => {
      if (gsec.open && parseRoute().view === 'explore') renderGalaxy();
    }).catch(() => {});
    return;
  }
  const network = state.network;
  const cached = galaxyCache[network];
  if (!cached) {
    loadGalaxy(network).then(() => {
      if (state.network === network && gsec.open && parseRoute().view === 'explore') renderGalaxy();
    });
    return;
  }
  const data = cached.data;
  // galaxy.json missing/empty (e.g. an older worker without the endpoint):
  // hide the section rather than leave a blank canvas + empty legend.
  // nodeCount survives the heavy-array release below, so remounts still pass.
  // A core-tier reply may carry zero nodes (all clusters small) while the
  // network still has some — nodes_total is the full count in that case.
  const nodeCount = data && (data.nodeCount || data.nodes_total ||
    (data.nodes && data.nodes.length) || (data.ids && data.ids.length) || 0);
  if (!data || !nodeCount) { gsec.hidden = true; return; }
  if (galaxyCtrl && galaxyMounted === network) { galaxyCtrl.resize(); return; }
  if (!data.nodes && !data.ids) {
    // heavy arrays (row or columnar) were released after an earlier mount
    // (below) — a fresh mount (network switch back) needs them again;
    // refetch once.
    delete galaxyCache[network];
    loadGalaxy(network).then(() => {
      if (state.network === network && gsec.open && parseRoute().view === 'explore') renderGalaxy();
    });
    return;
  }
  if (galaxyCtrl) galaxyCtrl.destroy();
  galaxyCtrl = window.kascovGalaxy.create(canvas, {
    friendlyName,
    onPickCoin: (id) => { location.hash = `#/${network}/c/${id}`; },
  });
  galaxyMounted = network;
  const isCoreTier = data.tier === 'core';
  galaxyCtrl.load(data);
  // the controller adopted/copied everything into typed arrays — release the
  // raw JSON arrays (multi-MB at production scale) instead of caching them
  // twice. Keep the light fields (templates for the legend, nodeCount for
  // the gate). Columnar payloads release their parallel arrays the same way.
  data.nodeCount = nodeCount;
  data.nodes = null;
  data.edges = null;
  data.apps = null;
  data.ids = data.nx = data.ny = data.nr = data.nt = data.ns = data.na = null;
  data.acx = data.acy = data.ar = data.asz = data.at = null;
  renderGalaxyLegend(data);
  // core tier on screen → pull the full set in behind it and hot-swap
  if (isCoreTier) upgradeGalaxy(network);
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
  /* genesis0 bulk listings move thousands of coins in one marketplace tx —
     real activity, but one operator's plumbing, not thousands of "apps".
     Honest counting: fold them into a single summary card and keep the
     headline count organic. */
  const isBatch = (f) => {
    const named = f.members.filter((m) => m.template && !/^p2(pk|sh)/.test(m.template));
    return named.length > 0 && named.every((m) => /^genesis0/.test(m.template));
  };
  const batches = fams.filter(isBatch);
  const organic = fams.filter((f) => !isBatch(f));
  const fcnt = $('#families-count');
  if (fcnt) fcnt.textContent = `${fmtInt(organic.length)} app${organic.length === 1 ? '' : 's'}` +
    (batches.length ? ` · ${fmtInt(batches.length)} marketplace batches` : '');
  // the galaxy section shares this families data
  const gsec = $('#section-galaxy');
  if (gsec) {
    const graphable = fams.filter((f) => f.size >= 2).length;
    if (graphable) {
      gsec.hidden = false;
      const gcnt = $('#galaxy-count');
      if (gcnt) gcnt.textContent = `${fmtInt(graphable)} app${graphable === 1 ? '' : 's'}`;
      promoteSection(gsec, 'kascov-galaxy-seen');
      /* '#/explore?galaxy=1' deep link (landing teaser card) — open the
         galaxy and bring it into view, once per page load */
      if (!galaxyLinkDone && parseRoute().galaxy) {
        galaxyLinkDone = true;
        gsec.open = true;
        gsec.scrollIntoView({ behavior: 'smooth', block: 'start' });
      }
      if (gsec.open) renderGalaxy();
    } else {
      gsec.hidden = true;
    }
  }
  const batchCard = batches.length
    ? `<div class="family-card"><div class="family-head"><span class="family-label">genesis0 · marketplace batches</span>` +
      `<span class="family-sub dim">${fmtInt(batches.length)} bulk listing transactions folded into this card — the biggest moved ` +
      `${fmtInt(Math.max(...batches.map((x) => x.size)))} coins at once. one marketplace's plumbing, counted honestly as one app.</span></div></div>`
    : '';
  host.innerHTML = batchCard + organic.slice(0, 6).map((f) => {
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
  renderCommunity();
  renderWhatsNew();
  updateTourOffer();
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
  renderPending(network);
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
  renderAnalytics(network);
  const liteEvents = filterEventsByKind(dedupByCovenant(live.recent_events), state.storyKind)
    .slice(0, STORY_COUNT);
  $('#story-list').innerHTML = liteEvents.length
    ? liteEvents.map((ev) => liteStoryRow(ev, live, network)).join('')
    : storyEmptyHtml();
  syncFilterChips(network);
  $('#result-count').textContent = `${fmtInt(s.covenants)} smart coins`;
  $('#coin-grid').innerHTML =
    `<div class="no-results"><p class="dim">loading all ${esc(fmtInt(s.covenants))} smart coins…</p></div>`;
  $('#grid-foot').innerHTML = '';
}

/* ------------------------------------------------------------- sentences */

/* stories/timeline chip label → wire event kind; 'all' passes everything */
const KIND_BY_CHIP = { born: 'genesis', moved: 'transition', retired: 'burn' };
function filterEventsByKind(events, chip) {
  const kind = KIND_BY_CHIP[chip];
  return kind ? events.filter((ev) => ev.kind === kind) : events;
}
const storyEmptyHtml = () =>
  `<li class="story-none dim">nothing ${esc(state.storyKind)} in the latest events — try another filter</li>`;

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
      ? `<strong>${esc(name)}</strong> was born, holding ${esc(amountWithUsd(entry.birthValue, network))}${esc(pieceBit)}`
      : `<strong>${esc(name)}</strong> was born`;
  }
  if (ev.kind === 'transition') {
    /* count by seq value, not array index — older events may be truncated */
    const nth = entry.c.events.filter((e) => e.kind === 'transition' && e.seq <= ev.seq).length;
    const bal = withBalance ? entry.balances.get(ev.accepting_daa) : null;
    const balBit = bal ? ` — now holding ${esc(amountWithUsd(bal, network))}` : '';
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
  const isFinal = !isAlive(entry.c) && lastEvent && ev.seq === lastEvent.seq;
  const gone = eventShape(entry, ev).consumedValue;
  const goneBit = gone > 0 ? ` — ${esc(amountWithUsd(gone, network))} left the covenant` : '';
  if (!isFinal) {
    const bal = withBalance ? entry.balances.get(ev.accepting_daa) : null;
    const balBit = bal ? `, ${esc(amountWithUsd(bal, network))} lives on` : '';
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
  if (isAlive(c)) bits.push(`holds ${fmtAmount(c.live_value, network)}`);
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
    if (!isAlive(e.c)) events.push({ kind: 'burn', ms: e.lastMs });
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
    kind: !isAlive(e.c) ? 'burn' : (e.moves > 0 ? 'transition' : 'genesis'),
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
  /* the kind chip narrows BEFORE the truncation so a filter can surface
     matches from the whole feed, not just the top slice */
  const shown = filterEventsByKind(events, state.storyKind).slice(0, STORY_COUNT);
  $('#story-list').innerHTML = shown.length
    ? shown.map((ev) => storyRow(ev, entry, live, network)).join('')
    : storyEmptyHtml();
}

/* stories chip click: repaint whichever feed variant is on screen */
function repaintStories() {
  const network = state.network;
  const entry = state.cache[network];
  if (entry) { renderStories(entry, network); return; }
  const live = state.live[network] && state.live[network].data;
  if (!live || !Array.isArray(live.recent_events)) return;
  const evs = filterEventsByKind(dedupByCovenant(live.recent_events), state.storyKind)
    .slice(0, STORY_COUNT);
  $('#story-list').innerHTML = evs.length
    ? evs.map((ev) => liteStoryRow(ev, live, network)).join('')
    : storyEmptyHtml();
}

/* --------------------------------------------- known bulk-traffic keys */

/* Keys we can name honestly. The one entry today is the bot that floods
   testnet-10 with plain p2pk covenant states — 157k+ of TN10's coins are
   its, and pretending they're organic apps would be a lie of omission. */
const GENERATOR_LABEL = 'TN10 traffic generator';
const KNOWN_KEYS = {
  'a39404c7f7a004b5eb51966981abeab40df55abf6f631c48399a42066bf182fa': GENERATOR_LABEL,
};

function knownKeyLabel(pubkey) {
  return KNOWN_KEYS[String(pubkey || '').toLowerCase()] || null;
}

/* Is this coin the traffic generator's? Exact when we hold its p2pk holders
   (detail payloads carry the keys); grid rows don't, so there the TN10 proxy
   is the bare 'p2pk state' shape — 157,484 of the 157,506 such coins are the
   generator's at last count, and the coin page verifies by key. */
function generatorLabel(c, network) {
  if (Array.isArray(c.holders) && c.holders.length) {
    for (const h of c.holders) {
      const l = knownKeyLabel(h.pubkey);
      if (l) return l;
    }
    return null; /* holders known, none match — organic for sure */
  }
  return network === 'testnet-10' && c.template === 'p2pk state' ? GENERATOR_LABEL : null;
}

/* Per-network filter default: testnet-10 opens on 'organic' (the generator's
   bulk coins hidden, counts still shown) and mainnet on 'all'. An explicit
   chip choice sticks across switches — except 'organic', which only exists
   where a known generator does. */
function networkFilterReset() {
  if (!state.filterTouched || (state.filter === 'organic' && state.network !== 'testnet-10')) {
    state.filter = state.network === 'testnet-10' ? 'organic' : 'all';
  }
}

/* ------------------------------------------------------------------- grid */

function matchesFilter(entry) {
  if (state.filter === 'watch') {
    if (!state.watch.has(entry.c.covenant_id)) return false;
  } else if (state.filter === 'organic') {
    if (generatorLabel(entry.c, state.network)) return false;
  } else if (state.filter !== 'all' && entry.c.status !== state.filter) {
    return false;
  }
  if (state.query && !entry.blob.includes(state.query)) return false;
  return true;
}

/* The status-filter chip row is static in index.html; the 'organic' chip
   joins it only on networks with a known generator, and pressed states
   follow state.filter (whose default is per-network, so the markup's
   hardcoded aria-pressed can't be trusted). */
function syncFilterChips(network) {
  const group = document.querySelector('#section-coins .chips[role="group"]');
  if (!group) return;
  let organic = group.querySelector('[data-filter="organic"]');
  if (network === 'testnet-10' && !organic) {
    organic = document.createElement('button');
    organic.type = 'button';
    organic.className = 'chip';
    organic.dataset.action = 'filter';
    organic.dataset.filter = 'organic';
    organic.textContent = 'organic';
    organic.title = 'hide the TN10 traffic generator’s coins — bulk plain-p2pk states minted by one known key';
    const all = group.querySelector('[data-filter="all"]');
    if (all) all.after(organic);
    else group.prepend(organic);
  } else if (network !== 'testnet-10' && organic) {
    organic.remove();
    if (state.filter === 'organic') networkFilterReset();
  }
  group.querySelectorAll('[data-action="filter"]').forEach((c) => {
    c.setAttribute('aria-pressed', String(c.dataset.filter === state.filter));
  });
}

const SORTS = {
  activity: (a, b) => (b.c.last_activity_daa || 0) - (a.c.last_activity_daa || 0),
  newest: (a, b) => b.bornMs - a.bornMs,
  oldest: (a, b) => a.bornMs - b.bornMs,
  richest: (a, b) => b.c.live_value - a.c.live_value,
  moves: (a, b) => b.moves - a.moves,
};

/* one grid card — shared by the full render and the append-only path.
   The semantic template ("what is this?") is the primary badge, first after
   the name; the animal name stays the identity. */
function coinCardHtml(e, network) {
  const alive = isAlive(e.c);
  const watched = state.watch.has(e.c.covenant_id);
  const namedTpl = semanticTemplate(e.c.template);
  const genLabel = generatorLabel(e.c, network);
  return `<article class="card">` +
    `<div class="card-head">${avatarSvg(e.c.covenant_id, 40)}` +
    `<div class="card-id"><a class="card-link" href="#/${esc(network)}/c/${esc(e.c.covenant_id)}">${esc(e.name)}</a>` +
    (namedTpl ? `<span class="flag flag-tpl flag-tpl-primary" title="recognized as ${esc(namedTpl)} — decoded from the chain’s own bytes; details on the coin page">${esc(namedTpl)}</span>` : '') +
    `<span class="pill ${alive ? 'pill-alive' : 'pill-retired'}" title="${esc(alive ? GLOSSARY.alive : GLOSSARY.retired)}">${alive ? 'alive' : 'retired'}</span>` +
    (genLabel ? `<span class="flag flag-off flag-gen" title="bulk testnet traffic from the known generator key — not an app">${esc(genLabel)}</span>` : '') +
    lineageBadge(e.c) +
    `</div>` +
    `<button type="button" class="star${watched ? ' starred' : ''}" data-action="watch" data-id="${esc(e.c.covenant_id)}"` +
    ` aria-pressed="${watched}" aria-label="${watched ? 'stop watching' : 'watch'} ${esc(e.name)}">★</button></div>` +
    `<p class="card-story">${esc(cardStory(e, network))}</p>` +
    `</article>`;
}

/* renderGrid memo: the filtered/sorted list is recomputed only when its
   inputs change (filter/query/sort/index generation/watchlist), and "show
   more" appends just the newly revealed cards instead of rebuilding every
   card on screen. watchGen bumps on any ★ toggle so watch state stays live. */
let watchGen = 0;
const gridMemo = { key: '', list: null, renderedKey: '', shown: 0 };

function renderGrid(entry, network) {
  const memoKey = [
    network, state.filter, state.query, state.sort,
    entry.index.generation, watchGen, entry.index.covs.length,
  ].join('');
  let list;
  if (gridMemo.key === memoKey && gridMemo.list) {
    list = gridMemo.list;
  } else {
    /* index.covs is already activity-sorted — the default sort is a no-op,
       and filter() preserves order, so only non-default sorts pay for one */
    list = entry.index.covs.filter(matchesFilter);
    if (state.sort !== 'activity' && SORTS[state.sort]) list = list.sort(SORTS[state.sort]);
    gridMemo.key = memoKey;
    gridMemo.list = list;
  }
  const loaded = entry.index.covs.length;
  /* the headline count is the whole network (from stats) when paginating; the
     filtered denominator stays the rows actually searched so "of N" is honest */
  const total = (entry.data.stats && entry.data.stats.covenants != null)
    ? entry.data.stats.covenants : loaded;
  /* the organic view says exactly what it hid — nothing feels swept away */
  $('#result-count').textContent = (state.filter === 'organic' && !state.query)
    ? `${list.length} organic · ${loaded - list.length} generator coin${loaded - list.length === 1 ? '' : 's'} hidden · ${total} total`
    : (state.query || state.filter !== 'all')
      ? `${list.length} of ${loaded} smart coin${loaded === 1 ? '' : 's'}`
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
      /* the grid now loads only the newest window, so a text search can miss an
         OLDER coin that simply hasn't been fetched yet. When the chain still has
         older pages (nextAfterDaa set), say so plainly and offer to search on
         rather than dead-ending on a misleading "no match" */
      const moreOnChain = Boolean(state.query) && entry.nextAfterDaa != null;
      /* ask the whole chain automatically (worker search endpoint) — matches
         the grid never loaded render as cards below the miss message */
      const remoteHtml = remoteGridCardsHtml(entry, network);
      grid.innerHTML = `<div class="no-results"><p>No smart coins match${state.query ? ` <strong>“${esc(state.query)}”</strong>` : ' that filter'}` +
        `${moreOnChain ? ` in the ${loaded} loaded coin${loaded === 1 ? '' : 's'}` : ''}.</p>` +
        (moreOnChain
          ? `<p class="dim">older coins from the chain aren’t loaded yet — search further back to keep looking.</p></div>`
          : `<p class="dim">Try a friendly name like <em>${esc(example)}</em>, or paste a coin’s id or a transaction id.</p></div>`) +
        remoteHtml;
      if (moreOnChain) {
        /* reuse the grid's load-more path: after each page loadMoreGrid rebuilds
           the index and re-renders against the live query, so a newly fetched
           match surfaces on its own — bounded by the user's clicks, never a loop */
        foot.innerHTML = entry.loadingMore
          ? `<button type="button" class="btn" disabled>searching older coins…</button>`
          : `<button type="button" class="btn" data-action="more-net">search older coins <span class="dim">from the chain</span></button>`;
        return;
      }
    }
    foot.innerHTML = '';
    return;
  }
  const shown = Math.min(state.shown, list.length);
  if (gridMemo.renderedKey === memoKey && gridMemo.shown > 0 && gridMemo.shown <= shown && grid.children.length === gridMemo.shown) {
    /* same list, only the window grew — append the newly revealed cards */
    if (shown > gridMemo.shown) {
      grid.insertAdjacentHTML('beforeend',
        list.slice(gridMemo.shown, shown).map((e) => coinCardHtml(e, network)).join(''));
    }
  } else {
    grid.innerHTML = list.slice(0, shown).map((e) => coinCardHtml(e, network)).join('');
  }
  gridMemo.renderedKey = memoKey;
  gridMemo.shown = shown;
  if (entry.loadingMore) {
    foot.innerHTML = `<button type="button" class="btn" disabled>loading more…</button>`;
  } else if (list.length > state.shown) {
    /* reveal already-loaded rows first — cheap, no round-trip */
    foot.innerHTML = `<button type="button" class="btn" data-action="more">show more <span class="dim">(${list.length - state.shown} hidden)</span></button>`;
  } else if (entry.nextAfterDaa != null) {
    /* everything loaded is on screen but the chain has older coins to fetch */
    foot.innerHTML = `<button type="button" class="btn" data-action="more-net">load more <span class="dim">from the chain</span></button>`;
  } else {
    foot.innerHTML = '';
  }
}

/* --------------------------------------------------- search suggestions */

const suggest = { items: [], active: -1 };

/* Server-side search (worker endpoint from M8). The grid only holds the
   newest window, so a query can miss coins that live further back — the
   endpoint sees the whole chain. Debounced and abortable so typing never
   queues requests; a 404 means an older worker without the endpoint and
   turns the feature off for the rest of the session. */
const remoteSearch = { supported: null, cache: new Map(), timer: 0, ctrl: null };

function remoteSearchRows(network, q) {
  return remoteSearch.cache.get(`${network}|${q}`) || null;
}

function scheduleRemoteSearch(network, q) {
  if (remoteSearch.supported === false) return;
  if (typeof AbortController === 'undefined') return;
  if (remoteSearch.cache.has(`${network}|${q}`)) return;
  clearTimeout(remoteSearch.timer);
  remoteSearch.timer = setTimeout(() => {
    if (remoteSearch.ctrl) remoteSearch.ctrl.abort();
    const ctrl = new AbortController();
    remoteSearch.ctrl = ctrl;
    fetch(`data/${network}/search?q=${encodeURIComponent(q)}&limit=10`, { signal: ctrl.signal })
      .then((res) => {
        if (res.status === 404) { remoteSearch.supported = false; return null; }
        if (!res.ok) return null;
        return res.json();
      })
      .then((data) => {
        if (!data || !Array.isArray(data.results)) return;
        remoteSearch.supported = true;
        if (remoteSearch.cache.size > 300) remoteSearch.cache.clear(); /* typo-bounded */
        remoteSearch.cache.set(`${network}|${q}`, data.results);
        onRemoteSearchResults(network, q);
      })
      .catch(() => { /* aborted mid-typing or offline — the next keystroke retries */ });
  }, 250);
}

/* results landed after the debounce — repaint whichever consumer still shows
   this query (the type-ahead and/or the grid's zero-results rescue) */
function onRemoteSearchResults(network, q) {
  if (state.network !== network || state.query !== q) return;
  const input = $('#search');
  if (input && document.activeElement === input) renderSuggest();
  const entry = state.cache[network];
  if (entry && parseRoute().view === 'explore') renderGrid(entry, network);
}

/* remote rows the paginated grid never loaded, as lightweight cards — their
   coin pages render via the off-grid detail path. Returns '' while the
   endpoint hasn't answered (and quietly schedules the ask). */
function remoteGridCardsHtml(entry, network) {
  const q = state.query;
  if (!q || q.length < 3 || /^[0-9a-f]{64}$/.test(q)) return '';
  if (remoteSearch.supported === false) return '';
  const rows = remoteSearchRows(network, q);
  if (!rows) { scheduleRemoteSearch(network, q); return ''; }
  const fresh = rows.filter((r) => r && typeof r.id === 'string' && !entry.index.byId.has(r.id));
  if (!fresh.length) return '';
  return `<p class="remote-found dim">found on the chain — older than the loaded window:</p>` +
    fresh.map((r) => {
      const alive = isAlive(r);
      const name = r.name || friendlyName(r.id);
      const tpl = semanticTemplate(r.template);
      return `<article class="card">` +
        `<div class="card-head">${avatarSvg(r.id, 40)}` +
        `<div class="card-id"><a class="card-link" href="#/${esc(network)}/c/${esc(r.id)}">${esc(name)}</a>` +
        (tpl ? `<span class="flag flag-tpl flag-tpl-primary">${esc(tpl)}</span>` : '') +
        `<span class="pill ${alive ? 'pill-alive' : 'pill-retired'}" title="${esc(alive ? GLOSSARY.alive : GLOSSARY.retired)}">${alive ? 'alive' : 'retired'}</span>` +
        `</div></div>` +
        `<p class="card-story">lives further back on the chain — tap to read its story.</p>` +
        `</article>`;
    }).join('');
}

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
  /* thin local pickings on a real query — ask the chain too and merge
     whatever the endpoint already answered (dedupe by id) */
  const rq = state.query;
  if (rq.length >= 3 && suggest.items.length < 3 && !/^[0-9a-f]{64}$/.test(rq) &&
      remoteSearch.supported !== false) {
    const rows = remoteSearchRows(state.network, rq);
    if (!rows) {
      scheduleRemoteSearch(state.network, rq);
    } else {
      const entry = state.cache[state.network];
      const seen = new Set(suggest.items.map((s) => s.e.c.covenant_id));
      for (const r of rows) {
        if (suggest.items.length >= 8) break;
        if (!r || typeof r.id !== 'string' || seen.has(r.id)) continue;
        seen.add(r.id);
        /* a row the grid DID load renders from its richer local record */
        const local = entry && entry.index.byId.get(r.id);
        suggest.items.push({
          e: local || { c: { covenant_id: r.id, status: r.status, template: r.template }, name: r.name || friendlyName(r.id) },
          why: 'remote', tx: null, score: 9,
        });
      }
    }
  }
  suggest.active = -1;
  if (!suggest.items.length) { closeSuggest(); return; }
  host.innerHTML = suggest.items.map((s, i) => {
    const alive = isAlive(s.e.c);
    const tpl = semanticTemplate(s.e.c.template);
    const href = `#/${esc(state.network)}/c/${esc(s.e.c.covenant_id)}` +
      (s.tx ? `?tx=${esc(s.tx)}` : '');
    const kind = s.why === 'name' ? '' :
      s.why === 'remote' ? `<span class="suggest-kind">from the chain</span>` :
      `<span class="suggest-kind">${esc(s.why)} ${esc(shortHex(s.tx || s.e.c.covenant_id, 8, 6))}</span>`;
    return `<a class="suggest-item" id="sugg-${i}" role="option" href="${href}" data-suggest="${i}">` +
      avatarSvg(s.e.c.covenant_id, 26) +
      `<span class="suggest-name">${markMatch(s.e.name, state.query)}</span>` +
      (tpl ? `<span class="flag flag-tpl suggest-tpl">${esc(tpl)}</span>` : '') +
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
  const usdSlot = $('#watch-usd-slot');
  if (usdSlot) usdSlot.innerHTML = usdToggleHtml(); /* stale cached index.html */
  $('#watch-row').innerHTML = watched.map((e) => {
    const alive = isAlive(e.c);
    const sub = alive
      ? `holds ${amountWithUsd(e.c.live_value, network)} · active ${relTimeShort(e.lastMs)}`
      : `retired ${relTimeShort(e.lastMs)}`;
    return miniCard(e, network, null, sub);
  }).join('');
}

function renderRecords(entry, network) {
  const covs = entry.index.covs;
  const alive = covs.filter((e) => isAlive(e.c));
  const retired = covs.filter((e) => !isAlive(e.c));
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

/* "what's running here" — bar length ∝ coins, the same number the row's
   text leads with; a template that only ever showed up in spend-time
   reveals keeps a zero bar and renders runs-only ("ran N× at spend"
   carries the truth) */
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
  const max = Math.max(1, ...rows.map((r) => Number(r.covenants) || 0));
  host.innerHTML = rows.map((r) => {
    const coins = Number(r.covenants) || 0;
    const w = coins > 0 ? Math.max((coins / max) * 100, 2).toFixed(1) : 0;
    const color = r.unrec ? 'var(--faint)' : templateColor(r.label);
    /* a row that only ever existed as spend-time reveals has no state to
       count — show just the honest runs figure, not a row of zeros */
    const bits = (coins === 0 && !(Number(r.ever_seen) > 0) && r.revealed_runs > 0) ? [] : [
      `${fmtInt(coins)} coin${coins === 1 ? '' : 's'}`,
      `${fmtInt(r.live_states)} live`,
      `${fmtInt(r.ever_seen)} ever`,
    ];
    if (r.revealed_runs > 0) bits.push(`ran ${fmtInt(r.revealed_runs)}× at spend`);
    const nameTip = GLOSSARY[r.label] ||
      (r.unrec ? 'state scripts kascov doesn\u2019t recognize as a known shape yet — matching never guesses'
        : `a compiled ${r.label} contract, recognized by its instruction skeleton with constructor arguments labeled`);
    const countsTip = `coins: distinct smart coins whose effective shape this is \u00b7 live: their state pieces unspent right now \u00b7 ever: all their state pieces indexed` +
      (r.revealed_runs > 0 ? ' \u00b7 ran at spend: hidden programs revealed and hash-verified when spent' : '');
    /* KCC-1 draft identity chip: the canonical hash when the family's reveals
       all share one build, the build count when they span several. Absent
       fields (older worker) render nothing. */
    const kcc1Chip = r.kcc1_template_hash
      ? `<span class="kcc1-chip mono" title="canonical template identity per the KCC-1 draft at revision 55b28d8 — all of this family’s reveals share this hash. the draft is still moving; kascov recomputes on meaningful revisions">${esc(String(r.kcc1_template_hash).slice(0, 10))}…</span>`
      : (Number(r.kcc1_template_hashes_count) > 1
        ? `<span class="kcc1-chip mono" title="this family spans ${esc(String(r.kcc1_template_hashes_count))} distinct KCC-1 draft template hashes (per-build identities)">${esc(String(r.kcc1_template_hashes_count))} builds</span>`
        : '');
    return `<div class="tpl-row"><span class="tpl-name" title="${esc(nameTip)}">${esc(r.label)}${kcc1Chip}</span>` +
      `<span class="tpl-track" aria-hidden="true"><span class="tpl-fill" style="width:${w}%;background:${color}"></span></span>` +
      `<span class="tpl-counts dim" title="${esc(countsTip)}">${esc(bits.join(' · '))}</span></div>`;
  }).join('');
}

/* ------------------------------------------------------- analytics board */

/* births vs burns over the whole history — two cumulative lines drawn from the
   same activity buckets the pulse chart reads; the gap between them is the
   living population. Vanilla SVG, CSS-var colors, to match paintActivity. */
function analyticsFlowSvg(data) {
  const width = data.bucket_daa || 1;
  const anchorDaa = data.tip_daa != null ? data.tip_daa
    : (data.buckets.length ? data.buckets[data.buckets.length - 1].daa : null);
  if (anchorDaa == null || !data.buckets.length) return '';
  const last = Math.floor(anchorDaa / width);
  let first = Math.floor((data.window_start_daa != null ? data.window_start_daa
    : data.buckets[0].daa) / width);
  let n = last - first + 1;
  if (n > ACTIVITY_MAX_COLS) { first = last - ACTIVITY_MAX_COLS + 1; n = ACTIVITY_MAX_COLS; }
  if (n < 2) return ''; /* one column can't make a trend line */

  const births = new Array(n).fill(0);
  const burns = new Array(n).fill(0);
  for (const b of data.buckets) {
    const i = Math.floor(b.daa / width) - first;
    if (i < 0 || i >= n) continue;
    births[i] += Number(b.births) || 0;
    burns[i] += Number(b.burns) || 0;
  }
  let cb = 0, cd = 0;
  const cumB = new Array(n), cumD = new Array(n);
  for (let i = 0; i < n; i++) { cb += births[i]; cd += burns[i]; cumB[i] = cb; cumD[i] = cd; }
  const totalB = cb, totalD = cd;
  if (totalB === 0) return '';
  /* The curves are EVENT flows — but a coin with several UTXOs can emit more
     than one burn event, so "coins retired/alive" must come from the grid
     stats, not event arithmetic (event math once claimed "0 alive" while 11k
     coins lived). Fall back to event counts only when stats are absent. */
  const stats = state.cache[state.network] && state.cache[state.network].data.stats;
  const retiredCoins = stats && stats.burned != null ? stats.burned : totalD;
  const alive = stats && stats.active != null ? stats.active : Math.max(0, totalB - totalD);
  const yMax = Math.max(1, totalB, totalD);

  const W = 720, H = 200, padT = 14, padB = 28, padX = 10;
  const innerW = W - padX * 2, innerH = H - padT - padB;
  const X = (i) => padX + (i / (n - 1)) * innerW;
  const Y = (v) => H - padB - (v / yMax) * innerH;
  const ptsB = cumB.map((v, i) => `${X(i).toFixed(1)},${Y(v).toFixed(1)}`).join(' ');
  const ptsD = cumD.map((v, i) => `${X(i).toFixed(1)},${Y(v).toFixed(1)}`).join(' ');
  /* alive area = between the two lines: born line forward, burn line back */
  const areaPts = ptsB + ' ' + cumD.map((v, i) => `${X(n - 1 - i).toFixed(1)},${Y(cumD[n - 1 - i]).toFixed(1)}`).join(' ');

  const spanMs = n * width * MS_PER_DAA;
  const withDate = spanMs > 20 * 3600 * 1000 || (Date.now() - activityMs(data, first * width)) > 20 * 3600 * 1000;
  const tickIdx = [...new Set([0, (n - 1) >> 1, n - 1])];
  const ticks = tickIdx.map((i) => {
    const anchor = i === 0 ? 'start' : i === n - 1 ? 'end' : 'middle';
    return `<text x="${X(i).toFixed(1)}" y="${H - 8}" text-anchor="${anchor}" class="pulse-tick">` +
      `${esc(fmtClock(activityMs(data, (first + i) * width), false, withDate))}</text>`;
  }).join('');

  /* the shaded "still alive" band only makes sense while births lead burns;
     with multi-UTXO coins the burn-event curve can cross above and the
     polygon would invert into visual noise */
  const area = totalD <= totalB ? `<polygon points="${areaPts}" fill="var(--accent)" opacity="0.12"/>` : '';
  return `<div class="an-legend" aria-hidden="true">` +
    `<span><i class="dot dot-born"></i>${fmtInt(totalB)} born</span>` +
    `<span><i class="dot" style="background:var(--accent)"></i>${fmtInt(alive)} alive</span>` +
    `<span><i class="dot dot-burn"></i>${fmtInt(retiredCoins)} retired</span></div>` +
    `<svg viewBox="0 0 ${W} ${H}" class="an-svg" role="img" preserveAspectRatio="xMidYMid meet" ` +
    `aria-label="Cumulative births versus retirement events over ${esc(fmtSpan(spanMs))}: ${fmtInt(totalB)} born, ${fmtInt(retiredCoins)} coins retired, ${fmtInt(alive)} alive">` +
    `<line x1="${padX}" y1="${(H - padB + 0.5).toFixed(1)}" x2="${W - padX}" y2="${(H - padB + 0.5).toFixed(1)}" class="pulse-axis"/>` +
    area +
    `<polyline points="${ptsB}" fill="none" stroke="var(--born)" stroke-width="2" stroke-linejoin="round"/>` +
    `<polyline points="${ptsD}" fill="none" stroke="var(--burn)" stroke-width="2" stroke-linejoin="round"/>` +
    ticks + `</svg>`;
}

/* per-template coin counts — how the population splits by recognized script
   shape. Distinct from the "what's running here" bars (which measure live
   state pieces); here we count distinct smart coins. */
function analyticsTemplatesHtml(data) {
  const rows = (data.templates || [])
    .map((x) => ({ label: x.name, coins: Number(x.covenants) || 0, unrec: false }))
    .filter((r) => r.coins > 0);
  if (data.unrecognized && Number(data.unrecognized.covenants) > 0) {
    rows.push({ label: 'unrecognized scripts', coins: Number(data.unrecognized.covenants), unrec: true });
  }
  if (!rows.length) return '';
  rows.sort((a, b) => b.coins - a.coins);
  const total = rows.reduce((s, r) => s + r.coins, 0);
  const top = rows.slice(0, 10);
  const max = Math.max(1, ...top.map((r) => r.coins));
  return top.map((r) => {
    const w = Math.max((r.coins / max) * 100, 2).toFixed(1);
    const color = r.unrec ? 'var(--faint)' : templateColor(r.label);
    const pct = total ? (r.coins / total) * 100 : 0;
    const share = pct >= 1 ? `${pct.toFixed(0)}%` : '<1%';
    return `<div class="lane-row"><span class="lane-ns">${esc(r.label)}</span>` +
      `<span class="lane-track" aria-hidden="true"><span class="lane-fill" style="width:${w}%;background:${color}"></span></span>` +
      `<span class="lane-counts dim">${fmtInt(r.coins)} coin${r.coins === 1 ? '' : 's'} · ${share}</span></div>`;
  }).join('');
}

/* survival curve — the share of retired coins still alive at each age bucket,
   a monotone descent from 100%. Complements the lifespans histogram (which
   shows where coins die; this shows how many are left). */
function analyticsSurvivalSvg(data) {
  const buckets = (data && data.buckets) || [];
  if (!buckets.length || !data.total) return '';
  const total = data.total;
  const k = buckets.length;
  const surv = [1];
  let dead = 0;
  for (let i = 0; i < k; i++) { dead += Number(buckets[i].count) || 0; surv.push(Math.max(0, (total - dead) / total)); }

  const W = 720, H = 210, padT = 12, padB = 40, padL = 30, padR = 8;
  const innerW = W - padL - padR, innerH = H - padT - padB;
  const X = (i) => padL + (i / k) * innerW;
  const Y = (s) => H - padB - s * innerH;
  const pts = surv.map((s, i) => `${X(i).toFixed(1)},${Y(s).toFixed(1)}`).join(' ');
  const areaPts = `${X(0).toFixed(1)},${(H - padB).toFixed(1)} ${pts} ${X(k).toFixed(1)},${(H - padB).toFixed(1)}`;

  let grid = '';
  for (const g of [0, 0.25, 0.5, 0.75, 1]) {
    const y = Y(g);
    grid += `<line x1="${padL}" y1="${y.toFixed(1)}" x2="${(W - padR).toFixed(1)}" y2="${y.toFixed(1)}" class="pulse-axis" opacity="${g === 0 ? 1 : 0.35}"/>` +
      `<text x="${(padL - 6).toFixed(1)}" y="${(y + 3).toFixed(1)}" text-anchor="end" class="pulse-tick">${Math.round(g * 100)}%</text>`;
  }
  /* show every bucket label if few, else thin to ~6 to stay legible */
  const step = k <= 8 ? 1 : Math.ceil(k / 6);
  let xlab = '';
  for (let i = 0; i < k; i++) {
    if (i % step !== 0 && i !== k - 1) continue;
    xlab += `<text x="${X(i + 0.5).toFixed(1)}" y="${(H - 8).toFixed(1)}" text-anchor="middle" class="pulse-tick">${esc(buckets[i].label)}</text>`;
  }

  const medMin = data.median_ms / 60000;
  const medLabel = medMin >= 1 ? `${medMin.toFixed(1)} min` : `${Math.round(data.median_ms / 1000)} s`;
  return `<p class="an-note dim">median lifespan <strong>${esc(medLabel)}</strong> · ${fmtInt(total)} retired coin${total === 1 ? '' : 's'}</p>` +
    `<svg viewBox="0 0 ${W} ${H}" class="an-svg" role="img" preserveAspectRatio="xMidYMid meet" ` +
    `aria-label="Survival curve: share of retired coins still alive at each age, median ${esc(medLabel)}">` +
    grid +
    `<polygon points="${areaPts}" fill="var(--accent)" opacity="0.12"/>` +
    `<polyline points="${pts}" fill="none" stroke="var(--accent)" stroke-width="2" stroke-linejoin="round"/>` +
    xlab + `</svg>`;
}

/* recent reorgs (optional) — the indexer's virtual-chain rollback ledger.
   Tiny sparkline of rollback depths (oldest→newest) plus a one-line summary.
   Schema (reorgs.json): { reorgs: [{ daa, at_ms, rolled_back }], … } newest first. */
function analyticsReorgsHtml(data) {
  const list = (data && data.reorgs) || [];
  if (!list.length) return '';
  const totalBlocks = list.reduce((s, r) => s + (Number(r.rolled_back) || 0), 0);
  const latest = list[0];
  const latestMs = Number(latest && latest.at_ms) || 0;
  const depths = list.slice(0, 60).reverse().map((r) => Math.max(0, Number(r.rolled_back) || 0));
  const max = Math.max(1, ...depths);
  const W = 280, H = 34, gap = 2;
  const slot = W / depths.length;
  const bw = Math.max(1, slot - gap);
  const bars = depths.map((d, i) => {
    const h = d > 0 ? Math.max((d / max) * (H - 4), 2) : 1;
    const x = i * slot + (slot - bw) / 2;
    return `<rect x="${x.toFixed(1)}" y="${(H - h).toFixed(1)}" width="${bw.toFixed(1)}" height="${h.toFixed(1)}" rx="1" fill="var(--move)"/>`;
  }).join('');
  /* headline the last hour, not the scary all-time total — most entries are
     routine 1-2 block DAG tip-flips */
  const hourAgo = Date.now() - 3600_000;
  const lastHour = list.filter((r) => Number(r.at_ms) > hourAgo).length;
  const summary = `<strong>${fmtInt(lastHour)} in the last hour</strong> · ` +
    `${fmtInt(list.length)} on record · ` +
    `${fmtInt(totalBlocks)} chain block${totalBlocks === 1 ? '' : 's'} rolled back` +
    (latestMs ? ` · latest ${esc(relTime(latestMs))}` : '');
  return `<p class="an-note dim">chain reorganizations the indexer rolled back through — ` +
    `small tip-flips are normal on a blockDAG.</p>` +
    `<p class="an-note">${summary}</p>` +
    `<svg viewBox="0 0 ${W} ${H}" class="an-spark" role="img" preserveAspectRatio="none" ` +
    `aria-label="Rollback depth of the ${esc(fmtInt(depths.length))} most recent reorgs, oldest to newest">${bars}</svg>`;
}

/* the analytics board — a collapsible dashboard of charts drawn from the
   small feeds the explore page already serves. Each card degrades on its own:
   a missing/404 feed hides just that card, and the whole board hides only when
   every card is empty. */
function renderAnalytics(network) {
  const section = $('#section-analytics');
  if (!section) return;
  const cards = {
    flow: { card: $('#acard-flow'), host: $('#analytics-flow') },
    tpl: { card: $('#acard-templates'), host: $('#analytics-templates') },
    surv: { card: $('#acard-survival'), host: $('#analytics-survival') },
    reorg: { card: $('#acard-reorgs'), host: $('#analytics-reorgs') },
  };
  let visible = 0;
  const reRender = () => {
    if (state.network === network && parseRoute().view === 'explore') renderAnalytics(network);
  };
  const show = (key, html) => {
    const c = cards[key];
    if (!c || !c.card || !c.host) return;
    if (html) { c.host.innerHTML = html; c.card.hidden = false; visible++; }
    else { c.card.hidden = true; }
  };
  const loading = (key, note) => {
    const c = cards[key];
    if (!c || !c.card || !c.host) return;
    c.host.innerHTML = `<p class="dim"><span class="radar" aria-hidden="true"></span>${esc(note)}</p>`;
    c.card.hidden = false;
    visible++;
  };

  /* births vs burns — reads its own 'all' activity window (separate from the
     pulse chart's range so neither disturbs the other) */
  const actHit = (state.activity[network] || {})['all'];
  if (!actHit) { loading('flow', 'reading births & burns…'); loadActivity(network, 'all').then(reRender); }
  else if (actHit.data) { show('flow', analyticsFlowSvg(actHit.data)); }
  else { show('flow', ''); }

  const tplHit = state.templates[network];
  if (!tplHit) { loading('tpl', 'reading contract types…'); loadTemplates(network).then(reRender); }
  else if (tplHit.data) { show('tpl', analyticsTemplatesHtml(tplHit.data)); }
  else { show('tpl', ''); }

  const lifeHit = state.lifespans[network];
  if (!lifeHit) { loading('surv', 'measuring lifespans…'); loadLifespans(network).then(reRender); }
  else if (lifeHit.data) { show('surv', analyticsSurvivalSvg(lifeHit.data)); }
  else { show('surv', ''); }

  /* reorgs are optional — stay hidden until a feed is known (no loading note,
     so an older worker without the endpoint never shows an empty card) */
  const reHit = state.reorgs[network];
  if (!reHit) { loadReorgs(network).then(reRender); show('reorg', ''); }
  else if (reHit.data) { show('reorg', analyticsReorgsHtml(reHit.data)); }
  else { show('reorg', ''); }

  section.hidden = visible === 0;
  if (visible > 0) promoteSection(section, 'kascov-analytics-seen');
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

  /* a <video autoplay> inside a view that starts hidden won't begin on its own
     when the view is later shown — so nudge it (muted → autoplay is allowed).
     This is why it only played after a refresh before. */
  const tv = document.getElementById('tour-video');
  if (tv) {
    tv.muted = true;
    const p = tv.play();
    if (p && p.catch) p.catch(() => {});
  }

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
  renderCommunity();
  renderWhatsNew();
  updateTourOffer();

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
    const anb = $('#section-analytics');
    if (anb) anb.hidden = true;
    $('#empty-net').innerHTML = emptyCardHtml(network);
    return;
  }

  $('#pulse-title').textContent = net.pulseTitle;
  renderWatchStrip(entry, network);
  renderPulse(entry, network);
  renderRecords(entry, network);
  renderTemplates(network);
  renderFamilies(network);
  renderPending(network);
  renderLanes(network);
  renderInscriptions(network);
  renderLifespans(network);
  renderAnalytics(network);
  loadTemplates(network).then(() => {
    if (state.network === network && parseRoute().view === 'explore') renderTemplates(network);
  });
  renderStories(entry, network);
  const sortSel = $('#sort');
  if (sortSel && sortSel.value !== state.sort) sortSel.value = state.sort;
  syncFilterChips(network);
  renderGrid(entry, network);
}

/* ----------------------------------------------------------------- detail */

function timelineItem(entry, ev, data, network, flashTx) {
  const meta = KIND_META[ev.kind] || KIND_META.transition;
  const ms = daaToMs(ev.accepting_daa, data);
  let nerdBits = state.nerd
    ? ` · <span class="mono dim">DAA ${esc(fmtInt(ev.accepting_daa))}</span>`
    : '';
  /* KIP-21 user lane: 4-byte app namespace + 16 zero bytes */
  if (ev.payload && ev.payload.length >= 40 && /^0{32}$/.test(ev.payload.slice(8, 40))) {
    nerdBits += ` · <span class="flag flag-tpl" title="KIP-21 based-app lane — this transaction carries app-sequencing data">lane 0x${esc(ev.payload.slice(0, 8))}</span>`;
  }
  /* tx payload peek: printable-ASCII if we can, else a short hex sample */
  let payloadBits = '';
  const pk = payloadPeek(ev.payload, ev.payload_len);
  if (pk) {
    payloadBits = `<p class="tl-payload"><span class="tl-payload-tag">payload</span> ` +
      `<span class="tl-payload-val${pk.mono ? ' mono' : ''}"` +
      (pk.title ? ` title="${esc(pk.title)}"` : '') + `>${esc(pk.label)}</span>` +
      (pk.bytes != null ? ` <span class="dim">${esc(fmtInt(pk.bytes))}B</span>` : '') + `</p>`;
  }
  /* KRC-20-style JSON inscription in the payload → a compact labelled chip */
  let inscBits = '';
  if (window.kascovPayload) {
    const insc = window.kascovPayload.decodeInscription(ev.payload);
    if (insc) {
      inscBits = `<p class="tl-insc"><span class="insc-chip" title="${esc(window.kascovPayload.chipTitle(insc))}">` +
        `${esc(window.kascovPayload.chipLabel(insc))}</span></p>`;
    }
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
    `<p class="tl-meta"><span title="${esc(utcTitle(ms))}">${esc(relTime(ms))}</span> · <span class="abs-t" title="${esc(utcTitle(ms))}">${esc(absShort(ms))}</span> · <a href="#/${esc(network)}/tx/${esc(ev.txid)}">view transaction →</a>${nerdBits}</p>` +
    inscBits +
    payloadBits +
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

/* ---- coin-page contract intelligence: read the program a coin actually
   committed to / revealed at spend, so the coin detail page can surface the
   same plain-English explainer and on-chain ZK verification the decode page
   gives — without making the reader leave the coin or open nerd mode. ---- */

/* find a readable program for this coin and decode it once. Prefers a UTXO
   with a recognized (non-p2pk/p2sh) template, then any revealed program (the
   bytes that actually ran), then any live committed script. */
function coinProgram(c) {
  const D = window.kascovDisasm;
  if (!D || !D.parseHex) return null;
  const utxos = (c && c.utxos) || [];
  let hex = '';
  for (const u of utxos) {
    const t = u.revealed_template || u.template;
    if (t && !/^p2(pk|sh)/.test(t)) { hex = u.revealed_hex || u.script_hex || ''; if (hex) break; }
  }
  if (!hex) for (const u of utxos) { if (u.revealed_hex) { hex = u.revealed_hex; break; } }
  if (!hex) for (const u of utxos) { if (u.script_hex) { hex = u.script_hex; break; } }
  if (!hex) return null;
  const bytes = D.parseHex(hex);
  if (!bytes) return null;
  const { instructions, truncated } = D.disassemble(bytes);
  const tpl = !truncated && D.matchTemplates ? D.matchTemplates(instructions, bytes) : null;
  return { hex, bytes, instructions, tpl };
}

/* optional exported ZK-system label ("Groth16" / "RISC Zero" / "ZK"), from the
   coin or any of its UTXOs — feature-detected; absent on older exports. */
function coinZkSystem(c) {
  if (!c) return '';
  if (c.zk_system) return String(c.zk_system);
  for (const u of (c.utxos || [])) if (u.zk_system) return String(u.zk_system);
  return '';
}

/* the prominent "what this contract does" block for the top of a coin page:
   the plain-English explainer for a recognized template, plus a ZK panel (with
   the zk_system label + live "verify the proof" button) wherever the program
   does on-chain zero-knowledge verification. */
function coinContractSectionHtml(c) {
  const prog = coinProgram(c);
  if (!prog) return '';
  const parts = [];
  if (prog.tpl) parts.push(explainerPanelHtml(prog.tpl));
  parts.push(zkPanelHtml(prog.instructions, prog.hex, coinZkSystem(c)));
  const body = parts.filter(Boolean).join('');
  if (!body) return '';
  return `<section class="contract-intel" aria-label="What this contract does">${body}</section>`;
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

/* the Write-mode template picker: each canonical contract + a set of valid
   default constructor args (source comes from gen.js's SOURCES at click time). */
const SILVERSCRIPT_EXAMPLES = {
  'SilverScript · Escrow': { label: 'Escrow', args: [`0x${'33'.repeat(32)}`, `0x${'11'.repeat(32)}`, `0x${'22'.repeat(32)}`] },
  'SilverScript · Mecenas': { label: 'Mecenas', args: [`0x${'44'.repeat(32)}`, `0x${'55'.repeat(32)}`, '100000000', '86400'] },
  'SilverScript · LastWill': { label: 'LastWill', args: [`0x${'66'.repeat(32)}`, `0x${'77'.repeat(32)}`, `0x${'88'.repeat(32)}`] },
};

function loadTemplate(name) {
  const src = $('#compiler-src');
  const args = $('#compiler-args');
  const out = $('#compiler-result');
  const ex = SILVERSCRIPT_EXAMPLES[name];
  const source = window.kascovGen && window.kascovGen.SOURCES ? window.kascovGen.SOURCES[name] : '';
  if (src) src.value = name === 'blank' || !source ? '' : source;
  if (args) args.value = name === 'blank' || !ex ? '' : ex.args.join('\n');
  if (out) out.innerHTML = '';
  const pub = $('#publish-result');
  if (pub) pub.innerHTML = '';
}

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

/* engine trace steps ({op, dstack, astack}) → the debugger's step shape */
function concreteSteps(trace) {
  const short = (h) => (h && h.length > 18 ? h.slice(0, 10) + '…' + h.slice(-4) : h);
  return trace.map((s, i) => ({
    offset: i,
    name: s.op.split(' ')[0],
    note: s.op.includes(' ') ? s.op.slice(s.op.indexOf(' ') + 1) : '',
    group: 'standard',
    indent: 0,
    dstack: (s.dstack || []).map(short),
    astack: (s.astack || []).map(short),
  }));
}

/* open the debugger on a CONCRETE engine trace (from a simulate run) — real
   stacks, real control flow, vs the symbolic decode-page trace */
function openSimTrace(trace) {
  if (!trace || !trace.length) return;
  dbgHost = null; /* render into the page's #dbg-panel */
  dbg = { concrete: true, i: 0, steps: concreteSteps(trace) };
  renderDebugger();
  const host = document.getElementById('dbg-panel');
  if (host) host.scrollIntoView({ block: 'start', behavior: 'smooth' });
}

/* open the debugger on a REAL on-chain spend replayed by the worker
   (/debug/{txid}) — rendered into the utxo row's own panel on the coin page */
function openRealTrace(d, host) {
  if (!d || !d.trace || !d.trace.length || !host) return;
  dbgHost = host;
  dbg = {
    concrete: true,
    real: true,
    pass: d.pass,
    verdict: d.verdict || '',
    truncated: Boolean(d.trace_truncated),
    i: 0,
    steps: concreteSteps(d.trace),
  };
  renderDebugger();
  host.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
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

function zkPanelHtml(instructions, hex, systemOverride) {
  const z = zkInfo(instructions);
  if (!z) return '';
  // an optional exported "zk_system" label (Groth16 / RISC Zero / ZK) wins over
  // the tag we sniff from the pushes — but we always fall back to the sniffed one.
  // NB: both are heuristics (push-size shape), never asserted by consensus, so a
  // specific system name is hedged as inferred; only the ZK-ness itself is certain.
  const system = systemOverride || z.system;
  const inferred = /groth|risc|plonk|stark|halo|nova/i.test(system);
  const sysChip = inferred
    ? `${esc(system)} <span class="zk-infer" title="proving system inferred from the script's push shape — not asserted by consensus">inferred</span>`
    : esc(system);
  const verb = inferred ? 'appears to verify' : 'verifies';
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
    `<span class="zk-sys">${sysChip}</span></div>` +
    `<p class="zk-desc">${selfContained
      ? `A self-contained <code class="mono">${esc(system)}</code>${inferred ? '-shaped' : ''} proof (public inputs + proof + verifying key + <code class="mono">OpZkPrecompile</code>). Verify it below — kascov runs the <em>exact</em> verifier Kaspa's L1 uses.`
      : `This contract calls <code class="mono">OpZkPrecompile</code> (KIP-16) — Kaspa's L1 ${verb} a ${esc(system)} proof <em>inside the script</em>, so the coin only moves if a valid zero-knowledge proof is supplied. Verified computation, settled on a ~10-blocks/sec BlockDAG.`}</p>` +
    verify +
    `</div>`;
}

/* ---- the visual script debugger (symbolic stepper) ---- */
let dbg = null; // { steps, i, concrete?, real?, pass?, verdict?, truncated? }
/* where the debugger draws: an explicit element (real-spend replay renders
   inside its utxo row) or, when null, the page's #dbg-panel */
let dbgHost = null;

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
  dbgHost = null; /* render into the page's #dbg-panel */
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
  const host = dbgHost && dbgHost.isConnected ? dbgHost : document.getElementById('dbg-panel');
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
  const realHead = dbg.real
    ? `<div class="dbg-real"><span class="dbg-real-badge">⛓ real spend replay</span>` +
      (dbg.pass === true ? ' <span class="flag flag-yes">engine: pass</span>'
        : dbg.pass === false ? ' <span class="flag flag-no">engine: fail</span>' : '') +
      (dbg.verdict ? ` <span class="dim">${esc(dbg.verdict)}</span>` : '') + `</div>`
    : '';
  host.innerHTML = `<div class="dbg">` + realHead +
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
    (dbg.real
      ? 'the real on-chain spend, opcode by opcode — the actual engine replaying the captured unlocking script against this state&rsquo;s program' +
        (dbg.truncated ? ' (long run — the trace is truncated)' : '') +
        '; the transaction context is reconstructed, so signature &amp; introspection checks can diverge from the original run'
      : dbg.concrete
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

/* an honest, approximate cost for a spend from its compute budget.
   spent_budget is in compute units (1 unit = 10000 script units); Kaspa's
   storage-mass fee floor is 100 sompi × max(compute grams, 2 × tx bytes).
   we don't know the spending tx's byte size here, so we can only show the
   compute-bound estimate — a floor that a tiny tx would actually pay. */
const SCRIPT_UNITS_PER_BUDGET = 10000;
const SOMPI_PER_COMPUTE_GRAM = 100;
function spentBudgetHtml(budget, network) {
  const grams = budget * SCRIPT_UNITS_PER_BUDGET;
  const sompi = grams * SOMPI_PER_COMPUTE_GRAM;
  return `<span class="dim spent-budget" title="fee floor is 100 sompi × max(compute grams, 2 × tx bytes) — this is the compute-bound estimate and ignores transaction size, so the real fee is at least this much">` +
    `this spend budgeted ${esc(fmtInt(budget))} unit${budget === 1 ? '' : 's'} ` +
    `(≈ ${esc(fmtInt(grams))} script units, ~${esc(amountWithUsd(sompi, network))} if compute-bound)</span>`;
}

/* "what changed between states" (M8): consecutive revealed_fields diffs,
   ordered by created_daa. Pure data-in → data-out so it can be poked from
   the console with synthetic input (window.kascovStateDiffs). */
function stateDiffs(utxos) {
  const reveals = (utxos || [])
    .filter((u) => u && Array.isArray(u.revealed_fields) && u.revealed_fields.length)
    .sort((a, b) => (a.created_daa || 0) - (b.created_daa || 0) ||
      String(a.outpoint || '').localeCompare(String(b.outpoint || '')));
  const diffs = [];
  for (let i = 1; i < reveals.length; i++) {
    const prev = new Map(reveals[i - 1].revealed_fields.map((f) => [f.name, f.value]));
    const next = new Map(reveals[i].revealed_fields.map((f) => [f.name, f.value]));
    const changed = [];
    const added = [];
    const removed = [];
    for (const [name, value] of next) {
      if (!prev.has(name)) added.push({ name, value });
      else if (prev.get(name) !== value) changed.push({ name, from: prev.get(name), to: value });
    }
    for (const [name, value] of prev) {
      if (!next.has(name)) removed.push({ name, value });
    }
    diffs.push({
      fromDaa: reveals[i - 1].created_daa,
      toDaa: reveals[i].created_daa,
      changed, added, removed,
      same: !changed.length && !added.length && !removed.length,
    });
  }
  return diffs;
}
window.kascovStateDiffs = stateDiffs; /* console-testable */

function stateDiffHtml(utxos) {
  const diffs = stateDiffs(utxos);
  if (!diffs.length) return '';
  const val = (v) => `<span class="mono" title="${esc(v)}">${esc(shortHex(String(v), 10, 6))}</span>`;
  const steps = diffs.map((d) => {
    const bits = [];
    for (const f of d.changed) {
      bits.push(`<li class="diff-row"><span class="diff-name dim">${esc(f.name)}</span> ` +
        `${val(f.from)} <span class="diff-arrow">→</span> ${val(f.to)}</li>`);
    }
    for (const f of d.added) {
      bits.push(`<li class="diff-row"><span class="diff-name dim">${esc(f.name)}</span> ` +
        `<span class="diff-add">added</span> ${val(f.value)}</li>`);
    }
    for (const f of d.removed) {
      bits.push(`<li class="diff-row"><span class="diff-name dim">${esc(f.name)}</span> ` +
        `<span class="diff-del">removed</span> ${val(f.value)}</li>`);
    }
    return `<div class="diff-step">` +
      `<p class="diff-head dim">state at DAA ${esc(fmtInt(d.fromDaa))} → state at DAA ${esc(fmtInt(d.toDaa))}</p>` +
      (d.same
        ? `<p class="dim diff-same">no field changed — the same revealed state moved forward</p>`
        : `<ul class="diff-list">${bits.join('')}</ul>`) +
      `</div>`;
  }).join('');
  return `<h3 class="nerd-h">what changed between states</h3>` +
    `<p class="dim diff-sub">fields revealed at spend, diffed across consecutive states</p>` +
    steps;
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
    ['live UTXOs', `<span class="mono">${esc(fmtInt(c.live_utxos))}</span> holding <span class="mono">${esc(amountWithUsd(c.live_value, network))}</span>`],
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
        (u.revealed_hex ? zkPanelHtml(window.kascovDisasm.disassemble(window.kascovDisasm.parseHex(u.revealed_hex) || []).instructions, u.revealed_hex, u.revealed_zk_system || u.zk_system || c.zk_system) : '') +
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
    /* spent states can be REPLAYED: the worker reruns the captured unlocking
       script through the real engine and returns a per-opcode trace
       (feature-detected — an older worker just reports it's unavailable) */
    const replay = !u.live && u.spent_txid
      ? `<div class="replay-row"><button type="button" class="btn dbg-btn replay-btn" data-action="replay-spend" data-txid="${esc(u.spent_txid)}">⧉ replay this spend</button>` +
        `<span class="dim replay-hint">the real on-chain spend, opcode by opcode</span>` +
        `<span class="replay-result"></span></div><div class="replay-panel"></div>`
      : '';
    return `<div class="utxo">` +
      `<div class="utxo-head"><span class="mono break">${esc(u.outpoint)}</span><span class="utxo-flags">${badges}</span></div>` +
      `<div class="utxo-meta"><span>${esc(amountWithUsd(u.value, network))}</span><span class="dim">created at DAA ${esc(fmtInt(u.created_daa))}</span>` +
      (u.spent_budget != null ? spentBudgetHtml(u.spent_budget, network) : '') +
      `</div>` +
      templateLine(u.template, u.state_fields) +
      `<pre class="script">${esc((u.script_asm || []).join('\n'))}</pre>` +
      (u.script_hex ? `<a class="decode-open" href="#/decode?s=${esc(u.script_hex)}">open in decoder →</a>` : '') +
      reveal +
      replay +
      revealPreviewHtml(u, program) +
      `</div>`;
  }).join('');
  return `<dl class="nerd-rows">${rows.map(([k, v]) => `<div class="nerd-row"><dt>${esc(k)}</dt><dd>${v}</dd></div>`).join('')}</dl>` +
    stateDiffHtml(allUtxos) +
    `<h3 class="nerd-h">UTXOs (${allUtxos.length})</h3>` +
    (utxos || '<p class="dim">no UTXOs recorded.</p>') +
    utxoFoot;
}

/* canonical crawler-visible share link (worker /share route — OG card +
   redirect); copying it never needs the route to exist locally */
const shareUrl = (network, id) => `https://kascov.io/share/${network}/${id}`;

/* long coins fold: show a window of events/UTXOs with expanders */
const STORY_WINDOW = 8;
const UTXO_WINDOW = 8;

/* live controller for the coin page's "moved together" graph — stopped and
   replaced on every re-render so old animation loops don't pile up */
let detailGraph = null;

function renderDetail(entry, covId, flashTx, program) {
  const network = state.network;
  const { data, index } = entry;
  const view = $('#view-detail');
  const gridRec = index.byId.get(covId);
  if (state.detailId !== covId) {
    state.detailId = covId;
    state.storyAll = false;
    state.utxoAll = false;
    state.tlKind = 'all';   /* per-coin timeline chips reset with the coin */
    state.tlLabel = 'all';
  }

  const detMap = state.details[network];
  const rec = detMap && detMap.get(covId);

  if (!gridRec && !rec) {
    /* Not in the loaded grid window — which no longer means unknown: the grid
       paginates, so galaxy clicks, share links and deep links routinely point
       past it. Ask the per-coin detail endpoint before declaring the coin
       unknown; only a 404 from THERE earns the not-found card. */
    const name = friendlyName(covId);
    document.title = `${name} — kascov`;
    view.innerHTML =
      `<a class="back" href="#/explore">← all smart coins</a>` +
      `<header class="detail-head">` +
      `<span role="img" aria-label="avatar of ${esc(name)}">${avatarSvg(covId, 88)}</span>` +
      `<div class="detail-id"><h1>${esc(name)}</h1>` +
      `<p class="id-chip"><span class="mono">${esc(shortHex(covId, 10, 8))}</span>` +
      `<button type="button" class="copy-btn" data-action="copy" data-copy="${esc(covId)}" aria-label="copy this coin’s full id">copy id</button></p>` +
      `</div></header>` +
      `<section aria-label="Life story"><h2>life story</h2>` +
      `<p class="dim">reading this coin’s full story…</p></section>`;
    loadDetail(network, covId)
      .then(() => {
        if (state.network === network && state.detailId === covId && parseRoute().view === 'detail') {
          renderDetail(entry, covId, flashTx, program);
        }
      })
      .catch((e) => {
        if (state.detailId !== covId) return;
        if (/404/.test(String(e && e.message))) {
          if (freshDeploys.has(covId)) {
            /* a coin THIS tab just deployed — not unknown, just not settled:
               testnet reorg churn can delay indexing by minutes */
            document.title = 'smart coin still settling — kascov';
            view.innerHTML = `<a class="back" href="#/explore">← all smart coins</a>` +
              `<div class="empty-card"><h2>Born — still settling.</h2>` +
              `<p class="dim">this coin was just deployed; the indexer is catching it through testnet reorgs. its page usually appears within a couple of minutes.</p>` +
              `<button type="button" class="btn" data-action="retry-detail">check again</button></div>`;
            return;
          }
          document.title = 'smart coin not found — kascov';
          const other = network === 'mainnet' ? 'testnet-10' : 'mainnet';
          view.innerHTML = `<a class="back" href="#/explore">← all smart coins</a>` +
            `<div class="empty-card"><h2>We haven’t met this smart coin.</h2>` +
            `<p class="dim">kascov hasn’t seen it on ${esc(NETWORKS[network].label)} — it may live on the other network, or the id might be mistyped.</p>` +
            `<button type="button" class="btn" data-action="network" data-network="${other}">look on ${other}</button></div>`;
        } else {
          const story = view.querySelector('section[aria-label="Life story"]');
          if (story) {
            story.innerHTML = `<h2>life story</h2><p class="dim">couldn’t load this coin’s story.</p>` +
              `<button type="button" class="btn" data-action="retry-detail">try again</button>`;
          }
        }
      });
    return;
  }

  /* the life story and scripts come from the per-coin detail endpoint —
     paint the header from the grid row instantly, fill in when it lands */
  if (!rec) {
    const alive0 = isAlive(gridRec.c);
    const watched0 = state.watch.has(covId);
    const namedTpl0 = semanticTemplate(gridRec.c.template);
    document.title = `${gridRec.name} — kascov`;
    const bits = [];
    bits.push(`${gridRec.c.genesis_daa != null ? 'born' : 'first seen'} ${relTime(gridRec.bornMs)}`);
    bits.push(gridRec.moves === 0 ? 'never moved' : gridRec.moves === 1 ? 'moved once' : `moved ${gridRec.moves} times`);
    if (alive0) bits.push(`currently holds ${amountWithUsd(gridRec.c.live_value, network)}`);
    else bits.push(`retired ${relTime(gridRec.lastMs)}`);
    view.innerHTML =
      `<a class="back" href="#/explore">← all smart coins</a>` +
      `<header class="detail-head">` +
      `<span role="img" aria-label="avatar of ${esc(gridRec.name)}">${avatarSvg(covId, 88)}</span>` +
      `<div class="detail-id"><h1>${esc(gridRec.name)}</h1>` +
      `<p class="detail-tags">` +
      (namedTpl0 ? `<span class="flag flag-tpl flag-tpl-primary">${esc(namedTpl0)}</span>` : '') +
      `<span class="pill ${alive0 ? 'pill-alive' : 'pill-retired'}" title="${esc(alive0 ? GLOSSARY.alive : GLOSSARY.retired)}">${alive0 ? 'alive' : 'retired'}</span>` +
      `<button type="button" class="star${watched0 ? ' starred' : ''}" data-action="watch" data-id="${esc(covId)}" aria-pressed="${watched0}" aria-label="watch this coin">★</button>` +
      lineageBadge(gridRec.c) +
      tokenBacklinkHtml(network, covId) +
      `<span class="dim">smart coin on ${esc(NETWORKS[network].label)}</span> ${usdToggleHtml()}</p>` +
      `<p class="id-chip"><span class="mono">${esc(shortHex(covId, 10, 8))}</span>` +
      `<button type="button" class="copy-btn" data-action="copy" data-copy="${esc(covId)}" aria-label="copy this coin’s full id">copy id</button>` +
      `<button type="button" class="copy-btn" data-action="copy" data-copy="${esc(shareUrl(network, covId))}" aria-label="copy a shareable link to this coin">share</button></p>` +
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
  const alive = isAlive(c);
  const watched = state.watch.has(c.covenant_id);
  /* surface recognized contract templates (but not the ubiquitous p2pk/p2sh
     shapes): spend-time reveals first (proven from the bytes), else the
     summary's semantic name (newer workers decode e.g. "KCC20 token" there) */
  const namedTemplate = (c.utxos || [])
    .map((u) => u.revealed_template || u.template)
    .find((t) => semanticTemplate(t)) || semanticTemplate(c.template);
  const genLabel = generatorLabel(c, network);
  document.title = `${rec.name} — kascov`;

  const summaryBits = [];
  summaryBits.push(`${c.genesis_daa != null ? 'born' : 'first seen'} ${relTime(rec.bornMs)} (${absShort(rec.bornMs)})`);
  summaryBits.push(rec.moves === 0 ? 'never moved' : rec.moves === 1 ? 'moved once' : `moved ${rec.moves} times`);
  if (alive) {
    summaryBits.push(`currently holds ${amountWithUsd(c.live_value, network)}${c.live_utxos > 1 ? ` in ${c.live_utxos} pieces` : ''}`);
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

  const events = c.events;

  /* timeline chips: kind row always (once there's something to filter), and
     a second row by spend-revealed program label when the coin's spends
     revealed at least two distinct ones. The "revealed label" of an event is
     carried by the UTXO(s) that event consumed (same join as eventShape). */
  const revealedLabels = (ev) => (c.utxos || [])
    .filter((u) => u.spent_txid === ev.txid && u.revealed_template)
    .map((u) => u.revealed_template);
  const labelSet = [...new Set(events.flatMap(revealedLabels))];
  const kindSel = state.tlKind || 'all';
  const labelSel = labelSet.includes(state.tlLabel) ? state.tlLabel : 'all';
  let tlEvents = filterEventsByKind(events, kindSel);
  if (labelSel !== 'all') tlEvents = tlEvents.filter((ev) => revealedLabels(ev).includes(labelSel));
  const tlChip = (action, value, sel) =>
    `<button type="button" class="chip" data-action="${action}" data-value="${esc(value)}"` +
    ` aria-pressed="${value === sel}">${esc(value)}</button>`;
  const kindChips = events.length > 1
    ? `<div class="chips tl-chips" role="group" aria-label="Filter the timeline by event">` +
      ['all', 'born', 'moved', 'retired'].map((k) => tlChip('tl-kind', k, kindSel)).join('') + `</div>`
    : '';
  const labelChips = labelSet.length >= 2
    ? `<div class="chips tl-chips" role="group" aria-label="Filter the timeline by revealed program">` +
      ['all', ...labelSet].map((l) => tlChip('tl-label', l, labelSel)).join('') + `</div>`
    : '';
  const tlCount = kindSel !== 'all' || labelSel !== 'all'
    ? `<p class="dim tl-count">${esc(fmtInt(tlEvents.length))} of ${esc(fmtInt(events.length))} events shown</p>`
    : '';
  const tlEmpty = !tlEvents.length
    ? `<li class="tl-item tl-note"><span class="tl-icon" aria-hidden="true">${ICONS.move}</span><div class="tl-body">` +
      `<p class="tl-text dim">no ${esc(labelSel !== 'all' ? labelSel : kindSel)} events on this coin</p></div></li>`
    : '';

  /* long life stories fold to a window; a highlighted event beyond the
     fold auto-expands so ?tx= deep links always land */
  if (flashTx && !state.storyAll) {
    const at = tlEvents.findIndex((ev) => ev.txid === flashTx);
    if (at >= STORY_WINDOW - 1) state.storyAll = true;
  }
  const foldStory = tlEvents.length > STORY_WINDOW + 4 && !state.storyAll;
  const shownEvents = foldStory ? tlEvents.slice(0, STORY_WINDOW) : tlEvents;
  const storyFoot = tlEvents.length > STORY_WINDOW + 4
    ? `<button type="button" class="btn btn-expand" data-action="story-all">` +
      (foldStory
        ? `show all ${fmtInt(tlEvents.length)} events ↓`
        : 'collapse the story ↑') +
      `</button>`
    : '';

  /* "moved together": other covenants that shared a transaction with this one,
     collected across the whole story and counted (used as spoke weights) */
  const togetherCounts = new Map();
  for (const ev of events) {
    if (Array.isArray(ev.with_covenants)) {
      for (const id of ev.with_covenants) {
        if (id === c.covenant_id) continue;
        togetherCounts.set(id, (togetherCounts.get(id) || 0) + 1);
      }
    }
  }
  const togetherSection = togetherCounts.size
    ? `<section class="together" aria-label="Coins moved together"><h2>moved together</h2>` +
      `<p class="dim together-sub">other smart coins that shared a transaction with this one — tap any dot to open it</p>` +
      `<canvas id="together-graph" class="together-graph" width="600" height="360" role="img"` +
      ` aria-label="graph of ${esc(fmtInt(togetherCounts.size))} coins that moved with this one"></canvas></section>`
    : '';

  /* holders panel: keys that have controlled a piece of this coin's state.
     optional field — only rendered when the detail export ships it */
  let holdersSection = '';
  if (Array.isArray(c.holders) && c.holders.length) {
    const holderRows = c.holders.map((h) => {
      const pk = String(h.pubkey || '');
      const keyLabel = knownKeyLabel(pk);
      const inner = `${avatarSvg(pk, 34)}<span class="holder-name">${esc(friendlyName(pk))}</span>` +
        `<span class="holder-key mono">${esc(shortHex(pk, 8, 6))}</span>`;
      const idCell = PUBKEY_RE.test(pk)
        ? `<a class="holder-id" href="#/${esc(network)}/addr/${esc(pk)}" title="${esc(pk)}">${inner}</a>`
        : `<span class="holder-id" title="${esc(pk)}">${inner}</span>`;
      const badges = [];
      if (keyLabel) badges.push(`<span class="flag flag-off flag-gen" title="a known key — this is bulk testnet traffic, not a person or an app">${esc(keyLabel)}</span>`);
      if (h.controls_now) badges.push(`<span class="pill pill-alive" title="this key controls a live piece of this coin right now">controls now</span>`);
      if (h.states_seen != null) badges.push(`<span class="dim">${esc(fmtInt(h.states_seen))} state${h.states_seen === 1 ? '' : 's'} seen</span>`);
      return `<li class="holder-row">${idCell}<span class="holder-meta">${badges.join(' ')}</span></li>`;
    }).join('');
    holdersSection = `<section class="holders" aria-label="Holders"><h2>holders</h2>` +
      `<p class="dim">keys that have controlled a piece of this coin’s state</p>` +
      `<ul class="holder-list">${holderRows}</ul></section>`;
  } else if (Array.isArray(c.holders) && (c.utxos || []).length) {
    /* the export answered and found no p2pk owners — for shapes that hide
       their controller (p2sh commitments), say so instead of omitting */
    holdersSection = `<section class="holders" aria-label="Holders"><h2>holders</h2>` +
      `<p class="dim">holders unknown — this covenant shape hides its controller until each spend.</p></section>`;
  }

  view.innerHTML =
    `<a class="back" href="#/explore">← all smart coins</a>` +
    `<header class="detail-head">` +
    `<span role="img" aria-label="avatar of ${esc(rec.name)}">${avatarSvg(c.covenant_id, 88)}</span>` +
    `<div class="detail-id">` +
    `<h1>${esc(rec.name)}</h1>` +
    `<p class="detail-tags">` +
    (namedTemplate ? `<span class="flag flag-tpl flag-tpl-primary" title="recognized as ${esc(namedTemplate)} — decoded from the chain’s own bytes">${esc(namedTemplate)}</span>` : '') +
    `<span class="pill ${alive ? 'pill-alive' : 'pill-retired'}">${alive ? 'alive' : 'retired'}</span>` +
    `<button type="button" class="star${watched ? ' starred' : ''}" data-action="watch" data-id="${esc(c.covenant_id)}"` +
    ` aria-pressed="${watched}" aria-label="${watched ? 'stop watching' : 'watch'} this coin">★</button>` +
    (genLabel ? `<span class="flag flag-off flag-gen" title="controlled by the known testnet traffic-generator key — bulk p2pk traffic, not an app">${esc(genLabel)}</span>` : '') +
    lineageBadge(c) +
    tokenBacklinkHtml(network, c.covenant_id) +
    `<span class="dim">smart coin on ${esc(NETWORKS[network].label)}</span> ${usdToggleHtml()}</p>` +
    `<p class="id-chip"><span class="mono">${esc(shortHex(c.covenant_id, 10, 8))}</span>` +
    `<button type="button" class="copy-btn" data-action="copy" data-copy="${esc(c.covenant_id)}" aria-label="copy this coin’s full id">copy id</button>` +
    `<button type="button" class="copy-btn" data-action="copy" data-copy="${esc(shareUrl(network, c.covenant_id))}" aria-label="copy a shareable link to this coin">share</button></p>` +
    (c.kcc1_template_hash
      ? `<p class="id-chip kcc1-row"><span class="dim" title="canonical template identity per the KCC-1 draft at revision 55b28d8 — the hash of this coin’s revealed program with its proven state range excised. the draft is still moving; kascov recomputes on meaningful revisions">KCC-1 template</span>` +
        ` <span class="mono">${esc(shortHex(c.kcc1_template_hash, 10, 8))}</span>` +
        `<button type="button" class="copy-btn" data-action="copy" data-copy="${esc(c.kcc1_template_hash)}" aria-label="copy the full KCC-1 template hash">copy</button></p>`
      : '') +
    `</div></header>` +
    `<p class="detail-summary">${esc(summaryBits.join(' · '))}.</p>` +
    coinContractSectionHtml(c) +
    `<section aria-label="Life story"><h2>life story</h2>${kindChips}${labelChips}${tlCount}${truncNote}` +
    `<ol class="timeline">${preface}${tlEmpty}${shownEvents.map((ev) => timelineItem(rec, ev, data, network, flashTx)).join('')}</ol>${storyFoot}</section>` +
    togetherSection +
    holdersSection +
    `<section class="nerd" aria-label="Technical details">` +
    `<button type="button" class="nerd-toggle" data-action="nerd" aria-expanded="${state.nerd}">` +
    `<span class="nerd-switch" aria-hidden="true"></span><span>nerd mode</span>` +
    `<span class="dim nerd-hint">raw ids, DAA scores, UTXOs &amp; scripts</span></button>` +
    `<div id="nerd-panel" class="nerd-panel" ${state.nerd ? '' : 'hidden'}>${state.nerd ? nerdPanel(rec, network, program) : ''}</div>` +
    `</section>`;

  /* wire the "moved together" force graph now that its canvas is in the DOM;
     always stop any prior controller so animation loops don't accumulate */
  if (detailGraph) { detailGraph.stop(); detailGraph = null; }
  if (togetherCounts.size && window.kascovGraph) {
    const canvas = view.querySelector('#together-graph');
    if (canvas) {
      const members = [...togetherCounts.entries()].map(([id, n]) => ({
        covenant_id: id, name: friendlyName(id), shared_txs: n,
      }));
      detailGraph = window.kascovGraph.render(canvas, { members, label: rec.name }, {
        onPick: (node) => { location.hash = `#/${network}/c/${node.id}`; },
      });
    }
  }

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
    `${open ? 'close the editor ↑' : '✎ edit & redeploy this →'}</button>` +
    `<span class="dim gen-cta-hint">re-make this contract with your own parameters, get deployable hex</span></p>`;
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

/* ---- guided visual builder (#/build) — the codeless path ----
   A self-contained form per template: plain-language fields, byte-perfect hex
   via disasm.js skeleton.emit, a live readable source + a downloadable deploy
   script. Independent of the decode-page generator (genState) above — this
   one never reads #decode-input, it starts from example parameters. */

const GUIDED_TEMPLATES = ['SilverScript · Mecenas', 'SilverScript · Escrow', 'SilverScript · LastWill'];

/* obvious repeated-nibble stand-ins so the builder is live on first paint;
   the user is meant to replace every key with their own (keygen prints them) */
const GUIDED_DEFAULTS = {
  'SilverScript · Mecenas': { recipient: '11'.repeat(32), funder_hash: '22'.repeat(32), pledge: '1', period: '3600' },
  'SilverScript · Escrow': { arbiter_hash: '33'.repeat(32), buyer: '11'.repeat(32), seller: '22'.repeat(32) },
  'SilverScript · LastWill': { inheritor_hash: '11'.repeat(32), cold_hash: '22'.repeat(32), hot_hash: '33'.repeat(32) },
};

/* plain-language labels keyed by the SilverScript param name (p.source) */
const GUIDED_LABELS = {
  recipient: 'Recipient', funder: 'Funder (you)', pledge: 'Pledge per period', period: 'Period',
  arbiter: 'Arbiter', buyer: 'Buyer', seller: 'Seller',
  inheritor: 'Inheritor', cold: 'Cold key (override)', hot: 'Hot key (everyday)',
};

let guidedState = null;
let lastGuidedBuild = null;
/* wallet-assisted one-click deploy state — persists across #guided-out
   re-renders (which happen on every keystroke) so a connected wallet stays
   connected while you keep editing. Address only; no keys ever touch kascov. */
let guidedWallet = { address: null };

/* is a Kaspa browser wallet (KasWare) present? feature-detected, never assumed. */
function hasKasware() {
  return typeof window !== 'undefined' && window.kasware && typeof window.kasware.requestAccounts === 'function';
}

/* the wallet strip inside the deploy panel — rendered from guidedWallet so it
   survives partial re-renders. Degrades to a subtle note when no wallet. */
function guidedWalletHtml() {
  if (!hasKasware()) {
    return `<p class="guided-wallet-note dim">no Kaspa wallet detected (KasWare) — you can still deploy below, or install a wallet to connect an address.</p>`;
  }
  if (guidedWallet.address) {
    return `<p class="guided-wallet-note">wallet connected: <code class="mono">${esc(guidedWallet.address)}</code> ` +
      `· <a href="https://faucet-testnet.kaspanet.io" target="_blank" rel="noopener">get testnet funds from the faucet ↗</a></p>`;
  }
  return `<div class="guided-wallet-connect">` +
    `<button type="button" class="btn" data-action="guided-connect-wallet">connect wallet</button>` +
    `<span class="dim">KasWare detected — connect to see your testnet address</span>` +
    `</div>`;
}

function guidedInit(template) {
  if (!GUIDED_TEMPLATES.includes(template)) template = GUIDED_TEMPLATES[0];
  const info = window.kascovDisasm && window.kascovDisasm.skeletonInfo(template);
  const values = {};
  if (info) {
    const defs = GUIDED_DEFAULTS[template] || {};
    for (const p of info.params) values[p.name] = defs[p.name] || '';
  }
  guidedState = { template, values, coinValue: '10' };
}

function renderGuidedBuilder() {
  const host = $('#guided-builder');
  if (!host) return;
  const D = window.kascovDisasm, G = window.kascovGen;
  if (!D || !G || !D.skeletonInfo || !D.emitFromSkeleton) {
    host.innerHTML = '<p class="dim">the in-browser builder isn’t available right now.</p>';
    return;
  }
  if (!guidedState) guidedInit(GUIDED_TEMPLATES[0]);
  host.innerHTML = guidedBuilderHtml();
}

function guidedBuilderHtml() {
  const info = window.kascovDisasm.skeletonInfo(guidedState.template);
  if (!info || !info.emitVerified) return '<p class="dim">this template can’t be assembled in the browser.</p>';
  const tabs = GUIDED_TEMPLATES.map((name) => {
    const on = guidedState.template === name;
    return `<button type="button" class="guided-tab${on ? ' guided-tab-on' : ''}" data-action="guided-template" data-template="${esc(name)}" aria-pressed="${on}">${esc(name.replace('SilverScript · ', ''))}</button>`;
  }).join('');
  const kindLabel = { pubkey: 'x-only pubkey · 64 hex', hash32: 'blake2b-256 · 64 hex', amount: 'TKAS', daa: 'DAA ticks' };
  const placeholder = { pubkey: '64 hex characters', hash32: '64 hex characters', amount: 'e.g. 1.5', daa: 'e.g. 3600' };
  const fields = info.params.map((p) => {
    const v = guidedState.values[p.name] || '';
    const check = window.kascovGen.validateField(p.kind, v);
    let conv = '';
    if (p.kind === 'daa' && check.ok) conv = `<span class="guided-conv dim">≈ ${esc(String(Math.round(Number(v) / 600)))} min at ~10 DAA/s</span>`;
    if (p.kind === 'amount' && check.ok) conv = `<span class="guided-conv dim">= ${esc(String(check.sompi))} sompi</span>`;
    return `<label class="guided-field gen-field">` +
      `<span class="gen-label">${esc(GUIDED_LABELS[p.source] || p.source)} <span class="dim">(${esc(kindLabel[p.kind] || p.kind)})</span></span>` +
      `<input type="text" data-guided-field="${esc(p.name)}" value="${esc(v)}" spellcheck="false" autocomplete="off" placeholder="${esc(placeholder[p.kind] || '')}">` +
      `<span class="gen-hint dim">${esc(p.hint || '')}</span>` + conv +
      (v && !check.ok ? `<span class="gen-err">${esc(check.err)}</span>` : '') +
      `</label>`;
  }).join('');
  const valueField = `<label class="guided-field gen-field">` +
    `<span class="gen-label">Coin value <span class="dim">(TKAS the newborn coin holds)</span></span>` +
    `<input type="text" data-guided-field="__value" value="${esc(guidedState.coinValue)}" spellcheck="false" autocomplete="off" placeholder="e.g. 10">` +
    `<span class="gen-hint dim">funded from your faucet wallet at deploy time, not minted</span>` +
    `</label>`;
  return `<div class="guided-tabs" role="group" aria-label="Template">${tabs}</div>` +
    `<div class="gen-fields">${fields}${valueField}</div>` +
    `<div id="guided-out">${guidedOutputsHtml()}</div>`;
}

function guidedOutputsHtml() {
  const info = window.kascovDisasm.skeletonInfo(guidedState.template);
  const args = {}, values = {};
  for (const p of info.params) {
    const check = window.kascovGen.validateField(p.kind, guidedState.values[p.name] || '');
    if (!check.ok) {
      lastGuidedBuild = null;
      return `<p class="gen-wait dim">fill in the fields above — the readable source and compiled hex appear here, live as you type.</p>`;
    }
    args[p.name] = check.value;
    values[p.name] = check;
  }
  const valCheck = window.kascovGen.validateField('amount', guidedState.coinValue || '');
  if (!valCheck.ok) { lastGuidedBuild = null; return `<p class="gen-wait dim">coin value: ${esc(valCheck.err)}</p>`; }

  const emitted = window.kascovDisasm.emitFromSkeleton(guidedState.template, args);
  if (!emitted) { lastGuidedBuild = null; return `<p class="gen-err">could not assemble the script — this should not happen; please report it.</p>`; }
  const hex = window.kascovDisasm.toHex(emitted);

  /* self-verify: the emitted bytes must decode back to exactly these args */
  const redecoded = window.kascovDisasm.disassemble(emitted);
  const back = window.kascovDisasm.matchTemplates(redecoded.instructions, emitted);
  const roundTrips = !!back && back.name === guidedState.template && info.params.every((p) => {
    const got = (back.fields.find((f) => f.name === p.name) || {}).value;
    return got === window.kascovDisasm.toHex(Uint8Array.from(args[p.name]));
  });
  if (!roundTrips) { lastGuidedBuild = null; return `<p class="gen-err">round-trip check failed — not offering this script. please report it.</p>`; }

  const source = window.kascovGen.buildSource(guidedState.template, info.params, values,
    { date: new Date().toISOString().slice(0, 10) });
  const deploy = window.kascovGen.buildDeployCommand(hex, String(valCheck.sompi));
  lastGuidedBuild = { template: guidedState.template, hex, sompi: String(valCheck.sompi) };

  const block = (title, body, hint) =>
    `<div class="gen-block"><div class="gen-block-head"><span>${esc(title)}</span>` +
    (hint ? `<span class="dim">${esc(hint)}</span>` : '') +
    `<button type="button" class="copy-btn" data-action="copy-block">copy</button></div>` +
    `<pre>${esc(body)}</pre></div>`;
  const short = guidedState.template.replace('SilverScript · ', '');
  const actions = `<div class="guided-actions">` +
    `<button type="button" class="btn btn-accent" data-action="guided-download">⬇ download deploy.sh</button>` +
    `<button type="button" class="btn" data-action="guided-publish">publish as verified source</button>` +
    `<a class="btn" href="#/decode?s=${esc(hex)}">▶ open in the decoder</a>` +
    `<span id="guided-publish-result" class="compile-publish-result"></span></div>`;
  /* one-click, no-toolchain deploy: the custodial worker signs & broadcasts.
     Feature-detected end-to-end — if the endpoint is gated off (404) or the
     network fails, the click falls back to the download-deploy.sh path above. */
  const deployPanel = `<div class="guided-deploy" id="guided-deploy">` +
    `<div class="guided-deploy-head"><span class="guided-deploy-title">Deploy on testnet-10 — no toolchain needed</span>` +
    `<span class="dim">signed & broadcast by the kascov worker; the download above stays available as a fallback</span></div>` +
    `<div id="guided-wallet" class="guided-wallet">${guidedWalletHtml()}</div>` +
    `<div class="guided-deploy-actions">` +
    `<button type="button" class="btn btn-accent" data-action="guided-deploy">🚀 deploy on testnet-10</button>` +
    `<span class="dim">deploys the exact ${esc(emitted.length + '')}-byte script above with a coin value of ${esc(String(valCheck.sompi))} sompi</span>` +
    `</div>` +
    `<div id="guided-deploy-result" class="guided-deploy-result" aria-live="polite"></div>` +
    `</div>`;
  return `<p class="gen-verify-ok">byte-perfect — re-decodes as ${esc(short)} with your parameters ✓</p>` +
    block('the contract, readable', source, 'canonical SilverScript source') +
    block('the contract, compiled', hex, `${emitted.length} bytes of Kaspa script`) +
    block('deploy it on testnet-10', deploy, 'or download the full script below') +
    actions +
    deployPanel;
}

/* coins born from THIS tab's one-click deploys — their pages may 404 for a
   few minutes while testnet reorg churn settles, so the not-found card and
   the deploy panel both soften into a "still settling" state for them */
const freshDeploys = new Set();

/* Poll the per-coin endpoint after a deploy (10s cadence, up to 3 minutes)
   and only swap in the coin link once the indexer actually serves the page.
   Stops silently if the deploy panel was re-rendered away. */
function awaitIndexed(net, cid, out) {
  const started = Date.now();
  const link = `<a class="btn btn-accent" href="#/${esc(net)}/c/${esc(cid)}">▶ open ${esc(cid.slice(0, 12))}… on kascov</a>`;
  const settle = (html) => {
    const wait = out.querySelector('.guided-deploy-wait');
    if (wait) wait.outerHTML = html;
  };
  const tick = () => {
    if (!out.isConnected || !out.querySelector('.guided-deploy-wait')) return;
    fetch(`data/${net}/c/${cid}.json`, { cache: 'no-cache' })
      .then((r) => {
        if (r.ok) { settle(link); return; }
        if (Date.now() - started > 180_000) {
          settle(`<p class="guided-deploy-wait dim">still settling — the page below will fill in once the indexer catches up.</p>` + link);
          return;
        }
        setTimeout(tick, 10_000);
      })
      .catch(() => {
        if (Date.now() - started > 180_000) { settle(link); return; }
        setTimeout(tick, 10_000);
      });
  };
  tick();
}

function renderBuild() {
  document.title = 'make your own smart coin — kascov';
  renderGuidedBuilder();
  initCompiler();
}

/* ------------------------------------------- transaction preflight (#/preflight)
   The playground's third mode: paste a transaction as JSON, the worker
   answers "would a node accept this?" — without broadcasting. Everything
   below is a pure html helper over the response shape
   { ok, verdict, findings[], masses?, fee?, executed? }, so each piece
   renders whatever fields this worker serves and skips the rest.
   Feature-detected end-to-end: a 404 from an older worker degrades to an
   honest "needs a newer worker" card. The pasted tx is sent to the
   preflight call and nowhere else — the endpoint is no-store. */

/* a small broken v1 tx demonstrating the classic budget trap
   (rusty-kaspa#1073): one input with a signature check and NO computeBudget
   — the effective allowance is 9,999 script units, one CheckSig needs
   100,000. The guide's trap section decodes the numbers. */
const PREFLIGHT_EXAMPLE = `{
  "version": 1,
  "inputs": [
    {
      "previousOutpoint": {
        "transactionId": "a9a7df1a71307c0518aa001920b42b9afecbaa503d61c279c08e45ee6bfc8bdb",
        "index": 0
      },
      "signatureScript": "",
      "sequence": 0,
      "sigOpCount": 1
    }
  ],
  "outputs": [
    {
      "value": 99999000,
      "scriptPublicKey": {
        "version": 0,
        "script": "201111111111111111111111111111111111111111111111111111111111111111ac"
      }
    }
  ],
  "lockTime": 0
}`;

function renderPreflight() {
  document.title = 'transaction preflight — kascov';
  /* the section is static html; a previous verdict survives view switches,
     the same way the decoder keeps its output */
}

function preflightVerdictHtml(d) {
  const meta = {
    ready: { cls: 'pf-ready', icon: '✓', label: 'ready to broadcast', sub: 'every check passed — a node should accept this transaction' },
    will_fail: { cls: 'pf-fail', icon: '✗', label: 'will fail', sub: 'a node would reject this transaction as-is — the findings below say why, and how to fix it' },
    incomplete: { cls: 'pf-incomplete', icon: '…', label: 'incomplete', sub: 'not enough of the transaction to decide — the findings below say what is missing' },
  }[String(d.verdict || '')] || { cls: 'pf-incomplete', icon: '?', label: String(d.verdict || 'no verdict'), sub: '' };
  return `<div class="pf-verdict ${meta.cls}"><span class="pf-icon" aria-hidden="true">${meta.icon}</span>` +
    `<strong>${esc(meta.label)}</strong>` +
    (meta.sub ? `<span class="pf-sub">${esc(meta.sub)}</span>` : '') + `</div>`;
}

function preflightFindingHtml(f) {
  const sev = f.severity === 'error' ? 'error' : f.severity === 'warn' ? 'warn' : 'info';
  const metaBits = [];
  if (Number.isInteger(f.input_index)) {
    metaBits.push(`<button type="button" class="pf-input-link" data-action="pf-goto" data-target="pf-input-${f.input_index}">input #${f.input_index}</button>`);
  }
  if (f.code) metaBits.push(`<span class="pf-code mono dim">${esc(String(f.code))}</span>`);
  const fix = f.suggestion
    ? `<div class="pf-fix">fix: <code class="mono">${esc(String(f.suggestion))}</code></div>`
    : '';
  return `<li class="pf-finding"><span class="pf-chip pf-chip-${sev}">${sev}</span>` +
    `<div class="pf-finding-body"><p>${esc(String(f.message || ''))}</p>` +
    (metaBits.length ? `<div class="pf-meta">${metaBits.join(' ')}</div>` : '') +
    fix + `</div></li>`;
}

function preflightMassesHtml(m) {
  if (!m) return '';
  /* the worker sends per-dimension limits ({compute, transient, storage} —
     transient's ceiling differs from the others); a bare number is tolerated */
  const lims = (m.limit && typeof m.limit === 'object') ? m.limit
    : { compute: m.limit, storage: m.limit, transient: m.limit };
  const bar = (label, val, limRaw) => {
    if (val == null) return '';
    const n = Number(val) || 0;
    const limit = Number(limRaw) || 0;
    const pct = limit > 0 ? Math.min(100, (n / limit) * 100) : 0;
    const over = limit > 0 && n > limit;
    return `<div class="pf-mass"><span class="pf-mass-label">${esc(label)}</span>` +
      `<span class="pf-mass-bar"><span class="pf-mass-fill${over ? ' pf-mass-over' : ''}" style="width:${pct.toFixed(1)}%"></span></span>` +
      `<span class="pf-mass-n mono">${esc(fmtInt(n))}${limit ? ` / ${esc(fmtInt(limit))}` : ''}${over ? ' — over the limit' : ''}</span></div>`;
  };
  return `<div class="pf-card"><h3>masses vs the block limit</h3>` +
    bar('compute', m.compute, lims.compute) + bar('storage', m.storage, lims.storage) + bar('transient', m.transient, lims.transient) +
    `</div>`;
}

function preflightFeeHtml(fee) {
  if (!fee || fee.estimate_sompi == null) return '';
  return `<div class="pf-card"><h3>fee</h3>` +
    `<p class="pf-fee-n mono">${esc(fmtInt(Number(fee.estimate_sompi) || 0))} sompi</p>` +
    (fee.note ? `<p class="dim">${esc(String(fee.note))}</p>` : '') + `</div>`;
}

function preflightExecutedHtml(list) {
  if (!Array.isArray(list) || !list.length) return '';
  const rows = list.map((r) => {
    const pass = r.pass === true;
    const idx = Number.isInteger(r.input_index) ? r.input_index : 0;
    const used = r.script_units_used;
    const allow = r.allowance;
    return `<li class="pf-exec" id="pf-input-${idx}">` +
      `<span class="flag ${pass ? 'flag-yes' : 'flag-no'}">${pass ? '✓ pass' : '✗ fail'}</span>` +
      `<span class="mono">input #${idx}</span>` +
      (used != null
        ? `<span class="dim">${esc(fmtInt(Number(used) || 0))} script units used${allow != null ? ` of ${esc(fmtInt(Number(allow) || 0))} allowed` : ''}</span>`
        : '') +
      `</li>`;
  }).join('');
  return `<div class="pf-card pf-exec-card"><h3>executed inputs — the real script engine</h3>` +
    `<p class="dim">inputs carrying both their witness and their UTXO&rsquo;s script ran through Kaspa&rsquo;s actual engine, with the budget your transaction declares.</p>` +
    `<ul class="pf-exec-list">${rows}</ul></div>`;
}

function preflightResultHtml(d) {
  if (!d || typeof d !== 'object') {
    return `<div class="pf-verdict pf-incomplete"><span class="pf-icon" aria-hidden="true">?</span><strong>no answer</strong><span class="pf-sub">the worker sent nothing readable back — try again in a moment</span></div>`;
  }
  if (d.ok === false && !d.verdict) {
    return `<div class="pf-verdict pf-incomplete"><span class="pf-icon" aria-hidden="true">?</span><strong>can&rsquo;t preflight</strong><span class="pf-sub">${esc(String(d.error || 'the worker rejected the request'))}</span></div>`;
  }
  const findings = Array.isArray(d.findings) && d.findings.length
    ? `<ul class="pf-findings">${d.findings.map(preflightFindingHtml).join('')}</ul>`
    : '';
  const cards = preflightMassesHtml(d.masses) + preflightFeeHtml(d.fee);
  const foot = `<p class="pf-foot dim">the checks are the <a href="/guide.html#trap" target="_blank" rel="noopener">guide&rsquo;s trap section</a>, automated — and once you broadcast, paste the txid into search: every transaction gets a page here.</p>`;
  return preflightVerdictHtml(d) + findings +
    (cards ? `<div class="pf-cards">${cards}</div>` : '') +
    preflightExecutedHtml(d.executed) + foot;
}

/* -------------------------------------------------------------- changelog */

/* the "have you seen the newest entry" stamp — date alone can repeat across
   entries shipped the same day, so the title disambiguates */
const changelogStamp = (e) => `${e.date || ''}|${e.title || ''}`;

/* the landing "what's new" card — the newest entry, with an unseen dot that
   clears once the visitor opens #/changelog */
function renderWhatsNew() {
  const sec = $('#section-whatsnew');
  const card = $('#whatsnew-card');
  if (!sec || !card) return; /* stale cached index.html */
  loadChangelog().then((list) => {
    if (!list) { sec.hidden = true; return; }
    if (parseRoute().view !== 'landing') return;
    const latest = list[0];
    let seen = null;
    try { seen = localStorage.getItem('kascov-changelog-seen'); } catch (e) { /* private mode */ }
    const fresh = seen !== changelogStamp(latest);
    card.innerHTML =
      `<h3>${fresh ? '<span class="new-dot" aria-hidden="true"></span>' : ''}what’s new</h3>` +
      `<p><strong>${esc(latest.title || '')}</strong> — ${esc(latest.body || '')}</p>` +
      `<span class="whatsnew-meta dim">${esc(latest.date || '')} · everything that changed →</span>`;
    sec.hidden = false;
  });
}

/* the landing "built with covenants" showcase — curated entries committed in
   web/community.json ({ name, by, blurb, links:{site?,repo?,example?}, date }).
   Hidden entirely when the file is missing or empty, so older deploys are
   unaffected. Adding a future project = edit community.json only. */
function renderCommunity() {
  const sec = $('#section-community');
  const host = $('#community-cards');
  if (!sec || !host) return; /* stale cached index.html */
  loadCommunity().then((list) => {
    if (!list) { sec.hidden = true; return; }
    if (parseRoute().view !== 'landing') return;
    /* curated content, but keep the cheap guard: internal hash routes or
       plain http(s) only */
    const safe = (u) => /^(https?:\/\/|#\/)/.test(u);
    host.innerHTML = list.map((e) => {
      const L = e.links || {};
      const links = [];
      if (L.repo && safe(String(L.repo))) links.push(`<a href="${esc(String(L.repo))}" target="_blank" rel="noopener noreferrer">the repo ↗</a>`);
      if (L.example && safe(String(L.example))) {
        const ex = String(L.example);
        links.push(`<a href="${esc(ex)}"${ex.startsWith('#') ? '' : ' target="_blank" rel="noopener noreferrer"'}>see it on kascov →</a>`);
      }
      if (L.site && safe(String(L.site))) links.push(`<a href="${esc(String(L.site))}" target="_blank" rel="noopener noreferrer">site ↗</a>`);
      return `<div class="community-card">` +
        `<h3>${esc(String(e.name || ''))}</h3>` +
        (e.by ? `<p class="community-by dim">by ${esc(String(e.by))}</p>` : '') +
        `<p>${esc(String(e.blurb || ''))}</p>` +
        (links.length ? `<p class="community-links">${links.join(' · ')}</p>` : '') +
        `</div>`;
    }).join('');
    sec.hidden = false;
  });
}

function renderChangelog() {
  document.title = 'what’s new — kascov';
  const host = $('#changelog-list');
  if (!host) return;
  host.innerHTML = '<li class="dim">loading…</li>';
  loadChangelog().then((list) => {
    if (parseRoute().view !== 'changelog') return;
    if (!list) { host.innerHTML = '<li class="dim">nothing here yet.</li>'; return; }
    host.innerHTML = list.map((e) =>
      `<li class="changelog-item">` +
      `<span class="changelog-date mono dim">${esc(String(e.date || ''))}</span>` +
      `<div class="changelog-body"><h2>${esc(String(e.title || ''))}</h2>` +
      `<p>${esc(String(e.body || ''))}</p></div></li>`
    ).join('');
    /* reading the page clears the landing card's unseen dot */
    try { localStorage.setItem('kascov-changelog-seen', changelogStamp(list[0])); } catch (err) { /* private mode */ }
  });
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
    const alive = isAlive(c);
    const sb = [`${c.genesis_daa != null ? 'born' : 'first seen'} ${relTimeShort(toMs(c.genesis_daa != null ? c.genesis_daa : c.first_seen_daa))}`];
    if (alive) sb.push(`holds ${amountWithUsd(c.live_value, network)}`);
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
    `<p class="page-sub">${bits.map(esc).join(' · ')} <span class="dim">on ${esc(net.label)}</span> ${usdToggleHtml()}</p></header>` +
    (rows.length ? `<div class="coin-grid">${cards}</div>`
      : `<div class="empty-card"><h2>this address hasn’t touched any smart coins we’ve seen.</h2>` +
        `<p class="dim">kascov matches p2pk covenant states only — plain payments to this address don’t appear here, and covenants with richer scripts may not name their owner.</p></div>`);
}

/* ------------------------------------------------------------ lane pages */

/* hour-bucketed event bars over the lane's recent life (same visual language
   as the analytics sparklines) — gaps between buckets render as quiet hours */
function laneActivitySvg(data) {
  const raw = (data.activity || [])
    .map((a) => ({ daa: Number(a.daa) || 0, count: Number(a.count) || 0 }))
    .filter((a) => a.count >= 0);
  if (!raw.length) return '';
  const width = Math.max(1, Number(data.bucket_daa) || 36_000);
  /* fill omitted (empty) buckets, capped to the last 72 so the SVG stays small */
  const last = raw[raw.length - 1].daa;
  const first = Math.max(raw[0].daa, last - 71 * width);
  const byDaa = new Map(raw.map((a) => [a.daa, a.count]));
  const series = [];
  for (let d = first; d <= last; d += width) series.push(byDaa.get(d) || 0);
  const max = Math.max(1, ...series);
  const W = 720, H = 56, gap = 2;
  const slot = W / series.length;
  const bw = Math.max(1, slot - gap);
  const bars = series.map((c, i) => {
    const h = c > 0 ? Math.max((c / max) * (H - 4), 2) : 1;
    const x = i * slot + (slot - bw) / 2;
    return `<rect x="${x.toFixed(1)}" y="${(H - h).toFixed(1)}" width="${bw.toFixed(1)}" height="${h.toFixed(1)}" rx="1" fill="var(--accent)"${c > 0 ? '' : ' opacity="0.35"'}/>`;
  }).join('');
  const spanMs = series.length * width * MS_PER_DAA;
  return `<h2>activity · events per hour</h2>` +
    `<p class="an-note dim">the last ${esc(fmtSpan(spanMs))} of this lane, oldest to newest · peak ${esc(fmtInt(max))} event${max === 1 ? '' : 's'}/hour</p>` +
    `<svg viewBox="0 0 ${W} ${H}" class="an-spark lane-spark" role="img" preserveAspectRatio="none" ` +
    `aria-label="Lane activity per hour over the last ${esc(fmtSpan(spanMs))}, oldest to newest, peaking at ${esc(fmtInt(max))} events per hour">${bars}</svg>`;
}

function renderLane(route) {
  const network = state.network;
  const view = $('#view-lane');
  const ns = route.id;
  const net = NETWORKS[network];
  const bytes = ns.match(/../g) || [];
  const printable = bytes.length === 4 && bytes.every((b) => { const c = parseInt(b, 16); return c >= 0x20 && c <= 0x7e; });
  const plain = printable ? bytes.map((b) => String.fromCharCode(parseInt(b, 16))).join('') : `0x${ns}`;
  document.title = `lane ${plain} — kascov`;
  const back = `<a class="back" href="#/${esc(network)}/explore">← all smart coins</a>`;
  const head = (sub) => back +
    `<header class="page-head lane-head"><h1>lane ${nsLabel(ns)}</h1>` +
    `<p class="page-sub">a KIP-21 payload lane — every transaction stamped with this 4-byte namespace <span class="dim">on ${esc(net.label)}</span></p>` +
    (sub ? `<p class="page-sub dim">${sub}</p>` : '') + `</header>`;
  const cached = lanePages.get(`${network}/${ns}`);
  const fresh = cached && Date.now() - cached.at < LANE_PAGE_TTL_MS;
  if (!fresh) {
    /* stale-while-revalidate: keep showing an expired record while the
       refetch runs; only the very first visit shows the loading line */
    loadLanePage(network, ns)
      .then(() => {
        const r = parseRoute();
        if (state.network === network && r.view === 'lane' && r.id === ns) renderLane(r);
      })
      .catch(() => {
        if (cached) return; /* stale data beats an error card */
        const r = parseRoute();
        if (state.network === network && r.view === 'lane' && r.id === ns) {
          view.innerHTML = head('') + `<div class="empty-card"><h2>couldn’t load this lane.</h2>` +
            `<p class="dim">the lookup didn’t answer — the worker may be busy. it’s not you.</p>` +
            `<button type="button" class="btn" data-action="retry-lane">try again</button></div>`;
        }
      });
  }
  if (!cached) {
    view.innerHTML = head('reading this lane’s traffic…');
    return;
  }
  if (cached.missing) {
    view.innerHTML = head('') + `<div class="empty-card"><h2>lane pages need a newer worker.</h2>` +
      `<p class="dim">this kascov worker doesn’t serve per-lane dashboards yet — the based-app overview on the explore page still works.</p></div>`;
    return;
  }
  const data = cached.data || {};
  const events = Number(data.events) || 0;
  const covenants = Number(data.covenants) || 0;
  const stats =
    `<div class="lane-stats">` +
    `<div class="stat"><span class="stat-n">${esc(fmtInt(events))}</span><span class="stat-label">event${events === 1 ? '' : 's'} in this lane</span></div>` +
    `<div class="stat"><span class="stat-n">${esc(fmtInt(covenants))}</span><span class="stat-label">smart coin${covenants === 1 ? '' : 's'} involved</span></div>` +
    `</div>`;
  if (!events) {
    view.innerHTML = head('') + stats +
      `<div class="empty-card"><h2>nothing in this lane yet.</h2>` +
      `<p class="dim">no indexed covenant transaction carries the <span class="mono">0x${esc(ns)}</span> namespace so far.</p></div>`;
    return;
  }
  const entry = state.cache[network];
  const recent = (data.recent || []).slice(0, 20).map((e) => {
    const covId = String(e.covenant_id || '');
    const gridRec = entry && entry.index.byId.get(covId);
    const name = gridRec ? gridRec.name : friendlyName(covId);
    const txid = String(e.txid || '');
    return `<li class="lane-ev">` +
      `<span class="lane-ev-kind">${esc(String(e.kind || 'event'))}</span>` +
      `<a class="lane-ev-coin" href="#/${esc(network)}/c/${esc(covId)}${txid ? `?tx=${esc(txid)}` : ''}">${avatarSvg(covId, 22)} ${esc(name)}</a>` +
      (txid ? `<span class="mono dim lane-ev-tx">tx ${esc(shortHex(txid, 8, 6))}</span>` : '') +
      `<span class="dim lane-ev-daa">DAA ${esc(fmtInt(e.accepting_daa || 0))}</span></li>`;
  }).join('');
  view.innerHTML = head('') + stats +
    `<section class="lane-activity" aria-label="Lane activity">${laneActivitySvg(data)}</section>` +
    `<section class="lane-recent" aria-label="Recent lane events"><h2>recent events</h2>` +
    (recent
      ? `<ul class="lane-ev-list">${recent}</ul>` +
        ((data.recent || []).length < events ? `<p class="dim">showing the ${fmtInt(Math.min(20, (data.recent || []).length))} newest of ${fmtInt(events)} events.</p>` : '')
      : '<p class="dim">no events recorded.</p>') +
    `</section>`;
}

/* --------------------------------------------------------- token directory */

/* #/{net}/tokens — every covenant token (KCC20-shaped coin) the worker
   decoded, as one table. Born modular (M5): renders from core/data's
   loadTokens cache and core helpers only, no shared view state — lift into
   web/views/tokens.js unchanged when the views seam lands. */

const TOKEN_NOTE_FALLBACK = 'decoded from chain — not validated: kascov shows what these covenants ' +
  'wrote in their own bytes; it does not (yet) check KCC20 balance rules';
const TOKEN_NOTE_VALIDATED = 'validated from chain — “verified” means every event in the token’s ' +
  'history matched the KCC20 rules and supply is conserved; anything kascov could not prove stays unvalidated';

/* The conservative per-token validation verdict, feature-detected: rows from
   a newer worker carry status verified|invalid|unvalidated (plus supply,
   minted, burned, holders); older rows keep status active|burned and none of
   it, and render exactly as before. A token is never CALLED verified unless
   the worker proved every event in its history — that's the whole point. */
const TOKEN_VSTATUS = {
  verified: {
    cls: 'pill-verified', label: 'verified ✓',
    tip: 'every event in this token’s history matched the KCC20 rules and supply is conserved — verified from chain',
  },
  invalid: {
    cls: 'pill-invalid', label: 'invalid',
    tip: 'this token broke a KCC20 rule somewhere in its history',
  },
  unvalidated: {
    cls: 'pill-unvalidated', label: 'unvalidated',
    tip: 'kascov could not match every event in this token’s history to a known KCC20 rule — not counted as verified',
  },
};

function tokenVstatus(t) {
  return t && TOKEN_VSTATUS[t.status] ? t.status : null;
}

function tokenStatusBadge(t) {
  const vs = tokenVstatus(t);
  if (!vs) return '';
  const m = TOKEN_VSTATUS[vs];
  const tip = (vs !== 'verified' && (t.invalid_reason || t.reason)) || m.tip;
  return `<span class="pill ${m.cls}" title="${esc(tip)}">${esc(m.label)}</span>`;
}

/* token amounts are raw on-chain units (KCC20 carries no decimals kascov
   would trust); anything non-numeric shows as itself rather than lying */
function fmtTokenAmount(a) {
  if (typeof a === 'number' && Number.isFinite(a)) return fmtInt(a);
  return a == null ? '—' : String(a);
}

/* the decoded fields object as compact label/value chips — long hex values
   shortened for display, the full value in the tooltip. The state amount is
   8 little-endian bytes: decode it to the human number (same decode the
   worker applies for supply), raw hex kept in the tooltip for nerds.
   owner_identifier stays truncated hex — it IS an identifier. */
function tokenFieldChips(fields) {
  const entries = Object.entries(fields || {}).slice(0, 6);
  if (!entries.length) return '<span class="dim">—</span>';
  return entries.map(([k, v]) => {
    const val = String(v == null ? '' : v);
    if (k === 'amount') {
      const n = leAmount(val);
      if (n != null) {
        return `<span class="tpl-field" title="amount (raw LE bytes): ${esc(val)}">amount <strong>${esc(n)}</strong></span>`;
      }
    }
    const shown = /^[0-9a-fA-F]{16,}$/.test(val) ? shortHex(val.toLowerCase(), 8, 6)
      : val.length > 28 ? `${val.slice(0, 28)}…` : val;
    return `<span class="tpl-field" title="${esc(k)}: ${esc(val)}">${esc(k)} <strong>${esc(shown)}</strong></span>`;
  }).join('');
}

function renderTokens() {
  const network = state.network;
  const view = $('#view-tokens');
  if (!view) return; /* stale cached index.html */
  const net = NETWORKS[network];
  document.title = `tokens on ${net.label} — kascov`;
  const back = `<a class="back" href="#/${esc(network)}/explore">← all smart coins</a>`;
  /* rows carrying a validation verdict mean a newer worker — the directory
     gains badges, supply/holders columns and token-page links; without them
     everything renders exactly as before */
  const head = (d, validated) => back +
    `<header class="page-head tokens-head"><h1>tokens</h1>` +
    `<p class="page-sub">covenant tokens on ${esc(net.label)} — every KCC20-shaped coin the indexer decoded, ` +
    `read straight from the chain’s bytes ${usdToggleHtml()}</p>` +
    (validated
      ? `<p class="tokens-note tokens-note-info">${esc((d && d.note) || TOKEN_NOTE_VALIDATED)}</p>`
      : `<p class="tokens-note">⚠ ${esc((d && d.note) || TOKEN_NOTE_FALLBACK)}</p>`) +
    `</header>`;
  const cached = tokenPages.get(network);
  const fresh = cached && Date.now() - cached.at < TOKENS_TTL_MS;
  if (!fresh) {
    /* stale-while-revalidate, same shape as lane pages: keep showing an
       expired record while the refetch runs */
    loadTokens(network)
      .then(() => {
        if (state.network === network && parseRoute().view === 'tokens') renderTokens();
      })
      .catch(() => {
        if (cached) return; /* stale data beats an error card */
        if (state.network === network && parseRoute().view === 'tokens') {
          view.innerHTML = head(null) + `<div class="empty-card"><h2>couldn’t load the token directory.</h2>` +
            `<p class="dim">the lookup didn’t answer — the worker may be busy. it’s not you.</p>` +
            `<button type="button" class="btn" data-action="retry-tokens">try again</button></div>`;
        }
      });
  }
  if (!cached) {
    view.innerHTML = head(null) + `<p class="dim">reading this network’s tokens…</p>`;
    return;
  }
  if (cached.missing) {
    view.innerHTML = head(null) + `<div class="empty-card"><h2>the token directory needs a newer worker.</h2>` +
      `<p class="dim">this kascov worker doesn’t serve decoded covenant tokens yet — the rest of the explorer works unchanged.</p></div>`;
    return;
  }
  const d = cached.data || {};
  const tokens = Array.isArray(d.tokens) ? d.tokens : [];
  const validated = tokens.some((t) => tokenVstatus(t));
  if (!tokens.length) {
    view.innerHTML = head(d, validated) + `<div class="empty-card"><h2>no covenant tokens found on this network yet.</h2>` +
      `<p class="dim">they’re coming — the first KCC20-shaped coins will show up here on their own.</p></div>`;
    return;
  }
  /* daa→ms: prefer the payload's own tip anchor, else the grid snapshot's
     (if loaded); with neither, show the raw DAA rather than a made-up time */
  const entry = state.cache[network];
  const toMs = (daa) => {
    if (daa == null) return null;
    if (d.tip_daa != null) {
      return (d.tip_at_ms != null ? d.tip_at_ms : d.generated_at_ms) - (d.tip_daa - daa) * MS_PER_DAA;
    }
    return entry ? daaToMs(daa, entry.data) : null;
  };
  /* Triaged explorer: hide nothing, but collapse the flood of empty/test
     deploys (no holders, no supply, no value, unverified) behind a toggle so
     real coins lead. Verified first, then holders, value, recency. An
     unvalidated directory (old worker) has none of these signals, so it keeps
     the worker's order untouched. */
  const num = (v) => (typeof v === 'number' && isFinite(v) ? v : 0);
  const hasSubstance = (t) =>
    tokenVstatus(t) === 'verified' || num(t.holders) > 0 ||
    num(t.supply) > 0 || num(t.live_value) > 0 || t.alive === true;
  const byActivity = (a, b) => (b.last_activity_daa || 0) - (a.last_activity_daa || 0);
  let shown = tokens.slice();
  let empty = [];
  if (validated) {
    shown = [];
    tokens.forEach((t) => (hasSubstance(t) ? shown : empty).push(t));
    shown.sort((a, b) =>
      ((tokenVstatus(b) === 'verified' ? 1 : 0) - (tokenVstatus(a) === 'verified' ? 1 : 0)) ||
      (num(b.holders) - num(a.holders)) ||
      (num(b.live_value) - num(a.live_value)) ||
      byActivity(a, b));
    empty.sort(byActivity);
  }
  const rowHtml = (t) => {
    const cid = String(t.covenant_id || '');
    const name = t.name || friendlyName(cid);
    const alive = t.alive === true || t.status === 'active';
    const ms = toMs(t.last_activity_daa);
    const when = ms != null ? `<span title="${esc(utcTitle(ms))}">${esc(relTimeShort(ms))}</span>`
      : t.last_activity_daa != null ? `<span class="mono dim">DAA ${esc(fmtInt(t.last_activity_daa))}</span>`
      : '<span class="dim">—</span>';
    /* rows carrying their own verdict link to the token page (badges,
       balances, timeline); verdict-less rows — minter/vault covenants and
       every old-worker row — keep going straight to the coin page, because
       the token endpoint only knows derived tokens (minter ids 404 there) */
    const href = tokenVstatus(t) ? `#/${esc(network)}/token/${esc(cid)}` : `#/${esc(network)}/c/${esc(cid)}`;
    /* deployer-claimed identity from the genesis payload: shown first when
       present, with the canonical name kept beside it — a claim is a claim,
       not uniqueness, and the tooltip says so */
    const claimed = t.claimed_name || t.claimed_ticker;
    const nameHtml = claimed
      ? `<span class="token-name" title="named on chain by its deployer in the genesis payload — claims aren’t unique; the canonical name stays ${esc(name)}">${esc(t.claimed_name || t.claimed_ticker)}${t.claimed_ticker && t.claimed_name ? ` <span class="dim mono">$${esc(t.claimed_ticker)}</span>` : ''}</span> <span class="dim token-canonical">${esc(name)}</span>`
      : `<span class="token-name">${esc(name)}</span>`;
    const rowArt = t.claimed_image_hash
      ? `<span class="token-art-wrap token-art-wrap-sm">${avatarSvg(cid, 26)}<img class="token-art token-art-sm" src="img/${esc(network)}/${esc(cid)}" alt="" onload="this.parentElement.classList.add('art-loaded')" onerror="this.remove()"></span>`
      : avatarSvg(cid, 26);
    return `<tr>` +
      `<td><a class="token-coin" href="${href}">${rowArt} ${nameHtml}</a></td>` +
      `<td>${t.template ? `<span class="flag flag-tpl">${esc(t.template)}</span>` : '<span class="dim">—</span>'}</td>` +
      `<td><div class="tokens-fields">${tokenFieldChips(t.fields)}</div></td>` +
      (validated
        ? `<td class="tokens-supply">${t.supply != null ? esc(fmtTokenAmount(t.supply)) : '<span class="dim">—</span>'}</td>` +
          `<td class="tokens-holders">${t.holders != null ? esc(fmtInt(t.holders)) : '<span class="dim">—</span>'}</td>`
        : '') +
      `<td class="tokens-value">${t.live_value != null ? esc(amountWithUsd(t.live_value, network)) : '<span class="dim">—</span>'}</td>` +
      `<td>${tokenStatusBadge(t) ||
        `<span class="pill ${alive ? 'pill-alive' : 'pill-retired'}" title="${esc(alive ? GLOSSARY.alive : GLOSSARY.retired)}">${alive ? 'alive' : 'retired'}</span>`}</td>` +
      `<td class="tokens-when">${when}</td>` +
      `</tr>`;
  };
  const tableHtml = (list) =>
    `<div class="tokens-tablewrap"><table class="tokens-table">` +
    `<thead><tr><th>token</th><th>template</th><th>decoded fields</th>` +
    (validated ? `<th>supply</th><th>holders</th>` : '') +
    `<th>holds</th><th>status</th><th>last activity</th></tr></thead>` +
    `<tbody>${list.map(rowHtml).join('')}</tbody></table></div>`;
  const countLine = validated
    ? `${fmtInt(shown.length)} token coin${shown.length === 1 ? '' : 's'}` +
      (empty.length ? ` with holders, supply or value · ${fmtInt(empty.length)} empty deploy${empty.length === 1 ? '' : 's'} tucked below` : '') +
      ' — tap any row’s name for its token page'
    : `${fmtInt(tokens.length)} token coin${tokens.length === 1 ? '' : 's'} — tap any row’s name for its full life story`;
  const emptyBlock = validated && empty.length
    ? `<details class="tokens-empty"><summary>${fmtInt(empty.length)} deploy${empty.length === 1 ? '' : 's'} with no holders, supply or value — mostly test &amp; placeholder coins</summary>` +
      tableHtml(empty) + `</details>`
    : '';
  view.innerHTML = head(d, validated) +
    `<p class="dim tokens-count">${esc(countLine)}</p>` +
    tableHtml(shown) + emptyBlock;
}

/* -------------------------------------------------------------- token page */

/* #/{net}/token/{id} — one decoded token: stat tiles, the validation verdict
   (conservative: "verified" is a proof, everything else says why not), top
   balances, and the classified mint/transfer/burn timeline. Born modular
   (M5): renders from core/data's loadTokenDetail cache and core helpers
   only, no shared view state — lifts into web/views/token.js unchanged. */

const TOKEN_EV_META = {
  mint:     { icon: 'born', cls: 'kind-born', chip: 'tok-mint' },
  transfer: { icon: 'move', cls: 'kind-move', chip: 'tok-transfer' },
  burn:     { icon: 'burn', cls: 'kind-burn', chip: 'tok-burn' },
};
const TOKEN_EVENTS_SHOWN = 200; /* defensive DOM cap — the endpoint may not cap */

/* an owner pubkey as a compact link to its address page (plain text when it
   isn't a pubkey shape we can route) */
function tokenOwnerLink(network, pk) {
  const s = String(pk || '');
  if (!s) return '';
  const inner = `${esc(shortHex(s.toLowerCase(), 8, 6))}`;
  return PUBKEY_RE.test(s)
    ? `<a class="mono" href="#/${esc(network)}/addr/${esc(s.toLowerCase())}" title="${esc(s)}">${inner}</a>`
    : `<span class="mono" title="${esc(s)}">${inner}</span>`;
}

function tokenEventItem(ev, network, toMs) {
  const kind = String(ev.token_kind || '');
  const meta = TOKEN_EV_META[kind] || TOKEN_EV_META.transfer;
  const ms = toMs(ev.accepting_daa);
  /* state-accurate wording: on mint/transfer rows `amount` is the created
     cell's FULL state amount — "now holds", never "minted/moved" (summing
     those would overstate; net-delta math is the worker's job, not ours).
     A burn's amount IS the consumed cell's holdings, so that row keeps it. */
  const amount = ev.amount != null ? `<strong>${esc(fmtTokenAmount(ev.amount))}</strong>` : '';
  const from = ev.owner_from ? tokenOwnerLink(network, ev.owner_from) : '';
  const to = ev.owner_to ? tokenOwnerLink(network, ev.owner_to) : '';
  let text;
  if (kind === 'mint') text = `${amount ? `cell now holds ${amount}` : 'cell created'}${to ? ` — owned by ${to}` : ''}`;
  else if (kind === 'burn') text = `${amount ? `burned a cell holding ${amount}` : 'burned'}${from ? ` — from ${from}` : ''}`;
  else text = `${amount ? `now holds ${amount}` : 'moved'}${from ? ` — from ${from}` : ''}${to ? `${from ? '' : ' —'} to ${to}` : ''}`;
  const when = ms != null
    ? `<span title="${esc(utcTitle(ms))}">${esc(relTime(ms))}</span> · <span class="abs-t" title="${esc(utcTitle(ms))}">${esc(absShort(ms))}</span>`
    : `<span class="mono dim">DAA ${esc(fmtInt(ev.accepting_daa || 0))}</span>`;
  return `<li class="tl-item ${meta.cls}">` +
    `<span class="tl-icon" aria-hidden="true">${ICONS[meta.icon]}</span>` +
    `<div class="tl-body">` +
    `<p class="tl-text"><span class="tok-ev-chip ${meta.chip}">${esc(kind || 'event')}</span> ${text}</p>` +
    `<p class="tl-meta">${when}` +
    (ev.txid ? ` · <a href="#/${esc(network)}/tx/${esc(ev.txid)}">view transaction →</a>` : '') +
    `</p></div></li>`;
}

/* the validation box — the page's honesty anchor: a teal proof statement for
   verified, the actual reason (never a shrug) for everything else */
function tokenValidationHtml(t, validation) {
  const v = validation || {};
  const vs = tokenVstatus(t);
  if (!vs) return '';
  const reason = v.reason || t.invalid_reason || t.reason || '';
  const checked = typeof v.checked === 'number'
    ? ` <span class="dim">(${esc(fmtInt(v.checked))} event${v.checked === 1 ? '' : 's'} checked)</span>` : '';
  if (vs === 'verified') {
    return `<section class="token-validation tv-verified" aria-label="Validation">` +
      `<span class="tv-badge">✓ verified from chain</span>` +
      `<p class="tv-body">every event in this token’s history matched the KCC20 rules and supply is conserved — verified from chain.${checked}</p>` +
      `</section>`;
  }
  const badge = vs === 'invalid' ? '✗ invalid' : 'unvalidated';
  const fallback = TOKEN_VSTATUS[vs].tip;
  return `<section class="token-validation tv-${vs}" aria-label="Validation">` +
    `<span class="tv-badge">${esc(badge)}</span>` +
    `<p class="tv-body">${esc(reason || fallback)}${checked}</p>` +
    `</section>`;
}

function renderTokenPage(route) {
  const network = state.network;
  const view = $('#view-token');
  if (!view) return; /* stale cached index.html */
  const id = route.id;
  const net = NETWORKS[network];
  const cached = tokenDetails.get(`${network}/${id}`);
  const back = `<a class="back" href="#/${esc(network)}/tokens">← all tokens</a>`;
  const fresh = cached && Date.now() - cached.at < TOKENS_TTL_MS;
  if (!fresh) {
    /* stale-while-revalidate, same shape as lane pages */
    loadTokenDetail(network, id)
      .then(() => {
        const r = parseRoute();
        if (state.network === network && r.view === 'token' && r.id === id) renderTokenPage(r);
      })
      .catch(() => {
        if (cached) return; /* stale data beats an error card */
        const r = parseRoute();
        if (state.network === network && r.view === 'token' && r.id === id) {
          view.innerHTML = back + `<div class="empty-card"><h2>couldn’t load this token.</h2>` +
            `<p class="dim">the lookup didn’t answer — the worker may be busy. it’s not you.</p>` +
            `<button type="button" class="btn" data-action="retry-token">try again</button></div>`;
        }
      });
  }
  if (!cached) {
    document.title = `token ${friendlyName(id)} — kascov`;
    view.innerHTML = back + `<p class="dim">reading this token’s story…</p>`;
    return;
  }
  if (cached.missing) {
    document.title = 'token not found — kascov';
    view.innerHTML = back + `<div class="empty-card"><h2>token pages need a newer worker.</h2>` +
      `<p class="dim">this kascov worker doesn’t serve per-token pages yet — or this id isn’t a token it knows. ` +
      `the underlying <a href="#/${esc(network)}/c/${esc(id)}">smart coin page</a> still works.</p></div>`;
    return;
  }
  const d = cached.data || {};
  const t = d.token || {};
  const name = t.name || friendlyName(id);
  document.title = `token ${name} — kascov`;

  /* daa→ms: the payload's own tip anchor first, the grid snapshot's second,
     the raw DAA (never a made-up time) last — same policy as the directory */
  const entry = state.cache[network];
  const toMs = (daa) => {
    if (daa == null) return null;
    if (d.tip_daa != null) {
      return (d.tip_at_ms != null ? d.tip_at_ms : d.generated_at_ms) - (d.tip_daa - daa) * MS_PER_DAA;
    }
    return entry ? daaToMs(daa, entry.data) : null;
  };

  /* deployer-claimed identity leads when the genesis payload carries one —
     canonical name demoted to a subtitle, never hidden: a claim is a claim */
  const claimedTitle = t.claimed_name
    ? `${t.claimed_name}${t.claimed_ticker ? ` ($${t.claimed_ticker})` : ''}`
    : (t.claimed_ticker ? `$${t.claimed_ticker}` : null);
  const header =
    `<header class="detail-head">` +
    `<span class="token-art-wrap" role="img" aria-label="avatar of ${esc(name)}">${avatarSvg(id, 88)}` +
    (t.claimed_image_hash
      ? `<img class="token-art" src="img/${esc(network)}/${esc(id)}" alt="" title="art served from bytes proven against the sha256 committed in this token’s genesis" onload="this.parentElement.classList.add('art-loaded')" onerror="this.remove()">`
      : '') +
    `</span>` +
    `<div class="detail-id"><h1>${esc(claimedTitle || name)}</h1>` +
    `<p class="detail-tags">` +
    tokenStatusBadge(t) +
    (t.template ? `<span class="flag flag-tpl">${esc(t.template)}</span>` : '') +
    (claimedTitle
      ? `<span class="flag flag-claimed" title="this name comes from the token’s genesis transaction payload — written on chain by its deployer at launch. claims aren’t unique; the canonical kascov name is ${esc(name)}">named on chain</span><span class="dim">${esc(name)}</span>`
      : '') +
    `<span class="dim">covenant token on ${esc(net.label)}</span></p>` +
    `<p class="id-chip"><span class="mono">${esc(shortHex(id, 10, 8))}</span>` +
    `<button type="button" class="copy-btn" data-action="copy" data-copy="${esc(id)}" aria-label="copy this token’s covenant id">copy id</button>` +
    `<a class="token-coin-link" href="#/${esc(network)}/c/${esc(id)}">underlying smart coin →</a></p>` +
    `<p class="trade-line" id="token-trade-line" hidden></p>` +
    `</div></header>`;

  /* claimed image: a LINK, never hotlinked — an unpinned URL can change under
     everyone's feet. hash-committed art (claimed_image_hash) is what the
     future verified-image pipeline will render. */
  const imageLine = t.claimed_image
    ? (t.claimed_image_hash
      ? `<p class="dim token-image-line"><span class="flag flag-claimed" title="the artwork shown is served from bytes kascov proved against the SHA-256 committed in this token's genesis — it can never be swapped">✓ art verified from chain</span>` +
        ` <a class="dim" href="${esc(t.claimed_image)}" target="_blank" rel="noopener noreferrer nofollow" title="the deployer's original source URL">source ↗</a></p>`
      : `<p class="dim token-image-line">token art link (unpinned): ` +
        `<a href="${esc(t.claimed_image)}" target="_blank" rel="noopener noreferrer nofollow">${esc(t.claimed_image.length > 48 ? t.claimed_image.slice(0, 45) + '…' : t.claimed_image)} ↗</a>` +
        ` <span title="no hash in the genesis payload — the link's content can change; kascov only renders hash-committed art">unpinned</span></p>`)
    : '';
  const fieldsLine = (t.fields && Object.keys(t.fields).length
    ? `<div class="tokens-fields token-page-fields">${tokenFieldChips(t.fields)}</div>` : '') + imageLine;

  const tiles = [
    ['supply', t.supply], ['minted', t.minted], ['burned', t.burned], ['holders', t.holders],
  ].filter(([, v]) => v != null);
  const stats = tiles.length
    ? `<div class="lane-stats token-stats">` + tiles.map(([label, v]) =>
        `<div class="stat"><span class="stat-n">${esc(fmtTokenAmount(v))}</span><span class="stat-label">${esc(label)}</span></div>`
      ).join('') + `</div>`
    : '';

  /* top holders: balance share against the live supply when the worker gave
     one, else against the sum of what it listed — never a made-up total */
  const balances = Array.isArray(d.balances) ? d.balances : [];
  let holdersSection = '';
  if (balances.length) {
    const listed = balances.reduce((s, b) => s + (typeof b.balance === 'number' ? b.balance : 0), 0);
    const base = typeof t.supply === 'number' && t.supply > 0 ? t.supply : listed;
    const holderRows = balances.map((b) => {
      const share = base > 0 && typeof b.balance === 'number' ? (b.balance / base) * 100 : null;
      const pct = share != null ? (share >= 9.95 ? share.toFixed(0) : share.toFixed(1)) : null;
      return `<tr>` +
        `<td>${tokenOwnerLink(network, b.owner)}</td>` +
        `<td class="tokens-supply">${esc(fmtTokenAmount(b.balance))}</td>` +
        `<td class="token-share">${pct != null
          ? `<span class="lane-track token-share-track"><span class="lane-fill" style="width:${Math.max(Math.min(share, 100), 1).toFixed(1)}%"></span></span> ${esc(pct)}%`
          : '<span class="dim">—</span>'}</td>` +
        `</tr>`;
    }).join('');
    const moreNote = typeof t.holders === 'number' && t.holders > balances.length
      ? ` — the ${esc(fmtInt(balances.length))} largest of ${esc(fmtInt(t.holders))} holders` : '';
    holdersSection = `<section class="token-balances" aria-label="Top holders"><h2>top holders</h2>` +
      `<p class="dim">who holds this token right now${moreNote}</p>` +
      `<div class="tokens-tablewrap"><table class="tokens-table token-balances-table">` +
      `<thead><tr><th>owner</th><th>balance</th><th>share</th></tr></thead>` +
      `<tbody>${holderRows}</tbody></table></div></section>`;
  }

  /* the classified timeline — every event the validator walked, as
     mint/transfer/burn chips with amounts */
  const events = Array.isArray(d.events) ? d.events : [];
  let timelineSection = '';
  if (events.length) {
    const shown = events.slice(0, TOKEN_EVENTS_SHOWN);
    const capNote = events.length > shown.length
      ? `<p class="dim">showing ${esc(fmtInt(shown.length))} of ${esc(fmtInt(events.length))} events.</p>` : '';
    timelineSection = `<section aria-label="Token history"><h2>token history</h2>` +
      `<ol class="timeline">${shown.map((ev) => tokenEventItem(ev, network, toMs)).join('')}</ol>${capNote}</section>`;
  }

  view.innerHTML = back + header + fieldsLine +
    stats +
    tokenValidationHtml(t, d.validation) +
    holdersSection +
    timelineSection;

  /* launchpad trade button: the genesis payload committed art hosted by a
     known launchpad — an on-chain fact that marks where the token launched.
     Filled in async from the curated registry; missing registry, no match,
     or an already-replaced view all mean: no button. */
  if (t.claimed_image) {
    loadLaunchpads().then((pads) => {
      const el = document.getElementById('token-trade-line');
      if (!pads || !el) return;
      const pad = pads.find((p) =>
        Array.isArray(p.image_prefixes) &&
        p.image_prefixes.some((pre) => t.claimed_image.startsWith(pre)) &&
        (!Array.isArray(p.networks) || p.networks.includes(network)));
      if (!pad || !pad.trade_url) return;
      const url = pad.trade_url.replace('{id}', id);
      el.innerHTML = `<a class="trade-btn" href="${esc(url)}" target="_blank" rel="noopener noreferrer" ` +
        `title="this token’s genesis committed art hosted by ${esc(pad.name)} — that marks where it launched. kascov only links out; trading happens on their site">` +
        `trade on ${esc(pad.name)} ↗</a>`;
      el.hidden = false;
    });
  }
}

/* a coin page's "part of token …" backlink — built only from data ALREADY in
   the token caches (the directory, or a token page opened this session), so
   it never costs a coin page an extra request. Old-shape directories (no
   validation verdicts → no token pages on that worker) never link. */
function tokenBacklinkHtml(network, covId) {
  let name = null;
  const dir = tokenPages.get(network);
  if (dir && dir.data && Array.isArray(dir.data.tokens)) {
    const row = dir.data.tokens.find((x) => x.covenant_id === covId);
    if (row && tokenVstatus(row)) name = row.name || friendlyName(covId);
  }
  if (!name) {
    const det = tokenDetails.get(`${network}/${covId}`);
    if (det && det.data && det.data.token) name = det.data.token.name || friendlyName(covId);
  }
  if (!name) return '';
  return `<a class="token-backlink" href="#/${esc(network)}/token/${esc(covId)}">part of token ${esc(name)} →</a>`;
}

/* ---------------------------------------------------------------- tx page */

/* #/{net}/tx/{txid} — one transaction's covenant footprint. Feature-detected
   three ways: a newer worker's enriched answer (events, created/spent cells,
   token actions) renders the whole story; an older worker's bare
   { covenant_id, covenant_ids } renders the coin links; a 404 says honestly
   that kascov never saw it. Born modular like renderTokenPage: renders from
   core/data's loadTxDetail cache and core helpers only. */

const TX_KIND_LABEL = { genesis: 'born', transition: 'moved', burn: 'retired' };
const TX_KIND_CHIP = { genesis: 'tok-mint', transition: 'tok-transfer', burn: 'tok-burn' };

/* a coin as avatar + friendly name, linking to its page with this tx flashed
   in the timeline; the grid's dedup-suffixed name wins when it's loaded */
function txCoinLink(network, covId, txid, fallbackName) {
  const id = String(covId || '');
  const entry = state.cache[network];
  const rec = entry && entry.index.byId.get(id);
  const name = (rec && rec.name) || fallbackName || friendlyName(id);
  return `<a class="token-coin tx-coin" href="#/${esc(network)}/c/${esc(id)}?tx=${esc(txid)}" title="${esc(id)}">` +
    `${avatarSvg(id, 26)} <span class="token-name">${esc(name)}</span></a>`;
}

/* the plain-language headline: "touched N smart coins — X born, Y moved, Z retired" */
function txStoryLine(events, covCount) {
  const n = (k) => events.filter((e) => e && e.kind === k).length;
  const parts = [
    [n('genesis'), 'born'], [n('transition'), 'moved'], [n('burn'), 'retired'],
  ].filter(([c]) => c > 0).map(([c, word]) => `${fmtInt(c)} ${word}`);
  return `this transaction touched ${fmtInt(covCount)} smart coin${covCount === 1 ? '' : 's'}` +
    (parts.length ? ` — ${parts.join(', ')}` : '');
}

function renderTxPage(route) {
  const network = state.network;
  const view = $('#view-tx');
  if (!view) return; /* stale cached index.html */
  const txid = route.id;
  const net = NETWORKS[network];
  document.title = `tx ${shortHex(txid, 10, 8)} — kascov`;
  const back = `<a class="back" href="#/${esc(network)}/explore">← all smart coins</a>`;
  const cached = txDetails.get(`${network}/${txid}`);
  const fresh = cached && Date.now() - cached.at < TOKENS_TTL_MS;
  if (!fresh) {
    /* stale-while-revalidate, same shape as token pages */
    loadTxDetail(network, txid)
      .then(() => {
        const r = parseRoute();
        if (state.network === network && r.view === 'tx' && r.id === txid) renderTxPage(r);
      })
      .catch(() => {
        if (cached) return; /* stale data beats an error card */
        const r = parseRoute();
        if (state.network === network && r.view === 'tx' && r.id === txid) {
          view.innerHTML = back + `<div class="empty-card"><h2>couldn’t look up this transaction.</h2>` +
            `<p class="dim">the lookup didn’t answer — the worker may be busy. it’s not you.</p>` +
            `<button type="button" class="btn" data-action="retry-tx">try again</button></div>`;
        }
      });
  }
  if (!cached) {
    view.innerHTML = back + `<p class="dim">reading this transaction…</p>`;
    return;
  }
  if (cached.missing) {
    view.innerHTML = back + `<div class="empty-card"><h2>not seen by kascov — it may not touch any smart coin.</h2>` +
      `<p class="dim">kascov only watches covenant traffic on ${esc(net.label)}: this could be a regular payment, still unconfirmed, or on the other network — ` +
      `<a href="${esc(txUrl(network, txid))}" target="_blank" rel="noopener noreferrer">check it on the block explorer ↗</a></p></div>`;
    return;
  }
  const d = cached.data || {};

  /* daa→ms: the payload's own tip anchor first (feature-detected), the grid
     snapshot's second, the raw DAA (never a made-up time) last — the same
     policy as token pages */
  const entry = state.cache[network];
  const toMs = (daa) => {
    if (daa == null) return null;
    if (d.tip_daa != null && (d.tip_at_ms != null || d.generated_at_ms != null)) {
      return (d.tip_at_ms != null ? d.tip_at_ms : d.generated_at_ms) - (d.tip_daa - daa) * MS_PER_DAA;
    }
    return entry ? daaToMs(daa, entry.data) : null;
  };

  const ms = toMs(d.accepting_daa);
  const whenBits = [];
  if (ms != null) {
    whenBits.push(`<span title="${esc(utcTitle(ms))}">accepted ${esc(relTime(ms))}</span>`);
    whenBits.push(`<span class="abs-t" title="${esc(utcTitle(ms))}">${esc(absShort(ms))}</span>`);
  }
  if (d.accepting_daa != null && (ms == null || state.nerd)) {
    whenBits.push(`<span class="mono dim">DAA ${esc(fmtInt(d.accepting_daa))}</span>`);
  }
  const header =
    `<header class="page-head tx-head"><h1>transaction</h1>` +
    `<p class="id-chip"><span class="mono" title="${esc(txid)}">${esc(shortHex(txid, 12, 10))}</span>` +
    `<button type="button" class="copy-btn" data-action="copy" data-copy="${esc(txid)}" aria-label="copy this transaction’s full id">copy txid</button>` +
    `<a class="token-coin-link" href="${esc(txUrl(network, txid))}" target="_blank" rel="noopener noreferrer">raw tx ↗</a></p>` +
    `<p class="page-sub dim">a transaction on ${esc(net.label)}` +
    (whenBits.length ? ` · ${whenBits.join(' · ')}` : '') +
    (d.accepting_block ? ` · in block <span class="mono" title="${esc(d.accepting_block)}">${esc(shortHex(String(d.accepting_block), 8, 6))}</span>` : '') +
    `</p></header>`;

  /* covenant events — the enriched shape; older workers only name the coins */
  const events = Array.isArray(d.events) ? d.events : null;
  let story = '';
  let eventsSection = '';
  if (events && events.length) {
    const covIds = [...new Set(events.map((e) => String((e && e.covenant_id) || '')).filter(Boolean))];
    story = `<p class="tx-story">${esc(txStoryLine(events, covIds.length || 1))}</p>`;
    const rows = events.slice().sort((a, b) => ((a && a.seq) || 0) - ((b && b.seq) || 0)).map((ev) => {
      const kind = String((ev && ev.kind) || '');
      const meta = KIND_META[kind] || KIND_META.transition;
      const chip = `<span class="tok-ev-chip ${TX_KIND_CHIP[kind] || 'tok-transfer'}" title="${esc(GLOSSARY[kind] || '')}">${esc(TX_KIND_LABEL[kind] || kind || 'event')}</span>`;
      const idx = ev && ev.tx_index != null
        ? ` <span class="dim mono" title="this transaction’s position within its accepting block">tx #${esc(fmtInt(ev.tx_index))}</span>` : '';
      return `<li class="tl-item ${meta.cls}">` +
        `<span class="tl-icon" title="${esc(GLOSSARY[kind] || '')}">${ICONS[meta.icon]}</span>` +
        `<div class="tl-body"><p class="tl-text">${chip} ${txCoinLink(network, ev.covenant_id, txid, ev.name)}${idx}</p></div></li>`;
    }).join('');
    eventsSection = `<section aria-label="Smart coins touched"><h2>smart coins touched</h2>` +
      `<ol class="timeline tx-events">${rows}</ol></section>`;
  } else {
    /* covenant-links-only fallback (older worker, or an enriched answer with
       no event rows) — still answers the real question: which coins? */
    const ids = [...new Set(
      [...(Array.isArray(d.covenant_ids) ? d.covenant_ids : []), d.covenant_id]
        .filter((x) => typeof x === 'string' && /^[0-9a-fA-F]{64}$/.test(x))
        .map((x) => x.toLowerCase())
    )];
    if (ids.length) {
      story = `<p class="tx-story">${esc(`this transaction touched ${fmtInt(ids.length)} smart coin${ids.length === 1 ? '' : 's'}`)}</p>`;
      eventsSection = `<section aria-label="Smart coins touched"><h2>smart coins touched</h2>` +
        `<ul class="tx-coin-list">${ids.map((id) => `<li>${txCoinLink(network, id, txid)}</li>`).join('')}</ul>` +
        `<p class="dim">this kascov worker doesn’t serve the full per-transaction story yet — each coin’s page carries its timeline.</p></section>`;
    }
  }

  /* token actions — the same conservative wording as token-page timelines:
     amounts are the cells' full decoded state amounts, never net deltas */
  const acts = Array.isArray(d.token_actions) ? d.token_actions : [];
  let tokenSection = '';
  if (acts.length) {
    const rows = acts.map((a) => {
      const tid = String((a && a.token_id) || '');
      const kind = String((a && a.token_kind) || '');
      const meta = TOKEN_EV_META[kind] || TOKEN_EV_META.transfer;
      const name = (a && a.name) || friendlyName(tid);
      const amount = a && a.amount != null
        ? ` — ${kind === 'burn' ? 'burned a cell holding' : 'cell now holds'} <strong>${esc(fmtTokenAmount(a.amount))}</strong>` : '';
      return `<li class="tl-item ${meta.cls}"><span class="tl-icon" aria-hidden="true">${ICONS[meta.icon]}</span>` +
        `<div class="tl-body"><p class="tl-text"><span class="tok-ev-chip ${meta.chip}">${esc(kind || 'event')}</span> ` +
        `<a class="token-coin tx-coin" href="#/${esc(network)}/token/${esc(tid)}" title="${esc(tid)}">${avatarSvg(tid, 26)} <span class="token-name">${esc(name)}</span></a>` +
        `${amount}</p></div></li>`;
    }).join('');
    tokenSection = `<section aria-label="Token actions"><h2>token actions</h2>` +
      `<ol class="timeline tx-events">${rows}</ol></section>`;
  }

  /* the cells this transaction created and consumed */
  const cells = d.cells || {};
  const created = Array.isArray(cells.created) ? cells.created : [];
  const spent = Array.isArray(cells.spent) ? cells.spent : [];
  let cellsSection = '';
  if (created.length || spent.length) {
    const createdRows = created.map((c) => `<li class="tx-cell">` +
      txCoinLink(network, c && c.covenant_id, txid) +
      `<span class="tx-cell-meta"><span>${esc(amountWithUsd((c && c.value) || 0, network))}</span>` +
      `<span class="dim mono">output #${esc(fmtInt((c && c.index) || 0))}</span>` +
      (c && c.template ? `<span class="flag flag-tpl">${esc(c.template)}</span>` : '') +
      `</span></li>`).join('');
    const spentRows = spent.map((c) => {
      const outpoint = c && c.txid
        ? `${shortHex(String(c.txid).toLowerCase(), 8, 6)}:${c.index != null ? c.index : '?'}` : '';
      /* has_witness means the worker captured the unlocking script — the
         real spend can replay through the engine, same flow as coin pages */
      const replay = c && c.has_witness
        ? `<div class="replay-row"><button type="button" class="btn dbg-btn replay-btn" data-action="replay-spend" data-txid="${esc(txid)}">⧉ replay this spend</button>` +
          `<span class="dim replay-hint">the real on-chain spend, opcode by opcode</span>` +
          `<span class="replay-result"></span></div><div class="replay-panel"></div>`
        : '';
      return `<li class="tx-cell">` +
        txCoinLink(network, c && c.covenant_id, txid) +
        `<span class="tx-cell-meta"><span>${esc(amountWithUsd((c && c.value) || 0, network))}</span>` +
        (outpoint ? `<span class="dim mono" title="the outpoint this transaction consumed">${esc(outpoint)}</span>` : '') +
        (c && c.revealed_template ? `<span class="flag flag-tpl" title="the program this cell actually ran — revealed and hash-verified at spend">${esc(c.revealed_template)}</span>` : '') +
        (c && c.role ? `<span class="flag flag-role flag-role-${esc(c.role)}" title="KCC-1 draft role: the leader input carries the covenant id for this transition${c.role === 'delegator' ? '; a delegator rides along under the leader’s id' : ''}">${esc(c.role)}</span>` : '') +
        `</span>${replay}</li>`;
    }).join('');
    cellsSection = `<section class="tx-cells" aria-label="Cells">` +
      (created.length
        ? `<div class="tx-cell-col"><h2>cells created</h2><p class="dim">new covenant state born in this transaction</p><ul class="tx-cell-list">${createdRows}</ul></div>` : '') +
      (spent.length
        ? `<div class="tx-cell-col"><h2>cells spent</h2><p class="dim">covenant state this transaction consumed</p><ul class="tx-cell-list">${spentRows}</ul></div>` : '') +
      `</section>`;
  }

  if (!eventsSection && !tokenSection && !cellsSection) {
    view.innerHTML = back + header +
      `<p class="dim">kascov knows this transaction, but its answer named no smart coins — that shouldn’t happen; please report it.</p>`;
    return;
  }
  view.innerHTML = back + header + story + eventsSection + tokenSection + cellsSection;
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
  /* '#/<network>/lane/<8-hex namespace>' — one KIP-21 lane's dashboard */
  m = path.match(/^#\/(?:(testnet-10|mainnet)\/)?lane\/([0-9a-fA-F]{8})$/);
  if (m) return { view: 'lane', network: m[1] || null, id: m[2].toLowerCase() };
  /* '#/tokens' and '#/<network>/tokens' — the decoded covenant-token directory */
  m = path.match(/^#\/(?:(testnet-10|mainnet)\/)?tokens\/?$/);
  if (m) return { view: 'tokens', network: m[1] || null };
  /* '#/<network>/token/<covenant id>' — one decoded token's page */
  m = path.match(/^#\/(?:(testnet-10|mainnet)\/)?token\/([0-9a-fA-F]{6,64})\/?$/);
  if (m) return { view: 'token', network: m[1] || null, id: m[2].toLowerCase() };
  /* '#/<network>/tx/<txid>' and bare '#/tx/<txid>' — one transaction's page */
  m = path.match(/^#\/(?:(testnet-10|mainnet)\/)?tx\/([0-9a-fA-F]{64})\/?$/);
  if (m) return { view: 'tx', network: m[1] || null, id: m[2].toLowerCase() };
  /* '#/explore' and '#/<network>/explore' — '?galaxy=1' opens the galaxy */
  m = path.match(/^#\/(?:(testnet-10|mainnet)\/)?explore\/?$/);
  if (m) return { view: 'explore', network: m[1] || null, galaxy: params.get('galaxy') === '1' };
  if (/^#\/changelog\/?$/.test(path)) return { view: 'changelog', network: null };
  if (/^#\/decode\/?$/.test(path)) return { view: 'decode', network: null, s: params.get('s') || '' };
  if (/^#\/playground\/?$/.test(path)) return { view: 'decode', network: null, s: params.get('s') || '' };
  if (/^#\/build\/?$/.test(path)) return { view: 'build', network: null };
  /* '#/preflight' — the playground's third mode; network-independent like
     #/decode (the POST's network segment only picks the mass limits) */
  if (/^#\/preflight\/?$/.test(path)) return { view: 'preflight', network: null };
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
  /* a lane namespace exists on any network — keep the page, switch the data */
  if (view === 'lane') return `#/${state.network}/lane/${id}`;
  /* the token directory exists on any network — keep the page, switch the data */
  if (view === 'tokens') return `#/${state.network}/tokens`;
  if (view === 'token') return `#/${state.network}/token/${id}`;
  /* a txid lives on one network only, but the page says so honestly —
     keep it across a switch, like lane pages */
  if (view === 'tx') return `#/${state.network}/tx/${id}`;
  if (view === 'explore') return `#/${state.network}/explore`;
  /* decode/dev/changelog are network-free — switching networks keeps the page (and its query) */
  if (view === 'decode' || view === 'dev' || view === 'build' || view === 'preflight' || view === 'changelog') return location.hash || `#/${view}`;
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
    networkFilterReset();
    closeSuggest();
  }
  if (state.watchNet !== state.network) {
    state.watch = loadWatch(state.network);
    state.watchNet = state.network;
  }
  /* mainnet views probe the $-rate (feature-detected, silent on a miss);
     the first arrival repaints so ≈ $ tails appear without a reload */
  const pricePending = loadPrice();
  if (pricePending) {
    pricePending.then((d) => {
      if (d && state.network === 'mainnet' && !document.hidden) render();
    });
  }
  syncStream();
  syncDetailStream();
  const panel = $('#panel');
  const views = {
    landing: $('#view-landing'),
    explore: $('#view-explore'),
    detail: $('#view-detail'),
    address: $('#view-address'),
    lane: $('#view-lane'),
    tokens: $('#view-tokens'),
    token: $('#view-token'),
    tx: $('#view-tx'),
    decode: $('#view-decode'),
    build: $('#view-build'),
    preflight: $('#view-preflight'),
    dev: $('#view-dev'),
    changelog: $('#view-changelog'),
  };
  /* a stale cached index.html may predate newer views — never crash on them */
  for (const k of Object.keys(views)) if (!views[k]) delete views[k];
  if (!views[route.view]) route.view = 'landing';

  document.querySelectorAll('.network-tab').forEach((b) => {
    b.setAttribute('aria-pressed', String(b.dataset.network === state.network));
  });
  /* decode + build + preflight are the modes of the unified "playground" nav
     entry; a token page lives under the "tokens" nav entry */
  const navFor = route.view === 'decode' || route.view === 'build' || route.view === 'preflight' ? 'playground'
    : route.view === 'token' ? 'tokens' : route.view;
  document.querySelectorAll('.nav-link').forEach((a) => {
    if (a.dataset.nav === navFor) a.setAttribute('aria-current', 'page');
    else a.removeAttribute('aria-current');
  });
  /* search is chain-wide (names, ids, txids, addresses — local + the worker
     endpoint), so it stays reachable on every view; ⌘K focuses it anywhere */
  $('#header-search').hidden = false;

  /* the decoder, dev docs, changelog, address, lane, token and tx pages never
     need a snapshot — don't block them on data (they fetch their own) */
  if ((route.view === 'decode' || route.view === 'dev' || route.view === 'build' || route.view === 'preflight' || route.view === 'address' || route.view === 'lane' || route.view === 'tokens' || route.view === 'token' || route.view === 'tx' || route.view === 'changelog') && views[route.view]) {
    panel.hidden = true;
    for (const [name, el] of Object.entries(views)) el.hidden = name !== route.view;
    views.detail.innerHTML = '';
    if (route.view === 'decode') renderDecode(route);
    else if (route.view === 'address') renderAddress(route);
    else if (route.view === 'lane') renderLane(route);
    else if (route.view === 'tokens') renderTokens();
    else if (route.view === 'token') renderTokenPage(route);
    else if (route.view === 'tx') renderTxPage(route);
    else if (route.view === 'build') renderBuild();
    else if (route.view === 'preflight') renderPreflight();
    else if (route.view === 'changelog') renderChangelog();
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

/* Live refresh: refetch the current network's snapshot and re-render in
   place when it actually changed. The detail view is left alone mid-visit so
   open sections don't collapse; its cache still updates for the next
   navigation.

   The 45s timer is a FALLBACK, not the driver: when the tiny -live.json feed
   works, pollLive's stats-delta triggers the (forced) refetch — so the timer
   only downloads the multi-MB snapshot itself when the live feed is
   unsupported (old worker) or stale. On a quiet chain, per-tab steady-state
   traffic is the 12s live poll alone. */
const REFRESH_MS = 45_000;
function liveFeedCoversRefresh(network) {
  const ls = state.live[network];
  if (!ls || ls.supported === false) return false;
  return ls.at != null && Date.now() - ls.at < 2 * REFRESH_MS;
}
async function refreshSnapshot(force) {
  if (document.visibilityState !== 'visible') return;
  const network = state.network;
  if (!force && liveFeedCoversRefresh(network)) return;
  try {
    const old = state.cache[network];
    /* re-request a window at least as wide as what's already loaded so a
       paginated net doesn't shrink back to one page on refresh; a full-snapshot
       worker ignores the limit and returns everything as before */
    const limit = old ? Math.max(GRID_PAGE, old.data.covenants.length) : GRID_PAGE;
    const data = await fetchGridPage(network, null, null, limit);
    if (old && data.generated_at_ms === old.data.generated_at_ms) return;
    data.__anchor = makeAnchor(data, network);
    const cursor = data.next_after_daa != null ? data.next_after_daa : null;
    state.cache[network] = {
      data, index: buildIndex(data), nextAfterDaa: cursor,
      nextAfterId: data.next_after_id != null ? data.next_after_id : null,
      loadingMore: false,
    };
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
    ls.at = Date.now(); /* freshness stamp — lets the 45s fallback stand down */
    updateLiveBadge();
    const cached = state.cache[network];
    if (cached && live.stats && (
      live.stats.events !== cached.data.stats.events ||
      live.stats.covenants !== cached.data.stats.covenants
    )) {
      refreshSnapshot(true);
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
/* a connection must live this long (or deliver a message) before the retry
   backoff resets — a proxy that accepts and instantly drops would otherwise
   reset it on every open and pin the reconnect loop at the base interval */
const STREAM_SETTLE_MS = 5_000;
const STREAM_POKE_MS = 500;

const stream = { es: null, network: null, retryMs: STREAM_RETRY_BASE_MS, retryTimer: 0, pokeTimer: 0, settleTimer: 0 };

function streamWanted() {
  const view = parseRoute().view;
  return document.visibilityState === 'visible' && (view === 'landing' || view === 'explore');
}

function closeStream() {
  if (stream.es) { stream.es.close(); stream.es = null; }
  stream.network = null;
  clearTimeout(stream.retryTimer);
  clearTimeout(stream.pokeTimer);
  clearTimeout(stream.settleTimer);
  stream.retryTimer = 0;
  stream.pokeTimer = 0;
  stream.settleTimer = 0;
}

function flashLiveBadge() {
  document.querySelectorAll('.live-badge').forEach((el) => {
    el.classList.remove('live-flash');
    void el.offsetWidth; /* restart the animation */
    el.classList.add('live-flash');
  });
}

/* kascov.io is served by our own Caddy now, which streams SSE unbuffered, so
   the stream connects same-origin there. Only the legacy Firebase hosts still
   buffer through the CDN, so those connect cross-origin to the VPS, which sends
   Access-Control-Allow-Origin: * on the stream route. */
const STREAM_ORIGIN = /(^|\.)kascov-explorer\.web\.app$|\.firebaseapp\.com$/.test(location.hostname)
  ? 'https://kascov.io/'
  : '';

/* Open/close/retarget the stream to match the current view + network.
   Idempotent — called from render() and visibilitychange. */
function syncStream() {
  if (typeof EventSource === 'undefined') return;
  if (!streamWanted()) { closeStream(); return; }
  if (stream.es && stream.network === state.network) return;
  closeStream();
  const network = state.network;
  const es = new EventSource(`${STREAM_ORIGIN}data/${network}/stream`);
  stream.es = es;
  stream.network = network;
  es.onopen = () => {
    clearTimeout(stream.settleTimer);
    stream.settleTimer = setTimeout(() => {
      if (stream.es === es) stream.retryMs = STREAM_RETRY_BASE_MS;
    }, STREAM_SETTLE_MS);
  };
  es.onmessage = (e) => {
    stream.retryMs = STREAM_RETRY_BASE_MS;
    /* pending (mempool) frames drive their own section and must not poke the
       confirmed feed; parse defensively so a keepalive can never throw. */
    let msg = null;
    try { msg = JSON.parse(e.data); } catch (_) { /* keepalive / non-JSON */ }
    if (msg && msg.kind === 'pending') {
      notePending(network, msg.txid, msg.covenant_id, msg.tx_kind);
      renderPending(network);
      return;
    }
    if (msg && msg.kind === 'pending_resolved') {
      resolvePending(network, msg.txid, msg.resolution);
      return;
    }
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

/* Per-coin stream: the coin page follows its own subject so the life story
   appends live (M8). New workers filter server-side via ?covenant=; OLD
   workers ignore the param and fan out network-wide — every event is
   re-checked client-side by covenant_id before acting, so both are safe. */
const DETAIL_REFETCH_DEBOUNCE_MS = 600;
const detailStream = { es: null, key: null, retryMs: STREAM_RETRY_BASE_MS, retryTimer: 0, refetchTimer: 0, settleTimer: 0 };

function closeDetailStream() {
  if (detailStream.es) { detailStream.es.close(); detailStream.es = null; }
  detailStream.key = null;
  clearTimeout(detailStream.retryTimer);
  clearTimeout(detailStream.refetchTimer);
  clearTimeout(detailStream.settleTimer);
  detailStream.retryTimer = 0;
  detailStream.refetchTimer = 0;
  detailStream.settleTimer = 0;
}

function syncDetailStream() {
  if (typeof EventSource === 'undefined') return;
  const route = parseRoute();
  const want = document.visibilityState === 'visible' && route.view === 'detail' &&
    route.id && /^[0-9a-f]{64}$/.test(route.id); /* short back-compat ids can't be filtered */
  if (!want) { closeDetailStream(); return; }
  const network = state.network;
  const covId = route.id;
  const key = `${network}|${covId}`;
  if (detailStream.es && detailStream.key === key) return;
  closeDetailStream();
  const es = new EventSource(`${STREAM_ORIGIN}data/${network}/stream?covenant=${covId}`);
  detailStream.es = es;
  detailStream.key = key;
  es.onopen = () => {
    clearTimeout(detailStream.settleTimer);
    detailStream.settleTimer = setTimeout(() => {
      if (detailStream.es === es) detailStream.retryMs = STREAM_RETRY_BASE_MS;
    }, STREAM_SETTLE_MS);
  };
  es.onmessage = (ev) => {
    detailStream.retryMs = STREAM_RETRY_BASE_MS;
    let msg = null;
    try { msg = JSON.parse(ev.data); } catch (err) { return; }
    if (!msg || msg.covenant_id !== covId) return;
    flashLiveBadge();
    /* the coin moved — refetch its story (debounced: one tx can emit
       several events, and testnet bursts shouldn't hammer the endpoint) */
    clearTimeout(detailStream.refetchTimer);
    detailStream.refetchTimer = setTimeout(() => refetchDetail(network, covId), DETAIL_REFETCH_DEBOUNCE_MS);
  };
  es.onerror = () => {
    if (detailStream.es !== es) return;
    closeDetailStream();
    detailStream.retryTimer = setTimeout(syncDetailStream, detailStream.retryMs);
    detailStream.retryMs = Math.min(detailStream.retryMs * 2, STREAM_RETRY_MAX_MS);
  };
}

/* Drop the cached story, refetch it, and repaint in place if the user is
   still on that coin's page (scroll preserved — no jump mid-read). */
function refetchDetail(network, covId) {
  const map = state.details[network];
  if (map) map.delete(covId);
  loadDetail(network, covId)
    .then(() => {
      const route = parseRoute();
      if (state.network !== network || route.view !== 'detail' || route.id !== covId) return;
      const entry = state.cache[network];
      if (!entry) return;
      const y = window.scrollY;
      renderDetail(entry, covId, route.tx, route.program);
      window.scrollTo({ top: y, behavior: 'instant' });
    })
    .catch(() => { /* transient refetch miss — the page keeps its last story */ });
}

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') {
    refreshSnapshot();
    pollLive();
  }
  syncStream();
  syncDetailStream();
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

/* the two timeline chip rows on a coin page share one repaint path */
function setTimelineChip(key, value) {
  state[key] = value;
  const route = parseRoute();
  const entry = state.cache[state.network];
  if (route.view === 'detail' && entry) {
    const y = window.scrollY;
    renderDetail(entry, route.id, route.tx, route.program);
    window.scrollTo({ top: y, behavior: 'instant' });
  }
}

/* The action registry: every delegated click routes data-action → handler.
   A new view brings its own entries instead of another branch in a chain. */
const ACTIONS = {
  'network'(el) {
    const net = el.dataset.network;
    if (!NETWORKS[net] || net === state.network) return;
    state.network = net;
    state.shown = PAGE_SIZE;
    networkFilterReset();
    /* encode the network in the hash so the choice survives reloads and
       shared links land on the right network. A specific coin can't exist on
       the other network, so switching from a detail page lands on that
       network's explorer overview rather than a guaranteed "not found". */
    const route = parseRoute();
    /* a specific coin (or token) can't exist on the other network — land on
       that network's overview instead of a guaranteed "not found" */
    const view = route.view === 'detail' ? 'explore'
      : route.view === 'token' ? 'tokens' : route.view;
    const target = routeHash(view, route.id);
    if (location.hash === target) render();
    else location.hash = target;
  },

  'filter'(el) {
    state.filter = el.dataset.filter;
    state.filterTouched = true;
    state.shown = PAGE_SIZE;
    /* scope to the filter chips — the pulse range pills are .chip too */
    document.querySelectorAll('[data-action="filter"]').forEach((c) => {
      c.setAttribute('aria-pressed', String(c.dataset.filter === state.filter));
    });
    const entry = state.cache[state.network];
    if (entry) renderGrid(entry, state.network);
  },

  'story-kind'(el) {
    const k = el.dataset.kind;
    if (!k || k === state.storyKind) return;
    state.storyKind = k;
    document.querySelectorAll('[data-action="story-kind"]').forEach((c) => {
      c.setAttribute('aria-pressed', String(c.dataset.kind === k));
    });
    repaintStories();
  },

  'tl-kind'(el) {
    setTimelineChip('tlKind', el.dataset.value);
  },

  'tl-label'(el) {
    setTimelineChip('tlLabel', el.dataset.value);
  },

  'usd'(el) {
    const on = toggleUsd();
    document.querySelectorAll('[data-action="usd"]').forEach((b) => {
      b.setAttribute('aria-pressed', String(on));
    });
    const route = parseRoute();
    const entry = state.cache[state.network];
    const y = window.scrollY;
    if (route.view === 'detail' && entry) renderDetail(entry, route.id, route.tx, route.program);
    else if (route.view === 'address') renderAddress(route);
    else if (route.view === 'tokens') renderTokens();
    else if (route.view === 'explore' && entry) renderWatchStrip(entry, state.network);
    window.scrollTo({ top: y, behavior: 'instant' });
  },

  'pulse-range'(el) {
    const r = el.dataset.range;
    if (!ACTIVITY_RANGES.includes(r) || r === state.pulseRange) return;
    state.pulseRange = r;
    try { localStorage.setItem('kascov-pulse-range', r); } catch (err) { /* private mode */ }
    document.querySelectorAll('[data-action="pulse-range"]').forEach((b) => {
      b.setAttribute('aria-pressed', String(b.dataset.range === r));
    });
    hidePulseTip();
    updatePulse(state.network);
  },

  'more'(el) {
    state.shown += PAGE_SIZE;
    const entry = state.cache[state.network];
    if (entry) renderGrid(entry, state.network);
  },

  'more-net'(el) {
    const network = state.network;
    const entry = state.cache[network];
    if (!entry || entry.nextAfterDaa == null || entry.loadingMore) return;
    /* loadMoreGrid flips entry.loadingMore synchronously (before its first
       await), so the immediate re-render shows the button's loading state */
    const pending = loadMoreGrid(network);
    renderGrid(entry, network);
    pending
      .then(() => {
        if (state.network !== network || parseRoute().view !== 'explore') return;
        state.shown += PAGE_SIZE; /* reveal a chunk of the freshly loaded rows */
        renderGrid(entry, network);
      })
      .catch(() => {
        /* loadMoreGrid's finally already cleared loadingMore */
        if (state.network === network && parseRoute().view === 'explore') {
          renderGrid(entry, network);
        }
      });
  },

  'nerd'(el) {
    state.nerd = !state.nerd;
    try { localStorage.setItem('kascov-nerd', state.nerd ? '1' : '0'); } catch (err) { /* ignore */ }
    const route = parseRoute();
    const entry = state.cache[state.network];
    if (route.view === 'detail' && entry) {
      const y = window.scrollY;
      renderDetail(entry, route.id, route.tx, route.program);
      window.scrollTo({ top: y, behavior: 'instant' });
    }
  },

  'watch'(el) {
    const id = el.dataset.id;
    if (!id) return;
    if (state.watch.has(id)) state.watch.delete(id);
    else state.watch.add(id);
    watchGen++; /* invalidates the grid render memo — stars must repaint */
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
  },

  'decode'(el) {
    runDecode(true);
  },

  'decode-load'(el) {
    const hex = DECODE_EXAMPLES[el.dataset.example];
    if (hex) {
      $('#decode-input').value = hex;
      runDecode(true);
    }
  },

  'decode-all'(el) {
    decodeShowAll = !decodeShowAll;
    runDecode(false);
  },

  'preflight-run'(el) {
    /* POST the pasted tx JSON to the worker's preflight — feature-detected:
       a 404 (older worker) degrades to an honest card, a network error to a
       one-liner. The raw pasted text IS the request body; it goes into this
       call and nowhere else. */
    const input = $('#preflight-input');
    const out = $('#preflight-out');
    if (!input || !out) return;
    const raw = input.value.trim();
    if (!raw) {
      out.innerHTML = '<p class="dim">paste a transaction as JSON above — or load the example — then preflight it.</p>';
      return;
    }
    try { JSON.parse(raw); } catch (err) {
      out.innerHTML = `<div class="pf-verdict pf-incomplete"><span class="pf-icon" aria-hidden="true">?</span>` +
        `<strong>that isn&rsquo;t JSON yet</strong><span class="pf-sub">${esc(String(err && err.message || 'unparseable'))}</span></div>`;
      return;
    }
    out.innerHTML = '<p class="dim"><span class="radar" aria-hidden="true"></span>preflighting — budgets, masses, fee, engine…</p>';
    fetch(`data/${state.network}/preflight`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: raw })
      .then((r) => {
        if (r.status === 404) {
          out.innerHTML = `<div class="empty-card"><h2>preflight needs a newer worker.</h2>` +
            `<p class="dim">this kascov worker doesn&rsquo;t answer preflight yet — the decoder and the builder still work, and the <a href="/guide.html#trap" target="_blank" rel="noopener">guide&rsquo;s trap section</a> walks the same checks by hand.</p></div>`;
          return undefined;
        }
        return r.json().catch(() => ({ ok: false, error: `the worker answered ${r.status} without a readable body` }));
      })
      .then((d) => {
        if (d === undefined) return; /* the newer-worker card is already up */
        out.innerHTML = preflightResultHtml(d);
      })
      .catch(() => {
        out.innerHTML = '<div class="pf-verdict pf-incomplete"><span class="pf-icon" aria-hidden="true">?</span><strong>preflight unreachable</strong><span class="pf-sub">network error — nothing was sent anywhere else; try again in a moment</span></div>';
      });
  },

  'preflight-example'(el) {
    /* fill the paste box with the #1073-style broken v1 tx and run it */
    const input = $('#preflight-input');
    if (!input) return;
    input.value = PREFLIGHT_EXAMPLE;
    ACTIONS['preflight-run'](el);
  },

  'pf-goto'(el) {
    /* findings' "input #n" anchors — scroll to the executed row without
       touching location.hash (a bare #pf-input-N would bounce the router) */
    const target = document.getElementById(el.dataset.target || '');
    if (!target) return;
    target.scrollIntoView({ behavior: 'smooth', block: 'center' });
    target.classList.add('pf-hilite');
    setTimeout(() => target.classList.remove('pf-hilite'), 1600);
  },

  'gen-example'(el) {
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
  },

  'gen-toggle'(el) {
    genState = genState && genState.open ? null : { open: true };
    runDecode(false);
    if (genState) {
      const panel = $('#gen-panel');
      if (panel) panel.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  },

  'guided-template'(el) {
    guidedInit(el.dataset.template);
    renderGuidedBuilder();
  },

  'guided-open'(el) {
    /* gallery card → jump into the builder with this template selected */
    guidedInit(el.dataset.template);
    renderGuidedBuilder();
    const sec = document.getElementById('build-guided');
    if (sec) sec.scrollIntoView({ behavior: 'smooth', block: 'start' });
  },

  'guided-download'(el) {
    if (!lastGuidedBuild || !window.kascovGen || !window.kascovGen.buildDeployScript) return;
    const b = lastGuidedBuild;
    const script = window.kascovGen.buildDeployScript(b.hex, b.sompi,
      { template: b.template, date: new Date().toISOString().slice(0, 10) });
    const blob = new Blob([script], { type: 'text/x-shellscript' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `deploy-${b.template.replace('SilverScript · ', '').toLowerCase()}.sh`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 5000);
  },

  'guided-publish'(el) {
    /* register the canonical source against this program's hash — recompiled
       server-side with your args, so any coin hashing to it shows the source */
    const out = document.getElementById('guided-publish-result');
    if (!lastGuidedBuild || !out || !guidedState) return;
    const src = window.kascovGen.SOURCES[lastGuidedBuild.template];
    const info = window.kascovDisasm.skeletonInfo(lastGuidedBuild.template);
    if (!src || !info) { out.innerHTML = ' <span class="compile-err">no source for this template</span>'; return; }
    const args = info.params.map((p) => {
      const check = window.kascovGen.validateField(p.kind, guidedState.values[p.name] || '');
      if (!check.ok) return '';
      if (p.kind === 'pubkey' || p.kind === 'hash32') return '0x' + check.display;
      if (p.kind === 'amount') return String(check.sompi);
      return check.display; /* daa — whole number of ticks */
    });
    out.innerHTML = ' <span class="dim">publishing…</span>';
    fetch(`data/${state.network}/publish`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ source: src, args }) })
      .then((r) => r.json())
      .then((d) => {
        out.innerHTML = d && d.ok
          ? ` <span class="compile-ok">✓ published — coins hashing <code class="mono">${esc(d.hash.slice(0, 12))}…</code> now show this source</span>`
          : ` <span class="compile-err">${esc((d && d.error) || 'publish failed')}</span>`;
      })
      .catch(() => { out.innerHTML = ' <span class="compile-err">publish unavailable — is the kascov node reachable?</span>'; });
  },

  'guided-connect-wallet'(el) {
    /* KasWare connect — read-only: we only surface the address + a faucet link.
       No signing here; the custodial worker deploys. Guarded + graceful. */
    const strip = document.getElementById('guided-wallet');
    if (!hasKasware()) { if (strip) strip.innerHTML = guidedWalletHtml(); return; }
    if (strip) strip.innerHTML = '<p class="guided-wallet-note dim">connecting…</p>';
    Promise.resolve()
      .then(() => window.kasware.requestAccounts())
      .then((accounts) => {
        const addr = Array.isArray(accounts) ? accounts[0] : (accounts && accounts[0]);
        guidedWallet.address = addr || null;
        if (strip) strip.innerHTML = guidedWalletHtml();
      })
      .catch((err) => {
        if (strip) strip.innerHTML = `<p class="guided-wallet-note dim">wallet not connected${err && err.message ? ' — ' + esc(String(err.message)) : ''}. you can still deploy below.</p>`;
      });
  },

  'guided-deploy'(el) {
    /* one-click custodial deploy. Endpoint is gated OFF by default (404 unless
       the operator set KASCOV_DEPLOY_KEY), so any 404 / network error falls
       back to the existing download-deploy.sh + CLI path. */
    const out = document.getElementById('guided-deploy-result');
    if (!out) return;
    if (!lastGuidedBuild || !lastGuidedBuild.hex) {
      out.innerHTML = '<p class="guided-deploy-err">nothing to deploy yet — fill in the fields above so a byte-perfect script is produced.</p>';
      return;
    }
    const fallback = (why) => {
      out.innerHTML = `<p class="guided-deploy-fallback">one-click deploy isn’t enabled on this worker — use <strong>⬇ download deploy.sh</strong> above and run it locally.` +
        (why ? ` <span class="dim">(${esc(why)})</span>` : '') + `</p>`;
    };
    const b = lastGuidedBuild;
    out.innerHTML = '<p class="guided-deploy-pending dim">deploying on testnet-10…</p>';
    fetch('data/testnet-10/deploy', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ program_hex: b.hex, value: Number(b.sompi) }),
    })
      .then((r) => {
        if (r.status === 404) { fallback('endpoint disabled'); return null; }
        if (!r.ok) { fallback('worker returned ' + r.status); return null; }
        return r.json().catch(() => null);
      })
      .then((d) => {
        if (d === null) return; /* fallback already shown, or gated off */
        if (d && d.ok && d.covenant_id) {
          const net = String(d.network || 'testnet-10');
          const cid = String(d.covenant_id).toLowerCase();
          freshDeploys.add(cid);
          /* the coin page can 404 for minutes during reorg churn — hold the
             link back until the indexer actually serves it (awaitIndexed) */
          out.innerHTML = `<div class="guided-deploy-ok">` +
            `<p>✓ deployed on ${esc(net)} — your covenant is live.</p>` +
            `<p class="guided-deploy-wait dim"><span class="radar" aria-hidden="true"></span>` +
            `born — waiting for the indexer to catch it through testnet reorgs…</p>` +
            `</div>`;
          awaitIndexed(net, cid, out);
        } else {
          out.innerHTML = `<p class="guided-deploy-err">deploy failed${d && d.error ? ' — ' + esc(String(d.error)) : ''}. the download-deploy.sh path above still works.</p>`;
        }
      })
      .catch(() => fallback('network error'));
  },

  'copy-block'(el) {
    const pre = el.closest('.gen-block');
    const text = pre && pre.querySelector('pre') ? pre.querySelector('pre').textContent : '';
    copyToClipboard(text).then((ok) => {
      const orig = el.textContent;
      el.textContent = ok ? 'copied!' : 'copy failed';
      el.classList.add('copied');
      setTimeout(() => { el.textContent = orig; el.classList.remove('copied'); }, 1400);
    });
  },

  'decode-download'(el) {
    downloadDisassembly();
  },

  'decode-input-toggle'(el) {
    const input = $('#decode-input');
    const collapsed = input.classList.toggle('collapsed');
    el.textContent = collapsed ? 'expand input ▼' : 'collapse input ▲';
  },

  'story-all'(el) {
    state.storyAll = !state.storyAll;
    const entry = state.cache[state.network];
    const route = parseRoute();
    if (entry && route.view === 'detail') {
      const y = window.scrollY;
      renderDetail(entry, route.id);
      window.scrollTo({ top: y, behavior: 'instant' });
    }
  },

  'utxo-all'(el) {
    state.utxoAll = !state.utxoAll;
    const entry = state.cache[state.network];
    const route = parseRoute();
    if (entry && route.view === 'detail') {
      const y = window.scrollY;
      renderDetail(entry, route.id);
      window.scrollTo({ top: y, behavior: 'instant' });
    }
  },

  'decode-share'(el) {
    runDecode(true);
    copyToClipboard(location.href).then((ok) => {
      const original = el.dataset.label || el.textContent;
      el.dataset.label = original;
      el.textContent = ok ? 'link copied!' : 'copy failed';
      setTimeout(() => { el.textContent = el.dataset.label; }, 1400);
    });
  },

  'copy'(el) {
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
  },

  'retry'(el) {
    delete state.cache[state.network];
    render();
  },

  'reveal-check'(el) {
    const box = el.closest('.reveal-preview');
    const val = box && box.querySelector('.reveal-input') ? box.querySelector('.reveal-input').value.trim().replace(/^0x/, '') : '';
    if (val) {
      const route = parseRoute();
      if (route.view === 'detail') {
        location.hash = `#/${state.network}/c/${route.id}?program=${val}`;
      }
    }
  },

  'compile-run'(el) {
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
  },

  'compile-example'(el) {
    const src = $('#compiler-src');
    const args = $('#compiler-args');
    if (src) src.value = SILVERSCRIPT_EXAMPLE.source;
    if (args) args.value = SILVERSCRIPT_EXAMPLE.args;
    const out = $('#compiler-result');
    if (out) out.innerHTML = '';
  },

  'compile-template'(el) {
    loadTemplate(el.dataset.template);
  },

  'compile-publish'(el) {
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
  },

  'zk-verify'(el) {
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
  },

  'galaxy-status'(el) {
    const box = el.closest('.galaxy-chips');
    if (box) box.querySelectorAll('.chip').forEach((c) => {
      c.classList.toggle('chip-on', c === el);
      c.setAttribute('aria-pressed', String(c === el));
    });
    if (galaxyCtrl) galaxyCtrl.setFilter({ status: el.dataset.val });
  },

  'galaxy-color'(el) {
    const box = el.closest('.galaxy-chips');
    if (box) box.querySelectorAll('.chip').forEach((c) => {
      c.classList.toggle('chip-on', c === el);
      c.setAttribute('aria-pressed', String(c === el));
    });
    if (galaxyCtrl) galaxyCtrl.setColorMode(el.dataset.val);
  },

  'sim-run'(el) {
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
  },

  'sim-trace'(el) {
    openSimTrace(lastSimTrace);
  },

  'replay-spend'(el) {
    /* replay a REAL on-chain spend through the engine (worker /debug/{txid});
       feature-detected — any failure degrades to a one-line note */
    const row = el.closest('.replay-row');
    const out = row && row.querySelector('.replay-result');
    const panel = row && row.nextElementSibling && row.nextElementSibling.classList.contains('replay-panel')
      ? row.nextElementSibling : null;
    const txid = el.dataset.txid || '';
    if (!row || !out || !panel || !/^[0-9a-f]{64}$/i.test(txid)) return;
    out.innerHTML = '<span class="dim">replaying through the script engine…</span>';
    fetch(`data/${state.network}/debug/${txid.toLowerCase()}`, { cache: 'no-cache' })
      .then((r) => { if (!r.ok) throw new Error(String(r.status)); return r.json(); })
      .then((d) => {
        if (d && d.ok && d.trace && d.trace.length) {
          out.innerHTML = '';
          openRealTrace(d, panel);
        } else {
          out.innerHTML = `<span class="dim">can’t replay — ${esc((d && (d.reason || d.verdict)) || 'no trace came back for this spend')}</span>`;
        }
      })
      .catch(() => {
        out.innerHTML = '<span class="dim">replay unavailable — this needs a newer kascov worker</span>';
      });
  },

  'dbg-open'(el) {
    openDebugger(el.dataset.prog || '');
  },

  'dbg-prev'(el) {
    dbgStep(-1);
  },

  'dbg-next'(el) {
    dbgStep(1);
  },

  'dbg-seek'(el) {
    dbgStep(0, parseInt(el.dataset.i, 10));
  },

  'dbg-close'(el) {
    dbg = null;
    /* clear whichever host the debugger was drawn into (a replay panel on the
       coin page, or the decode page's #dbg-panel) */
    const host = dbgHost && dbgHost.isConnected ? dbgHost : document.getElementById('dbg-panel');
    dbgHost = null;
    if (host) host.innerHTML = '';
  },

  'retry-detail'(el) {
    render(); /* the detail map has no entry for a failed fetch — this refetches */
  },

  'retry-addr'(el) {
    render(); /* failed lookups are never cached — this refetches */
  },

  'retry-lane'(el) {
    render(); /* failed lane loads are never cached — this refetches */
  },

  'retry-tokens'(el) {
    render(); /* failed token loads are never cached — this refetches */
  },

  'retry-token'(el) {
    render(); /* failed token-page loads are never cached — this refetches */
  },

  'retry-tx'(el) {
    render(); /* failed tx-page loads are never cached — this refetches */
  },
};

document.addEventListener('click', (e) => {
  const el = e.target.closest('[data-action]');
  if (!el) return;
  const action = el.dataset.action;
  if (Object.prototype.hasOwnProperty.call(ACTIONS, action)) ACTIONS[action](el, e);
});

/* the debugger scrubber (range input) — separate from click delegation */
document.addEventListener('input', (e) => {
  const el = e.target.closest('[data-action="dbg-slider"]');
  if (el) dbgStep(0, parseInt(el.value, 10));
});

/* render the galaxy lazily when its section is expanded (toggle doesn’t
   bubble, so capture) */
document.addEventListener('toggle', (e) => {
  const t = e.target;
  if (!t || !t.id) return;
  if (t.id === 'section-galaxy' && t.open) renderGalaxy();
  /* promoted sections: remember the user's last choice (promoteSection
     replays it on later visits) */
  const seenKey = t.id === 'section-galaxy' ? 'kascov-galaxy-seen'
    : t.id === 'section-analytics' ? 'kascov-analytics-seen' : null;
  if (seenKey) {
    try { localStorage.setItem(seenKey, t.open ? 'open' : 'closed'); } catch (err) { /* private mode */ }
  }
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

/* Ask the worker whether a transaction touched a smart coin; navigate to its
   tx page on a hit (warming the page's cache with the answer we already
   hold), remember the miss so the grid can answer honestly. */
async function resolveTxQuery(network, txid, input) {
  if (state.txLookup[txid]) return; /* already pending or answered */
  state.txLookup[txid] = 'pending';
  try {
    const res = await fetch(`data/${network}/tx/${txid}.json`, { cache: 'no-cache' });
    if (res.ok) {
      const data = await res.json();
      txDetails.set(`${network}/${txid}`, { data, at: Date.now() });
      delete state.txLookup[txid];
      if (state.network === network && state.query === txid) {
        if (input) input.value = '';
        state.query = '';
        closeSuggest();
        location.hash = `#/${network}/tx/${txid}`;
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

/* guided builder fields (#/build): live output refresh while typing (caret-safe
   — only #guided-out re-renders), full rebuild on blur so field hints settle */
document.addEventListener('input', (e) => {
  const field = e.target.closest && e.target.closest('#guided-builder') ? e.target : null;
  if (!field || !guidedState || !field.dataset.guidedField) return;
  const key = field.dataset.guidedField;
  if (key === '__value') guidedState.coinValue = field.value;
  else guidedState.values[key] = field.value;
  const out = $('#guided-out');
  if (out) out.innerHTML = guidedOutputsHtml();
});
document.addEventListener('change', (e) => {
  if (e.target.closest && e.target.closest('#guided-builder') && guidedState &&
      e.target.dataset && e.target.dataset.guidedField) {
    renderGuidedBuilder();
  }
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

/* galaxy search — center + highlight the matching coin */
const galaxySearch = $('#galaxy-search');
let galaxySearchTimer = 0;
if (galaxySearch) {
  galaxySearch.addEventListener('input', () => {
    clearTimeout(galaxySearchTimer);
    galaxySearchTimer = setTimeout(() => {
      if (galaxyCtrl) galaxyCtrl.search(galaxySearch.value);
    }, 200);
  });
}

/* keep the galaxy sized to its container */
window.addEventListener('resize', () => { if (galaxyCtrl) galaxyCtrl.resize(); });

window.addEventListener('hashchange', render);

/* fill static guide icons */
document.querySelectorAll('.guide-icon').forEach((el) => {
  el.innerHTML = ICONS[el.dataset.icon] || '';
});

/* ------------------------------------------------ first-visit story tour */

/* Eight steps over the LIVE page: watch → understand → see it whole → touch.
   Vanilla, dismissible everywhere, strictly opt-in: first visits land on the
   front page with a visible invitation (the hero button below), and the tour
   itself only starts from ?tour=1 links. */
const tour = { step: -1, el: null };

const TOUR_STEPS = [
  {
    target: () => document.querySelector('.live-badge'),
    /* the opening line matches what the badge actually says right now —
       promising "green means live" while it shows "catching up" reads broken */
    text: () => {
      const b = document.querySelector('.live-badge');
      if (b && b.classList.contains('live-on')) {
        return 'kascov is watching the Kaspa chain <strong>right now</strong> — green means it saw the tip seconds ago.';
      }
      if (b && (b.classList.contains('live-lag') || b.classList.contains('live-stale'))) {
        return 'kascov watches the Kaspa chain live — right now it’s <strong>catching up</strong>, replaying every block in order until it reaches the tip.';
      }
      return 'kascov keeps a fresh <strong>snapshot</strong> of the Kaspa chain — it re-checks for new life every few seconds.';
    },
  },
  {
    target: () => document.querySelector('#story-list .story'),
    text: 'smart coins are <strong>born</strong>, they <strong>move</strong>, they <strong>retire</strong>. these are real events, moments old.',
  },
  {
    /* opening the section is part of targeting — the canvas only paints once
       the <details> is open, and a network without apps skips the step */
    target: () => {
      const sec = $('#section-galaxy');
      if (!sec || sec.hidden) return null;
      sec.open = true;
      return $('#galaxy-canvas');
    },
    text: 'the <strong>galaxy</strong> — every app on the network, one map. each dot is a smart coin; scroll to zoom, click a dot to open it.',
  },
  {
    target: () => {
      const sec = $('#section-analytics');
      if (!sec || sec.hidden) return null;
      sec.open = true;
      return sec;
    },
    text: 'the same feeds behind this page, drawn as <strong>charts</strong> — births against retirements, survival, contract types.',
  },
  {
    target: () => document.querySelector('#story-list .story'),
    text: 'let’s follow this one…',
    enter: (el) => { if (el) location.hash = el.getAttribute('href'); },
    advanceOnRender: true, /* move on when the coin page has actually painted */
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
    target: () => document.querySelector('.nav-link[data-nav="playground"]'),
    text: 'and the playground: <strong>build a covenant without code</strong> — fill in the blanks, deploy in one click on the testnet. enjoy the telescope 🔭',
    last: true,
    cta: 'try the builder →',
  },
];

function endTour(finished) {
  if (tour.el) tour.el.remove();
  tour.el = null;
  tour.step = -1;
  try { localStorage.setItem('kascov-tour', 'done'); } catch (e) { /* private mode */ }
  updateTourOffer(); /* the hero invitation retires with the flag */
  if (finished) location.hash = '#/build';
}

/* the landing hero's opt-in invitation — visible until the tour has been
   taken (or skipped) once; the tour stamps the same flag either way */
function tourSeen() {
  try { return localStorage.getItem('kascov-tour') === 'done'; }
  catch (e) { return true; } /* private mode: never nag */
}

function updateTourOffer() {
  const hero = document.querySelector('#view-landing .hero');
  if (!hero) return;
  const existing = document.getElementById('hero-tour');
  if (tourSeen()) { if (existing) existing.remove(); return; }
  if (existing) {
    /* keep the link on the network the reader is actually looking at */
    const a = existing.querySelector('a');
    if (a) a.setAttribute('href', `#/${state.network}/explore?tour=1`);
    return;
  }
  const p = document.createElement('p');
  p.id = 'hero-tour';
  p.className = 'hero-tour';
  p.innerHTML =
    `<a class="btn btn-accent hero-tour-btn" href="#/${esc(state.network)}/explore?tour=1">▶ take the 30-second tour</a>` +
    `<span class="dim">new here? eight quick steps — skippable any time</span>`;
  hero.appendChild(p);
}

function showTourStep(i) {
  const step = TOUR_STEPS[i];
  if (!step) { endTour(false); return; }
  const target = step.target();
  /* getClientRects catches elements that exist but sit inside a hidden view
     (every section of every view stays in the DOM) — those aren't targets */
  if (!target || !target.getClientRects().length) {
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
  /* copy can depend on live page state (step 1 reads the badge) */
  const text = typeof step.text === 'function' ? step.text() : step.text;
  tour.el.innerHTML =
    `<div class="tour-spot" style="top:${r.top + window.scrollY - 6}px;left:${Math.max(0, r.left - 6)}px;width:${Math.min(r.width + 12, window.innerWidth)}px;height:${r.height + 12}px"></div>` +
    `<div class="tour-card" style="top:${cardTop}px;left:${Math.max(12, Math.min(r.left, window.innerWidth - 348))}px">` +
    `<p class="tour-text">${text}</p>` +
    `<div class="tour-nav">` +
    `<span class="dim tour-count">${i + 1}/${TOUR_STEPS.length}</span>` +
    (i > 0 ? `<button type="button" class="btn tour-skip tour-back" data-tour="back">← back</button>` : '') +
    `<button type="button" class="btn tour-skip" data-tour="skip">skip</button>` +
    `<button type="button" class="btn btn-accent" data-tour="${step.last ? 'finish' : 'next'}">${step.last ? (step.cta || 'finish →') : 'next →'}</button>` +
    `</div></div>`;
  if (step.enter) {
    step.enter(target);
    if (step.advanceOnRender) {
      /* advance once the NEXT step's target has actually painted (the coin
         page fills in when its detail fetch lands) — not on a blind timer.
         A short floor keeps this card readable; a ceiling keeps the tour
         moving even if the render never completes. */
      const started = Date.now();
      const next = TOUR_STEPS[i + 1];
      const poll = () => {
        if (tour.step !== i) return; /* dismissed, skipped, or moved on */
        const waited = Date.now() - started;
        if ((next && next.target() && waited >= 600) || waited > 8000) {
          showTourStep(i + 1);
          return;
        }
        setTimeout(poll, 150);
      };
      setTimeout(poll, 150);
    }
  }
}

/* step back — transition steps (enter) replay a navigation, so reverse skips
   over them; a step that lives on the explore page walks the view back there
   first and waits for it to paint */
function tourBack() {
  let i = tour.step - 1;
  while (i >= 0 && TOUR_STEPS[i].enter) i--;
  if (i < 0) return;
  const step = TOUR_STEPS[i];
  const onScreen = () => {
    const t = step.target();
    return t && t.getClientRects().length > 0;
  };
  if (onScreen()) { showTourStep(i); return; }
  location.hash = `#/${state.network}/explore`;
  let tries = 0;
  const poll = () => {
    if (tour.step < 0) return; /* dismissed while walking back */
    if (onScreen()) { showTourStep(i); return; }
    if (++tries < 40) setTimeout(poll, 250);
  };
  setTimeout(poll, 250);
}

function maybeStartTour() {
  /* strictly opt-in: only a ?tour=1 link (the hero invitation, the guide's
     tour line, shared links) starts it. A first visit lands on the front
     page and keeps its scroll — no hijacking into the explorer. */
  const forced = /[?&]tour=1/.test(location.hash) || /[?&]tour=1/.test(location.search);
  if (!forced) return;
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
  else if (act === 'back') tourBack();
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

networkFilterReset(); /* testnet-10 boots with the traffic generator hidden */
render();
pollLive();
setTimeout(maybeStartTour, 900);


