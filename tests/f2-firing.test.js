import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Vehicles, Vehicle } from '../src/vehicles.js';
import { CollisionWorld } from '../src/collision.js';
import { Ordnance } from '../src/ordnance.js';
import { PowerupStation } from '../src/props.js';
import { Units } from '../src/registry.js';
import { Unit } from '../src/units.js';
import { Soldier } from '../src/soldier.js';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { SimBridge } from '../src/sim/runtime.js';
import { attachOrdnanceSim, ordnanceTests } from '../src/ordnance-sim.js';
import { attachF2 } from '../src/f2-sim.js';
import { weaponDefinition } from './fixtures/prediction-weapon-cases.js';

class Crew extends Unit {
  constructor(collision) {
    super({ side: 'fixture', className: 'fixture', def: {}, soldier: new Soldier(collision, {}),
      actor: null, animator: null, presentation: null, combat: { dead: false, zoom: 1, zoomStep: 0 } });
    this.generation = 1; this.alive = true; this.team = 1; this.removed = false;
  }
}

function box(w, h, d) {
  const g = new THREE.BoxGeometry(w, h, d);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
}
async function fixture(native, def, options = {}) {
  const ground = box(1000, 2, 1000), hull = box(2, 2, 4);
  const instance = await loadSim();
  const collision = new CollisionWorld(ground.positions, ground.indices, ground.polygons, undefined, undefined, await loadRayKernel(instance));
  const scene = new THREE.Scene(), units = new Units();
  const o = new Ordnance({ scene, collision, effects: { play() {} }, lights: { flash() {}, add() {}, update() {} }, texture() {}, model() {} });
  o.units = units;
  const bones = options.fallbackAimer ? [{ name: 'gun', parent: -1, local: new THREE.Matrix4().makeTranslation(2, 1, 3).toArray() }] : options.multibarrel ? [
    { name: 'gun', parent: -1, local: new THREE.Matrix4().toArray() },
    { name: 'barrel1', parent: 0, local: new THREE.Matrix4().makeTranslation(-1, 1, 2).toArray() },
    { name: 'barrel2', parent: 0, local: new THREE.Matrix4().makeTranslation(1, 1, 2).toArray() },
  ] : [];
  const aimers = options.fallbackAimer ? [{ node: 'gun', FireOutsideLimits: 1, barrels: [] }] : options.multibarrel ? [{ node: 'gun', FireOutsideLimits: options.limits ? 0 : 1, AimerYawLimits: '-10 10', AimerPitchLimits: '-5 5',
    barrels: [{ BarrelNodeName: 'barrel1', FirePointName: 'barrel1', BarrelRecoil: 1 }, { BarrelNodeName: 'barrel2', FirePointName: 'barrel2', BarrelRecoil: 1 }] }] : [];
  const data = { models: { hull: { bones, parts: [] } }, collision: { hull }, classes: { hull: {
    model: 'hull', base: options.flying ? 'flyer' : 'hover', settings: { MaxHealth: 1000, SetAltitude: 2 },
    seats: [{ name: 'BODY', settings: {}, weapons: [{ def, aimers }] }], explosion: null,
  } }, spawns: [{ name: 'pad', matrix: new THREE.Matrix4().makeTranslation(options.translated ? 20 : 0, 0, 0).toArray(), team: 1, respawnTime: 10, classes: { 1: 'hull' } }] };
  if (options.tow) {
    const walkerBones = ['pelvis', 'left_thigh', 'left_calf', 'left_foot'].map((name, i) => ({ name, parent: -1,
      local: new THREE.Matrix4().makeTranslation(0, [6, 5, 2, 0][i], 0).toArray() }));
    data.models.walker = { bones: walkerBones, parts: [] }; data.collision.walker = box(4, 12, 4);
    data.classes.walker = { model: 'walker', base: 'commandwalker', settings: { MaxHealth: 1000 },
      seats: [{ name: 'BODY', settings: {}, weapons: [] }], explosion: null };
    data.spawns.push({ name: 'target', matrix: new THREE.Matrix4().makeTranslation(options.trip ? 12 : 0, 0, options.trip ? 0 : 30).toArray(), team: 2, respawnTime: 10, classes: { 2: 'walker' } });
  }
  if (options.other) data.spawns.push({ name: 'other', matrix: new THREE.Matrix4().makeTranslation(80, 0, 0).toArray(), team: 1, respawnTime: .1, classes: { 1: 'hull' } });
  if (options.trip) {
    Object.assign(data.classes.hull.settings, { ForwardSpeed: 12, StrafeSpeed: 0, TurnRate: 1, Traction: 1000, Acceleration: 1000, Deceleration: 1000 });
    data.classes.walker.explosion = { class: 'fixture-trip-blast', Damage: 0, DamageRadiusInner: 0, DamageRadiusOuter: 0 };
  }
  const buffer = new ArrayBuffer(0), indexBuffer = new ArrayBuffer(0);
  const vs = new Vehicles({ scene, data, buffer, indexBuffer, collision, units, ordnance: o, texture() {}, effects: { play() {} }, flightEnvironment: options.environment });
  vs.clock = () => vs.time;
  vs.flightAssists = Boolean(options.flying);
  // Rider writeback is a separate phase; this fixture observes firing only.
  vs.syncCrews = () => {};
  const v = vs.vehicles[0], crew = units.add(new Crew(collision));
  if (options.flying) {
    v.flightState = 'Flying'; v.grounded = false; v.position.y = 12; v.previous.copy(v.position);
    v.matrix.setPosition(v.position); v.syncBody();
    v.flightLockId = vs.vehicles[1].aiTarget.id; v.flightLockGeneration = vs.vehicles[1].aiTarget.generation;
    v.flightLockState = 'Locked'; v.flightLockTimer = 10;
  }
  crew.networkHuman = Boolean(options.network);
  let draws = 0;
  const bridge = new SimBridge(instance, { classes: { Vehicle, Unit: Crew }, tests: ordnanceTests });
  attachOrdnanceSim(o, bridge);
  if (native) attachF2(vs, bridge, { killY: -300, deathRegions: [], scripted: [] }, { data, buffer, indexBuffer }, Crew);
  assert.equal(vs.board(crew, v), true);
  v.seats[0].weapons[0].state.random = () => ((++draws * 17) % 31) / 31;
  if (native) {
    vs.fireCrews = vs.updateTethers = v.fireSeat = () => { throw Error('JavaScript mounted phase reached'); };
  }
  const shots = [], launched = [];
  o.onLaunch = b => launched.push([b.networkFireChannel, ...b.position.toArray(), ...b.direction.toArray()]);
  vs.onFired = (h, seat, shot, owner, p, d) => shots.push([seat, shot.yaw, shot.pitch, shot.charge, shot.targetId, shot.targetGeneration, ...p.toArray(), ...d.toArray()]);
  return { vs, o, v, crew, shots, launched, target: vs.vehicles[1], draws: () => draws };
}
const fields = ['timer', 'clip', 'reserve', 'heat', 'overheated', 'wait', 'reloadLeft', 'salvosLeft', 'salvoWait', 'kick', 'pattern', 'firing'];
for (const base of ['bolt', 'missile', 'beam', 'beamFallback', 'multibarrel', 'defaultChannel']) test(`F2 whole phase matches mounted ${base}, reload and heat without game files`, async () => {
  const def = weaponDefinition(`fixture-${base}`, 0, { ordnance: { class: `fixture-${base}-round`, base: base === 'multibarrel' || base === 'defaultChannel' ? 'bolt' : base === 'beamFallback' ? 'beam' : base, Velocity: 70, LifeSpan: .3 }, HeatPerShot: .2, HeatRecoverRate: .3 });
  if (base === 'defaultChannel') delete def.channel;
  const js = await fixture(false, def, { multibarrel: base === 'multibarrel', fallbackAimer: base === 'beamFallback' }), rust = await fixture(true, def, { multibarrel: base === 'multibarrel', fallbackAimer: base === 'beamFallback' });
  for (let tick = 0; tick < 240; tick++) {
    for (const f of [js, rust]) {
      if (base === 'defaultChannel' && tick === 150) f.v.seats[0].weapons[0].state.channel = 1;
      f.crew.intent.buttons[0] = tick % 37 < 30 ? 1 : 0;
      f.crew.intent.reload = tick % 59 === 0;
      f.crew.intent.aim = new THREE.Vector3(0, .1, 1);
      f.vs.update(1 / 60);
      f.o.update(1 / 60, false);
      if (tick === 180) PowerupStation.prototype.serve.call({ vehicleHealth: 0, giveHealth: 0, vehicleAmmo: 2, giveAmmo: 2 }, f.v);
    }
    const a = js.v.seats[0].weapons[0].state, b = rust.v.seats[0].weapons[0].state;
    for (const key of fields) assert.equal(b[key], a[key], `${base} tick ${tick} ${key}`);
    assert.deepEqual(rust.shots, js.shots, `${base} tick ${tick} emission`);
    assert.equal(rust.draws(), js.draws());
    assert.deepEqual(rust.launched, js.launched, `${base} tick ${tick} launch metadata`);
    assert.deepEqual(rust.o.beams.map(b => [b.localOrigin?.toArray(), b.position.toArray(), b.direction.toArray(), b.end.toArray()]),
      js.o.beams.map(b => [b.localOrigin?.toArray(), b.position.toArray(), b.direction.toArray(), b.end.toArray()]), `${base} tick ${tick} following beam`);
  }
  assert.ok(js.shots.length > 2);
});

