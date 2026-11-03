// World-hash plugin for vehicles-sim.test.js. Besides the battle hashes,
// compare every aircraft rig node and the frame its camera follows. These
// numbers feed muzzle and rider positions but are not all in worldState.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { rigNodes } from '../src/vehicles-sim.js';

let vehicles, server, rust, updating = false, jsTicks = 0, jsAim = 0, flying = 0;

export function battle({ game, server: local, options }) {
  server = local;
  vehicles = game.vehicles;
  rust = options.hulls === 'rust';
  if (rust) assert.ok(vehicles.sim?.flight && vehicles.sim?.posing, 'flight and poses are on the module');
  assert.ok(vehicles.vehicles.some((v) => v.flying && v.alive && vehicles.board(server.actor.unit, v, 0)), 'the player boards an aircraft');
  const update = vehicles.update;
  vehicles.update = function (...args) {
    updating = true;
    try { return update.apply(this, args); } finally { updating = false; }
  };
  const proto = Object.getPrototypeOf(vehicles.vehicles[0]);
  for (const method of ['tick', 'aimTick']) {
    const original = proto[method];
    proto[method] = function (...args) {
      if (updating && this.flying) {
        if (method === 'tick') jsTicks++;
        else jsAim++;
      }
      return original.apply(this, args);
    };
  }
}

export function tick(tick) {
  if (tick === 1) assert.ok(server.world.actions(server.id, {
    epoch: 1, id: 1, tick, generation: server.actor.unit.generation, actions: ['takeoff'],
  }), 'the world accepts takeoff');
}

export function ticked(tick) {
  const numbers = [];
  for (const v of vehicles.vehicles) {
    if (!v.flying) continue;
    if (v.flightState === 'Flying') flying++;
    if (rust && v.alive) assert.ok(vehicles.sim.posed(v), `${v.name}'s pose is on the module`);
    numbers.push(v.age, v.gear, ...v.visual.toArray(), ...(v.flightFrameQ?.toArray() ?? [NaN, NaN, NaN, NaN]));
    for (const node of rigNodes(v)) {
      numbers.push(...node.position.toArray(), ...node.quaternion.toArray(), ...node.matrixWorld.elements);
    }
  }
  const hash = createHash('sha256').update(Buffer.from(new Float64Array(numbers).buffer)).digest('hex');
  console.error(`aircraft-pose ${tick} ${hash}`);
}

export function finish() {
  assert.ok(flying > 0, 'the battle exercises airborne aircraft');
  if (rust) {
    assert.equal(jsTicks, 0, 'aircraft physics never falls back to JavaScript');
    assert.equal(jsAim, 0, 'aircraft posing never falls back to JavaScript');
  } else {
    assert.ok(jsTicks > 0 && jsAim > 0, 'the reference exercises JavaScript flight and poses');
  }
}
