import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles, Vehicle } from '../src/vehicles.js';
import { attachF2 } from '../src/f2-sim.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { loadSim } from '../src/sim/module.js';

function box(w, h, d, y) {
  const geometry = new THREE.BoxGeometry(w, h, d).translate(0, y, 0);
  return { positions: Array.from(geometry.attributes.position.array), indices: Array.from(geometry.index.array),
    polygons: Array(geometry.index.count / 3).fill(0) };
}

async function fleet(native) {
  const floor = box(2000, 2, 2000, -1);
  const collision = new CollisionWorld(new Float32Array(floor.positions), new Uint32Array(floor.indices), new Uint32Array(floor.polygons));
  const def = { model: 'tank', base: 'hover', settings: { MaxHealth: 1000, ForwardSpeed: 12, TurnRate: 1 },
    seats: [{ name: 'BODY', settings: {}, weapons: [] }], explosion: null };
  const post = { desc: { name: 'cp' }, team: 1 };
  const data = { models: { tank: { bones: [], parts: [] } }, collision: { tank: box(2, 2, 4, 1) },
    classes: { tank: def, replacement: def }, spawns: [0, 100].map((x, i) => ({ name: `pad${i}`,
      matrix: new THREE.Matrix4().makeTranslation(x, 0, 0).toArray(), team: 1, post: 'cp', respawnTime: .01,
      classes: { 1: 'tank', 2: 'replacement' } })) };
  const buffer = new ArrayBuffer(0), indexBuffer = new ArrayBuffer(0);
  const system = new Vehicles({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(), data, buffer, indexBuffer,
    collision, posts: { posts: [post] }, texture: () => null, ordnance: { explosion() {} }, effects: { play() {} } });
  system.clock = () => system.time;
  if (native) {
    class Unit {}
    attachF2(system, bridgeFor(await loadSim(), { classes: { Unit, Vehicle } }),
      { killY: -300, deathRegions: [], scripted: [] }, { data, buffer, indexBuffer }, Unit, true);
  }
  return { system, post };
}

function observe(v) {
  return { name: v.name, alive: v.alive, health: v.health, position: v.position.toArray(), velocity: v.velocity.toArray(),
    yaw: v.yaw, yawRate: v.yawRate, seats: v.seats.map(s => [s.aimYaw, s.pitch, s.lookYaw, s.lookPitch]) };
}

for (const replacement of [false, true]) {
  test(`F2 preserves another hull's pending look during ${replacement ? 'variant construction' : 'respawn'}`, async () => {
    const run = async native => {
      const { system, post } = await fleet(native);
      try {
        const [pilot, other] = system.vehicles;
        // Give the old life a distinct aim so preserving it across respawn
        // would also fail, even when the other hull's input survives.
        other.seats[0].lookYaw = .7; other.seats[0].lookPitch = .2;
        system.update(1 / 60);
        other.setHealth(0);
        if (replacement) post.team = 2;
        const seat = pilot.seats[0];
        seat.aimYaw = seat.lookYaw = .4;
        seat.pitch = seat.lookPitch = -.1;
        system.update(1 / 60);
        const spawned = system.slots[1].vehicle;
        assert.ok(spawned.alive);
        assert.equal(spawned.name, replacement ? 'replacement' : 'tank');
        assert.equal(pilot.seats[0].lookYaw, .4, 'the pending yaw reaches the ground substeps');
        assert.equal(pilot.seats[0].lookPitch, -.1, 'the pending pitch reaches the ground substeps');
        assert.equal(spawned.seats[0].lookYaw, null, 'the new life resets its own look');
        assert.equal(spawned.seats[0].pitch, 0, 'the new life resets its own pitch');
        return system.vehicles.map(observe);
      } finally { system.dispose(); }
    };
    assert.deepEqual(await run(true), await run(false), 'the whole phase matches JavaScript');
  });
}
