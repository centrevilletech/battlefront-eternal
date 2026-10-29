import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles, VehicleDriver } from '../src/vehicles.js';
const box = (x, y, z, w, h, d) => {
  const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
};
// Unless a test says otherwise the ship flies as an X-wing's ODF has it: its
// speeds, its handling and its lift off a pad (the body section's keys), and
// its pilot's chase camera.
export const FIXTURE_SPEEDS = { MinSpeed: '20.0', MidSpeed: '50.0', MaxSpeed: '82.0', Acceleraton: '30.0' };
export const FIXTURE_HANDLING = { PitchRate: '1.0', PitchFilter: '2.9', TurnRate: '1.25', TurnFilter: '5.75', BankAngle: '0.7', BankFilter: '2.5',
  LevelFilter: '2.0', TakeoffTime: '1.0', TakeoffSpeed: '3.0', LandingTime: '2.0', LandingSpeed: '5.0', TakeoffHeight: '2.5' };
export const FIXTURE_CAMERA = { EyePointOffset: '0 1 1', TrackCenter: '0.0 2.0 -4.0', TrackOffset: '0.0 2.0 6.0', TiltValue: '5.0', FirstPersonFOV: '52',
  '5e3171cc': '20.0', '21050bf6': '4.25' }; // AimTension and MoveTension, kept hashed as the import keeps them
export function fixture(settings = { ...FIXTURE_SPEEDS, ...FIXTURE_HANDLING }, name = 'ship', environment = {}, camera = FIXTURE_CAMERA) {
  const floor = box(0, -1, 0, 20000, 2, 20000);
  const c = new CollisionWorld(floor.positions, floor.indices, floor.polygons);
  const hull = box(0, 1, 0, 2, 2, 4);
  const d = { model: 'ship', label: 'Test ship', base: 'flyer', settings, seats: [{ name: 'BODY', settings: camera, weapons: [] }], explosion: null };
  const data = { models: { ship: { bones: [], parts: [] } }, collision: { ship: hull }, classes: { [name]: d },
    spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1, post: 'cp', respawnTime: 2, classes: { 1: name } }] };
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0), collision: c, texture: () => null,
    camera: new THREE.PerspectiveCamera(50, 16 / 9, .1, 2000), posts: { posts: [{ desc: { name: 'cp' }, team: 1 }] }, ordnance: { explosion() {} }, effects: { play() { return null; } }, flightEnvironment: environment });
  const v = system.vehicles[0];
  v.driver = {}; v.seatIndex = 0; // the player at the stick
  // His mouse: counts through VehicleDriver.look, as the page sends them.
  const driver = Object.create(VehicleDriver.prototype);
  driver.vehicle = v;
  const mouse = (dx, dy) => driver.look(dx, dy, 2048, false); // 1 count = 1 rad
  return { c, system, v, mouse };
}
