import { expect, test } from '@playwright/test';

const emptySnapshot = JSON.stringify({
  covenants: [],
  generated_at_ms: 1_782_988_769_573,
  network: 'testnet-10',
  stats: {
    active: 0,
    burned: 0,
    covenants: 0,
    events: 0,
    last_activity_daa: 0,
    live_value: 0,
  },
  tip_at_ms: 1_782_988_769_573,
  tip_daa: 0,
});

async function stubNetworkData(page) {
  await page.route('**/data/**', (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/data/testnet-10.json') {
      return route.fulfill({
        body: emptySnapshot,
        contentType: 'application/json',
        status: 200,
      });
    }
    return route.fulfill({
      body: '{"ok":false}',
      contentType: 'application/json',
      status: 404,
    });
  });
}

test.beforeEach(async ({ page }) => {
  await page.route('https://fonts.googleapis.com/**', (route) => route.abort());
  await page.route('https://fonts.gstatic.com/**', (route) => route.abort());
  await stubNetworkData(page);
});

test('explorer header stays usable through compact desktop widths', async ({ page }) => {
  await page.goto('/#/explore', { waitUntil: 'domcontentloaded' });
  await expect(page.locator('#header-search')).toBeVisible();

  for (const width of [721, 740, 768, 844]) {
    await page.setViewportSize({ width, height: 600 });

    const layout = await page.evaluate(() => {
      const search = document.querySelector('#search').getBoundingClientRect();
      const firstNav = document.querySelector('.site-nav .nav-link').getBoundingClientRect();
      const overlaps = !(
        search.right <= firstNav.left ||
        firstNav.right <= search.left ||
        search.bottom <= firstNav.top ||
        firstNav.bottom <= search.top
      );
      return {
        documentOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
        overlaps,
        searchWidth: search.width,
      };
    });

    expect(layout, `${width}px header layout`).toMatchObject({
      documentOverflow: 0,
      overlaps: false,
    });
    expect(layout.searchWidth, `${width}px search width`).toBeGreaterThanOrEqual(120);
  }
});

test('mobile navigation reveals overflow and keeps the active route visible', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 568 });
  await page.goto('/#/explore', { waitUntil: 'domcontentloaded' });
  await expect(page.locator('.site-nav')).toHaveClass(/can-scroll-right/);
  await expect(page.locator('.site-nav')).toHaveCSS('mask-image', /gradient/);

  await page.evaluate(() => {
    location.hash = '#/guide';
  });
  const active = page.locator('.site-nav [aria-current="page"]');
  await expect(active).toHaveText('guide');
  await expect.poll(async () => active.evaluate((link) => {
    const item = link.getBoundingClientRect();
    const rail = link.closest('.site-nav').getBoundingClientRect();
    return item.left >= rail.left - 1 && item.right <= rail.right + 1;
  })).toBe(true);
});

test('holder-map motion control keeps the touch-target baseline', async ({ browser }) => {
  const context = await browser.newContext({
    hasTouch: true,
    isMobile: true,
    viewport: { width: 390, height: 844 },
  });
  const page = await context.newPage();
  await page.route('https://fonts.googleapis.com/**', (route) => route.abort());
  await page.route('https://fonts.gstatic.com/**', (route) => route.abort());
  await stubNetworkData(page);
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  await page.evaluate(() => {
    const button = document.createElement('button');
    button.className = 'hb-motion';
    button.textContent = 'pause motion';
    document.body.appendChild(button);
  });

  await expect.poll(() => page.locator('.hb-motion').evaluate((button) => ({
    coarse: matchMedia('(pointer: coarse)').matches,
    height: button.getBoundingClientRect().height,
    minHeight: getComputedStyle(button).minHeight,
  }))).toMatchObject({
    coarse: true,
    minHeight: '44px',
  });
  await expect(page.locator('.hb-motion')).toHaveCSS('min-height', '44px');
  await context.close();
});

test('explorer exposes one descriptive page heading', async ({ page }) => {
  await page.goto('/#/explore', { waitUntil: 'domcontentloaded' });
  const headings = page.getByRole('heading', { level: 1 });
  await expect(headings).toHaveCount(1);
  await expect(headings).toHaveText('Explore Kaspa smart coins');
});

test('copy remains usable when clipboard permission is unavailable', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: () => Promise.reject(new DOMException('blocked', 'NotAllowedError')) },
    });
  });
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  await page.evaluate(() => {
    const button = document.createElement('button');
    button.id = 'copy-test-trigger';
    button.dataset.action = 'copy';
    button.dataset.copy = 'kaspa:test-value';
    button.textContent = 'copy';
    document.body.appendChild(button);
  });
  await page.locator('#copy-test-trigger').click();

  const dialog = page.locator('#manual-copy-dialog');
  await expect(dialog).toBeVisible();
  const value = dialog.locator('textarea');
  await expect(value).toHaveValue('kaspa:test-value');
  await expect(value).toBeFocused();
});
