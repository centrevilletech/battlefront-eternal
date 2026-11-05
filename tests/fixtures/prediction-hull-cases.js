import * as THREE from 'three';
import { CollisionWorld } from '../../src/collision.js';
import { Vehicles } from '../../src/vehicles.js';
import { captureGroundCorrection } from '../../src/runtime/network-entity-ground.js';
import { captureFlightCorrection } from '../../src/runtime/network-entity-flight.js';
import { fixture as flightFixture } from '../flight-fixture.js';

const box = (w, h, d, y = 0) => {
  const g = new THREE.BoxGeometry(w, h, d).translate(0, y, 0);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
};

// Both synthetic and imported hulls use the same flat collision world. The
// imported model, class, suspension and animation data remain unmodified.
export function hullFromPack(data, buffers, name, options = {}) {
  const floor = box(20000, 2, 20000, -1);
  const collision = new CollisionWorld(floor.positions, floor.indices, floor.polygons, undefined, undefined, options.rays || null);
  const system = new Vehicles({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(50, 16 / 9, .1, 2000), collision,
    data: { ...data, blockers: null, spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1, post: '', classes: { 1: name } }] },
    ...buffers, texture: () => null, effects: { play() { return null; } }, ordnance: { explosion() {} },
    flightEnvironment: options.environment || {} });
  const v = system.vehicles[0]; v.driver = {}; v.seatIndex = 0; v.captureTick();
  return { system, v, c: collision };
}

export function syntheticHull(kind = 'hover', options = {}) {
  let result;
  if (kind === 'flyer') result = flightFixture(undefined, 'ship', options.environment || {});
  else {
    const name = kind === 'wheeled' ? 'test_hailfire' : kind;
    const settings = { MaxHealth: 1000, ForwardSpeed: 12, ReverseSpeed: 6, StrafeSpeed: ['hover', 'springs'].includes(kind) ? 5 : 0,
      HealthType: kind === 'creature' ? 'animal' : 'vehicle', WaterDamageInterval: .05, WaterDamageAmount: 10 };
    const def = { model: 'hull', base: ['walker', 'creature'].includes(kind) ? 'walker' : 'hover', settings,
      seats: [{ name: 'BODY', settings: {}, weapons: [] }, { name: 'GUNNER', settings: { TurnRate: 1.25, PitchRate: .7 }, weapons: [] }], explosion: null };
    if (kind === 'springs') {
      Object.assign(settings, { GravityScale: 4, Traction: 20, VelocitySpring: 5, VelocityDamp: 2.5, OmegaXSpring: 3,
        OmegaXDamp: 3, OmegaZSpring: 5, OmegaZDamp: 1.5, LevelSpring: 2, LevelDamp: 1 });
      def.springs = [{ at: [-.8, .4, 1], radius: 1, length: .5, factors: [1, 1, -1, 1, -1, 1] },
        { at: [.8, .4, 1], radius: 1, length: .5, factors: [1, 1, -1, 1, 1, 1] },
        { at: [0, .8, -1], radius: 1.4, length: .5, factors: [1, 1, 2, 1, 0, 1] }];
    }
    result = hullFromPack({ models: { hull: { bones: [], parts: [] } }, collision: { hull: box(2, 2, 4, 1) }, classes: { [name]: def } },
      { buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0) }, name, options);
    if (kind === 'wheeled') result.v.wheels = [-.8, .8].map(x => ({ x, radius: .35, phase: 0, previous: 0, tickFrom: 0,
      axis: new THREE.Vector3(1, 0, 0), bone: new THREE.Bone() }));
    if (result.v.walking) {
      result.v.pose = {}; result.v.clips = new Map([
        ['walk_leftfoot_rightfoot', { duration: .45, stride: .8 }], ['walk_rightfoot_leftfoot', { duration: .55, stride: 1.1 }],
      ]);
    }
  }
  const { v, system } = result;
  v.captureTick(); v.age = 3.25;
  if (options.legs) {
    v.def.fx = { legs: { rayLength: 1.2, width: 2, orientRoll: false, pairs: [
      { left: { at: [-.7, 0, 1] }, right: { at: [.7, .2, 1] } },
      { left: { at: [-.7, .15, -1] }, right: { at: [.7, 0, -1] } },
    ] } };
    if (options.sharedLegSide) v.legSide = [.7, -.3];
    else delete v.legSide;
  }
  if (options.disabled) v.body.enabled = false;
  if (options.deathRegion) system.inDeathRegion = () => true;
  if (options.deepWater) system.water = { level: 100, over: () => true };
  if (options.belowWorld) { v.position.y = -400; v.previous.copy(v.position); v.captureTick(); }
  if (options.crash) {
    v.flightState = 'Flying'; v.position.set(0, 1, 0); v.velocity.set(0, -70, 40); v.airspeed = 80;
    v.quaternion.setFromAxisAngle(new THREE.Vector3(1, 0, 0), Math.PI / 3); v.captureTick();
  }
  if (options.targets) {
    v.flightState = 'Flying'; v.position.set(0, 20, 0); v.velocity.set(0, 0, 50); v.airspeed = 50; v.captureTick();
    v.seats[0].weapons.push({ state: { lockTargetId: 71, lockGeneration: 6, lockElapsed: 9, lockComplete: true } });
    const target = (id, x, team, named = true) => ({ id, generation: 2, alive: true, team, position: new THREE.Vector3(x, 20, 200),
      ...(named ? { aimPoint(out) { return out.copy(this.position); } } : {}) });
    const targets = [v.aiTarget, target(21, 0, 1), target(22, -1, 2), target(23, 1, 2, false), { ...target(24, 0, 2), alive: false }];
    system.flightAssists = true;
    system.units = { all: () => targets, byId: id => targets.find(t => t.id === id), relation: (a, b) => a === b ? 1 : -1, remove() {} };
  }
  if (options.dead) v.alive = false;
  return result;
}

const primitive = value => value === null || ['number', 'boolean', 'string', 'undefined'].includes(typeof value);
const primitives = object => Object.fromEntries(Object.entries(object).filter(([, value]) => primitive(value)));
const vectorFields = 'position previous velocity quaternion rotation matrix itemCenter tickFrom supportNormal flightStart flightEnd flightTrickRight flightPreviousQ flightTickQ flightStartQ flightTrickLoopQ flightTrickEndQ flightFrameQ flightLeanQ'.split(' ');
export function hullObservation(v, history = true) {
  if (!v) return null;
  const scalar = primitives(v);
  delete scalar.predictionTick;
  return { correction: Array.from(v.flying ? captureFlightCorrection(v) : captureGroundCorrection(v)), scalar,
    vectors: Object.fromEntries(vectorFields.filter(key => v[key]?.toArray).map(key => [key, v[key].toArray()])),
    seats: v.seats.map(primitives), wheels: v.wheels.map(primitives), legSide: v.legSide ? [...v.legSide] : null,
    weaponLocks: v.seats.map(seat => seat.weapons.map(w => ['lockTargetId', 'lockGeneration', 'lockElapsed', 'lockComplete'].map(key => w.state[key]))),
    predictionTick: v.predictionTick,
    path: history ? Array.from(v.predictionPath || [], ([tick, p]) => [tick, p.toArray()]) : null,
    states: history ? Array.from(v.predictionStates || [], ([tick, bytes]) => [tick, bytes && Array.from(bytes)]) : null };
}
