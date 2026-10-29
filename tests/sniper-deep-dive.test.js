import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Vector3, Scene } from 'three';
import { Weapon } from '../src/weapons.js';
import { latePresses } from '../src/combat-weapon-input.js';
import { Units } from '../src/registry.js';
import { Ordnance } from '../src/ordnance.js';
import { HitHistory } from '../src/runtime/hit-history.js';
import { SniperTrace } from '../src/sniper-trace.js';
import { lighting } from '../src/materials.js';

const sniperDef = {
  class: 'test_sniper', base: 'cannon', channel: 0, ammo: 7, RoundsPerClip: 6, ReloadTime: 2, ShotDelay: 1, TriggerSingle: 1,
  YawSpread: 0, PitchSpread: 0, ordnance: { class: 'test_sniper_ord', base: 'bolt', Velocity: 2000, LifeSpan: 0.15, FadeOutTime: 1, Damage: 160 },
};
const aim = { origin: new Vector3(), direction: new Vector3(0, 0, 1) };
const step = (weapon, dt, pressed, late = 0) => {
  const trigger = { held: pressed, pressed };
  if (late) latePresses({ primary: trigger, secondary: { pressed: false } }, [late, 0]);
  return weapon.update(dt, true, trigger, false, aim);
};

test('a press that ran late is locked out from when it was pressed, so the next on-time press fires', () => {
  const run = (late) => {
    const weapon = new Weapon(sniperDef);
    let fired = step(weapon, 1 / 60, true, late).length;
    step(weapon, 1 / 60, false);
    // The shooter's page fires again right as its own lockout ends: one
    // second after his first press, which here ran `late` ticks after it.
    for (let tick = 2; tick < 60 - late; tick++) step(weapon, 1 / 60, false);
    fired += step(weapon, 1 / 60, true).length;
    return fired;
  };
  assert.equal(run(0), 2);
  assert.equal(run(6), 2, 'a press 100 ms late no longer swallows the next one');
});

function fixture() {
  const units = new Units();
  const unit = (x, z) => units.add({ alive: true, generation: 1, position: new Vector3(x, 0, z), get sphere() { return { center: this.head, radius: 0.2 }; },
    head: new Vector3(x, 1.65, z), hitShapes() { this.head.set(this.position.x, 1.65, this.position.z); return [{ type: 'sphere', center: this.head, radius: 0.17, part: 'head' }]; } });
  const shooter = unit(0, 0), target = unit(0, 60); shooter.networkHuman = true;
  const history = new HitHistory(units); const hits = [];
  const ordnance = new Ordnance({ scene: new Scene(), collision: { cast: () => null }, effects: { play() {} }, lights: { add() {} }, texture: () => null });
  ordnance.units = units; ordnance.hitHistory = history; ordnance.onHitUnit = (u, def, point, owner, part) => hits.push(part);
  return { units, shooter, target, history, ordnance, hits };
}

test('a lag-compensated round leaves from where its shooter is, not where he was when he saw the target', () => {
  const f = fixture();
  // The shooter strafes 6 m/s across his aim while the target stands still,
  // in front of where he was 12 ticks ago.
  f.target.position.x = 0.8;
  for (let tick = 0; tick <= 20; tick++) { f.shooter.position.x = tick * 0.1; f.history.capture(tick); }
  f.shooter.networkShot = { id: 1, viewTick: 8 };
  // His page fires from his predicted body: where the server has him now.
  f.ordnance.bolt(sniperDef.ordnance, new Vector3(f.shooter.position.x, 1.65, 0), new Vector3(0, 0, 1), f.shooter);
  f.ordnance.update(1 / 60, false);
  assert.deepEqual(f.hits, [], 'his line from where he is passes 1.2 m beside that head');
  f.target.position.x = f.shooter.position.x;
  for (let tick = 21; tick <= 30; tick++) f.history.capture(tick);
  f.shooter.networkShot = { id: 2, viewTick: 20 };
  f.ordnance.bolt(sniperDef.ordnance, new Vector3(f.shooter.position.x, 1.65, 0), new Vector3(0, 0, 1), f.shooter);
  f.ordnance.update(1 / 60, false);
  assert.deepEqual(f.hits, ['head'], 'a head straight down his line from where he stands is struck');
});

