// Electron startup wiring with real discovery/import/source verification and a
// controlled preparer. Run in an isolated checkout without public game data.
import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { CONTENT_VERSION } from '../electron/prepare.js';
import { exportContent, importContent } from '../electron/content-archive.js';
const { _electron } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root = fileURLToPath(new URL('../', import.meta.url));
const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'bfe-startup-'));
let app;
try {
  const home = path.join(temp, 'home'), data = path.join(temp, 'user-data'), source = path.join(temp, 'source');
  const oldGame = path.join(temp, 'old-game'), game = path.join(temp, 'local-game');
  const lvl = path.join(oldGame, 'GameData/Data/_LVL_PC');
  await fs.mkdir(path.join(lvl, 'SIDE'), { recursive: true });
  await fs.mkdir(path.join(lvl, 'Common/ENG'), { recursive: true });
  await fs.writeFile(path.join(lvl, 'Common/ENG/common.lvl'), 'owned retail fixture');
  await fs.cp(oldGame, game, { recursive: true });
  await fs.mkdir(home); await fs.mkdir(data); await fs.mkdir(path.join(source, 'common'), { recursive: true });
  await fs.writeFile(path.join(source, 'common/common.json'), '{}');
  await fs.mkdir(path.join(source, 'maps/nab2'), { recursive: true });
  await fs.writeFile(path.join(source, 'maps/nab2/map.json'), '{}');
  const oldState = { version: CONTENT_VERSION.state, mapImportVersion: CONTENT_VERSION.maps - 1, muzzleVersion: CONTENT_VERSION.sides, commonVersion: CONTENT_VERSION.common, game: lvl, done: ['map:nab2', 'audio'], complete: true, failed: [], skipped: [] };
  await fs.writeFile(path.join(source, '.prepare.json'), JSON.stringify(oldState));
  const archive = path.join(temp, 'old.bfepack');
  await exportContent(source, archive);
  const imported = await importContent(archive, path.join(data, 'imports'));
  const settings = path.join(data, 'desktop-settings.json');
  const setSettings = () => fs.writeFile(settings, JSON.stringify({ gameDir: game, chosenContentDir: imported.path }));
  await setSettings();
  const bootstrap = path.join(temp, 'bootstrap.cjs');
  // Hold preparation until the test signals completion. This tests that menu
  // actions cannot expose old content while the preparer owns it.
  await fs.writeFile(bootstrap, `
    const {app}=require('electron');
    app.setPath('home',${JSON.stringify(home)}); app.setPath('userData',${JSON.stringify(data)});
    (async()=>{
      const {Preparer,CONTENT_VERSION}=await import(${JSON.stringify(pathToFileURL(path.join(root, 'electron/prepare.js')).href)});
      const fs=require('node:fs/promises'),path=require('node:path');
      Preparer.prototype.start=async function(options){
        this.child={}; this.last=options; global.repairOptions=options;
        const state=JSON.parse(await fs.readFile(path.join(options.content,'.prepare.json'),'utf8').catch(()=>'{}'));
        global.repairState=state;
        global.finishRepair=async success=>{
          await fs.mkdir(path.join(options.content,'common'),{recursive:true});
          await fs.writeFile(path.join(options.content,'common/common.json'),'{}');
          await fs.writeFile(path.join(options.content,'.prepare.json'),JSON.stringify({...state,version:CONTENT_VERSION.state,mapImportVersion:CONTENT_VERSION.maps,muzzleVersion:CONTENT_VERSION.sides,commonVersion:CONTENT_VERSION.common,game:path.join(options.game,'GameData/Data/_LVL_PC'),addOns:[],complete:true,failed:success?[]:['map:nab2'],skipped:[]}));
          this.child=null; this.emit({event:'finished',failed:success?[]:['map:nab2']});
        };
      };
      Preparer.prototype.stop=function(){this.child=null;};
      await import(${JSON.stringify(pathToFileURL(path.join(root, 'electron/main.js')).href)});
    })();
  `);
  const launch = () => _electron.launch({ env: { ...process.env, HOME: home, USERPROFILE: home, BFE_CONTENT_DIR: '', BFE_DEV_URL: '' }, executablePath: process.env.BFE_ELECTRON || path.join(root, 'node_modules/electron/dist/electron'), args: [bootstrap, '--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'], timeout: 60000 });
  app = await launch(); let page = await app.firstWindow();
  await page.waitForFunction(() => window.bfeDesktop.prepare.state().then(s => s.mode === 'preparing'));
  assert.equal((await app.evaluate(() => global.repairOptions)).content, imported.path);
  assert.deepEqual((await app.evaluate(() => global.repairState)).done, oldState.done);
  assert.equal((await app.evaluate(() => global.repairState)).game, path.join(game, 'GameData/Data/_LVL_PC'));
  await page.evaluate(() => window.bfeDesktop.prepare.action('play'));
  assert.ok(page.url().includes('__desktop'));
  await app.evaluate(() => global.finishRepair(false));
  await page.getByText('SOMETHING WENT WRONG', { exact: true }).waitFor();
  assert.equal(await page.getByRole('button', { name: 'PLAY', exact: true }).count(), 0);
  await app.close(); app = await launch(); page = await app.firstWindow();
  await page.waitForFunction(() => window.bfeDesktop.prepare.state().then(s => s.mode === 'preparing'));
  assert.equal((await app.evaluate(() => global.repairOptions)).content, imported.path);
  await app.evaluate(() => global.finishRepair(true));
  await page.waitForURL(url => !url.pathname.includes('__desktop'));
  await app.close(); app = await launch(); page = await app.firstWindow();
  await page.waitForURL(url => !url.pathname.includes('__desktop'));
  assert.equal(await app.evaluate(() => !!global.repairOptions), false);
  // A different source cannot reuse an imported checkpoint, even when its
  // recorded source path happens to name this installation.
  await app.close();
  await fs.writeFile(path.join(imported.path, '.prepare.json'), JSON.stringify({ ...oldState, game: path.join(game, 'GameData/Data/_LVL_PC') }));
  await fs.writeFile(path.join(game, 'GameData/Data/_LVL_PC/Common/ENG/common.lvl'), 'different archives');
  await setSettings(); app = await launch(); page = await app.firstWindow();
  await page.waitForFunction(() => window.bfeDesktop.prepare.state().then(s => s.mode === 'preparing'));
  assert.notEqual((await app.evaluate(() => global.repairOptions)).content, imported.path);
  assert.deepEqual(await app.evaluate(() => global.repairState), {});
  assert.equal(JSON.parse(await fs.readFile(path.join(imported.path, '.prepare.json'))).mapImportVersion, oldState.mapImportVersion);
  console.log('PASS: automatic imported repair, source verification, menu gate, failure/restart recovery, current restart, and changed-source isolation');
} finally { await app?.close(); await fs.rm(temp, { recursive: true, force: true }); }
