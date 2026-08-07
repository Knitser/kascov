import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const windowsCaddy = readFileSync(
  new URL('../scripts/kascov.windows.Caddyfile', import.meta.url),
  'utf8',
);
const wslCaddy = readFileSync(
  new URL('../scripts/kascov.Caddyfile', import.meta.url),
  'utf8',
);
const preview = readFileSync(
  new URL('../scripts/dev-serve.mjs', import.meta.url),
  'utf8',
);

test('every Caddy template sends OpenAPI to the worker before the SPA fallback', () => {
  for (const config of [windowsCaddy, wslCaddy]) {
    const workerMatcher = config
      .split('\n')
      .find((line) => line.includes('path /data/* /share/*'));
    assert.ok(workerMatcher, 'worker path matcher must exist');
    assert.match(workerMatcher, /\/openapi\.json/);
  }
});

test('the local preview exposes OpenAPI and supports a localhost HTTP worker safely', () => {
  assert.match(preview, /WORKER_EXACT = new Set\(\[[\s\S]*?'\/openapi\.json'/);
  assert.match(preview, /request as httpRequest/);
  assert.match(preview, /UPSTREAM_URL\.protocol/);
  assert.match(preview, /LOCAL_UPSTREAM/);
});
