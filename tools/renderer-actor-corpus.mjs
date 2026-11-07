// One runner invocation per immutable corpus case. The runner owns application
// launch, CDP port, settings file and GPU log; no machine paths live here.
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
const args = process.argv.slice(2);
const arg = (name, fallback) => args.includes(name) ? args[args.indexOf(name) + 1] : fallback;
const manifestPath = resolve(arg('--manifest', fileURLToPath(new URL('./renderer-corpus/actors-manifest.json', import.meta.url))));
const manifestBytes = readFileSync(manifestPath);
const manifest = JSON.parse(manifestBytes);
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
// --cases limits a run to named case ids (a retry of failures); unknown ids are an error.
const cases = manifest.cases.filter(c => !selected.length || selected.includes(c.id));
if (!cases.length || selected.some(id => !cases.some(c => c.id === id))) throw Error('Unknown corpus cases');
mkdirSync(out, { recursive: true });
const results = [];
let next = 0;
// Each worker takes the next case and runs the supplied runner once for it; the
// runner owns the application, CDP port and GPU. A case passes on exit 0 and a report that says so.
async function worker() {
  for (;;) {
    const c = cases[next++]; if (!c) return;
    const rc = await new Promise((done) => {
      const child = spawn(runner, [dist, c.id, out, reference], { stdio: 'inherit', env: { ...process.env, BFE_CORPUS_MANIFEST: manifestPath, BFE_CORPUS_CASE: c.id, OUT: out, LABEL: c.id } });
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
const programs = new Set(), replayPrograms = new Set(), visibleReplayPrograms = new Set(), replayCoverage = {};
for(const c of cases){try{
 const r=JSON.parse(readFileSync(`${out}/${c.id}.json`));
 for(const p of r.shaderPrograms || [])if(p.accepted && p.hash)programs.add(p.key);
 for(const pair of r.pairs || [])for(const key of pair.variants || [pair.key]){
  replayPrograms.add(key);
  const coverage=replayCoverage[key] ||= {pairs:0,draws:0,covered:0,exact:true};
  coverage.pairs++;coverage.draws+=pair.draws || 0;coverage.covered+=pair.covered || 0;coverage.exact &&= pair.differing===0;
  if(pair.draws>0 && pair.covered>0)visibleReplayPrograms.add(key);
 }
}catch{}}

const missingPrograms = (manifest.programs || []).filter(key => !programs.has(key));
const summary = { replayCoverage, visibleReplayPrograms:[...visibleReplayPrograms].sort(), unreplayedPrograms:(manifest.programs || []).filter(key=>!replayPrograms.has(key)), replayPrograms: [...replayPrograms].sort(), missingPrograms, programs: [...programs].sort(), version: manifest.version, corpusHash: createHash('sha256').update(manifestBytes).digest('hex'), dist, reference, complete: cases.length === manifest.cases.length, cases: results, baselinePass: results.every(r=>r.baselinePass), pass: results.length === cases.length && results.every(r => r.pass) && (cases.length < manifest.cases.length || missingPrograms.length === 0) };
writeFileSync(`${out}/summary.json`, JSON.stringify(summary, null, 2));
process.exitCode = summary.pass ? 0 : 1;
