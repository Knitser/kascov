/* kascov — the whole-network "galaxy" App Graph. One zoomable/pannable map of
   every multi-contract app (union-find clusters of covenants that moved
   together). Positions + weighted edges are precomputed by the worker
   (`/data/<net>/galaxy.json`), so the browser never runs a force sim: we just
   pan/zoom a static world with viewport culling and level-of-detail. Pure
   Canvas 2D, no deps. Redraws only on interaction (idle CPU = 0).

   Data shape (index-addressed, integer world coords):
     { bounds:{minx,miny,w,h}, templates:[name…],
       apps:[{cx,cy,r,size,t,alive}…],            // one super-node per cluster
       nodes:[{id,t,s,x,y,r,a}…],                 // t=template idx (-1=none), s=1 active
       edges:[[i,j,w]…] }                          // pairwise, weighted
   Columnar shape (?fmt=2, feature-detected by `ids`): nodes[] is replaced by
   parallel arrays ids/nx/ny/nr/nt/ns/na that map 1:1 onto our typed arrays,
   and apps[] by acx/acy/ar/asz/at/aalive.
   Either shape may be the reduced core tier (tier:'core', big clusters only);
   load(full, {preserveView:true}) hot-swaps the full set without moving the
   camera (the worker keeps positions + bounds identical across tiers).

   Rendering ("deep space, alive"): overview, constellation and detail LODs
   cross-fade instead of dumping the full coin set onto the first zoom step.
   A pre-rendered vignette + two parallax starfield layers ground the scene as
   css background layers on the canvas element (compositor-blended: zero
   per-frame raster cost). Overview/constellation views render a deterministic
   sparse sample of app aggregates; detail views query a compact spatial index
   and draw only visible nodes as cached orbs (flat discs for dense frames).
   Labels are collision-checked and viewport-contained. A gentle twinkle loop
   runs ONLY while the pointer is over the canvas (and never under
   prefers-reduced-motion) — the default idle state stays event-driven with
   zero CPU. */
