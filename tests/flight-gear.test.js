// A flyer's landing gear, gear doors and S-foils: its bank's takeoff clip
// posed at the flight state (src/flight/gear.js), the gear its own on the
// host that steps or corrects its flight and its replicated flight state's
// on a page that only has its records. Synthetic rigs and clips only; the
// retail clips' poses are in docs/vehicles.md "Landing gear".
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { loadClips } from '../src/anim.js';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles } from '../src/vehicles.js';
import { poseFlightGear, shownGear, followFlightState, gearFoot } from '../src/flight/gear.js';
import { heightAbove } from '../src/flight/model.js';
import { modelPack } from '../src/models.js';
import { vehicleBatches } from '../src/vehicle-rig.js';
import { EntitySeats } from '../src/runtime/entity-seats.js';
import { EntityGameState } from '../src/runtime/network-entity-game.js';
import { captureEntityVehicle, captureEntityMount } from '../src/runtime/network-entity-capture.js';
import { FIXTURE_SPEEDS, FIXTURE_HANDLING, FIXTURE_CAMERA } from './flight-fixture.js';
import { createSnapshotSender } from './snapshot-send.js';
import { PredictedShots } from '../src/runtime/predicted-shots.js';

const X = new THREE.Vector3(1, 0, 0), Z = new THREE.Vector3(0, 0, 1);
const turn = (axis, angle) => new THREE.Quaternion().setFromAxisAngle(axis, angle).toArray();
// A clip's samples per track, frame by frame: [qx, qy, qz, qw, tx, ty, tz].
function takeoffClips(names, tracks, frames) {
  const floats = new Float32Array(tracks.length * frames * 7);
  tracks.forEach((track, k) => { for (let f = 0; f < frames; f++) floats.set(track.at(f / (frames - 1)), (k * frames + f) * 7); });
  return { meta: { fps: 30, bones: names, clips: { takeoff: { frames, flags: 0, offset: 0, tracks: tracks.map((t) => t.bone), travel: [0, 0, 0] } } }, floats };
}
const lerp = (a, b, u) => a.map((x, i) => x + (b[i] - x) * u);
const slerp = (a, b, u) => new THREE.Quaternion().fromArray(a).slerp(new THREE.Quaternion().fromArray(b), u).toArray();
// As the retail Jedi starfighter's clip has it: DummyRoot lifted throughout
// (the original seats its model on its origin by it), the gear leg folded
// up 90 degrees by the last frame, its door open 100 degrees at the first.
const RIG = [
  { name: 'DummyRoot', parent: -1, at: [0, 0, 0] },
  { name: 'bone_gear', parent: 0, at: [0, -0.2, 1] },
  { name: 'bone_door', parent: 0, at: [0.3, -0.1, 1] },
  { name: 'hp_fire', parent: 0, at: [0, 0, 2] },
];
const ID = [0, 0, 0, 1];
const TRACKS = [
  { bone: 0, at: () => [...ID, 0, 0.75, 0] },
  { bone: 1, at: (u) => [...slerp(ID, turn(X, Math.PI / 2), u), 0, -0.2, 1] },
  { bone: 2, at: (u) => [...slerp(turn(Z, 1.75), ID, u), 0.3, -0.1, 1] },
];
const box = (x, y, z, w, h, d) => {
  const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
};
// A model pack in the extractor's layout (models.bin: eight floats a vertex,
// position, normal and uv; models_index.bin: uint32 indices into it): each
// part { bone, positions (in its bone's frame, or the model's with joints),
// indices, joints }.
function modelParts(list) {
  const vertices = list.reduce((n, p) => n + p.positions.length / 3, 0), elements = list.reduce((n, p) => n + p.indices.length, 0);
  const floats = new Float32Array(vertices * 8), indices = new Uint32Array(elements), parts = [];
  let at = 0, index = 0;
  for (const p of list) {
    const count = p.positions.length / 3;
    parts.push({ bone: p.bone, joints: p.joints, texture: 'none', flags: 1, vertexOffset: at * 32, vertexCount: count, indexOffset: index * 4, indexCount: p.indices.length });
    for (let k = 0; k < count; k++) { floats.set(p.positions.slice(k * 3, k * 3 + 3), (at + k) * 8); floats[(at + k) * 8 + 4] = 1; }
    for (let k = 0; k < p.indices.length; k++) indices[index + k] = p.indices[k] + at;
    at += count; index += p.indices.length;
  }
  return { parts, buffer: floats.buffer, indexBuffer: indices.buffer };
}
// The hull (as its collision box) and a gear leg 1 m long hanging from
// bone_gear: straight down at the clip's first frame, so its foot is 1.2 m
// under the origin with the gear down.
const HULL = { bone: 'DummyRoot', positions: Array.from(box(0, 1, 0, 2, 2, 4).positions), indices: Array.from(box(0, 1, 0, 2, 2, 4).indices) };
const LEG = { bone: 'bone_gear', positions: [0, 0, 0, .1, 0, 0, 0, -1, 0], indices: [0, 1, 2] };
// A real flyer of the shared Vehicles system with that rig and clip (or none),
// and its model's parts (none: no geometry, so no gear to stand on).
function craft({ clip = true, parts = [], rig = RIG, tracks = TRACKS, collisionBox = [0, 1, 0, 2, 2, 4], weapons = [] } = {}) {
  const floor = box(0, -1, 0, 20000, 2, 20000);
  const collision = new CollisionWorld(floor.positions, floor.indices, floor.polygons);
  const bones = rig.map((b) => ({ name: b.name, parent: b.parent, local: new THREE.Matrix4().makeTranslation(...b.at).toArray() }));
  const names = rig.map((b) => b.name);
  const { meta, floats } = takeoffClips(names, tracks, 31);
  const model = modelParts(parts);
  const data = { models: { ship: { bones, parts: model.parts } }, collision: { ship: box(...collisionBox) },
    classes: { ship: { model: 'ship', label: 'ship', base: 'flyer', settings: { ...FIXTURE_SPEEDS, ...FIXTURE_HANDLING }, seats: [{ name: 'BODY', settings: FIXTURE_CAMERA, weapons }], explosion: null } },
    animations: clip ? { ship: meta } : {},
    spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1, post: 'cp', respawnTime: 2, classes: { 1: 'ship' } }] };
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer: model.buffer, indexBuffer: model.indexBuffer, animationBuffer: floats.buffer, collision, texture: () => null,
    camera: new THREE.PerspectiveCamera(50, 16 / 9, .1, 2000), posts: { posts: [{ desc: { name: 'cp' }, team: 1 }] }, ordnance: { explosion() {} }, effects: { play() { return null; } } });
  return { system, v: system.vehicles[0] };
}
const bone = (v, name) => v.bones[v.bones.findIndex((b) => b.name === name)];
const angleOf = (b, q) => b.quaternion.angleTo(new THREE.Quaternion().fromArray(q));
const run = (f, n, input = {}) => { for (let i = 0; i < n; i++) f.system.update(1 / 60, f.v, input); };

