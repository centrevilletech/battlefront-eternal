// Tab's scoreboard in a network game: a real bfeserver (its 32 a side
// filled with Classic AI bots) and two rendered Chromium clients joined
// over the native UDP transport (tools/net-page-bridge.mjs), one on each
// team. It checks, with real key events:
// - Tab held shows the table (not a players-only list): every Character of
//   both teams, the bots by name with their kills and deaths, both players
//   as humans with their pings; it closes on the release;
// - the numbers are the server's: a death on one client shows on the
//   other's table;
// - DETAILS from Tab's table: a bot's record arrives during play, and the
//   other player's ends with his CONNECTION;
// - chat and Tab keep apart: Tab in the chat entry keeps the focus there
//   and opens nothing; T does not open chat over the table;
// and writes screenshots of both clients' tables and the DETAILS pages to
// SCOREBOARD_SHOTS (default /tmp/bf-punch/scoreboard/network).
//
//   BFE_SERVER_REPO=<bfeserver checkout> BFE_DATA_DIR=<prepared content> \
//   PLAYWRIGHT_MODULE=.../playwright/index.mjs node tests/scoreboard-network-browser.mjs
//   [--map=nab2] [--port=28651] [--health-port=28652] [--vite-port=18507]
//   SCOREBOARD_TEST_URL=http://127.0.0.1:<port> uses a running Vite.
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { createNetworkBridge } from '../electron/network.js';
import { startPageNetwork } from '../tools/net-page-bridge.mjs';

const arg = (name, fallback) => { const found = process.argv.find(value => value.startsWith(`--${name}=`)); return found === undefined ? fallback : found.slice(name.length + 3); };
const engine = resolve('.'), data = resolve(process.env.BFE_DATA_DIR || 'public'), serverRepo = resolve(process.env.BFE_SERVER_REPO || '../battlefront-eternal-server');
const gamePort = Number(arg('port', 28651)), healthPort = Number(arg('health-port', 28652)), vitePort = Number(arg('vite-port', 18507));
const map = arg('map', 'nab2'), side = arg('side', 'rep');
const OUT = process.env.SCOREBOARD_SHOTS || '/tmp/bf-punch/scoreboard/network';
const [width, height] = (process.env.SCOREBOARD_SIZE || '1280x720').split('x').map(Number);
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const sleep = ms => new Promise(done => setTimeout(done, ms));
const say = message => console.log(`[scoreboard-network] ${message}`);
await mkdir(OUT, { recursive: true });
const directory = await mkdtemp(join(tmpdir(), 'bfe-scoreboard-net-'));
const failures = [], errors = [], logs = [];
let child, vite, browser, pageNetwork;

async function check(name, fn) {
  try { await fn(); console.log(`ok - ${name}`); }
  catch (error) { failures.push(`${name}: ${error.message}`); console.log(`not ok - ${name}: ${error.message}`); }
}

// The board as the page has it.
const board = page => page.evaluate(() => {
  const b = __bf.scoreboard, m = b.model;
  const row = r => ({ slot: r.slot, name: r.name, kills: r.kills, deaths: r.deaths, points: r.points, ping: r.ping, human: r.human, local: r.local });
  return { state: m.state, page: m.page, layer: __bf.hud.layer === b.view.layer, controls: __bf.controls.enabled, ping: b.view.pingColumn,
    lists: { 1: m.lists[1].map(row), 2: m.lists[2].map(row) }, roster: [1, 2].map(t => __bf.battle.characters.filter(c => c.team === t).length) };
});
const rows = state => [...state.lists[1], ...state.lists[2]];
// The board's clock takes at most a tenth of a second a frame, and
// SwiftShader frames come slowly: the intros want a few real seconds.
const SETTLE = 4000;
// Tab tapped: the table pinned, its intros played out.
async function pin(page) {
  await page.keyboard.down('Tab'); await sleep(80); await page.keyboard.up('Tab');
  await page.waitForFunction(() => __bf.scoreboard.state === 'peek', null, { timeout: 5000 });
  await sleep(SETTLE);
}
async function unpin(page) {
  if (await page.evaluate(() => __bf.scoreboard.state) === 'closed') return;
  await page.keyboard.down('Tab');
  const began = await page.evaluate(() => __bf.scoreboard.input.time);
  await page.waitForFunction(t => __bf.scoreboard.input.time > t + .6, began);
  await page.keyboard.up('Tab');
  await page.waitForFunction(() => __bf.scoreboard.state === 'closed', null, { timeout: 5000 });
}
// What a client shows, when a wait runs out.
const diagnose = (page) => page.evaluate(() => ({ tick: __bf.worldClock.tick, team: __bf.playerTeam, alive: __bf.playerUnit.alive, generation: __bf.playerUnit.generation,
  hidden: document.hidden, running: __bf.client.running, queued: __bf.controls.human?.length, pendingActions: __bf.client.pendingActions?.length, ackActions: __bf.client.ackActions,
  id: __bf.playerUnit.id, applied: (({ playerId, generation, phase } = {}) => ({ playerId, generation, phase }))(__bf.client.entity?.eligibility),
  pending: (({ playerId, generation, phase } = {}) => ({ playerId, generation, phase }))(__bf.client.entity?.pendingEligibility?.summary),
  spawn: __bf.spawnPort.humanState(), screen: __bf.spawnScreen?.isOpen ?? null, menu: __bf.menu.isOpen, status: __bf.client.host.statusText, closed: __bf.client.host.closed,
  eligibility: __bf.client.entity?.eligibility ? { posts: __bf.client.entity.eligibility.eligiblePosts?.length, classes: __bf.client.entity.eligibility.eligibleClasses?.length } : null })).catch((error) => ({ error: error.message }));
