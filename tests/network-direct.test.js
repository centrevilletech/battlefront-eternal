import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createNetworkBridge } from '../electron/network.js';
for (const host of ['127.0.0.1', '::1', ...(process.platform === 'linux' ? ['::1%lo'] : [])]) test(`desktop direct info uses ${host} without online consent`, async () => {
  const directory = mkdtempSync(path.join(tmpdir(), 'bfe-direct-'));
  const info = { id: 'a'.repeat(64), name: 'Direct test' };
  const server = createServer((request, response) => response.end(JSON.stringify(info)));
  await new Promise(resolve => server.listen(0, host.split('%')[0], resolve));
  const bridge = createNetworkBridge(directory, () => {});
  try {
    const target = { address: host, port: server.address().port };
    assert.deepEqual(await bridge('directInfo', target), info);
    // Gameplay goes straight to the address over native UDP; there is no
    // separate signaling connection to open for a direct join.
    await assert.rejects(bridge('directConnect', target), /Unknown network action/);
    assert.equal((await bridge('notice')).accepted, false);
  } finally {
    await bridge('close');
    await new Promise(resolve => server.close(resolve));
    rmSync(directory, { recursive: true, force: true });
  }
});
