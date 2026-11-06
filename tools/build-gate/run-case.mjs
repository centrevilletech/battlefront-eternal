// One case of the whole-build gate, on one machine with one display: the
// reference records, a repeat of the reference and the candidate replay, then
// the comparison. The application is started and stopped by two commands the
// caller supplies, so the same case runs on a farm slot and on a laptop:
//   BFE_GATE_START  shell command given the dist as $1; returns once the app answers on CDP_PORT
//   BFE_GATE_STOP   shell command run after each session
// Usage: node tools/build-gate/run-case.mjs --reference DIST --candidate DIST --case ID --out DIR
//        [--raw DIR] [--stages auto|on|off] [--keep-frames]
import fs from 'node:fs';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { compareCase, removeRaw } from './compare.mjs';

const args = process.argv.slice(2), arg = (name, fallback) => args.includes(name) ? args[args.indexOf(name) + 1] : fallback;
const reference = arg('--reference'), candidate = arg('--candidate'), id = arg('--case'), outRoot = arg('--out');
if (!reference || !candidate || !id || !outRoot) throw Error('Supply --reference, --candidate, --case and --out');
const out = path.resolve(outRoot, id), raw = path.resolve(arg('--raw', process.env.BFE_GATE_RAW || path.join(outRoot, '.raw')), id);
const hasRenderer = (dist) => fs.existsSync(path.join(dist, 'assets')) && fs.readdirSync(path.join(dist, 'assets')).some(f => /^bfe_render_bg-.*\.wasm$/.test(f));
const mode = arg('--stages', 'auto');
const stages = mode === 'on' || mode === 'auto' && hasRenderer(reference) && hasRenderer(candidate);
fs.rmSync(out, { recursive: true, force: true }); removeRaw(raw);
fs.mkdirSync(out, { recursive: true }); fs.mkdirSync(raw, { recursive: true });
const here = path.dirname(fileURLToPath(import.meta.url));
const sh = (command, extra = []) => command && spawnSync('sh', ['-c', command, '_', ...extra], { stdio: 'inherit' });

// A session that failed to load (a slow start can time out) gets one more try
// with the same inputs; a case that is not applicable is a result, not a failure.
async function attempt(role, dist) {
  let code = await session(role, dist);
  const file = `${out}/${role}/capture.json`;
  const done = () => fs.existsSync(file) && (JSON.parse(fs.readFileSync(file)).valid || JSON.parse(fs.readFileSync(file)).notApplicable);
  if (code && !done()) { console.error(`${role}: retrying once`); code = await session(role, dist); }
  return code;
}

function session(role, dist) {
  const start = sh(process.env.BFE_GATE_START, [dist]);
  if (start && start.status) return Promise.resolve(1);
  const env = { ...process.env, BFE_GATE_CASE: id, BFE_GATE_ROLE: role, OUT: out, BFE_GATE_RAW: raw, BFE_GATE_STAGES: stages ? '1' : '0',
    EMU_SCREEN: process.env.EMU_SCREEN || '1600x900x24', WARM: process.env.WARM || '12', RENDER_WARM: process.env.RENDER_WARM || '3' };
  return new Promise(done => {
    const child = spawn(process.execPath, [path.join(here, 'capture.mjs')], { stdio: 'inherit', env });
    const timer = setTimeout(() => child.kill('SIGTERM'), 15 * 60 * 1000);
    child.on('error', e => { console.error(e); done(1); });
    child.on('exit', code => { clearTimeout(timer); done(code ?? 1); });
  }).finally(() => sh(process.env.BFE_GATE_STOP));
}

let report;
try {
  const rc = { reference: await attempt('reference', reference) };
  const recorded = fs.existsSync(`${out}/reference/capture.json`) && JSON.parse(fs.readFileSync(`${out}/reference/capture.json`));
  if (recorded?.valid && !recorded.notApplicable) {
    rc.repeat = await attempt('repeat', reference);
    rc.candidate = await attempt('candidate', candidate);
  }
  report = compareCase(out, raw, id, { keepFrames: args.includes('--keep-frames') });
  report.exitCodes = rc; report.stagesRequested = stages;
} catch (error) {
  report = { id, status: 'invalid', error: String(error.stack || error) };
} finally { removeRaw(raw); }
fs.writeFileSync(`${out}.json`, JSON.stringify(report, null, 2));
console.log(JSON.stringify({ id, status: report.status, stages: report.stages?.exact, frameExact: report.frame?.exact, residual: report.frame?.residualPixels }));
