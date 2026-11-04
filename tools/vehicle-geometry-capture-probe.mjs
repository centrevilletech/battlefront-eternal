// Query the actual hull fallback fire point inside a sampled world tick.
// This covers models whose ordinary weapons always have real fire nodes.
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
let calls = 0;
export function battle({ game, world }) {
  const step = world.step;
  world.step = function (...args) {
    if (game.worldClock.tick + 1 === 60) for (const hull of game.vehicles.vehicles) if (hull.alive) {
      hull.nose(new Vector3()); calls++;
    }
    return step.apply(this, args);
  };
}
export function ended() { assert.ok(calls > 0, 'real hull fallback fire points were queried'); }
