// The simulation port's verification ladder, a command a level (the plan's
// V1 to V6; V7, the emulator and laptop timing, runs outside the repository):
//
//   node tools/sim-verify.mjs v1          twin tests: the kernels' JavaScript twins (tests/*-kernel.test.js), the
//                                         store's and the capture's (tests/sim-store.test.js, tests/sim-capture.test.js)
//                                         and the Rust crates' tests
//   node tools/sim-verify.mjs v2          per-phase replays of the corpus BFE_REPLAY_CORPUS names
//                                         (rust/sim-replay's, and rust/sim's behind its replay feature)
//   node tools/sim-verify.mjs v3 [opts]   in-battle shadow checks (tools/check-ray-kernel.mjs; its options)
//   node tools/sim-verify.mjs v4 [opts]   the world-hash matrix (tools/world-hash-matrix.mjs; its options)
//   node tools/sim-verify.mjs v5 [ENV=V]  the prediction checks, each run as is and with the variant's
//                                         environment (default BFE_MOVEMENT=wasm), their digests compared
//   node tools/sim-verify.mjs v6 [opts]   world-hash under this Node and under Electron's (ELECTRON_RUN_AS_NODE),
//                                         or any other (--other PATH): the first tick they part, reported
//
// Battles read the game's files from BFE_CONTENT_DIR (or public/); the
// prediction checks from BFE_DATA_DIR (defaulting to BFE_CONTENT_DIR). Run
// every level under the Node the port takes as its reference (Node 24.22,
// the production server's V8): the results of another V8 differ in the
// last bit. The exit status is 0 when the level passes.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const [level, ...rest] = process.argv.slice(2);
const content = process.env.BFE_CONTENT_DIR || path.join(root, 'public');
const env = { ...process.env, BFE_CONTENT_DIR: content, BFE_DATA_DIR: process.env.BFE_DATA_DIR || content };
const node = process.execPath;

function run(command, args, options = {}) {
  console.log(`$ ${[command, ...args].join(' ')}${options.cwd && options.cwd !== root ? `   (in ${path.relative(root, options.cwd) || '.'})` : ''}`);
  const { capture, env: extra, ...rest } = options;
  const r = spawnSync(command, args, { cwd: root, ...rest, env: { ...env, ...extra }, stdio: capture ? ['ignore', 'pipe', 'pipe'] : 'inherit', encoding: 'utf8', maxBuffer: 1 << 28 });
  if (r.error) throw r.error;
  return r;
}
const rustCrates = () => ['rays', 'anim', 'sim-replay', ...(fs.existsSync(path.join(root, 'rust/sim/Cargo.toml')) ? ['sim'] : [])]
  .filter((name) => fs.existsSync(path.join(root, 'rust', name, 'Cargo.toml')));
const simHasReplay = () => fs.existsSync(path.join(root, 'rust/sim/Cargo.toml')) && /^\s*replay\s*=/m.test(fs.readFileSync(path.join(root, 'rust/sim/Cargo.toml'), 'utf8'));

