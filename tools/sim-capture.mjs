// Captures battles for the simulation's per-phase replays in Rust
// (rust/sim-replay): each battle runs exactly as tools/world-hash.mjs runs
// it, with tools/sim-capture/plugin.mjs recording the chosen phases' frames
// (tools/sim-capture/specs.mjs) in the BFECAP format
// (tools/sim-capture/format.mjs).
//
//   BFE_CONTENT_DIR=<game-data> node tools/sim-capture.mjs --out DIR
//     [--specs ordnance,items] [--every 7] [--around 1] [--node PATH] [--jobs N]
//     [--matrix FILE] [--only REGEX] [--no-check] [-- world-hash options]
//
// A capture is three runs of the battle: a scan for the ticks with deaths,
// spawns, boardings and explosions; the capture itself, which samples every
// Nth tick and every tick within --around ticks of one of those; and a
// plain world-hash run. All three must print the same fingerprints, or the
// capture changed the battle and is thrown away (--no-check skips the plain
// run). One battle comes from the world-hash options after --, or many from
// a matrix file of "LABEL options" lines (tools/sim-corpus.txt is the
// standing corpus), --jobs at a time. Each battle leaves DIR/LABEL.bfecap,
// its summary LABEL.bfecap.json and fingerprints LABEL.hash.txt;
// DIR/manifest.json lists them all. The Rust replays read DIR through
// BFE_REPLAY_CORPUS.
//
// Captures hold what the game's files hold (positions, definitions): keep
// them with the game's files, never in a repository. Make them under the
// Node the port takes as its reference (the production server's V8): every
// capture records its Node and V8, and the replays refuse captures from
// another V8.
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const argv = process.argv.slice(2);
const dash = argv.indexOf('--');
const own = dash < 0 ? argv : argv.slice(0, dash), battleArgs = dash < 0 ? [] : argv.slice(dash + 1);
const opt = (name, fallback) => { const i = own.indexOf(`--${name}`); return i < 0 ? fallback : own[i + 1]; };
const out = opt('out', null);
if (!out) { console.error('usage: node tools/sim-capture.mjs --out DIR [options] [-- world-hash options] (see the top of the file)'); process.exit(2); }
const specs = opt('specs', 'ordnance,items'), every = Number(opt('every', 7)), around = Number(opt('around', 1));
const node = opt('node', process.execPath), check = !own.includes('--no-check');
const content = process.env.BFE_CONTENT_DIR || path.join(root, 'public');
fs.mkdirSync(out, { recursive: true });

