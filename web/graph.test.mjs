import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const source = readFileSync(new URL('./graph.js', import.meta.url), 'utf8');
const sandbox = { window: {} };
vm.runInNewContext(source, sandbox);

const { _hitNode } = sandbox.window.kascovGraph;
const nodes = [
  { id: '__hub__', hub: true, x: 50, y: 50, r: 15 },
  { id: 'neighbor', x: 100, y: 100, r: 7 },
];

test('mouse clicks only activate the visible dot and a small edge tolerance', () => {
  assert.equal(_hitNode(nodes, 117, 100, 'mouse'), null);
  assert.equal(_hitNode(nodes, 110, 100, 'mouse').id, 'neighbor');
});

test('touch keeps an accessible target without making the whole field clickable', () => {
  assert.equal(_hitNode(nodes, 117, 100, 'touch').id, 'neighbor');
  assert.equal(_hitNode(nodes, 130, 100, 'touch'), null);
});

test('the non-interactive hub is never returned', () => {
  assert.equal(_hitNode(nodes, 50, 50, 'mouse'), null);
});
