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

  function hitNode(nodes, mx, my, pointerType) {
    let best = null, bd = Infinity;
    for (const n of nodes) {
      if (n.hub) continue;
      const dx = n.x - mx, dy = n.y - my, d2 = dx * dx + dy * dy;
      // Mouse interaction should closely match the painted dot. Touch keeps a
      // larger accessible target, but it is still local to the dot rather than
      // turning the surrounding graph card into an invisible link.
      const hitRadius = pointerType === 'touch'
        ? Math.max(18, n.r + 8)
        : n.r + 4;
      if (d2 <= hitRadius * hitRadius && d2 < bd) { bd = d2; best = n; }
    }
    return best;
  }

  /* nodes: [{id, label, hub?}], edges: [[i,j]]. Returns a controller with
     .stop() and a click handler wired to onPick(node). */
  function render(canvas, family, opts) {
    const members = family.members.slice(0, 40);
    canvas.classList.toggle('together-graph-compact', members.length <= 4);
    canvas.classList.toggle('together-graph-medium', members.length > 4 && members.length <= 12);

    let dpr = Math.min(window.devicePixelRatio || 1, 2);
    let W = canvas.clientWidth || 600;
    let H = canvas.clientHeight || 380;
    canvas.width = W * dpr;
    canvas.height = H * dpr;
    const ctx = canvas.getContext('2d');
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

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
    let hoverNode = null;
    let lastPointerType = 'mouse';

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
      ctx.lineWidth = 1;
      for (const [i, j] of edges) {
        const edge = ctx.createLinearGradient(nodes[i].x, nodes[i].y, nodes[j].x, nodes[j].y);
        edge.addColorStop(0, 'rgba(73,234,203,0.34)');
        edge.addColorStop(1, `${colorFor(nodes[j].id)}88`);
        ctx.strokeStyle = edge;
        ctx.beginPath();
        ctx.moveTo(nodes[i].x, nodes[i].y);
        ctx.lineTo(nodes[j].x, nodes[j].y);
        ctx.stroke();
      }
      // nodes
      for (const n of nodes) {
        if (n.hub) {
          ctx.save();
          ctx.shadowColor = 'rgba(73,234,203,0.42)';
          ctx.shadowBlur = 16;
          const hubGlow = ctx.createRadialGradient(n.x - 4, n.y - 5, 1, n.x, n.y, 17);
          hubGlow.addColorStop(0, 'rgba(123,255,229,0.34)');
          hubGlow.addColorStop(1, 'rgba(73,234,203,0.07)');
          ctx.fillStyle = hubGlow;
          ctx.strokeStyle = 'rgba(73,234,203,0.94)';
          ctx.lineWidth = 1.5;
          ctx.beginPath(); ctx.arc(n.x, n.y, 16, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
          ctx.shadowBlur = 0;
          ctx.strokeStyle = 'rgba(73,234,203,0.14)';
          ctx.lineWidth = 1;
          ctx.beginPath(); ctx.arc(n.x, n.y, 22, 0, Math.PI * 2); ctx.stroke();
          ctx.fillStyle = '#8ff5df';
          ctx.beginPath(); ctx.arc(n.x, n.y, 2.5, 0, Math.PI * 2); ctx.fill();
          ctx.font = '600 10px ui-monospace, monospace';
          ctx.textAlign = 'center';
          ctx.fillStyle = 'rgba(177,207,198,0.72)';
          ctx.fillText('THIS COIN', n.x, n.y + 38);
          ctx.restore();
          continue;
        }
        const color = colorFor(n.id);
        ctx.save();
        ctx.fillStyle = `${color}1f`;
        ctx.beginPath(); ctx.arc(n.x, n.y, n.r + 6, 0, Math.PI * 2); ctx.fill();
        ctx.shadowColor = color;
        ctx.shadowBlur = n === hoverNode ? 18 : 10;
        ctx.fillStyle = color;
        ctx.beginPath(); ctx.arc(n.x, n.y, n.r, 0, Math.PI * 2); ctx.fill();
        ctx.shadowBlur = 0;
        ctx.strokeStyle = 'rgba(235,255,249,0.44)';
        ctx.lineWidth = 1;
        ctx.beginPath(); ctx.arc(n.x, n.y, n.r - 0.5, 0, Math.PI * 2); ctx.stroke();
        if (n === hoverNode) {
          ctx.strokeStyle = 'rgba(225,255,247,0.95)';
          ctx.lineWidth = 1.5;
          ctx.beginPath();
          ctx.arc(n.x, n.y, n.r + 4, 0, Math.PI * 2);
          ctx.stroke();
        }
        ctx.restore();
      }
      if (hoverNode) {
        const label = hoverNode.label;
        ctx.save();
        ctx.font = '12px ui-monospace, monospace';
        const tw = Math.ceil(ctx.measureText(label).width);
        const tx = Math.max(8, Math.min(W - tw - 20, hoverNode.x - tw / 2 - 10));
        const ty = hoverNode.y > 42 ? hoverNode.y - hoverNode.r - 31 : hoverNode.y + hoverNode.r + 12;
        ctx.fillStyle = 'rgba(3,14,11,0.92)';
        ctx.fillRect(tx, ty, tw + 20, 25);
        ctx.strokeStyle = 'rgba(73,234,203,0.28)';
        ctx.strokeRect(tx + 0.5, ty + 0.5, tw + 19, 24);
        ctx.fillStyle = 'rgba(230,240,248,0.96)';
        ctx.textAlign = 'left';
        ctx.fillText(label, tx + 10, ty + 17);
        ctx.restore();
      }
    }

    function pointerPosition(ev) {
      const rect = canvas.getBoundingClientRect();
      return {
        x: (ev.clientX - rect.left) * (W / Math.max(1, rect.width)),
        y: (ev.clientY - rect.top) * (H / Math.max(1, rect.height)),
      };
    }

    function onPointerMove(ev) {
      lastPointerType = ev.pointerType || 'mouse';
      const p = pointerPosition(ev);
      const next = hitNode(nodes, p.x, p.y, lastPointerType);
      if (next === hoverNode) return;
      hoverNode = next;
      canvas.style.cursor = next ? 'pointer' : 'default';
      draw();
    }
    function onPointerLeave() {
      if (!hoverNode && canvas.style.cursor === 'default') return;
      hoverNode = null;
      canvas.style.cursor = 'default';
      draw();
    }

    // Clicks resolve through the same precise hit test that drives hover.
    function onClick(ev) {
      const p = pointerPosition(ev);
      const best = hitNode(nodes, p.x, p.y, ev.pointerType || lastPointerType);
      if (best && opts && opts.onPick) opts.onPick(best);
    }
    function onPointerDown(ev) { lastPointerType = ev.pointerType || 'mouse'; }

    canvas.addEventListener('click', onClick);
    canvas.addEventListener('pointerdown', onPointerDown);
    canvas.addEventListener('pointermove', onPointerMove);
    canvas.addEventListener('pointerleave', onPointerLeave);
    canvas.style.cursor = 'default';

    // Keep painted and hit-test coordinates aligned when the responsive detail
    // page changes width. Shift the settled layout with the new center.
    const resizeObserver = typeof ResizeObserver === 'function'
      ? new ResizeObserver(() => {
          const nextW = canvas.clientWidth || W;
          const nextH = canvas.clientHeight || H;
          if (nextW === W && nextH === H) return;
          const dx = (nextW - W) / 2;
          const dy = (nextH - H) / 2;
          for (const n of nodes) { n.x += dx; n.y += dy; }
          W = nextW;
          H = nextH;
          dpr = Math.min(window.devicePixelRatio || 1, 2);
          canvas.width = Math.round(W * dpr);
          canvas.height = Math.round(H * dpr);
          ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
          draw();
        })
      : null;
    if (resizeObserver) resizeObserver.observe(canvas);

    tick();
    return {
      stop() {
        running = false;
        canvas.removeEventListener('click', onClick);
        canvas.removeEventListener('pointerdown', onPointerDown);
        canvas.removeEventListener('pointermove', onPointerMove);
        canvas.removeEventListener('pointerleave', onPointerLeave);
        if (resizeObserver) resizeObserver.disconnect();
      },
    };
  }

  window.kascovGraph = { render, colorFor, _hitNode: hitNode };
})();