test('F2 resets only the respawning hull\'s mounted weapons', async () => {
  const def = weaponDefinition('fixture-mounted-respawn');
  const js = await fixture(false, def, { other: true }), rust = await fixture(true, def, { other: true });
  const life = rust.target.aiTarget.generation, sourceLife = rust.v.aiTarget.generation;
  const freshClip = rust.target.seats[0].weapons[0].state.clip;
  for (let tick = 0; tick < 90; tick++) {
    for (const f of [js, rust]) {
      if (tick === 7) f.target.damage(Infinity);
      // A pending foreign edit must survive the other hull's spawn callback.
      if (tick === 13) { f.v.seats[0].weapons[0].state.spreadScale = .25; f.v.seats[0].weapons[0].state.channel = 1; }
      f.crew.intent.buttons[0] = 1;
      f.vs.update(1 / 60);
    }
    const a = js.v.seats[0].weapons[0].state, b = rust.v.seats[0].weapons[0].state;
    for (const key of [...fields, 'spreadScale', 'channel']) assert.equal(b[key], a[key], `respawn tick ${tick} ${key}`);
    assert.equal(rust.draws(), js.draws());
    assert.deepEqual(rust.shots, js.shots);
  }
  assert.equal(rust.target.aiTarget.generation, life + 1);
  assert.equal(rust.v.aiTarget.generation, sourceLife);
  assert.equal(rust.target.seats[0].weapons[0].state.clip, freshClip);
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
  assert.equal(rust.v.towTarget, 1, 'the harpoon attached to the enemy leg');
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


test('F2 commits a tow trip before reciprocal blast damage', async t => {
  const def = weaponDefinition('fixture-trip-tow', 0, { ordnance: { class: 'fixture-trip-cable', base: 'towcable', CableLength: 100 }, LockOnRange: 80 });
  const js = await fixture(false, def, { tow: true, trip: true }), rust = await fixture(true, def, { tow: true, trip: true });
  for (const f of [js, rust]) f.o.onExplode = (def, p, owner) => { if (owner === f.target) f.v.damage(7, f.target); };
  let tick = 0;
  for (; tick < 1800 && js.target.alive; tick++) {
    for (const f of [js, rust]) { f.crew.intent.buttons[0] = 1; f.crew.intent.forward = 1; f.crew.intent.turn = -1; f.vs.update(1 / 60); }
    assert.equal(rust.v.towTarget, js.v.towTarget, `cable tick ${tick}`);
    assert.equal(rust.v.towWinding, js.v.towWinding, `winding tick ${tick}`);
    assert.equal(rust.target.health, js.target.health, `walker tick ${tick}`);
    assert.equal(rust.v.health, js.v.health, `reciprocal damage tick ${tick}`);
  }
  assert.ok(tick < 1800, 'three circles must trip the synthetic walker');
  assert.equal(rust.v.health, rust.v.maxHealth - 7, 'the trip blast keeps its source damage');
  assert.equal(rust.v.towTarget, -1);
  t.diagnostic(`trip after ${tick} ticks; reciprocal source damage 7`);
});


test('F2 lock range starts at the translated seat eye', async () => {
  const def = weaponDefinition('fixture-lock-range', 0, { base: 'launcher', LockOnRange: 32, LockOnAngle: 20, LockTime: .1,
    ordnance: { class: 'fixture-lock-range-round', base: 'missile', Velocity: 50, LifeSpan: 2 } });
  const js = await fixture(false, def, { tow: true, network: true, translated: true }), rust = await fixture(true, def, { tow: true, network: true, translated: true });
  for (let tick = 0; tick < 30; tick++) {
    for (const f of [js, rust]) { f.crew.intent.aim = new THREE.Vector3(0, 0, 1); f.vs.update(1 / 60); }
    assert.equal(rust.v.seats[0].weapons[0].state.lockTargetId, js.v.seats[0].weapons[0].state.lockTargetId);
  }
  assert.equal(js.v.seats[0].weapons[0].state.lockTargetId, -1);
});


test('F2 flight assists locked pilot missiles and retains native flight tuning', async () => {
  const def = weaponDefinition('fixture-flight-missile', 0, { ordnance: { class: 'fixture-flight-round', base: 'missile', Velocity: 50, LifeSpan: 2 } });
  const js = await fixture(false, def, { flying: true, tow: true, network: true }), rust = await fixture(true, def, { flying: true, tow: true, network: true });
  for (let tick = 0; tick < 90; tick++) {
    for (const f of [js, rust]) {
      f.crew.intent.buttons[0] = 1; f.crew.intent.aim = new THREE.Vector3(.05, 0, 1); f.vs.update(1 / 60); f.o.update(1 / 60, false);
    }
    assert.deepEqual(rust.shots, js.shots, `pilot shot ${tick}`);
    assert.deepEqual(rust.o.rounds.map(r => [r.position.toArray(), r.velocity.toArray(), r.life, r.targetId, r.targetGeneration, r.flightMissile, r.flightClass]),
      js.o.rounds.map(r => [r.position.toArray(), r.velocity.toArray(), r.life, r.targetId, r.targetGeneration, r.flightMissile, r.flightClass]), `pilot missile ${tick}`);
  }
  assert.ok(js.shots.length > 0); assert.ok(js.shots.some(s => s[4] === js.target.aiTarget.id));
  assert.ok(js.shots.some(s => s[10] !== 0), 'the assist bends shots vertically toward the target');
});

// A new warning used to intern its text on the first high-altitude tick.
// Reach that transition after warming the actual scheduler, not a text helper.
test('warm F2 flight reaches a new warning without allocating native storage', async () => {
  const f = await fixture(true, weaponDefinition('fixture-flight-warning'),
    { flying: true, tow: true, network: true, environment: { ceiling: 15 } });
  const keys = { forward: 1, right: 0, stickX: 0, stickY: 0 };
  f.crew.intent.flight = keys;
  let ceiling = false;
  try {
    for (let tick = 0; tick < 600; tick++) {
      if (tick === 200) keys.stickY = .2;
      f.vs.update(1 / 60);
      if (tick > 60) assert.equal(f.vs.f2.x.f2_allocations(), 0, `flight allocation at ${tick}`);
      if (tick >= 200 && f.v.flightWarning === 'Flight ceiling') ceiling = true;
    }
    assert.ok(ceiling, `the warmed pilot reaches a previously unused warning: ${f.v.position.toArray()} ${f.v.flightState}`);
  } finally { f.vs.dispose(); }
});
