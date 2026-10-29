import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Weapon } from '../src/weapons.js';
import { updateWeaponLock, findLockTarget, botLockTarget, seatLockTarget } from '../src/weapon-targeting.js';
import { findReticleTarget, lockSpan } from '../src/hud-units.js';

const DEG = Math.PI / 180;
// The heavy trooper's missile launcher (rep_weap_inf_torpedo_launcher).
const launcher = () => new Weapon({ class: 'launcher', base: 'launcher', channel: 0, ammo: 4, RoundsPerClip: 2,
  ShotDelay: 2, TriggerSingle: 1, LockTime: 1, LockOnRange: 175, LockOnAngle: 6, LockOffAngle: 10,
  TargetEnemy: 1, TargetPerson: 0, TargetVehicle: 1, ordnance: { base: 'missile', Velocity: 55, LifeSpan: 6 } });

// An AAT-sized hull (8.1 x 3.8 x 9.8 m) as the registry lists it: its collision
// box in its own frame and the bounding sphere around that box.
function hull(id, position, yaw = 0, size = [8.14, 3.8, 9.83]) {
  const vehicle = { bounds: new THREE.Box3(new THREE.Vector3(-size[0] / 2, 0, -size[2] / 2), new THREE.Vector3(size[0] / 2, size[1], size[2] / 2)),
    matrix: new THREE.Matrix4().compose(position, new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), yaw), new THREE.Vector3(1, 1, 1)) };
  const centre = vehicle.bounds.getCenter(new THREE.Vector3()).applyMatrix4(vehicle.matrix);
  return { id, generation: 1, alive: true, team: 2, kind: 6, vehicle, position,
    sphere: { center: centre, radius: vehicle.bounds.getSize(new THREE.Vector3()).length() / 2 },
    aimPoint(out) { return out.copy(centre); } };
}
const eye = new THREE.Vector3(0, 1.6, 0);
const look = (yaw) => new THREE.Vector3(Math.sin(yaw), 0, Math.cos(yaw));
function world(list, collision = { cast: () => null }) {
  return { units: { all: () => list, byId: id => list.find(u => u.id === id), relation: (a, b) => a === b ? 1 : -1 }, collision };
}
// Hold a view for `seconds` and report the lock.
function hold(w, ctx, dir, seconds = 2) {
  for (let i = 0; i < seconds * 60; i++) updateWeaponLock(w, 1 / 60, { ...ctx, origin: eye, dir, viewer: { id: 0, team: 1 } });
  return { id: w.lockTargetId, complete: w.lockComplete };
}
// The view's yaw at which the hull's nearest edge sits `beyond` degrees off it.
function yawBeyondHull(tank, beyond) {
  let lo = 0, hi = Math.PI / 2;
  for (let i = 0; i < 60; i++) {
    const mid = (lo + hi) / 2;
    if (lockSpan(eye, look(mid), tank).angle < beyond * DEG) lo = mid; else hi = mid;
  }
  return lo;
}