const levels = {
  v1() {
    const twins = fs.readdirSync(path.join(root, 'tests')).filter((f) => /kernel\.test\.js$|^sim-(capture|store)\.test\.js$/.test(f)).map((f) => `tests/${f}`);
    const js = run(node, ['--test', ...twins]);
    const rust = run('cargo', ['test', ...rustCrates().flatMap((c) => ['-p', c])], { cwd: path.join(root, 'rust') });
    return js.status === 0 && rust.status === 0;
  },
  v2() {
    if (!process.env.BFE_REPLAY_CORPUS) { console.error('v2: BFE_REPLAY_CORPUS names no corpus (tools/sim-capture.mjs --out DIR makes one)'); return false; }
    const own = run('cargo', ['test', '-p', 'sim-replay', '--', '--nocapture'], { cwd: path.join(root, 'rust') });
    const sim = simHasReplay() ? run('cargo', ['test', '-p', 'sim', '--features', 'replay', '--', '--nocapture'], { cwd: path.join(root, 'rust') }) : { status: 0 };
    return own.status === 0 && sim.status === 0;
  },
  v3() {
    return run(node, ['tools/check-ray-kernel.mjs', ...rest]).status === 0;
  },
  v4() {
    return run(node, ['tools/world-hash-matrix.mjs', ...rest]).status === 0;
  },
  v5() {
    // KEY=VALUE pairs: the variant's environment.
    const variant = Object.fromEntries((rest.length ? rest : ['BFE_MOVEMENT=wasm']).map((kv) => kv.split('=')));
    const maps = ['nab2', 'hot1', 'geo1', 'kas2'];
    const checks = [
      ['own-player-correction-check', {}], ['vehicle-correction-check', {}], ['prediction-timing', {}], ['vehicle-prediction-timing', {}],
      ...maps.map((map) => ['ground-player-correction-check', { BFE_TEST_MAP: map, BFE_TEST_SIDE: map === 'hot1' ? 'imp' : 'rep' }]),
    ];
    // What may differ between the two runs: where and how they ran.
    const labels = new Set(['source', 'movement', 'runtimeSourceSha256']);
    const strip = (text) => {
      try { return JSON.stringify(JSON.parse(text), (k, v) => (labels.has(k) ? undefined : v)); } catch { return text.trim(); }
    };
    let ok = true;
    for (const [tool, extra] of checks) {
      const a = run(node, [`tools/${tool}.mjs`], { capture: true, env: extra }), b = run(node, [`tools/${tool}.mjs`], { capture: true, env: { ...extra, ...variant } });
      const fa = a.status === 0 ? strip(a.stdout) : `failed: ${(a.stderr.match(/(Error|AssertionError)[^\n]*/) ?? [a.stderr.trim().split('\n').pop()])[0]}`;
      const fb = b.status === 0 ? strip(b.stdout) : `failed: ${(b.stderr.match(/(Error|AssertionError)[^\n]*/) ?? [b.stderr.trim().split('\n').pop()])[0]}`;
      const same = fa === fb;
      if (!same) ok = false;
      const label = `${tool}${extra.BFE_TEST_MAP ? ` ${extra.BFE_TEST_MAP}` : ''}`;
      console.log(`${same ? 'same     ' : 'DIFFERENT'} ${label}${fa.startsWith('failed') ? ` (both ${fa})` : ''}`);
      if (!same) console.log(`  as is:   ${fa.slice(0, 400)}\n  variant: ${fb.slice(0, 400)}`);
    }
    return ok;
  },
  v6() {
    const opt = (name, fallback) => { const i = rest.indexOf(`--${name}`); return i < 0 ? fallback : rest[i + 1]; };
    // Electron as npm's electron package installs it (dist/ and path.txt).
    const pathTxt = path.join(root, 'node_modules/electron/path.txt');
    const electron = fs.existsSync(pathTxt) ? path.join(root, 'node_modules/electron/dist', fs.readFileSync(pathTxt, 'utf8').trim()) : null;
    const other = opt('other', electron);
    if (!other || !fs.existsSync(other)) { console.error('v6: no Electron in node_modules (npm ci without its download?): name a binary with --other PATH'); return false; }
    const args = rest.filter((a, i) => a !== '--other' && rest[i - 1] !== '--other');
    const battle = args.length ? args : ['--map', 'nab2', '--side', 'rep', '--seed', '1', '--ticks', '2400'];
    // An Electron runs as Node with this; a Node ignores it.
    const otherEnv = { ELECTRON_RUN_AS_NODE: '1' };
    const versions = (binary, e) => {
      const r = spawnSync(binary, ['-p', 'JSON.stringify(process.versions)'], { env: { ...env, ...e }, encoding: 'utf8' });
      if (r.status !== 0) throw Error(`${binary} does not run as Node: ${r.stderr || r.error}`);
      return JSON.parse(r.stdout);
    };
    const [mine, theirs] = [versions(node, {}), versions(other, otherEnv)];
    console.log(`this Node ${mine.node} (V8 ${mine.v8}) against ${path.basename(other)} ${theirs.electron ? `Electron ${theirs.electron}, ` : ''}Node ${theirs.node} (V8 ${theirs.v8})`);
    const runs = [run(node, ['tools/world-hash.mjs', ...battle], { capture: true }), run(other, ['tools/world-hash.mjs', ...battle], { capture: true, env: otherEnv })];
    if (runs.some((r) => r.status !== 0)) { console.error(runs.map((r) => r.stderr.slice(-800)).join('\n')); return false; }
    const [a, b] = runs.map((r) => r.stdout.trim().split('\n').map((line) => line.split(' ')));
    if (a.length === b.length && a.every((x, i) => x.join(' ') === b[i].join(' '))) { console.log(`the same, all ${a.length} ticks`); return true; }
    // Each hash on its own: the first tick it differs, and on how many ticks.
    for (const [k, name] of ['state', 'digest', 'published'].entries()) {
      const ticks = a.filter((x, i) => x[k + 1] !== b[i]?.[k + 1]).map((x) => x[0]);
      console.log(`${name.padEnd(9)} ${ticks.length ? `parts at tick ${ticks[0]}, different on ${ticks.length} of ${a.length} ticks` : `the same, all ${a.length} ticks`}`);
    }
    return false;
  },
};

if (!levels[level]) {
  console.error('usage: node tools/sim-verify.mjs v1|v2|v3|v4|v5|v6 [options] (see the top of the file)');
  process.exit(2);
}
const v8 = process.versions.v8;
if (!v8.startsWith('13.6.')) console.error(`sim-verify: note: this is V8 ${v8}; the port's reference is V8 13.6 (Node 24.22, the production server's)`);
process.exit(levels[level]() ? 0 : 1);
