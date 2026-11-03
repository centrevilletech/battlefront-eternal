// The standing world-hash gate in one command: fingerprints every battle of
// tools/world-hash-matrix.txt with tools/world-hash.mjs, in each variant
// asked for (the JavaScript path first, as the reference, then the ones a
// change must match), several at a time, and compares them tick by tick on
// all three hashes (replay state, deep digest, published bytes).
//
//   BFE_CONTENT_DIR=<game-data> node tools/world-hash-matrix.mjs --out DIR
//     [--matrix FILE] [--only REGEX] [--variants js,mv] [--variant NAME=OPTIONS]
//     [--jobs N] [--cpus 0-3,8] [--node PATH] [--shard K/N] [--resume]
//     [--against DIR[:VARIANT]] [--compare-only] [--any-v8]
//
// Variants are world-hash options added to every battle's: js (none: the
// collision's and movement's JavaScript, the rounds on the module), rays
// (--rays wasm), mv (--movement wasm, which brings the ray kernel), simjs
// and mvsimjs (js and mv with the rounds JavaScript's, --sim js), or any
// NAME=OPTIONS given with --variant (as a port adds its switch). The first
// is the reference the others are compared with.
//
// Each run of a battle leaves DIR/LABEL.VARIANT.txt (its fingerprints),
// .err (its log) and .json (exit status, ticks, wall time, world step, and
// the Node and V8 that ran it and the commit). --against compares every
// variant with another run's (its same variant, or the one named), say a
// reference recorded before a change; runs made under different V8
// versions are never compared (their Math functions differ: the reference
// is the V8 the production server runs), unless --any-v8 says so.
//
// --node runs the battles under another Node than this one. --cpus pins
// each battle to one of the CPUs listed (as many at once as CPUs, unless
// --jobs says fewer). --shard K/N runs only the K-th of N shares of the work
// (balanced by ticks), so several machines or slots can share one DIR;
// --compare-only then compares what DIR holds. --resume keeps the battles
// DIR already holds whole. The exit status is 0 when every battle ran whole
// and every comparison is the same.
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const opts = (name) => argv.flatMap((a, i) => (a === `--${name}` ? [argv[i + 1]] : []));
const out = opt('out', null);
if (!out) { console.error('usage: node tools/world-hash-matrix.mjs --out DIR [options] (see the top of the file)'); process.exit(2); }

const BUILTIN_VARIANTS = { js: [], rays: ['--rays', 'wasm'], mv: ['--movement', 'wasm'], wasm: ['--movement', 'wasm'], simjs: ['--sim', 'js'], mvsimjs: ['--movement', 'wasm', '--sim', 'js'],
  combat: ['--combat', 'rust'], units: ['--units', 'rust'], unitscombat: ['--units', 'rust', '--combat', 'rust'] };
const custom = Object.fromEntries(opts('variant').map((v) => { const at = v.indexOf('='); return [v.slice(0, at), v.slice(at + 1).split(/\s+/).filter(Boolean)]; }));
const variantNames = String(opt('variants', 'js,mv')).split(',').filter(Boolean);
const variants = variantNames.map((name) => {
  const args = custom[name] ?? BUILTIN_VARIANTS[name];
  if (!args) throw Error(`unknown variant ${name}: one of ${Object.keys(BUILTIN_VARIANTS).join(', ')}, or define it with --variant ${name}=OPTIONS`);
  return { name, args };
});

// The battles: "LABEL options" lines, # for comments.
function readMatrix(file) {
  return fs.readFileSync(file, 'utf8').split('\n').map((line) => line.replace(/#.*/, '').trim()).filter(Boolean).map((line) => {
    const [label, ...args] = line.split(/\s+/);
    const value = (name, fallback) => { const i = args.indexOf(`--${name}`); return i < 0 ? fallback : args[i + 1]; };
    const battles = args.includes('--twice') ? 2 : Number(value('battles', 1));
    return { label, args, ticks: Number(value('ticks', 2400)) * battles, battles };
  });
}
const matrixFile = path.resolve(opt('matrix', path.join(root, 'tools/world-hash-matrix.txt')));
const only = opt('only', null);
const configs = readMatrix(matrixFile).filter((c) => !only || new RegExp(only).test(c.label));
if (!configs.length) throw Error(`no battle in ${matrixFile}${only ? ` matches ${only}` : ''}`);
const labels = new Set();
for (const c of configs) { if (labels.has(c.label)) throw Error(`${c.label} twice in ${matrixFile}`); labels.add(c.label); }

