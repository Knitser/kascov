/* kascov — the living BlockDAG hero.
   A flowing lattice of Kaspa blocks: columns drift left, each block links to
   parents in the previous column, one selected-parent chain glows teal, and
   covenants ignite as lifecycle-colored nodes (born / moved / retired).
   Restrained on purpose — slow, soft, legible behind the headline. Honors
   reduced-motion with a static frame. Purely decorative (aria-hidden). */
(() => {
  'use strict';
  const cv = document.getElementById('dag-canvas');
  if (!cv || !cv.getContext) return;
  const ctx = cv.getContext('2d');
  const REDUCE = matchMedia('(prefers-reduced-motion: reduce)').matches;

  const COL_W = 96;          // px between columns
  const SPEED = 13;          // px/s drift
  const LIFE = { born: '#5be49b', move: '#8ab4ff', burn: '#ffb067' };
  const LIFE_KEYS = Object.keys(LIFE);
  const rnd = (a, b) => a + Math.random() * (b - a);

  let W = 0, H = 0, dpr = 1, cols = [], offset = 0, last = 0, raf = 0, seedChain = 0;

  function makeCol(x, prev) {
    const count = Math.round(rnd(2, 5));
    const top = H * 0.1, span = H * 0.8;
    const nodes = [];
    for (let i = 0; i < count; i++) {
      const y = top + (i + 0.5) / count * span + rnd(-1, 1) * (span / count) * 0.28;
      const parents = [];
      if (prev && prev.nodes.length) {
        const p1 = Math.floor(rnd(0, prev.nodes.length));
        parents.push(p1);
        if (Math.random() < 0.45 && prev.nodes.length > 1) {
          const p2 = Math.floor(rnd(0, prev.nodes.length));
          if (p2 !== p1) parents.push(p2);
        }
      }
      // an occasional covenant ignition
      const life = Math.random() < 0.14 ? LIFE_KEYS[Math.floor(rnd(0, LIFE_KEYS.length))] : null;
      nodes.push({ y, r: rnd(2.3, 4.1), parents, chain: false, life, born: performance.now() + rnd(-400, 0) });
    }
    // extend the selected-parent chain: pick the node whose parent is the prev chain node
    let chainIdx;
    if (prev) {
      const prevChain = prev.nodes.findIndex((n) => n.chain);
      const opts = nodes.map((n, i) => i).filter((i) => nodes[i].parents.includes(prevChain));
      chainIdx = opts.length ? opts[Math.floor(rnd(0, opts.length))] : Math.floor(rnd(0, nodes.length));
    } else {
      chainIdx = Math.floor(rnd(0, nodes.length));
    }
    nodes[chainIdx].chain = true;
    return { x, nodes };
  }

  function rebuild() {
    cols = [];
    offset = 0;
    const n = Math.ceil(W / COL_W) + 3;
    let prev = null;
    for (let i = 0; i < n; i++) {
      const c = makeCol(i * COL_W, prev);
      cols.push(c);
      prev = c;
    }
  }

  function resize() {
    dpr = Math.min(2, window.devicePixelRatio || 1);
    W = cv.clientWidth || cv.parentElement.clientWidth || 0;
    H = cv.clientHeight || cv.parentElement.clientHeight || 0;
    // hero not laid out yet (SPA view hidden) — wait, keep cols empty
    if (W < 40 || H < 40) { cols = []; return; }
    cv.width = Math.round(W * dpr);
    cv.height = Math.round(H * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    rebuild();
    if (REDUCE) draw(performance.now());
  }

  function nodeX(c) { return c.x - offset; }

  function draw(now) {
    ctx.clearRect(0, 0, W, H);

    // edges first
    ctx.lineWidth = 1;
    for (let ci = 1; ci < cols.length; ci++) {
      const c = cols[ci], p = cols[ci - 1];
      const cx = nodeX(c), px = nodeX(p);
      for (const node of c.nodes) {
        for (const pi of node.parents) {
          const par = p.nodes[pi];
          if (!par) continue;
          const isChain = node.chain && par.chain;
          ctx.strokeStyle = isChain ? 'rgba(73,234,203,0.32)' : 'rgba(120,170,160,0.10)';
          ctx.lineWidth = isChain ? 1.4 : 1;
          ctx.beginPath();
          ctx.moveTo(px, par.y);
          ctx.lineTo(cx, node.y);
          ctx.stroke();
        }
      }
    }

    // nodes
    for (const c of cols) {
      const x = nodeX(c);
      if (x < -20 || x > W + 20) continue;
      for (const node of c.nodes) {
        const appear = Math.min(1, (now - node.born) / 700);
        const baseR = node.r * appear;
        if (node.life) {
          // covenant ignition: filled lifecycle dot + slow ring
          const col = LIFE[node.life];
          const phase = (now / 2600 + node.y) % 1;
          ctx.globalAlpha = appear;
          ctx.fillStyle = col;
          ctx.shadowColor = col; ctx.shadowBlur = 12;
          ctx.beginPath(); ctx.arc(x, node.y, baseR + 0.6, 0, 7); ctx.fill();
          ctx.shadowBlur = 0;
          ctx.globalAlpha = appear * (1 - phase) * 0.5;
          ctx.strokeStyle = col; ctx.lineWidth = 1.2;
          ctx.beginPath(); ctx.arc(x, node.y, baseR + phase * 16, 0, 7); ctx.stroke();
          ctx.globalAlpha = 1;
        } else if (node.chain) {
          ctx.fillStyle = '#49eacb';
          ctx.shadowColor = '#49eacb'; ctx.shadowBlur = 9;
          ctx.globalAlpha = appear;
          ctx.beginPath(); ctx.arc(x, node.y, baseR, 0, 7); ctx.fill();
          ctx.shadowBlur = 0; ctx.globalAlpha = 1;
        } else {
          ctx.fillStyle = 'rgba(180,214,206,0.5)';
          ctx.globalAlpha = appear;
          ctx.beginPath(); ctx.arc(x, node.y, baseR, 0, 7); ctx.fill();
          ctx.globalAlpha = 1;
        }
      }
    }
  }

  function tick(now) {
    // build lazily once the hero is actually visible (survives SPA view
    // switches where the ResizeObserver may not have fired yet)
    if (!cols.length || !W) {
      resize();
      if (!cols.length) { last = now; raf = requestAnimationFrame(tick); return; }
    }
    const dt = Math.min(0.05, (now - last) / 1000 || 0);
    last = now;
    offset += SPEED * dt;
    // recycle: when the first column has fully exited, drop it and append one
    while (cols.length && nodeX(cols[0]) < -COL_W) {
      const first = cols.shift();
      offset -= COL_W;
      const lastCol = cols[cols.length - 1];
      cols.push(makeCol(lastCol.x + COL_W, lastCol));
    }
    draw(now);
    raf = requestAnimationFrame(tick);
  }

  const ro = new ResizeObserver(() => resize());
  ro.observe(cv);
  resize();
  if (!REDUCE) {
    last = performance.now();
    raf = requestAnimationFrame(tick);
    document.addEventListener('visibilitychange', () => {
      if (document.hidden) { cancelAnimationFrame(raf); raf = 0; }
      else if (!raf) { last = performance.now(); raf = requestAnimationFrame(tick); }
    });
  }
})();
