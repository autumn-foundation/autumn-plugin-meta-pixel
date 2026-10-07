// End-to-end check in a real browser (Chromium, Playwright).
//
// It starts examples/app with a CSP from `csp::with_meta_pixel_sources`,
// stubs the two Meta hosts, and checks the fbq calls that the page makes.
//
// Run:
//   cargo build --example app
//   NODE_PATH=<dir with playwright> node tests/e2e/browser.mjs [path/to/app]

import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import assert from 'node:assert/strict';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

// autumn's default CSP, fixed by the plugin (the text of the startup error).
const CSP = "default-src 'self'; img-src 'self' data: https://www.facebook.com; "
  + "style-src 'self' 'unsafe-inline'; script-src 'self' https://connect.facebook.net; "
  + "connect-src 'self' https://www.facebook.com; form-action 'self'; "
  + "frame-ancestors 'none'; base-uri 'self'";
const PIXEL = '1234567890';

// Stands in for fbevents.js: record each fbq call.
const FAKE_FBEVENTS = `(function () {
  var f = window.fbq;
  window.__calls = [];
  window.__disablePushState = f.disablePushState === true;
  f.callMethod = function () { window.__calls.push(Array.prototype.slice.call(arguments)); };
  f.queue.forEach(function (a) { window.__calls.push(Array.prototype.slice.call(a)); });
  f.queue = [];
})();`;

const freePort = () => new Promise((resolve) => {
  const s = createServer();
  s.listen(0, '127.0.0.1', () => {
    const { port } = s.address();
    s.close(() => resolve(port));
  });
});

const appPath = process.argv[2] || 'target/debug/examples/app';
const port = await freePort();
const base = `http://127.0.0.1:${port}`;
const app = spawn(appPath, [], {
  env: {
    ...process.env,
    AUTUMN_SERVER__HOST: '127.0.0.1',
    AUTUMN_SERVER__PORT: String(port),
    AUTUMN_SECURITY__HEADERS__CONTENT_SECURITY_POLICY: CSP,
    AUTUMN_META_PIXEL__PIXEL_IDS: PIXEL,
  },
  stdio: ['ignore', 'ignore', 'inherit'],
});

