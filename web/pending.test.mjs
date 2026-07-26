import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import { createPendingModel } from './core/pending.js';

/* The mempool view is a geometry contract as much as a model: its height must
   not depend on its content, a row must never be remounted, and the leave
   animation must finish before the model deletes the row. Those live in CSS and
   in the render layer, so they are pinned here the way web/responsive.test.mjs
   pins the phone contract — by reading the shipped source as text. */
const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
const app = readFileSync(new URL('./app.js', import.meta.url), 'utf8');
const index = readFileSync(new URL('./index.html', import.meta.url), 'utf8');
const modelSrc = readFileSync(new URL('./core/pending.js', import.meta.url), 'utf8');
const stateSrc = readFileSync(new URL('./core/state.js', import.meta.url), 'utf8');
const pendingBlock = css.match(/\n\.pending \{([\s\S]*?)\n\}/)[1];
const frameBlock = css.match(/#pending-row \{([\s\S]*?)\n\}/)[1];
const rowBlock = css.match(/\n\.pending-row \{([\s\S]*?)\n\}/)[1];
/* The render body is asserted with its comments STRIPPED: those comments name
   the very mistakes they warn about (replaceChildren, scrollTop = scrollHeight),
   and a "this must not appear" test has to read code, not prose. */
const renderPendingSrc = app
  .slice(app.indexOf('function renderPending('), app.indexOf('function notePending('))
  .replace(/\/\*[\s\S]*?\*\//g, '');

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

test('a dropped pending row clears after its shorter visible interval', () => {
  const clock = fakeClock();
  const model = createPendingModel({
    now: clock.now,
    setTimer: clock.setTimer,
    clearTimer: clock.clearTimer,
    droppedMs: 700,
  });
  model.pending({ txid: 'dropped', covenant_id: 'coin', tx_kind: 'transition' });
  model.resolve({ txid: 'dropped', resolution: 'dropped' });

  assert.equal(model.view().rows[0].resolution, 'dropped');
  clock.advance(699);
  assert.equal(model.view().rows.length, 1);
  clock.advance(1);
  assert.equal(model.view().rows.length, 0);
});

test('the mempool viewport reserves one fixed height so filling and emptying never moves the page', () => {
  assert.match(pendingBlock, /--pending-row-h:\s*2\.75rem/);
  assert.match(pendingBlock, /--pending-rows:\s*6/);
  assert.match(frameBlock, /height:\s*calc\(var\(--pending-row-h\) \* var\(--pending-rows\)/);
  /* content-dependent sizing in any form would put the shove straight back */
  assert.doesNotMatch(frameBlock, /min-height|max-height|justify-content/);
  assert.match(frameBlock, /overflow-y:\s*auto/);
  assert.match(frameBlock, /overflow-x:\s*clip/);
  assert.match(frameBlock, /position:\s*relative/);
  assert.match(css, /\.pending\[data-expanded="true"\] \{ --pending-rows: 12; \}/);
  /* every row is exactly one slot tall, so the viewport's bottom edge can never
     land mid-row (the old max-height 11rem / 40.98px pairing showed 4.29 rows,
     which is literally the "can't see more than 4" report) */
  assert.match(rowBlock, /height:\s*var\(--pending-row-h\)/);
  assert.doesNotMatch(rowBlock, /border-bottom/);
  /* the separator is an inset shadow on the row, so it costs no layout, travels
     with the row during an exit collapse and cannot dangle under the last one */
  assert.match(css, /\.pending-row \+ \.pending-row \{ box-shadow: inset 0 1px 0 var\(--border\); \}/);
  /* the empty state is an overlay, so it can never contribute height */
  assert.match(css, /\.pending-empty \{[^}]*position:\s*absolute[^}]*inset:\s*0/);
});

test('the leave animation always finishes before the model deletes the row', () => {
  const ms = (name) => Number(pendingBlock.match(new RegExp(`--pending-${name}:\\s*(\\d+)ms`))[1]);
  const confirmedMs = Number(
    modelSrc.match(/confirmedMs = Math\.max\(0, Number\(options\.confirmedMs\) \|\| (\d+)\)/)[1],
  );
  const droppedMs = Number(
    modelSrc.match(/droppedMs = Math\.max\(0, Number\(options\.droppedMs\) \|\| (\d+)\)/)[1],
  );
  assert.ok(ms('confirm-hold') + ms('confirm-exit') < confirmedMs);
  assert.ok(ms('drop-hold') + ms('drop-exit') < droppedMs);
  /* rows LEAVE by collapsing inside the frame, never by vanishing */
  assert.match(css, /@keyframes pending-exit \{[\s\S]*?to \{[^}]*height:\s*0/);
  assert.match(css, /@keyframes pending-exit-drop \{[\s\S]*?to \{[^}]*height:\s*0/);
  assert.match(
    css,
    /\.pending-row\.pending-confirmed \{[^}]*animation:\s*pending-exit var\(--pending-confirm-exit\)[^}]*var\(--pending-confirm-hold\)/,
  );
  assert.match(
    css,
    /\.pending-row\.pending-dropped \{[^}]*animation:\s*pending-exit-drop var\(--pending-drop-exit\)[^}]*var\(--pending-drop-hold\)/,
  );
});

test('the confirm state is declared statically and is never a full-row wash', () => {
  assert.doesNotMatch(css, /@keyframes pending-confirm/);
  assert.doesNotMatch(css, /@keyframes pending-drop\b/);
  /* the tint is a bounded, moving band on a pseudo-element — not the row's own
     background, which is what made the screenshot a flat grey-green slab */
  assert.match(css, /\.pending-row\.pending-confirmed::after \{[^}]*linear-gradient\(90deg/);
  assert.match(css, /@keyframes pending-sweep \{ from \{ transform: translateX\(-100%\)/);
  /* all four confirm signals are ordinary declarations, so reduced motion still
     shows a genuinely confirmed row */
  assert.match(css, /\.pending-row\.pending-confirmed::before \{[^}]*background:\s*var\(--accent\)/);
  assert.match(css, /\.pending-row\.pending-confirmed \.pending-name \{[^}]*color:\s*var\(--accent\)/);
  assert.match(css, /\.pending-row\.pending-confirmed \.pending-state \{[^}]*color:\s*var\(--accent\)/);
  assert.match(css, /\.pending-row\.pending-confirmed \.pending-mark::before \{/);
  /* accent on the coin name MEANS confirmed, so hover must not borrow it */
  assert.match(
    css,
    /\.pending-row:hover \.pending-name, \.pending-row:focus-visible \.pending-name \{\n\s*text-decoration: underline;/,
  );
  /* reduced motion names the resolved selectors — a media query adds no specificity */
  const reduce = css.slice(css.lastIndexOf('@media (prefers-reduced-motion: reduce)'));
  assert.match(reduce, /\.pending-row\.pending-confirmed,[\s\S]*?\.pending-row\.pending-dropped \{ animation: none; \}/);
  assert.match(reduce, /\.pending-row\.pending-confirmed::after \{ display: none; \}/);
});

test('the feed keeps surviving rows mounted so their animations cannot restart', () => {
  assert.doesNotMatch(renderPendingSrc, /createDocumentFragment|replaceChildren/);
  assert.match(renderPendingSrc, /host\.insertBefore\(row, host\.children\[i\] \|\| null\)/);
  assert.match(renderPendingSrc, /if \(host\.children\[i\] !== row\)/);
  /* The reuse key is the TXID alone. applySnapshot mints a fresh generation for
     every row it does not carry over, so keying on `txid|generation` rebuilt the
     whole feed on every SSE reconnect and every tab return — the remount this
     rewrite exists to stop. */
  assert.match(renderPendingSrc, /new Set\(rows\.map\(\(data\) => data\.txid\)\)/);
  assert.doesNotMatch(renderPendingSrc, /\$\{data\.txid\}\|\$\{data\.generation\}/);
  /* a re-broadcast still gets a fresh node, detected from the DOM: a row still
     wearing a resolved class while the model reports it pending IS the re-entry */
  assert.match(renderPendingSrc, /const reborn = Boolean\(prior\) && !data\.resolution/);
  assert.match(renderPendingSrc, /prior\.classList\.contains\('pending-confirmed'\)/);
  /* leftovers are pruned BEFORE the position walk, so no survivor is ever moved */
  assert.ok(renderPendingSrc.indexOf('el.remove()') < renderPendingSrc.indexOf('insertBefore'));
  /* a pruned row can hold focus, and a detached activeElement drops focus to
     <body> — which restarts the next Tab at the top of the document */
  assert.match(renderPendingSrc, /if \(focused && !focused\.isConnected\)/);
  assert.match(renderPendingSrc, /\.focus\(\{ preventScroll: true \}\)/);
});

test('nothing programmatically scrolls the mempool feed', () => {
  assert.doesNotMatch(app, /scrollTop\s*=\s*\w+\.scrollHeight/);
  assert.doesNotMatch(renderPendingSrc, /scrollTop|scrollIntoView/);
  /* newest-first at the top is what removes the need for any autoscroll */
  assert.match(renderPendingSrc, /\[\.\.\.view\.rows\]\.reverse\(\)/);
  assert.match(frameBlock, /overflow-anchor:\s*auto/);
  /* the affordance for rows 7-24: a real thin bar on modern engines, the WebKit
     pseudo-elements on older ones, and a paint-only depth cue when full */
  assert.match(frameBlock, /scrollbar-width:\s*thin/);
  assert.match(css, /#pending-row::-webkit-scrollbar-thumb \{ background: var\(--border-strong\)/);
  assert.match(css, /#pending-row\.is-overflowing::after \{[^}]*position: sticky[^}]*height: 15px; margin-top: -15px/);
  assert.match(renderPendingSrc, /classList\.toggle\('is-overflowing'/);
});

test('the mempool feed is a browsable log and only its connection announces', () => {
  const section = index.match(/<section class="pending"[\s\S]*?<\/section>/)[0];
  assert.doesNotMatch(section, /id="section-pending"[^>]*aria-live/);
  /* the count is readable in the heading but is NOT a live region: it was
     rewritten from every model mutation, i.e. re-announced several times a
     second on a busy mempool. Only a connection change interrupts a reader. */
  assert.doesNotMatch(section, /id="pending-count"[^>]*aria-live/);
  assert.match(section, /class="sr-only" id="pending-announce" aria-live="polite" aria-atomic="true"/);
  assert.match(app, /function announcePendingConnection\(connection\) \{/);
  assert.match(app, /if \(!live \|\| connection === pendingSpoken\) return;/);
  assert.match(section, /id="pending-row" role="log" aria-live="off"[^>]*tabindex="0"/);
  assert.match(
    section,
    /<button type="button" class="pending-more"[^>]*data-action="pending-expand"[^>]*aria-expanded=/,
  );
  /* 24 links behind a 6-slot frame stay out of the sequential tab order: the
     frame is the single stop and arrows move a roving focus inside it */
  assert.match(app, /row\.tabIndex = -1;/);
  assert.match(app, /function bindPendingKeys\(host\) \{/);
  assert.match(renderPendingSrc, /bindPendingKeys\(host\);/);
  /* the module cache-buster moved with the change */
  assert.match(index, /src="\/app\.js\?v=20260726-mempool"/);
});

test('the wait indicator is CSS-drawn and needs no hidden attribute', () => {
  assert.doesNotMatch(app, /pending-spin|\u23f3/);
  assert.doesNotMatch(css, /pending-spin/);
  assert.doesNotMatch(app, /spin\.hidden/);
  /* one always-present mark; the ROW's class picks its appearance, so an author
     `display` can never outrank the UA's [hidden] rule again */
  assert.match(css, /\.pending-mark::after \{[^}]*conic-gradient[\s\S]*?animation: radar-spin/);
  assert.match(css, /\.pending-row\.pending-confirmed \.pending-mark::after \{ display: none; \}/);
  assert.match(css, /\.pending-row\.pending-dropped \.pending-mark::after \{ display: none; \}/);
});

test('the expanded row count is a persisted preference mirrored on every single render', () => {
  assert.match(stateSrc, /pendingExpanded:\s*false/);
  assert.match(stateSrc, /localStorage\.getItem\('kascov-mempool-expanded'\) === '1'/);
  assert.match(app, /'pending-expand'\(el\) \{/);
  assert.match(app, /localStorage\.setItem\('kascov-mempool-expanded'/);
  /* the toggle's only effect is the CSS row count, mirrored onto the section */
  assert.match(app, /section\.dataset\.expanded = state\.pendingExpanded \? 'true' : 'false'/);
  /* BOTH render paths write it. Skipping the probing path left the first paint
     without [data-expanded], so a reader who had chosen "show more" got a
     6-slot frame and then a 12-slot one a round-trip later: a 264px shove on
     every load, for exactly the people who opened the feed up. */
  assert.match(app, /function syncPendingChrome\(section, connection, total, shown\) \{/);
  assert.match(renderPendingSrc, /syncPendingChrome\(section, 'connecting', 0, 0\);/);
  assert.match(renderPendingSrc, /syncPendingChrome\(section, view\.connection, view\.total, view\.rows\.length\);/);
  /* and the static markup already carries both attributes, so even the frame
     before app.js runs is the same height as the frame after it */
  assert.match(index, /id="section-pending"[^>]*data-connection="connecting" data-expanded="false"/);
  /* the count is honest about the DOM cap instead of promising rows that the
     row cap never rendered */
  assert.match(app, /total > shown \? `\$\{shown\}\/\$\{total\} · \$\{label\}`/);
});