// Tab opens nothing over the spawn screen (it picks posts there): a
// player the bots killed spawns again first. A choice made just as the
// server re-attaches the session (a new life number) goes with that life's
// input, so the player chooses again until he is in.
async function alive(page) {
  for (let attempt = 0; attempt < 8; attempt++) {
    if (await page.evaluate(() => __bf.playerUnit.alive && __bf.spawnPort.humanState().phase === 'alive')) return;
    await page.waitForFunction(() => __bf.client.entity?.eligibility && ['unspawned', 'awaiting'].includes(__bf.spawnPort.humanState().phase), null, { timeout: 60000 });
    await page.evaluate(deka => { const roster = __bf.spawnPort.roster(__bf.playerTeam); __bf.spawnPort.request({ className: (deka && roster.find(r => r.className === 'cis_inf_droideka') || roster[0]).className, post: null }); }, Boolean(process.env.DROIDEKA_DEATH_NETWORK));
    try { await page.waitForFunction(() => __bf.playerUnit.alive && __bf.spawnPort.humanState().phase === 'alive', null, { timeout: 15000 }); return; } catch {}
  }
  throw Error(`never spawned: ${JSON.stringify(await diagnose(page))}`);
}
// DETAILS of a row: selected and opened as a double click would.
async function details(page, slot) {
  await page.evaluate(slot => {
    const m = __bf.scoreboard.model;
    for (const side of [1, 2]) { const index = m.lists[side].findIndex(r => r.slot === slot); if (index >= 0) { m.select(side, index); break; } }
    m.showDetails();
  }, slot);
  await sleep(SETTLE);
  return page.evaluate(() => ({ page: __bf.scoreboard.model.page, name: __bf.scoreboard.model.subject?.name, rows: __bf.scoreboard.view.pages.details.content.map(r => [r.label, r.value]) }));
}
const shot = (page, name) => page.screenshot({ path: `${OUT}/${name}-${width}x${height}.png` });