function cpuList(text) {
  return text.split(',').flatMap((part) => {
    const [a, b = a] = part.split('-').map(Number);
    if (!Number.isInteger(a) || !Number.isInteger(b) || b < a) throw Error(`--cpus ${text}`);
    return Array.from({ length: b - a + 1 }, (_, i) => a + i);
  });
}
const cpus = opt('cpus', null) === null ? null : cpuList(opt('cpus'));
const jobs = Number(opt('jobs', cpus ? cpus.length : Math.max(1, Math.floor(os.availableParallelism() / 2))));
const node = opt('node', process.execPath);
const content = process.env.BFE_CONTENT_DIR || path.join(root, 'public');
fs.mkdirSync(out, { recursive: true });

// What ran the battles: a comparison across V8 versions means nothing.
function runtimeOf(binary) {
  const r = spawnSync(binary, ['-p', 'JSON.stringify({ node: process.version, v8: process.versions.v8 })'], { encoding: 'utf8' });
  if (r.status !== 0) throw Error(`${binary} does not run: ${r.stderr}`);
  return JSON.parse(r.stdout);
}
// (BFE_COMMIT names it where the checkout's history is not at hand.)
function commitOf() {
  if (process.env.BFE_COMMIT) return process.env.BFE_COMMIT;
  const head = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim() || null;
  const dirty = spawnSync('git', ['status', '--porcelain', '--', 'src', 'tools', 'rust'], { cwd: root, encoding: 'utf8' }).stdout.trim();
  return head && dirty ? `${head}+changes` : head;
}

const file = (label, variant, ext, dir = out) => path.join(dir, `${label}.${variant}.${ext}`);
const readJson = (f) => { try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return null; } };
const linesOf = (f) => { try { return fs.readFileSync(f, 'utf8').split('\n').filter(Boolean); } catch { return null; } };
function whole(config, variant, dir = out) {
  const meta = readJson(file(config.label, variant, 'json', dir));
  return meta?.exit === 0 && meta.ticks === config.ticks ? meta : null;
}

// The work, longest battles first, a battle's variants side by side; a
// shard takes the battles a greedy split by ticks gives it.
let tasks = configs.flatMap((config) => variants.map((variant) => ({ config, variant })));
tasks.sort((a, b) => b.config.ticks - a.config.ticks || configs.indexOf(a.config) - configs.indexOf(b.config));
const shard = opt('shard', null);
if (shard) {
  const [k, n] = shard.split('/').map(Number);
  if (!(n >= 1 && k >= 1 && k <= n)) throw Error(`--shard ${shard}: K/N with 1 <= K <= N`);
  const load = new Array(n).fill(0), mine = new Set();
  for (const config of [...configs].sort((a, b) => b.ticks - a.ticks || configs.indexOf(a) - configs.indexOf(b))) {
    const s = load.indexOf(Math.min(...load));
    load[s] += config.ticks;
    if (s === k - 1) mine.add(config);
  }
  tasks = tasks.filter((t) => mine.has(t.config));
}
if (argv.includes('--resume')) tasks = tasks.filter((t) => !whole(t.config, t.variant.name));
if (argv.includes('--compare-only')) tasks = [];

const runtime = runtimeOf(node), commit = commitOf();
function run(task, cpu) {
  const { config, variant } = task;
  const args = ['tools/world-hash.mjs', ...config.args, ...variant.args];
  const [command, commandArgs] = cpu === null ? [node, args] : ['taskset', ['-c', String(cpu), node, ...args]];
  const started = Date.now();
  return new Promise((resolve) => {
    const stdout = fs.openSync(file(config.label, variant.name, 'txt'), 'w'), stderr = fs.openSync(file(config.label, variant.name, 'err'), 'w');
    const child = spawn(command, commandArgs, { cwd: root, env: { ...process.env, BFE_CONTENT_DIR: content }, stdio: ['ignore', stdout, stderr] });
    const done = (exit) => {
      fs.closeSync(stdout); fs.closeSync(stderr);
      const ticks = linesOf(file(config.label, variant.name, 'txt'))?.length ?? 0;
      const log = fs.readFileSync(file(config.label, variant.name, 'err'), 'utf8');
      const steps = [...log.matchAll(/world step ([0-9.]+) ms/g)].map((m) => Number(m[1]));
      const meta = { label: config.label, variant: variant.name, args, exit, ticks, expected: config.ticks, wallSeconds: (Date.now() - started) / 1000,
        stepMs: steps.length ? steps.reduce((a, b) => a + b, 0) / steps.length : null, cpu, ...runtime, commit, finished: new Date().toISOString() };
      fs.writeFileSync(file(config.label, variant.name, 'json'), JSON.stringify(meta, null, 1) + '\n');
      resolve(meta);
    };
    child.on('error', () => done(-1));
    child.on('close', (code, signal) => done(code ?? (signal ? `signal ${signal}` : -1)));
  });
}

