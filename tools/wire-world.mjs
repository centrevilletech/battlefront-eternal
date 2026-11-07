// world-hash plugin: exercise and require the native snapshot runtime.
// Usage: node tools/world-hash.mjs --plugin tools/wire-world.mjs ...
import assert from 'node:assert/strict';
export function battle({ game, server }) {
  assert.ok(game.simBridge, 'wire requires the shared simulation instance');
  assert.equal(server.host.inspect('local')?.wire, 'rust');
}