(() => {
  'use strict';
  const TAU = Math.PI * 2;
  const PALETTE = ['#5be49b', '#8ab4ff', '#ffb067', '#c792ea', '#f78c6c', '#49eacb', '#89ddff', '#e4c05b'];
  const UNKNOWN_COLOR = 'rgba(150,160,180,0.85)';
  const ACTIVE_COLOR = '#5be49b';
  const BURNED_COLOR = 'rgba(130,140,160,0.5)';
  const DETAIL_START = 13;
  const DETAIL_END = 17;
  const EDGE_DETAIL = 22;
  const LABEL_DETAIL = 24;
  const APP_FOCUS_MIN = 6;
  const APP_FOCUS_MAX = 15;
  // far-mode batched dot tints: three dim palette-adjacent hues (teal / blue
  // / warm) so the tiny-app starfield has depth without per-dot styles
  const DOT_TINTS = ['rgba(128,190,172,0.55)', 'rgba(138,166,205,0.55)', 'rgba(196,176,150,0.5)'];

  // ---- deterministic helpers (no Math.random anywhere per frame) ----
  function hashStr(s) {
    let h = 2166136261;
    for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619); }
    return h >>> 0;
  }
  function mulberry32(seed) {
    let a = seed >>> 0;
    return function () {
      a |= 0; a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }
  function parseColor(c) {
    if (typeof c === 'string') {
      if (c[0] === '#' && c.length === 7) {
        return [parseInt(c.slice(1, 3), 16), parseInt(c.slice(3, 5), 16), parseInt(c.slice(5, 7), 16), 1];
      }
      const m = /rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)(?:\s*,\s*([\d.]+))?\s*\)/.exec(c);
      if (m) return [+m[1], +m[2], +m[3], m[4] === undefined ? 1 : +m[4]];
    }
    return [150, 160, 180, 1];
  }
  const mixTo = (a, b, f) => Math.round(a + (b - a) * f);
  function rgba(r, g, b, a) { return `rgba(${r},${g},${b},${a})`; }
  const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
  const smoothstep = (lo, hi, v) => {
    const t = clamp((v - lo) / (hi - lo), 0, 1);
    return t * t * (3 - 2 * t);
  };
  const hashIndex = (i) => {
    let h = Math.imul((i + 1) ^ 0x9e3779b9, 0x85ebca6b);
    h ^= h >>> 13;
    return Math.imul(h, 0xc2b2ae35) >>> 0;
  };

  // A cluster click is a first-stage focus, not a teleport into maximum
  // detail. Keeping it inside the aggregate/detail cross-fade preserves the
  // surrounding galaxy (especially when the cluster sits near an outer edge);
  // the wheel/pinch gesture can then continue naturally into coin-level LOD.
  function appFocusScale(appRadius, fitScale, viewportW, viewportH, currentScale) {
    const extent = Math.max(appRadius, 8);
    const fit = (Math.min(viewportW, viewportH) * 0.4) / extent;
    const desired = clamp(fit, fitScale * APP_FOCUS_MIN, fitScale * APP_FOCUS_MAX);
    return Math.max(currentScale, desired);
  }
  const hasIdentity = (id) => typeof id === 'string' && id.length > 0;
  function visualIds(coreIds, total) {
    const out = new Array(total).fill('');
    for (let i = 0; i < Math.min(coreIds.length, total); i++) out[i] = coreIds[i];
    return out;
  }

  // geometric radius buckets (~±6% quantization) so sprite cache stays small
  const ORB_BUCKETS = [];
  for (let b = 0; b < 40; b++) ORB_BUCKETS.push(1.5 * Math.pow(2, b / 6));
  function orbBucket(r) {
    const bi = Math.round(Math.log2(r / 1.5) * 6);
    return bi < 0 ? 0 : bi > 39 ? 39 : bi;
  }
  const NEB_BUCKETS = [];
  for (let b = 0; b < 36; b++) NEB_BUCKETS.push(2 * Math.pow(2, b / 5));
  function nebBucket(r) {
    const bi = Math.round(Math.log2(r / 2) * 5);
    return bi < 0 ? 0 : bi > 35 ? 35 : bi;
  }

  function create(canvas, opts) {
    opts = opts || {};
    const friendlyName = opts.friendlyName || ((id) => id.slice(0, 8));
    const templateColorFn = opts.templateColor || null;
    const onPickCoin = opts.onPickCoin || (() => {});

    const ctx = canvas.getContext('2d');
    const host = canvas.parentElement;
    if (host && getComputedStyle(host).position === 'static') host.style.position = 'relative';

    // floating DOM tooltip (labels/hover) — not drawn on the canvas
    const tip = document.createElement('div');
    tip.className = 'galaxy-tip';
    tip.setAttribute('aria-hidden', 'true');
    if (host) host.appendChild(tip);

    // ---- state ----
    let N = 0;
    let nx, ny, nr, nt, ns, na, ids; // typed arrays + id string list
    let identitiesLoaded = 0;
    let apps = [];
    let edges = null; // flat Int32Array [i,j,w, i,j,w, …]
    let tplColors = [];
    let templates = [];
    let bounds = { minx: 0, miny: 0, w: 1, h: 1 };
    let netName = '';
    let colorMode = 'template'; // 'template' | 'status'
    let filter = { status: 'all', minSize: 2, template: null };
    let visible = null; // Uint8Array — passes the filter

    let dpr = Math.min(window.devicePixelRatio || 1, 2);
    let W = 0, H = 0;
    let scale = 1, fitScale = 1, panX = 0, panY = 0;
    let hoverNode = -1, hoverApp = -1, focusNode = -1;
    let dragging = false, dragMoved = false, lastX = 0, lastY = 0;
    const activePointers = new Map();
    let pinch = null;
    let gestureWasPinch = false;
    let rafPending = false;
    let destroyed = false; // a destroyed controller must never paint again
                           // (a queued rAF draw would smear its stale view
                           // over whatever now owns the canvas)
    let anim = null; // {t0, dur, from:{scale,panX,panY}, to:{…}}
    let animRaf = 0;
    let wheelTarget = null;
    let wheelRaf = 0;
    let wheelLast = 0;
    let wheelTickAt = 0;
    let nodeGrid = null, appGrid = null;
    let maxAppRadius = 0;
    const viewNodes = [];
    const viewApps = [];
    const hitNodes = [];
    const hitApps = [];
    let frameStats = { lod: 'empty', nodes: 0, apps: 0, labels: 0 };

    // ---- atmosphere + sprite caches ----
    // The vignette + parallax starfields live on the canvas ELEMENT as css
    // background layers (blob urls, rebuilt only on resize/network change):
    // the compositor blends them for free, so the per-frame canvas raster
    // pays nothing for atmosphere — a 3-layer full-viewport blit per frame
    // was the single biggest fixed cost in the draw budget. Parallax is
    // just background-position (starfields never zoom-scale, as before).
    let bgVignette = null;            // opaque vignette, viewport-sized (device px)
    let starFar = null, starNear = null; // parallax star layers (device px, wrap-safe)
    let bgGen = 0;                    // guards stale async blob encodes
    let bgUrls = [];                  // applied blob: urls (revoked on rebuild)
    const orbCache = new Map();       // int key -> offscreen orb sprite
    const nebCache = new Map();       // int key -> offscreen nebula sprite
    const reduceMotion = (typeof window.matchMedia === 'function')
      ? window.matchMedia('(prefers-reduced-motion: reduce)')
      : { matches: false };
    let ambientOn = false, ambientRaf = 0, lastAmbient = 0;

    // ---- data load ----
    function load(d, o) {
      // preserveView (tier hot-swap): keep the current pan/zoom — bounds are
      // identical across tiers, so the fit wouldn't change anyway, and we
      // must not yank the camera mid-interaction. Hover indices may not
      // survive the swap, so they reset.
      const keepView = !!(o && o.preserveView) && (N > 0 || apps.length > 0);
      data_reset(d);
      if (keepView) {
        hoverNode = -1;
        hoverApp = -1;
        focusNode = -1;
        hideTip();
        requestDraw();
      } else {
        resize(); // sizes the backing store, computes fit, draws
      }
    }

    function loadVisual(d) {
      if (!d || !d.nx || !d.ny || !d.nr || !d.nt || !d.ns || !d.na) return false;
      const nextN = d.nx.length;
      if (!nextN) return false;

      // Core identities are a stable prefix because apps are sorted largest
      // first. Extend that prefix with identity-free visual nodes; the later
      // full hot-swap replaces every placeholder without moving the camera.
      const coreIds = ids || [];
      N = nextN;
      ids = visualIds(coreIds, nextN);
      nx = Float32Array.from(d.nx);
      ny = Float32Array.from(d.ny);
      nr = Float32Array.from(d.nr);
      nt = Int16Array.from(d.nt);
      ns = Uint8Array.from(d.ns);
      na = Int32Array.from(d.na);

      const es = d.edges || [];
      edges = new Int32Array(es.length * 3);
      for (let k = 0; k < es.length; k++) {
        edges[k * 3] = es[k][0];
        edges[k * 3 + 1] = es[k][1];
        edges[k * 3 + 2] = es[k][2] || 1;
      }
      nodeGrid = buildSpatialGrid(N, (i) => nx[i], (i) => ny[i]);
      applyFilter();
      hoverNode = -1;
      hoverApp = -1;
      focusNode = -1;
      hideTip();
      requestDraw();
      return true;
    }

    function data_reset(d) {
      templates = d.templates || [];
      bounds = d.bounds || { minx: 0, miny: 0, w: 1, h: 1 };
      const newNet = d.network || '';
      if (newNet !== netName) { netName = newNet; if (W > 0) buildBackground(); }
      if (d.acx) {
        // columnar apps (?fmt=2) — rebuild the small per-app objects locally
        const M = d.acx.length;
        apps = new Array(M);
        for (let i = 0; i < M; i++) {
          apps[i] = {
            cx: d.acx[i], cy: d.acy[i], r: d.ar[i], size: d.asz[i], t: d.at[i],
            alive: d.aalive ? d.aalive[i] : null,
          };
        }
      } else {
        apps = (d.apps || []).map((app) => Object.assign({ alive: null }, app));
      }
      if (d.ids) {
        // columnar payload (?fmt=2) — the parallel arrays map straight onto
        // our typed arrays; ids is adopted as-is (no per-node objects at all)
        N = d.ids.length;
        ids = d.ids;
        nx = Float32Array.from(d.nx || []);
        ny = Float32Array.from(d.ny || []);
        nr = Float32Array.from(d.nr || []);
        nt = Int16Array.from(d.nt || []);
        ns = Uint8Array.from(d.ns || []);
        na = Int32Array.from(d.na || []);
        identitiesLoaded = d.identities_loaded == null
          ? ids.reduce((sum, id) => sum + (hasIdentity(id) ? 1 : 0), 0)
          : d.identities_loaded;
      } else {
        const nodes = d.nodes || [];
        N = nodes.length;
        nx = new Float32Array(N);
        ny = new Float32Array(N);
        nr = new Float32Array(N);
        nt = new Int16Array(N);
        ns = new Uint8Array(N);
        na = new Int32Array(N);
        ids = new Array(N);
        for (let i = 0; i < N; i++) {
          const n = nodes[i];
          nx[i] = n.x; ny[i] = n.y; nr[i] = n.r || 3;
          nt[i] = n.t == null ? -1 : n.t;
          ns[i] = n.s ? 1 : 0;
          na[i] = n.a == null ? -1 : n.a;
          ids[i] = n.id;
        }
        identitiesLoaded = N;
      }
      const es = d.edges || [];
      edges = new Int32Array(es.length * 3);
      for (let k = 0; k < es.length; k++) {
        edges[k * 3] = es[k][0]; edges[k * 3 + 1] = es[k][1]; edges[k * 3 + 2] = es[k][2] || 1;
      }
      // one distinct hue per template so the legend is actually informative
      // (the site's shape-based color would collapse the 3 SilverScript
      // contracts to one green); an optional callback can override a color.
      tplColors = templates.map((name, i) => {
        if (templateColorFn) { const c = templateColorFn(name, i); if (c && c !== '__default__') return c; }
        return PALETTE[i % PALETTE.length];
      });
      // template colors may have changed — cached sprites are stale
      orbCache.clear();
      nebCache.clear();
      flatCache = [];
      maxAppRadius = 0;
      for (let a = 0; a < apps.length; a++) maxAppRadius = Math.max(maxAppRadius, apps[a].r || 0);
      nodeGrid = buildSpatialGrid(N, (i) => nx[i], (i) => ny[i]);
      appGrid = buildSpatialGrid(apps.length, (i) => apps[i].cx, (i) => apps[i].cy);
      applyFilter();
    }

    function applyFilter() {
      visible = new Uint8Array(N);
      for (let i = 0; i < N; i++) {
        if (filter.status === 'active' && ns[i] !== 1) continue;
        if (filter.status === 'burned' && ns[i] !== 0) continue;
        if (filter.template != null && nt[i] !== filter.template) continue;
        if (filter.minSize > 2) {
          const app = apps[na[i]];
          if (app && app.size < filter.minSize) continue;
        }
        visible[i] = 1;
      }
      if (hoverNode >= 0 && !visible[hoverNode]) {
        hoverNode = -1;
        hoverApp = -1;
        hideTip();
      }
      if (focusNode >= 0 && !visible[focusNode]) focusNode = -1;
    }

    function appPassesFilter(app) {
      if (filter.minSize > 2 && app.size < filter.minSize) return false;
      // Older cached payloads do not have per-app alive counts. Keep their
      // overview honest by showing the app instead of pretending its status.
      if (app.alive == null || filter.status === 'all') return true;
      if (filter.status === 'active') return app.alive > 0;
      if (filter.status === 'burned') return app.alive < app.size;
      return true;
    }

    // Compact CSR spatial grid: two typed-array passes build it once per data
    // load, then pan/zoom/hover only touch cells intersecting the viewport.
    // This replaces O(all-coins) pointer scans on production-sized networks.
    function buildSpatialGrid(count, getX, getY) {
      if (!count) return null;
      const cell = 192;
      const minx = Math.floor((bounds.minx - cell) / cell) * cell;
      const miny = Math.floor((bounds.miny - cell) / cell) * cell;
      const cols = Math.max(1, Math.ceil((Math.max(1, bounds.w) + cell * 2) / cell));
      const rows = Math.max(1, Math.ceil((Math.max(1, bounds.h) + cell * 2) / cell));
      const counts = new Uint32Array(cols * rows);
      const bucketOf = (x, y) => {
        const gx = clamp(Math.floor((x - minx) / cell), 0, cols - 1);
        const gy = clamp(Math.floor((y - miny) / cell), 0, rows - 1);
        return gy * cols + gx;
      };
      for (let i = 0; i < count; i++) counts[bucketOf(getX(i), getY(i))]++;
      const offsets = new Uint32Array(counts.length + 1);
      for (let i = 0; i < counts.length; i++) offsets[i + 1] = offsets[i] + counts[i];
      const cursor = offsets.slice(0, counts.length);
      const items = new Uint32Array(count);
      for (let i = 0; i < count; i++) items[cursor[bucketOf(getX(i), getY(i))]++] = i;
      return { cell, minx, miny, cols, rows, offsets, items };
    }

    function collectGrid(grid, x0, y0, x1, y1, out) {
      out.length = 0;
      if (!grid) return out;
      const gx0 = clamp(Math.floor((x0 - grid.minx) / grid.cell), 0, grid.cols - 1);
      const gy0 = clamp(Math.floor((y0 - grid.miny) / grid.cell), 0, grid.rows - 1);
      const gx1 = clamp(Math.floor((x1 - grid.minx) / grid.cell), 0, grid.cols - 1);
      const gy1 = clamp(Math.floor((y1 - grid.miny) / grid.cell), 0, grid.rows - 1);
      if (gx0 > gx1 || gy0 > gy1) return out;
      for (let gy = gy0; gy <= gy1; gy++) {
        for (let gx = gx0; gx <= gx1; gx++) {
          const b = gy * grid.cols + gx;
          for (let p = grid.offsets[b]; p < grid.offsets[b + 1]; p++) out.push(grid.items[p]);
        }
      }
      return out;
    }

    // ---- transforms ----
    function computeFit() {
      let pad = 40;
      if (N > 0 && N <= 200) {
        // small networks draw boosted orbs (1.6x) with halos at every zoom —
        // pad the fit for the biggest orb's sprite extent or the outermost
        // halo clips at the canvas edge. Mirrors draw(): at fit zf=1 the
        // size multiplier is 1.0, boost 1.6, then bucket + halo tier.
        let rmax = 1.5;
        for (let i = 0; i < N; i++) if (nr[i] > rmax) rmax = nr[i];
        const rs = ORB_BUCKETS[orbBucket(Math.max(1.5, rmax) * 1.6)];
        pad = Math.max(pad, rs * orbExt(rs) + 12);
      }
      const bw = Math.max(1, bounds.w), bh = Math.max(1, bounds.h);
      fitScale = Math.min((W - pad * 2) / bw, (H - pad * 2) / bh);
      if (!isFinite(fitScale) || fitScale <= 0) fitScale = 1;
      scale = fitScale;
      // center the bounds box
      const cx = bounds.minx + bw / 2, cy = bounds.miny + bh / 2;
      panX = W / 2 - cx * scale;
      panY = H / 2 - cy * scale;
    }
    const sx = (wx) => wx * scale + panX;
    const sy = (wy) => wy * scale + panY;
    const wx = (px) => (px - panX) / scale;
    const wy = (py) => (py - panY) / scale;
    const zoomFactor = () => scale / fitScale;
    // Aggregate "app" bubbles exist to summarize tens of thousands of dots.
    // On a small network (mainnet today: a handful of covenants) they hide
    // everything behind giant circles — draw the real nodes at every zoom.
    const isOverview = () => N > 200 && zoomFactor() < 2.4;

    // ---- atmosphere (pre-rendered once per resize / network change) ----
    function buildBackground() {
      const dw = Math.max(2, Math.round(W * dpr)), dh = Math.max(2, Math.round(H * dpr));
      // vignette: very dark teal-black, center-weighted, near-black corners
      bgVignette = document.createElement('canvas');
      bgVignette.width = dw; bgVignette.height = dh;
      const vg = bgVignette.getContext('2d');
      const grad = vg.createRadialGradient(dw * 0.5, dh * 0.42, 0, dw * 0.5, dh * 0.42, Math.max(dw, dh) * 0.75);
      grad.addColorStop(0, '#081713');
      grad.addColorStop(0.55, '#04100d');
      grad.addColorStop(1, '#010405');
      vg.fillStyle = grad;
      vg.fillRect(0, 0, dw, dh);

      const seed = hashStr(netName || 'kascov');
      const area = W * H;
      starFar = makeStarLayer(dw, dh, seed ^ 0x9e3779b9,
        Math.max(90, Math.min(480, Math.round(area * 0.00036))), 0.72, true);
      starNear = makeStarLayer(dw, dh, seed ^ 0x85ebca6b,
        Math.max(60, Math.min(320, Math.round(area * 0.00022))), 1.0, false);
      applyBackgroundCss();
    }

    // css layer order is topmost-first: starNear, starFar, vignette
    function applyBackgroundCss() {
      const gen = ++bgGen;
      canvas.style.backgroundColor = '#010405';
      const layers = [starNear, starFar, bgVignette];
      Promise.all(layers.map((c) => new Promise((res) => c.toBlob((b) => res(b && URL.createObjectURL(b)), 'image/png'))))
        .then((urls) => {
          if (gen !== bgGen || urls.some((u) => !u)) {
            urls.forEach((u) => { if (u) URL.revokeObjectURL(u); });
            return;
          }
          bgUrls.forEach((u) => URL.revokeObjectURL(u));
          bgUrls = urls;
          canvas.style.backgroundImage = urls.map((u) => `url(${u})`).join(', ');
          canvas.style.backgroundRepeat = 'repeat, repeat, no-repeat';
          canvas.style.backgroundSize = `${W}px ${H}px, ${W}px ${H}px, 100% 100%`;
          syncBackgroundPan();
        });
    }
    // starfields pan at 0.5x / 0.25x of world pan (depth); vignette is fixed.
    // repeat-tiling wraps them seamlessly, so raw offsets are fine.
    function syncBackgroundPan() {
      canvas.style.backgroundPosition =
        `${panX * 0.5}px ${panY * 0.5}px, ${panX * 0.25}px ${panY * 0.25}px, 0 0`;
    }

    function makeStarLayer(dw, dh, seed, count, dim, withNebula) {
      const c = document.createElement('canvas');
      c.width = dw; c.height = dh;
      const g = c.getContext('2d');
      const rnd = mulberry32(seed);
      if (withNebula) {
        // one or two extremely faint nebula washes in template-palette hues,
        // placed deterministically from the network name hash — atmosphere.
        // Each wash is painted at all 3×3 tile offsets so the layer wraps
        // seamlessly under the parallax pan (no visible rectangle edges).
        const nWash = 1 + (seed & 1);
        for (let w = 0; w < nWash; w++) {
          const [cr, cg, cb] = parseColor(PALETTE[Math.floor(rnd() * PALETTE.length)]);
          const cx = rnd() * dw, cy = rnd() * dh;
          const rad = (0.35 + rnd() * 0.35) * Math.max(dw, dh);
          for (let tx = -1; tx <= 1; tx++) {
            for (let ty = -1; ty <= 1; ty++) {
              const ox = cx + tx * dw, oy = cy + ty * dh;
              if (ox + rad < 0 || ox - rad > dw || oy + rad < 0 || oy - rad > dh) continue;
              const ng = g.createRadialGradient(ox, oy, 0, ox, oy, rad);
              ng.addColorStop(0, rgba(cr, cg, cb, 0.05));
              ng.addColorStop(0.6, rgba(cr, cg, cb, 0.022));
              ng.addColorStop(1, rgba(cr, cg, cb, 0));
              g.fillStyle = ng;
              g.fillRect(0, 0, dw, dh);
            }
          }
        }
      }
      for (let i = 0; i < count; i++) {
        const x = rnd() * dw, y = rnd() * dh;
        const r = (0.35 + rnd() * 1.0) * dim * dpr;
        const a = (0.15 + rnd() * 0.25) * dim;
        const teal = rnd() < 0.16;
        g.fillStyle = teal ? rgba(125, 222, 200, a) : rgba(214, 232, 238, a);
        g.beginPath();
        g.arc(x, y, r, 0, TAU);
        g.fill();
        // duplicate stars near the edges so the wrap-tiling has no seam
        if (x < 4 * dpr) { g.beginPath(); g.arc(x + dw, y, r, 0, TAU); g.fill(); }
        if (x > dw - 4 * dpr) { g.beginPath(); g.arc(x - dw, y, r, 0, TAU); g.fill(); }
        if (y < 4 * dpr) { g.beginPath(); g.arc(x, y + dh, r, 0, TAU); g.fill(); }
        if (y > dh - 4 * dpr) { g.beginPath(); g.arc(x, y - dh, r, 0, TAU); g.fill(); }
      }
      return c;
    }

    function drawBackground() {
      // atmosphere is css — the canvas only clears to transparent and nudges
      // the parallax offsets; the compositor does the blending
      ctx.clearRect(0, 0, W, H);
      syncBackgroundPan();
    }

    // ---- orb sprites (color × size-bucket × status) ----
    // Halo tier follows SCREEN size (r is already screen px): tiny orbs
    // (<=4px — dense mega-clusters blit tens of thousands per frame) are
    // core-only sprites whose quad matches a plain arc, so halo fill-rate
    // never dominates; mid orbs (4–12px) get a tight low-alpha halo so dense
    // rings read as rings instead of fusing to milk; only big orbs (sparse
    // networks, deep zoom) pay for the full soft glow. The tier is a pure
    // function of the bucketed radius, so the bucket index already encodes
    // it in the sprite cache key.
    function orbExt(r) { return r <= 4 ? 1 : r <= 12 ? 1.6 : 2.5; }
    function makeOrbSprite(colorStr, r, active) {
      const ext = orbExt(r);
      const HR = r * ext; // halo radius
      const s = Math.min(384, Math.max(8, Math.ceil(HR * 2 * dpr)));
      const c = document.createElement('canvas');
      c.width = s; c.height = s;
      const g = c.getContext('2d');
      let [cr, cg, cb, ca] = parseColor(colorStr);
      let aMul = ca;
      if (!active) {
        // burned/retired: dimmer + slightly desaturated
        const grey = 0.2126 * cr + 0.7152 * cg + 0.0722 * cb;
        cr = mixTo(cr, grey, 0.45); cg = mixTo(cg, grey, 0.45); cb = mixTo(cb, grey, 0.45);
        aMul = Math.min(ca, 0.55);
      }
      const mid = s / 2;
      const grad = g.createRadialGradient(mid, mid, 0, mid, mid, mid);
      const hotW = active ? 0.88 : 0.45;
      const warmW = active ? 0.5 : 0.25;
      if (ext === 1) {
        // core-only: hot center -> body -> quick soft edge, no halo at all
        grad.addColorStop(0, rgba(mixTo(cr, 255, hotW), mixTo(cg, 255, hotW), mixTo(cb, 255, hotW), aMul));
        grad.addColorStop(0.30, rgba(mixTo(cr, 255, warmW), mixTo(cg, 255, warmW), mixTo(cb, 255, warmW), 0.95 * aMul));
        grad.addColorStop(0.78, rgba(cr, cg, cb, 0.9 * aMul));
        grad.addColorStop(1, rgba(cr, cg, cb, 0));
      } else {
        const body = 1 / ext;            // body edge = node radius
        const tight = ext < 2;           // mid tier: halo stays quiet
        const haloA = tight ? 0.04 : 0.10; // dense packs must not fuse to milk
        const edgeA = tight ? 0.30 : 0.38;
        grad.addColorStop(0, rgba(mixTo(cr, 255, hotW), mixTo(cg, 255, hotW), mixTo(cb, 255, hotW), aMul));
        grad.addColorStop(0.10, rgba(mixTo(cr, 255, warmW), mixTo(cg, 255, warmW), mixTo(cb, 255, warmW), 0.95 * aMul));
        grad.addColorStop(body * 0.7, rgba(cr, cg, cb, 0.85 * aMul));
        grad.addColorStop(body, rgba(cr, cg, cb, edgeA * aMul));
        grad.addColorStop(Math.min(0.9, body + 0.3), rgba(cr, cg, cb, haloA * aMul));
        grad.addColorStop(1, rgba(cr, cg, cb, 0));
      }
      g.fillStyle = grad;
      g.fillRect(0, 0, s, s);
      return c;
    }
    function orbSpriteFor(i, bi) {
      // numeric key — no per-node string allocation on the hot path
      const ci = colorMode === 'status' ? (ns[i] ? 0 : 1) : (nt[i] >= 0 ? nt[i] + 3 : 2);
      const key = ci * 1024 + bi * 2 + ns[i];
      let spr = orbCache.get(key);
      if (!spr) {
        spr = makeOrbSprite(colorFor(i), ORB_BUCKETS[bi], ns[i] === 1);
        orbCache.set(key, spr);
      }
      return spr;
    }

    // ---- nebula sprites for far-mode app aggregates ----
    function makeNebulaSprite(colorStr, r, variant) {
      const OR = r * 1.75; // cloud extends past the disc radius
      const s = Math.min(512, Math.max(12, Math.ceil(OR * 2 * dpr)));
      const c = document.createElement('canvas');
      c.width = s; c.height = s;
      const g = c.getContext('2d');
      const [cr, cg, cb, ca] = parseColor(colorStr);
      const mid = s / 2;
      const rpx = mid / 1.75; // disc radius in sprite px
      // broad soft cloud
      const cloud = g.createRadialGradient(mid, mid, 0, mid, mid, mid);
      cloud.addColorStop(0, rgba(cr, cg, cb, 0.34 * ca));
      cloud.addColorStop(0.45, rgba(cr, cg, cb, 0.18 * ca));
      cloud.addColorStop(1, rgba(cr, cg, cb, 0));
      g.fillStyle = cloud;
      g.fillRect(0, 0, s, s);
      // brighter dense core, offset slightly (direction varies per app)
      const rot = variant * (Math.PI / 2) + 0.6;
      const ox = mid + Math.cos(rot) * 0.22 * rpx, oy = mid + Math.sin(rot) * 0.22 * rpx;
      const core = g.createRadialGradient(ox, oy, 0, ox, oy, rpx * 0.62);
      const wr = mixTo(cr, 255, 0.35), wg = mixTo(cg, 255, 0.35), wb = mixTo(cb, 255, 0.35);
      core.addColorStop(0, rgba(wr, wg, wb, 0.5 * ca));
      core.addColorStop(0.55, rgba(cr, cg, cb, 0.22 * ca));
      core.addColorStop(1, rgba(cr, cg, cb, 0));
      g.fillStyle = core;
      g.fillRect(0, 0, s, s);
      // faint elliptical rim
      g.strokeStyle = rgba(cr, cg, cb, 0.13 * ca);
      g.lineWidth = Math.max(1, s * 0.012);
      g.beginPath();
      g.ellipse(mid, mid, rpx * 1.02, rpx * 0.8, rot, 0, TAU);
      g.stroke();
      return c;
    }
    function nebSpriteFor(app, a, bi) {
      const statusClass = app.alive == null ? 2
        : app.alive <= 0 ? 1 : app.alive >= app.size ? 0 : 2;
      const ci = colorMode === 'status' ? statusClass : (app.t >= 0 ? app.t + 3 : 2);
      const variant = a & 3;
      const key = ci * 1024 + bi * 4 + variant;
      let spr = nebCache.get(key);
      if (!spr) {
        const color = colorMode === 'status'
          ? (statusClass === 0 ? ACTIVE_COLOR
            : statusClass === 1 ? BURNED_COLOR : 'rgba(105,190,176,0.82)')
          : (app.t >= 0 ? tplColors[app.t] : 'rgba(150,160,180,0.7)');
        spr = makeNebulaSprite(color, NEB_BUCKETS[bi], variant);
        nebCache.set(key, spr);
      }
      return spr;
    }

    // ---- draw ----
    function requestDraw() {
      if (rafPending) return;
      rafPending = true;
      requestAnimationFrame(() => { rafPending = false; draw(); });
    }

    function draw() {
      if (destroyed) return;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      drawBackground();
      if (!N && !apps.length) {
        frameStats = { lod: 'empty', nodes: 0, apps: 0, labels: 0 };
        return;
      }

      const zf = zoomFactor();
      const detailMix = N <= 200 ? 1 : smoothstep(DETAIL_START, DETAIL_END, zf);
      const near = zf >= EDGE_DETAIL || (N <= 200 && N > 0);
      // visible world rect (for culling)
      const vx0 = wx(-20), vy0 = wy(-20), vx1 = wx(W + 20), vy1 = wy(H + 20);
      frameStats = {
        lod: detailMix <= 0 ? (isOverview() ? 'overview' : 'constellation')
          : detailMix >= 1 ? 'detail' : 'transition',
        nodes: 0, apps: 0, labels: 0,
      };

      // Overview and constellation layers remain present into the first part
      // of the detail transition. That cross-fade removes the old hard cut
      // from a quiet starfield to a wall of full-size circles.
      if (detailMix < 1) {
        if (isOverview()) drawGalaxyHaze();
        ctx.save();
        ctx.globalAlpha = 1 - detailMix * 0.92;
        drawApps(vx0, vy0, vx1, vy1, zf);
        ctx.restore();
      }

      if (detailMix > 0) {
        const padWorld = 28 / scale;
        collectGrid(nodeGrid, vx0 - padWorld, vy0 - padWorld, vx1 + padWorld, vy1 + padWorld, viewNodes);
        let visibleNodeCount = 0;
        for (let n = 0; n < viewNodes.length; n++) {
          const i = viewNodes[n];
          if (visible[i] && nx[i] >= vx0 && nx[i] <= vx1 && ny[i] >= vy0 && ny[i] <= vy1) visibleNodeCount++;
        }
        // edges: a hovered app's connections always; all in-view edges only
        // once individual nodes have enough screen-space to read. Dense views
        // suppress the global web entirely; otherwise it becomes an opaque
        // fan long before individual relationships can be understood.
        const showAllEdges = near && N <= 200 && visibleNodeCount <= 700;
        if (edges && (showAllEdges || hoverNode >= 0)) drawEdges(vx0, vy0, vx1, vy1, showAllEdges);
        drawNodes(viewNodes, vx0, vy0, vx1, vy1, zf, detailMix);
      }

      drawHud();
    }

    function drawGalaxyHaze() {
      const cx = sx(bounds.minx + bounds.w / 2);
      const cy = sy(bounds.miny + bounds.h / 2);
      const rx = Math.max(80, Math.abs(bounds.w * scale) * 0.48);
      const ry = Math.max(60, Math.abs(bounds.h * scale) * 0.48);
      const radius = Math.max(rx, ry);
      ctx.save();
      ctx.translate(cx, cy);
      ctx.scale(rx / radius, ry / radius);
      const haze = ctx.createRadialGradient(0, 0, radius * 0.02, 0, 0, radius);
      haze.addColorStop(0, 'rgba(97,220,194,0.10)');
      haze.addColorStop(0.38, 'rgba(79,170,154,0.055)');
      haze.addColorStop(0.72, 'rgba(64,125,116,0.026)');
      haze.addColorStop(1, 'rgba(25,75,68,0)');
      ctx.fillStyle = haze;
      ctx.beginPath();
      ctx.arc(0, 0, radius, 0, TAU);
      ctx.fill();
      ctx.restore();
    }

    function drawNodes(candidates, vx0, vy0, vx1, vy1, zf, layerAlpha) {
      const mBase = 0.72 + 0.28 * smoothstep(DETAIL_START, 30, zf);
      // small networks: scale sprites up so a handful of coins reads as
      // intentional celestial bodies, not lost pixels
      const boost = N <= 200 ? 1.55 : 1;
      const twinkle = ambientOn && !reduceMotion.matches;
      const tNow = twinkle ? performance.now() : 0;
      let visCount = 0;
      for (let n = 0; n < candidates.length; n++) {
        const i = candidates[n];
        if (!visible[i]) continue;
        const x = nx[i], y = ny[i];
        if (x < vx0 || x > vx1 || y < vy0 || y > vy1) continue;
        visCount++;
      }
      frameStats.nodes = visCount;
      const denseMode = visCount > 1800;
      for (let n = 0; n < candidates.length; n++) {
        const i = candidates[n];
        if (!visible[i]) continue;
        const x = nx[i], y = ny[i];
        if (x < vx0 || x > vx1 || y < vy0 || y > vy1) continue;
        const px = sx(x), py = sy(y);
        const rr = (1.8 + Math.max(0, nr[i] - 4) * 0.55) * mBase * boost;
        const dimmed = hoverApp >= 0 && na[i] !== hoverApp;
        // slow sinusoidal twinkle on ~2% of nodes, seeded by index — only
        // while the ambient (pointer-over) loop is running
        const shimmer = twinkle && ((i * 2654435761 >>> 0) % 47) === 3;
        let alpha = layerAlpha * (dimmed ? 0.22 : 1);
        if (shimmer) alpha *= 0.68 + 0.32 * Math.sin(tNow * 0.0026 + i * 1.7);
        let r; // final screen radius (labels key off it below)
        if (denseMode && rr <= 13) {
          // flat disc, unbucketed radius (13 ≈ the ≤12 sprite tier plus the
          // bucket quantization slack, so tiers don't shift between modes);
          // skip the globalAlpha round-trip when it would be 1 anyway
          r = rr;
          ctx.beginPath();
          ctx.arc(px, py, r, 0, TAU);
          ctx.fillStyle = flatStyleFor(i);
          if (alpha !== 1) {
            ctx.globalAlpha = alpha;
            ctx.fill();
            ctx.globalAlpha = 1;
          } else {
            ctx.fill();
          }
        } else {
          const bi = orbBucket(rr);
          r = ORB_BUCKETS[bi];
          const HR = r * orbExt(r);
          if (alpha !== 1) {
            ctx.globalAlpha = alpha;
            ctx.drawImage(orbSpriteFor(i, bi), px - HR, py - HR, HR * 2, HR * 2);
            ctx.globalAlpha = 1;
          } else {
            ctx.drawImage(orbSpriteFor(i, bi), px - HR, py - HR, HR * 2, HR * 2);
          }
        }
        if (i === hoverNode) {
          // single hovered node: one-off glow ring (the only shadowBlur draw)
          const pulse = twinkle ? 1.5 + Math.sin(tNow * 0.005) * 1.5 : 0;
          ctx.save();
          ctx.globalAlpha = 1;
          ctx.shadowColor = 'rgba(120,255,225,0.9)';
          ctx.shadowBlur = 12;
          ctx.lineWidth = 1.5;
          ctx.strokeStyle = 'rgba(240,255,250,0.95)';
          ctx.beginPath();
          ctx.arc(px, py, r + 3.5 + pulse, 0, TAU);
          ctx.stroke();
          ctx.restore();
        }
      }
      ctx.globalAlpha = 1;
      if (zf >= LABEL_DETAIL && visCount <= 900) {
        frameStats.labels = drawLabels(candidates, vx0, vy0, vx1, vy1, mBase * boost);
      }
    }

    function drawLabels(candidates, vx0, vy0, vx1, vy1, radiusMul) {
      const ranked = [];
      for (let n = 0; n < candidates.length; n++) {
        const i = candidates[n];
        if (!visible[i]) continue;
        if (nx[i] < vx0 || nx[i] > vx1 || ny[i] < vy0 || ny[i] > vy1) continue;
        if (i !== focusNode && ns[i] !== 1 && nr[i] < 6) continue;
        ranked.push(i);
      }
      ranked.sort((a, b) => {
        const sa = (a === focusNode ? 1e9 : 0) + ns[a] * 10000 + nr[a] * 100 - (hashIndex(a) & 255);
        const sb = (b === focusNode ? 1e9 : 0) + ns[b] * 10000 + nr[b] * 100 - (hashIndex(b) & 255);
        return sb - sa;
      });

      const boxes = [];
      ctx.font = '12px ui-monospace, monospace';
      ctx.lineWidth = 3.5;
      for (let n = 0; n < ranked.length && boxes.length < 18; n++) {
        const i = ranked[n];
        if (!hasIdentity(ids[i])) continue;
        const text = friendlyName(ids[i]);
        const width = Math.ceil(ctx.measureText(text).width);
        const px = sx(nx[i]), py = sy(ny[i]);
        const r = Math.max(2, (1.8 + Math.max(0, nr[i] - 4) * 0.55) * radiusMul);
        let tx = px + r + 6;
        if (tx + width > W - 12) tx = px - r - width - 6;
        const ty = py + 4;
        const box = { x0: tx - 4, y0: ty - 15, x1: tx + width + 4, y1: ty + 6 };
        // Do not draw partial names along the border. This is the concrete fix
        // for the stray right-edge labels visible in the old detail view.
        if (box.x0 < 10 || box.x1 > W - 10 || box.y0 < 10 || box.y1 > H - 10) continue;
        let collides = false;
        for (let b = 0; b < boxes.length; b++) {
          const q = boxes[b];
          if (box.x0 < q.x1 && box.x1 > q.x0 && box.y0 < q.y1 && box.y1 > q.y0) {
            collides = true;
            break;
          }
        }
        if (collides) continue;
        boxes.push(box);
        ctx.strokeStyle = 'rgba(4,12,10,0.92)';
        ctx.strokeText(text, tx, ty);
        ctx.fillStyle = 'rgba(230,240,248,0.94)';
        ctx.fillText(text, tx, ty);
      }
      return boxes.length;
    }

    // dense-frame flat disc color, cached per color × status (mirrors the
    // sprite's burned dim/desaturate treatment; cleared with the sprite
    // caches on data/template-color changes). Plain array indexed by the
    // small integer key — this sits on the hottest per-node path.
    let flatCache = [];
    function flatStyleFor(i) {
      const ci = colorMode === 'status' ? (ns[i] ? 0 : 1) : (nt[i] >= 0 ? nt[i] + 3 : 2);
      const key = ci * 2 + ns[i];
      let s = flatCache[key];
      if (s === undefined) {
        let [cr, cg, cb, ca] = parseColor(colorFor(i));
        let aMul = ca;
        if (ns[i] !== 1) {
          const grey = 0.2126 * cr + 0.7152 * cg + 0.0722 * cb;
          cr = mixTo(cr, grey, 0.45); cg = mixTo(cg, grey, 0.45); cb = mixTo(cb, grey, 0.45);
          aMul = Math.min(ca, 0.55);
        }
        s = rgba(cr, cg, cb, 0.92 * aMul);
        flatCache[key] = s;
      }
      return s;
    }

    function drawApps(vx0, vy0, vx1, vy1, zf) {
      const twinkle = ambientOn && !reduceMotion.matches;
      const extra = maxAppRadius + 16 / scale;
      collectGrid(appGrid, vx0 - extra, vy0 - extra, vx1 + extra, vy1 + extra, viewApps);
      let inView = 0;
      for (let n = 0; n < viewApps.length; n++) {
        const app = apps[viewApps[n]];
        if (!appPassesFilter(app)) continue;
        if (app.cx < vx0 - app.r || app.cx > vx1 + app.r || app.cy < vy0 - app.r || app.cy > vy1 + app.r) continue;
        inView++;
      }
      // A stable deterministic sample keeps the 30k+ app overview airy. The
      // old renderer painted every tiny square, turning the sunflower layout
      // into a solid moiré disc. More points appear naturally as zoom culls
      // the world down; the first 72 largest apps are always retained.
      const budget = isOverview()
        ? clamp(Math.round(W * H / 300), 900, 2200)
        : clamp(Math.round(W * H / 180), 1500, 3600);
      const stride = Math.max(1, Math.ceil(inView / budget));
      let dotPaths = null;
      for (let n = 0; n < viewApps.length; n++) {
        const a = viewApps[n];
        const app = apps[a];
        if (!appPassesFilter(app)) continue;
        if (app.cx < vx0 - app.r || app.cx > vx1 + app.r || app.cy < vy0 - app.r || app.cy > vy1 + app.r) continue;
        if (a >= 72 && a !== hoverApp && stride > 1 && hashIndex(a) % stride !== 0) continue;
        const px = sx(app.cx), py = sy(app.cy);
        const rs = app.r * scale * 0.5;
        frameStats.apps++;
        if (rs < 9 && a !== hoverApp) {
          if (!dotPaths) dotPaths = [new Path2D(), new Path2D(), new Path2D()];
          const statusGroup = app.alive == null ? -1
            : app.alive <= 0 ? 1 : app.alive >= app.size ? 0 : 2;
          const group = colorMode === 'status' && statusGroup >= 0
            ? statusGroup
            : ((app.t >= 0 ? app.t : 2) + (hashIndex(a) % 3)) % 3;
          const p = dotPaths[group % 3];
          const dr = 0.65 + Math.min(1.05, rs * 0.22) + Math.min(0.4, Math.log2(app.size + 1) * 0.08);
          p.moveTo(px + dr, py);
          p.arc(px, py, dr, 0, TAU);
          continue;
        }
        const r = Math.max(2, rs);
        const bi = nebBucket(r);
        const rb = NEB_BUCKETS[bi];
        const OHR = rb * 1.75;
        const spr = nebSpriteFor(app, a, bi);
        ctx.drawImage(spr, px - OHR, py - OHR, OHR * 2, OHR * 2);
        if (a === hoverApp) {
          // hovered app: brighter core + gentle outer glow ring (single draw)
          const pulse = twinkle ? 1.5 + Math.sin(performance.now() * 0.004) * 1.5 : 0;
          ctx.save();
          ctx.globalCompositeOperation = 'lighter';
          ctx.globalAlpha = 0.45;
          ctx.drawImage(spr, px - OHR, py - OHR, OHR * 2, OHR * 2);
          ctx.restore();
          ctx.save();
          ctx.shadowColor = 'rgba(73,234,203,0.8)';
          ctx.shadowBlur = 10;
          ctx.lineWidth = 1.5;
          ctx.strokeStyle = 'rgba(73,234,203,0.75)';
          ctx.beginPath();
          ctx.arc(px, py, rb + 4 + pulse, 0, TAU);
          ctx.stroke();
          ctx.restore();
        }
      }
      if (dotPaths) {
        for (let g = 0; g < 3; g++) {
          ctx.fillStyle = DOT_TINTS[g];
          ctx.fill(dotPaths[g]);
        }
      }
    }

    // deterministic perpendicular bow control point for edge k — ±6% of
    // length, seeded by edge index (same curve every frame)
    function edgeCtrl(k, x0, y0, x1, y1) {
      const h = (k * 2654435761) >>> 0;
      const bow = ((h & 1023) / 1023 - 0.5) * 0.12;
      edgeCX = (x0 + x1) / 2 - (y1 - y0) * bow;
      edgeCY = (y0 + y1) / 2 + (x1 - x0) * bow;
    }
    let edgeCX = 0, edgeCY = 0;

    function drawEdges(vx0, vy0, vx1, vy1, near) {
      // Subtly bowed quadratics with a two-pass fake gradient (dim full
      // stroke + brighter mid-segment), stroked PER EDGE: one giant batched
      // Path2D stroke forces the rasterizer to build an alpha-correct
      // coverage mask for the whole path, which measured ~9x slower than
      // the same geometry stroked edge-by-edge (and per-edge is the old
      // renderer's proven-cheap mechanic; overlap double-blend at alpha
      // 0.10 is invisible). Dense views (>1500 in-view edges) drop the
      // mid-segment pass — at that density it reads as mush anyway — so
      // heavy frames pay a single cheap stroke per edge.
      const M = edges.length / 3;
      const hots = hoverNode >= 0 ? [] : null;
      if (near) {
        let inView = 0;
        for (let k = 0; k < M; k++) {
          const i = edges[k * 3], j = edges[k * 3 + 1];
          if (!visible[i] || !visible[j]) continue;
          const ax = nx[i], ay = ny[i], bx = nx[j], by = ny[j];
          if ((ax < vx0 && bx < vx0) || (ax > vx1 && bx > vx1) || (ay < vy0 && by < vy0) || (ay > vy1 && by > vy1)) continue;
          inView++;
        }
        // sparse views get the full pretty treatment (bow + bright mid
        // segment); past ~1.5k in-view edges the 1px alpha strokes fuse
        // into lace where neither the bow nor the gradient is resolvable,
        // so dense frames pay one straight line per edge instead — the old
        // renderer's exact (and cheapest) mechanic.
        const twoPass = inView <= 1500;
        ctx.lineWidth = 1;
        // constant style per pass keeps the parsed-color fast path; a touch
        // brighter when the mid pass is skipped
        ctx.strokeStyle = twoPass ? 'rgba(110,190,170,0.10)' : 'rgba(110,190,170,0.14)';
        for (let k = 0; k < M; k++) {
          const i = edges[k * 3], j = edges[k * 3 + 1];
          if (!visible[i] || !visible[j]) continue;
          const ax = nx[i], ay = ny[i], bx = nx[j], by = ny[j];
          if ((ax < vx0 && bx < vx0) || (ax > vx1 && bx > vx1) || (ay < vy0 && by < vy0) || (ay > vy1 && by > vy1)) continue;
          if (hots && (i === hoverNode || j === hoverNode)) { hots.push(k); continue; }
          const x0 = sx(ax), y0 = sy(ay), x1 = sx(bx), y1 = sy(by);
          ctx.beginPath();
          ctx.moveTo(x0, y0);
          if (twoPass) {
            edgeCtrl(k, x0, y0, x1, y1);
            ctx.quadraticCurveTo(edgeCX, edgeCY, x1, y1);
          } else {
            ctx.lineTo(x1, y1);
          }
          ctx.stroke();
        }
        if (twoPass) {
          // brighter mid-segment (t ∈ [0.28, 0.72] of the same quadratic,
          // via blossom) — second loop so the style stays constant
          ctx.strokeStyle = 'rgba(150,230,210,0.20)';
          const t0 = 0.28, t1 = 0.72, u0 = 1 - t0, u1 = 1 - t1;
          for (let k = 0; k < M; k++) {
            const i = edges[k * 3], j = edges[k * 3 + 1];
            if (!visible[i] || !visible[j]) continue;
            if (hots && (i === hoverNode || j === hoverNode)) continue;
            const ax = nx[i], ay = ny[i], bx = nx[j], by = ny[j];
            if ((ax < vx0 && bx < vx0) || (ax > vx1 && bx > vx1) || (ay < vy0 && by < vy0) || (ay > vy1 && by > vy1)) continue;
            const x0 = sx(ax), y0 = sy(ay), x1 = sx(bx), y1 = sy(by);
            edgeCtrl(k, x0, y0, x1, y1);
            const cxp = edgeCX, cyp = edgeCY;
            const mx0 = u0 * u0 * x0 + 2 * u0 * t0 * cxp + t0 * t0 * x1;
            const my0 = u0 * u0 * y0 + 2 * u0 * t0 * cyp + t0 * t0 * y1;
            const mx1 = u1 * u1 * x0 + 2 * u1 * t1 * cxp + t1 * t1 * x1;
            const my1 = u1 * u1 * y0 + 2 * u1 * t1 * cyp + t1 * t1 * y1;
            const mcx = u0 * u1 * x0 + (t0 * u1 + t1 * u0) * cxp + t0 * t1 * x1;
            const mcy = u0 * u1 * y0 + (t0 * u1 + t1 * u0) * cyp + t0 * t1 * y1;
            ctx.beginPath();
            ctx.moveTo(mx0, my0);
            ctx.quadraticCurveTo(mcx, mcy, mx1, my1);
            ctx.stroke();
          }
        }
      } else if (hots) {
        // not near: only the hovered app's edges are drawn
        for (let k = 0; k < M; k++) {
          const i = edges[k * 3], j = edges[k * 3 + 1];
          if (i !== hoverNode && j !== hoverNode) continue;
          if (!visible[i] || !visible[j]) continue;
          const ax = nx[i], ay = ny[i], bx = nx[j], by = ny[j];
          if ((ax < vx0 && bx < vx0) || (ax > vx1 && bx > vx1) || (ay < vy0 && by < vy0) || (ay > vy1 && by > vy1)) continue;
          hots.push(k);
        }
      }
      if (hots && hots.length) {
      // hot (hovered coin) edges glow teal: wide dim underlay + bright
        // core on the full curve + brighter mid-segment. Very large apps can
        // still own tens of thousands of links, so keep a stable sample; a
        // solid fan communicates less than a few hundred representative ties.
        const t0 = 0.28, t1 = 0.72, u0 = 1 - t0, u1 = 1 - t1;
        const hotStride = Math.max(1, Math.ceil(hots.length / 96));
        for (let n = 0; n < hots.length; n += hotStride) {
          const k = hots[n];
          const i = edges[k * 3], j = edges[k * 3 + 1];
          const x0 = sx(nx[i]), y0 = sy(ny[i]), x1 = sx(nx[j]), y1 = sy(ny[j]);
          edgeCtrl(k, x0, y0, x1, y1);
          const cxp = edgeCX, cyp = edgeCY;
          ctx.beginPath();
          ctx.moveTo(x0, y0);
          ctx.quadraticCurveTo(cxp, cyp, x1, y1);
          ctx.lineWidth = 4;
          ctx.strokeStyle = 'rgba(73,234,203,0.10)';
          ctx.stroke();
          ctx.lineWidth = 1.6;
          ctx.strokeStyle = 'rgba(73,234,203,0.45)';
          ctx.stroke();
          const mx0 = u0 * u0 * x0 + 2 * u0 * t0 * cxp + t0 * t0 * x1;
          const my0 = u0 * u0 * y0 + 2 * u0 * t0 * cyp + t0 * t0 * y1;
          const mx1 = u1 * u1 * x0 + 2 * u1 * t1 * cxp + t1 * t1 * x1;
          const my1 = u1 * u1 * y0 + 2 * u1 * t1 * cyp + t1 * t1 * y1;
          const mcx = u0 * u1 * x0 + (t0 * u1 + t1 * u0) * cxp + t0 * t1 * x1;
          const mcy = u0 * u1 * y0 + (t0 * u1 + t1 * u0) * cyp + t0 * t1 * y1;
          ctx.beginPath();
          ctx.moveTo(mx0, my0);
          ctx.quadraticCurveTo(mcx, mcy, mx1, my1);
          ctx.lineWidth = 1.8;
          ctx.strokeStyle = 'rgba(170,255,235,0.65)';
          ctx.stroke();
        }
        ctx.lineWidth = 1;
      }
    }

    function drawHud() {
      // subtle "zoom to explore" hint at far zoom
      if (isOverview() && apps.length) {
        ctx.fillStyle = 'rgba(150,165,185,0.38)';
        ctx.font = '11px ui-monospace, monospace';
        const prevLs = ctx.letterSpacing;
        if (prevLs !== undefined) ctx.letterSpacing = '0.08em';
        ctx.fillText('scroll to explore · drag to pan · click a cluster to focus', 14, H - 14);
        if (prevLs !== undefined) ctx.letterSpacing = prevLs;
      }
    }

    function colorFor(i) {
      if (colorMode === 'status') return ns[i] ? ACTIVE_COLOR : BURNED_COLOR;
      return nt[i] >= 0 ? tplColors[nt[i]] : UNKNOWN_COLOR;
    }

    // ---- ambient life (pointer-over only; never under reduced motion) ----
    function startAmbient() {
      if (ambientOn || reduceMotion.matches) return;
      ambientOn = true;
      lastAmbient = 0;
      ambientRaf = requestAnimationFrame(ambientTick);
    }
    function stopAmbient() {
      if (!ambientOn) return;
      ambientOn = false;
      if (ambientRaf) cancelAnimationFrame(ambientRaf);
      ambientRaf = 0;
      requestDraw(); // settle back to the static (event-driven) state
    }
    function ambientTick(t) {
      if (!ambientOn) return;
      ambientRaf = requestAnimationFrame(ambientTick);
      if (t - lastAmbient < 33) return; // ~30fps is plenty for a shimmer
      if (anim) return; // zoom animation already drives frames
      // only redraw when something can actually shimmer
      if (isOverview() && hoverApp < 0) return;
      if (!N && !apps.length) return;
      lastAmbient = t;
      requestDraw();
    }

    // ---- hit testing (spatially indexed; redraw/hover are event-driven) ----
    function nodeAt(px, py) {
      let best = -1, bd = 16 * 16;
      const cx = wx(px), cy = wy(py);
      const reach = 20 / scale;
      collectGrid(nodeGrid, cx - reach, cy - reach, cx + reach, cy + reach, hitNodes);
      for (let n = 0; n < hitNodes.length; n++) {
        const i = hitNodes[n];
        if (!visible[i]) continue;
        const dx = sx(nx[i]) - px, dy = sy(ny[i]) - py, d2 = dx * dx + dy * dy;
        const rr = Math.max(7, Math.min(16, nr[i] * 1.15)) ** 2;
        if (d2 <= Math.max(16 * 16, rr) && d2 < bd) { bd = d2; best = i; }
      }
      return best;
    }
    function appAt(px, py) {
      let best = -1, bd = 28 * 28;
      const cx = wx(px), cy = wy(py);
      const reach = 32 / scale;
      collectGrid(appGrid, cx - reach, cy - reach, cx + reach, cy + reach, hitApps);
      for (let n = 0; n < hitApps.length; n++) {
        const a = hitApps[n];
        const app = apps[a];
        if (!appPassesFilter(app)) continue;
        const dx = sx(app.cx) - px, dy = sy(app.cy) - py, d2 = dx * dx + dy * dy;
        // Hit the readable center, not an enormous invisible app-radius disc.
        // Overlapping large aggregates otherwise make nearby clicks ambiguous.
        const r = clamp(app.r * scale * 0.5, 7, 28);
        if (d2 < r * r && d2 < bd) { bd = d2; best = a; }
      }
      return best;
    }

    // ---- interaction ----
    function onWheel(ev) {
      ev.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const px = ev.clientX - rect.left, py = ev.clientY - rect.top;
      stopProgrammaticAnim();
      hoverNode = -1;
      hoverApp = -1;
      hideTip();
      canvas.style.cursor = 'grab';
      if (!wheelTarget) wheelTarget = { scale, panX, panY };
      const modeMul = ev.deltaMode === 1 ? 16 : ev.deltaMode === 2 ? H : 1;
      const delta = clamp(ev.deltaY * modeMul, -120, 120);
      const wpx = (px - wheelTarget.panX) / wheelTarget.scale;
      const wpy = (py - wheelTarget.panY) / wheelTarget.scale;
      const nextScale = clamp(wheelTarget.scale * Math.exp(-delta * 0.00185), fitScale * 0.75, fitScale * 60);
      wheelTarget = {
        scale: nextScale,
        panX: px - wpx * nextScale,
        panY: py - wpy * nextScale,
      };
      wheelLast = performance.now();
      if (!wheelRaf) {
        wheelTickAt = wheelLast;
        wheelRaf = requestAnimationFrame(stepWheel);
      }
    }
    function stepWheel(now) {
      wheelRaf = 0;
      if (!wheelTarget || destroyed) return;
      const dt = clamp(now - wheelTickAt, 1, 34);
      wheelTickAt = now;
      const ease = 1 - Math.exp(-dt / 58);
      scale += (wheelTarget.scale - scale) * ease;
      panX += (wheelTarget.panX - panX) * ease;
      panY += (wheelTarget.panY - panY) * ease;
      draw();
      const settled = Math.abs(wheelTarget.scale - scale) < fitScale * 0.001
        && Math.abs(wheelTarget.panX - panX) < 0.15
        && Math.abs(wheelTarget.panY - panY) < 0.15
        && now - wheelLast > 70;
      if (settled) {
        scale = wheelTarget.scale;
        panX = wheelTarget.panX;
        panY = wheelTarget.panY;
        wheelTarget = null;
        draw();
      } else {
        wheelRaf = requestAnimationFrame(stepWheel);
      }
    }
    function stopWheel() {
      if (wheelRaf) cancelAnimationFrame(wheelRaf);
      wheelRaf = 0;
      wheelTarget = null;
    }
    // onUp lives on WINDOW (so drags can release outside the canvas), which
    // means it hears every pointerup on the whole page for as long as the
    // controller exists — including on other views where the canvas is hidden
    // and its rect degenerates to (0,0), making arbitrary page clicks hit-test
    // against phantom node positions ("random coin opens"). A click only
    // counts if the pointer went DOWN on the canvas.
    let pointerFromCanvas = false;
    function onDown(ev) {
      pointerFromCanvas = true;
      activePointers.set(ev.pointerId, { x: ev.clientX, y: ev.clientY });
      dragMoved = false;
      stopWheel();
      stopProgrammaticAnim();
      canvas.setPointerCapture && canvas.setPointerCapture(ev.pointerId);
      if (activePointers.size === 1) {
        dragging = true;
        lastX = ev.clientX;
        lastY = ev.clientY;
      } else if (activePointers.size === 2) {
        const points = Array.from(activePointers.values());
        const rect = canvas.getBoundingClientRect();
        const centerX = (points[0].x + points[1].x) / 2 - rect.left;
        const centerY = (points[0].y + points[1].y) / 2 - rect.top;
        pinch = {
          distance: Math.max(1, Math.hypot(points[1].x - points[0].x, points[1].y - points[0].y)),
          scale,
          worldX: wx(centerX),
          worldY: wy(centerY),
        };
        gestureWasPinch = true;
        dragging = false;
        dragMoved = true;
        hideTip();
      }
    }
    // Hover hit-testing is spatially indexed, but pointer hardware can still
    // emit faster than paint. Queue the latest position at most once per frame.
    let hoverQueued = false;
    let hoverPx = 0, hoverPy = 0;
    function onMove(ev) {
      const rect = canvas.getBoundingClientRect();
      const px = ev.clientX - rect.left, py = ev.clientY - rect.top;
      if (activePointers.has(ev.pointerId)) {
        activePointers.set(ev.pointerId, { x: ev.clientX, y: ev.clientY });
      }
      if (pinch && activePointers.size >= 2) {
        const points = Array.from(activePointers.values()).slice(0, 2);
        const distance = Math.max(1, Math.hypot(points[1].x - points[0].x, points[1].y - points[0].y));
        const centerX = (points[0].x + points[1].x) / 2 - rect.left;
        const centerY = (points[0].y + points[1].y) / 2 - rect.top;
        scale = clamp(pinch.scale * distance / pinch.distance, fitScale * 0.75, fitScale * 60);
        panX = centerX - pinch.worldX * scale;
        panY = centerY - pinch.worldY * scale;
        requestDraw();
        return;
      }
      if (dragging) {
        const ddx = ev.clientX - lastX, ddy = ev.clientY - lastY;
        if (Math.abs(ddx) + Math.abs(ddy) > 2) dragMoved = true;
        panX += ddx; panY += ddy;
        lastX = ev.clientX; lastY = ev.clientY;
        hideTip();
        requestDraw();
        return;
      }
      hoverPx = px; hoverPy = py;
      if (hoverQueued) return;
      hoverQueued = true;
      requestAnimationFrame(() => { hoverQueued = false; resolveHover(hoverPx, hoverPy); });
    }
    function resolveHover(px, py) {
      const aggregates = N > 200 && zoomFactor() < DETAIL_END;
      if (aggregates) {
        const a = appAt(px, py);
        if (a !== hoverApp) { hoverApp = a; requestDraw(); }
        if (a >= 0) showTip(px, py, `app · ${apps[a].size} coins`, 'click to zoom in');
        else hideTip();
        canvas.style.cursor = a >= 0 ? 'pointer' : 'grab';
        hoverNode = -1;
      } else {
        const i = nodeAt(px, py);
        const prevApp = hoverApp;
        if (i !== hoverNode) {
          hoverNode = i;
          hoverApp = i >= 0 ? na[i] : -1;
          requestDraw();
        } else if (i < 0 && prevApp >= 0) { hoverApp = -1; requestDraw(); }
        if (i >= 0) {
          const tname = nt[i] >= 0 ? templates[nt[i]] : 'unrecognized';
          const identified = hasIdentity(ids[i]);
          showTip(
            px,
            py,
            identified ? friendlyName(ids[i]) : 'coin identity loading…',
            `${tname} · ${ns[i] ? 'active' : 'burned'}`,
          );
          canvas.style.cursor = identified ? 'pointer' : 'default';
        } else { hideTip(); canvas.style.cursor = 'grab'; }
      }
    }
    function onUp(ev) {
      const fromCanvas = pointerFromCanvas;
      activePointers.delete(ev.pointerId);
      if (gestureWasPinch) {
        dragging = false;
        if (activePointers.size === 0) {
          gestureWasPinch = false;
          pinch = null;
          pointerFromCanvas = false;
        }
        return;
      }
      pointerFromCanvas = false;
      dragging = false;
      // ignore pointerups that didn't start on the canvas, and anything that
      // arrives while the canvas isn't actually on screen (hidden view)
      if (!fromCanvas || !canvas.isConnected || canvas.offsetParent === null) return;
      const rect = canvas.getBoundingClientRect();
      const px = ev.clientX - rect.left, py = ev.clientY - rect.top;
      if (dragMoved) return;
      // a real click
      if (N > 200 && zoomFactor() < DETAIL_END) {
        const a = appAt(px, py);
        if (a >= 0) { zoomToApp(a); return; }
        // Nodes are not painted in aggregate LOD, so an empty-space click
        // must stay empty instead of opening a hidden coin underneath it.
        return;
      }
      const i = nodeAt(px, py);
      if (i >= 0 && hasIdentity(ids[i])) onPickCoin(ids[i]);
    }

    function zoomToApp(a) {
      const app = apps[a];
      // Keep this first focus step inside the aggregate/detail blend. The old
      // 19x floor / 36x cap made small edge clusters erase all surrounding
      // context in a single click.
      const target = N > 200
        ? appFocusScale(app.r, fitScale, W, H, scale)
        : Math.max(scale, fitScale * 1.15);
      animateTo(target, app.cx, app.cy);
    }
    function animateTo(toScale, worldCx, worldCy, duration) {
      stopWheel();
      stopProgrammaticAnim();
      const from = { scale, panX, panY };
      toScale = clamp(toScale, fitScale * 0.75, fitScale * 60);
      const toPanX = W / 2 - worldCx * toScale;
      const toPanY = H / 2 - worldCy * toScale;
      anim = { t0: performance.now(), dur: duration || 520, from, to: { scale: toScale, panX: toPanX, panY: toPanY } };
      animRaf = requestAnimationFrame(stepAnim);
    }
    function stepAnim(now) {
      animRaf = 0;
      if (!anim) return;
      const t = Math.min(1, (now - anim.t0) / anim.dur);
      const e = 1 - Math.pow(1 - t, 3); // smooth, responsive ease-out cubic
      scale = anim.from.scale + (anim.to.scale - anim.from.scale) * e;
      panX = anim.from.panX + (anim.to.panX - anim.from.panX) * e;
      panY = anim.from.panY + (anim.to.panY - anim.from.panY) * e;
      draw();
      if (t < 1) animRaf = requestAnimationFrame(stepAnim); else anim = null;
    }
    function stopProgrammaticAnim() {
      if (animRaf) cancelAnimationFrame(animRaf);
      animRaf = 0;
      anim = null;
    }

    function showTip(px, py, title, sub) {
      tip.innerHTML = `<strong>${escapeHtml(title)}</strong>${sub ? `<span>${escapeHtml(sub)}</span>` : ''}`;
      tip.style.display = 'block';
      const left = clamp(px + 14, 8, Math.max(8, W - tip.offsetWidth - 8));
      const top = clamp(py + 14, 8, Math.max(8, H - tip.offsetHeight - 8));
      tip.style.left = left + 'px';
      tip.style.top = top + 'px';
    }
    function hideTip() { tip.style.display = 'none'; }
    function escapeHtml(s) { return String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c])); }

    // ---- public controls ----
    function setFilter(f) { filter = Object.assign(filter, f); applyFilter(); requestDraw(); }
    function setColorMode(m) {
      if (m === colorMode) return;
      colorMode = m;
      // Cache keys are compact integers and overlap across color modes.
      orbCache.clear();
      nebCache.clear();
      flatCache = [];
      requestDraw();
    }
    function search(query) {
      const q = (query || '').trim().toLowerCase();
      if (!q) {
        focusNode = -1;
        hoverNode = -1;
        hoverApp = -1;
        hideTip();
        requestDraw();
        return null;
      }
      for (let i = 0; i < N; i++) {
        if (hasIdentity(ids[i]) &&
            (ids[i].toLowerCase().startsWith(q) || friendlyName(ids[i]).toLowerCase().includes(q))) {
          filter = Object.assign(filter, {}); // no filter change
          focusNode = i;
          hoverNode = i;
          hoverApp = na[i];
          animateTo(Math.max(scale, fitScale * LABEL_DETAIL), nx[i], ny[i]);
          return ids[i];
        }
      }
      return null;
    }
    function focus(id) {
      for (let i = 0; i < N; i++) if (ids[i] === id) {
        focusNode = i;
        animateTo(Math.max(scale, fitScale * LABEL_DETAIL), nx[i], ny[i]);
        return true;
      }
      return false;
    }
    function zoom(command) {
      const centerX = wx(W / 2), centerY = wy(H / 2);
      hoverNode = -1;
      hoverApp = -1;
      hideTip();
      canvas.style.cursor = 'grab';
      if (command === 'reset') {
        focusNode = -1;
        const cx = bounds.minx + Math.max(1, bounds.w) / 2;
        const cy = bounds.miny + Math.max(1, bounds.h) / 2;
        animateTo(fitScale, cx, cy, 460);
      } else {
        animateTo(scale * (command === 'in' ? 1.65 : 1 / 1.65), centerX, centerY, 260);
      }
    }
    function resize() {
      const hadSize = W > 0 && H > 0;
      const oldCenterX = hadSize ? wx(W / 2) : 0;
      const oldCenterY = hadSize ? wy(H / 2) : 0;
      const oldZoom = hadSize && fitScale > 0 ? scale / fitScale : 1;
      W = canvas.clientWidth || 600;
      H = canvas.clientHeight || 420;
      dpr = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.round(W * dpr);
      canvas.height = Math.round(H * dpr);
      // dpr / viewport changed: sprites and background layers are stale
      orbCache.clear();
      nebCache.clear();
      buildBackground();
      computeFit();
      if (hadSize) {
        scale = fitScale * clamp(oldZoom, 0.75, 60);
        panX = W / 2 - oldCenterX * scale;
        panY = H / 2 - oldCenterY * scale;
      }
      draw();
    }

    // ---- wire events ----
    canvas.style.cursor = 'grab';
    canvas.addEventListener('wheel', onWheel, { passive: false });
    canvas.addEventListener('pointerdown', onDown);
    canvas.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onCancel);
    canvas.addEventListener('pointerenter', startAmbient);
    canvas.addEventListener('pointerleave', onLeave);
    function onLeave() {
      hoverNode = -1;
      hoverApp = -1;
      hideTip();
      canvas.style.cursor = 'grab';
      stopAmbient();
    }
    function onCancel() {
      pointerFromCanvas = false;
      dragging = false;
      dragMoved = false;
      activePointers.clear();
      pinch = null;
      gestureWasPinch = false;
      hoverNode = -1;
      hoverApp = -1;
      hideTip();
      canvas.style.cursor = 'grab';
      requestDraw();
    }

    function destroy() {
      destroyed = true;
      canvas.removeEventListener('wheel', onWheel);
      canvas.removeEventListener('pointerdown', onDown);
      canvas.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointercancel', onCancel);
      canvas.removeEventListener('pointerenter', startAmbient);
      canvas.removeEventListener('pointerleave', onLeave);
      stopProgrammaticAnim();
      stopWheel();
      ambientOn = false;
      if (ambientRaf) cancelAnimationFrame(ambientRaf);
      ambientRaf = 0;
      orbCache.clear();
      nebCache.clear();
      flatCache = [];
      bgVignette = null; starFar = null; starNear = null;
      bgGen++; // cancels any in-flight blob encode
      bgUrls.forEach((u) => URL.revokeObjectURL(u));
      bgUrls = [];
      canvas.style.background = ''; // resets image/color/repeat/size/position
      if (tip.parentElement) tip.parentElement.removeChild(tip);
    }

    // debug-only frame timer (not part of the public contract): average
    // draw() cost over n synchronous frames — used to police the perf budget
    function _bench(n) {
      const count = n || 60;
      const t0 = performance.now();
      for (let i = 0; i < count; i++) draw();
      return (performance.now() - t0) / count;
    }
    function _debug() {
      return {
        nodes: N,
        identities: identitiesLoaded,
        apps: apps.length,
        zoom: zoomFactor(),
        frame: Object.assign({}, frameStats),
        hoverNode,
        hoverApp,
        focusNode,
      };
    }

    const colorForTemplate = (i) => (i >= 0 && i < tplColors.length ? tplColors[i] : UNKNOWN_COLOR);
    return {
      load, loadVisual, setFilter, setColorMode, search, focus, zoom, resize, destroy,
      templates: () => templates, colorForTemplate, _bench, _debug,
    };
  }

  window.kascovGalaxy = {
    create,
    _appFocusScale: appFocusScale,
    _hasIdentity: hasIdentity,
    _visualIds: visualIds,
  };
})();