test('a flyer is drawn in its takeoff clip at its gear: down landed, stowed flying, DummyRoot left where the hull is seated', () => {
  const { v } = craft();
  assert.equal(v.pose, null, 'a flyer runs none of the walker clip machinery');
  assert.ok(v.clips.get('takeoff'), 'the bank\'s takeoff clip loads with the class');
  const gear = bone(v, 'bone_gear'), door = bone(v, 'bone_door'), root = bone(v, 'DummyRoot');
  for (const [state, value, u] of [['Landed', 1, 0], ['Flying', 0, 1], ['TakingOff', .5, .5], ['Landing', .25, .75]]) {
    v.flightState = state; v.gear = value;
    v.render(1, 0);
    assert.ok(angleOf(gear, slerp(ID, turn(X, Math.PI / 2), u)) < 1e-3, `${state}: the leg at ${u} of the clip`);
    assert.ok(angleOf(door, slerp(turn(Z, 1.75), ID, u)) < 1e-3, `${state}: the door at ${u} of the clip`);
    assert.deepEqual(root.position.toArray(), [0, 0, 0], 'the clip\'s constant lift is left out');
    assert.ok(root.quaternion.angleTo(new THREE.Quaternion()) < 1e-9);
    assert.deepEqual(gear.position.toArray().map((n) => +n.toFixed(5)), [0, -0.2, 1], 'the leg folds about its own joint, never slides through the hull');
  }
  // In the world: up off the pad, over to flight, and down again.
  const f = { system: v.system, v };
  v.driver = {}; v.seatIndex = 0; v.gear = 1; v.flightState = 'Landed';
  run(f, 1, { takeoff: true });
  assert.equal(v.flightState, 'TakingOff');
  run(f, 30);
  assert.equal(v.gear, 1, 'the gear stays down through the lift');
  assert.ok(angleOf(door, turn(Z, 1.75)) < 1e-3, 'doors open on the way up');
  let half = false;
  for (let i = 0; i < 400 && v.flightState !== 'Flying'; i++) {
    run(f, 1);
    if (!half && v.gear <= .5) { half = true; assert.ok(Math.abs(angleOf(gear, ID) - Math.PI / 2 * (1 - v.gear)) < 1e-2, 'the leg folds as the gear goes up'); }
  }
  assert.ok(half && v.flightState === 'Flying');
  assert.ok(angleOf(gear, turn(X, Math.PI / 2)) < 1e-3 && angleOf(door, ID) < 1e-3, 'stowed and shut in flight');
  run(f, 1, { takeoff: true });
  assert.equal(v.flightState, 'Landing');
  for (let i = 0; i < 1200 && v.flightState === 'Landing'; i++) run(f, 1);
  assert.equal(v.flightState, 'Landed');
  assert.ok(angleOf(gear, ID) < 1e-3 && angleOf(door, turn(Z, 1.75)) < 1e-3, 'down and open again on the ground');
});

