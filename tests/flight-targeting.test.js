import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { fixture } from './flight-fixture.js';
import { createHullReplay } from './helpers/hull-prediction.js';

// These are real native collision and registry facts. A JavaScript cast stub
// cannot observe or replace a query made inside the prediction session.
function sky(blocked = false) {
  const f = fixture();
  f.v.position.set(0, 50, 0); f.v.flightState = 'Flying'; f.v.airspeed = 20;
  f.system.flightEnvironment.space = true; f.system.flightAssists = true;
  const friend = { id: 2, generation: 1, alive: true, team: blocked ? 2 : 1, position: new THREE.Vector3(0, 50, 200) };
  const enemy = { id: 3, generation: 1, alive: true, team: 2, position: new THREE.Vector3(4, 50, 200) };
  const all = [friend, enemy];
  f.system.units = { all: () => all, byId: id => all.find(t => t.id === id), relation: (a, b) => a === b ? 1 : -1, remove() {} };
  if (blocked) {
    const wall = new THREE.BoxGeometry(1, 20, 1).translate(0, 50, 100);
    f.c.addBody(wall.attributes.position.array, wall.index.array, new Uint32Array(wall.index.count / 3));
  }
  f.v.resetPresentationHistory();
  return { ...f, friend, enemy, replay: createHullReplay(f.v) };
}
const command = (tick, actions = []) => ({ tick, sequence: tick, generation: 1, down: [], actions, look: [] });
test('a native flyer session assists and locks an enemy while ignoring the teammate ahead', () => {
  const f = sky();
  try {
    const pending = [command(1)];
    assert.equal(f.replay.predict(pending, 0).flightAssistId, f.enemy.id);
    pending.push(command(2, ['targetLock']));
    assert.equal(f.replay.predict(pending, 0).flightLockId, f.enemy.id);
  } finally { f.replay.dispose(); f.system.dispose(); }
});
test('native target acquisition rejects the obscured higher score before selecting the visible enemy', () => {
  const f = sky(true);
  try {
    const predicted = f.replay.predict([command(1)], 0);
    assert.equal(predicted.flightAssistId, f.enemy.id);
    assert.notEqual(predicted.flightAssistId, f.friend.id);
  } finally { f.replay.dispose(); f.system.dispose(); }
});
