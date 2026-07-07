/* kascov — a tiny canvas force-directed graph for covenant "apps". A family is
   a set of smart coins that shared transactions (union-found); we draw it as a
   hub-and-spokes cluster, let the springs settle it, and make each coin
   clickable. No deps. */
(() => {
  'use strict';

  const COLORS = ['#5be49b', '#8ab4ff', '#ffb067', '#49eacb', '#c792ea', '#f78c6c', '#89ddff'];
  // deterministic color from a covenant id
  function colorFor(id) {
    let h = 0;
    for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
    return COLORS[h % COLORS.length];
  }

  /* nodes: [{id, label, hub?}], edges: [[i,j]]. Returns a controller with
     .stop() and a click handler wired to onPick(node). */
  function render(canvas, family, opts) {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const W = canvas.clientWidth || 600;
    const H = canvas.clientHeight || 380;
    canvas.width = W * dpr;
    canvas.height = H * dpr;
    const ctx = canvas.getContext('2d');
    ctx.scale(dpr, dpr);

    const members = family.members.slice(0, 40);
    // hub node + one node per member, spokes hub->member
    const nodes = [{ id: '__hub__', label: family.label || 'app', hub: true, x: W / 2, y: H / 2, vx: 0, vy: 0 }];
    members.forEach((m, i) => {
      const a = (i / members.length) * Math.PI * 2;
      nodes.push({
        id: m.covenant_id,
        label: m.name || (m.covenant_id.slice(0, 6)),
        x: W / 2 + Math.cos(a) * 120 + (i % 3) * 6,
        y: H / 2 + Math.sin(a) * 120,
        vx: 0, vy: 0,
        r: 6 + Math.min(6, (m.shared_txs || 1)),
      });
    });
    const edges = members.map((_, i) => [0, i + 1]);

    const REST = Math.min(150, 60 + members.length * 2);
    let running = true;
    let alpha = 1;

    function tick() {
      // repulsion (all pairs)
      for (let i = 0; i < nodes.length; i++) {
        for (let j = i + 1; j < nodes.length; j++) {
          const a = nodes[i], b = nodes[j];
          let dx = b.x - a.x, dy = b.y - a.y;
          let d2 = dx * dx + dy * dy || 0.01;
          const f = (2600 * alpha) / d2;
          const d = Math.sqrt(d2);
          const fx = (dx / d) * f, fy = (dy / d) * f;
          a.vx -= fx; a.vy -= fy; b.vx += fx; b.vy += fy;
        }
      }
      // springs (edges pull toward REST)
      for (const [i, j] of edges) {
        const a = nodes[i], b = nodes[j];
        const dx = b.x - a.x, dy = b.y - a.y;
        const d = Math.sqrt(dx * dx + dy * dy) || 0.01;
        const f = (d - REST) * 0.02 * alpha;
        const fx = (dx / d) * f, fy = (dy / d) * f;
        a.vx += fx; a.vy += fy; b.vx -= fx; b.vy -= fy;
      }
      // centering + integrate
      for (const n of nodes) {
        if (n.hub) { n.x += (W / 2 - n.x) * 0.08; n.y += (H / 2 - n.y) * 0.08; n.vx = n.vy = 0; continue; }
        n.vx += (W / 2 - n.x) * 0.002 * alpha;
        n.vy += (H / 2 - n.y) * 0.002 * alpha;
        n.vx *= 0.86; n.vy *= 0.86;
        n.x += n.vx; n.y += n.vy;
        n.x = Math.max(14, Math.min(W - 14, n.x));
        n.y = Math.max(14, Math.min(H - 14, n.y));
      }
      alpha *= 0.985;
      draw();
      if (running && alpha > 0.02) requestAnimationFrame(tick);
    }

    function draw() {
      ctx.clearRect(0, 0, W, H);
      // edges
      ctx.strokeStyle = 'rgba(120,200,180,0.16)';
      ctx.lineWidth = 1;
      for (const [i, j] of edges) {
        ctx.beginPath();
        ctx.moveTo(nodes[i].x, nodes[i].y);
        ctx.lineTo(nodes[j].x, nodes[j].y);
        ctx.stroke();
      }
      // nodes
      for (const n of nodes) {
        if (n.hub) {
          ctx.fillStyle = 'rgba(73,234,203,0.14)';
          ctx.strokeStyle = '#49eacb';
          ctx.lineWidth = 1.5;
          ctx.beginPath(); ctx.arc(n.x, n.y, 15, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
          continue;
        }
        ctx.fillStyle = colorFor(n.id);
        ctx.beginPath(); ctx.arc(n.x, n.y, n.r, 0, Math.PI * 2); ctx.fill();
      }
    }

    // click → nearest node within radius
    function onClick(ev) {
      const rect = canvas.getBoundingClientRect();
      const mx = ev.clientX - rect.left, my = ev.clientY - rect.top;
      let best = null, bd = 18 * 18;
      for (const n of nodes) {
        if (n.hub) continue;
        const dx = n.x - mx, dy = n.y - my, d2 = dx * dx + dy * dy;
        if (d2 < bd) { bd = d2; best = n; }
      }
      if (best && opts && opts.onPick) opts.onPick(best);
    }
    canvas.addEventListener('click', onClick);
    canvas.style.cursor = 'pointer';

    tick();
    return {
      stop() { running = false; canvas.removeEventListener('click', onClick); },
    };
  }

  window.kascovGraph = { render, colorFor };
})();
