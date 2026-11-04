// One runner invocation per immutable corpus case. The runner owns application
// launch, CDP port, settings file and GPU log; no machine paths live here.
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
const manifestBytes = readFileSync(new URL('./renderer-corpus/manifest.json', import.meta.url));
const manifest = JSON.parse(manifestBytes);
const args = process.argv.slice(2);
const arg = (name, fallback) => args.includes(name) ? args[args.indexOf(name) + 1] : fallback;
const runner = arg('--runner', process.env.BFE_CORPUS_RUNNER);
if (!runner) throw Error('Supply --runner executable (see docs/renderer-programs.md)');
const out = resolve(arg('--out', 'renderer-corpus-out'));
const dist = resolve(arg('--dist', 'dist-desktop'));
const jobs = Number(arg('--jobs', '1'));
const selected = arg('--cases', '').split(',').filter(Boolean);
const cases = manifest.cases.filter(c => !selected.length || selected.includes(c.id));
if (!cases.length || selected.some(id => !cases.some(c => c.id === id))) throw Error('Unknown corpus cases');
mkdirSync(out, { recursive: true });
const results = [];
let next = 0;
async function worker() {
  for (;;) {
    const c = cases[next++]; if (!c) return;
    const rc = await new Promise((done) => {
      const child = spawn(runner, [dist, c.id, out], { stdio: 'inherit', env: { ...process.env, BFE_CORPUS_CASE: c.id, OUT: out, LABEL: c.id } });
      child.on('error', e => { console.error(e); done(1); });
      child.on('exit', code => done(code ?? 1));
    });
    let report;
    try { report = JSON.parse(readFileSync(`${out}/${c.id}.json`)); } catch { report = { error: 'Missing report' }; }
    results.push({ id: c.id, rc, pass: rc === 0 && report.pass === true, baselinePass: report.baselinePass, valid: report.valid, ownedPixels: report.worstOwnedPixels, threePixels: report.worstRustPixels, controlPct: report.worstControlPct, error: report.error });
    console.log(JSON.stringify(results.at(-1)));
  }
}
await Promise.all(Array.from({ length: Math.max(1, Math.min(jobs, cases.length)) }, worker));
results.sort((a,b) => a.id.localeCompare(b.id));
const summary = { version: manifest.version, corpusHash: createHash('sha256').update(manifestBytes).digest('hex'), dist, complete: cases.length === manifest.cases.length, cases: results, pass: results.every(r => r.pass), baselinePass: results.every(r => r.baselinePass) };
writeFileSync(`${out}/summary.json`, JSON.stringify(summary, null, 2));
process.exitCode = summary.pass ? 0 : 1;
