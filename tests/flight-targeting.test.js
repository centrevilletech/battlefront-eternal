import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { updateFlightTarget } from '../src/flight/targeting.js';
import { Units } from '../src/registry.js';

// A fighter of team 1 at the origin, nose along +Z, over a clear sky (no
// cast hits), with a teammate dead ahead and an enemy a little off the nose.
function sky() {
  const units = new Units();
  units.teams = { relations: { 1: { 1: 1, 2: -1 }, 2: { 1: -1, 2: 1 } }, factions: {}, capture: {} };
  const unit = (id, team, x, z) => ({ id, team, alive: true, generation: 1, position: new THREE.Vector3(x, 0, z) });
  const friend = unit(2, 1, 0, 200), enemy = unit(3, 2, 4, 200);
  const all = [friend, enemy];
  units.all = () => all;
  units.byId = (id) => all.find((u) => u.id === id);
  const v = { team: 1, aiTarget: { id: 1, team: 1 }, position: new THREE.Vector3(), quaternion: new THREE.Quaternion(), body: {}, seats: [],
    flightConfig: { assistCone: 3, lockCone: 15, retainCone: 25, lockRange: 1600, lockDwell: 1, losGrace: .3 },
    flightLockId: -1, flightLockTimer: 0, flightLostLOS: 0,
    system: { flightAssists: true, units, collision: { cast: () => null } } };
  return { v, friend, enemy };
}

test('a fighter assists and locks onto enemies, never a teammate in front of it', () => {
  const { v, enemy } = sky();
  updateFlightTarget(v, 1 / 60, false);
  assert.equal(v.flightAssistId, enemy.id, 'the aim assist takes the enemy off the nose, not the teammate on it');
  updateFlightTarget(v, 1 / 60, true);
  assert.equal(v.flightLockId, enemy.id, 'a lock press locks the enemy');
});

test('the best target is the highest score it can see, the sight ray going to each in score order', () => {
  const { v, friend, enemy } = sky();
  friend.team = 2;  // now both enemies: the one on the nose scores higher
  const rays = [];
  v.system.collision.cast = (from, dir, distance) => { rays.push(dir.x); return rays.length === 1 ? { distance: distance / 2, body: { owner: null } } : null; };
  updateFlightTarget(v, 1 / 60, false);
  assert.equal(v.flightAssistId, enemy.id, 'the higher scorer was hidden, so the next one');
  assert.equal(rays.length, 2, 'a ray for each candidate in score order until one passes');
});
