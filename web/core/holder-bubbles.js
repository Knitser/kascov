/* A small, dependency-free holder bubble map.
   The model is deterministic, capped, and only animates while visible. */

export const HOLDER_BUBBLE_LIMIT = 100;

const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));
const COLORS = {
  presence: '#70c7ba',
  covenant: '#7f8ce0',
  pubkey: '#5be49b',
  script: '#7a8d88',
  other: '#7a8d88',
};

const clamp = (n, lo, hi) => Math.max(lo, Math.min(hi, n));

function ownerKey(owner) {
  return String(owner || '').trim().toLowerCase();
}

function ownerKind(owner) {
  const key = ownerKey(owner);
  const split = key.indexOf(':');
  return split === -1 ? 'other' : (key.slice(0, split) || 'other');
}

function seedOf(text) {
  let seed = 2166136261;
  for (const ch of String(text || '')) {
    seed ^= ch.charCodeAt(0);
    seed = Math.imul(seed, 16777619);
  }
  return seed >>> 0;
}

function linkCurrentHolders(nodes, events) {
  const byOwner = new Map(nodes.map((node, index) => [node.ownerKey, index]));
  const links = new Map();
  const rows = Array.isArray(events) ? events : [];
  rows.forEach((event, eventIndex) => {
    const from = byOwner.get(ownerKey(event && event.owner_from));
    const to = byOwner.get(ownerKey(event && event.owner_to));
    if (from == null || to == null || from === to) return;
    const a = Math.min(from, to);
    const b = Math.max(from, to);
    const key = `${a}:${b}`;
    const link = links.get(key) || { a, b, count: 0, newest: eventIndex };
    link.count += 1;
    link.newest = Math.min(link.newest, eventIndex);
    links.set(key, link);
    nodes[from].activity = Math.max(nodes[from].activity, 1 - eventIndex / Math.max(rows.length, 1));
    nodes[to].activity = Math.max(nodes[to].activity, 1 - eventIndex / Math.max(rows.length, 1));
  });
  return [...links.values()];
}

function stepModel(model, phase = 0, settling = false) {
  const { nodes, links, width, height } = model;
  const homePull = settling ? 0.012 : 0.0007;
  const damping = settling ? 0.72 : 0.965;

  for (const node of nodes) {
    node.vx += (node.homeX - node.x) * homePull;
    node.vy += (node.homeY - node.y) * homePull;
    if (!settling) {
      const wobble = phase + node.seed * 0.000002;
      node.vx += Math.cos(wobble * 0.71) * 0.0025;
      node.vy += Math.sin(wobble * 0.83) * 0.0025;
    }
  }

  /* A line is evidence of an observed move, so linked holders also get a
     gentle spring. It is deliberately weak: balance, not graph degree,
     remains the dominant visual signal. */
  for (const link of links) {
    const a = nodes[link.a];
    const b = nodes[link.b];
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const distance = Math.max(Math.hypot(dx, dy), 0.001);
    const wanted = a.r + b.r + 18;
    const force = (distance - wanted) * (settling ? 0.0018 : 0.00045) *
      Math.min(2.2, 1 + Math.log2(link.count + 1) * 0.25);
    const fx = dx / distance * force;
    const fy = dy / distance * force;
    a.vx += fx;
    a.vy += fy;
    b.vx -= fx;
    b.vy -= fy;
  }

  for (const node of nodes) {
    node.x += node.vx;
    node.y += node.vy;
    node.vx *= damping;
    node.vy *= damping;
  }

  /* At most 100 nodes: a direct pair pass is cheaper than maintaining a
     spatial index and is comfortably below one millisecond on this canvas. */
  for (let i = 0; i < nodes.length; i += 1) {
    for (let j = i + 1; j < nodes.length; j += 1) {
      const a = nodes[i];
      const b = nodes[j];
      let dx = b.x - a.x;
      let dy = b.y - a.y;
      let distance = Math.hypot(dx, dy);
      const wanted = a.r + b.r + 3;
      if (distance >= wanted) continue;
      if (distance < 0.001) {
        const angle = (a.seed + b.seed) * 0.000001;
        dx = Math.cos(angle);
        dy = Math.sin(angle);
        distance = 1;
      }
      const overlap = (wanted - distance) * 0.52;
      const nx = dx / distance;
      const ny = dy / distance;
      const total = a.r + b.r;
      const moveA = b.r / total;
      const moveB = a.r / total;
      a.x -= nx * overlap * moveA;
      a.y -= ny * overlap * moveA;
      b.x += nx * overlap * moveB;
      b.y += ny * overlap * moveB;
      a.vx -= nx * 0.015;
      a.vy -= ny * 0.015;
      b.vx += nx * 0.015;
      b.vy += ny * 0.015;
    }
  }

  const pad = 7;
  for (const node of nodes) {
    const minX = pad + node.r;
    const maxX = width - pad - node.r;
    const minY = pad + node.r;
    const maxY = height - pad - node.r;
    if (node.x < minX) { node.x = minX; node.vx = Math.abs(node.vx) * 0.45; }
    if (node.x > maxX) { node.x = maxX; node.vx = -Math.abs(node.vx) * 0.45; }
    if (node.y < minY) { node.y = minY; node.vy = Math.abs(node.vy) * 0.45; }
    if (node.y > maxY) { node.y = maxY; node.vy = -Math.abs(node.vy) * 0.45; }
  }
}

