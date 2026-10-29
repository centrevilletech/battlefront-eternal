// The renderer chooses game settings; paths, executable and management token
// remain owned by the main process. The detached server survives UI closure.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync, openSync, closeSync, statSync } from 'node:fs';
import path from 'node:path';
import { MAX_MATCH_PLAYERS } from '../src/battle-limits.js';
import { SLOW_LINKS_FLAG } from './slow-links.js';
// The hosted server's loopback admin port and its game port (UDP gameplay,
// TCP /info). Overridable where another program already holds them.
const port = (name, fallback) => { const value = Number(process.env[name]); return Number.isInteger(value) && value > 0 && value < 65536 ? value : fallback; };
const ADMIN_PORT = port('BFE_HOST_ADMIN_PORT', 18460), GAME_PORT = port('BFE_HOST_GAME_PORT', 18461);
// Started by a native Linux Steam, whose reaper waits for every process the
// game starts, a server kept after the game would keep it "playing" in
// Steam, and Steam's STOP would end the server: the user's systemd starts
// it instead, outside the game's processes. Resolves the unit's name, or
// null where there is no user systemd (the server then starts as usual).
export function startOutside({ executable, args, env, log, run = spawn }) {
  return new Promise((resolve) => {
    const unit = `battlefront-eternal-server-${Date.now()}`;
    const child = run('systemd-run', ['--user', '--collect', '--quiet', `--unit=${unit}`, `--property=StandardOutput=append:${log}`, `--property=StandardError=append:${log}`,
      ...Object.entries(env).map(([key, value]) => `--setenv=${key}=${value}`), executable, ...args], { stdio: 'ignore', windowsHide: true });
    child.on('error', () => resolve(null));
    child.on('exit', code => resolve(code === 0 ? unit : null));
  });
}
const unitActive = unit => spawnSync('systemctl', ['--user', 'is-active', '--quiet', unit], { timeout: 3000 }).status === 0;

export function createHostController({ executable, serverRoot, engineRoot, contentRoot, stateRoot, consent, outside = () => false }) {
  mkdirSync(stateRoot, { recursive: true, mode: 0o700 });
  let starting = null;
  async function admin(value) {
    const token = readFileSync(path.join(stateRoot, 'operator-token'), 'utf8').trim();
    const response = await fetch(`http://127.0.0.1:${ADMIN_PORT}/admin`, { method: 'POST', headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` }, body: JSON.stringify(value), signal: AbortSignal.timeout(3000) });
    if (!response.ok) throw Error('Cannot manage the hosted server');
    return response.json();
  }
  async function status() {
    try { const result = await admin({ action: 'status' }); return { ...result, active: true, starting: Boolean(starting) }; }
    catch { return { active: false, starting: Boolean(starting) }; }
  }
  async function start(options) {
    const current = await status(); if (current.active) return joinInfo();
    if (!options || !['lan', 'public'].includes(options.mode) || typeof options.name !== 'string' || !options.name.trim() || options.name.length > 80 || !Number.isInteger(options.maxPlayers) || options.maxPlayers < 2 || options.maxPlayers > MAX_MATCH_PLAYERS || !/^[a-z0-9_-]{1,32}$/.test(options.map) || !['rep', 'cis', 'all', 'imp'].includes(options.side) || !['auto', 'always', 'never'].includes(options.relay)) throw Error('Invalid host settings');
    if (options.mode === 'public' && !await consent()) throw Error('Accept the online privacy notice before hosting publicly.');
    const content = contentRoot();
    if (!existsSync(path.join(content, 'common/common.json')) || !existsSync(path.join(content, 'maps', options.map, 'map.json'))) throw Error('Import the selected map from your game before hosting.');
    const entry = path.join(serverRoot, 'src/main.js');
    if (!existsSync(entry) || !existsSync(path.join(engineRoot, 'src/game.js'))) throw Error('The server runtime is missing. Install the complete desktop release.');
    const config = { name: options.name.trim(), engine: engineRoot, data: content, rotation: [`swbf1/${options.map}:${options.side}:conquest`], maxPlayers: options.maxPlayers, onlineNoticeAccepted: options.mode === 'public', master: 'https://online.battlefronteternal.com', relayPolicy: options.mode === 'lan' ? 'never' : options.relay, honorGlobalBans: true, gamePort: GAME_PORT };
    writeFileSync(path.join(stateRoot, 'config.json'), JSON.stringify(config, null, 2), { mode: 0o600 });
    const log = path.join(stateRoot, 'server.log');
    if (existsSync(log) && statSync(log).size > 8388608) writeFileSync(log, '');
    const fd = openSync(log, 'a', 0o600);
    const serverEnv = { ELECTRON_RUN_AS_NODE: '1', BFE_STATE_DIR: stateRoot, BFE_ENGINE_DIR: engineRoot, BFE_DATA_DIR: content, PORT: String(ADMIN_PORT) };
    let failure = null, unit = null;
    try {
      // Its connections (the master) wait on slow links as ours do.
      const args = [SLOW_LINKS_FLAG, entry, '--serve'];
      if (outside()) unit = await startOutside({ executable, args, env: serverEnv, log });
      if (!unit) {
        const child = spawn(executable, args, { detached: true, windowsHide: true, stdio: ['ignore', fd, fd], env: { ...process.env, ...serverEnv } });
        child.on('error', error => { failure = error; }); child.on('exit', code => { if (code !== null) failure = Error(`Server exited (${code}). See the hosting log.`); }); child.unref();
      }
    } finally { closeSync(fd); }
    const ready = (async () => {
      for (let i = 0; i < 120; i++) {
        if (unit && i > 0 && !unitActive(unit)) failure ??= Error('Server exited. See the hosting log.');
        if (failure) throw failure;
        try { return await joinInfo(); } catch {}
        await new Promise(resolve => setTimeout(resolve, 500));
      }
      throw Error('The server is still starting. Check its status or hosting log.');
    })();
    return ready;
  }
  async function joinInfo() {
    const current = await admin({ action: 'status' });
    if (current.status !== 'running') throw Error('Hosted server is still starting');
    const response = await fetch(`http://127.0.0.1:${GAME_PORT}/info`, { signal: AbortSignal.timeout(3000) });
    if (!response.ok) throw Error('Hosted server is unavailable');
    return { ...await response.json(), address: '127.0.0.1', port: GAME_PORT };
  }
  return async (action, value) => {
    if (action === 'status') return status();
    if (action === 'start') {
      if (starting) return starting;
      starting = start(value);
      try { return await starting; } finally { starting = null; }
    }
    if (action === 'join') return joinInfo();
    if (action === 'stop') return admin({ action: 'stop' });
    if (action === 'players') return admin({ action: 'status' });
    if (action === 'restart') return admin({ action: 'restart' });
    if (action === 'kick' || action === 'ban' || action === 'unban') {
      if (!value || !/^[a-f0-9]{64}$/.test(value.id)) throw Error('Invalid player identity');
      return admin({ action, id: value.id, hours: action === 'ban' ? 24 : undefined, reason: String(value.reason || '').slice(0, 240) });
    }
    throw Error('Unknown hosting action');
  };
}
