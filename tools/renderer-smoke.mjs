// Launch the committed renderer in Electron with no prepared game content.
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import electron from 'electron';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const backend = process.argv[2] ?? (process.platform === 'win32' ? 'warp' : 'swiftshader');
if (!['warp', 'gl', 'swiftshader'].includes(backend)) throw new Error('Expected warp, gl or swiftshader');
if (backend === 'warp' && process.platform !== 'win32') throw new Error('WARP requires Windows');
const server = await createServer({ publicDir: false, server: { host: '127.0.0.1', port: 0 }, clearScreen: false });
await server.listen();
const url = server.resolvedUrls.local[0];
// Electron treats a present ELECTRON_RUN_AS_NODE as enabled on Windows,
// even when its value is empty. Remove the key from the child environment.
const reportDir = await mkdtemp(join(tmpdir(), 'renderer-smoke-'));
const reportPath = join(reportDir, 'report.jsonl');
const childEnv = { ...process.env, BFE_SMOKE_REPORT: reportPath };
for (const key of Object.keys(childEnv)) if (key.toUpperCase() === 'ELECTRON_RUN_AS_NODE') delete childEnv[key];
console.log(`Starting Electron for ANGLE ${backend}`);
try {
  const child = spawn(electron, [...(process.env.BFE_SMOKE_NO_SANDBOX === '1' ? ['--no-sandbox'] : []), fileURLToPath(new URL('../tests/support/renderer-smoke-electron.cjs', import.meta.url)), url, backend], {
    stdio: ['ignore', 'pipe', 'pipe'], env: childEnv,
  });
  child.stdout.on('data', (data) => process.stdout.write(data));
  child.stderr.on('data', (data) => process.stderr.write(data));
  const timer = setTimeout(() => child.kill(), 180_000);
  try {
    process.exitCode = await new Promise((resolve, reject) => { child.once('error', reject); child.once('exit', (code) => resolve(code ?? 1)); });
  } finally { clearTimeout(timer); }
} finally {
  try { console.log(await readFile(reportPath, 'utf8')); }
  catch { console.error('Electron produced no smoke report'); process.exitCode = 1; }
  await rm(reportDir, { recursive: true, force: true });
  await server.close();
}
