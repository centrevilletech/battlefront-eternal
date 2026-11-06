// Synthetic authority fixtures use one module instance for collision, rounds
// and hulls, as createGame does. Presentation-only fixtures need none of this.
import * as THREE from 'three';
export { CollisionWorld } from '../../tools/collision-fixture.mjs';
import { bridgeFor } from '../../src/sim/runtime.js';
import { Unit } from '../../src/units.js';
import { Vehicle, Vehicles } from '../../src/vehicles.js';
import { Ordnance } from '../../src/ordnance.js';
import { attachOrdnanceSim, ordnanceTests } from '../../src/ordnance-sim.js';
import { attachF2 } from '../../src/f2-sim.js';
export function nativeVehicles(system, pack) {
  const bridge = bridgeFor(system.collision.fixtureInstance, { classes: { Unit, Vehicle }, tests: {
    ...ordnanceTests, unit: v => Boolean(v && !v.seats && !v.rig && (v.soldier || v.constructor === Object && 'generation' in v && 'team' in v)),
  } });
  const rounds = new Ordnance({ scene: new THREE.Scene(), collision: system.collision,
    texture: () => null, effects: { play() {} }, lights: { flash() {}, add() {} } });
  const previousRounds = system.ordnance;
  rounds.onExplode = (def, point, owner) => previousRounds?.explosion?.(def, point, owner);
  attachOrdnanceSim(rounds, bridge); system.ordnance = rounds;
  system.clock = () => system.time;
  system.seatRandom = () => .5;
  attachF2(system, bridge, { killY: -300, deathRegions: [], scripted: [] }, pack, Unit);
  return system;
}

export function nativeTurrets(crews) {
  const data = { models: {}, collision: {}, classes: {}, spawns: [] };
  const buffer = new ArrayBuffer(0), indexBuffer = new ArrayBuffer(0);
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer, indexBuffer,
    collision: crews.collision, texture: () => null, effects: { play() {} } });
  nativeVehicles(system, { data, buffer, indexBuffer });
  crews.ordnance = system.ordnance;
  system.f2.seats.attachTurrets(crews);
  return system;
}
