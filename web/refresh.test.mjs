import assert from 'node:assert/strict';
import test from 'node:test';

import { createRefreshGate } from './core/refresh.js';

test('bursts share one in-flight refresh and one trailing refresh', async () => {
  let clock = 1_000;
  const timers = [];
  const gate = createRefreshGate(100, {
    now: () => clock,
    setTimer: (fn, ms) => { timers.push({ fn, ms }); return timers.length; },
  });
  let release;
  let calls = 0;
  const first = gate.run('testnet-10', async () => {
    calls += 1;
    await new Promise((resolve) => { release = resolve; });
  });
  const shared = gate.run('testnet-10', async () => { calls += 1; });

  await Promise.resolve();
  assert.equal(calls, 1);
  assert.equal(shared, first);
  release();
  await first;
  assert.equal(timers.length, 1);
  assert.equal(timers[0].ms, 100);

  clock += 100;
  timers[0].fn();
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(calls, 2);
});

test('network keys do not block each other', async () => {
  const gate = createRefreshGate(100);
  let calls = 0;
  await Promise.all([
    gate.run('testnet-10', async () => { calls += 1; }),
    gate.run('mainnet', async () => { calls += 1; }),
  ]);
  assert.equal(calls, 2);
});
