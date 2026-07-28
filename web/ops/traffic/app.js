const state = {
  snapshot: null,
  window: '24h',
  loading: false,
};

const $ = (selector) => document.querySelector(selector);
const svgNs = 'http://www.w3.org/2000/svg';

function svgNode(name, attributes = {}, text = '') {
  const node = document.createElementNS(svgNs, name);
  for (const [key, value] of Object.entries(attributes)) node.setAttribute(key, String(value));
  if (text) node.textContent = text;
  return node;
}

function formatNumber(value) {
  return new Intl.NumberFormat(undefined, { notation: value >= 100000 ? 'compact' : 'standard' })
    .format(Number(value || 0));
}

function formatBytes(value) {
  let amount = Number(value || 0);
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let index = 0;
  while (amount >= 1024 && index < units.length - 1) {
    amount /= 1024;
    index += 1;
  }
  return `${amount.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

function formatLatency(value) {
  if (typeof value === 'string') return `${value}s`;
  const seconds = Number(value || 0);
  return seconds < 1 ? `≤${Math.round(seconds * 1000)}ms` : `≤${seconds.toFixed(1)}s`;
}

function formatTime(value, options = {}) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'unknown';
  return new Intl.DateTimeFormat(undefined, options).format(date);
}

function metricCard(label, value, note, primary = false) {
  const article = document.createElement('article');
  article.className = `metric${primary ? ' is-primary' : ''}`;
  const labelNode = document.createElement('p');
  labelNode.className = 'metric-label';
  labelNode.textContent = label;
  const valueNode = document.createElement('p');
  valueNode.className = 'metric-value';
  valueNode.textContent = value;
  const noteNode = document.createElement('p');
  noteNode.className = 'metric-note';
  noteNode.textContent = note;
  article.append(labelNode, valueNode, noteNode);
  return article;
}

function renderMetrics(report) {
  const active = state.snapshot.windows['5m']?.visitors?.active_browsers_approx || 0;
  const visitors = report.visitors || {};
  const requests = report.requests || {};
  const latency = report.latency_seconds || {};
  $('#metric-grid').replaceChildren(
    metricCard('Active browsers', formatNumber(active), 'page or first-party API activity · last 5m', true),
    metricCard('Unique browsers', formatNumber(visitors.unique_browsers_approx), `approximate · ${state.window}`),
    metricCard('Page views', formatNumber(visitors.page_views), `${formatNumber(visitors.sessions_30m)} sessions`),
    metricCard('API calls', formatNumber(requests.api_calls), `${formatNumber(requests.external_api_calls_approx)} external / unknown`),
    metricCard('All requests', formatNumber(requests.total), `${formatNumber(requests.bot_requests)} crawler / monitor`),
    metricCard('Errors', formatNumber(requests.errors), '4xx and 5xx responses'),
    metricCard('Response bytes', formatBytes(requests.bytes_out), 'served before browser caching'),
    metricCard('Latency p95', formatLatency(latency.p95_upper_bound), `p50 ${formatLatency(latency.p50_upper_bound)}`),
  );
}

function linePath(series, field, width, height, maxValue) {
  if (!series.length) return '';
  return series.map((point, index) => {
    const x = series.length === 1 ? width / 2 : (index / (series.length - 1)) * width;
    const y = height - (Number(point[field] || 0) / maxValue) * height;
    return `${index ? 'L' : 'M'}${x.toFixed(2)},${y.toFixed(2)}`;
  }).join(' ');
}

function renderRequestChart(series = []) {
  const svg = $('#request-chart');
  const width = 820;
  const height = 220;
  const left = 58;
  const top = 20;
  svg.replaceChildren();

  const title = svgNode('title', {}, `Requests and API calls during the selected ${state.window} window`);
  svg.append(title);
  if (!series.length) {
    svg.append(svgNode('text', { x: 450, y: 150, class: 'chart-empty' }, 'No requests in this window'));
    return;
  }

  const maxValue = Math.max(1, ...series.map((point) => Number(point.requests || 0)));
  const defs = svgNode('defs');
  const gradient = svgNode('linearGradient', { id: 'request-fill', x1: 0, x2: 0, y1: 0, y2: 1 });
  gradient.append(
    svgNode('stop', { offset: '0%', 'stop-color': '#49e2cf', 'stop-opacity': 0.22 }),
    svgNode('stop', { offset: '100%', 'stop-color': '#49e2cf', 'stop-opacity': 0 }),
  );
  defs.append(gradient);
  svg.append(defs);

  const plot = svgNode('g', { transform: `translate(${left} ${top})` });
  for (let index = 0; index <= 4; index += 1) {
    const y = (index / 4) * height;
    const value = Math.round(maxValue * (1 - index / 4));
    plot.append(svgNode('line', { x1: 0, x2: width, y1: y, y2: y, class: 'chart-grid' }));
    plot.append(svgNode('text', { x: -12, y: y + 4, class: 'chart-label', 'text-anchor': 'end' }, formatNumber(value)));
  }

  const totalPath = linePath(series, 'requests', width, height, maxValue);
  const apiPath = linePath(series, 'api_calls', width, height, maxValue);
  plot.append(svgNode('path', { d: `${totalPath} L${width},${height} L0,${height} Z`, class: 'chart-area' }));
  plot.append(svgNode('path', { d: totalPath, class: 'chart-line-total' }));
  plot.append(svgNode('path', { d: apiPath, class: 'chart-line-api' }));

  const labelIndexes = [...new Set([0, Math.floor((series.length - 1) / 2), series.length - 1])];
  for (const index of labelIndexes) {
    const x = series.length === 1 ? width / 2 : (index / (series.length - 1)) * width;
    const anchor = index === 0 ? 'start' : index === series.length - 1 ? 'end' : 'middle';
    const label = formatTime(series[index].timestamp, {
      month: state.window === '30d' ? 'short' : undefined,
      day: state.window === '7d' || state.window === '30d' ? 'numeric' : undefined,
      hour: '2-digit',
      minute: state.window === '5m' || state.window === '24h' ? '2-digit' : undefined,
    });
    plot.append(svgNode('text', { x, y: height + 28, class: 'chart-label', 'text-anchor': anchor }, label));
  }
  svg.append(plot);
}

function renderApiSource(requests = {}) {
  const first = Number(requests.first_party_api_calls || 0);
  const external = Number(requests.external_api_calls_approx || 0);
  const total = Math.max(0, first + external);
  const firstPercent = total ? (first / total) * 100 : 0;
  const externalPercent = total ? 100 - firstPercent : 0;
  const host = $('#api-source');
  host.replaceChildren();

  const totalNode = document.createElement('p');
  totalNode.className = 'source-total';
  const totalStrong = document.createElement('strong');
  totalStrong.textContent = formatNumber(total);
  totalNode.append(totalStrong, document.createTextNode(` API calls during ${state.window}`));

  const bar = document.createElement('div');
  bar.className = 'source-bar';
  bar.setAttribute('aria-label', `${firstPercent.toFixed(0)}% first-party, ${externalPercent.toFixed(0)}% external`);
  const firstBar = document.createElement('span');
  firstBar.className = 'source-first';
  firstBar.style.width = `${firstPercent}%`;
  const externalBar = document.createElement('span');
  externalBar.className = 'source-external';
  externalBar.style.width = `${externalPercent}%`;
  bar.append(firstBar, externalBar);

  const legend = document.createElement('div');
  legend.className = 'source-legend';
  const items = [
    ['kascov UI', first, firstPercent],
    ['external / unknown', external, externalPercent],
  ];
  for (const [label, value, percent] of items) {
    const item = document.createElement('div');
    item.className = 'source-item';
    const strong = document.createElement('strong');
    strong.textContent = `${formatNumber(value)} · ${percent.toFixed(0)}%`;
    item.append(strong, document.createTextNode(label));
    legend.append(item);
  }
  host.append(totalNode, bar, legend);
}

function renderStatuses(statuses = {}) {
  const host = $('#status-classes');
  host.replaceChildren();
  const entries = Object.entries(statuses);
  const total = Math.max(1, entries.reduce((sum, [, value]) => sum + Number(value || 0), 0));
  if (!entries.length) {
    const empty = document.createElement('p');
    empty.className = 'metric-note';
    empty.textContent = 'No responses in this window.';
    host.append(empty);
    return;
  }
  for (const [label, value] of entries) {
    const row = document.createElement('div');
    row.className = `status-row${label.startsWith('4') || label.startsWith('5') ? ' is-error' : label.startsWith('3') ? ' is-redirect' : ''}`;
    const name = document.createElement('span');
    name.textContent = label;
    const track = document.createElement('div');
    track.className = 'status-track';
    const fill = document.createElement('span');
    fill.style.width = `${Math.max(1, Number(value) / total * 100)}%`;
    track.append(fill);
    const count = document.createElement('span');
    count.className = 'status-count';
    count.textContent = formatNumber(value);
    row.append(name, track, count);
    host.append(row);
  }
}

function renderTable(selector, values = {}) {
  const body = $(selector);
  body.replaceChildren();
  const entries = Object.entries(values);
  if (!entries.length) {
    const row = document.createElement('tr');
    row.className = 'empty-row';
    const cell = document.createElement('td');
    cell.colSpan = 2;
    cell.textContent = 'No matching traffic in this window.';
    row.append(cell);
    body.append(row);
    return;
  }
  for (const [name, value] of entries) {
    const row = document.createElement('tr');
    const nameCell = document.createElement('td');
    nameCell.title = name;
    nameCell.textContent = name;
    const valueCell = document.createElement('td');
    valueCell.textContent = formatNumber(value);
    row.append(nameCell, valueCell);
    body.append(row);
  }
}

function setLiveState(generatedAt, failed = false) {
  const host = $('#live-state');
  host.classList.remove('is-live', 'is-stale', 'is-error');
  if (failed) {
    host.classList.add('is-error');
    $('#live-label').textContent = 'snapshot unavailable';
    return;
  }
  const ageSeconds = Math.max(0, (Date.now() - new Date(generatedAt).getTime()) / 1000);
  host.classList.add(ageSeconds <= 180 ? 'is-live' : 'is-stale');
  $('#live-label').textContent = ageSeconds <= 180
    ? `live · ${formatTime(generatedAt, { hour: '2-digit', minute: '2-digit', second: '2-digit' })}`
    : `stale · ${formatTime(generatedAt, { dateStyle: 'medium', timeStyle: 'short' })}`;
}

function render() {
  if (!state.snapshot) return;
  const report = state.snapshot.windows?.[state.window];
  if (!report) return;
  renderMetrics(report);
  renderRequestChart(report.series);
  renderApiSource(report.requests);
  renderStatuses(report.status_classes);
  renderTable('#api-table', report.top_api_endpoints);
  renderTable('#page-table', report.top_pages);
  setLiveState(state.snapshot.generated_at);

  const first = report.window?.first;
  const last = report.window?.last;
  $('#coverage').textContent = first && last
    ? `Coverage: ${formatTime(first, { dateStyle: 'medium', timeStyle: 'short' })} → ${formatTime(last, { dateStyle: 'medium', timeStyle: 'short' })}`
    : `No matching requests during ${state.window}.`;
}

async function loadSnapshot() {
  if (state.loading) return;
  state.loading = true;
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 10000);
  try {
    const response = await fetch(`./traffic.json?t=${Date.now()}`, {
      cache: 'no-store',
      credentials: 'same-origin',
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`traffic snapshot returned HTTP ${response.status}`);
    const snapshot = await response.json();
    if (snapshot.schema !== 1 || !snapshot.windows) throw new Error('traffic snapshot has an unsupported format');
    state.snapshot = snapshot;
    $('#error-notice').hidden = true;
    render();
  } catch (error) {
    const notice = $('#error-notice');
    notice.textContent = `Could not refresh traffic: ${error.message || error}`;
    notice.hidden = false;
    setLiveState(state.snapshot?.generated_at, !state.snapshot);
  } finally {
    clearTimeout(timeout);
    state.loading = false;
  }
}

document.querySelectorAll('[data-window]').forEach((button) => {
  button.addEventListener('click', () => {
    state.window = button.dataset.window;
    document.querySelectorAll('[data-window]').forEach((candidate) => {
      candidate.setAttribute('aria-pressed', String(candidate === button));
    });
    render();
  });
});

loadSnapshot();
setInterval(() => {
  if (document.visibilityState === 'visible') loadSnapshot();
}, 30000);