test('a craft without a takeoff clip keeps its modelled pose in flight (nothing pushed through its hull)', () => {
  const { v } = craft({ clip: false });
  const before = v.bones.map((b) => [...b.position.toArray(), ...b.quaternion.toArray()]);
  for (const gear of [1, .5, 0]) {
    v.flightState = gear ? 'TakingOff' : 'Flying'; v.gear = gear;
    poseFlightGear(v, 1 / 60);
    assert.deepEqual(v.bones.map((b) => [...b.position.toArray(), ...b.quaternion.toArray()]), before);
  }
});

test('a craft drawn from its records follows its flight state: held through the lift, stowed over TakeoffTime, down over the landing', () => {
  const { v } = craft();
  const c = v.flightConfig, dt = 1 / 60;
  v.gear = 1; // never stepped on this host: its own gear says nothing
  v.flightState = 'Flying';
  followFlightState(v, true);
  assert.equal(shownGear(v, dt), 0, 'first seen in flight: stowed at once');
  v.flightState = 'Landing';
  for (let t = 0; t < c.landingTime / 2 - 1e-9; t += dt) shownGear(v, dt);
  const half = shownGear(v, 0);
  assert.ok(Math.abs(half - .5) < .05, `half way down half way through the landing (${half})`);
  for (let t = 0; t < c.landingTime; t += dt) shownGear(v, dt);
  assert.equal(shownGear(v, 0), 1);
  v.flightState = 'Landed';
  assert.equal(shownGear(v, dt), 1);
  // The lift: on the pad the gear stays down, however long.
  v.position.set(0, .5, 0);
  v.flightState = 'TakingOff';
  for (let i = 0; i < 120; i++) shownGear(v, dt);
  assert.equal(shownGear(v, 0), 1, 'held down while the craft is under TakeoffHeight');
  v.position.y = c.takeoffHeight + c.landedHeight + 1;
  for (let i = 0; i < 30; i++) shownGear(v, dt);
  assert.ok(Math.abs(shownGear(v, 0) - (1 - 30 * dt / c.takeoffTime)) < 1e-9, 'stows at 1/TakeoffTime once up');
  v.flightState = 'Flying';
  for (let i = 0; i < 120; i++) shownGear(v, dt);
  assert.equal(shownGear(v, 0), 0);
  // A new life starts from the hull's own state; a handover from a replay
  // starts where the replay left the gear.
  v.aiTarget.generation += 1; v.gear = 1;
  assert.equal(shownGear(v, dt), 1, 'a new life is not drawn with the old one\'s gear');
  followFlightState(v, false); v.gear = .3;
  assert.equal(shownGear(v, dt), .3, 'his own craft: its replayed gear');
  v.flightState = 'Flying';
  followFlightState(v, true);
  assert.equal(shownGear(v, 0), .3, 'given back to its records where his replay left it');
  assert.ok(shownGear(v, dt) < .3);
});

