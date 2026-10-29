// What the desktop gives the server browser beyond the master's listing: a
// ping to each server, and only the battle's own keys of a server's query.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:net';
import { measureLatency, withLatency } from '../electron/network.js';
import { battleParams } from '../src/runtime/battle-query.js';

test('the ping to a server is a TCP handshake with its game port, the lowest over its addresses', async t => {
  const server = createServer(socket => socket.destroy());
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => server.close());
  const port = server.address().port;
  const latency = await measureLatency('127.0.0.1', port);
  assert.ok(Number.isInteger(latency) && latency >= 1 && latency < 1000, `answered in ${latency} ms`);
  const closed = createServer(); await new Promise(resolve => closed.listen(0, '127.0.0.1', resolve)); const gone = closed.address().port; await new Promise(resolve => closed.close(resolve));
  assert.equal(await measureLatency('127.0.0.1', gone), null, 'nothing listening');
  assert.equal(await measureLatency('not an address', port), null);
  const listed = [
    { name: 'both', endpoints: [{ address: '8.8.8.8', port: 1 }, { address: '2001:db8::1', port: 1 }] },
    { name: 'private', endpoints: [] },
    { name: 'silent', endpoints: [{ address: '8.8.4.4', port: 1 }] },
  ];
  const times = { '8.8.8.8': 31, '2001:db8::1': 24, '8.8.4.4': null };
  await withLatency(listed, async address => times[address]);
  assert.deepEqual(listed.map(server => server.latencyMs), [24, null, null]);
});

test('a server\'s battle query reaches a page as the battle\'s own keys only', () => {
  const query = battleParams('?map=bes2&side=imp&ai=off&seed=1&match=' + 'a'.repeat(32) + '&noreload=1&shell=1&ai=classic&era=gcw');
  assert.equal(String(query), 'map=bes2&side=imp&ai=off&seed=1&match=' + 'a'.repeat(32) + '&era=gcw', 'unknown keys and repeats dropped');
  assert.equal(String(battleParams('?map=../x&side=rep&ai=enhanced&seed=12345678901')), 'side=rep', 'values checked too');
  assert.equal(String(battleParams(undefined)), '');
});
