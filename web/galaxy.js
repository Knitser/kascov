/* kascov — the whole-network "galaxy" App Graph. One zoomable/pannable map of
   every multi-contract app (union-find clusters of covenants that moved
   together). Positions + weighted edges are precomputed by the worker
   (`/data/<net>/galaxy.json`), so the browser never runs a force sim: we just
   pan/zoom a static world with viewport culling and level-of-detail. Pure
   Canvas 2D, no deps. Redraws only on interaction (idle CPU = 0).

   Data shape (index-addressed, integer world coords):
     { bounds:{minx,miny,w,h}, templates:[name…],
       apps:[{cx,cy,r,size,t}…],                  // one super-node per cluster
       nodes:[{id,t,s,x,y,r,a}…],                 // t=template idx (-1=none), s=1 active
       edges:[[i,j,w]…] }                          // pairwise, weighted
   Columnar shape (?fmt=2, feature-detected by `ids`): nodes[] is replaced by
   parallel arrays ids/nx/ny/nr/nt/ns/na that map 1:1 onto our typed arrays,
   and apps[] by acx/acy/ar/asz/at (mirroring cx/cy/r/size/t).
   Either shape may be the reduced core tier (tier:'core', big clusters only);
   load(full, {preserveView:true}) hot-swaps the full set without moving the
   camera (the worker keeps positions + bounds identical across tiers).

   Rendering ("deep space, alive"): a pre-rendered vignette + two parallax
   starfield layers ground the scene as css background layers on the canvas
   element (compositor-blended: zero per-frame raster cost); sparse views
   draw nodes as pre-rendered glowing orb sprites (offscreen canvases keyed
   by color × size-bucket × status, halo extent tiered by screen size, no
   per-node shadowBlur, ever) while dense views (>2.5k culled-in orbs) fall
   back to flat discs on the canvas oval fast path; far-mode app aggregates
   are cached nebula-cloud sprites for the few big clusters and batched dim
   tinted dots for the thousands of small ones; edges are subtly bowed
   quadratics with a two-pass fake gradient. A gentle twinkle loop runs ONLY
   while the pointer is over the canvas (and never under
   prefers-reduced-motion) — the default idle state stays event-driven with
   zero CPU. */
