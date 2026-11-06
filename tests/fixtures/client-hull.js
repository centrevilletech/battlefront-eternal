// Synthetic page hulls keep prediction coverage independent of imported maps.
// The same command stream covers held keys, late commands and life changes.
import './native-anim.js';
import * as THREE from 'three';
import { CollisionWorld } from '../../tools/collision-fixture.mjs';
import { Vehicle, Vehicles } from '../../src/vehicles.js';

export function hullFixture(kind, kernel) {
  const walking = kind === 'walker' || kind === 'walker-legs';
  const flying = kind === 'flyer' || kind === 'flyer-targets';
  const geometry = (w, h, d, y) => {
    const g = new THREE.BoxGeometry(w, h, d).translate(0, y, 0);
    return { positions: g.attributes.position.array, indices: g.index.array,
      polygons: new Uint32Array(g.index.count / 3) };
  };
  const floor = geometry(2000, 2, 2000, -1);
  const collision = new CollisionWorld(floor.positions, floor.indices, floor.polygons, undefined, undefined, kernel);
  const settings = { MaxHealth: 1000, ForwardSpeed: 12, ReverseSpeed: 8, StrafeSpeed: walking ? 0 : 4,
    Acceleration: 5, Deceleration: 7, TurnRate: 1.3, SetAltitude: .7,
    MinSpeed: 20, MidSpeed: 40, MaxSpeed: 72, Acceleraton: 30, TakeoffTime: 1,
    TakeoffHeight: 3, TakeoffSpeed: 3, PitchRate: .8, PitchFilter: 2.75, TurnFilter: 5.75 };
  const def = { model: 'hull', base: flying ? 'flyer' : walking ? 'walker' : 'hover',
    settings, seats: [{ name: 'BODY', settings: {}, weapons: [] }], explosion: null };
  const data = { models: { hull: { bones: [], parts: [] } }, collision: { hull: geometry(2, 2, 4, 1) },
    classes: { hull: def }, spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1,
      post: '', respawnTime: 2, classes: { 1: 'hull' } }] };
  if (kind === 'springs') def.springs = [
    { at: [-.8, .4, 1], radius: 1, length: .5, factors: [1, 1, -1, 1, -1, 1] },
    { at: [.8, .4, 1], radius: 1, length: .5, factors: [1, 1, -1, 1, 1, 1] },
    { at: [0, .8, -1], radius: 1.4, length: .5, factors: [1, 1, 2, 1, 0, 1] },
  ];
  if (kind === 'walker-legs') {
    // Real clip metadata lets both owners resolve immutable gait handles.
    // Empty tracks keep the synthetic fixture free of animation/game files.
    data.animations = { hull: { fps: 30, bones: [], clips: {
      walk_leftfoot_rightfoot: { frames: 14, flags: 0, offset: 0, tracks: [], travel: [0, 0, .8] },
      walk_rightfoot_leftfoot: { frames: 17, flags: 0, offset: 0, tracks: [], travel: [0, 0, 1.1] },
    } } };
    def.fx = { legs: { rayLength: 1.2, width: 2, orientRoll: false, pairs: [
      { left: { at: [-.7, 0, 1] }, right: { at: [.7, .2, 1] } },
      { left: { at: [-.7, .15, -1] }, right: { at: [.7, 0, -1] } },
    ] } };
  }
  const buffer = new ArrayBuffer(0), indexBuffer = new ArrayBuffer(0);
  const system = new Vehicles({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(), data, buffer,
    indexBuffer, collision, texture: () => null, ordnance: { explosion() {} }, effects: { play() {} } });
  const vehicle = system.vehicles[0]; vehicle.driver = {}; vehicle.seatIndex = 0;
  if (kind === 'wheeled') {
    vehicle.wheeled = true; vehicle.repulsor = false; vehicle.strafeSpeed = 0;
    vehicle.wheels = [-.8, .8].map(x => ({ x, radius: .35, phase: 0, previous: 0,
      tickFrom: 0, axis: new THREE.Vector3(1, 0, 0), bone: new THREE.Bone() }));
  }
  if (kind === 'flyer-targets') {
    // A friend ahead must never hide an enemy from assist/lock acquisition.
    const targets = [1, 2].map((team, i) => ({ id: i + 20, generation: 1, alive: true,
      team, position: new THREE.Vector3(i * 4, .5, 200) }));
    system.units = { all: () => targets, byId: id => targets.find(t => t.id === id),
      relation: (a, b) => a === b ? 1 : -1, remove() {} };
    system.flightAssists = true;
  }
  return { system, vehicle, data, buffer, indexBuffer, Vehicle };
}

export function hullCommand(tick, generation = 1) {
  return { tick, generation, sequence: tick, epoch: 1, sent: tick % 13 !== 0,
    down: tick < 100 ? ['KeyW'] : tick < 150 ? ['KeyS', 'KeyD'] : [],
    actions: tick === 1 ? ['takeoff'] : [], look: tick % 7 === 0 ? [[3, -2], [-1, 1]] : [],
    mouseScale: .75, invertMouse: tick % 29 === 0,
    flightSettings: { sensitivity: .8, curve: 1.3 }, flightBindings: null };
}
