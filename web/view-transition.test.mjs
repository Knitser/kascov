import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const app = readFileSync(new URL('./app.js', import.meta.url), 'utf8');

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
