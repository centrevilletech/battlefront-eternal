import test from 'node:test';
import assert from 'node:assert/strict';
import { startLanDiscovery, discoverLan } from '../electron/lan.js';
for (const [host, target] of [['127.0.0.1', '127.0.0.1'], ['::1', '::1']]) test(`LAN discovery and reply over ${target}`, async () => {
  const info = { id: 'a'.repeat(64), name: 'LAN test', port: 18451 };
  const server = await startLanDiscovery({ host, port: 0, getInfo: () => info });
  try {
    const found = await discoverLan({ targets: [target], port: server.port, timeout: 100 });
    assert.deepEqual(found, [{ ...info, address: target }]);
  } finally { await server.close(); }
});