test('over the entity protocol a craft is drawn landed, stowing and stowed as the world flies it; the pilot\'s own keeps his replayed gear', () => {
  const server = craft(), client = craft();
  const hull = server.v; hull.aiTarget.id = 40;
  const registry = { contentEpoch: 1, classes: new Map([[2, 'ship']]), weapons: new Map(), inputCodes: [], bindingProfiles: new Map([[0, {}]]) };
  const units = { byIdMap: new Map(), byId() { return null; }, all() { return []; } };
  const game = { ordnance: { clearProjectilePresentation() {} }, playerUnit: { id: 1, generation: 1, seat: null }, playerTeam: 1, units, vehicles: client.system, turretSeats: { turrets: [] }, live: { buildings: [] },
    driver: null, gunner: null, factory: { build() { throw Error('unexpected body build'); } }, bodies: new Map(), adopt() {}, setHumanTeam() {} };
  game.client = { seats: new EntitySeats(game) };
  const adapter = new EntityGameState(game, registry); adapter.baseline();
  const sender = createSnapshotSender(adapter);
  let tick = 10;
  const send = () => { tick += 1; sender.send(tick, [captureEntityVehicle(hull, { tick, classId: 2 }), captureEntityMount(hull, { tick, slot: 0 })]); };
  const shown = () => client.system.vehicles.find((x) => x.aiTarget.id === 40);
  const f = { system: server.system, v: hull };
  hull.driver = {}; hull.seatIndex = 0;
  send();
  const remote = shown();
  assert.ok(remote, 'the client hull takes the server identity');
  assert.equal(shownGear(remote, 1 / 60), 1, 'parked: down');
  run(f, 1, { takeoff: true }); send();
  let stowing = null;
  for (let i = 0; i < 400 && hull.flightState !== 'Flying'; i++) {
    run(f, 1); send();
    const drawn = shownGear(remote, 1 / 60);
    // The world tests the height the tick starts at; a record has where it
    // ended, so a page may start stowing one tick ahead.
    if (hull.gear === 1) assert.ok(drawn >= 1 - 1 / 60 / hull.flightConfig.takeoffTime - 1e-9, `down through the lift, as in the world (${drawn})`);
    else if (stowing === null) stowing = drawn;
  }
  assert.ok(stowing !== null && stowing < 1, 'stowing once the world has it stowing');
  for (let i = 0; i < 90; i++) { run(f, 1); send(); shownGear(remote, 1 / 60); }
  assert.equal(remote.flightState, 'Flying');
  assert.equal(shownGear(remote, 1 / 60), 0, 'stowed in flight');
  // The bones as a page draws it, from the frame's dt.
  adapter.present(tick); client.system.present(1, 1 / 60);
  assert.ok(angleOf(bone(remote, 'bone_gear'), turn(X, Math.PI / 2)) < 1e-3, 'its leg drawn folded');
  // A craft in flight first seen now (a client joining) is drawn stowed at once.
  const late = craft(), lateGame = { ...game, vehicles: late.system };
  lateGame.client = { seats: new EntitySeats(lateGame) };
  const lateAdapter = new EntityGameState(lateGame, registry); lateAdapter.baseline();
  createSnapshotSender(lateAdapter).send(tick, [captureEntityVehicle(hull, { tick, classId: 2 }), captureEntityMount(hull, { tick, slot: 0 })]);
  assert.equal(shownGear(late.system.vehicles.find((x) => x.aiTarget.id === 40), 1 / 60), 0);
  // His own: the records leave its exact replay state, gear included, alone.
  adapter.ownsMountAim = (vehicle, seat) => vehicle === remote && seat === 0;
  remote.gear = .42; run(f, 1); send();
  assert.equal(shownGear(remote, 1 / 60), .42);
});

// Where the leg's foot is drawn (its tip under bone_gear), and the lowest
// corner of the hull, in the world, as the scene graph poses them.
const legFoot = (v) => { v.root.updateMatrixWorld(true); return new THREE.Vector3(0, -1, 0).applyMatrix4(bone(v, 'bone_gear').matrixWorld); };

