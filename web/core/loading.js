/* Route and transfer policy kept pure so the expensive-loading rules can be
   regression-tested without booting the DOM-heavy application module. */

const SLOW_CONNECTION_RE = /(^|-)2g$/;

function routeNeedsSnapshot(view) {
  return view === 'landing' || view === 'explore';
}

function connectionAllowsHeavyData(connection, coarsePointer = false) {
  if (connection && connection.saveData) return false;
  if (connection && SLOW_CONNECTION_RE.test(String(connection.effectiveType || ''))) return false;
  return !coarsePointer;
}

function galaxyPreloadPolicy({
  explicit = false,
  preference = null,
  connection = null,
  coarsePointer = false,
} = {}) {
  if (explicit) return 'open';
  if (preference !== 'open') return 'none';
  return connectionAllowsHeavyData(connection, coarsePointer) ? 'restore' : 'none';
}

export { routeNeedsSnapshot, connectionAllowsHeavyData, galaxyPreloadPolicy };
