/* Network-aware hash construction lives outside the DOM-heavy app so route
   transitions can be regression-tested without booting the whole explorer. */

const NETWORKS = new Set(['testnet-10', 'mainnet']);

function routeParam(value) {
  return encodeURIComponent(String(value)).replace(/[!'()*]/g, (char) =>
    `%${char.charCodeAt(0).toString(16).toUpperCase()}`);
}

function networkRouteHash(route, network) {
  if (!NETWORKS.has(network)) throw new Error(`unknown network: ${network}`);
  const view = route && route.view;
  const id = route && route.id;

  /* Coin and decoded-token ids are network-specific. Switching their network
     should land somewhere valid instead of manufacturing a guaranteed 404. */
  if (view === 'detail') return `#/${network}/explore`;
  if (view === 'token') return `#/${network}/tokens`;

  if (view === 'address') return `#/${network}/addr/${id}`;
  if (view === 'lane') return `#/${network}/lane/${id}`;
  if (view === 'tokens') return `#/${network}/tokens`;
  if (view === 'tx') return `#/${network}/tx/${id}`;
  if (view === 'explore') {
    return `#/${network}/explore${route.galaxy ? '?galaxy=1' : ''}`;
  }
  if (view === 'decode') {
    return `#/${network}/decode${route.s ? `?s=${routeParam(route.s)}` : ''}`;
  }
  if (view === 'dev' || view === 'build' || view === 'preflight') {
    return `#/${network}/${view}`;
  }
  /* not network-scoped: switching networks must leave you where you were
     instead of dropping you on the landing page */
  if (view === 'changelog') return '#/changelog';
  if (view === 'guide') return '#/guide';
  return '#/';
}

export { networkRouteHash };