let browser;
try {
  for (let i = 0; ; i++) {
    try {
      if ((await fetch(base + '/')).ok) break;
    } catch {
      // Not up yet.
    }
    if (i > 300) throw new Error('the example app did not start');
    await new Promise((r) => setTimeout(r, 100));
  }

  browser = await chromium.launch();
  const page = await browser.newPage();
  const violations = [];
  const metaRequests = [];
  page.on('console', (m) => {
    if (/Content Security Policy|integrity/i.test(m.text())) violations.push(m.text());
  });
  await page.route('https://connect.facebook.net/**', (route) => {
    metaRequests.push(route.request().url());
    route.fulfill({ contentType: 'text/javascript', body: FAKE_FBEVENTS });
  });
  await page.route('https://www.facebook.com/**', (route) => {
    metaRequests.push(route.request().url());
    route.fulfill({ status: 200, body: '' });
  });
  const calls = async () => page.evaluate(() => window.__calls || null);
  // Waits until event `name` has fired `n` times. Gives all calls.
  const waitEvent = async (name, n = 1) => {
    await page.waitForFunction(
      ([k, m]) => (window.__calls || []).filter((x) => x[2] === k).length >= m,
      [name, n],
      { timeout: 10000 },
    );
    return calls();
  };
  const waitCalls = async (n) => {
    await page.waitForFunction((k) => (window.__calls || []).length >= k, n, { timeout: 10000 });
    return calls();
  };

  // 1. No consent: no pixel, no request to Meta.
  await page.goto(base + '/');
  await page.waitForLoadState('networkidle');
  assert.equal(await page.evaluate(() => typeof window.fbq), 'undefined');
  assert.equal(await page.locator('#autumn-meta-pixel-config').count(), 0);
  assert.deepEqual(metaRequests, []);

  // 2. Consent: init, PageView, and the ViewContent data block.
  await page.goto(base + '/accept');
  let c = await waitCalls(3);
  assert.deepEqual(c.slice(0, 3), [
    ['init', PIXEL],
    ['trackSingle', PIXEL, 'PageView'],
    ['trackSingle', PIXEL, 'ViewContent', {
      content_ids: ['sku-1'], content_type: 'product', value: 25, currency: 'EUR',
    }],
  ]);
  const loader = page.locator('script[src^="/static/_plugins/meta-pixel/meta-pixel."]');
  assert.equal(await loader.count(), 1);
  assert.match(await loader.getAttribute('integrity'), /^sha384-/);
  // The loader removes the noscript image when JavaScript runs.
  assert.equal(await page.locator('noscript').count(), 0);
  assert.equal(await page.evaluate(() => window.__disablePushState), false);

  // 3. Click: Contact.
  await page.click('#contact');
  c = await waitCalls(4);
  assert.deepEqual(c[3], ['trackSingle', PIXEL, 'Contact', {}]);

  // 4. htmx: AddToCart from HX-Trigger, CartOpened from the swapped block.
  await page.click('#add');
  c = await waitCalls(6);
  const names = c.slice(4).map((x) => x[2]).sort();
  assert.deepEqual(names, ['AddToCart', 'CartOpened']);
  assert.ok(c.some((x) => x[0] === 'trackSingleCustom' && x[2] === 'CartOpened'));
  // One more swap is a new response, so its block fires once more.
  await page.click('#add');
  c = await waitCalls(8);
  assert.equal(c.filter((x) => x[2] === 'CartOpened').length, 2);
  assert.equal(c.filter((x) => x[2] === 'AddToCart').length, 2);
  assert.equal(c.length, 8, JSON.stringify(c));

  // 5. A click with hx-trigger "consume" (stopPropagation) still fires.
  await page.click('#ask');
  await waitEvent('Lead');
  c = await waitEvent('CartOpened', 3);

  // 6. hx-boost to /thanks: Purchase with eventID fires once, no noscript hit.
  await page.click('#buy');
  await page.waitForURL(base + '/thanks');
  c = await waitEvent('Purchase');
  await page.waitForTimeout(300);
  c = await calls();
  const purchases = c.filter((x) => x[2] === 'Purchase');
  assert.deepEqual(purchases, [
    ['trackSingle', PIXEL, 'Purchase', { value: 25, currency: 'EUR' }, { eventID: 'order-1001' }],
  ]);

  // 7. Back: htmx restores the page from its history cache. No event fires again.
  const settled = c.length;
  await page.goBack();
  await page.waitForURL(base + '/');
  await page.waitForTimeout(500);
  c = await calls();
  assert.equal(c.filter((x) => x[2] === 'ViewContent').length, 1, JSON.stringify(c.slice(settled)));
  assert.ok(!metaRequests.some((u) => u.includes('noscript=1')), metaRequests.join('\n'));

  // 8. Withdraw consent: the loader revokes and sends no more events.
  await page.click('#withdraw');
  await page.waitForFunction(() => (window.__calls || []).some((x) => x[0] === 'consent'));
  c = await calls();
  assert.deepEqual(c.at(-1), ['consent', 'revoke']);
  const afterRevoke = c.length;
  await page.click('#contact');
  await page.waitForTimeout(300);
  assert.equal((await calls()).length, afterRevoke);
  // The next full load has no pixel.
  await page.goto(base + '/');
  assert.equal(await page.locator('#autumn-meta-pixel-config').count(), 0);
  await page.goto(base + '/accept');
  await waitCalls(3);

  // 9. A full load of /thanks: Purchase with eventID.
  await page.goto(base + '/thanks');
  c = await waitCalls(3);
  assert.deepEqual(c[2], ['trackSingle', PIXEL, 'Purchase', { value: 25, currency: 'EUR' }, { eventID: 'order-1001' }]);

  // 10. GPC request: the server sends no pixel.
  const gpcPage = await browser.newPage({ extraHTTPHeaders: { 'Sec-GPC': '1' } });
  await gpcPage.context().addCookies(await page.context().cookies());
  await gpcPage.goto(base + '/');
  assert.equal(await gpcPage.locator('#autumn-meta-pixel-config').count(), 0);

  assert.deepEqual(violations, []);
  console.log('e2e: ok');
} finally {
  if (browser) await browser.close();
  app.kill();
}
