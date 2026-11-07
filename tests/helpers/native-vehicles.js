import '../fixtures/native-anim.js';
import {unitOwner} from './unit-owner.js';
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
  // Registry construction differs among these small fixtures. Complete the
  // real native owner at their first live entry, after their cold setup.
  let owner;
  const ensure=()=>owner ||= unitOwner(system,bridge);
  Object.defineProperty(system,'f3',{configurable:true,
    get(){return ensure().phase.hulls;},
    set(value){Object.defineProperty(system,'f3',{value,writable:true,configurable:true});}});
  const board=system.board;
  system.board=(unit,...args)=>{ensure().adopt(unit);return board(unit,...args);};
  system.update=dt=>ensure().phase.phase(dt);
  return system;
}

export function nativeTurrets(crews) {
  // A synthetic page may have posed this rig before importing an authority
  // fixture. Its published local records, rather than stale shell fields,
  // are that fixture's cold seed pose.
  for (const t of crews.turrets) t.rig.root.traverse(node => {
    if (!node.matrixAutoUpdate) node.matrix.decompose(node.position,node.quaternion,node.scale);
  });
  const data = { models: {}, collision: {}, classes: {}, spawns: [] };
  const buffer = new ArrayBuffer(0), indexBuffer = new ArrayBuffer(0);
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer, indexBuffer,
    collision: crews.collision, texture: () => null, effects: { play() {} } });
  if(crews.units)system.attachUnits(crews.units);
  system.fixtureLive=crews.live;
  nativeVehicles(system, { data, buffer, indexBuffer });
  system.f3;
  crews.ordnance = system.ordnance;
  system.f2.seats.attachTurrets(crews);
  return system;
}