export function buildHolderBubbleModel(rows, base, events, width = 900, height = 420) {
  const clean = (Array.isArray(rows) ? rows : [])
    .map((row) => ({ ...row, balance: Number(row && row.balance) }))
    .filter((row) => Number.isFinite(row.balance) && row.balance > 0 && ownerKey(row.owner))
    .slice(0, HOLDER_BUBBLE_LIMIT);
  const total = clean.reduce((sum, row) => sum + row.balance, 0);
  if (!clean.length || total <= 0) return { width, height, base: 0, nodes: [], links: [] };

  const maxRadius = Math.min(height * 0.31, width * 0.19);
  const fillArea = width * height * 0.46;
  const biggest = Math.max(...clean.map((row) => row.balance));
  const areaScale = Math.min(fillArea / total, Math.PI * maxRadius * maxRadius / biggest);
  const minRadius = clamp(width / 155, 4.5, 6.5);
  const cx = width / 2;
  const cy = height / 2;
  const denominator = Number.isFinite(Number(base)) && Number(base) > 0 ? Number(base) : total;

  const nodes = clean.map((row, index) => {
    const seed = seedOf(row.owner);
    const angle = index * GOLDEN_ANGLE + (seed % 360) * Math.PI / 1800;
    const spread = index === 0 ? 0 : Math.sqrt(index / Math.max(clean.length - 1, 1));
    const homeX = cx + Math.cos(angle) * spread * width * 0.35;
    const homeY = cy + Math.sin(angle) * spread * height * 0.32;
    const r = Math.max(minRadius, Math.sqrt(row.balance * areaScale / Math.PI));
    return {
      ...row,
      ownerKey: ownerKey(row.owner),
      kind: row.kind || ownerKind(row.owner),
      color: COLORS[row.kind || ownerKind(row.owner)] || COLORS.other,
      share: row.balance / denominator * 100,
      rank: index + 1,
      seed,
      r,
      x: homeX,
      y: homeY,
      homeX,
      homeY,
      vx: 0,
      vy: 0,
      activity: 0,
    };
  });
  const links = linkCurrentHolders(nodes, events);
  const model = { width, height, base: denominator, nodes, links };

  /* Settle before first paint: no visible pile-up and no async warm-up. */
  for (let i = 0; i < 180; i += 1) stepModel(model, 0, true);
  return model;
}

export function hitHolderBubble(model, x, y) {
  if (!model || !Array.isArray(model.nodes)) return null;
  let found = null;
  let nearest = Infinity;
  for (const node of model.nodes) {
    const distance = Math.hypot(node.x - x, node.y - y);
    if (distance <= node.r + 3 && distance < nearest) {
      found = node;
      nearest = distance;
    }
  }
  return found;
}

function percentLabel(share) {
  if (!Number.isFinite(share)) return '—';
  if (share >= 9.95) return `${share.toFixed(0)}%`;
  if (share >= 0.095) return `${share.toFixed(1)}%`;
  return '<0.1%';
}