(() => {
  'use strict';
  const TAU = Math.PI * 2;
  const PALETTE = ['#5be49b', '#8ab4ff', '#ffb067', '#c792ea', '#f78c6c', '#49eacb', '#89ddff', '#e4c05b'];
  const UNKNOWN_COLOR = 'rgba(150,160,180,0.85)';
  const ACTIVE_COLOR = '#5be49b';
  const BURNED_COLOR = 'rgba(130,140,160,0.5)';
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
    let hoverNode = -1, hoverApp = -1;
    let dragging = false, dragMoved = false, lastX = 0, lastY = 0;
    let rafPending = false;
    let destroyed = false; // a destroyed controller must never paint again
                           // (a queued rAF draw would smear its stale view
                           // over whatever now owns the canvas)
    let anim = null; // {t0, dur, from:{scale,panX,panY}, to:{…}}

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
        requestDraw();
      } else {
        resize(); // sizes the backing store, computes fit, draws
      }
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
          apps[i] = { cx: d.acx[i], cy: d.acy[i], r: d.ar[i], size: d.asz[i], t: d.at[i] };
        }
      } else {
        apps = d.apps || [];
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
    const isFar = () => N > 200 && zoomFactor() < 2.2;

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
      const ci = colorMode === 'status' ? 0 : (app.t >= 0 ? app.t + 3 : 2);
      const variant = a & 3;
      const key = ci * 1024 + bi * 4 + variant;
      let spr = nebCache.get(key);
      if (!spr) {
        const color = colorMode === 'status' ? 'rgba(120,180,150,0.9)'
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
      if (!N && !apps.length) return;

      const zf = zoomFactor();
      const far = isFar();
      const near = zf >= 6 || (N <= 200 && N > 0);
      // visible world rect (for culling)
      const vx0 = wx(-20), vy0 = wy(-20), vx1 = wx(W + 20), vy1 = wy(H + 20);

      if (far) {
        drawApps(vx0, vy0, vx1, vy1);
        drawHud();
        return;
      }

      // edges: the hovered app's connections always; all in-view edges when near
      if (edges && (near || hoverApp >= 0)) drawEdges(vx0, vy0, vx1, vy1, near);

      // coin nodes (culled) as pre-rendered glowing orbs. Labels only render
      // deep-zoom AND spaced out — in dense mega-clusters hundreds of
      // adjacent names smear into noise, so each label reserves a
      // screen-space cell and neighbors stay quiet (hover still names any
      // dot at any zoom).
      const labels = zf >= 10;
      const labelCells = labels ? new Set() : null;
      let drawnLabels = 0;
      const mBase = Math.min(1.4, scale / fitScale * 0.6 + 0.4);
      // small networks: scale sprites up so a handful of coins reads as
      // intentional celestial bodies, not lost pixels
      const boost = N <= 200 ? 1.6 : 1;
      const twinkle = ambientOn && !reduceMotion.matches;
      const tNow = twinkle ? performance.now() : 0;
      // Pre-count the culled set (cheap: compares only). Past ~2.5k visible
      // orbs a frame, halo sprites lose on every renderer — the padded
      // quads blow the fill-rate budget and their stacked low-alpha halos
      // fuse dense rings into milky lace. Dense frames therefore draw flat
      // discs through the canvas single-arc "oval" fast path (per-node
      // beginPath/arc/fill — measured ~2x FASTER to rasterize than one big
      // batched Path2D, which drops off the fast path); sparse frames (deep
      // zoom, small networks, tight filters) keep the full glow sprites,
      // where they're already cheap and visibly worth it.
      let visCount = 0;
      for (let i = 0; i < N; i++) {
        if (!visible[i]) continue;
        const x = nx[i], y = ny[i];
        if (x < vx0 || x > vx1 || y < vy0 || y > vy1) continue;
        visCount++;
      }
      const denseMode = visCount > 2500;
      for (let i = 0; i < N; i++) {
        if (!visible[i]) continue;
        const x = nx[i], y = ny[i];
        if (x < vx0 || x > vx1 || y < vy0 || y > vy1) continue;
        const px = sx(x), py = sy(y);
        const rr = Math.max(1.5, nr[i] * mBase) * boost;
        const dimmed = hoverApp >= 0 && na[i] !== hoverApp;
        // slow sinusoidal twinkle on ~2% of nodes, seeded by index — only
        // while the ambient (pointer-over) loop is running
        const shimmer = twinkle && ((i * 2654435761 >>> 0) % 47) === 3;
        let alpha = dimmed ? 0.25 : 1;
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
        if (labels && drawnLabels < 24 && r >= 3) {
          /* one label per cell WIDER than a worst-case name (~26 chars at
             12px mono ≈ 190px) so neighbors can't overlap, and a dark halo
             so text stays readable over dense ring clusters */
          const cell = `${Math.floor(px / 240)},${Math.floor(py / 30)}`;
          if (!labelCells.has(cell)) {
            labelCells.add(cell);
            ctx.font = '12px ui-monospace, monospace';
            ctx.lineWidth = 3.5;
            ctx.strokeStyle = 'rgba(4,12,10,0.9)';
            ctx.strokeText(friendlyName(ids[i]), px + r + 4, py + 4);
            ctx.fillStyle = 'rgba(230,240,248,0.95)';
            ctx.fillText(friendlyName(ids[i]), px + r + 4, py + 4);
            drawnLabels++;
          }
        }
      }
      ctx.globalAlpha = 1;
      drawHud();
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

    function drawApps(vx0, vy0, vx1, vy1) {
      const twinkle = ambientOn && !reduceMotion.matches;
      // Tiny aggregates (screen radius < 4px) don't rate a nebula sprite —
      // at TN10 scale that would be thousands of overlapping 30–60px quads
      // fusing into a moiré fabric while the blits eat the frame budget.
      // They batch into three Path2D passes of plain dim dots (slight tint
      // variation per template group, no per-dot styles); only the few big
      // clusters get the cloud treatment. A hovered tiny app still takes the
      // sprite path so the hover ring/tooltip behavior is unchanged.
      let dotPaths = null;
      for (let a = 0; a < apps.length; a++) {
        const app = apps[a];
        if (filter.minSize > 2 && app.size < filter.minSize) continue;
        if (app.cx < vx0 - app.r || app.cx > vx1 + app.r || app.cy < vy0 - app.r || app.cy > vy1 + app.r) continue;
        const px = sx(app.cx), py = sy(app.cy);
        const rs = app.r * scale * 0.5;
        if (rs < 4 && a !== hoverApp) {
          if (!dotPaths) dotPaths = [new Path2D(), new Path2D(), new Path2D()];
          // tint group: template hue + a deterministic per-app scatter —
          // on dominant-template networks (TN10: 96% one template) a pure
          // template split would collapse the whole field to one flat color
          const p = dotPaths[((app.t >= 0 ? app.t : 2) + (((a * 2654435761) >>> 8) % 3)) % 3];
          // single rect per dot (an arc costs 2 path calls and reads the
          // same at <=4px); dim + tiny = starlike
          const dr = 0.7 + Math.min(1.5, rs * 0.35);
          p.rect(px - dr, py - dr, dr * 2, dr * 2);
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
      const hots = hoverApp >= 0 ? [] : null;
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
          if (hots && (na[i] === hoverApp || na[j] === hoverApp)) { hots.push(k); continue; }
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
            if (hots && (na[i] === hoverApp || na[j] === hoverApp)) continue;
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
          if (na[i] !== hoverApp && na[j] !== hoverApp) continue;
          if (!visible[i] || !visible[j]) continue;
          const ax = nx[i], ay = ny[i], bx = nx[j], by = ny[j];
          if ((ax < vx0 && bx < vx0) || (ax > vx1 && bx > vx1) || (ay < vy0 && by < vy0) || (ay > vy1 && by > vy1)) continue;
          hots.push(k);
        }
      }
      if (hots && hots.length) {
        // hot (hovered app) edges glow teal: wide dim underlay + bright
        // core on the full curve + brighter mid-segment. Few edges (one
        // app's connections), so per-edge style switches are fine.
        const t0 = 0.28, t1 = 0.72, u0 = 1 - t0, u1 = 1 - t1;
        for (let n = 0; n < hots.length; n++) {
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
      if (zoomFactor() < 2.2 && apps.length) {
        ctx.fillStyle = 'rgba(150,165,185,0.38)';
        ctx.font = '11px ui-monospace, monospace';
        const prevLs = ctx.letterSpacing;
        if (prevLs !== undefined) ctx.letterSpacing = '0.08em';
        ctx.fillText('scroll to zoom · drag to pan · click a dot to open a coin', 14, H - 14);
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
      if (isFar() && hoverApp < 0) return;
      if (!N && !apps.length) return;
      lastAmbient = t;
      requestDraw();
    }

    // ---- hit testing (linear; redraw/hover are event-driven) ----
    function nodeAt(px, py) {
      let best = -1, bd = 16 * 16;
      for (let i = 0; i < N; i++) {
        if (!visible[i]) continue;
        const dx = sx(nx[i]) - px, dy = sy(ny[i]) - py, d2 = dx * dx + dy * dy;
        const rr = Math.max(6, nr[i]) ** 2;
        if (d2 < Math.max(bd, rr)) { bd = d2; best = i; }
      }
      return best;
    }
    function appAt(px, py) {
      let best = -1, bd = Infinity;
      for (let a = 0; a < apps.length; a++) {
        const app = apps[a];
        if (filter.minSize > 2 && app.size < filter.minSize) continue;
        const dx = sx(app.cx) - px, dy = sy(app.cy) - py, d2 = dx * dx + dy * dy;
        const r = Math.max(6, app.r * scale * 0.5);
        if (d2 < r * r && d2 < bd) { bd = d2; best = a; }
      }
      return best;
    }

    // ---- interaction ----
    function onWheel(ev) {
      ev.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const px = ev.clientX - rect.left, py = ev.clientY - rect.top;
      const wpx = wx(px), wpy = wy(py);
      const factor = Math.exp(-ev.deltaY * 0.0016);
      scale = Math.min(fitScale * 60, Math.max(fitScale * 0.6, scale * factor));
      panX = px - wpx * scale;
      panY = py - wpy * scale;
      requestDraw();
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
      dragging = true; dragMoved = false;
      lastX = ev.clientX; lastY = ev.clientY;
      canvas.setPointerCapture && canvas.setPointerCapture(ev.pointerId);
    }
    // Hover hit-testing is O(N) over every node; at tens of thousands of
    // coins an unthrottled pointermove burns a linear scan per event. Queue
    // the latest position and resolve it at most once per frame.
    let hoverQueued = false;
    let hoverPx = 0, hoverPy = 0;
    function onMove(ev) {
      const rect = canvas.getBoundingClientRect();
      const px = ev.clientX - rect.left, py = ev.clientY - rect.top;
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
      const far = isFar();
      if (far) {
        const a = appAt(px, py);
        if (a !== hoverApp) { hoverApp = a; requestDraw(); }
        if (a >= 0) showTip(px, py, `app · ${apps[a].size} coins`, 'click to zoom in');
        else hideTip();
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
          showTip(px, py, friendlyName(ids[i]), `${tname} · ${ns[i] ? 'active' : 'burned'}`);
          canvas.style.cursor = 'pointer';
        } else { hideTip(); canvas.style.cursor = 'grab'; }
      }
    }
    function onUp(ev) {
      const fromCanvas = pointerFromCanvas;
      pointerFromCanvas = false;
      dragging = false;
      // ignore pointerups that didn't start on the canvas, and anything that
      // arrives while the canvas isn't actually on screen (hidden view)
      if (!fromCanvas || !canvas.isConnected || canvas.offsetParent === null) return;
      const rect = canvas.getBoundingClientRect();
      const px = ev.clientX - rect.left, py = ev.clientY - rect.top;
      if (dragMoved) return;
      // a real click
      if (isFar()) {
        const a = appAt(px, py);
        if (a >= 0) { zoomToApp(a); return; }
      }
      const i = nodeAt(px, py);
      if (i >= 0) onPickCoin(ids[i]);
    }

    function zoomToApp(a) {
      const app = apps[a];
      // Fit the app's ACTUAL member spread, not the layout radius — on tiny
      // networks (mainnet today) ar understates the extent and the old
      // fitScale*5 floor zoomed past both members into empty space.
      let extent = Math.max(app.r, 8);
      for (let i = 0; i < N; i++) {
        if (na[i] !== a) continue;
        const d = Math.hypot(nx[i] - app.cx, ny[i] - app.cy) + nr[i];
        if (d > extent) extent = d;
      }
      const fit = (Math.min(W, H) * 0.4) / extent;
      // big worlds must land past the far->near threshold (2.2) or the zoom
      // strands the user between LODs; small worlds draw nodes at every zoom
      // so the extent fit wins as-is
      const floor = N > 200 ? fitScale * 2.5 : fitScale * 1.15;
      const target = Math.min(fitScale * 12, Math.max(floor, fit));
      animateTo(target, app.cx, app.cy);
    }
    function animateTo(toScale, worldCx, worldCy) {
      const from = { scale, panX, panY };
      const toPanX = W / 2 - worldCx * toScale;
      const toPanY = H / 2 - worldCy * toScale;
      anim = { t0: performance.now(), dur: 480, from, to: { scale: toScale, panX: toPanX, panY: toPanY } };
      stepAnim();
    }
    function stepAnim() {
      if (!anim) return;
      const t = Math.min(1, (performance.now() - anim.t0) / anim.dur);
      const e = t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2; // easeInOutQuad
      scale = anim.from.scale + (anim.to.scale - anim.from.scale) * e;
      panX = anim.from.panX + (anim.to.panX - anim.from.panX) * e;
      panY = anim.from.panY + (anim.to.panY - anim.from.panY) * e;
      draw();
      if (t < 1) requestAnimationFrame(stepAnim); else anim = null;
    }

    function showTip(px, py, title, sub) {
      tip.innerHTML = `<strong>${escapeHtml(title)}</strong>${sub ? `<span>${escapeHtml(sub)}</span>` : ''}`;
      tip.style.display = 'block';
      tip.style.left = px + 14 + 'px';
      tip.style.top = py + 14 + 'px';
    }
    function hideTip() { tip.style.display = 'none'; }
    function escapeHtml(s) { return String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c])); }

    // ---- public controls ----
    function setFilter(f) { filter = Object.assign(filter, f); applyFilter(); requestDraw(); }
    function setColorMode(m) { colorMode = m; requestDraw(); }
    function search(query) {
      const q = (query || '').trim().toLowerCase();
      if (!q) return null;
      for (let i = 0; i < N; i++) {
        if (ids[i].toLowerCase().startsWith(q) || friendlyName(ids[i]).toLowerCase().includes(q)) {
          filter = Object.assign(filter, {}); // no filter change
          hoverNode = i; hoverApp = na[i];
          animateTo(Math.max(scale, fitScale * 6), nx[i], ny[i]);
          return ids[i];
        }
      }
      return null;
    }
    function focus(id) {
      for (let i = 0; i < N; i++) if (ids[i] === id) { animateTo(Math.max(scale, fitScale * 6), nx[i], ny[i]); return true; }
      return false;
    }
    function resize() {
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
      draw();
    }

    // ---- wire events ----
    canvas.style.cursor = 'grab';
    canvas.addEventListener('wheel', onWheel, { passive: false });
    canvas.addEventListener('pointerdown', onDown);
    canvas.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    canvas.addEventListener('pointerenter', startAmbient);
    canvas.addEventListener('pointerleave', onLeave);
    function onLeave() { hideTip(); stopAmbient(); }

    function destroy() {
      destroyed = true;
      canvas.removeEventListener('wheel', onWheel);
      canvas.removeEventListener('pointerdown', onDown);
      canvas.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      canvas.removeEventListener('pointerenter', startAmbient);
      canvas.removeEventListener('pointerleave', onLeave);
      anim = null;
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

    const colorForTemplate = (i) => (i >= 0 && i < tplColors.length ? tplColors[i] : UNKNOWN_COLOR);
    return { load, setFilter, setColorMode, search, focus, resize, destroy, templates: () => templates, colorForTemplate, _bench };
  }

  window.kascovGalaxy = { create };
})();