test('a launcher locks a hull under its reticle, not one beside it (rhn1 AAT, 15 m)', () => {
  const tank = hull(4, new THREE.Vector3(0, 0, 15)), ctx = world([tank]);
  // 30 degrees off its centre the reticle misses the hull by 8 degrees.
  const away = look(30 * DEG);
  assert.ok(Math.abs(lockSpan(eye, away, tank).angle / DEG - 8.3) < .6, `${lockSpan(eye, away, tank).angle / DEG}`);
  // The original's cone, widened by the 6.66 m bounding radius, takes it.
  const delta = tank.sphere.center.clone().sub(eye), t = delta.dot(away), d2 = delta.lengthSq() - t * t;
  assert.ok(Math.abs(tank.sphere.radius - 6.66) < .01 && d2 <= (tank.sphere.radius + t * 6 * DEG) ** 2);
  assert.deepEqual(hold(launcher(), ctx, away), { id: -1, complete: false });
  // Nor does the reticle turn red for it; on the hull it does.
  const reticle = (dir) => findReticleTarget({ origin: eye, dir, units: [tank], weapon: launcher(), viewer: { team: 1 }, los: () => true })?.unit;
  assert.equal(reticle(away), undefined);
  assert.equal(reticle(look(20 * DEG)), tank);
  // On the hull (20 degrees off its centre, over its flank): acquired, complete after LockTime.
  const w = launcher();
  assert.equal(lockSpan(eye, look(20 * DEG), tank).angle, 0);
  for (let i = 0; i < 30; i++) updateWeaponLock(w, 1 / 60, { ...ctx, origin: eye, dir: look(20 * DEG), viewer: { id: 0, team: 1 } });
  assert.equal(w.lockTargetId, 4); assert.equal(w.lockComplete, false);
  assert.deepEqual(hold(w, ctx, look(20 * DEG), .55), { id: 4, complete: true });
  // LockOnAngle around the hull's edge: 5 degrees off acquires, 7 does not.
  assert.deepEqual(hold(launcher(), ctx, look(yawBeyondHull(tank, 5))), { id: 4, complete: true });
  assert.deepEqual(hold(launcher(), ctx, look(yawBeyondHull(tank, 7))), { id: -1, complete: false });
});

test('a completed lock holds while the hull stays inside LockOffAngle', () => {
  const tank = hull(4, new THREE.Vector3(0, 0, 60)), ctx = world([tank]), w = launcher();
  assert.deepEqual(hold(w, ctx, look(0), 1.1), { id: 4, complete: true });
  assert.deepEqual(hold(w, ctx, look(yawBeyondHull(tank, 8)), .2), { id: 4, complete: true });
  assert.deepEqual(hold(w, ctx, look(yawBeyondHull(tank, 12)), .2), { id: -1, complete: false });
  // An unfinished acquisition never uses the wider cone.
  assert.deepEqual(hold(launcher(), ctx, look(yawBeyondHull(tank, 8))), { id: -1, complete: false });
});

test('no lock behind, beside, out of range, through a wall, or on friends and soldiers', () => {
  const tank = hull(4, new THREE.Vector3(0, 0, 40));
  for (const yaw of [90, 135, 180, -90]) assert.deepEqual(hold(launcher(), world([tank]), look(yaw * DEG)), { id: -1, complete: false }, `${yaw}`);
  assert.deepEqual(hold(launcher(), world([hull(5, new THREE.Vector3(0, 0, 190))]), look(0)), { id: -1, complete: false });
  assert.deepEqual(hold(launcher(), world([hull(5, new THREE.Vector3(0, 0, 175))]), look(0)), { id: 5, complete: true }, 'hull within LockOnRange');
  assert.deepEqual(hold(launcher(), world([tank], { cast: () => ({ body: {} }) }), look(0)), { id: -1, complete: false });
  assert.deepEqual(hold(launcher(), world([{ ...tank, team: 1 }]), look(0)), { id: -1, complete: false });
  assert.deepEqual(hold(launcher(), world([{ ...tank, vehicle: null, kind: 0 }]), look(0)), { id: -1, complete: false });
});

