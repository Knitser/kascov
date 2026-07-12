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
*/
(() => {
  'use strict';
  const TAU = Math.PI * 2;
  const PALETTE = ['#5be49b', '#8ab4ff', '#ffb067', '#c792ea', '#f78c6c', '#49eacb', '#89ddff', '#e4c05b'];
  const UNKNOWN_COLOR = 'rgba(150,160,180,0.85)';
  const ACTIVE_COLOR = '#5be49b';
  const BURNED_COLOR = 'rgba(130,140,160,0.5)';

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
    let colorMode = 'template'; // 'template' | 'status'
    let filter = { status: 'all', minSize: 2, template: null };
    let visible = null; // Uint8Array — passes the filter

    let dpr = Math.min(window.devicePixelRatio || 1, 2);
    let W = 0, H = 0;
    let scale = 1, fitScale = 1, panX = 0, panY = 0;
    let hoverNode = -1, hoverApp = -1;
    let dragging = false, dragMoved = false, lastX = 0, lastY = 0;
    let rafPending = false;
    let anim = null; // {t0, dur, from:{scale,panX,panY}, to:{…}}

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
      const pad = 40;
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

    // ---- draw ----
    function requestDraw() {
      if (rafPending) return;
      rafPending = true;
      requestAnimationFrame(() => { rafPending = false; draw(); });
    }

    function draw() {
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, W, H);
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

      // coin nodes (culled). Labels only render deep-zoom AND spaced out —
      // in dense mega-clusters hundreds of adjacent names smear into noise,
      // so each label reserves a screen-space cell and neighbors stay quiet
      // (hover still names any dot at any zoom).
      const labels = zf >= 10;
      const labelCells = labels ? new Set() : null;
      let drawnLabels = 0;
      for (let i = 0; i < N; i++) {
        if (!visible[i]) continue;
        const x = nx[i], y = ny[i];
        if (x < vx0 || x > vx1 || y < vy0 || y > vy1) continue;
        const px = sx(x), py = sy(y);
        const r = Math.max(1.5, nr[i] * Math.min(1.4, scale / fitScale * 0.6 + 0.4));
        ctx.beginPath();
        ctx.arc(px, py, r, 0, TAU);
        ctx.fillStyle = colorFor(i);
        ctx.globalAlpha = hoverApp >= 0 && na[i] !== hoverApp ? 0.28 : 1;
        ctx.fill();
        if (i === hoverNode) {
          ctx.globalAlpha = 1;
          ctx.lineWidth = 2;
          ctx.strokeStyle = '#fff';
          ctx.stroke();
        }
        ctx.globalAlpha = 1;
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
      drawHud();
    }

    function drawApps(vx0, vy0, vx1, vy1) {
      for (let a = 0; a < apps.length; a++) {
        const app = apps[a];
        if (filter.minSize > 2 && app.size < filter.minSize) continue;
        if (app.cx < vx0 - app.r || app.cx > vx1 + app.r || app.cy < vy0 - app.r || app.cy > vy1 + app.r) continue;
        const px = sx(app.cx), py = sy(app.cy);
        const r = Math.max(2, app.r * scale * 0.5);
        ctx.beginPath();
        ctx.arc(px, py, r, 0, TAU);
        ctx.fillStyle = colorMode === 'status' ? 'rgba(120,180,150,0.55)'
          : (app.t >= 0 ? withAlpha(tplColors[app.t], 0.55) : 'rgba(150,160,180,0.4)');
        ctx.fill();
        if (a === hoverApp) { ctx.lineWidth = 1.5; ctx.strokeStyle = '#49eacb'; ctx.stroke(); }
      }
    }

    function drawEdges(vx0, vy0, vx1, vy1, near) {
      ctx.lineWidth = 1;
      const M = edges.length / 3;
      for (let k = 0; k < M; k++) {
        const i = edges[k * 3], j = edges[k * 3 + 1];
        const hot = hoverApp >= 0 && (na[i] === hoverApp || na[j] === hoverApp);
        if (!near && !hot) continue;
        if (!visible[i] || !visible[j]) continue;
        const ax = nx[i], ay = ny[i], bx = nx[j], by = ny[j];
        // cull edges fully outside the viewport
        if ((ax < vx0 && bx < vx0) || (ax > vx1 && bx > vx1) || (ay < vy0 && by < vy0) || (ay > vy1 && by > vy1)) continue;
        ctx.strokeStyle = hot ? 'rgba(73,234,203,0.5)' : 'rgba(120,200,180,0.12)';
        ctx.beginPath();
        ctx.moveTo(sx(ax), sy(ay));
        ctx.lineTo(sx(bx), sy(by));
        ctx.stroke();
      }
    }

    function drawHud() {
      // subtle "zoom to explore" hint at far zoom
      if (zoomFactor() < 2.2 && apps.length) {
        ctx.fillStyle = 'rgba(160,170,190,0.5)';
        ctx.font = '12px ui-monospace, monospace';
        ctx.fillText('scroll to zoom · drag to pan · click a dot to open a coin', 14, H - 14);
      }
    }

    function colorFor(i) {
      if (colorMode === 'status') return ns[i] ? ACTIVE_COLOR : BURNED_COLOR;
      return nt[i] >= 0 ? tplColors[nt[i]] : UNKNOWN_COLOR;
    }
    function withAlpha(c, a) {
      // accept #rrggbb only; otherwise return as-is
      if (typeof c === 'string' && c[0] === '#' && c.length === 7) {
        const r = parseInt(c.slice(1, 3), 16), g = parseInt(c.slice(3, 5), 16), b = parseInt(c.slice(5, 7), 16);
        return `rgba(${r},${g},${b},${a})`;
      }
      return c;
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
      computeFit();
      draw();
    }

    // ---- wire events ----
    canvas.style.cursor = 'grab';
    canvas.addEventListener('wheel', onWheel, { passive: false });
    canvas.addEventListener('pointerdown', onDown);
    canvas.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    canvas.addEventListener('pointerleave', hideTip);

    function destroy() {
      canvas.removeEventListener('wheel', onWheel);
      canvas.removeEventListener('pointerdown', onDown);
      canvas.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      canvas.removeEventListener('pointerleave', hideTip);
      anim = null;
      if (tip.parentElement) tip.parentElement.removeChild(tip);
    }

    const colorForTemplate = (i) => (i >= 0 && i < tplColors.length ? tplColors[i] : UNKNOWN_COLOR);
    return { load, setFilter, setColorMode, search, focus, resize, destroy, templates: () => templates, colorForTemplate };
  }

  window.kascovGalaxy = { create };
})();
