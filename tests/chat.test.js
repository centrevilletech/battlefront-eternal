import test from 'node:test';
import assert from 'node:assert/strict';
import { ChatLog, CHAT_COLORS, CHAT_LIMIT } from '../src/chat.js';
test('chat preserves channels, bounds history, and treats markup as text', () => {
  const log = new ChatLog();
  for (const channel of ['global', 'team', 'server']) assert.equal(log.receive({ channel, text: '<b>Hello</b>' }).channel, channel);
  assert.equal(new Set(Object.values(CHAT_COLORS)).size, 3);
  assert.equal(log.messages[0].text, '<b>Hello</b>');
  assert.equal(log.receive({ channel: 'bad', text: 'no' }), null);
  assert.equal(log.receive({ text: '\n\t' }), null);
  for (let i = 0; i < 10; i++) log.receive({ text: `${i}` }, i);
  assert.equal(log.messages.length, 8);
  assert.equal(log.messages[0].text, '2');
  assert.equal(log.receive({ text: 'a'.repeat(1000) }).text.length, CHAT_LIMIT);
});
