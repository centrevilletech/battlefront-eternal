// The whole-build pixel gate: one command that compares a candidate desktop
// build with a frozen reference build on real GPUs, over every map at
// Classic, Balanced and High, on foot, in vehicles (chase view and cockpit),
// with the HUD, and on the front end's own screens.
//
// Fresh whole-game sessions are not pixel identical (start-up timing), so
// each case records the reference's renderer inputs once and replays them in
// the reference, a repeat of the reference and the candidate, comparing RGBA
// exactly (tools/build-gate/compare.mjs). The finished frames are compared
// as well, against the repeat reference's own run-to-run noise, which is the
// only comparison a build with no stage interface (a three.js-only build) allows.
//
//   node tools/build-gate.mjs --reference DIST --candidate DIST --out DIR
//        [--kind foot,vehicle,shell] [--map nab2,hot1] [--preset classic,high]
//        [--cases id,id] [--jobs N] [--wrap "COMMAND PREFIX"] [--node PATH]
//        [--stages auto|on|off] [--keep-frames] [--list]
//
// --wrap is the command that runs one case where the GPU is (a farm job
// runner, a laptop wrapper); each case is `<wrap> <node> tools/build-gate/run-case.mjs ...`
// and at most --jobs of them run at once. Without --wrap the cases run here.
// run-case.mjs documents the BFE_GATE_START / BFE_GATE_STOP commands that
// launch the application. Exit status 1 if any case fails or is invalid.
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { gateCases } from './build-gate/cases.mjs';

const args = process.argv.slice(2), arg = (name, fallback) => args.includes(name) ? args[args.indexOf(name) + 1] : fallback;
const list = (name) => (arg(name, '') || '').split(',').filter(Boolean);
let cases = gateCases();
for (const [flag, key] of [['--kind', 'kind'], ['--map', 'map'], ['--preset', 'preset'], ['--cases', 'id']]) {
  const wanted = list(flag);
  if (wanted.length) cases = cases.filter(c => wanted.includes(c[key]));
}
if (args.includes('--list')) { for (const c of cases) console.log(c.id); process.exit(0); }
const reference = arg('--reference'), candidate = arg('--candidate'), outArg = arg('--out');
if (!reference || !candidate || !outArg) throw Error('Supply --reference, --candidate and --out');
if (!cases.length) throw Error('No case matches');
const out = path.resolve(outArg), jobs = Number(arg('--jobs', '1')), wrap = (arg('--wrap', '') || '').split(/\s+/).filter(Boolean);
if (!Number.isInteger(jobs) || jobs < 1) throw Error('--jobs must be a positive integer');
const node = arg('--node', 'node'), stages = arg('--stages', 'auto');
fs.mkdirSync(out, { recursive: true });

const results = [];
let next = 0;
async function worker() {
  for (;;) {
    const c = cases[next++];
    if (!c) return;
    const command = [...wrap, node, 'tools/build-gate/run-case.mjs', '--reference', path.resolve(reference), '--candidate', path.resolve(candidate), '--case', c.id, '--out', out, '--stages', stages, ...(args.includes('--keep-frames') ? ['--keep-frames'] : [])];
    const rc = await new Promise(done => {
      const child = spawn(command[0], command.slice(1), { stdio: ['ignore', 'inherit', 'inherit'] });
      child.on('error', e => { console.error(e); done(1); });
      child.on('exit', code => done(code ?? 1));
    });
    let report;
    try { report = JSON.parse(fs.readFileSync(path.join(out, `${c.id}.json`))); }
    catch { report = { id: c.id, status: 'invalid', error: `no result (runner exit ${rc})` }; }
    results.push(report);
    console.log(`${report.status.padEnd(10)} ${c.id}${report.frame ? ` frame ${report.frame.verdict}` : ''}${report.error ? ` ${report.error}` : ''}${report.reason ? ` (${report.reason})` : ''}`);
  }
}
await Promise.all(Array.from({ length: Math.min(jobs, cases.length) }, worker));
results.sort((a, b) => a.id.localeCompare(b.id));
const count = (status) => results.filter(r => r.status === status).length;
const summary = { reference: path.resolve(reference), candidate: path.resolve(candidate), cases: results.length, pass: count('pass'), fail: count('fail'), frameOnly: count('frame-only'), skipped: count('skipped'), invalid: count('invalid'),
  complete: results.length === gateCases().length, results };
fs.writeFileSync(path.join(out, 'summary.json'), JSON.stringify(summary, null, 2));
const sum = (r, key) => r.frames ? r.frames.reduce((n, f) => n + (f[key]?.differing || 0), 0) : '-';
const vis = (r, key) => r.frames ? r.frames.reduce((n, f) => n + (f[key]?.visible || 0), 0) : '-';
const rows = results.map(r => `| ${r.id} | ${r.status} | ${r.stages?.available ? (r.stages.exact ? 'exact' : 'DIFFERS') : '-'} | ${r.frame ? r.frame.verdict : '-'} | ${sum(r, 'repeat')} / ${vis(r, 'repeat')} | ${sum(r, 'candidate')} / ${vis(r, 'candidate')} | ${r.frame ? (r.frame.stateAgrees ? 'same' : 'differs') : '-'} |`);
fs.writeFileSync(path.join(out, 'summary.md'), `| case | status | stage replay | finished frame | repeat run px / visible | candidate px / visible | world state |\n|---|---|---|---|---|---|---|\n${rows.join('\n')}\n`);
console.log(`${summary.cases} cases: ${summary.pass} pass, ${summary.fail} fail, ${summary.frameOnly} frame-only, ${summary.skipped} skipped, ${summary.invalid} invalid${summary.complete ? '' : ' (a subset)'}`);
process.exitCode = summary.fail || summary.invalid ? 1 : 0;
