/* kascov — the blockDAG, breathing.
   A continuous simulation (not a looping sequence): blocks are born near the
   right edge at roughly Kaspa cadence, link back to 1-3 recent parents with
   curved edges, and the whole field drifts left — time flowing — until old
   blocks fade out past the edge. Two depth layers drift at different speeds
   for parallax. Occasionally a block flashes teal (accepted); rarely one
   ignites amber and dims. Restrained on purpose: dim strokes, soft sprite
   glows, a center-dark zone so the headline stays legible.

   Full-bleed: the canvas is re-parented from .hero (whose overflow:hidden
   clips it to the 1080px column) to the front of <body>, absolutely
   positioned over the hero's document rect at full viewport width — all
   styling done here in JS, nothing in style.css changes. Visibility is
   driven by an IntersectionObserver on the hero, so SPA view switches and
   scroll-out both pause the loop; visibilitychange handles hidden tabs;
   prefers-reduced-motion gets one static frame. Purely decorative
   (aria-hidden). Exports window.kascovDag = { kick } as before. */
(() => {
  'use strict';
  const cv = document.getElementById('dag-canvas');
  if (!cv || !cv.getContext) return;
  const hero = cv.parentElement;           // .hero — grab before re-parenting
  if (!hero) return;
  const ctx = cv.getContext('2d');
  const REDUCE = matchMedia('(prefers-reduced-motion: reduce)').matches;

  /* ---- full-bleed placement: escape .hero{overflow:hidden} ------------- */
  document.body.insertBefore(cv, document.body.firstChild);
  const st = cv.style;
  st.position = 'absolute';
  st.left = '0px';
  st.top = '0px';
  st.right = 'auto';
  st.bottom = 'auto';
  st.zIndex = '0';                         // hero copy is position:relative z:1
  st.pointerEvents = 'none';
  st.display = 'none';                     // shown when the hero intersects
  st.maskImage = 'none';                   // fades are drawn per-element
  st.webkitMaskImage = 'none';

  /* ---- tuning ----------------------------------------------------------- */
  const TAIL = 90;                         // glow spill below the hero (px)
  const LAYER_SPEED = [15, 24];            // px/s drift: back, front
  const AVG_SPEED = 20;
  const KIND_A = { slate: 0.34, teal: 0.42, blue: 0.36, amber: 0.30 };
  const RGB = {
    slate: [166, 204, 197],
    teal: [73, 234, 203],
    blue: [138, 180, 255],
    amber: [255, 176, 103],
  };
  const rnd = (a, b) => a + Math.random() * (b - a);

  /* one radial-gradient sprite per color, blitted at varying alpha/scale —
     never per-node shadowBlur */
  function makeSprite(c) {
    const s = document.createElement('canvas');
    s.width = s.height = 64;
    const g = s.getContext('2d');
    const grad = g.createRadialGradient(32, 32, 0, 32, 32, 32);
    grad.addColorStop(0, `rgba(${c[0]},${c[1]},${c[2]},1)`);
    grad.addColorStop(0.16, `rgba(${c[0]},${c[1]},${c[2]},0.85)`);
    grad.addColorStop(0.42, `rgba(${c[0]},${c[1]},${c[2]},0.22)`);
    grad.addColorStop(1, `rgba(${c[0]},${c[1]},${c[2]},0)`);
    g.fillStyle = grad;
    g.fillRect(0, 0, 64, 64);
    return s;
  }
  const SPR = {};
  for (const k in RGB) SPR[k] = makeSprite(RGB[k]);
  const EDGE = 'rgb(126,190,176)';
  const EDGE_HOT = 'rgb(73,234,203)';

  /* ---- state ------------------------------------------------------------ */
  let W = 0, H = 0, heroH = 0, dpr = 1, lastTop = -1;
  let blocks = [];                 // {wx,y0,bobA,bobF,bobP,bow,r,layer,parents,born,kind,flash,ring}
  const offset = [0, 0];           // per-layer world drift (px)
  let simT = 0;                    // simulation clock (s) — survives prefill
  let spawnClock = 0, nextSpawn = 0.1, pulseClock = 0, nextPulse = 2;
  let raf = 0, last = 0, heroVisible = false, frameCount = 0;
  const perf = { n: 0, sum: 0 };

  const targetCount = () => Math.min(110, Math.max(55, W / 18));
  const spawnBase = () => ((W + 80) / AVG_SPEED) / targetCount();
  const yOf = (b) => b.y0 + Math.sin(simT * b.bobF + b.bobP) * b.bobA;
  const sx = (b) => b.wx - offset[b.layer];

  /* per-element alpha: left-edge death, right-edge birth softness, bottom
     fade-out (no hard clip), and a soft center-dark ellipse under the
     headline so text contrast never suffers */
  function fade(x, y) {
    let f = 1;
    if (x < 90) f *= Math.max(0, x / 90);
    else if (x > W - 40) f *= Math.max(0, (W - x) / 40);
    const fy0 = heroH * 0.66, fy1 = heroH * 0.98;
    if (y > fy0) {
      const t = Math.min(1, (y - fy0) / (fy1 - fy0));
      f *= (1 - t) * (1 - t);
    }
    const dx = (x - W * 0.5) / (W * 0.30), dy = (y - heroH * 0.36) / (heroH * 0.44);
    const g = dx * dx + dy * dy;
    if (g < 1) { const q = 1 - g; f *= 1 - 0.55 * q * q; }
    return f;
  }

  function spawn(screenX) {
    const layer = Math.random() < 0.42 ? 0 : 1;
    const kindRoll = Math.random();
    const kind = kindRoll < 0.08 ? 'teal'
      : kindRoll < 0.11 ? 'amber'
      : kindRoll < 0.16 ? 'blue' : 'slate';
    /* parents: 1-3 recent same-layer blocks a plausible hop to the left */
    const cands = [];
    for (let i = blocks.length - 1, seen = 0; i >= 0 && seen < 60; i--, seen++) {
      const b = blocks[i];
      if (b.layer !== layer) continue;
      const dx = screenX - sx(b);
      if (dx > 24 && dx < 270) cands.push(b);
    }
    const parents = [];
    const want = cands.length ? 1 + (Math.random() < 0.55 ? 1 : 0) + (Math.random() < 0.18 ? 1 : 0) : 0;
    for (let i = 0; i < want && cands.length; i++) {
      const j = Math.floor(Math.random() * cands.length);
      parents.push(cands[j]);
      cands.splice(j, 1);
    }
    blocks.push({
      wx: screenX + offset[layer],
      y0: heroH * rnd(0.05, 0.85),
      bobA: rnd(1.4, 3.2),
      bobF: rnd(0.25, 0.7),
      bobP: rnd(0, 6.28),
      bow: Math.random() < 0.5 ? -1 : 1,
      r: rnd(2.1, 4.0) * (layer ? 1 : 0.72),
      layer, parents, kind,
      born: simT,
      flash: kind === 'slate' ? 0 : 1,
      ring: kind === 'teal' || kind === 'amber' ? 0 : -1,
    });
  }

  function step(dt) {
    simT += dt;
    offset[0] += LAYER_SPEED[0] * dt;
    offset[1] += LAYER_SPEED[1] * dt;

    spawnClock += dt;
    while (spawnClock >= nextSpawn) {
      spawnClock -= nextSpawn;
      nextSpawn = spawnBase() * rnd(0.55, 1.5);   // Kaspa cadence, jittered
      if (blocks.length < targetCount() * 1.3) spawn(W - 6 - Math.random() * 36);
    }

    /* occasionally an existing block gets "accepted": teal flash + ring */
    pulseClock += dt;
    if (pulseClock >= nextPulse) {
      pulseClock = 0;
      nextPulse = rnd(1.4, 3.4);
      for (let tries = 0; tries < 4 && blocks.length; tries++) {
        const b = blocks[Math.floor(Math.random() * blocks.length)];
        const x = sx(b);
        if (x > W * 0.15 && x < W * 0.92) { b.flash = 1; b.ring = 0; break; }
      }
    }

    /* decay + cull in one pass */
    let w = 0;
    for (let i = 0; i < blocks.length; i++) {
      const b = blocks[i];
      if (b.flash > 0) b.flash = Math.max(0, b.flash - dt / 1.6);
      if (b.ring >= 0) { b.ring += dt / 1.2; if (b.ring > 1) b.ring = -1; }
      if (sx(b) > -40) blocks[w++] = b;
    }
    blocks.length = w;
  }

  /* fast-forward one full traversal so the field never starts empty */
  function prefill() {
    blocks.length = 0;
    offset[0] = offset[1] = 0;
    spawnClock = 0;
    nextSpawn = 0.05;
    const total = (W + 100) / LAYER_SPEED[0];   // one full slow-layer traversal
    for (let t = 0; t < total; t += 0.25) step(0.25);
  }

  function draw() {
    ctx.clearRect(0, 0, W, H);

    /* edges first: thin curved links back to parents */
    ctx.lineWidth = 1;
    for (const b of blocks) {
      if (!b.parents.length) continue;
      const bx = sx(b);
      if (bx < -50 || bx > W + 50) continue;
      const by = yOf(b);
      const ta = Math.min(1, (simT - b.born) / 0.55);
      const fb = fade(bx, by) * ta * (2 - ta);
      if (fb <= 0.01) continue;
      const hot = b.flash > 0.35;
      const layerDim = b.layer ? 1 : 0.6;
      for (const p of b.parents) {
        const px = sx(p), py = yOf(p);
        const a = Math.min(fb, fade(px, py)) * (hot ? 0.30 : 0.12) * layerDim;
        if (a <= 0.008) continue;
        ctx.globalAlpha = a;
        ctx.strokeStyle = hot ? EDGE_HOT : EDGE;
        ctx.beginPath();
        ctx.moveTo(px, py);
        ctx.quadraticCurveTo((px + bx) / 2, (py + by) / 2 + (bx - px) * 0.12 * b.bow, bx, by);
        ctx.stroke();
      }
    }

    /* nodes: one sprite blit each (plus a teal overlay while flashing) */
    for (const b of blocks) {
      const x = sx(b);
      if (x < -30 || x > W + 30) continue;
      const y = yOf(b);
      const ta = Math.min(1, (simT - b.born) / 0.55);
      const f = fade(x, y) * ta * (2 - ta) * (b.layer ? 1 : 0.6);
      if (f <= 0.01) continue;
      const fl = b.flash * b.flash;
      const s = b.r * 7 * (1 + fl * 0.7);
      if (b.kind === 'slate') {
        ctx.globalAlpha = KIND_A.slate * f;
        ctx.drawImage(SPR.slate, x - s / 2, y - s / 2, s, s);
        if (fl > 0.01) {                       // acceptance: teal bloom on top
          ctx.globalAlpha = fl * 0.6 * f;
          ctx.drawImage(SPR.teal, x - s / 2, y - s / 2, s, s);
        }
      } else {
        ctx.globalAlpha = (KIND_A[b.kind] + fl * 0.5) * f;
        ctx.drawImage(SPR[b.kind], x - s / 2, y - s / 2, s, s);
      }
      if (b.ring >= 0) {                       // expanding acceptance ring
        const rr = b.r + b.ring * 22;
        ctx.globalAlpha = (1 - b.ring) * (1 - b.ring) * 0.35 * f;
        ctx.strokeStyle = b.kind === 'amber' ? 'rgb(255,176,103)' : EDGE_HOT;
        ctx.beginPath();
        ctx.arc(x, y, rr, 0, 6.2832);
        ctx.stroke();
      }
    }
    ctx.globalAlpha = 1;
  }

  /* ---- geometry: track the hero's document rect at viewport width ------- */
  function layout() {
    const r = hero.getBoundingClientRect();
    if (r.width < 40 || r.height < 40) return false;   // hidden / not laid out
    const vw = document.documentElement.clientWidth;
    const top = Math.round(r.top + window.scrollY);
    const hh = Math.round(r.height);
    if (vw !== W || hh !== heroH || top !== lastTop) {
      W = vw;
      heroH = hh;
      H = hh + TAIL;
      lastTop = top;
      st.top = top + 'px';
      st.width = W + 'px';
      st.height = H + 'px';
      dpr = Math.min(2, window.devicePixelRatio || 1);
      const bw = Math.round(W * dpr), bh = Math.round(H * dpr);
      if (cv.width !== bw || cv.height !== bh) {
        cv.width = bw;
        cv.height = bh;
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }
    return true;
  }

  function tick(now) {
    raf = requestAnimationFrame(tick);
    const t0 = performance.now();
    if (!layout()) return;                     // hero hidden; IO will stop us
    if (!blocks.length) prefill();
    const dt = Math.min(0.05, (now - last) / 1000 || 0.016);
    last = now;
    step(dt);
    draw();
    frameCount++;
    const ms = performance.now() - t0;
    perf.sum += ms;
    if (++perf.n > 240) { perf.sum = ms; perf.n = 1; }
  }

  function staticFrame() {
    if (!layout()) return;
    if (!blocks.length) prefill();
    draw();
  }

  /* Always (re)start a fresh loop — never trust a stale raf id (background-
     tab throttling / bfcache freezes once left the old loop unrevivable).
     No document.hidden gate here: hidden tabs never deliver rAF anyway, and
     the visibilitychange listener below cancels the loop the moment the tab
     actually hides. */
  function start() {
    if (!heroVisible) return;
    if (REDUCE) { staticFrame(); return; }
    if (raf) cancelAnimationFrame(raf);
    last = performance.now();
    raf = requestAnimationFrame(tick);
  }
  function stop() {
    if (raf) { cancelAnimationFrame(raf); raf = 0; }
  }
  /* force a re-measure + (re)start — the SPA calls views into being after
     this script runs, so geometry is always re-derived on wake */
  function kick() {
    if (heroVisible) layout();
    start();
  }

  /* the robust wake-up: fire the moment the hero is actually on screen,
     whatever code path revealed it (SPA route switch, scroll, load) — and
     park completely when it isn't */
  const io = new IntersectionObserver((es) => {
    for (const e of es) {
      heroVisible = e.isIntersecting;
      if (heroVisible) { st.display = 'block'; kick(); }
      else { st.display = 'none'; stop(); }
    }
  }, { threshold: 0.01 });
  io.observe(hero);

  /* hero resizes (fonts, stats loading, breakpoints) and content shifting
     above it (body height) both move our rect — re-measure */
  const ro = new ResizeObserver(() => {
    if (!heroVisible) return;
    layout();
    if (REDUCE) staticFrame();
  });
  ro.observe(hero);
  ro.observe(document.body);
  window.addEventListener('resize', () => {
    if (!heroVisible) return;
    layout();
    if (REDUCE) staticFrame();
  });

  document.addEventListener('visibilitychange', () => (document.hidden ? stop() : start()));
  /* bfcache restore / refocus resume (previously only a hard refresh did) */
  window.addEventListener('pageshow', start);
  window.addEventListener('focus', start);

  window.kascovDag = {
    kick,
    /* diagnostics (additive; app.js only ever knew about kick) */
    frames: () => frameCount,
    frameStats: () => ({
      frames: frameCount,
      running: !!raf,
      blocks: blocks.length,
      avgMs: perf.n ? perf.sum / perf.n : 0,
    }),
  };
})();