// Tick by tick: the first line that differs, and in which hashes.
const COLUMNS = ['state', 'digest', 'published'];
function compareLines(a, b) {
  const n = Math.max(a.length, b.length);
  for (let i = 0; i < n; i++) {
    if (a[i] === b[i]) continue;
    if (a[i] === undefined || b[i] === undefined) return { line: i + 1, tick: (a[i] ?? b[i]).split(' ')[0], columns: ['length'], a: a[i] ?? null, b: b[i] ?? null };
    const x = a[i].split(' '), y = b[i].split(' ');
    return { line: i + 1, tick: x[0], columns: COLUMNS.filter((_, k) => x[k + 1] !== y[k + 1]), a: a[i], b: b[i] };
  }
  return null;
}

const finished = [];
let next = 0, active = 0, startedAll = Date.now();
const free = cpus ? [...cpus] : null;
await new Promise((resolve) => {
  if (!tasks.length) return resolve();
  const pump = () => {
    while (active < jobs && next < tasks.length) {
      const task = tasks[next++], cpu = free ? free.shift() : null;
      active++;
      run(task, cpu).then((meta) => {
        active--; if (free) free.push(cpu);
        finished.push(meta);
        const ok = meta.exit === 0 && meta.ticks === meta.expected;
        console.log(`[${finished.length}/${tasks.length}] ${meta.label}.${meta.variant} ${ok ? `${meta.ticks} ticks` : `FAILED (exit ${meta.exit}, ${meta.ticks} of ${meta.expected} ticks)`} in ${meta.wallSeconds.toFixed(0)} s${meta.stepMs === null ? '' : `, world step ${meta.stepMs.toFixed(2)} ms`}`);
        if (next >= tasks.length && active === 0) resolve(); else pump();
      });
    }
  };
  pump();
});

