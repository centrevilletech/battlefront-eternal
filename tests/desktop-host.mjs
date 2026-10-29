// Requires prepared .host-runtime and owned local content; never packages assets.
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createHostController } from '../electron/host.js';
import { NETWORK_CONTRACT } from '../src/runtime/network-contract.js';
const stateRoot = mkdtempSync(path.join(tmpdir(), 'bfe-host-test-'));
const runtimeRoot = path.resolve(process.env.BFE_HOST_RUNTIME_ROOT || '.host-runtime');
const settings = { executable: path.resolve(process.env.BFE_ELECTRON_PATH || 'node_modules/electron/dist/electron'), serverRoot: path.join(runtimeRoot, 'server'), engineRoot: path.join(runtimeRoot, 'engine'), contentRoot: () => path.resolve(process.env.BFE_DATA_DIR || 'public'), stateRoot, consent: async () => false };
const host = createHostController(settings);
try {
  await assert.rejects(host('start', { name: 'test', mode: 'public', map: 'nab2', side: 'rep', maxPlayers: 8, relay: 'auto' }), /privacy notice/);
  const info = await host('start', { name: 'Desktop host acceptance', mode: 'lan', map: 'nab2', side: 'rep', maxPlayers: 8, relay: 'never' });
  // The bundled server speaks this engine's contract over native UDP.
  assert.equal(info.simulation, NETWORK_CONTRACT.simulation);
  assert.equal(info.protocol, NETWORK_CONTRACT.protocol);
  assert.equal(info.transport, 'udp');
  assert.match(info.id, /^[a-f0-9]{64}$/); assert.equal(info.address, '127.0.0.1');
  const restartedUI = createHostController(settings);
  assert.equal((await restartedUI('join')).id, info.id, 'a reopened UI discovers the existing server');
  assert.equal((await restartedUI('status')).status, 'running');
  await restartedUI('stop');
  for (let i = 0; i < 50 && (await host('status')).active; i++) await new Promise(resolve => setTimeout(resolve, 100));
  assert.equal((await host('status')).active, false);
  console.log('Detached Electron Node host, reattachment and authenticated stop passed');
} finally {
  if ((await host('status')).active) await host('stop');
  await new Promise(resolve => setTimeout(resolve, 200));
  rmSync(stateRoot, { recursive: true, force: true });
}
