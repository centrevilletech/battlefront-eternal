import {unitOwner} from '../helpers/unit-owner.js';
import {ClassicRandom,uniformRandom} from '../../src/ai/random.js';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Vehicles, Vehicle } from '../../src/vehicles.js';
import { CollisionWorld } from '../../src/collision.js';
import { Ordnance } from '../../src/ordnance.js';
import { Units } from '../../src/registry.js';
import { Unit } from '../../src/units.js';
import { Soldier } from '../../src/soldier.js';
import { loadSim } from '../../src/sim/module.js';
import { loadRayKernel } from '../../src/ray-kernel.js';
import { SimBridge } from '../../src/sim/runtime.js';
import { attachOrdnanceSim, ordnanceTests } from '../../src/ordnance-sim.js';
import { attachF2 } from '../../src/f2-sim.js';

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

export async function mountedFleet(def, options = {}) {
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
  for (let i = 1; i < (options.seats ?? 1); i++) data.classes.hull.seats.push({ name: `TURRET${i}`, settings: {}, weapons: [] });
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
  const v = vs.vehicles[0], crew = units.add(new Crew(collision));
  if (options.flying) {
    v.flightState = 'Flying'; v.grounded = false; v.position.y = 12; v.previous.copy(v.position);
    v.matrix.setPosition(v.position); v.syncBody();
    v.flightLockId = vs.vehicles[1].aiTarget.id; v.flightLockGeneration = vs.vehicles[1].aiTarget.generation;
    v.flightLockState = 'Locked'; v.flightLockTimer = 10;
  }
  crew.networkHuman = Boolean(options.network);
  let draws = 0;
  const spread=new ClassicRandom(17),observed=new ClassicRandom(17);
  const bridge = new SimBridge(instance, { classes: { Vehicle, Unit: Crew }, tests: ordnanceTests });
  attachOrdnanceSim(o, bridge);
  attachF2(vs, bridge, { killY: -300, deathRegions: [], scripted: [] }, { data, buffer, indexBuffer }, Crew);
  unitOwner(vs,bridge);
  assert.equal(vs.board(crew, v), true);
  v.seats[0].weapons[0].state.random = uniformRandom(spread);
  const shots = [], launched = [];
  o.onLaunch = (b,context) => {
    const sample=context?.sample;
    launched.push([sample?.channel??b.networkFireChannel,...(sample?.position??b.position).toArray(),...(sample?.direction??b.direction).toArray()]);
  };
  vs.onFired = (h, seat, shot, owner, p, d) => shots.push([seat, shot.yaw, shot.pitch, shot.charge, shot.targetId, shot.targetGeneration, ...p.toArray(), ...d.toArray()]);
  return { vs, o, v, crew, shots, launched, target: vs.vehicles[1], draws: () => {while(observed.state!==spread.state) {observed.uniform();draws++;}return draws;} };
}
