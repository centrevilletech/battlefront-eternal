// Electron startup wiring with real discovery/import/source verification and a
// controlled preparer. Run in an isolated checkout without public game data.
//
//   PLAYWRIGHT_MODULE=<playwright>/index.mjs BFE_ELECTRON=<electron binary> \
//     xvfb-run -a node tests/desktop-startup-electron.mjs
//
// The preparer and its check (tools/prepare.py --check) are stood in for, so
// no retail data is needed: finishRepair(success, extra) ends a run as the
// tool would, writing .prepare.json, and checkResult is what a check finds.
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
    app.setPath('home',process.env.TEST_HOME); app.setPath('userData',process.env.TEST_DATA);
    (async()=>{
      const {Preparer,CONTENT_VERSION}=await import(${JSON.stringify(pathToFileURL(path.join(root, 'electron/prepare.js')).href)});
      const fs=require('node:fs/promises'),path=require('node:path');
      global.checks=[];
      Preparer.prototype.check=async function(options){
        global.checks.push(options.game);
        return global.checkResult||{event:'check',ok:true,problems:[],addOns:[{name:'tat3',folder:'Tat3',maps:[{id:'tat3',name:'TATOOINE: JABBA',problems:[]}],problems:[]}],counts:{maps:17,sides:11,sublevels:30},workers:4};
      };
      Preparer.prototype.start=async function(options){
        this.child={}; this.last=options; global.repairOptions=options;
        this.snapshot={...this.snapshot,status:'running',enhancedSound:options.enhancedSound!==false,steps:[],failed:[],addOnFailures:[]};
        const state=JSON.parse(await fs.readFile(path.join(options.content,'.prepare.json'),'utf8').catch(()=>'{}'));
        global.repairState=state;
        global.finishRepair=async (success,extra={})=>{
          await fs.mkdir(path.join(options.content,'common'),{recursive:true});
          await fs.writeFile(path.join(options.content,'common/common.json'),'{}');
          const failures=success?[]:[{key:'map:nab2',label:'Naboo: Theed',reason:'no archive for nab2: expected GameData/Data/_LVL_PC/NAB/nab2.lvl'}];
          await fs.writeFile(path.join(options.content,'.prepare.json'),JSON.stringify({...state,version:CONTENT_VERSION.state,mapImportVersion:CONTENT_VERSION.maps,muzzleVersion:CONTENT_VERSION.sides,commonVersion:CONTENT_VERSION.common,game:path.join(options.game,'GameData/Data/_LVL_PC'),addOns:[],complete:true,failed:failures.map(f=>f.key),failures,skipped:[],extras:{enhancedSound:options.enhancedSound!==false,soundSteps:options.enhancedSound===false?30:0},addOnFailures:[],...extra}));
          this.child=null; this.emit({event:'finished',failed:failures.map(f=>f.key),failures,addOnFailures:extra.addOnFailures||[]});
        };
      };
      Preparer.prototype.stop=function(){this.child=null;};
      await import(${JSON.stringify(pathToFileURL(path.join(root, 'electron/main.js')).href)});
    })();
  `);
  const launch = ({ testHome = home, testData = data } = {}) => _electron.launch({ env: { ...process.env, HOME: testHome, USERPROFILE: testHome, TEST_HOME: testHome, TEST_DATA: testData, BFE_CONTENT_DIR: '', BFE_DEV_URL: '' }, executablePath: process.env.BFE_ELECTRON || path.join(root, 'node_modules/electron/dist/electron'), args: [bootstrap, '--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'], timeout: 60000 });
  const loaderState = (page) => page.evaluate(() => window.bfeDesktop.prepare.state());
  // Polls the loader's state until mode (and test(state)) hold. Not
  // page.waitForFunction: a predicate's promise counts as truthy at once.
  const waitMode = async (page, mode, test = () => true, timeout = 30000) => {
    const deadline = Date.now() + timeout;
    for (;;) {
      const state = await loaderState(page).catch(() => null);
      if (state?.mode === mode && test(state)) return state;
      if (Date.now() > deadline) throw Error(`timed out waiting for ${mode}: ${JSON.stringify(state)?.slice(0, 400)}`);
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  };

  // A copy the player chose before (gameDir) repairs an old import without
  // asking, its check passing first.
  app = await launch(); let page = await app.firstWindow();
  await waitMode(page, 'preparing');
  assert.deepEqual(await app.evaluate(() => global.checks), [game]);
  assert.equal((await app.evaluate(() => global.repairOptions)).content, imported.path);
  assert.deepEqual((await app.evaluate(() => global.repairState)).done, oldState.done);
  assert.equal((await app.evaluate(() => global.repairState)).game, path.join(game, 'GameData/Data/_LVL_PC'));
  await page.evaluate(() => window.bfeDesktop.prepare.action('play'));
  assert.ok(page.url().includes('__desktop'));
  // A failed map of the game's own still blocks, and says what failed and what to do.
  await app.evaluate(() => global.finishRepair(false));
  await page.getByText('SOMETHING WENT WRONG', { exact: true }).waitFor();
  assert.equal(await page.getByRole('button', { name: 'PLAY', exact: true }).count(), 0);
  const message = (await loaderState(page)).message;
  assert.match(message, /Naboo: Theed \(map:nab2\): no archive for nab2/);
  assert.match(message, /Verify integrity of game files/);
  await app.close(); app = await launch(); page = await app.firstWindow();
  await waitMode(page, 'preparing');
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
  await waitMode(page, 'preparing');
  assert.notEqual((await app.evaluate(() => global.repairOptions)).content, imported.path);
  assert.deepEqual(await app.evaluate(() => global.repairState), {});
  assert.equal(JSON.parse(await fs.readFile(path.join(imported.path, '.prepare.json'))).mapImportVersion, oldState.mapImportVersion);
  await app.close();

  // A first run: Steam's copy found (a copy placed by hand beside it too),
  // and nothing starts before the player says so.
  const freshHome = path.join(temp, 'fresh-home'), freshData = path.join(temp, 'fresh-data');
  const library = path.join(freshHome, '.local/share/Steam');
  const steamGame = path.join(library, 'steamapps/common/Star Wars Battlefront (Classic 2004)');
  const stray = path.join(library, 'steamapps/common/SWBF Copy');
  await fs.mkdir(freshData, { recursive: true });
  await fs.cp(oldGame, steamGame, { recursive: true });
  await fs.cp(oldGame, stray, { recursive: true });
  await fs.writeFile(path.join(library, 'steamapps/appmanifest_1058020.acf'), '"AppState" { "installdir" "Star Wars Battlefront (Classic 2004)" }');
  // The app's folder holds what the copy placed by hand left (a modded map).
  const managed = path.join(freshData, 'game-data');
  await fs.mkdir(path.join(managed, 'maps/oldmod'), { recursive: true });
  await fs.mkdir(path.join(managed, 'common'), { recursive: true });
  await fs.writeFile(path.join(managed, 'maps/oldmod/map.json'), '{}');
  await fs.writeFile(path.join(managed, 'common/common.json'), '{}');
  await fs.writeFile(path.join(managed, '.prepare.json'), JSON.stringify({ ...oldState, mapImportVersion: CONTENT_VERSION.maps, game: path.join(stray, 'GameData/Data/_LVL_PC'), addOns: [] }));
  app = await launch({ testHome: freshHome, testData: freshData }); page = await app.firstWindow();
  await waitMode(page, 'confirm', (s) => s.check?.ok);
  await page.getByText('STAR WARS BATTLEFRONT FOUND', { exact: true }).waitFor();
  let view = await loaderState(page);
  assert.equal(view.install, steamGame);
  assert.deepEqual(view.copy, { dir: steamGame, source: 'steam', how: 'your Steam library' });
  assert.deepEqual(view.copies.map((copy) => [copy.dir, copy.source]), [[steamGame, 'steam'], [stray, 'folder']]);
  assert.equal(view.enhancedSound, true);
  assert.ok(view.time.total > view.time.sound && view.time.sound > 0);
  await page.getByText('TATOOINE: JABBA (Tat3)', { exact: true }).waitFor();
  assert.equal(await app.evaluate(() => !!global.repairOptions), false, 'nothing is prepared before PREPARE');
  // A check that fails offers no PREPARE; CHECK AGAIN after a repair does.
  await app.evaluate(() => { global.checkResult = { event: 'check', ok: false, problems: [{ code: 'missing', path: 'GameData/Data/_LVL_PC/SIDE/rep.lvl', message: 'GameData/Data/_LVL_PC/SIDE/rep.lvl (an army) is missing.' }], addOns: [] }; });
  await page.evaluate(() => window.bfeDesktop.prepare.action('recheck'));
  await page.getByText("THIS COPY CAN'T BE PREPARED", { exact: true }).waitFor();
  await page.getByText('GameData/Data/_LVL_PC/SIDE/rep.lvl (an army) is missing.', { exact: true }).waitFor();
  assert.equal(await page.getByRole('button', { name: 'PREPARE', exact: true }).count(), 0);
  await app.evaluate(() => { global.checkResult = null; });
  await page.getByRole('button', { name: 'CHECK AGAIN', exact: true }).click();
  await page.getByRole('button', { name: 'PREPARE', exact: true }).waitFor();
  // Another copy can be picked (it is checked in turn), and picked back.
  await page.getByRole('button', { name: 'USE THIS COPY', exact: true }).click();
  await waitMode(page, 'confirm', (s) => s.install === stray && s.check);
  assert.ok((await app.evaluate(() => global.checks)).includes(stray));
  await page.getByRole('button', { name: 'USE THIS COPY', exact: true }).click();
  await waitMode(page, 'confirm', (s) => s.install === steamGame && s.check?.ok);
  // Enhanced sound off, then PREPARE: the choices are kept.
  await page.getByRole('checkbox').uncheck();
  await waitMode(page, 'confirm', (s) => s.enhancedSound === false);
  await page.getByRole('button', { name: 'PREPARE', exact: true }).click();
  await waitMode(page, 'preparing');
  const options = await app.evaluate(() => global.repairOptions);
  assert.equal(options.game, steamGame);
  assert.equal(options.content, managed);
  assert.equal(options.enhancedSound, false);
  // Another copy than the folder was prepared from: what it left is gone.
  assert.deepEqual(await app.evaluate(() => global.repairState), {});
  await assert.rejects(fs.access(path.join(managed, 'maps/oldmod')));
  const saved = JSON.parse(await fs.readFile(path.join(freshData, 'desktop-settings.json'), 'utf8'));
  assert.equal(saved.gameDir, steamGame);
  assert.equal(saved.enhancedSound, false);
  // An add-on map left out doesn't stop the game: ready, saying which and
  // why, and waiting for PLAY.
  const left = [{ addOn: 'Mod1', map: 'mod1', name: 'MOD ONE', part: 'map', step: 'side:mod', reason: 'no archive for side mod' }];
  await app.evaluate((_electron, failures) => global.finishRepair(true, { addOnFailures: failures }), left);
  await page.getByText('ADD-ON MAPS LEFT OUT', { exact: true }).waitFor();
  await page.getByText('MOD ONE (Mod1): left out. no archive for side mod', { exact: true }).waitFor();
  await page.waitForTimeout(3500);
  assert.ok(page.url().includes('__desktop'), 'it waits for PLAY');
  await page.getByRole('button', { name: 'PLAY', exact: true }).click();
  await page.waitForURL(url => !url.pathname.includes('__desktop'));
  await app.close();

  // Next launch: straight to the menu. The loader's YOUR GAME CONTENT adds
  // the Enhanced sound from the same copy, without asking.
  app = await launch({ testHome: freshHome, testData: freshData }); page = await app.firstWindow();
  await page.waitForURL(url => !url.pathname.includes('__desktop'));
  assert.equal(await app.evaluate(() => !!global.repairOptions), false);
  await page.goto('app://bfe/__desktop/prepare.html');
  await page.getByText('YOUR GAME CONTENT', { exact: true }).waitFor();
  assert.equal((await loaderState(page)).prepared.addOnFailures.length, 1);
  await page.getByText('MOD ONE (Mod1): left out. no archive for side mod', { exact: true }).waitFor();
  await page.getByRole('button', { name: 'ADD ENHANCED SOUND', exact: true }).click();
  await waitMode(page, 'preparing');
  const sound = await app.evaluate(() => global.repairOptions);
  assert.deepEqual([sound.game, sound.content, sound.enhancedSound], [steamGame, managed, true]);
  assert.equal((await app.evaluate(() => global.repairState)).game, path.join(steamGame, 'GameData/Data/_LVL_PC'), 'the same copy: nothing cleared');
  console.log('PASS: automatic imported repair, source verification, menu gate, failure/restart recovery, current restart, changed-source isolation, confirm before a first preparation (copies, check, sound choice), clearing what another copy left, add-on maps left out, and adding the Enhanced sound later');
} finally { await app?.close(); await fs.rm(temp, { recursive: true, force: true }); }
