import { contentPath } from './helpers/content.js';
import assert from 'node:assert/strict';
import test from 'node:test';
import { existsSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// Content-gated repeatability of the required Rust world. Synthetic kernel
// golden fixtures run in CI without these files.
const root = fileURLToPath(new URL('../', import.meta.url));
const content = contentPath();

// A battle on each kind of map, as a page plays it: acknowledging
// snapshots, shooting from a view behind the world, drawing every third tick.
const battles = [
  ['nab2', 'rep', 1, 1200, []],
  ['tat1', 'all', 2, 1200, ['--ack', '--view', '2.5']],
  ['bes1', 'rep', 1, 1200, ['--interval', '3']],
  ['hot1', 'imp', 1, 900, ['--ack']],
];

function fingerprints(args) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ['tools/world-hash.mjs', ...args], { cwd: root, env: { ...process.env, BFE_CONTENT_DIR: content }, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '', err = '';
    child.stdout.on('data', (chunk) => { out += chunk; });
    child.stderr.on('data', (chunk) => { err += chunk; });
    child.on('error', reject);
    child.on('close', (code) => code === 0 ? resolve(out.trim().split('\n')) : reject(Error(`world-hash ${args.join(' ')} failed (${code}): ${err.slice(-2000)}`)));
  });
}

for (const [map, side, seed, ticks, extra] of battles) {
  const args = ['--map', map, '--side', side, '--seed', String(seed), '--ticks', String(ticks), ...extra];
  test(`${args.join(' ')}: every tick the same in WebAssembly`, { skip: !existsSync(`${content}/maps/${map}/map.json`) && 'no game files' }, async () => {
    const [js, rays] = await Promise.all([fingerprints(args), fingerprints(args)]);
    for (const [name, run] of [['repeated world', rays]]) {
      assert.equal(run.length, ticks);
      const first = js.findIndex((line, i) => line !== run[i]);
      assert.equal(first, -1, first < 0 ? '' : `${name}: tick ${first + 1} differs:\n  js   ${js[first]}\n  wasm ${run[first]}`);
    }
    assert.equal(js.length, ticks);
  });
}
