// Native-dialog/IPC integration without a Steam installation. Run under Xvfb
// on Linux, with PLAYWRIGHT_MODULE and BFE_ELECTRON pointing at local tools.
import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { CONTENT_VERSION } from '../electron/prepare.js';
import { exportContent, importContent } from '../electron/content-archive.js';
const { _electron } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root = fileURLToPath(new URL('../', import.meta.url));
const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'bfe-native-transfer-'));
let app;
try {
  const home = path.join(temp, 'empty-home'), data = path.join(temp, 'user-data'), content = path.join(temp, 'fixture');
  await fs.mkdir(home); await fs.mkdir(data);
  const prepared = JSON.stringify({ version: CONTENT_VERSION.state, mapImportVersion: CONTENT_VERSION.maps, muzzleVersion: CONTENT_VERSION.sides, commonVersion: CONTENT_VERSION.common, done: ['map:nab2', 'textures'], complete: true, failed: [], skipped: [] });
  for (const [name, text] of Object.entries({ 'common/common.json': '{}', 'maps/nab2/map.json': '{"sha256":"preserve-me"}', '.prepare.json': prepared })) {
    await fs.mkdir(path.dirname(path.join(content, name)), { recursive: true }); await fs.writeFile(path.join(content, name), text);
  }
  const archive = path.join(temp, 'fixture.bfepack'), output = path.join(temp, 'exported.bfepack');
  await exportContent(content, archive);
  const bootstrap = path.join(temp, 'bootstrap.cjs');
  await fs.writeFile(bootstrap, `const {app}=require('electron'); app.setPath('home',${JSON.stringify(home)}); app.setPath('userData',${JSON.stringify(data)}); import(${JSON.stringify(pathToFileURL(path.join(root, 'electron/main.js')).href)});`);
  const launch = () => _electron.launch({ env: { ...process.env, HOME: home, USERPROFILE: home, BFE_CONTENT_DIR: '', BFE_DEV_URL: '' }, executablePath: process.env.BFE_ELECTRON || path.join(root, 'node_modules/electron/dist/electron'), args: [bootstrap, '--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'], timeout: 60000 });
  app = await launch();
  let page = await app.firstWindow();
  await page.getByRole('button', { name: 'IMPORT CONTENT EXPORT' }).waitFor();
  assert.equal(await page.locator('#title').textContent(), 'STAR WARS BATTLEFRONT NOT FOUND');
  await app.evaluate(({ dialog }, file) => { dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [file] }); }, archive);
  await page.getByRole('button', { name: 'IMPORT CONTENT EXPORT' }).dispatchEvent('click');
  await page.getByText('READY FOR BATTLE', { exact: true }).waitFor();
  const installed = await page.evaluate(() => window.bfeDesktop.contentDir());
  assert.equal(await fs.readFile(path.join(installed, 'maps/nab2/map.json'), 'utf8'), '{"sha256":"preserve-me"}');
  assert.equal(JSON.parse(await fs.readFile(path.join(data, 'desktop-settings.json'))).chosenContentDir, installed);
  await app.evaluate(({ dialog }, file) => { dialog.showSaveDialog = async () => ({ canceled: false, filePath: file }); }, output);
  await page.evaluate(() => { void window.bfeDesktop.contentTransfer('export'); });
  await page.getByText('CONTENT EXPORTED', { exact: true }).waitFor();
  const roundtrip = await importContent(output, path.join(temp, 'roundtrip'));
  assert.equal(await fs.readFile(path.join(roundtrip.path, '.prepare.json'), 'utf8'), prepared);
  assert.equal(await page.evaluate(() => window.bfeDesktop.contentTransfer('delete')), false);
  const bad = path.join(temp, 'broken.bfepack'); await fs.writeFile(bad, 'not a ZIP');
  await app.evaluate(({ dialog }, file) => { dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [file] }); }, bad);
  await page.evaluate(() => { void window.bfeDesktop.contentTransfer('import'); });
  await page.getByText('CONTENT TRANSFER STOPPED', { exact: true }).waitFor();
  assert.equal(await page.evaluate(() => window.bfeDesktop.contentDir()), installed);
  await app.close(); app = await launch(); page = await app.firstWindow();
  await page.waitForURL(url => url.protocol === 'app:' && !url.pathname.includes('__desktop'));
  assert.equal(await page.evaluate(() => window.bfeDesktop.contentDir()), installed);
  await app.close();
  await fs.writeFile(path.join(installed, '.prepare.json'), JSON.stringify({ ...JSON.parse(prepared), mapImportVersion: CONTENT_VERSION.maps - 1 }));
  app = await launch(); page = await app.firstWindow();
  await page.getByText('GAME DATA NEEDS AN UPDATE', { exact: true }).waitFor();
  assert.equal(await page.getByRole('button', { name: 'PLAY', exact: true }).count(), 0);
  await page.evaluate(() => window.bfeDesktop.prepare.action('play'));
  await page.getByText('GAME DATA NEEDS AN UPDATE', { exact: true }).waitFor();
  console.log('PASS: first-run import, native export, corrupt-file rollback, restart without Steam, and stale import blocked without Steam');
} finally { await app?.close(); await fs.rm(temp, { recursive: true, force: true }); }
