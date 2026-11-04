import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Vehicles, Vehicle } from '../src/vehicles.js';
import { CollisionWorld } from '../src/collision.js';
import { Ordnance } from '../src/ordnance.js';
import { PowerupStation } from '../src/props.js';
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
async function fixture(native, def, options = {}) {
  const ground = box(1000, 2, 1000), hull = box(2, 2, 4);
  const collision = new CollisionWorld(ground.positions, ground.indices, ground.polygons);
  const scene = new THREE.Scene(), units = new Units();
  const o = new Ordnance({ scene, collision, effects: { play() {} }, lights: { flash() {}, add() {}, update() {} }, texture() {}, model() {} });
  o.units = units;
  const bones = options.multibarrel ? [
    { name: 'gun', parent: -1, local: new THREE.Matrix4().toArray() },
    { name: 'barrel1', parent: 0, local: new THREE.Matrix4().makeTranslation(-1, 1, 2).toArray() },
    { name: 'barrel2', parent: 0, local: new THREE.Matrix4().makeTranslation(1, 1, 2).toArray() },
  ] : [];
  const aimers = options.multibarrel ? [{ node: 'gun', FireOutsideLimits: options.limits ? 0 : 1, AimerYawLimits: '-10 10', AimerPitchLimits: '-5 5',
    barrels: [{ BarrelNodeName: 'barrel1', FirePointName: 'barrel1', BarrelRecoil: 1 }, { BarrelNodeName: 'barrel2', FirePointName: 'barrel2', BarrelRecoil: 1 }] }] : [];
  const data = { models: { hull: { bones, parts: [] } }, collision: { hull }, classes: { hull: {
    model: 'hull', base: 'hover', settings: { MaxHealth: 1000, SetAltitude: 2 },
    seats: [{ name: 'BODY', settings: {}, weapons: [{ def, aimers }] }], explosion: null,
  } }, spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1, respawnTime: 10, classes: { 1: 'hull' } }] };
  if (options.tow) {
    const walkerBones = ['pelvis', 'left_thigh', 'left_calf', 'left_foot'].map((name, i) => ({ name, parent: -1,
      local: new THREE.Matrix4().makeTranslation(0, [6, 5, 2, 0][i], 0).toArray() }));
    data.models.walker = { bones: walkerBones, parts: [] }; data.collision.walker = box(4, 12, 4);
    data.classes.walker = { model: 'walker', base: 'commandwalker', settings: { MaxHealth: 1000 },
      seats: [{ name: 'BODY', settings: {}, weapons: [] }], explosion: null };
    data.spawns.push({ name: 'target', matrix: new THREE.Matrix4().makeTranslation(0, 0, 30).toArray(), team: 2, respawnTime: 10, classes: { 2: 'walker' } });
  }
  const buffer = new ArrayBuffer(0), indexBuffer = new ArrayBuffer(0);
  const vs = new Vehicles({ scene, data, buffer, indexBuffer, collision, units, ordnance: o, texture() {}, effects: { play() {} } });
  vs.clock = () => vs.time;
  // Rider writeback is a separate phase; this fixture observes firing only.
  vs.syncCrews = () => {};
  const v = vs.vehicles[0], crew = units.add(new Crew());
  crew.networkHuman = Boolean(options.network);
  const lease = new SeatLease({ unit: crew, team: crew.team, vehicle: v, index: 0 });
  crew.seat = v.seats[0].lease = lease;
  if (options.tow) {
    const cast = collision.cast.bind(collision), target = vs.vehicles[1];
    collision.cast = (origin, direction, far, out, ignore) => {
      if (ignore === v.body && direction.z > .1 && far >= 20) {
        out.body = target.body; out.distance = 30; out.point.set(0, 2, 0).applyMatrix4(target.matrix); out.normal.set(0, 0, -1); out.terrain = false;
        return out;
      }
      return cast(origin, direction, far, out, ignore);
    };
  }
  let draws = 0;
  v.seats[0].weapons[0].state.random = () => ((++draws * 17) % 31) / 31;
  const bridge = new SimBridge(await loadSim(), { classes: { Vehicle, Unit: Crew }, tests: ordnanceTests });
  attachOrdnanceSim(o, bridge);
  if (native) attachF2(vs, bridge, { killY: -300, deathRegions: [], scripted: [] }, { data, buffer, indexBuffer }, Crew);
  if (native) {
    vs.fireCrews = vs.updateTethers = v.fireSeat = () => { throw Error('JavaScript mounted phase reached'); };
  }
  const shots = [], launched = [];
  o.onLaunch = b => launched.push([b.networkFireChannel, ...b.position.toArray(), ...b.direction.toArray()]);
  vs.onFired = (h, seat, shot, owner, p, d) => shots.push([seat, shot.yaw, shot.pitch, shot.charge, shot.targetId, shot.targetGeneration, ...p.toArray(), ...d.toArray()]);
  return { vs, o, v, crew, shots, launched, target: vs.vehicles[1], draws: () => draws };
}
const fields = ['timer', 'clip', 'reserve', 'heat', 'overheated', 'wait', 'reloadLeft', 'salvosLeft', 'salvoWait', 'kick', 'pattern', 'firing'];
for (const base of ['bolt', 'missile', 'beam', 'multibarrel', 'defaultChannel']) test(`F2 whole phase matches mounted ${base}, reload and heat without game files`, async () => {
  const def = weaponDefinition(`fixture-${base}`, 0, { ordnance: { class: `fixture-${base}-round`, base: base === 'multibarrel' || base === 'defaultChannel' ? 'bolt' : base, Velocity: 70, LifeSpan: .3 }, HeatPerShot: .2, HeatRecoverRate: .3 });
  if (base === 'defaultChannel') delete def.channel;
  const js = await fixture(false, def, { multibarrel: base === 'multibarrel' }), rust = await fixture(true, def, { multibarrel: base === 'multibarrel' });
  for (let tick = 0; tick < 240; tick++) {
    for (const f of [js, rust]) {
      f.crew.intent.buttons[0] = tick % 37 < 30 ? 1 : 0;
      f.crew.intent.reload = tick % 59 === 0;
      f.crew.intent.aim = new THREE.Vector3(0, .1, 1);
      f.vs.update(1 / 60);
      if (tick === 180) PowerupStation.prototype.serve.call({ vehicleHealth: 0, giveHealth: 0, vehicleAmmo: 2, giveAmmo: 2 }, f.v);
    }
    const a = js.v.seats[0].weapons[0].state, b = rust.v.seats[0].weapons[0].state;
    for (const key of fields) assert.equal(b[key], a[key], `${base} tick ${tick} ${key}`);
    assert.deepEqual(rust.shots, js.shots, `${base} tick ${tick} emission`);
    assert.equal(rust.draws(), js.draws());
    assert.deepEqual(rust.launched, js.launched, `${base} tick ${tick} launch metadata`);
  }
  assert.ok(js.shots.length > 2);
});


