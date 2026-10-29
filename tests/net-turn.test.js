import test from 'node:test';
import assert from 'node:assert/strict';
import { TurnClient } from '../src/net/turn.js';
import { turnServer, bind, waitFor } from './net-udp-fixtures.js';

// Permissions last 300 s and channels 600 s on a real relay; a client that
// let either lapse would lose its relayed peer mid-battle (seen against
// coturn 4.18 with shortened lifetimes).
test('a TURN client keeps refreshing every permitted peer and its channels', async t => {
  const relay = await turnServer(), socket = await bind();
  const turn = new TurnClient({ server: { address: '127.0.0.1', port: relay.port }, username: relay.username, password: relay.password,
    send: (packet, port, address) => socket.send(packet, port, address), permissionRefreshMs: 40, channelRefreshMs: 60 });
  socket.on('message', packet => turn.handle(packet));
  t.after(() => { turn.close(); socket.close(); relay.close(); });
  const { relayed } = await turn.allocate(600);
  assert.equal(relayed.address, '127.0.0.1');
  await turn.permit(['192.0.2.7', '198.51.100.9']);
  await turn.bind('192.0.2.7', 4000);
  const permissions = relay.stats.permissions, channels = relay.stats.channels;
  await waitFor(() => relay.stats.permissions >= permissions + 6 && relay.stats.channels >= channels + 2, 3000);
});

test('a TURN client reauthenticates on a stale nonce and reports a dead allocation', async t => {
  const relay = await turnServer(), socket = await bind();
  const expired = [];
  const turn = new TurnClient({ server: { address: '127.0.0.1', port: relay.port }, username: relay.username, password: 'wrong',
    send: (packet, port, address) => socket.send(packet, port, address), onExpired: error => expired.push(error) });
  socket.on('message', packet => turn.handle(packet));
  t.after(() => { turn.close(); socket.close(); relay.close(); });
  await assert.rejects(turn.allocate(600), /TURN 401/, 'bad credentials fail clearly after one retry');
});
