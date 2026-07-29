import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const html = readFileSync(new URL('./ops/traffic/index.html', import.meta.url), 'utf8');
const css = readFileSync(new URL('./ops/traffic/style.css', import.meta.url), 'utf8');
const app = readFileSync(new URL('./ops/traffic/app.js', import.meta.url), 'utf8');
const caddy = readFileSync(new URL('../scripts/kascov.windows.Caddyfile', import.meta.url), 'utf8');
const timer = readFileSync(new URL('../scripts/kascov-traffic-snapshot.timer', import.meta.url), 'utf8');

test('private traffic dashboard exposes the agreed aggregate views', () => {
  assert.match(html, /data-window="5m"/);
  assert.match(html, /data-window="24h"/);
  assert.match(html, /data-window="7d"/);
  assert.match(html, /data-window="30d"/);
  assert.match(html, /id="request-chart"/);
  assert.match(html, /id="api-source"/);
  assert.match(html, /id="status-classes"/);
  assert.match(html, /id="api-table"/);
  assert.match(html, /id="page-table"/);
});

test('dashboard is private-by-design and has no third-party runtime', () => {
  assert.match(html, /noindex,\s*nofollow,\s*noarchive/);
  assert.match(html, /No cookies, tracking pixels, or visitor identifiers/);
  assert.doesNotMatch(html, /https?:\/\//);
  assert.doesNotMatch(app, /\.innerHTML\s*=/);
  assert.match(app, /cache:\s*'no-store'/);
  assert.match(app, /credentials:\s*'same-origin'/);
});

test('dashboard remains usable on phones and with reduced motion', () => {
  assert.match(css, /@media \(max-width: 680px\)/);
  assert.match(css, /\.metric-grid,\s*\.dashboard-grid\s*\{\s*grid-template-columns:\s*1fr/);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(css, /:focus-visible/);
});

test('production route requires authentication and refreshes out of band', () => {
  assert.match(caddy, /@trafficDashboard path \/ops\/traffic \/ops\/traffic\/\*/);
  assert.match(caddy, /basic_auth @trafficDashboard/);
  assert.match(caddy, /import secrets\/kascov-traffic-users/);
  assert.match(caddy, /Cache-Control "private, no-store"/);
  assert.match(caddy, /log_skip @trafficDashboard/);
  assert.match(timer, /OnUnitActiveSec=60s/);
});