test('a craft with gear stands on its feet where it spawns and where it lands; one without rests as before', () => {
  const f = craft({ parts: [HULL, LEG] }), v = f.v;
  assert.ok(Math.abs(v.gearFoot - 1.2) < 1e-6, `the foot 1.2 m under the origin with the gear down (${v.gearFoot})`);
  assert.ok(Math.abs(v.position.y - v.gearFoot) < 1e-9, 'spawned with its origin the foot\'s depth over the pad');
  assert.ok(Math.abs(legFoot(v).y) < 1e-6, `its foot drawn on the pad (${legFoot(v).y})`);
  assert.ok(v.position.y + v.bounds.min.y > 0, 'its hull clear of the pad');
  v.driver = {}; v.seatIndex = 0;
  run(f, 1, { takeoff: true });
  for (let i = 0; i < 400 && v.flightState !== 'Flying'; i++) run(f, 1);
  assert.equal(v.flightState, 'Flying');
  run(f, 1, { takeoff: true });
  assert.equal(v.flightState, 'Landing');
  run(f, 150);
  assert.equal(v.flightState, 'Landed');
  assert.ok(Math.abs(v.position.y - v.gearFoot) < 1e-9, `landed on its feet, not 0.15 m over its hull bottom (${v.position.y})`);
  assert.ok(Math.abs(legFoot(v).y) < 1e-6, 'its foot on the ground again');
  // Without a takeoff clip nothing changes: 0.5 m (its hover) over its
  // collision where it spawns, 0.15 m after a landing.
  const plain = craft({ clip: false, parts: [HULL, LEG] }), p = plain.v;
  assert.equal(p.gearFoot, null);
  assert.equal(p.position.y, .5);
  p.driver = {}; p.seatIndex = 0;
  run(plain, 1, { takeoff: true });
  for (let i = 0; i < 400 && p.flightState !== 'Flying'; i++) run(plain, 1);
  run(plain, 1, { takeoff: true }); run(plain, 150);
  assert.equal(p.flightState, 'Landed');
  assert.ok(Math.abs(p.position.y - .15) < 1e-9);
});

test('a craft whose gear stands above its origin (the MAF\'s) rests with its origin in the pad, and still lifts TakeoffHeight', () => {
  // Hull and collision from 0.3 m over the origin up; the foot 0.29 m over it.
  const rig = RIG.map((b) => (b.name === 'bone_gear' ? { ...b, at: [0, 1.29, 1] } : b));
  const tracks = TRACKS.map((t) => (t.bone === 1 ? { bone: 1, at: (u) => [...slerp(ID, turn(X, Math.PI / 2), u), 0, 1.29, 1] } : t));
  const hull = { bone: 'DummyRoot', positions: Array.from(box(0, 1.15, 0, 2, 1.7, 4).positions), indices: Array.from(box(0, 1.15, 0, 2, 1.7, 4).indices) };
  const f = craft({ parts: [hull, LEG], rig, tracks, collisionBox: [0, 1.15, 0, 2, 1.7, 4] }), v = f.v;
  assert.ok(Math.abs(v.gearFoot + .29) < 1e-6, `the foot 0.29 m above the origin (${v.gearFoot})`);
  assert.ok(Math.abs(v.position.y - v.gearFoot) < 1e-9 && v.position.y < -.28, `its origin 0.29 m into the pad (${v.position.y})`);
  assert.ok(Math.abs(legFoot(v).y) < 1e-6, 'its foot on the pad');
  assert.ok(Math.abs(heightAbove(v) + .29) < 1e-6, `its height found from over its foot (${heightAbove(v)})`);
  v.driver = {}; v.seatIndex = 0;
  run(f, 1, { takeoff: true });
  assert.equal(v.flightState, 'TakingOff');
  let over = null;
  for (let i = 0; i < 600 && v.flightState !== 'Flying'; i++) { run(f, 1); if (over === null && v.gear < 1) over = v.position.y; }
  assert.equal(v.flightState, 'Flying');
  const c = v.flightConfig;
  assert.ok(over > c.takeoffHeight + c.landedHeight - .1, `the gear held down until it is TakeoffHeight up (${over})`);
  run(f, 1, { takeoff: true });
  assert.equal(v.flightState, 'Landing', `${v.flightWarning} ${v.landingReason} y ${v.position.y}`);
  for (let i = 0; i < 1200 && v.flightState === 'Landing'; i++) run(f, 1);
  assert.equal(v.flightState, 'Landed');
  assert.ok(Math.abs(v.position.y - v.gearFoot) < 1e-9, 'down on its foot again');
});