test('the nearest direction of a long hull is found along its edges, not at its nearest point', () => {
  // A 40 m hull running away from the viewer, 3 m to the side of the view.
  const train = { id: 9, generation: 1, alive: true, team: 2, kind: 6, position: new THREE.Vector3(),
    vehicle: { bounds: new THREE.Box3(new THREE.Vector3(3, -1, 10), new THREE.Vector3(5, 1, 50)), matrix: new THREE.Matrix4() } };
  const span = lockSpan(new THREE.Vector3(), new THREE.Vector3(0, 0, 1), train);
  assert.ok(Math.abs(span.angle - Math.atan2(3, 50)) < 1e-6, `${span.angle / DEG}`);
  assert.ok(Math.abs(span.distance - Math.hypot(3, 10)) < 1e-6);
  // The nearest of two hulls on the view wins; the one nearer the line first.
  const near = hull(1, new THREE.Vector3(3, 0, 30)), far = hull(2, new THREE.Vector3(0, 0, 80));
  assert.equal(findLockTarget({ origin: eye, dir: look(0), units: [near, far], weapon: launcher(), viewer: { team: 1 }, angle: 6 }).unit, near);
  assert.equal(findLockTarget({ origin: eye, dir: look(0), units: [hull(1, new THREE.Vector3(9, 0, 30)), far], weapon: launcher(), viewer: { team: 1 }, angle: 6 }).unit, far);
});

test('a bot hands its controller target to its launcher; a human, local or networked, locks by view', () => {
  const target = hull(3, new THREE.Vector3(80, 0, 0));
  const controller = { perception: { fire: { weaponTarget: { unit: target, generation: 1 } } } };
  assert.equal(botLockTarget({ controller }), target);
  assert.equal(botLockTarget({ controller: { perception: { fire: { weaponTarget: { unit: target, generation: 0 } } } } }), null);
  assert.equal(botLockTarget({ controller: null }), null);
  for (const human of [{ human: true }, { networkHuman: true }, { combat: { player: true } }]) assert.equal(botLockTarget({ ...human, controller }), undefined);
  assert.equal(botLockTarget(null), undefined);
  // A network human whose pooled body still names a target 90 degrees away
  // does not lock it.
  const w = launcher(), ctx = world([target]);
  for (let i = 0; i < 120; i++) updateWeaponLock(w, 1 / 60, { ...ctx, origin: eye, dir: look(0), viewer: { id: 0, team: 1 }, target: botLockTarget({ networkHuman: true, controller }) });
  assert.equal(w.lockTargetId, -1);
  for (let i = 0; i < 61; i++) updateWeaponLock(w, 1 / 60, { ...ctx, origin: eye, dir: look(0), viewer: { id: 0, team: 1 }, target: botLockTarget({ controller }) });
  assert.equal(w.lockComplete, true);
});

test('a seated bot hands over its seat controller\'s target, and only what the launcher may lock', () => {
  const tank = hull(3, new THREE.Vector3(0, 0, 60));
  // In a seat the bot's own controller is idle; the seat's (SeatUnit) picks targets.
  const seat = { perception: { fire: { weaponTarget: { unit: tank, generation: 1 } } } };
  const lease = { team: 1, unit: { controller: null }, controllable: { controller: seat } };
  assert.equal(seatLockTarget(lease), tank);
  assert.equal(seatLockTarget({ ...lease, controllable: null, unit: { controller: seat } }), tank);
  assert.equal(seatLockTarget({ ...lease, human: true }), undefined);
  assert.equal(seatLockTarget({ ...lease, unit: { networkHuman: true } }), undefined);
  // A soldier chosen for the seat's cannon never becomes its missile's lock.
  const soldier = { id: 8, generation: 1, alive: true, team: 2, kind: 0, position: new THREE.Vector3(0, 0, 30),
    sphere: { center: new THREE.Vector3(0, 1, 30), radius: .5 } };
  const w = launcher(), ctx = world([soldier, tank]);
  for (let i = 0; i < 61; i++) updateWeaponLock(w, 1 / 60, { ...ctx, origin: eye, dir: look(90 * DEG), viewer: { id: 0, team: 1 }, target: soldier });
  assert.equal(w.lockTargetId, -1);
  for (let i = 0; i < 61; i++) updateWeaponLock(w, 1 / 60, { ...ctx, origin: eye, dir: look(90 * DEG), viewer: { id: 0, team: 1 }, target: seatLockTarget(lease) });
  assert.deepEqual([w.lockTargetId, w.lockComplete], [3, true]);
});
