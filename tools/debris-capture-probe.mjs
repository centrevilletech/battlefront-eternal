// Run the real page debris path inside sampled headless steps. This does
// not change the authoritative tick: use the probe in both baseline and
// candidate runs. Actual content supplies models, bones and collisions.
import assert from 'node:assert/strict';
import { VehicleEffects } from '../src/vehicle-effects.js';
let game, spawned = 0, flights = 0, rests = 0, settles = 0, removed = 0;
export function battle(context) {
  game = context.game;
  const fx = new VehicleEffects({ effects: game.effects, debris: game.debris });
  const world = context.world, step = world.step;
  world.step = function (...args) {
    const tick = game.worldClock.tick + 1, d = game.debris;
    if (tick === 10 || tick === 180) {
      const before = d.pieces.length;
      for (const v of game.vehicles.vehicles.filter(v => v.def.fx?.chunks?.length).slice(0, 3)) fx.blowApart(v, v.def.fx);
      const unit = game.playerUnit;
      if (unit.classDef.chunks?.length) {
        unit.actor.root.updateMatrixWorld(true);
        d.blowApart(unit.classDef.chunks, unit.actor.byName, unit.position, unit.velocity, tick === 10 ? -50 : 0);
      }
      spawned += d.pieces.length - before;
    }
    if (tick === 25 && d.pieces.length) d.bounce(d.pieces[0]);
    if (tick === 90 && d.pieces.length) d.rest(d.pieces[0], d.pieces[0].position.y - d.pieces[0].lift);
    const before = d.pieces.length;
    d.update(1 / 60);
    flights += d.pieces.filter(p => !p.resting).length; rests += d.pieces.filter(p => p.resting).length; settles += d.pieces.filter(p => p.settle).length;
    removed += before - d.pieces.length;
    if (tick === 2300) d.clear();
    return step.apply(this, args);
  };
}
export function ended() {
  assert.ok(spawned > 0, 'real chunks were launched'); assert.ok(flights > 0, 'real chunks flew'); assert.ok(rests > 0, 'real chunks rested');
  console.error(JSON.stringify({ debrisProbe: { spawned, flights, rests, settles, removed } }));
}
