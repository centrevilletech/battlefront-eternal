// Real Electron preload, network utility process and native UDP with owned
// local content. Run under xvfb-run on Linux; start bfeserver first, and
// either Vite (BFE_TEST_URL) or build the desktop app and set
// BFE_TEST_CONTENT_DIR to a prepared content folder.
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
const { _electron: electron } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const address = process.env.BFE_TEST_ADDRESS || '127.0.0.1';
const port = Number(process.env.BFE_TEST_PORT || 18451);
const info = await (await fetch(`http://${address}:${port}/info`)).json();
const directory = await mkdtemp(join(tmpdir(), 'bfe-electron-match-'));
const query = new URLSearchParams(info.query);
for (const [key, value] of Object.entries({ network: '1', worker: '1', server: info.id, address, port, shadows: 'off' })) query.set(key, String(value));
let app;
try {
  const source = process.env.BFE_TEST_CONTENT_DIR && !process.env.BFE_TEST_URL ? `--content-dir=${resolve(process.env.BFE_TEST_CONTENT_DIR)}` : `--dev-url=${process.env.BFE_TEST_URL || 'http://127.0.0.1:18449'}`;
  app = await electron.launch({ executablePath: resolve('node_modules/electron/dist/electron'), args: [resolve('.'), '--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', `--user-data-dir=${directory}`, source, `--query=?${query}`], timeout: 60000 });
  const page = await app.firstWindow();
  await page.setViewportSize({ width: 640, height: 480 });
  const errors = [];
  page.on('pageerror', error => { errors.push(error.message); console.error(error.message); });
  page.on('console', message => { if (message.type() === 'error' && /frame failed|world worker failed/.test(message.text())) { errors.push(message.text()); console.error(message.text()); } });
  // The page's game traffic must run through the network utility process.
  const network = () => app.evaluate(({ app }) => app.getAppMetrics().filter(metric => metric.type === 'Utility').map(metric => metric.serviceName || metric.name));
  await page.waitForFunction(() => window.__bf?.client?.ready, null, { timeout: 180000 });
  const security = await page.evaluate(async () => ({ node: typeof window.require, identity: await bfeDesktop.network.request('identity'), consent: await bfeDesktop.network.request('notice') }));
  assert.equal(security.node, 'undefined');
  assert.equal(security.consent.accepted, false, 'LAN needs no online notice consent');
  assert.equal('privateKey' in security.identity, false);
  await page.evaluate(() => __bf.begin());
  await page.waitForFunction(() => __bf.worldClock.tick > 0 && __bf.playerUnit.id > 1, null, { timeout: 30000 });
  const before = await page.evaluate(() => ({ tick: __bf.worldClock.tick, id: __bf.playerUnit.id }));
  await page.waitForFunction(tick => __bf.worldClock.tick > tick + 60, before.tick, { timeout: 30000 });
  const after = await page.evaluate(() => ({ tick: __bf.worldClock.tick, id: __bf.playerUnit.id, status: __bf.client.host.statusText, path: __bf.client.host.transport.path }));
  assert.equal(after.id, before.id);
  assert.match(after.status, /^(Spectating \(content differs\) · )?(Direct|Relay) IPv[46] · \d+ ms$/);
  assert.ok(['direct', 'relay'].includes(after.path?.kind), 'a native UDP path is reported');
  assert.ok((await network()).length >= 1, 'Electron runs utility processes, including the network process');
  assert.deepEqual(errors, []);
  console.log('Real Electron multiplayer passed', JSON.stringify(after));
} finally {
  await app?.close();
  await rm(directory, { recursive: true, force: true });
}
