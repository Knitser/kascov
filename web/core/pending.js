/* kascov core/pending — deterministic client model for the live mempool.
   Snapshot reconciliation and SSE mutations meet here so the UI never has to
   guess which response is newer. */

function createPendingModel(options = {}) {
  const rowCap = Math.max(1, Number(options.rowCap) || 24);
  const liveCap = Math.max(rowCap, Number(options.liveCap) || 128);
  const now = typeof options.now === 'function' ? options.now : Date.now;
  const setTimer = typeof options.setTimer === 'function' ? options.setTimer : setTimeout;
  const clearTimer = typeof options.clearTimer === 'function' ? options.clearTimer : clearTimeout;
  const confirmedMs = Math.max(0, Number(options.confirmedMs) || 900);
  const droppedMs = Math.max(0, Number(options.droppedMs) || 700);
  const onChange = typeof options.onChange === 'function' ? options.onChange : () => {};
  const entries = new Map();
  const tombstones = new Map();
  let mutation = 0;
  let reconcileId = 0;
  let generation = 0;
  let connection = 'offline';

  const eventsFor = (row) => {
    const source = row && Array.isArray(row.events) && row.events.length
      ? row.events
      : [row];
    const seen = new Set();
    const events = [];
    for (const event of source) {
      const covenantId = event && (event.covenantId || event.covenant_id);
      if (typeof covenantId !== 'string' || !covenantId) continue;
      const txKind = event.txKind || event.tx_kind || 'pending';
      const key = `${covenantId}\u0001${txKind}`;
      if (seen.has(key)) continue;
      seen.add(key);
      events.push({ covenantId, txKind });
    }
    return events;
  };

  const mergeEvents = (left, right) => {
    const seen = new Set();
    return [...left, ...right].filter((event) => {
      const key = `${event.covenantId}\u0001${event.txKind}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  };

  const add = (row, version) => {
    if (!row || !row.txid) return null;
    const events = eventsFor(row);
    const entry = {
      txid: String(row.txid),
      covenantIds: [...new Set(events.map((event) => event.covenantId))],
      events,
      txKind: row.tx_kind || (events[0] && events[0].txKind) || 'pending',
      resolution: null,
      version,
      generation: ++generation,
      enteredAt: row.ts != null ? Number(row.ts) : now(),
      timer: 0,
    };
    entries.set(entry.txid, entry);
    return entry;
  };

  const trim = () => {
    while (entries.size > liveCap) entries.delete(entries.keys().next().value);
  };

  return {
    beginReconcile() {
      return { id: ++reconcileId, version: mutation };
    },

    applySnapshot(ticket, snapshot) {
      if (!ticket || ticket.id !== reconcileId) return false;
      const rows = snapshot && Array.isArray(snapshot.pending) ? snapshot.pending : [];
      const next = new Map();
      const seen = new Set();
      for (const row of rows) {
        if (!row || !row.txid) continue;
        const txid = String(row.txid);
        seen.add(txid);
        const current = entries.get(txid);
        const tombstone = tombstones.get(txid);
        if (tombstone && tombstone.version > ticket.version) continue;
        if (current && current.version > ticket.version) {
          const events = mergeEvents(eventsFor(row), current.events);
          current.events = events;
          current.covenantIds = [...new Set(events.map((event) => event.covenantId))];
          next.set(txid, current);
        } else {
          const events = eventsFor(row);
          next.set(txid, {
            txid,
            covenantIds: [...new Set(events.map((event) => event.covenantId))],
            events,
            txKind: row.tx_kind || (events[0] && events[0].txKind) || 'pending',
            resolution: null,
            version: ticket.version,
            generation: ++generation,
            enteredAt: row.age_ms != null ? now() - Math.max(0, Number(row.age_ms) || 0) : now(),
            timer: 0,
          });
        }
      }
      for (const [txid, entry] of entries) {
        if (entry.version > ticket.version && !seen.has(txid)) next.set(txid, entry);
      }
      entries.clear();
      for (const [txid, entry] of next) entries.set(txid, entry);
      for (const [txid, tombstone] of tombstones) {
        if (tombstone.version <= ticket.version) tombstones.delete(txid);
      }
      trim();
      onChange();
      return true;
    },

    pending(row) {
      if (!row || !row.txid) return false;
      mutation += 1;
      const prior = entries.get(String(row.txid));
      if (prior && !prior.resolution) {
        prior.events = mergeEvents(prior.events, eventsFor(row));
        prior.covenantIds = [...new Set(prior.events.map((event) => event.covenantId))];
        prior.version = mutation;
        tombstones.delete(String(row.txid));
        onChange();
        return true;
      }
      if (prior && prior.timer) clearTimer(prior.timer);
      entries.delete(String(row.txid));
      tombstones.delete(String(row.txid));
      add(row, mutation);
      trim();
      onChange();
      return true;
    },

    resolve(row) {
      if (!row || !row.txid) return false;
      const txid = String(row.txid);
      mutation += 1;
      const entry = entries.get(txid);
      if (!entry) {
        tombstones.set(txid, { version: mutation });
        while (tombstones.size > liveCap) tombstones.delete(tombstones.keys().next().value);
        onChange();
        return true;
      }
      entry.resolution = row.resolution || 'confirmed';
      entry.version = mutation;
      const entryGeneration = entry.generation;
      const delay = entry.resolution === 'dropped' ? droppedMs : confirmedMs;
      if (entry.timer) clearTimer(entry.timer);
      entry.timer = setTimer(() => {
        const current = entries.get(txid);
        if (!current || current.generation !== entryGeneration) return;
        entries.delete(txid);
        mutation += 1;
        tombstones.set(txid, { version: mutation });
        while (tombstones.size > liveCap) tombstones.delete(tombstones.keys().next().value);
        onChange();
      }, delay);
      onChange();
      return true;
    },

    setConnection(value) {
      const next = ['offline', 'connecting', 'live', 'retrying', 'paused'].includes(value)
        ? value
        : 'offline';
      if (next === connection) return;
      connection = next;
      onChange();
    },

    view() {
      const all = [...entries.values()];
      return {
        total: all.length,
        connection,
        rows: all.slice(-rowCap).map((entry) => ({
          txid: entry.txid,
          covenantIds: [...entry.covenantIds],
          events: entry.events.map((event) => ({ ...event })),
          txKind: entry.txKind,
          resolution: entry.resolution,
          generation: entry.generation,
          enteredAt: entry.enteredAt,
        })),
      };
    },
  };
}

export { createPendingModel };
