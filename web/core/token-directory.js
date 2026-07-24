const VALIDATION = new Set(['verified', 'invalid', 'unvalidated']);

function tokenValidation(token) {
  return token && VALIDATION.has(token.status) ? token.status : 'unknown';
}

function tokenLifecycle(token) {
  if (token && typeof token.alive === 'boolean') return token.alive ? 'alive' : 'retired';
  return token && token.status === 'active' ? 'alive' : 'retired';
}

function tokenSearchText(token) {
  return [
    token && token.covenant_id,
    token && token.name,
    token && token.claimed_name,
    token && token.claimed_ticker,
    token && token.template,
  ].filter(Boolean).join(' ').toLowerCase();
}

function selectTokens(tokens, options = {}) {
  const query = String(options.query || '').trim().toLowerCase();
  const validation = options.validation || 'all';
  const lifecycle = options.lifecycle || 'all';
  const sort = options.sort || 'holders';
  const num = (value) => {
    const n = Number(value);
    return Number.isFinite(n) ? n : 0;
  };

  const rows = (tokens || []).filter((token) =>
    (!query || tokenSearchText(token).includes(query)) &&
    (validation === 'all' || tokenValidation(token) === validation) &&
    (lifecycle === 'all' || tokenLifecycle(token) === lifecycle)
  );

  const score = {
    holders: (token) => num(token.holders),
    supply: (token) => num(token.supply),
    value: (token) => num(token.live_value),
    activity: (token) => num(token.last_activity_daa),
  };
  if (sort === 'name') {
    rows.sort((a, b) => tokenSearchText(a).localeCompare(tokenSearchText(b)));
  } else {
    const get = score[sort] || score.holders;
    rows.sort((a, b) => get(b) - get(a) ||
      score.activity(b) - score.activity(a) ||
      tokenSearchText(a).localeCompare(tokenSearchText(b)));
  }
  return rows;
}

export { selectTokens, tokenLifecycle, tokenSearchText, tokenValidation };
