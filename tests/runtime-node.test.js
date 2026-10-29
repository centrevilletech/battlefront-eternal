import { test } from 'node:test';
import assert from 'node:assert/strict';
test('headless engine imports in Node without browser texture/WASM loading', async () => {
  assert.equal(typeof document, 'undefined');
  assert.equal(typeof window, 'undefined');
  const { createGame } = await import('../src/game.js');
  assert.equal(typeof createGame, 'function');
});
