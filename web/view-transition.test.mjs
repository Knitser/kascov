import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const app = readFileSync(new URL('./app.js', import.meta.url), 'utf8');
const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');

test('live rerenders cannot restart the current view entrance fade', () => {
  const helper = app.slice(
    app.indexOf('function enterView('),
    app.indexOf('\nasync function render()', app.indexOf('function enterView(')),
  );
  assert.match(helper, /viewName !== lastView\) fadeIn\(view\)/);
  assert.match(helper, /else view\.classList\.remove\('is-entering'\)/);
  assert.equal(
    (app.match(/enterView\(views\[route\.view\], route\.view\)/g) || []).length,
    3,
  );
  assert.doesNotMatch(app, /fadeIn\(views\[route\.view\]\)/);
});

test('programmatic route focus stays accessible without painting a page-sized focus ring', () => {
  const helper = app.slice(
    app.indexOf('function finishViewNavigation('),
    app.indexOf('\n/* Data/SSE refreshes', app.indexOf('function finishViewNavigation(')),
  );
  assert.match(helper, /target\.classList\.add\('route-focus-target'\)/);
  assert.match(helper, /target\.classList\.remove\('route-focus-target'\)/);
  assert.match(
    css,
    /\.route-focus-target:focus\s*\{\s*outline:\s*none;\s*\}/,
  );
});
