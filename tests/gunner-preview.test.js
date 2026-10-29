import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Gunner, previewSteer } from '../src/gunner.js';
import { poseTurret, TurretSeats } from '../src/seats.js';
import { Turret } from '../src/props.js';

// A turret as the page sees one: a real Turret (its own steer, pose and
// aim frame) on a drum and a gun node with a mesh each, no world behind it,
// at (10, -2, 5) facing 0.3 rad. eye: its EyePointOffset; box: the built
// model's box (the eye inside it hides the drum and gun in first person).
function turret({ facing = 0.3, yaw = 0, pitch = 0, yawVel = 0, pitchVel = 0, eye = '0 1.5 1', box = [[-1, 0, -1], [1, 2.5, 1.5]], settings = {} } = {}) {
  const root = new THREE.Group();
  const drum = new THREE.Group(); drum.name = 'drum'; root.add(drum);
  drum.position.set(0, 0.6, 0);
  drum.add(new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1)));
  const gun = new THREE.Group(); gun.name = 'gun'; gun.position.set(0, 0.8, 0.6); drum.add(gun);
  gun.add(new THREE.Mesh(new THREE.BoxGeometry(0.2, 0.2, 2)));
  const matrix = new THREE.Matrix4().compose(new THREE.Vector3(10, -2, 5), new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), facing), new THREE.Vector3(1, 1, 1));
  root.matrix.copy(matrix); root.matrix.decompose(root.position, root.quaternion, root.scale);
  root.updateMatrixWorld(true);
  const desc = { base: 'armedbuilding', class: 'test_turret', matrix: matrix.toArray(), weapon: null,
    settings: { TurretNodeName: 'drum', AimerNodeName: 'gun', MaxTurnSpeed: 2.85, PitchLimits: '-45 45', AimerPitchLimits: '-57.3 57.3', AimerYawLimits: '-5.73 5.73',
      PitchTurnFactor: 0, EyePointOffset: eye, TrackCenter: '0 1.5 -2', TrackOffset: '0 0.5 1.5', TiltValue: 5, ...settings } };
  const t = new Turret(desc, { built: { root, byName: { drum, gun }, model: { box } } }, { built: null }, null);
  Object.assign(t, { yaw, pitch, yawVel, pitchVel, gunYaw: 0.05, gunPitch: -0.1 });
  return t;
}
const command = (look, extra = {}) => ({ look, mouseScale: 0.5, invertMouse: false, ...extra });
function pageGunner(t) {
  const camera = new THREE.PerspectiveCamera(); camera.aspect = 16 / 9;
  const g = new THREE.BoxGeometry(400, 1, 400).translate(0, -10, 0);
  const collision = new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
  const gunner = new Gunner({ camera, collision, ordnance: null, combat: null, animator: { gunner() {} }, clips: new Map(), hiddenLayer: 1 });
  gunner.turret = t; // as SeatReader.setGunner does: no enter()
  return gunner;
}

test('previewSteer runs the drum ahead exactly as the world steers it, without touching the turret', () => {
  const world = turret(), page = turret();
  const pending = [command([[6, 0], [2, 1]]), command([[0, -4]]), command([]), command([[8, 0]], { mouseScale: 0.25, invertMouse: true })];
  // The world: gunner.update's steering, a tick per command.
  const soldier = Object.fromEntries(['position', 'previous', 'settled', 'velocity'].map((k) => [k, new THREE.Vector3()]));
  soldier.aimYaw = 0; soldier.aimPitch = 0;
  const worldGunner = new Gunner({ camera: new THREE.PerspectiveCamera(), collision: { cast: () => null }, ordnance: null, combat: null, animator: { gunner() {} }, clips: new Map() });
  worldGunner.turret = world;
  for (const c of pending) {
    for (const [dx, dy] of c.look) worldGunner.look(dx, dy, c.mouseScale, c.invertMouse);
    worldGunner.update(1 / 60, soldier, { take: () => false, held: () => false });
  }
  const before = { ...page };
  const state = previewSteer(page, { yaw: page.yaw, pitch: page.pitch, yawVel: page.yawVel, pitchVel: page.pitchVel }, pending, null, 0.5, false);
  assert.ok(Math.abs(world.yaw) > 1e-3, 'the drum turned');
  for (const key of ['yaw', 'pitch', 'yawVel', 'pitchVel']) {
    assert.ok(Math.abs(state[key] - world[key]) < 1e-12, `${key}: preview ${state[key]} vs world ${world[key]}`);
    assert.equal(page[key], before[key], `${key} of the turret itself is unchanged`);
  }
  // Loose looks (not yet in a command) count as one more tick.
  const more = previewSteer(page, { yaw: 0, pitch: 0, yawVel: 0, pitchVel: 0 }, pending, [[6, 0]], 0.5, false);
  const same = previewSteer(page, { yaw: 0, pitch: 0, yawVel: 0, pitchVel: 0 }, [...pending, command([[6, 0]])], null, 0.5, false);
  assert.ok(Math.abs(more.yaw - same.yaw) < 1e-12);
});

