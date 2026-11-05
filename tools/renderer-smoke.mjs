// Launch the committed renderer in Electron with no prepared game content.
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import electron from 'electron';
import { readShaderManifests } from './renderer-smoke-manifests.mjs';
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const backend = process.argv[2] ?? (process.platform === 'win32' ? 'warp' : 'swiftshader');
if (!['warp', 'gl', 'swiftshader'].includes(backend)) throw new Error('Expected warp, gl or swiftshader');
if (backend === 'warp' && process.platform !== 'win32') throw new Error('WARP requires Windows');
const manifestCatalog = await readShaderManifests();
const server = await createServer({ publicDir: false, server: { host: '127.0.0.1', port: Number(process.env.BFE_SMOKE_PORT || 18450), strictPort: false, hmr: false }, clearScreen: false });
await server.listen();
const url = server.resolvedUrls.local[0];
// Electron treats a present ELECTRON_RUN_AS_NODE as enabled on Windows,
// even when its value is empty. Remove the key from the child environment.
const reportDir = await mkdtemp(join(tmpdir(), 'renderer-smoke-'));
const reportPath = join(reportDir, 'report.jsonl');
const manifestsPath = join(reportDir, 'manifests.json');
await writeFile(manifestsPath, JSON.stringify(manifestCatalog));
const childEnv = { ...process.env, BFE_SMOKE_REPORT: reportPath, BFE_SMOKE_MANIFESTS: manifestsPath, BFE_SMOKE_URL: url, BFE_SMOKE_BACKEND: backend };
for (const key of Object.keys(childEnv)) if (key.toUpperCase() === 'ELECTRON_RUN_AS_NODE') delete childEnv[key];
console.log(`Starting Electron for ANGLE ${backend}`);
try {
  const child = spawn(electron, [...(process.env.BFE_SMOKE_NO_SANDBOX === '1' ? ['--no-sandbox'] : []), fileURLToPath(new URL('../tests/support/renderer-smoke-electron.cjs', import.meta.url))], {
    stdio: ['ignore', 'pipe', 'pipe'], env: childEnv,
  });
  child.stdout.on('data', (data) => process.stdout.write(data));
  child.stderr.on('data', (data) => process.stderr.write(data));
  const timer = setTimeout(() => {console.error(`Renderer smoke ${backend} exceeded the twelve-minute driver budget`);child.kill();}, 12 * 60_000);
  try {
    process.exitCode = await new Promise((resolve, reject) => { child.once('error', reject); child.once('exit', (code, signal) => { console.log(`Electron exited: code=${code}, signal=${signal}`); resolve(code ?? 1); }); });
  } finally { clearTimeout(timer); }
} finally {
  try {
    const report=await readFile(reportPath,'utf8');console.log(report);
    if(process.env.BFE_SMOKE_OUTPUT)await writeFile(process.env.BFE_SMOKE_OUTPUT,report);
  } catch(error) {console.error(`Cannot read or save smoke report: ${error.message}`);process.exitCode=1;}
  await rm(reportDir, { recursive: true, force: true });
  await server.close();
}
