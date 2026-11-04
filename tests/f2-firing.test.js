import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Vehicles, Vehicle } from '../src/vehicles.js';
import { CollisionWorld } from '../src/collision.js';
import { Ordnance } from '../src/ordnance.js';
import { Units } from '../src/registry.js';
import { SeatLease, blankSeatIntent } from '../src/seats.js';
import { loadSim } from '../src/sim/module.js';
import { SimBridge } from '../src/sim/runtime.js';
import { attachOrdnanceSim, ordnanceTests } from '../src/ordnance-sim.js';
import { attachF2 } from '../src/f2-sim.js';
import { weaponDefinition } from './fixtures/prediction-weapon-cases.js';

class Crew {
  constructor() {
    this.generation = 1; this.alive = true; this.team = 1;
    this.position = new THREE.Vector3(); this.velocity = new THREE.Vector3();
    this.intent = blankSeatIntent(); this.seat = null;
  }
  aimPoint(out) { return out.copy(this.position); }
}
function box(w, h, d) {
  const g = new THREE.BoxGeometry(w, h, d);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
}
async function fixture(native, def) {
  const ground = box(1000, 2, 1000), hull = box(2, 2, 4);
  const collision = new CollisionWorld(ground.positions, ground.indices, ground.polygons);
  const scene = new THREE.Scene(), units = new Units();
  const o = new Ordnance({ scene, collision, effects: { play() {} }, lights: { flash() {}, add() {}, update() {} }, texture() {}, model() {} });
  o.units = units;
  const data = { models: { hull: { bones: [], parts: [] } }, collision: { hull }, classes: { hull: {
    model: 'hull', base: 'hover', settings: { MaxHealth: 1000, SetAltitude: 2 },
    seats: [{ name: 'BODY', settings: {}, weapons: [{ def, aimers: [] }] }], explosion: null,
  } }, spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1, respawnTime: 10, classes: { 1: 'hull' } }] };
  const buffer = new ArrayBuffer(0), indexBuffer = new ArrayBuffer(0);
  const vs = new Vehicles({ scene, data, buffer, indexBuffer, collision, units, ordnance: o, texture() {}, effects: { play() {} } });
  vs.clock = () => vs.time;
  // Rider writeback is a separate phase; this fixture observes firing only.
  vs.syncCrews = () => {};
  const v = vs.vehicles[0], crew = units.add(new Crew());
  const lease = new SeatLease({ unit: crew, vehicle: v, index: 0 });
  crew.seat = v.seats[0].lease = lease;
  let draws = 0;
  v.seats[0].weapons[0].state.random = () => ((++draws * 17) % 31) / 31;
  const bridge = new SimBridge(await loadSim(), { classes: { Vehicle, Unit: Crew }, tests: ordnanceTests });
  attachOrdnanceSim(o, bridge);
  if (native) attachF2(vs, bridge, { killY: -300, deathRegions: [], scripted: [] }, { data, buffer, indexBuffer }, Crew);
  if (native) {
    vs.fireCrews = vs.updateTethers = v.fireSeat = () => { throw Error('JavaScript mounted phase reached'); };
  }
  const shots = [];
  vs.onFired = (h, seat, shot, owner, p, d) => shots.push([seat, shot.yaw, shot.pitch, shot.charge, shot.targetId, shot.targetGeneration, ...p.toArray(), ...d.toArray()]);
  return { vs, o, v, crew, shots, draws: () => draws };
}
const fields = ['timer', 'clip', 'reserve', 'heat', 'overheated', 'wait', 'reloadLeft', 'salvosLeft', 'salvoWait', 'kick', 'pattern', 'firing'];
for (const base of ['bolt', 'missile', 'beam']) test(`F2 whole phase matches mounted ${base}, reload and heat without game files`, async () => {
  const def = weaponDefinition(`fixture-${base}`, 0, { ordnance: { class: `fixture-${base}-round`, base, Velocity: 70, LifeSpan: .3 }, HeatPerShot: .2, HeatRecoverRate: .3 });
  const js = await fixture(false, def), rust = await fixture(true, def);
  for (let tick = 0; tick < 240; tick++) {
    for (const f of [js, rust]) {
      f.crew.intent.buttons[0] = tick % 37 < 30 ? 1 : 0;
      f.crew.intent.reload = tick % 59 === 0;
      f.crew.intent.aim = new THREE.Vector3(0, .1, 1);
      f.vs.update(1 / 60);
    }
    const a = js.v.seats[0].weapons[0].state, b = rust.v.seats[0].weapons[0].state;
    for (const key of fields) assert.equal(b[key], a[key], `${base} tick ${tick} ${key}`);
    assert.deepEqual(rust.shots, js.shots, `${base} tick ${tick} emission`);
    assert.equal(rust.draws(), js.draws());
  }
  assert.ok(js.shots.length > 2);
});