test('droidekas are struck through p_ord and p_rollsphere, with no head', async () => {
  const { Unit } = await import('../src/units.js');
  const proto = Unit.prototype;
  const s = { position: new Vector3(0, 0, 0), aimYaw: 0, rolled: false, ballRadius: 0.65 };
  const unit = Object.create(proto);
  Object.assign(unit, { soldier: s, walker: true, tickCount: 1, spheresTick: 0, rootScale: 1, bounds: { center: new Vector3(), radius: 1 },
    shapes: [0, 1].map(() => ({ type: 'sphere', part: 'body', center: new Vector3(), radius: 0, half: 0, yaw: 0, halfSize: new Vector3() })) });
  unit.hitSet = unit.shapes;
  const shapes = unit.hitShapes();
  assert.equal(shapes.length, 2);
  assert.ok(shapes.every((shape) => shape.part === 'body'));
  const ord = shapes.find((shape) => shape.type === 'cylinder');
  assert.deepEqual(ord.center.toArray().map((n) => +n.toFixed(3)), [0, 1, 0.506]);
  assert.equal(ord.radius, 0.8); assert.equal(ord.half, 1);
  s.rolled = true; unit.tickCount++;
  assert.equal(unit.hitShapes().length, 1, 'rolled, the ball alone');
});

test('the Enhanced sniper tracer flies its bolt, lands with the world\'s impact, and shows a head shot once', () => {
  const flashes = [], played = [];
  const trace = new SniperTrace({ add() {}, flash: (...args) => flashes.push(args) });
  const def = sniperDef.ordnance;
  const source = {};
  const start = new Vector3(0, 1.6, 0), dir = new Vector3(0, 0, 1);
  const frame = (t, touch = null) => { lighting.time.value = t; trace.begin(); touch?.(); trace.end(); };
  frame(10, () => trace.touch(source, def, start, new Vector3(0, 1.6, 30), dir, false, 1));
  assert.equal(trace.records.length, 1);
  assert.equal(trace.puffs.length, 1, 'the muzzle puffs');
  assert.ok(trace.streaks.count >= 2, 'the puff and the smoke');
  // The round struck 40 m off within the next frame; the drawn bolt, at
  // 650 m/s, is still on its way, and the world's impact and head shot wait.
  frame(10.02, () => trace.touch(source, def, start, new Vector3(0, 1.6, 40), dir, true, 1));
  assert.equal(trace.defer(new Vector3(0, 1.6, 40), () => played.push('impact')), true);
  trace.headshot(new Vector3(0, 1.6, 40));
  trace.headshot(new Vector3(0, 1.6, 40.2));
  assert.equal(flashes.length, 0, 'nothing plays before the drawn bolt gets there');
  assert.ok(trace.records[0].head.z > 5 && trace.records[0].head.z < 40, 'the bolt is drawn in flight');
  frame(10.04, () => trace.touch(source, def, start, new Vector3(0, 1.6, 40), dir, true, 1));
  frame(10.07, () => trace.touch(source, def, start, new Vector3(0, 1.6, 40), dir, true, 1));
  assert.deepEqual(played, ['impact'], 'the world\'s impact plays as the bolt arrives');
  assert.equal(flashes.length, 2, 'the impact light, and one head shot flourish for both copies');
  assert.ok(trace.sparks.length > 0);
  // The bolt gone, the tracer fades and its smoke runs out.
  for (let t = 10.1; t < 14; t += 0.05) frame(t);
  assert.equal(trace.records.length, 0);
  assert.equal(trace.streaks.count, 0);
  // A round that runs out of life in the air has no impact.
  frame(20, () => trace.touch({}, def, start, new Vector3(0, 1.6, 300), dir, true, 1));
  for (let t = 20.05; t < 21; t += 0.05) frame(t);
  assert.equal(flashes.length, 2);
  void THREE;
});

