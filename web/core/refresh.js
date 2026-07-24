/* A keyed, trailing refresh gate. Bursty callers share one in-flight task and
   collapse everything that arrives inside the interval into one final run. */

function createRefreshGate(intervalMs, {
  now = () => Date.now(),
  setTimer = (fn, ms) => setTimeout(fn, ms),
} = {}) {
  const slots = new Map();

  function run(key, task) {
    let slot = slots.get(key);
    if (!slot) {
      slot = {
        inflight: null,
        pending: null,
        timer: null,
        latestTask: null,
        lastStarted: null,
      };
      slots.set(key, slot);
    }
    slot.latestTask = task;

    if (slot.inflight) return slot.inflight;
    if (slot.pending) return slot.pending;

    const elapsed = slot.lastStarted == null ? intervalMs : now() - slot.lastStarted;
    const wait = Math.max(0, intervalMs - elapsed);
    if (wait > 0) {
      slot.pending = new Promise((resolve, reject) => {
        slot.timer = setTimer(() => {
          slot.timer = null;
          slot.pending = null;
          const next = slot.latestTask;
          slot.latestTask = null;
          if (!next) { resolve(null); return; }
          run(key, next).then(resolve, reject);
        }, wait);
      });
      return slot.pending;
    }

    slot.lastStarted = now();
    const current = slot.latestTask;
    slot.latestTask = null;
    const executing = Promise.resolve().then(current);
    slot.inflight = executing.finally(() => {
      slot.inflight = null;
      /* Something arrived while the task was running. `run` applies the
         remaining interval and schedules exactly one trailing refresh. */
      if (slot.latestTask) run(key, slot.latestTask);
    });
    return slot.inflight;
  }

  return { run };
}

export { createRefreshGate };