try {
  await writeFile(join(directory, 'config.json'), JSON.stringify({ name: 'Scoreboard', engine, data, maxPlayers: 4, gamePort, bind: '127.0.0.1', discovery: false, onlineNoticeAccepted: false, rotation: [`swbf1/${map}:${side}:conquest`] }), { mode: 0o600 });
  child = spawn(process.execPath, [join(serverRepo, 'src/main.js'), '--serve'], { env: { ...process.env, BFE_STATE_DIR: directory, PORT: String(healthPort) }, stdio: ['ignore', 'pipe', 'pipe'] });
  // SCOREBOARD_SERVER_LOG=<file> keeps the server's whole output there.
  const serverLog = process.env.SCOREBOARD_SERVER_LOG ? (await import('node:fs')).createWriteStream(process.env.SCOREBOARD_SERVER_LOG) : null;
  for (const stream of [child.stdout, child.stderr]) stream.on('data', chunk => { serverLog?.write(chunk); logs.push(String(chunk)); if (logs.length > 200) logs.shift(); });
  let info;
  for (let attempt = 0; attempt < 480 && !info; attempt++) {
    if (child.exitCode !== null) throw Error('server stopped: ' + logs.join(''));
    try { const response = await fetch(`http://127.0.0.1:${gamePort}/info`); if (response.ok) info = await response.json(); } catch {}
    if (!info) await sleep(250);
  }
  if (!info) throw Error('server did not come up: ' + logs.slice(-10).join(''));
  say(`server ${info.simulation} ${info.query}`);
  let base = process.env.SCOREBOARD_TEST_URL;
  if (!base) {
    const { createServer } = await import('vite');
    const { ISOLATION_HEADERS } = await import('../vite.config.js');
    // 'mpa': a missing optional file must 404, as the server's reader has
    // it, not come back as index.html, or the content fingerprints differ
    // (src/runtime/content-hash.js) and both players only spectate.
    vite = await createServer({ configFile: false, appType: 'mpa', root: engine, publicDir: data, logLevel: 'error', optimizeDeps: { noDiscovery: true, include: [] },
      server: { host: '127.0.0.1', port: vitePort, strictPort: true, watch: null, headers: ISOLATION_HEADERS } });
    await vite.listen();
    base = `http://127.0.0.1:${vitePort}`;
  }
  browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--autoplay-policy=no-user-gesture-required'] });
  pageNetwork = await startPageNetwork();
  const names = ['JOSH', 'AMY'];
  const pages = [];
  for (let i = 0; i < 2; i++) {
    const context = await browser.newContext({ viewport: { width, height } });
    const page = await context.newPage();
    const identity = join(directory, `client${i}`);
    const bridge = createNetworkBridge(identity, message => page.evaluate(message => window.dispatchEvent(new CustomEvent('bfe-test-signal', { detail: message })), message).catch(() => {}));
    await page.exposeFunction('bfeTestRequest', bridge);
    await pageNetwork.attach(page, identity);
    await page.addInitScript(({ name }) => {
      localStorage.setItem('battlefront-eternal.settings', JSON.stringify({ waterQuality: 'low', antialiasing: 'off', shadows: 'off', renderScale: 0.5, playerName: name }));
      window.bfeDesktop = { network: {
        request: (...args) => window.bfeTestRequest(...args),
        openSession: id => window.bfeTestOpenSession(id),
        onSignal: callback => { const listener = event => callback(event.detail); window.addEventListener('bfe-test-signal', listener); return () => window.removeEventListener('bfe-test-signal', listener); },
      } };
    }, { name: names[i] });
    page.on('pageerror', error => { errors.push(`client${i}: ${error.message}`); say(`client${i} page error: ${error.message}`); });
    // Prepared content can lack presentation-only files (enhanced audio,
    // texture variants); their 404s are not errors here.
    page.on('console', message => { if (message.type() === 'error' && !/^Failed to load resource: the server responded with a status of 404/.test(message.text())) { errors.push(`client${i}: ${message.text()}`); say(`client${i} console error: ${message.text()}`); } });
    await page.route('**/@vite/client', route => route.fulfill({ contentType: 'application/javascript', body: '' }));
    const query = new URLSearchParams(info.query);
    // The Classic board (menus=classic): the battle's own is the page view.
    for (const [key, value] of Object.entries({ network: '1', worker: '1', server: info.id, address: '127.0.0.1', port: gamePort, shadows: 'off', menus: 'classic' })) query.set(key, String(value));
    await page.goto(`${base}/?${query}`);
    pages.push(page);
  }
  await Promise.all(pages.map(page => page.waitForFunction(() => window.__bf?.client?.ready, null, { timeout: 300000 })));
  await Promise.all(pages.map(page => page.evaluate(async () => { await __bf.ready; __bf.begin(); })));
  const waitFor = async (page, fn, arg, what) => {
    try { await page.waitForFunction(fn, arg, { timeout: 120000 }); }
    catch (error) { throw Error(`${what}: ${error.message.split('\n')[0]} ${JSON.stringify(await diagnose(page))}`); }
  };
  // Both players alive, on the teams the server gives them (it balances
  // them, so one on each).
  for (const [i, page] of pages.entries()) {
    await waitFor(page, () => __bf.client.entity?.eligibility && __bf.playerUnit.generation > 0 && ['unspawned', 'awaiting'].includes(__bf.spawnPort.humanState().phase), null, `client${i} at the spawn screen`);
    await alive(page);
    await page.evaluate(() => { if (__bf.menu.isOpen) { __bf.controls.locked = true; __bf.menu.close(true); } });
  }
  say(`teams: ${JSON.stringify(await Promise.all(pages.map((page) => page.evaluate(() => __bf.playerTeam))))}`);
  say('both players in; letting the bots fight');
  await sleep(30000);
  const [josh, amy] = pages;

  await check('Tab held shows every Character of both teams, the bots by name, the players with their pings; let go, it closes', async () => {
    await alive(josh);
    // Held well past the half second (see SETTLE).
    await josh.keyboard.down('Tab');
    const began = await josh.evaluate(() => __bf.scoreboard.input.time);
    await josh.waitForFunction(t => __bf.scoreboard.input.time > t + .6, began);
    const s = await board(josh);
    assert.equal(s.state, 'peek');
    assert.equal(s.layer, true, 'the board is the HUD\'s layer');
    assert.equal(s.controls, false, 'the soldier takes no input');
    assert.equal(s.ping, true, 'a network game\'s table has the PING column');
    assert.deepEqual([s.lists[1].length, s.lists[2].length], s.roster, 'every Character of both teams');
    const all = rows(s), bots = all.filter(r => !r.human), humans = all.filter(r => r.human);
    assert.ok(bots.length >= 60 && bots.every(r => r.name && r.ping === null), `bots listed by name, without ping (${bots.length})`);
    assert.deepEqual(humans.map(r => r.name).sort(), names.slice().sort(), 'both players listed');
    assert.ok(humans.every(r => Number.isFinite(r.ping)), `each player has his ping (${humans.map(r => r.ping)})`);
    assert.equal(all.find(r => r.local)?.name, 'JOSH');
    assert.ok(bots.some(r => r.deaths > 0) && bots.some(r => r.kills > 0), 'the bots\' kills and deaths arrive');
    say(`table: ${JSON.stringify({ 1: s.lists[1].slice(0, 5), 2: s.lists[2].slice(0, 5) })}`);
    await josh.keyboard.up('Tab');
    await sleep(500);
    const after = await board(josh);
    assert.equal(after.state, 'closed', 'let go after half a second: closed');
    assert.equal(after.controls, true, 'the soldier has his controls back');
  });

  await check('screenshots: both clients\' Tab tables', async () => {
    for (const [i, page] of pages.entries()) {
      await alive(page);
      await pin(page);
      await shot(page, `tab-${names[i].toLowerCase()}`);
      await unpin(page);
    }
  });

  await check('a death on one client shows on the other\'s table', async () => {
    // The port's rows are the table's, live whether or not it is up.
    const amyRow = page => page.evaluate(() => __bf.scoreboardPort.roster().find(r => r.name === 'AMY'));
    await alive(amy);
    const before = await amyRow(josh);
    await amy.evaluate(() => __bf.spawnPort.suicide());
    await josh.waitForFunction(deaths => __bf.scoreboardPort.roster().find(r => r.name === 'AMY')?.deaths === deaths + 1, before.deaths, { timeout: 15000 });
    await amy.waitForFunction(deaths => __bf.scoreboardPort.roster().find(r => r.name === 'AMY')?.deaths === deaths + 1, before.deaths, { timeout: 15000 });
  });

  await check('DETAILS from Tab\'s table: a bot\'s record arrives during play; the other player\'s ends with his CONNECTION', async () => {
    await alive(josh);
    await pin(josh);
    const s = await board(josh);
    const bot = rows(s).filter(r => !r.human).sort((a, b) => b.deaths + b.kills - a.deaths - a.kills)[0];
    const record = await details(josh, bot.slot);
    assert.equal(record.page, 'details');
    const value = label => record.rows.find(([l]) => l === label)?.[1];
    say(`${bot.name}'s DETAILS: ${JSON.stringify(record.rows)}`);
    assert.ok(value('FAVORITE CLASS'), 'his favourite class');
    assert.notEqual(value('LONGEST LIVING STREAK'), ' 0:00', 'his longest life');
    assert.ok(!record.rows.some(([l]) => l === 'CONNECTION'), 'a bot has no connection');
    await shot(josh, 'details-bot');
    await josh.evaluate(() => __bf.scoreboard.model.back());
    await sleep(500);
    const other = rows(await board(josh)).find(r => r.name === 'AMY');
    const player = await details(josh, other.slot);
    say(`AMY's DETAILS: ${JSON.stringify(player.rows)}`);
    assert.deepEqual(player.rows.at(-1), ['CONNECTION', 'DIRECT IPV4']);
    await shot(josh, 'details-player');
    await josh.evaluate(() => __bf.scoreboard.model.back());
    await sleep(500);
    await unpin(josh);
  });

  await check('chat and Tab keep apart', async () => {
    await alive(josh);
    await josh.keyboard.press('KeyT');
    await josh.waitForFunction(() => __bf.chat.isOpen, null, { timeout: 5000 });
    await josh.keyboard.down('Tab'); await sleep(100); await josh.keyboard.up('Tab');
    await sleep(500);
    let s = await josh.evaluate(() => ({ board: __bf.scoreboard.state, chat: __bf.chat.isOpen, focus: document.activeElement?.getAttribute('aria-label') }));
    assert.deepEqual(s, { board: 'closed', chat: true, focus: 'Chat message' }, 'Tab in the entry: no table, the focus stays');
    await josh.keyboard.type('gg');
    assert.equal(await josh.evaluate(() => document.activeElement.value), 'gg');
    await josh.keyboard.press('Escape');
    await josh.waitForFunction(() => !__bf.chat.isOpen, null, { timeout: 5000 });
    await alive(josh);
    await pin(josh);
    await josh.keyboard.press('KeyT');
    await sleep(500);
    s = await josh.evaluate(() => ({ board: __bf.scoreboard.state, chat: __bf.chat.isOpen }));
    assert.deepEqual(s, { board: 'peek', chat: false }, 'T over the table opens no chat');
    await unpin(josh);
  });

  if (process.env.DROIDEKA_DEATH_NETWORK) await check('remote droideka death stays hidden after its dissolve over real UDP', async () => {
    const flags = await Promise.all(pages.map(page => page.evaluate(() => __bf.playerUnit.walker)));
    const index = flags.indexOf(true); assert.ok(index >= 0, 'one human spawned as a droideka');
    const victim = pages[index], observer = pages[1 - index];
    await alive(victim);
    const id = await victim.evaluate(() => __bf.playerUnit.id);
    await observer.waitForFunction(id => __bf.units.byId(id)?.alive, id);
    await observer.evaluate(id => {
      const u = __bf.units.byId(id), generation = u.generation;
      window.__deathRows = [];
      const record = () => {
        const row = { generation: u.generation, dead: u.dead, mode: u.soldier.mode,
          visible: u.actor.root.visible, removed: u.removed, gone: Boolean(u.gone),
          dissolve: u.actor.body.userData.heated?.dissolve || 0 };
        __deathRows.push(row);
        if (__deathRows.length < 1200 && u.generation === generation) requestAnimationFrame(record);
      }; requestAnimationFrame(record);
    }, id);
    await victim.evaluate(() => __bf.spawnPort.suicide());
    await observer.waitForFunction(() => __deathRows.some(r => r.dead && r.mode === 4 && !r.visible), null, { timeout: 30000 });
    await sleep(2000);
    const rows = await observer.evaluate(() => __deathRows);
    const end = rows.findIndex(r => r.dead && r.mode === 4 && !r.visible);
    assert.ok(rows.some(r => r.dead && r.visible && r.dissolve > 0), 'the dead body dissolves');
    assert.ok(rows.slice(end).filter(r => r.generation === rows[end].generation).every(r => !r.visible), 'later records never show that life again');
    say(`remote droideka: ${rows.length} frames, hidden from frame ${end}`);
  });

  assert.deepEqual(errors, [], 'page errors');
} catch (error) {
  failures.push(error.stack || error.message);
} finally {
  await browser?.close().catch(() => {});
  pageNetwork?.close();
  await vite?.close().catch(() => {});
  if (child && child.exitCode === null) { child.kill('SIGTERM'); await sleep(500); }
  await rm(directory, { recursive: true, force: true });
}
if (failures.length) {
  console.log(`\n${failures.length} failed:\n${failures.join('\n')}`);
  if (logs.length) console.log('server log tail:\n' + logs.slice(-15).join(''));
  process.exit(1);
}
console.log(`\nall passed; screenshots in ${OUT}`);