export function createHolderBubbleMap(canvas, options = {}) {
  if (!canvas || typeof canvas.getContext !== 'function') return null;
  const rows = Array.isArray(options.rows) ? options.rows : [];
  const events = Array.isArray(options.events) ? options.events : [];
  const wrapper = canvas.closest('[data-holder-bubbles]');
  const inspector = wrapper && wrapper.querySelector('[data-holder-inspector]');
  const motionButton = wrapper && wrapper.querySelector('[data-holder-motion]');
  const context = canvas.getContext('2d', { alpha: true });
  if (!context) return null;

  const media = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)');
  let reduceMotion = Boolean(media && media.matches);
  let manuallyPaused = false;
  let visible = true;
  let destroyed = false;
  let frame = 0;
  let phase = 0;
  let model = null;
  let hovered = null;
  let selected = null;
  let down = null;
  let lastTouchOwner = null;
  let lastTouchAt = 0;
  let lastPaintAt = 0;
  let resizeFrame = 0;
  let dpr = 1;
  const sprites = new Map();
  const coarsePointer = Boolean(window.matchMedia && window.matchMedia('(pointer: coarse)').matches);
  const frameInterval = 1000 / (coarsePointer ? 30 : 45);

  const showInspector = (node) => {
    if (!inspector) return;
    if (!node) {
      inspector.hidden = true;
      inspector.removeAttribute('href');
      return;
    }
    const kind = inspector.querySelector('[data-hb-kind]');
    const owner = inspector.querySelector('[data-hb-owner]');
    const balance = inspector.querySelector('[data-hb-balance]');
    const share = inspector.querySelector('[data-hb-share]');
    if (kind) kind.textContent = `${node.kind || 'owner'} · #${node.rank}`;
    if (owner) owner.textContent = node.ownerLabel || node.owner;
    if (balance) balance.textContent = node.balanceLabel || String(node.balance);
    if (share) share.textContent = percentLabel(node.share);
    if (node.href) {
      inspector.href = node.href;
      inspector.removeAttribute('aria-disabled');
    } else {
      inspector.removeAttribute('href');
      inspector.setAttribute('aria-disabled', 'true');
    }
    inspector.hidden = false;
  };

  const getSprite = (node) => {
    const radius = Math.max(6, Math.round(node.r / 4) * 4);
    const key = `${node.kind}:${radius}:${dpr}`;
    if (sprites.has(key)) return sprites.get(key);
    const glow = Math.min(14, Math.max(6, radius * 0.2));
    const size = Math.ceil((radius + glow) * 2);
    const sprite = document.createElement('canvas');
    sprite.width = Math.ceil(size * dpr);
    sprite.height = Math.ceil(size * dpr);
    const ctx = sprite.getContext('2d');
    ctx.scale(dpr, dpr);
    const center = size / 2;
    ctx.shadowColor = node.color;
    ctx.shadowBlur = glow;
    const gradient = ctx.createRadialGradient(
      center - radius * 0.32, center - radius * 0.36, radius * 0.08,
      center, center, radius,
    );
    gradient.addColorStop(0, '#eefcf8');
    gradient.addColorStop(0.13, node.color);
    gradient.addColorStop(0.74, node.color);
    gradient.addColorStop(1, '#10231f');
    ctx.fillStyle = gradient;
    ctx.beginPath();
    ctx.arc(center, center, radius, 0, Math.PI * 2);
    ctx.fill();
    ctx.shadowBlur = 0;
    ctx.strokeStyle = 'rgba(222, 255, 247, .42)';
    ctx.lineWidth = 1;
    ctx.stroke();
    const result = { canvas: sprite, size, radius };
    sprites.set(key, result);
    return result;
  };

  const draw = () => {
    if (!model || destroyed) return;
    const { width, height, nodes, links } = model;
    context.clearRect(0, 0, width, height);

    context.save();
    context.strokeStyle = 'rgba(112, 199, 186, .055)';
    context.lineWidth = 1;
    for (let ring = 1; ring <= 3; ring += 1) {
      context.beginPath();
      context.ellipse(width / 2, height / 2, width * 0.16 * ring, height * 0.145 * ring, 0, 0, Math.PI * 2);
      context.stroke();
    }
    context.restore();

    for (const link of links) {
      const a = nodes[link.a];
      const b = nodes[link.b];
      const alpha = Math.min(0.32, 0.1 + Math.log2(link.count + 1) * 0.055);
      context.strokeStyle = `rgba(112, 199, 186, ${alpha})`;
      context.lineWidth = Math.min(2.2, 0.7 + Math.log2(link.count + 1) * 0.36);
      context.beginPath();
      context.moveTo(a.x, a.y);
      context.lineTo(b.x, b.y);
      context.stroke();
    }

    for (const node of nodes) {
      const sprite = getSprite(node);
      const active = node === hovered || node === selected;
      context.save();
      context.globalAlpha = active ? 1 : 0.88;
      context.drawImage(
        sprite.canvas,
        node.x - sprite.size / 2,
        node.y - sprite.size / 2,
        sprite.size,
        sprite.size,
      );
      context.restore();

      if (node.activity > 0.72 && !reduceMotion && !manuallyPaused) {
        const pulse = (Math.sin(phase * 1.8 + node.seed) + 1) / 2;
        context.strokeStyle = `rgba(91, 228, 155, ${0.08 + pulse * 0.16})`;
        context.lineWidth = 1.2;
        context.beginPath();
        context.arc(node.x, node.y, node.r + 4 + pulse * 3, 0, Math.PI * 2);
        context.stroke();
      }

      if (active) {
        context.strokeStyle = node === selected ? '#eefcf8' : 'rgba(238, 252, 248, .75)';
        context.lineWidth = node === selected ? 2.4 : 1.5;
        context.beginPath();
        context.arc(node.x, node.y, node.r + 2.5, 0, Math.PI * 2);
        context.stroke();
      }

      if (node.r >= 22) {
        const main = percentLabel(node.share);
        const fontSize = clamp(node.r * 0.31, 11, 22);
        context.fillStyle = '#06110e';
        context.textAlign = 'center';
        context.textBaseline = 'middle';
        context.font = `700 ${fontSize}px system-ui, sans-serif`;
        context.fillText(main, node.x, node.y + (node.r >= 42 ? -1 : 0));
        if (node.r >= 42) {
          context.globalAlpha = 0.75;
          context.font = `600 ${clamp(node.r * 0.13, 9, 12)}px ui-monospace, monospace`;
          context.fillText(node.ownerLabel || `#${node.rank}`, node.x, node.y + fontSize * 0.9);
          context.globalAlpha = 1;
        }
      }
    }
  };

  const animate = (now) => {
    frame = 0;
    if (destroyed || manuallyPaused || reduceMotion || document.hidden || !visible) return;
    if (!lastPaintAt || now - lastPaintAt >= frameInterval) {
      lastPaintAt = now;
      phase += 0.018;
      stepModel(model, phase, false);
      draw();
    }
    frame = requestAnimationFrame(animate);
  };

  const syncMotion = () => {
    if (motionButton) {
      const paused = manuallyPaused || reduceMotion;
      motionButton.textContent = paused ? 'start motion' : 'pause motion';
      motionButton.setAttribute('aria-pressed', String(paused));
      motionButton.disabled = reduceMotion;
      motionButton.title = reduceMotion ? 'motion is disabled by your system preference' : '';
    }
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
    draw();
    if (!manuallyPaused && !reduceMotion && !document.hidden && visible) {
      frame = requestAnimationFrame(animate);
    }
  };

  const resize = () => {
    resizeFrame = 0;
    if (destroyed) return;
    const rect = canvas.getBoundingClientRect();
    const width = Math.max(320, Math.round(rect.width || 900));
    const height = Math.max(280, Math.round(rect.height || 420));
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    context.setTransform(dpr, 0, 0, dpr, 0, 0);
    sprites.clear();
    model = buildHolderBubbleModel(rows, options.base, events, width, height);
    if (selected) selected = model.nodes.find((node) => node.ownerKey === selected.ownerKey) || null;
    if (hovered) hovered = model.nodes.find((node) => node.ownerKey === hovered.ownerKey) || null;
    showInspector(hovered || selected);
    syncMotion();
  };

  const scheduleResize = () => {
    if (resizeFrame || destroyed) return;
    resizeFrame = requestAnimationFrame(resize);
  };

  const localPoint = (event) => {
    const rect = canvas.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  };

  const onPointerMove = (event) => {
    if (!model || event.pointerType === 'touch') return;
    const point = localPoint(event);
    const next = hitHolderBubble(model, point.x, point.y);
    if (next === hovered) return;
    hovered = next;
    canvas.style.cursor = hovered && hovered.href ? 'pointer' : 'default';
    showInspector(hovered || selected);
    draw();
  };

  const onPointerLeave = () => {
    hovered = null;
    canvas.style.cursor = 'default';
    showInspector(selected);
    draw();
  };

  const onPointerDown = (event) => {
    const point = localPoint(event);
    down = { ...point, at: performance.now(), pointerType: event.pointerType };
  };

  const onPointerUp = (event) => {
    if (!down || !model) return;
    const point = localPoint(event);
    const wasTap = Math.hypot(point.x - down.x, point.y - down.y) < 8 &&
      performance.now() - down.at < 700;
    const pointerType = down.pointerType;
    down = null;
    if (!wasTap) return;
    const node = hitHolderBubble(model, point.x, point.y);
    if (!node) {
      selected = null;
      showInspector(hovered);
      draw();
      return;
    }
    selected = node;
    showInspector(node);
    canvas.setAttribute('aria-label',
      `Selected holder ${node.ownerLabel || node.owner}, ${percentLabel(node.share)} of supply. ` +
      'Use Enter to open it or the arrow keys to inspect another holder.');
    draw();
    if (!node.href) return;
    if (pointerType === 'touch') {
      const now = Date.now();
      if (lastTouchOwner === node.ownerKey && now - lastTouchAt < 1400) options.onOpen?.(node.href, node);
      lastTouchOwner = node.ownerKey;
      lastTouchAt = now;
    } else {
      options.onOpen?.(node.href, node);
    }
  };

  const onKeyDown = (event) => {
    if (!model || !model.nodes.length) return;
    const keys = ['ArrowRight', 'ArrowDown', 'ArrowLeft', 'ArrowUp'];
    if (keys.includes(event.key)) {
      event.preventDefault();
      const current = selected ? model.nodes.indexOf(selected) : -1;
      const step = event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 1;
      const next = (current + step + model.nodes.length) % model.nodes.length;
      selected = model.nodes[next];
      showInspector(selected);
      draw();
      return;
    }
    if (event.key === 'Enter' && selected && selected.href) {
      event.preventDefault();
      options.onOpen?.(selected.href, selected);
    }
    if (event.key === 'Escape') {
      selected = null;
      showInspector(hovered);
      draw();
    }
  };

  const onVisibility = () => syncMotion();
  const onMedia = (event) => {
    reduceMotion = event.matches;
    syncMotion();
  };
  const onMotion = () => {
    manuallyPaused = !manuallyPaused;
    syncMotion();
  };

  canvas.addEventListener('pointermove', onPointerMove);
  canvas.addEventListener('pointerleave', onPointerLeave);
  canvas.addEventListener('pointerdown', onPointerDown);
  canvas.addEventListener('pointerup', onPointerUp);
  canvas.addEventListener('keydown', onKeyDown);
  motionButton?.addEventListener('click', onMotion);
  document.addEventListener('visibilitychange', onVisibility);
  media?.addEventListener?.('change', onMedia);

  const resizeObserver = typeof ResizeObserver !== 'undefined'
    ? new ResizeObserver(scheduleResize) : null;
  resizeObserver?.observe(canvas);
  const intersectionObserver = typeof IntersectionObserver !== 'undefined'
    ? new IntersectionObserver((entries) => {
        visible = Boolean(entries[0] && entries[0].isIntersecting);
        syncMotion();
      }, { rootMargin: '120px' }) : null;
  intersectionObserver?.observe(canvas);
  resize();

  return {
    destroy() {
      destroyed = true;
      if (frame) cancelAnimationFrame(frame);
      if (resizeFrame) cancelAnimationFrame(resizeFrame);
      resizeObserver?.disconnect();
      intersectionObserver?.disconnect();
      canvas.removeEventListener('pointermove', onPointerMove);
      canvas.removeEventListener('pointerleave', onPointerLeave);
      canvas.removeEventListener('pointerdown', onPointerDown);
      canvas.removeEventListener('pointerup', onPointerUp);
      canvas.removeEventListener('keydown', onKeyDown);
      motionButton?.removeEventListener('click', onMotion);
      document.removeEventListener('visibilitychange', onVisibility);
      media?.removeEventListener?.('change', onMedia);
      sprites.clear();
    },
    resize: scheduleResize,
    model: () => model,
  };
}
