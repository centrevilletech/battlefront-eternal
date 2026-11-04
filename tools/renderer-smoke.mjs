// Launch the committed renderer in Electron with no prepared game content.
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import electron from 'electron';

const backend = process.argv[2] ?? (process.platform === 'win32' ? 'warp' : 'swiftshader');
if (!['warp', 'gl', 'swiftshader'].includes(backend)) throw new Error('Expected warp, gl or swiftshader');
if (backend === 'warp' && process.platform !== 'win32') throw new Error('WARP requires Windows');
const server = await createServer({ publicDir: false, server: { host: '127.0.0.1', port: 0 }, clearScreen: false });
await server.listen();
const url = server.resolvedUrls.local[0];
try {
  const child = spawn(electron, [...(process.env.BFE_SMOKE_NO_SANDBOX === '1' ? ['--no-sandbox'] : []), fileURLToPath(new URL('../tests/support/renderer-smoke-electron.cjs', import.meta.url)), url, backend], {
    stdio: 'inherit', env: { ...process.env, ELECTRON_RUN_AS_NODE: '' },
  });
  const timer = setTimeout(() => child.kill(), 180_000);
  try {
    process.exitCode = await new Promise((resolve, reject) => { child.once('error', reject); child.once('exit', (code) => resolve(code ?? 1)); });
  } finally { clearTimeout(timer); }
} finally { await server.close(); }
