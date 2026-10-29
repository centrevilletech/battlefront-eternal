import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { Weapon, Arsenal } from '../src/weapons.js';
import { updateWeaponLock, lockTickRadii } from '../src/weapon-targeting.js';
import { BattleHud } from '../src/hud.js';
import { Ordnance } from '../src/ordnance.js';

const aim = { stick: 0, posture: 'stand', zoomed: false };
const released = { held: false, pressed: false }, pressed = { held: true, pressed: true };
const launcher = (extra = {}) => ({ class: 'launcher', base: 'launcher', channel: 0, ammo: 4,
  RoundsPerClip: 2, ShotDelay: 2, ReloadTime: 1, TriggerSingle: 1, LockTime: 1,
  LockOnRange: 175, LockOnAngle: 6, LockOffAngle: 10, TargetEnemy: 1,
  TargetPerson: 0, TargetVehicle: 1, ordnance: { base: 'missile', Velocity: 40, LifeSpan: 8 }, ...extra });
function target(id = 1, x = 0, z = 40) {
  return { id, generation: 1, alive: true, health: 100, team: 2, kind: 6,
    position: new THREE.Vector3(x, 0, z), sphere: { radius: .1 },
    aimPoint(out) { return out.copy(this.position); } };
}
function context(list, collision = { cast: () => null }) {
  return { units: { all: () => list, byId: id => list.find(u => u.id === id), relation: (a, b) => a === b ? 1 : -1 },
    collision, origin: new THREE.Vector3(), dir: new THREE.Vector3(0, 0, 1), viewer: { id: 0, team: 1 } };
}

test('range and timing fields on rifles, pistols, EMP and tow weapons do not grant missile locks', () => {
  for (const def of [launcher({ base: 'cannon' }), launcher({ ordnance: { base: 'bullet' } }), launcher({ base: 'towcableweapon' })]) {
    const w = new Weapon(def); const t = target();
    for (let i = 0; i < 120; i++) updateWeaponLock(w, 1 / 60, context([t]));
    assert.equal(w.lockComplete, false); assert.equal(w.lockTargetId, -1);
  }
});

test('launcher acquisition uses masks, LOS, lock time, retention cone and generation', () => {
  const w = new Weapon(launcher()), tank = target(), ctx = context([tank]);
  updateWeaponLock(w, .5, ctx); assert.equal(w.lockComplete, false);
  updateWeaponLock(w, .5, ctx); assert.equal(w.lockComplete, true);
  tank.position.x = 40 * Math.tan(8 * Math.PI / 180);
  updateWeaponLock(w, .1, ctx); assert.equal(w.lockComplete, true, 'retained outside six-degree acquisition cone');
  tank.position.x = 40 * Math.tan(12 * Math.PI / 180);
  updateWeaponLock(w, .1, ctx); assert.equal(w.lockTargetId, -1);
  tank.position.x = 0;
  updateWeaponLock(w, 1, ctx); assert.equal(w.lockComplete, true);
  tank.generation++; updateWeaponLock(w, .1, ctx); assert.equal(w.lockComplete, false);
  ctx.collision.cast = () => ({ body: {} });
  updateWeaponLock(w, 1, ctx); assert.equal(w.lockTargetId, -1, 'wall breaks lock');
  ctx.collision.cast = () => null;
  tank.kind = 1; updateWeaponLock(w, 1, ctx); assert.equal(w.lockTargetId, -1, 'person excluded');
  tank.kind = 6; tank.team = 1;
  updateWeaponLock(w, 1, ctx); assert.equal(w.lockTargetId, -1, 'friendly excluded');
});

test('a shot carries only a completed lock; firing and switching reset acquisition', () => {
  const w = new Weapon(launcher()), t = target();
  w.updateLock(.4, t);
  assert.equal(w.update(0, true, pressed, false, aim)[0].targetId, -1);
  w.wait = 0; w.updateLock(1, t);
  const shot = w.update(0, true, pressed, false, aim)[0];
  assert.equal(shot.targetId, t.id); assert.equal(shot.targetGeneration, t.generation);
  assert.equal(w.lockComplete, false);
  w.wait = 0; w.updateLock(1, t); w.deselect(); assert.equal(w.lockTargetId, -1);
  const persistent = new Weapon(launcher({ ClearLockOnFire: 0 }));
  persistent.updateLock(1, t); persistent.update(0, true, pressed, false, aim);
  assert.equal(persistent.lockComplete, true);
});

test('Rebel-style trigger emits two timed salvos after release, not two simultaneous rockets', () => {
  for (const hz of [30, 60, 120]) {
    const w = new Weapon(launcher({ ShotDelay: 0, InitialSalvoDelay: .25, SalvoDelay: .25, SalvoCount: 2 }));
    assert.equal(w.update(0, true, pressed, false, aim).length, 0);
    const times = [];
    for (let tick = 1; tick <= hz; tick++) {
      for (const shot of w.update(1 / hz, true, released, false, aim)) times.push(tick / hz);
    }
    assert.equal(times.length, 2);
    assert.ok(Math.abs(times[0] - .25) <= 1 / hz + 1e-8);
    assert.ok(Math.abs(times[1] - .5) < 1e-8);
    assert.equal(w.loaded, 0);
  }
});

