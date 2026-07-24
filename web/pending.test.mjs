import assert from 'node:assert/strict';
import test from 'node:test';

import { createPendingModel } from './core/pending.js';

function fakeClock() {
  let now = 0;
  let nextId = 0;
  const tasks = new Map();
  return {
    now: () => now,
    setTimer(fn, delay) {
      const id = ++nextId;
      tasks.set(id, { at: now + delay, fn });
      return id;
    },
    clearTimer(id) {
      tasks.delete(id);
    },
    advance(ms) {
      const end = now + ms;
      for (;;) {
        const due = [...tasks.entries()]
          .filter(([, task]) => task.at <= end)
          .sort((a, b) => a[1].at - b[1].at || a[0] - b[0])[0];
        if (!due) break;
        const [id, task] = due;
        tasks.delete(id);
        now = task.at;
        task.fn();
      }
      now = end;
    },
  };
}

test('an SSE pending frame that arrives during snapshot fetch survives reconciliation', () => {
  const model = createPendingModel();
  const ticket = model.beginReconcile();

  model.pending({
    txid: 'newer',
    covenant_id: 'coin-newer',
    tx_kind: 'transition',
  });
  model.applySnapshot(ticket, {
    pending: [{ txid: 'older', covenant_id: 'coin-older', tx_kind: 'genesis' }],
  });

  assert.deepEqual(model.view().rows.map((row) => row.txid), ['older', 'newer']);
});

test('snapshot and SSE events for the same transaction merge without losing either covenant', () => {
  const model = createPendingModel();
  const ticket = model.beginReconcile();
  model.pending({ txid: 'shared', covenant_id: 'coin-live', tx_kind: 'burn' });
  model.applySnapshot(ticket, {
    pending: [{
      txid: 'shared',
      events: [{ covenant_id: 'coin-snapshot', tx_kind: 'transition' }],
    }],
  });

  assert.deepEqual(model.view().rows[0].events, [
    { covenantId: 'coin-snapshot', txKind: 'transition' },
    { covenantId: 'coin-live', txKind: 'burn' },
  ]);
});

test('a reconnect reconciliation replaces stale rows and ignores an older response', () => {
  const model = createPendingModel();
  model.pending({ txid: 'stale', covenant_id: 'coin-stale', tx_kind: 'transition' });

  const older = model.beginReconcile();
  const reconnect = model.beginReconcile();
  assert.equal(model.applySnapshot(older, {
    pending: [{ txid: 'wrong', covenant_id: 'coin-wrong', tx_kind: 'genesis' }],
  }), false);
  assert.equal(model.applySnapshot(reconnect, {
    pending: [{ txid: 'current', covenant_id: 'coin-current', tx_kind: 'transition' }],
  }), true);

  assert.deepEqual(model.view().rows.map((row) => row.txid), ['current']);
});

test('a resolution received before the in-flight snapshot cannot resurrect that transaction', () => {
  const model = createPendingModel();
  const ticket = model.beginReconcile();

  model.resolve({ txid: 'raced', resolution: 'confirmed' });
  model.applySnapshot(ticket, {
    pending: [{ txid: 'raced', covenant_id: 'coin-raced', tx_kind: 'transition' }],
  });

  assert.deepEqual(model.view().rows, []);
});

test('a confirmed pending row clears after its visible resolution interval', () => {
  const clock = fakeClock();
  const model = createPendingModel({
    now: clock.now,
    setTimer: clock.setTimer,
    clearTimer: clock.clearTimer,
    confirmedMs: 900,
  });
  model.pending({ txid: 'confirmed', covenant_id: 'coin', tx_kind: 'genesis' });
  model.resolve({ txid: 'confirmed', resolution: 'confirmed' });

  assert.equal(model.view().rows[0].resolution, 'confirmed');
  clock.advance(899);
  assert.equal(model.view().rows.length, 1);
  clock.advance(1);
  assert.equal(model.view().rows.length, 0);
});

test('a re-entered tx gets a new generation and an old resolution timer cannot clear it', () => {
  const clock = fakeClock();
  const model = createPendingModel({
    now: clock.now,
    setTimer: clock.setTimer,
    clearTimer: clock.clearTimer,
    confirmedMs: 900,
  });
  model.pending({ txid: 'reentry', covenant_id: 'coin-a', tx_kind: 'transition' });
  const firstGeneration = model.view().rows[0].generation;
  model.resolve({ txid: 'reentry', resolution: 'confirmed' });
  clock.advance(400);
  model.pending({ txid: 'reentry', covenant_id: 'coin-b', tx_kind: 'transition' });
  const current = model.view().rows[0];

  assert.notEqual(current.generation, firstGeneration);
  assert.equal(current.resolution, null);
  assert.deepEqual(current.covenantIds, ['coin-b']);
  clock.advance(500);
  assert.equal(model.view().rows.length, 1);
});

test('snapshot and repeated SSE frames keep every covenant event on one stable transaction row', () => {
  const model = createPendingModel();
  const ticket = model.beginReconcile();
  model.applySnapshot(ticket, {
    pending: [{
      txid: 'multi-snapshot',
      covenant_id: 'coin-a',
      tx_kind: 'transition',
      events: [
        { covenant_id: 'coin-a', tx_kind: 'transition' },
        { covenant_id: 'coin-b', tx_kind: 'burn' },
      ],
    }],
  });
  model.pending({ txid: 'multi-sse', covenant_id: 'coin-c', tx_kind: 'genesis' });
  const generation = model.view().rows.find((row) => row.txid === 'multi-sse').generation;
  model.pending({ txid: 'multi-sse', covenant_id: 'coin-d', tx_kind: 'transition' });

  const rows = model.view().rows;
  assert.deepEqual(rows[0].events, [
    { covenantId: 'coin-a', txKind: 'transition' },
    { covenantId: 'coin-b', txKind: 'burn' },
  ]);
  assert.deepEqual(rows[1].covenantIds, ['coin-c', 'coin-d']);
  assert.equal(rows[1].generation, generation);
});

test('the live cap bounds memory while the row cap exposes only the newest transactions', () => {
  const model = createPendingModel({ liveCap: 3, rowCap: 2 });
  for (const txid of ['a', 'b', 'c', 'd']) {
    model.pending({ txid, covenant_id: `coin-${txid}`, tx_kind: 'transition' });
  }

  assert.equal(model.view().total, 3);
  assert.deepEqual(model.view().rows.map((row) => row.txid), ['c', 'd']);
});

test('connection state is explicit instead of implying that a retrying stream is live', () => {
  const model = createPendingModel();
  assert.equal(model.view().connection, 'offline');

  model.setConnection('connecting');
  assert.equal(model.view().connection, 'connecting');
  model.setConnection('live');
  assert.equal(model.view().connection, 'live');
  model.setConnection('retrying');
  assert.equal(model.view().connection, 'retrying');
});
