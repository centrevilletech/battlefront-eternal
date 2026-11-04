// Exercise real droideka transitions and destructible droid deaths during
// a headless battle. The capture scanner, recorder and plain run use the
// same script, so instrumentation still has to preserve every fingerprint.
import assert from 'node:assert/strict';

let game, server, action = 0, deaths = 0;
export function battle({ game: world, server: local }) {
  game = world; server = local;
  assert.ok(server.actor.unit.walker, 'the probe player is a droideka');
}
export function tick(tick) {
  if (tick === 30 || tick === 180) server.world.actions(server.id, {
    epoch: 1, id: ++action, tick, generation: server.actor.unit.generation, actions: ['use'],
  });
  if (tick >= 120 && tick % 60 === 0) {
    const u = game.factory.built.find(u => u.alive && !u.removed && u.combat.chunks > 0 && u.soldier.air !== 'thrown');
    if (u) { u.combat.damage(Infinity); deaths++; }
  }
  if (tick === 400) {
    const u = server.actor.unit;
    u.soldier.mode = 0; u.soldier.time = 0; u.soldier.reverse = false;
    u.combat.damage(Infinity);
  }
}
export function finish() { assert.ok(deaths >= 3, 'the capture exercises at least three real chunk-selection death draws'); }
