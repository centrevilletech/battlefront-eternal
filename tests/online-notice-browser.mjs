// Run with PLAYWRIGHT_MODULE pointing to a Playwright installation.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const source = await readFile(new URL('../src/shell/online-notice.js', import.meta.url));
const server = createServer((req, res) => {
  res.setHeader('content-type', req.url === '/notice.js' ? 'text/javascript' : 'text/html');
  res.end(req.url === '/notice.js' ? source : '<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"></head><body style="background:#07182d"><button id="open">Online play</button></body></html>');
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const browser = await chromium.launch({ headless: true, executablePath: process.env.BFE_CHROMIUM });
try {
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  await page.evaluate(async () => {
    const { onlineNotice } = await import('/notice.js');
    window.calls = 0; window.fail = false; window.accepted = false;
    const bridge = { request: async () => {
      window.calls++; await new Promise(resolve => { window.save = resolve; });
      if (window.fail) throw Error('Disk is full. Try again.');
      window.accepted = true; return true;
    } };
    window.show = () => { window.result = undefined; const notice = { accepted: window.accepted, version: '2026-10-11', operator: 'Josh Lambert', master: 'https://online.battlefronteternal.com' }; const result = onlineNotice(bridge, notice); result.then(value => window.result = value); return result; };
    document.querySelector('#open').onclick = () => { void window.show(); };
    window.backgroundKeys = 0; window.addEventListener('keydown', () => window.backgroundKeys++);
  });
  for (const [width, height] of [[320,568], [800,600], [1440,900]]) {
    await page.setViewportSize({ width, height });
    await page.locator('#open').click();
    const dialog = page.getByRole('dialog'); await dialog.waitFor();
    assert.equal(await page.evaluate(() => { const d = document.querySelector('dialog'); return d.scrollWidth <= d.clientWidth; }), true);
    await page.getByRole('button', { name: 'Accept and continue' }).scrollIntoViewIfNeeded();
    const layout = await page.evaluate(() => {
      const d = document.querySelector('dialog'), footer = d.querySelector('footer'), paragraph = footer.previousElementSibling.previousElementSibling.previousElementSibling;
      const bounds = d.getBoundingClientRect(); return { top: bounds.top, bottom: bounds.bottom, separated: footer.getBoundingClientRect().top >= paragraph.getBoundingClientRect().bottom };
    });
    assert.ok(layout.top >= 0 && layout.bottom <= height && layout.separated);
    await page.screenshot({ path: `/tmp/bfe-online-notice-${width}.png` });
    await page.getByRole('button', { name: 'Not now' }).click();
    assert.equal(await page.evaluate(() => window.result), false);
  }
  await page.locator('#open').click();
  await page.evaluate(() => { void window.show(); });
  assert.equal(await page.locator('dialog').count(), 1);
  await page.evaluate(() => window.fail = true);
  await page.getByRole('button', { name: 'Accept and continue' }).click();
  assert.equal(await page.getByRole('button', { name: 'Accept and continue' }).isDisabled(), true);
  await page.keyboard.press('Enter'); await page.keyboard.press('Escape');
  assert.equal(await page.locator('dialog').count(), 1);
  assert.equal(await page.evaluate(() => window.calls), 1);
  await page.evaluate(() => window.save());
  await page.getByRole('alert').filter({ hasText: 'Disk is full' }).waitFor();
  await page.evaluate(() => window.fail = false);
  await page.keyboard.press('Enter');
  assert.equal(await page.evaluate(() => window.calls), 2);
  await page.evaluate(() => window.save());
  await page.waitForFunction(() => window.result === true);
  assert.equal(await page.locator('dialog').count(), 0);
  assert.equal(await page.evaluate(() => window.backgroundKeys), 0);
  assert.equal(await page.locator('#open').evaluate(el => document.activeElement === el), true);
  await page.locator('#open').click();
  assert.equal(await page.locator('dialog').count(), 0);
  assert.equal(await page.evaluate(() => window.calls), 2);
  console.log('PASS: responsive notice, isolated keyboard input, cancel, single-flight save, failure/retry, saved consent');
} finally { await browser.close(); server.close(); }
