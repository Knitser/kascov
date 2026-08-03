import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const source = readFileSync(new URL('./galaxy.js', import.meta.url), 'utf8');
const sandbox = { window: {} };
vm.runInNewContext(source, sandbox);

const { _appFocusScale, _hasIdentity, _visualIds } = sandbox.window.kascovGalaxy;

test('a small edge cluster keeps surrounding aggregates during first focus', () => {
  const fitScale = 0.1;
  const target = _appFocusScale(8, fitScale, 1600, 900, fitScale);

  assert.equal(target / fitScale, 15);
});

test('cluster focus never zooms the camera backwards', () => {
  const fitScale = 0.1;
  const currentScale = fitScale * 16;

  assert.equal(_appFocusScale(8, fitScale, 1600, 900, currentScale), currentScale);
});

test('large clusters still receive a useful first zoom step', () => {
  const fitScale = 0.1;
  const target = _appFocusScale(500, fitScale, 1600, 900, fitScale);

  assert.ok(target / fitScale >= 6);
  assert.ok(target / fitScale <= 15);
});

test('visual-tier placeholder nodes are not treated as clickable identities', () => {
  assert.equal(_hasIdentity(''), false);
  assert.equal(_hasIdentity(null), false);
  assert.equal(_hasIdentity('001122'), true);
});

test('visual tier preserves the core identity prefix and pads outer nodes', () => {
  assert.deepEqual(Array.from(_visualIds(['aa', 'bb'], 5, true)), ['aa', 'bb', '', '', '']);
});

test('visual tier drops core identities when its layout fingerprint changed', () => {
  assert.deepEqual(Array.from(_visualIds(['aa', 'bb'], 4, false)), ['', '', '', '']);
});

test('search matches a token by claimed name and by ticker, with or without $', () => {
  const { _searchMatches } = sandbox.window.kascovGalaxy;
  const id = 'c58c826d0aa9cee62f93208718c674883f5c89a8aca4933dc41fb0391539abe2';
  const friendly = 'humble-crimson-tortoise';
  const info = { label: 'kascov', ticker: 'KASCOV', art: null };

  assert.equal(_searchMatches(id, friendly, info, 'kascov'), true);      // name
  assert.equal(_searchMatches(id, friendly, info, '$kascov'), true);     // $ticker
  assert.equal(_searchMatches(id, friendly, info, 'kasc'), true);        // partial
  assert.equal(_searchMatches(id, friendly, info, 'c58c826d'), true);    // id prefix
  assert.equal(_searchMatches(id, friendly, info, 'tortoise'), true);    // derived name
  assert.equal(_searchMatches(id, friendly, info, 'nacho'), false);
  // a coin with no token info still matches id + derived name, nothing else
  assert.equal(_searchMatches(id, friendly, null, 'kascov'), false);
  assert.equal(_searchMatches(id, friendly, null, 'tortoise'), true);
  // a bare "$" must not become match-everything
  assert.equal(_searchMatches(id, friendly, info, '$'), false);
});