test('preview sets the yaw and pitch shown over the world aim, back through the tick by alpha', () => {
  const t = turret({ yaw: 0.2, pitch: 0.1, yawVel: 0.6, pitchVel: -0.3 });
  const gunner = pageGunner(t);
  gunner.preview([], null, 0.5, false, 1);
  assert.ok(Math.abs(gunner.previewYaw) < 1e-12 && Math.abs(gunner.previewPitch) < 1e-12, 'at the tick shown: the world aim itself');
  gunner.preview([], null, 0.5, false, 0);
  assert.ok(Math.abs(gunner.previewYaw + 0.6 / 60) < 1e-12, 'at the tick before: the tick turn undone');
  assert.ok(Math.abs(gunner.previewPitch - 0.3 / 60) < 1e-12);
  t.yawVel = 0;
  gunner.preview([command([[40, 0]]), command([[40, 0]])], [], 0.5, false, 1);
  assert.ok(gunner.previewYaw < 0, 'mouse right turns the view right (yaw down), ahead of the world');
  const two = gunner.previewYaw;
  gunner.preview([command([[40, 0]]), command([[40, 0]])], [[40, 0]], 0.5, false, 1);
  assert.ok(gunner.previewYaw < two, 'the looks not yet in a command turn it further');
  assert.equal(t.yaw, 0.2, 'the turret keeps the world aim');
  gunner.turret = null;
  assert.equal(gunner.previewYaw, 0, 'leaving clears the preview');
});

test('the page camera follows a turret it was never entered on, and poses its drum and gun', () => {
  const t = turret({ yaw: -0.5, pitch: 0.15 });
  const gunner = pageGunner(t);
  const camera = gunner.camera;
  camera.position.set(100, 50, 100);
  gunner.updateCamera(1 / 60, 1, false);
  assert.ok(gunner.view, 'a view is made for the turret');
  assert.ok(camera.position.distanceTo(t.position) < 5, `the chase camera sits by the turret (${camera.position.distanceTo(t.position).toFixed(2)} m)`);
  // The drum and gun as Turret.pose would leave them for the same angles.
  const reference = turret({ yaw: -0.5, pitch: 0.15 });
  reference.pose(0, null);
  assert.ok(t.drum.quaternion.angleTo(reference.drum.quaternion) < 1e-6, 'drum posed');
  assert.ok(t.gun.quaternion.angleTo(reference.gun.quaternion) < 1e-6, 'gun posed at its own angles');
  // With a preview, the drum shows the previewed yaw.
  gunner.previewYaw = 0.3;
  gunner.updateCamera(1 / 60, 1, false);
  const previewed = turret({ yaw: -0.2 }); previewed.pose(0, null);
  assert.ok(t.drum.quaternion.angleTo(previewed.drum.quaternion) < 1e-6, 'drum at the previewed yaw');
  // Another turret: a fresh view.
  const view = gunner.view;
  gunner.turret = turret();
  gunner.updateCamera(1 / 60, 1, false);
  assert.notEqual(gunner.view, view);
});