test('a salvo keeps its channel busy, respects partial clips and preserves per-round patterns', () => {
  const def = launcher({ ShotDelay: 0, SalvoCount: 2, SalvoDelay: .25, ShotsPerSalvo: 2,
    RoundsPerClip: 3, ShotPatternPitchYaw: [[0, -1], [0, 1]] });
  const arsenal = new Arsenal([def, { ...def, class: 'other' }]), w = arsenal.current(0);
  const first = w.update(0, true, pressed, false, aim);
  assert.equal(first.length, 2); assert.ok(first[0].yaw < 0 && first[1].yaw > 0);
  assert.equal(arsenal.next(0), false);
  assert.equal(w.update(.25, true, released, false, aim).length, 1);
  assert.equal(w.loaded, 0); assert.equal(arsenal.next(0), true);
});

test('zero-delay unlimited weapons cannot hang the firing loop', () => {
  const w = new Weapon(launcher({ RoundsPerClip: 0, TriggerSingle: 0, ShotDelay: 0 }));
  assert.equal(w.update(1, true, pressed, false, aim).length, 1);
});

test('guided missiles track moving targets and never adopt a recycled unit', () => {
  const t = target(), ctx = context([t]);
  const ord = new Ordnance({ scene: new THREE.Scene(), collision: ctx.collision,
    model: () => null, effects: {}, lights: { add() {} } });
  ord.units = ctx.units;
  const def = { base: 'missile', Velocity: 40, LifeSpan: 8, TurnRate: 2, ScaleTime: .1 };
  const guided = ord.fire(def, ctx.origin, ctx.dir, null, null, null, null, { targetId: t.id, targetGeneration: t.generation });
  const free = ord.fire(def, ctx.origin, ctx.dir, null);
  t.position.x = 10; ord.steer(guided, .1); ord.steer(free, .1);
  assert.ok(guided.velocity.x > 0); assert.equal(free.velocity.x, 0);
  const last = guided.desired.clone(); t.generation++; t.position.x = -30;
  ord.steer(guided, .1); assert.equal(guided.targetId, -1); assert.deepEqual(guided.desired, last);
});

test('native cursor uses eight aim ticks and a separate fading diamond, without a circle', () => {
  const draw = (def, marker, texture = { image: { width: 64, height: 64 } }) => {
    const quads = [];
    const hud = Object.defineProperty(Object.create(BattleHud.prototype), 'height', { value: 768 });
    Object.assign(hud, { reticle: { x: .5, y: .6, weapon: new Weapon(def) }, lockMarker: marker,
      aimCamera: new THREE.PerspectiveCamera(46.826, 4 / 3),
      texture: () => texture, anchor: (x, y) => new THREE.Matrix4().makeTranslation(x, y, 0),
      batch: { quad: (...args) => quads.push(args) }, solid: {} });
    hud.drawReticle(); hud.drawLockDiamond(); return quads;
  };
  const idle = draw(launcher(), null); assert.equal(idle.length, 9);
  const locking = draw(launcher(), { x: .7, y: .4, progress: .5 }); assert.equal(locking.length, 13);
  assert.equal(locking[9].at(-1)[3], 75 / 255);
  const rifle = draw(launcher({ LockOffAngle: 0, base: 'cannon' }), { x: .7, y: .4, progress: 1 });
  assert.equal(rifle.length, 1);
  const upgraded = draw(launcher(), null, { image: { width: 256, height: 256 }, userData: { nativeWidth: 64, nativeHeight: 64 } });
  assert.deepEqual(upgraded[0].slice(1, 5), [-32, -32, 32, 32]);
  const cam = new THREE.PerspectiveCamera(65, 16 / 9), wide = lockTickRadii(launcher(), cam);
  cam.fov = 35; const zoom = lockTickRadii(launcher(), cam);
  assert.ok(zoom[0] > wide[0]);
});

for (const side of ['all', 'rep', 'cis', 'imp']) {
  const file = `public/sides/${side}/units.json`;
  test(`retail ${side} assault launcher retains its own projectile schedule`, { skip: !fs.existsSync(file) }, () => {
    const data = JSON.parse(fs.readFileSync(file));
    const def = Object.values(data.units).flatMap(u => u.weapons || []).find(w => w.class === `${side}_weap_inf_torpedo_launcher`);
    assert.ok(def);
    const w = new Weapon(def), times = [];
    for (let tick = 0; tick <= 60; tick++) for (const shot of w.update(tick ? 1 / 60 : 0, true, tick ? released : pressed, false, aim)) times.push(tick / 60);
    assert.deepEqual(times, side === 'all' ? [.25, .5] : [0]);
  });
}


test('unfinished acquisition does not use the wider lock-off cone', () => {
  const w = new Weapon(launcher()), tank = target(), ctx = context([tank]);
  updateWeaponLock(w, .5, ctx);
  tank.position.x = 40 * Math.tan(8 * Math.PI / 180);
  updateWeaponLock(w, .5, ctx);
  assert.equal(w.lockComplete, false); assert.equal(w.lockTargetId, -1);
});