test('the crosshair ray converges on a soldier under it, not the ground behind him', async () => {
  const { PlayerCombat } = await import('../src/combat.js');
  const self = { collision: { cast: () => null }, ordnance: null, crosshairReach: PlayerCombat.prototype.crosshairReach };
  const from = new Vector3(0, 2, 0), forward = new Vector3(0, 0, 1);
  assert.equal(self.crosshairReach(from, forward), 60, 'nothing in the way: 60 m');
  const units = new Units();
  const target = units.add({ alive: true, generation: 1, combat: {}, position: new Vector3(0, 0, 40), get sphere() { return { center: new Vector3(0, 2, 40), radius: 1 }; },
    hitShapes() { return [{ type: 'sphere', center: new Vector3(0, 2, 40), radius: 0.17, part: 'head' }]; } });
  units.add({ alive: true, generation: 1, combat: self, position: new Vector3(0, 0, 1), get sphere() { return { center: new Vector3(0, 2, 1), radius: 1 }; },
    hitShapes() { return [{ type: 'sphere', center: new Vector3(0, 2, 1), radius: 0.5, part: 'body' }]; } });
  self.ordnance = { units };
  assert.ok(Math.abs(self.crosshairReach(from, forward) - 39.83) < 1e-6, 'onto his head, the shooter himself left out');
  target.alive = false;
  assert.equal(self.crosshairReach(from, forward), 60, 'a body lying dead is not aimed at');
});

test('the Enhanced sniper look is built with the battle, so its programs compile in the warm-up, and draws every instance once grown', () => {
  const scene = new Scene();
  const ordnance = new Ordnance({ scene, collision: { cast: () => null }, effects: {}, lights: { add() {}, flash() {} }, texture: () => null });
  ordnance.prepare();
  const names = [];
  scene.traverse((o) => names.push(o.name));
  assert.ok(names.includes('sniper-trace') && names.includes('sniper-flares'), 'in the scene before the first shot');
  const trace = ordnance.sniperTrace();
  const flares = trace.flares;
  flares.geometry._maxInstanceCount = flares.capacity; // as three records it at the first draw
  const capacity = flares.capacity;
  lighting.time.value = 50;
  // The most flashes it keeps is one more than its buffer was made for.
  for (let i = 0; i < capacity + 5; i++) trace.burst({ kind: 0, at: new Vector3(i, 0, 0), color: [1, 1, 1], radius: 0.5, life: 1, brightness: 1 });
  trace.begin();
  trace.end();
  assert.equal(flares.count, capacity + 1);
  assert.ok(flares.capacity > capacity);
  assert.equal(flares.geometry._maxInstanceCount, undefined, 'every instance is drawn after growing');
  assert.deepEqual(flares.buffer.updateRanges.map((r) => [r.start, r.count]), [[0, (capacity + 1) * 14]]);
  assert.equal(trace.streaks.capacity, 512, 'smoke, vapour and sparks at their caps (48 shots, 25 puffs, 161 sparks) fit the first buffer');
});

test('while warming, the empty sniper batches draw one dark instance each, so their pipelines are built before the first shot', () => {
  const trace = new SniperTrace({ add() {}, flash() {} });
  lighting.time.value = 40;
  trace.begin();
  trace.end();
  for (const batch of [trace.streaks, trace.flares]) {
    assert.equal(batch.count, 0);
    assert.equal(batch.geometry.instanceCount, 1, 'one instance drawn');
    assert.ok(batch.mesh.visible);
    assert.ok(batch.data.subarray(0, batch.stride).every((x) => x === 0), 'all zero: no brightness');
    assert.deepEqual(batch.buffer.updateRanges.map((r) => [r.start, r.count]), [[0, batch.stride]]);
    batch.buffer.updateRanges.length = 0;
  }
  trace.warming = 0;
  trace.begin();
  trace.end();
  assert.equal(trace.streaks.geometry.instanceCount, 0, 'warmed: empty draws nothing');
  assert.equal(trace.flares.mesh.visible, false);
});

test('spent sniper sparks and embers are reused', () => {
  const trace = new SniperTrace({ add() {}, flash() {} });
  const frame = (t) => { lighting.time.value = t; trace.begin(); trace.end(); };
  frame(30);
  trace.spray(new Vector3(0, 1, 10), new Vector3(0, 0, 1), [1, 0, 0], 12, 1);
  trace.embers(new Vector3(0, 1, 10), new Vector3(0, 0, 1), 6);
  const first = new Set(trace.sparks);
  assert.equal(first.size, 18);
  for (let t = 30.05; t < 32.5; t += 0.05) frame(t);
  assert.equal(trace.sparks.length, 0);
  assert.equal(trace.sparkPool.length, 18);
  trace.spray(new Vector3(0, 1, 10), new Vector3(0, 0, 1), [1, 0, 0], 12, 1);
  assert.ok(trace.sparks.every((s) => first.has(s) && s.age === 0 && !s.ember), 'the same objects, started afresh');
});
