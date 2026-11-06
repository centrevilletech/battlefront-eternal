// One runner invocation per immutable corpus case. The runner owns application
// launch, CDP port, settings file and GPU log; no machine paths live here.
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readManifest } from './renderer-corpus/manifest.mjs';
const args = process.argv.slice(2);
const arg = (name, fallback) => args.includes(name) ? args[args.indexOf(name) + 1] : fallback;
const manifestFile = arg('--manifest', process.env.BFE_CORPUS_MANIFEST);
if (manifestFile) process.env.BFE_CORPUS_MANIFEST = resolve(manifestFile);
const manifest = readManifest();
const manifestBytes = Buffer.from(JSON.stringify(manifest));
const runner = arg('--runner', process.env.BFE_CORPUS_RUNNER);
if (!runner) throw Error('Supply --runner executable (see docs/renderer-programs.md)');
const out = resolve(arg('--out', 'renderer-corpus-out'));
const dist = resolve(arg('--dist', 'dist-desktop'));
const against = arg('--against');
if (!against) throw Error('Supply --against frozen reference dist');
const reference = resolve(against);
const jobs = Number(arg('--jobs', '1'));
if (!Number.isInteger(jobs) || jobs < 1) throw Error('--jobs must be a positive integer');
const selected = arg('--cases', '').split(',').filter(Boolean);
const rerun = new Set(arg('--rerun', '').split(',').filter(Boolean));
const resume = args.includes('--resume');
const corpusHash = createHash('sha256').update(manifestBytes).digest('hex');
if (resume) {
  const previous = JSON.parse(readFileSync(`${out}/summary.json`));
  if (previous.dist !== dist || previous.reference !== reference || previous.corpusHash !== corpusHash) throw Error('Resume requires the same frozen builds and manifest');
}
const cases = manifest.cases.filter(c => !selected.length || selected.includes(c.id));
if (!cases.length || selected.some(id => !cases.some(c => c.id === id)) || [...rerun].some(id => !cases.some(c => c.id === id))) throw Error('Unknown corpus cases');
mkdirSync(out, { recursive: true });
const results = [];
let next = 0;
async function worker() {
  for (;;) {
    const c = cases[next++]; if (!c) return;
    const rc = await new Promise((done) => {
      if (resume && !rerun.has(c.id)) {
        try {
          const previous = JSON.parse(readFileSync(`${out}/${c.id}.json`));
          if (previous.pass === true && previous.complete === true && previous.valid === true) { done(0); return; }
        } catch {}
      }
      const child = spawn(runner, [dist, c.id, out, reference], { stdio: 'inherit', env: { ...process.env, BFE_CORPUS_CASE: c.id, OUT: out, LABEL: c.id } });
      child.on('error', e => { console.error(e); done(1); });
      child.on('exit', code => done(code ?? 1));
    });
    let report;
    try { report = JSON.parse(readFileSync(`${out}/${c.id}.json`)); } catch { report = { error: 'Missing report' }; }
    results.push({ id: c.id, rc, pass: rc === 0 && report.pass === true && report.complete === true, frameExact: report.frameExact, baselinePass: report.baselinePass, pairs: report.pairs?.length, valid: report.valid, error: report.error });
    console.log(JSON.stringify(results.at(-1)));
  }
}
await Promise.all(Array.from({ length: Math.max(1, Math.min(jobs, cases.length)) }, worker));
results.sort((a,b) => a.id.localeCompare(b.id));
const programs = new Set();
for (const c of cases) { try { const r=JSON.parse(readFileSync(`${out}/${c.id}.json`)); for(const p of r.shaderPrograms || []) if(p.accepted && p.hash) programs.add(p.key); } catch {} }
const missingPrograms = (manifest.programs || []).filter(key => !programs.has(key));
const summary = { missingPrograms, programs: [...programs].sort(), version: manifest.version, corpusHash, dist, reference, complete: cases.length === manifest.cases.length, cases: results, baselinePass: results.every(r=>r.baselinePass), pass: results.length === cases.length && results.every(r => r.pass) && (cases.length < manifest.cases.length || missingPrograms.length === 0) };
writeFileSync(`${out}/summary.json`, JSON.stringify(summary, null, 2));
process.exitCode = summary.pass ? 0 : 1;