test('gearFoot skins a part on its joints as a page draws it, the same on every host', () => {
  const rig = { bones: RIG.map((b) => ({ name: b.name, parent: b.parent, local: new THREE.Matrix4().makeTranslation(...b.at).toArray() })) };
  const names = rig.bones.map((b) => b.name);
  const { meta, floats } = takeoffClips(names, TRACKS, 31);
  const clip = loadClips(meta, floats.buffer, names).get('takeoff');
  // The leg in the model's frame on bone_gear's joint (a skinned part): its
  // foot at (0, -1.2, 1) bound, straight down at the first frame.
  const model = modelParts([{ bone: 'DummyRoot', joints: [1, 1, 1], positions: [0, -.2, 1, .1, -.2, 1, 0, -1.2, 1], indices: [0, 1, 2] }]);
  const parts = modelPack({ ship: model.parts }, model.buffer, model.indexBuffer).get('ship');
  const foot = gearFoot(rig, vehicleBatches(rig, parts, () => null, { dispose: false }), clip);
  assert.ok(Math.abs(foot - 1.2) < 1e-6, String(foot));
  assert.equal(gearFoot(rig, vehicleBatches(rig, parts, () => null, { dispose: false }), clip), foot, 'computed again, the same number');
  assert.equal(gearFoot(rig, [], clip), null, 'no geometry, no gear to stand on');
  assert.equal(gearFoot(rig, vehicleBatches(rig, parts, () => null), null), null, 'no clip, none either');
});

test('a craft whose collision reaches down to its feet (the Y-wing\'s) takes off and lands on them', () => {
  const f = craft({ parts: [HULL, LEG], collisionBox: [0, .4, 0, 2, 3.2, 4] }), v = f.v;
  assert.ok(Math.abs(v.bounds.min.y + 1.2) < 1e-6 && Math.abs(v.gearFoot - 1.2) < 1e-6, 'its collision ends at its feet');
  assert.ok(Math.abs(legFoot(v).y) < 1e-6 && Math.abs(v.position.y + v.bounds.min.y) < 1e-6, 'feet and collision on the pad');
  v.driver = {}; v.seatIndex = 0;
  run(f, 1, { takeoff: true });
  assert.equal(v.flightState, 'TakingOff', v.flightWarning);
  for (let i = 0; i < 400 && v.flightState !== 'Flying'; i++) run(f, 1);
  run(f, 1, { takeoff: true });
  assert.equal(v.flightState, 'Landing', `not refused for the ground it lands on (${v.flightWarning})`);
  run(f, 150);
  assert.equal(v.flightState, 'Landed');
  assert.ok(Math.abs(v.position.y - v.gearFoot) < 1e-6 && Math.abs(legFoot(v).y) < 1e-6, `landed at ${v.position.y}, foot ${legFoot(v).y}`);
});

test('the world and a page stand a craft on the same feet: the same foot from the same data, its parked height from the world', () => {
  const server = craft({ parts: [HULL, LEG] }), client = craft({ parts: [HULL, LEG] });
  assert.equal(client.v.gearFoot, server.v.gearFoot, 'every host computes the same foot');
  const hull = server.v; hull.aiTarget.id = 40;
  const registry = { contentEpoch: 1, classes: new Map([[2, 'ship']]), weapons: new Map(), inputCodes: [], bindingProfiles: new Map([[0, {}]]) };
  const game = { ordnance: { clearProjectilePresentation() {} }, playerUnit: { id: 1, generation: 1, seat: null }, playerTeam: 1, units: { byIdMap: new Map(), byId() { return null; }, all() { return []; } },
    vehicles: client.system, turretSeats: { turrets: [] }, live: { buildings: [] }, driver: null, gunner: null, factory: { build() { throw Error('unexpected body build'); } }, bodies: new Map(), adopt() {}, setHumanTeam() {} };
  game.client = { seats: new EntitySeats(game) };
  const adapter = new EntityGameState(game, registry); adapter.baseline();
  const sender = createSnapshotSender(adapter);
  let tick = 10;
  const send = () => { tick += 1; sender.send(tick, [captureEntityVehicle(hull, { tick, classId: 2 }), captureEntityMount(hull, { tick, slot: 0 })]); };
  const shown = () => client.system.vehicles.find((x) => x.aiTarget.id === 40);
  send();
  // Records carry positions in 1/256 m steps (the entity codec).
  assert.ok(Math.abs(shown().position.y - hull.position.y) < 2e-3 && Math.abs(hull.position.y - hull.gearFoot) < 1e-9, `parked on its feet in both (${shown().position.y}, ${hull.position.y})`);
  const f = { system: server.system, v: hull };
  hull.driver = {}; hull.seatIndex = 0;
  run(f, 1, { takeoff: true }); send();
  for (let i = 0; i < 400 && hull.flightState !== 'Flying'; i++) { run(f, 1); send(); }
  run(f, 1, { takeoff: true }); send();
  for (let i = 0; i < 150 && hull.flightState !== 'Landed'; i++) { run(f, 1); send(); }
  run(f, 1); send();
  assert.equal(shown().flightState, 'Landed');
  assert.ok(Math.abs(shown().position.y - hull.gearFoot) < 2e-3 && Math.abs(hull.position.y - hull.gearFoot) < 1e-9, `landed on its feet on the page too (${shown().position.y})`);
});

