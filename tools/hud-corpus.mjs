// Compare a frozen build and candidate in separate real-GPU battle sessions.
// The external runner receives DIST CASE_ID CASE_OUTPUT REFERENCE_DIST and
// writes reference/control/candidate captures through hud-corpus/capture.mjs.
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { cases, states } from './hud-corpus/manifest.mjs';
import { compareCase } from './renderer-corpus/compare.mjs';
const argv = process.argv.slice(2), option = (name, fallback) => {
  const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1];
};
const runner = option('runner'), dist = option('dist'), reference = option('against'), out = option('out', 'hud-corpus-out');
if (!runner || !dist || !reference) throw Error('--runner, --dist and --against are required');
const vehicles = argv.includes('--vehicles');
const selected = option('cases', '').split(',').filter(Boolean), jobs = Math.max(1, Math.min(12, Number(option('jobs', 1))));
const queue = cases.filter(c => !selected.length || selected.includes(c.id));
if (selected.some(id => !queue.some(c => c.id === id))) throw Error('Unknown HUD case');
fs.mkdirSync(out, { recursive: true });
let next = 0;
const results = [];
async function work() {
  while (next < queue.length) {
    const testCase = queue[next++], directory = path.join(out, testCase.id); fs.mkdirSync(directory, { recursive: true });
    try {
      const code = await new Promise((resolve, reject) => {
        const child = spawn(runner, [dist, testCase.id, directory, reference], { stdio: 'inherit', env: { ...process.env, BFE_HUD_VEHICLES: vehicles ? '1' : '0' } });
        child.on('error', reject); child.on('exit', resolve);
      });
      if (code) throw Error(`Capture runner exited ${code}`);
      const referenceReport = JSON.parse(fs.readFileSync(path.join(directory, 'reference', `${testCase.id}.json`)));
      const expected = states.length + (vehicles ? referenceReport.fixture.vehicleSeats.length : 0);
      for (const role of ['control', 'candidate']) {
        const other = JSON.parse(fs.readFileSync(path.join(directory, role, `${testCase.id}.json`)));
        if (JSON.stringify(other.states) !== JSON.stringify(referenceReport.states)) throw Error('HUD state order differs');
      }
      const result = compareCase(directory, testCase.id, expected); results.push(result);
      fs.writeFileSync(path.join(directory, 'comparison.json'), JSON.stringify(result, null, 2));
      console.log(JSON.stringify({ id: result.id, pass: result.pass, states: result.points.length }));
    } catch (error) { results.push({ id: testCase.id, pass: false, error: String(error.stack || error) }); }
  }
}
await Promise.all(Array.from({ length: jobs }, work));
results.sort((a, b) => a.id.localeCompare(b.id));
const report = { pass: results.length === queue.length && results.every(r => r.pass), complete: results.length === queue.length,
  fullMatrix: queue.length === cases.length, states, cases: results };
fs.writeFileSync(path.join(out, 'report.json'), JSON.stringify(report, null, 2));
if (!report.pass) process.exitCode = 1;
