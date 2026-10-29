import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles } from '../src/vehicles.js';
// A real ground hull with a pilot seat and a turreted gunner seat, built by the
// shared Vehicles system from synthetic data (no retail content). Shared by the
// mount replication tests; flight-fixture.js is the flyer equivalent.
const box = (x, y, z, w, h, d) => {
  const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
};
export function mountFixture({ classes = ['tank'], slots = 1 } = {}) {
  const floor = box(0, -1, 0, 20000, 2, 20000);
  const collision = new CollisionWorld(floor.positions, floor.indices, floor.polygons);
  const hull = box(0, 1, 0, 3, 2, 5);
  const bones = [{ name: 'dummyroot', parent: -1, local: new THREE.Matrix4().toArray() },
    { name: 'turret_gun', parent: 0, local: new THREE.Matrix4().makeTranslation(0, 1.5, 0).toArray() },
    { name: 'hp_fire', parent: 1, local: new THREE.Matrix4().makeTranslation(0, 0, 2).toArray() }];
  const seats = [
    { name: 'PILOT', settings: { EyePointOffset: '0 1 1', TrackCenter: '0 2 -4', TrackOffset: '0 2 6' }, weapons: [] },
    { name: 'GUNNER', settings: { EyePointOffset: '0 2 0', TrackCenter: '0 2 -4', TrackOffset: '0 2 6', TurretNodeName: 'turret_gun', FirePointName: 'hp_fire', YawLimits: '-180 180', PitchLimits: '-20 40' }, weapons: [] },
  ];
  const data = { models: { tank: { bones, parts: [] } }, collision: { tank: hull },
    classes: Object.fromEntries(classes.map(name => [name, { model: 'tank', label: name, base: 'hover', settings: {}, seats, explosion: null }])),
    spawns: Array.from({ length: slots }, (_, i) => ({ name: `pad${i}`, matrix: new THREE.Matrix4().makeTranslation(i * 20, 0, 0).toArray(), team: 1, post: 'cp', respawnTime: 2, classes: { 1: classes[0] } })) };
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0), collision, texture: () => null,
    camera: new THREE.PerspectiveCamera(50, 16 / 9, .1, 2000), posts: { posts: [{ desc: { name: 'cp' }, team: 1 }] }, ordnance: { explosion() {} }, effects: { play() { return null; } } });
  const v = system.variant(system.slots[0], classes[0]);
  v.spawn(1);
  return { collision, system, v };
}