test('first person looks from the eye down the aim, the drum and gun hidden from inside the turret, and comes back', () => {
  const t = turret({ yaw: 0.4, pitch: -0.2 });
  const gunner = pageGunner(t);
  const camera = gunner.camera;
  gunner.updateCamera(1 / 60, 1, true);
  const look = new THREE.Vector3(0, 0, -1).applyQuaternion(camera.quaternion);
  const aim = t.viewDirection(new THREE.Vector3());
  assert.ok(look.angleTo(aim) < 1e-6, 'looks down the aim');
  // EyePointOffset in the aim frame: at the drum's pivot (0.6 m up), along
  // the aim 1 m and 1.5 m up the frame's own up, which tips with the pitch.
  const yaw = t.facing + t.yaw, pitch = t.pitch;
  const up = new THREE.Vector3(-Math.sin(yaw) * Math.sin(pitch), Math.cos(pitch), -Math.cos(yaw) * Math.sin(pitch));
  const eye = new THREE.Vector3(10, -1.4, 5).addScaledVector(aim, 1).addScaledVector(up, 1.5);
  assert.ok(camera.position.distanceTo(eye) < 1e-6, `at EyePointOffset in the aim frame (${camera.position.distanceTo(eye)})`);
  const meshes = []; t.drum.traverse((o) => { if (o.isMesh) meshes.push(o); });
  assert.equal(meshes.length, 2);
  assert.ok(meshes.every((m) => m.layers.mask === 1 << 1), 'drum and gun on the hidden layer');
  assert.equal(gunner.hiddenRig, t);
  gunner.updateCamera(1 / 60, 1, false);
  assert.ok(meshes.every((m) => m.layers.mask === 1), 'shown again in third person');
  gunner.updateCamera(1 / 60, 1, true);
  gunner.turret = null;
  assert.ok(meshes.every((m) => m.layers.mask === 1), 'shown again on leaving');
  assert.equal(gunner.hiddenRig, null);
  // An eye above the turret's box (the Yavin tower's) keeps them in view.
  const above = turret({ eye: '0 3 0.5' });
  const g3 = pageGunner(above);
  g3.updateCamera(1 / 60, 1, true);
  assert.equal(g3.hiddenRig, null, 'an eye outside the box keeps the drum and gun');
  // No authored eye (the Hoth turret's 0 0 0): the drum's pivot.
  const bare = turret({ eye: '0 0 0' });
  const g2 = pageGunner(bare);
  g2.updateCamera(1 / 60, 1, true);
  const pivot = bare.drum.getWorldPosition(new THREE.Vector3());
  assert.ok(g2.camera.position.distanceTo(pivot) < 1e-6, 'eye at the drum pivot');
});

test('poseTurret matches Turret.pose for the drum and the gun angles', () => {
  const a = turret({ yaw: 1.1 }), b = turret({ yaw: 1.1 });
  a.gunYaw = b.gunYaw = 0.08; a.gunPitch = b.gunPitch = 0.3;
  a.pose(0, null);
  poseTurret(b, 1.1);
  assert.ok(a.drum.quaternion.angleTo(b.drum.quaternion) < 1e-6);
  assert.ok(a.gun.quaternion.angleTo(b.gun.quaternion) < 1e-6);
});

test('TurretSeats.present poses the bots\' turrets from their replicated angles, not the player\'s', () => {
  const bot = turret({ yaw: 0.7 }), human = turret({ yaw: 0.7 }), empty = turret({ yaw: 0.7 });
  bot.lease = { human: false }; human.lease = { human: true };
  const seats = new TurretSeats({ live: { turrets: [bot, human, empty] } });
  seats.present();
  const reference = turret({ yaw: 0.7 }); reference.pose(0, null);
  assert.ok(bot.drum.quaternion.angleTo(reference.drum.quaternion) < 1e-6, 'bot turret posed');
  assert.ok(human.drum.quaternion.angleTo(new THREE.Quaternion()) < 1e-6, 'the player\'s is left to the gunner');
  assert.ok(empty.drum.quaternion.angleTo(new THREE.Quaternion()) < 1e-6, 'an unmanned one is not touched');
});