// The comparisons, over everything DIR holds for these battles.
const against = opt('against', null);
const [againstDir, againstVariant = null] = against ? against.split(':') : [null];
const anyV8 = argv.includes('--any-v8');
const rows = [];
let failures = 0;
for (const config of configs) {
  const row = { label: config.label, args: config.args.join(' '), ticks: config.ticks, variants: {}, comparisons: [] };
  for (const variant of variants) {
    const meta = readJson(file(config.label, variant.name, 'json'));
    row.variants[variant.name] = meta ? { exit: meta.exit, ticks: meta.ticks, stepMs: meta.stepMs, v8: meta.v8, node: meta.node, commit: meta.commit } : null;
    if (!whole(config, variant.name)) { failures++; row.comparisons.push({ a: variant.name, verdict: meta ? `incomplete (exit ${meta.exit}, ${meta.ticks} of ${config.ticks} ticks)` : 'missing' }); }
  }
  const reference = variants[0].name, refLines = whole(config, reference) ? linesOf(file(config.label, reference, 'txt')) : null;
  for (const variant of variants.slice(1)) {
    if (!refLines || !whole(config, variant.name)) continue;
    const a = readJson(file(config.label, reference, 'json')), b = readJson(file(config.label, variant.name, 'json'));
    if (a.v8 !== b.v8 && !anyV8) { failures++; row.comparisons.push({ a: reference, b: variant.name, verdict: `not compared: V8 ${a.v8} against ${b.v8}` }); continue; }
    const diff = compareLines(refLines, linesOf(file(config.label, variant.name, 'txt')));
    if (diff) failures++;
    row.comparisons.push({ a: reference, b: variant.name, verdict: diff ? 'DIFFERENT' : 'same', diff });
  }
  if (againstDir) {
    for (const variant of variants) {
      const theirs = againstVariant ?? variant.name;
      if (!whole(config, variant.name)) continue;
      const otherMeta = whole(config, theirs, againstDir);
      const label = `${variant.name} vs ${path.basename(path.resolve(againstDir))}:${theirs}`;
      if (!otherMeta) { failures++; row.comparisons.push({ a: variant.name, b: label, verdict: 'missing in the other run' }); continue; }
      const mine = readJson(file(config.label, variant.name, 'json'));
      if (mine.v8 !== otherMeta.v8 && !anyV8) { failures++; row.comparisons.push({ a: variant.name, b: label, verdict: `not compared: V8 ${mine.v8} against ${otherMeta.v8}` }); continue; }
      const diff = compareLines(linesOf(file(config.label, variant.name, 'txt')), linesOf(file(config.label, theirs, 'txt', againstDir)));
      if (diff) failures++;
      row.comparisons.push({ a: variant.name, b: label, verdict: diff ? 'DIFFERENT' : 'same', diff });
    }
  }
  // Several battles in one process: does a later one part from the first?
  // (What a module keeps between battles; reported, not a failure.)
  if (config.battles > 1 && refLines) {
    // (A later battle's lines carry its number: "2:1 ...".)
    const each = config.ticks / config.battles, first = refLines.slice(0, each);
    row.repeats = [];
    for (let k = 1; k < config.battles; k++) {
      const diff = compareLines(first, refLines.slice(k * each, (k + 1) * each).map((line) => line.replace(/^\d+:/, '')));
      row.repeats.push(diff ? `battle ${k + 1} parts from the first at its tick ${diff.tick} (${diff.columns.join(', ')})` : `battle ${k + 1} the same as the first`);
    }
  }
  rows.push(row);
}

const v8s = [...new Set(rows.flatMap((r) => Object.values(r.variants).filter(Boolean).map((v) => `${v.node} (V8 ${v.v8})`)))];
const commits = [...new Set(rows.flatMap((r) => Object.values(r.variants).filter(Boolean).map((v) => v.commit)))];
const ticksPerVariant = configs.reduce((n, c) => n + c.ticks, 0);
const table = [
  `World-hash matrix: ${configs.length} battles, ${ticksPerVariant.toLocaleString('en-US')} ticks a variant, variants ${variants.map((v) => `${v.name}${v.args.length ? ` (${v.args.join(' ')})` : ''}`).join(', ')}${againstDir ? `, against ${againstDir}` : ''}.`,
  `Ran under ${v8s.join(', ') || 'nothing yet'}; commit ${commits.join(', ') || 'none'}.`,
  '',
  `| Battle | Ticks | ${variants.map((v) => `${v.name} step ms`).join(' | ')} | Verdict |`,
  `| --- | ---: | ${variants.map(() => '---:').join(' | ')} | --- |`,
  ...rows.map((r) => `| ${r.label} | ${r.ticks} | ${variants.map((v) => r.variants[v.name]?.stepMs?.toFixed(2) ?? '-').join(' | ')} | ${[
    ...r.comparisons.map((c) => (c.b ? `${c.b.includes(' vs ') ? c.b : `${c.a}=${c.b}`}: ${c.verdict}${c.diff ? ` at tick ${c.diff.tick} (${c.diff.columns.join(', ')})` : ''}` : `${c.a}: ${c.verdict}`)),
    ...(r.repeats ?? [])].join('; ') || '-'} |`),
  '',
  failures ? `${failures} failing (different, incomplete or not comparable).` : `All the same: ${rows.length} battles${variants.length > 1 || againstDir ? '' : ' (one variant, nothing compared)'}.`,
];
fs.writeFileSync(path.join(out, 'summary.md'), table.join('\n') + '\n');
fs.writeFileSync(path.join(out, 'summary.json'), JSON.stringify({ matrix: path.relative(root, matrixFile), variants, against, runtimes: v8s, commits, ticksPerVariant, wallSeconds: (Date.now() - startedAll) / 1000, failures, rows }, null, 1) + '\n');
console.log(table.join('\n'));
process.exit(failures ? 1 : 0);