let battles;
if (opt('matrix', null)) {
  const only = opt('only', null);
  battles = fs.readFileSync(opt('matrix'), 'utf8').split('\n').map((l) => l.replace(/#.*/, '').trim()).filter(Boolean)
    .map((line) => { const [label, ...args] = line.split(/\s+/); return { label, args }; })
    .filter((b) => !only || new RegExp(only).test(b.label));
} else {
  const value = (name, fallback) => { const i = battleArgs.indexOf(`--${name}`); return i < 0 ? fallback : battleArgs[i + 1]; };
  battles = [{ label: opt('label', `${value('map', 'nab2')}-${value('side', 'rep')}-${value('seed', 1)}`), args: battleArgs }];
}
const jobs = Number(opt('jobs', Math.min(battles.length, Math.max(1, Math.floor(os.availableParallelism() / 2)))));

const v8 = JSON.parse(spawnSync(node, ['-p', 'JSON.stringify(process.versions)'], { encoding: 'utf8' }).stdout).v8;
if (!v8.startsWith('13.6.')) console.error(`sim-capture: note: ${node} is V8 ${v8}; the port's reference is V8 13.6 (the production server's Node 24.22)`);

function worldHash(args, stdout, stderr) {
  return new Promise((resolve) => {
    const o = fs.openSync(stdout, 'w'), e = fs.openSync(stderr, 'w');
    const child = spawn(node, ['tools/world-hash.mjs', ...args], { cwd: root, env: { ...process.env, BFE_CONTENT_DIR: content }, stdio: ['ignore', o, e] });
    child.on('close', (code) => { fs.closeSync(o); fs.closeSync(e); resolve(code); });
  });
}
const plugin = ['--plugin', 'tools/sim-capture/plugin.mjs'];

async function capture({ label, args }) {
  const at = (ext) => path.join(out, `${label}.${ext}`);
  const started = Date.now();
  const scanCode = await worldHash([...args, ...plugin, '--capture-scan', at('scan.json'), '--capture-specs', specs], at('scan.hash.txt'), at('scan.err'));
  if (scanCode !== 0) return { label, ok: false, why: `the scan failed (${at('scan.err')})` };
  const scan = JSON.parse(fs.readFileSync(at('scan.json'), 'utf8'));
  const keep = new Set();
  for (const t of scan.ticks) for (let d = -around; d <= around; d++) if (t + d > 0) keep.add(t + d);
  fs.writeFileSync(at('keep.json'), JSON.stringify([...keep].sort((a, b) => a - b)) + '\n');
  const runs = [worldHash([...args, ...plugin, '--capture-out', at('bfecap'), '--capture-specs', specs, '--capture-every', String(every), '--capture-keep', at('keep.json')], at('hash.txt'), at('err'))];
  if (check) runs.push(worldHash(args, at('plain.hash.txt'), at('plain.err')));
  const [code, plainCode] = await Promise.all(runs);
  if (code !== 0) return { label, ok: false, why: `the capture failed (${at('err')})` };
  const mine = fs.readFileSync(at('hash.txt'), 'utf8');
  if (fs.readFileSync(at('scan.hash.txt'), 'utf8') !== mine) return { label, ok: false, why: 'the scan and the capture fingerprinted different battles' };
  if (check) {
    if (plainCode !== 0) return { label, ok: false, why: `the plain run failed (${at('plain.err')})` };
    if (fs.readFileSync(at('plain.hash.txt'), 'utf8') !== mine) return { label, ok: false, why: 'the capture changed the battle: its fingerprints are not a plain run\'s' };
    fs.rmSync(at('plain.hash.txt')); fs.rmSync(at('plain.err'));
  }
  fs.rmSync(at('scan.hash.txt'));
  const summary = JSON.parse(fs.readFileSync(at('bfecap.json'), 'utf8'));
  const ticks = mine.split('\n').filter(Boolean).length;
  let sampled = 0;
  for (let t = 1; t <= ticks; t++) if (t % every === 0 || keep.has(t)) sampled++;
  return { label, ok: true, args, file: `${label}.bfecap`, specs: summary.specs, ticks, events: scan.events.length, sampled,
    frames: summary.frames, bytes: summary.bytes, node: summary.node, v8: summary.v8, commit: summary.commit, checked: check, seconds: (Date.now() - started) / 1000 };
}

const results = [];
let next = 0;
await Promise.all(Array.from({ length: Math.max(1, jobs) }, async () => {
  while (next < battles.length) {
    const battle = battles[next++];
    const result = await capture(battle);
    results.push(result);
    console.log(result.ok ? `${result.label}: ${result.ticks} ticks, ${Object.entries(result.frames).map(([n, c]) => `${c} ${n}`).join(', ') || 'tick records only'}, ${(result.bytes / 1e6).toFixed(1)} MB, ${result.events} events, ${result.seconds.toFixed(0)} s${result.checked ? ', fingerprints as a plain run\'s' : ''}`
      : `${result.label}: FAILED: ${result.why}`);
  }
}));
// The manifest: every capture DIR holds, this run's and earlier ones.
const manifestFile = path.join(out, 'manifest.json');
const previous = fs.existsSync(manifestFile) ? JSON.parse(fs.readFileSync(manifestFile, 'utf8')).captures : [];
const captures = [...previous.filter((c) => !results.some((r) => r.label === c.label)), ...results.filter((r) => r.ok)].sort((a, b) => a.label.localeCompare(b.label));
fs.writeFileSync(manifestFile, JSON.stringify({ format: 1, specs, every, around, captures }, null, 1) + '\n');
const failed = results.filter((r) => !r.ok).length;
console.log(`${results.length - failed} of ${results.length} captured into ${out}${failed ? `; ${failed} failed` : ''}`);
process.exit(failed ? 1 : 0);
