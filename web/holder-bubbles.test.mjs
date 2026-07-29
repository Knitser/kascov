import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import {
  HOLDER_BUBBLE_LIMIT,
  buildHolderBubbleModel,
  hitHolderBubble,
  moveHolderBubble,
} from './core/holder-bubbles.js';

const owner = (kind, digit) => `${kind}:${String(digit).repeat(64)}`;
const rows = [
  { owner: owner('covenant', 1), balance: 1000 },
  { owner: owner('pubkey', 2), balance: 400 },
  { owner: owner('presence', 3), balance: 250 },
  { owner: owner('pubkey', 4), balance: 160 },
  { owner: owner('script', 5), balance: 80 },
  { owner: owner('pubkey', 6), balance: 40 },
];
const events = [
  { owner_from: rows[0].owner, owner_to: rows[1].owner },
  { owner_from: rows[1].owner, owner_to: rows[0].owner },
  { owner_from: rows[2].owner, owner_to: rows[1].owner },
  { owner_from: owner('pubkey', 9), owner_to: rows[0].owner },
];

test('holder map is deterministic, bounded, and keeps area proportional to balance', () => {
  const a = buildHolderBubbleModel(rows, 1930, events, 900, 420);
  const b = buildHolderBubbleModel(rows, 1930, events, 900, 420);
  assert.equal(a.nodes.length, rows.length);
  assert.deepEqual(
    a.nodes.map((node) => [node.x.toFixed(3), node.y.toFixed(3), node.r.toFixed(3)]),
    b.nodes.map((node) => [node.x.toFixed(3), node.y.toFixed(3), node.r.toFixed(3)]),
  );
  assert.ok(a.nodes[0].r > a.nodes[1].r);
  const areaRatio = (a.nodes[0].r ** 2) / (a.nodes[1].r ** 2);
  assert.ok(Math.abs(areaRatio - 2.5) < 0.02);
  for (const node of a.nodes) {
    assert.ok(node.x - node.r >= 0);
    assert.ok(node.x + node.r <= a.width);
    assert.ok(node.y - node.r >= 0);
    assert.ok(node.y + node.r <= a.height);
  }
});

test('only observed moves between current holders become map links', () => {
  const model = buildHolderBubbleModel(rows, 1930, events, 900, 420);
  assert.equal(model.links.length, 2);
  const strongest = model.links.find((link) => link.a === 0 && link.b === 1);
  assert.equal(strongest.count, 2);
  assert.ok(model.nodes[0].activity > 0);
  assert.ok(model.nodes[1].activity > 0);
});

test('the render cap and hit target keep the hot path bounded', () => {
  const many = Array.from({ length: HOLDER_BUBBLE_LIMIT + 25 }, (_, index) => ({
    owner: `pubkey:${index.toString(16).padStart(64, '0')}`,
    balance: HOLDER_BUBBLE_LIMIT + 25 - index,
  }));
  const model = buildHolderBubbleModel(many, 10000, [], 900, 420);
  assert.equal(model.nodes.length, HOLDER_BUBBLE_LIMIT);
  const top = model.nodes[0];
  assert.equal(hitHolderBubble(model, top.x, top.y)?.owner, top.owner);
  assert.equal(hitHolderBubble(model, -100, -100), null);
});

test('dragging pins the grabbed holder, pushes neighbours, and stays in bounds', () => {
  const model = buildHolderBubbleModel(rows, 1930, events, 900, 420);
  const grabbed = model.nodes[0];
  const neighbour = model.nodes[1];
  const targetX = 10 + grabbed.r;
  const targetY = model.height / 2;
  neighbour.x = targetX + grabbed.r * 0.4;
  neighbour.y = targetY;
  neighbour.vx = 0;
  neighbour.vy = 0;
  const before = neighbour.x;

  moveHolderBubble(model, grabbed, -200, targetY, 12, 0);

  assert.equal(grabbed.x, 7 + grabbed.r);
  assert.equal(grabbed.y, targetY);
  assert.ok(neighbour.x > before, 'the neighbour should yield to the grabbed bubble');
  assert.ok(neighbour.vx > 0, 'the neighbour should inherit some drag momentum');
});

test('token pages mount the canvas map and explain what motion and links mean', () => {
  const app = readFileSync(new URL('./app.js', import.meta.url), 'utf8');
  const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
  assert.match(app, /createHolderBubbleMap\(holderCanvas/);
  assert.match(app, /drag a bubble to rearrange/);
  assert.match(app, /lines = observed moves · drift is visual/);
  assert.match(app, /route\.view !== 'token'\) stopHolderBubbleMap\(\)/);
  assert.match(css, /\.holders-bubbles-canvas\s*\{[\s\S]*?touch-action:\s*pan-y/);
  assert.match(css, /\.holders-bubbles-canvas\.is-dragging/);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.hb-live i/);
});
