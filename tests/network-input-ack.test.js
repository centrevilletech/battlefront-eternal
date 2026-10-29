import test from 'node:test';
import assert from 'node:assert/strict';
import { InputMailbox } from '../src/runtime/transport.js';

test('input acknowledgment records execution time independently of snapshot publication', () => {
  const mailbox = new InputMailbox();
  const command = (sequence,tick) => ({epoch:1,sequence,tick,generation:1,down:[],actions:[],look:[]});
  mailbox.push(command(1,10));
  mailbox.take(10,1);
  mailbox.take(11,1);
  mailbox.take(12,1);
  assert.equal(mailbox.ack,1);
  assert.equal(mailbox.ackTick,10,'20Hz publication must not label on-time input late');
  mailbox.push(command(2,13));
  mailbox.take(16,1);
  assert.equal(mailbox.ack,2);
  assert.equal(mailbox.ackTick,16,'genuinely late execution retains its actual tick');
});
