#!/usr/bin/env node
// One real client against a local bfeserver, scripted: joins at the spawn
// screen, spawns, fires, dies, chats, and prints what the page shows at each
// step (animator action, death camera, HUD heading, chat names, scores). CPU
// mode: the page runs the whole client but draws nothing. Needs Playwright
// (PLAYWRIGHT_MODULE) and a Chromium (BFE_CHROMIUM, default /usr/bin/chromium).
//
//   BFE_DATA_DIR=<imported game data> [BFE_SERVER_DATA_DIR=<the server's own data,
//   e.g. a gameplay subset>] BFE_SERVER_REPO=<server checkout> \
//   PLAYWRIGHT_MODULE=/tmp/pw/node_modules/playwright node tools/mp-inspect.mjs [--seconds=20]
//   [--rotation=swbf1/tat3:rep:conquest]
//
// Nothing here touches public services.
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { createServer } from 'vite';
import { createNetworkBridge } from '../electron/network.js';
import { startPageNetwork } from './net-page-bridge.mjs';

const arg = (name, fallback) => { const found = process.argv.find(value => value.startsWith(`--${name}=`)); return found === undefined ? fallback : found.slice(name.length + 3); };
const engine = resolve('.'), data = resolve(process.env.BFE_DATA_DIR || 'public'), serverData = resolve(process.env.BFE_SERVER_DATA_DIR || data), serverRepo = resolve(process.env.BFE_SERVER_REPO || '../battlefront-eternal-server');
const gamePort = Number(arg('port', 18761)), healthPort = Number(arg('health-port', 18762)), vitePort = Number(arg('vite-port', 18763));
const seconds = Number(arg('seconds', 20)), name = arg('name', 'Josh Test'), rotation = arg('rotation', 'swbf1/nab2:rep:conquest').split(',');
const playwright = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const { chromium } = playwright.chromium ? playwright : playwright.default;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const stage = message => console.error(`[mp-inspect] ${message}`);
const directory = await mkdtemp(join(tmpdir(), 'bfe-inspect-'));
let child, vite, browser, pageNetwork;
const logs = [];
try {
  await writeFile(join(directory, 'config.json'), JSON.stringify({ name: 'Inspect', engine, data: serverData, maxPlayers: 4, gamePort, bind: '127.0.0.1', discovery: false, onlineNoticeAccepted: false, rotation }), { mode: 0o600 });
  child = spawn(process.execPath, [join(serverRepo, 'src/main.js'), '--serve'], { env: { ...process.env, BFE_STATE_DIR: directory, PORT: String(healthPort) }, stdio: ['ignore', 'pipe', 'pipe'] });
  for (const stream of [child.stdout, child.stderr]) stream.on('data', chunk => { logs.push(String(chunk)); if (logs.length > 200) logs.shift(); });
  let info;
  for (let attempt = 0; attempt < 240 && !info; attempt++) {
    if (child.exitCode !== null) throw Error('server stopped: ' + logs.join(''));
    try { const response = await fetch(`http://127.0.0.1:${gamePort}/info`); if (response.ok) info = await response.json(); } catch {}
    if (!info) await sleep(250);
  }
  if (!info) throw Error('server did not come up: ' + logs.slice(-10).join(''));
  stage(`server ${info.simulation} ${info.query}`);
  vite = await createServer({ configFile: false, root: engine, publicDir: data, logLevel: 'error', optimizeDeps: { noDiscovery: true, include: [] }, server: { host: '127.0.0.1', port: vitePort, strictPort: true, watch: null } });
  await vite.listen();
  browser = await chromium.launch({ executablePath: process.env.BFE_CHROMIUM || '/usr/bin/chromium', args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--autoplay-policy=no-user-gesture-required', '--no-sandbox'] });
  const context = await browser.newContext({ viewport: { width: 640, height: 480 } });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => { errors.push(error.message); stage('page error: ' + error.message); });
  page.on('console', message => { if (message.type() === 'error') { errors.push(message.text()); stage('console error: ' + message.text()); } });
  const bridge = createNetworkBridge(join(directory, 'client'), message => page.evaluate(message => window.dispatchEvent(new CustomEvent('bfe-test-signal', { detail: message })), message).catch(() => {}));
  await page.exposeFunction('bfeTestRequest', bridge);
  // The page's battle port reaches a real network process in this process.
  pageNetwork = await startPageNetwork();
  await pageNetwork.attach(page, join(directory, 'client'));
  await page.addInitScript(({ name }) => {
    let game; Object.defineProperty(window, '__bf', { configurable: true, get: () => game, set: value => { game = value; game.graphics.render = () => {}; game.graphics.renderer.render = () => {}; } });
    localStorage.setItem('battlefront-eternal.settings', JSON.stringify({ waterQuality: 'low', antialiasing: 'off', shadows: 'off', renderScale: 0.5, playerName: name }));
    window.bfeDesktop = { network: { request: (...args) => window.bfeTestRequest(...args), openSession: id => window.bfeTestOpenSession(id), onSignal: callback => { const listener = event => callback(event.detail); window.addEventListener('bfe-test-signal', listener); return () => window.removeEventListener('bfe-test-signal', listener); } } };
  }, { name });
  await page.route('**/@vite/client', route => route.fulfill({ contentType: 'application/javascript', body: '' }));
  const query = new URLSearchParams(info.query);
  for (const [key, value] of Object.entries({ network: '1', worker: '1', server: info.id, address: '127.0.0.1', port: gamePort, shadows: 'off', loading: '0' })) query.set(key, String(value));
  stage('loading page');
  await page.goto(`http://127.0.0.1:${vitePort}/?${query}`);
  await page.waitForFunction(() => window.__bf?.client?.ready, null, { timeout: 180000 });
  await page.evaluate(async () => { await __bf.ready; __bf.begin(); });
  const state = () => page.evaluate(() => {
    const u = __bf.playerUnit, a = u.animator, c = __bf.client;
    return { tick: __bf.worldClock.tick, phase: __bf.spawnPort.humanState().phase, team: __bf.playerTeam, alive: u.alive, dead: __bf.combat.dead, removed: u.removed,
      id: u.id, generation: u.generation, className: u.className, action: a.action, visible: u.actor.root.visible,
      deathCamera: __bf.deathView ? { active: __bf.deathView.active, time: Math.round((__bf.deathShot?.active ? __bf.deathShot.time : __bf.deathView.time) * 100) / 100, killer: __bf.deathView.killerUnit ?? null, known: __bf.deathShot?.known ?? null } : null,
      screen: __bf.spawnScreen?.isOpen ?? null, status: c.host.statusText, path: c.host.path, closed: c.host.closed };
  });
  stage('joined: ' + JSON.stringify(await state()));
  // The first own correction and spawn summary name the life the choice must
  // carry; a request before them belongs to no life and is dropped.
  await page.waitForFunction(() => __bf.client.entity?.eligibility && __bf.playerUnit.generation > 0 && ['unspawned', 'awaiting'].includes(__bf.spawnPort.humanState().phase), null, { timeout: 60000 });
  stage('at the screen: ' + JSON.stringify(await state()));
  const roster = await page.evaluate(() => __bf.spawnPort.roster(__bf.playerTeam).map(c => c.className));
  stage('roster for team: ' + JSON.stringify(roster));
  await page.evaluate(className => __bf.spawnPort.request({ className, post: null }), roster[0]);
  await page.waitForFunction(() => __bf.playerUnit.alive && __bf.spawnPort.humanState().phase === 'alive', null, { timeout: 60000 });
  stage('spawned: ' + JSON.stringify(await state()));
  // Chat round trip: what name does the server echo?
  await page.evaluate(() => { window.__chatSeen = []; const receive = __bf.chat.receive.bind(__bf.chat); __bf.chat.receive = message => { window.__chatSeen.push(message); return receive(message); }; __bf.chat.send({ text: 'hello from the probe', channel: 'global' }); });
  await sleep(1500);
  stage('chat: ' + JSON.stringify(await page.evaluate(() => window.__chatSeen.map(m => ({ name: m.name, channel: m.channel, text: m.text })))));
  // Fire for a second while turning, then watch the HUD heading against the look.
  await page.evaluate(() => { __bf.controls.press?.('Mouse0'); });
  await sleep(1200);
  await page.evaluate(() => { __bf.controls.release?.('Mouse0'); });
  const scores = () => page.evaluate(() => ({ human: __bf.battle.human ? { slot: __bf.battle.characters.indexOf(__bf.battle.human), name: __bf.battle.human.name, kills: __bf.battle.human.kills, deaths: __bf.battle.human.deaths, mine: __bf.playerUnit.character === __bf.battle.human } : null, characters: __bf.battle.characters.length, humans: __bf.battle.characters.filter(c => c.human).map(c => ({ name: c.name, team: c.team, kills: c.kills, deaths: c.deaths, link: c.link })) }));
  stage('scores: ' + JSON.stringify(await scores()));
  // Tab: every Character, bots included, and each player's ping (the board's own rows).
  stage('tab table: ' + JSON.stringify(await page.evaluate(() => { const board = __bf.scoreboard; if (!board) return null; const opened = board.peek(); const row = r => [r.name, r.kills, r.deaths, r.points, r.ping, r.human, r.local]; const out = { opened, lists: { 1: board.model.lists[1].map(row), 2: board.model.lists[2].map(row) } }; board.close(); return out; })));
  // Die: the pause menu's RESPAWN, then watch the death flow tick by tick.
  const before = await state();
  await page.evaluate(() => __bf.spawnPort.suicide());
  const samples = [];
  for (let n = 0; n < 70; n++) { await sleep(100); samples.push(await state()); if (samples.at(-1).phase === 'awaiting' && n > 45) break; }
  const transitions = samples.filter((s, i) => i === 0 || JSON.stringify([s.phase, s.dead, s.removed, s.action, s.deathCamera?.active, s.screen, s.visible]) !== JSON.stringify([samples[i - 1].phase, samples[i - 1].dead, samples[i - 1].removed, samples[i - 1].action, samples[i - 1].deathCamera?.active, samples[i - 1].screen, samples[i - 1].visible]));
  stage('death flow (changes only): ' + JSON.stringify({ before: { action: before.action, generation: before.generation }, transitions }));
  stage('death clip available: ' + JSON.stringify(await page.evaluate(() => { const a = __bf.playerUnit.animator; return { backward: a.clips?.has?.('human_rifle_stand_death_backward') ?? null, die: a.clips?.has?.('human_rifle_die') ?? null, clipCount: a.clips?.size ?? null, sample: [...(a.clips?.keys?.() ?? [])].filter(k => k.includes('death') || k.includes('die')).slice(0, 8) }; })));
  await sleep(2500);
  stage('scores after death: ' + JSON.stringify(await scores()));
  stage('errors: ' + JSON.stringify(errors));
} finally {
  await browser?.close().catch(() => {});
  pageNetwork?.close();
  await vite?.close().catch(() => {});
  if (child && child.exitCode === null) { child.kill('SIGTERM'); await sleep(500); }
  await rm(directory, { recursive: true, force: true });
}