// An X-wing's laser in outline: an S-foil hinged at (0.6, 0.3, -1.75) that
// the takeoff clip turns 15 degrees about its fore/aft axis by its last frame,
// and the laser's hardpoint (its aimer) out on the foil's tip.
const FOIL_RIG = [
  { name: 'DummyRoot', parent: -1, at: [0, 0, 0] },
  { name: 'xwing_bl_hatch3', parent: 0, at: [.6, .3, -1.75] },
  { name: 'hp_laser_01', parent: 1, at: [4.36, .02, 6.79] },
];
const OPEN = THREE.MathUtils.degToRad(15);
const FOIL_TRACKS = [{ bone: 1, at: (u) => [...slerp(ID, turn(Z, OPEN), u), .6, .3, -1.75] }];
const LASER = { class: 'all_weap_fly_xwing_cannon', aimers: [{ node: 'hp_laser_01' }], ammo: 0,
  def: { class: 'all_weap_fly_xwing_cannon', base: 'cannon', channel: 0, ShotDelay: .1, ordnance: { base: 'bolt', Velocity: 100, LifeSpan: 1 } } };
// Where the tip is with the foil turned by `angle`, the hull at `at`, level.
const tip = (at, angle) => new THREE.Vector3(4.36, .02, 6.79).applyAxisAngle(Z, angle).add(new THREE.Vector3(.6, .3, -1.75)).add(at);

test('an X-wing\'s lasers leave the foil tips where its gear has the foils, in the world and in its pilot\'s predicted shots', () => {
  const world = craft({ rig: FOIL_RIG, tracks: FOIL_TRACKS, weapons: [LASER] }), v = world.v;
  const page = craft({ rig: FOIL_RIG, tracks: FOIL_TRACKS, weapons: [LASER] }).v;
  v.driver = {}; v.seatIndex = 0;
  const w = v.seats[0].weapons[0], muzzle = new THREE.Vector3();
  for (const gear of [1, .5, 0]) {
    v.gear = gear; v.flightState = gear === 1 ? 'Landed' : gear === 0 ? 'Flying' : 'TakingOff';
    v.aimTick(1, 1 / 60);
    v.muzzleOfSeat(0, w, muzzle);
    const angle = gear === .5 ? OPEN / 2 : gear === 1 ? 0 : OPEN;
    assert.ok(muzzle.distanceTo(tip(v.position, angle)) < 1e-5, `world, gear ${gear}: from the tip with the foil ${THREE.MathUtils.radToDeg(angle)} degrees open (${muzzle.distanceTo(tip(v.position, angle))})`);
    // The pilot's page: his craft drawn at its replayed gear, his shot drawn
    // from its drawn tip, which is where the world's round leaves.
    page.position.copy(v.position); page.quaternion.copy(v.quaternion); page.gear = gear; page.flightState = 'Flying';
    page.captureTick(); page.sleeping = false; page.present(1, 1 / 60);
    const shots = new PredictedShots(new THREE.Scene(), { cast: () => null }, () => 0, null);
    const unit = { id: 8, generation: 2, alive: true, seat: { vehicle: page, index: 0 }, combat: { dead: false, origin: new THREE.Vector3(), firePoint: () => null, flash() {}, animator: { fire() {} }, firstPerson: false } };
    assert.equal(shots.fire({ id: 1, generation: 2, pressed: true, button: 0, aimYaw: 0, aimPitch: 0 }, unit), true);
    const shot = [...shots.shots.values()][0];
    assert.ok(shot.origin.distanceTo(muzzle) < 1e-5, `page, gear ${gear}: the predicted bolt leaves the world's muzzle (${shot.origin.distanceTo(muzzle)})`);
  }
});