test('F2 launches and holds a tow cable, then releases a destroyed end', async () => {
  const def = weaponDefinition('fixture-tow', 0, { ordnance: { class: 'fixture-cable', base: 'towcable', CableLength: 100 }, LockOnRange: 80 });
  const js = await fixture(false, def, { tow: true }), rust = await fixture(true, def, { tow: true });
  for (let tick = 0; tick < 20; tick++) {
    for (const f of [js, rust]) { f.crew.intent.buttons[0] = 1; f.vs.update(1 / 60); }
    assert.equal(rust.v.towTarget, js.v.towTarget);
    assert.deepEqual(rust.v.towLocal.toArray(), js.v.towLocal.toArray());
    assert.equal(rust.v.towLeg, js.v.towLeg);
    assert.equal(rust.v.towWinding, js.v.towWinding);
    assert.deepEqual(rust.shots, js.shots);
  }
  assert.equal(rust.v.towTarget, 1, 'the scripted harpoon attached to the enemy leg');
  for (const f of [js, rust]) { f.target.damage(Infinity); f.vs.update(1 / 60); }
  assert.equal(rust.v.towTarget, -1); assert.equal(js.v.towTarget, -1);
});


test('F2 consumes an out-of-limits barrel turn and suppresses emission', async () => {
  const def = weaponDefinition('fixture-limits');
  const js = await fixture(false, def, { multibarrel: true, limits: true }), rust = await fixture(true, def, { multibarrel: true, limits: true });
  for (let tick = 0; tick < 60; tick++) {
    for (const f of [js, rust]) { f.crew.intent.buttons[0] = 1; f.crew.intent.aim = new THREE.Vector3(1, 1, 0); f.vs.update(1 / 60); }
    assert.equal(rust.v.seats[0].weapons[0].barrel, js.v.seats[0].weapons[0].barrel);
    assert.deepEqual(rust.shots, js.shots);
  }
  // An aimer has no inLimits flag before its first pose, so the initial
  // shot may pass. Later turns still consume their barrel and ammunition.
  assert.ok(rust.shots.length <= 1); assert.ok(rust.v.seats[0].weapons[0].barrel > rust.shots.length);
});


test('F2 acquires and fires an ordinary mounted missile lock', async () => {
  const def = weaponDefinition('fixture-lock', 0, { base: 'launcher', LockOnRange: 80, LockOnAngle: 20, LockOffAngle: 30,
    LockTime: .1, ClearLockOnFire: 1, ordnance: { class: 'fixture-lock-round', base: 'missile', Velocity: 50, LifeSpan: 2 } });
  const js = await fixture(false, def, { tow: true, network: true }), rust = await fixture(true, def, { tow: true, network: true });
  let locked = 0;
  for (let tick = 0; tick < 120; tick++) {
    for (const f of [js, rust]) { f.crew.intent.buttons[0] = tick > 60 ? 1 : 0; f.crew.intent.aim = new THREE.Vector3(0, 0, 1); f.vs.update(1 / 60); }
    const a = js.v.seats[0].weapons[0].state, b = rust.v.seats[0].weapons[0].state;
    for (const k of ['lockTargetId', 'lockGeneration', 'lockElapsed', 'lockComplete']) assert.equal(b[k], a[k], `${tick} ${k}`);
    if (b.lockComplete) locked++;
    assert.deepEqual(rust.shots, js.shots);
  }
  assert.ok(locked > 0); assert.ok(js.shots.some(s => s[4] === js.target.aiTarget.id));
});
