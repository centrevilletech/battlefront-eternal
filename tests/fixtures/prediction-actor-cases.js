import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld } from '../../tools/collision-fixture.mjs';
import { Soldier } from '../../src/soldier.js';
import { Droideka } from '../../src/droideka.js';
import { Clip } from '../../src/anim.js';
import { PlayerCombat } from '../../src/combat.js';
import { InfantryPrediction } from '../../src/runtime/infantry-prediction.js';
import { DroidekaPrediction } from '../../src/runtime/droideka-prediction.js';
import { captureSoldierCorrection, restoreSoldierCorrection, SOLDIER_REPLAY_SCHEMA } from '../../src/runtime/network-entity-correction.js';
import { captureDroidekaState, restoreDroidekaState } from '../../src/runtime/network-droideka-state.js';

function box(x, y, z, width, height, depth, polygon = 0) {
  const g = new THREE.BoxGeometry(width, height, depth).translate(x, y, z);
  return { positions: g.attributes.position.array, indices: Uint32Array.from(g.index.array),
    polygons: Uint32Array.from({ length: g.index.count / 3 }, (_, i) => polygon + Math.floor(i / 2)) };
}
function merge(parts) {
  const positions = [], indices = [], polygons = [];
  for (const p of parts) { const offset = positions.length / 3; positions.push(...p.positions); indices.push(...p.indices.map(i => i + offset)); polygons.push(...p.polygons); }
  return { positions: Float32Array.from(positions), indices: Uint32Array.from(indices), polygons: Uint32Array.from(polygons) };
}

export function actorCourse(rays = null, geometry = null) {
  const parts = [box(0, -.5, 0, 60, 1, 60), box(8, 1.5, 0, .5, 3, 20, 6), box(4, 1.3, -10, 8, .2, 8, 12), box(-12, 5, 8, 5, 10, 5, 18)];
  for (let i = 0; i < 5; i++) parts.push(box(0, (i + 1) * .15, 7 + i * .6, 4, (i + 1) * .3, .6, 24 + i * 6));
  const data = geometry || merge(parts), world = new CollisionWorld(data.positions, data.indices, data.polygons,
    data.terrainPositions, data.terrainIndices, rays);
  const hull = box(0, 0, 0, 3, 1.5, 3), shape = world.createBodyShape(hull.positions, hull.indices, hull.polygons);
  const moving = world.addMovingBody(shape, new THREE.Matrix4().setPosition(-4, .75, 3), { name: 'moving' });
  const prop = box(-2, 1, 4, 1, 2, 1), fixed = world.addBody(prop.positions, prop.indices, prop.polygons, { name: 'fixed' });
  return { world, moving, fixed, geometry: data, update(tick) {
    world.moveBody(moving, new THREE.Matrix4().makeRotationY(tick * .007).setPosition(-4 + Math.sin(tick * .03) * 2, .75, 3));
    fixed.enabled = tick % 70 < 45;
  }, dispose() { world.removeBody(moving); world.removeBody(fixed); shape.bvh.geometry.dispose(); world.bvh.geometry.dispose(); } };
}

export function actorClips() {
  const clips = new Map();
  for (const name of ['idle', 'turnl', 'turnr', 'death01', 'idle_to_leftfoot', 'walk_leftfoot_rightfoot', 'walk_rightfoot_leftfoot', 'leftfoot_to_idle', 'rightfoot_to_idle', 'fold', 'roll', 'unfold']) {
    const frames = name === 'death01' ? 43 : name === 'fold' || name === 'unfold' ? 19 : 13, data = new Float32Array(frames * 7);
    for (let i = 0; i < frames; i++) { data[i * 7 + 3] = 1; data[i * 7 + 6] = name.includes('foot') ? i / 30 + (i % 3) / 300 : 0; }
    clips.set(name, new Clip(name, { frames, tracks: [0], offset: 0, flags: 1, travel: [0, 0, .4] }, data, 30));
  }
  return clips;
}

export function createPredictionActor(scenario, course, content = null) {
  const kind = scenario.kind, clips = content?.clips || actorClips();
  let soldier, actor;
  if (kind === 'droideka') {
    const root = new THREE.Group(), bone = new THREE.Bone(); bone.name = 'dummyroot'; root.add(bone);
    actor = content?.actor?.() || { root, bones: [bone] };
    soldier = new Droideka(course.world, content?.definition || { AnimationSpeed: 2, ...scenario.definition }, clips, actor);
    soldier.arm(content?.weapons || [{ aimers: ['dummyroot'] }, { base: 'shield', MaxShield: 100, AddShield: -15, AddShieldOff: 5 }]);
    soldier.waterClock = 3.25;
  } else soldier = new Soldier(course.world, content?.definition || scenario.definition || {});
  soldier.spawn(...(scenario.spawn || [0, 0, 0, 0]));
  if (kind === 'droideka') soldier.nextGun(new THREE.Vector3());
  soldier.getupTime = content?.getupTime ?? .9; soldier.diveTime = content?.diveTime ?? .6; soldier.maxFlyHeight = content?.maxFlyHeight ?? 14;
  if (scenario.patch) for (const [key, value] of Object.entries(scenario.patch)) {
    if (soldier[key]?.isVector3) soldier[key].fromArray(value); else soldier[key] = value;
  }
  if (scenario.ignoreMount) soldier.passThrough = course.moving;
  const weapons = [{ clip: 17, reserve: 34, timer: .2, def: { base: 'blaster' } }, { def: { base: 'shield' }, heat: .4 }];
  const combat = Object.assign(Object.create(PlayerCombat.prototype), { soldier, kick: { rate: .4, hold: .8, offset: .1, extra: 'retained' },
    gunUnderwater: false, arsenal: { weapons, current: channel => weapons[channel] },
    tick() { throw Error('prediction stepped authoritative combat'); }, fire() { throw Error('prediction fired an authoritative weapon'); } });
  if (scenario.kick) combat.kick = structuredClone(scenario.kick);
  let deathCalls = 0;
  if (kind === 'droideka') { soldier.onDeathBlast = () => deathCalls++; soldier.onExploded = () => deathCalls++; }
  const callbacks = [soldier.onDeathBlast, soldier.onExploded], arsenal = structuredClone(weapons), kick = structuredClone(combat.kick);
  return { soldier, combat, actor, clips, callbacks, arsenal, kick, deathCalls: () => deathCalls };
}

export function actorPredictionCases() {
  return [
    { name: 'walk-turn-posture-dive-getup', kind: 'soldier', ticks: 420, pattern: 'posture' },
    { name: 'sideways-dive-and-getup', kind: 'soldier', ticks: 240, pattern: 'dive', spawn: [-12, 0, -10, 0] },
    { name: 'stairs-and-crowd-continuation', kind: 'soldier', ticks: 240, spawn: [0, 0, 4, 0], pattern: 'walk', crowd: true },
    { name: 'failed-step-up-and-seeded-scratch', kind: 'soldier', ticks: 150, spawn: [6, 0, 0, Math.PI / 2], pattern: 'walk', seededScratch: true,
      patch: { contactCount: 7, contactMean: [.25, -.5, 1.5], stepBlocked: true } },
    { name: 'blocked-headroom', kind: 'soldier', ticks: 320, spawn: [4, 0, -15, 0], pattern: 'headroom' },
    { name: 'fall-hard-landing', kind: 'soldier', ticks: 300, patch: { position: [-12, 40, 8], settled: [-12, 40, 8], air: 'fall', grounded: false }, pattern: 'walk' },
    { name: 'explosion-thrown', kind: 'soldier', ticks: 200, patch: { air: 'thrown', grounded: false, velocity: [4, 9, -3] }, pattern: 'walk' },
    { name: 'hover-jet-and-ceiling', kind: 'soldier', ticks: 360, pattern: 'jet', definition: { JetType: 'hover', JetFuelRechargeRate: .1, JetFuelCost: .12, JetFuelInitialCost: .15, JetFuelMinBorder: .2 } },
    { name: 'jump-jet-and-recharge', kind: 'soldier', ticks: 360, pattern: 'jet', definition: { JetJump: 21, JetPush: 0, JetFuelRechargeRate: .12, JetFuelCost: 0, JetFuelInitialCost: .3, JetFuelMinBorder: .3 } },
    { name: 'moving-body-and-ignored-mount', kind: 'soldier', ticks: 240, spawn: [-4, 0, 0, 0], ignoreMount: true, pattern: 'walk' },
    { name: 'inactive-kick-keeps-absent-fields', kind: 'soldier', ticks: 60, kick: { retained: 9 }, pattern: 'idle' },
    { name: 'droideka-walk-reverse-fold-roll-unfold', kind: 'droideka', ticks: 280, pattern: 'droideka', rollback: true },
    { name: 'droideka-reverse-clip-travel', kind: 'droideka', ticks: 180, pattern: 'reverse' },
    { name: 'droideka-constructor-shield-snapshot', kind: 'droideka', ticks: 90, pattern: 'shield', capturedFlags: true },
    { name: 'droideka-blocked-unfold', kind: 'droideka', ticks: 120, spawn: [4, 0, -10, 0], patch: { mode: 12, position: [4, 0, -10], settled: [4, 0, -10] }, pattern: 'unfold' },
    { name: 'droideka-death-clip-quiet', kind: 'droideka', ticks: 150, pattern: 'idle', patch: { dead: true } },
    { name: 'droideka-rolled-death-quiet', kind: 'droideka', ticks: 20, pattern: 'idle', patch: { mode: 12, dead: true } },
  ];
}

export function actorInput(scenario, tick) {
  const input = { forward: 0, right: 0, turn: 0, pitch: 0, jump: false, crouch: false, prone: false, use: false };
  if (scenario.pattern === 'walk') Object.assign(input, { forward: 1, right: tick > 150 ? .25 : 0, turn: tick > 180 ? -.1 : 0 });
  if (scenario.pattern === 'posture') Object.assign(input, { forward: tick % 100 < 50 ? .5 : 1, right: tick > 220 ? (tick % 120 < 60 ? 1 : -1) : .2,
    prone: tick === 40 || tick === 110, crouch: tick === 150 || tick === 180, jump: [200, 270, 340].includes(tick), turn: tick % 11 === 0 ? -.2 : 0 });
  if (scenario.pattern === 'headroom') Object.assign(input, { forward: tick < 80 || tick > 210 ? .5 : 0, prone: tick === 10, crouch: tick === 100, jump: [150, 260].includes(tick) });
  if (scenario.pattern === 'dive') Object.assign(input, { right: tick < 120 ? 1 : -1, jump: tick === 40 || tick === 150 });
  if (scenario.pattern === 'jet') Object.assign(input, { forward: .8, right: .1, jump: [35, 45, 115, 200, 212, 290].includes(tick), pitch: tick < 110 ? .2 : -.1 });
  if (scenario.pattern === 'droideka') Object.assign(input, { forward: tick < 80 ? 1 : tick < 160 ? -.6 : .7, right: tick % 19 === 0 ? .25 : 0, use: tick === 55 || tick === 150 || tick === 235 });
  if (scenario.pattern === 'reverse') Object.assign(input, { forward: tick < 40 ? 1 : tick < 135 ? -.7 : 0, right: tick > 135 ? .4 : 0 });
  if (scenario.pattern === 'unfold') input.use = [1, 45, 90].includes(tick);
  if (tick % 31 === 0) input.turn = -0;
  return input;
}

export function actorCapture(s) { return s instanceof Droideka ? captureDroidekaState(s) : captureSoldierCorrection(s, SOLDIER_REPLAY_SCHEMA); }
export function actorRestore(s, bytes) { return s instanceof Droideka ? restoreDroidekaState(s, bytes) : restoreSoldierCorrection(s, bytes, SOLDIER_REPLAY_SCHEMA); }
const vectors = ['position', 'previous', 'settled', 'velocity', 'normal', 'avoidance', 'contactMean'];
const scalar = ['aimYaw', 'aimPitch', 'stateTime', 'lock', 'fuel', 'dive', 'groundAdjustment', 'collisionDamage', 'tickLength', 'lookX', 'lookY', 'posture', 'air', 'diveFrom', 'grounded', 'stepBlocked', 'contactCount', 'mode', 'time', 'reverse', 'legs', 'spin', 'roll', 'lean', 'dead', 'blastClock', 'gun', 'waterClock'];
export function actorObservation(s, kick) {
  return { bytes: Array.from(actorCapture(s)), vectors: Object.fromEntries(vectors.filter(k => s[k]).map(k => [k, s[k].toArray()])),
    state: Object.fromEntries(scalar.filter(k => Object.hasOwn(s, k)).map(k => [k, s[k]])), kick: { ...kick },
    shield: s.shield ? { ...s.shield } : null, lastGun: s.lastGun ? { ...s.lastGun } : null };
}
export function actorRigState(actor) {
  if (!actor) return null;
  return [actor.root, ...actor.bones].map(b => [b.position.toArray(), b.quaternion.toArray(), b.scale.toArray(), b.matrixWorld.toArray()]);
}

export function runActorPredictionCase(scenario, { rays = null, makeNative = null, makeCourse = null, create = null, before = null, after = null } = {}) {
  const course = makeCourse ? makeCourse(rays) : actorCourse(rays), fixture = create ? create(scenario, course) : createPredictionActor(scenario, course);
  const { soldier: s, combat } = fixture, native = makeNative?.(course.world), observations = [], wrappers = [], rig = actorRigState(fixture.actor);
  const muzzleRig = actorRigState(s.muzzleActor), muzzleAnimator = s.muzzleAnimator;
  const Wrapper = scenario.kind === 'droideka' ? DroidekaPrediction : InfantryPrediction;
  let prediction, continuation, kick;
  const coverage = { jump: 0, fall: 0, thrown: 0, jet: 0, prone: 0, crouch: 0, dive: 0, lock: 0, shield: 0, stepBlocked: 0, modes: new Set() };
  try {
    for (let tick = 0; tick < scenario.ticks; tick++) {
      const authority = actorCapture(s);
      if (scenario.rollback && continuation) actorRestore(s, continuation);
      course.update(tick);
      if (tick % 17 === 0) {
        prediction?.dispose?.();
        prediction = new Wrapper(s, combat, kick ?? combat.kick, { native }); wrappers.push(prediction);
        if (scenario.capturedFlags && tick === 0) { combat.gunUnderwater = true; combat.arsenal.weapons[1].def.base = 'blaster'; }
      }
      const input = actorInput(scenario, tick), dt = 1 / 60;
      if (s instanceof Droideka) s.look(tick % 9 - 4, tick % 5 - 2, .7, tick % 19 === 0);
      else s.lookRaw(tick % 9 - 4, tick % 5 - 2, .7, tick % 19 === 0);
      if (scenario.crowd && tick % 11 === 0) { s.position.x += .03125; s.settled.x -= .015625; s.avoidance.z += .125; }
      if (scenario.seededScratch && tick % 23 === 0) { s.contactCount = tick + 3; s.contactMean.set(.125, -.25, tick / 8); s.stepBlocked = true; }
      before?.({ fixture, course, prediction, native, tick, input });
      const ingressScratch = scenario.seededScratch ? [s.contactCount, s.contactMean.toArray(), s.stepBlocked] : null;
      prediction.step(input, dt, scenario.pattern === 'shield' ? tick === 1 || tick === 18 : tick % 47 === 8);
      const observation = actorObservation(s, prediction.kick); observation.ingressScratch = ingressScratch; observations.push(observation);
      if (s.air) coverage[s.air]++; if (s.posture === 'prone' || s.posture === 'crouch') coverage[s.posture]++;
      if (s.dive) coverage.dive++; if (s.lock > 0) coverage.lock++; if (s.shield?.on) coverage.shield++; if (s instanceof Droideka) coverage.modes.add(s.mode);
      if (s.stepBlocked) coverage.stepBlocked++;
      continuation = actorCapture(s); kick = { ...prediction.kick };
      after?.({ fixture, course, prediction, native, tick, input, observation });
      if (scenario.rollback) { actorRestore(s, authority); assert.deepEqual(actorCapture(s), authority, 'authoritative body rollback'); }
      assert.deepEqual(combat.kick, fixture.kick, 'authoritative recoil stays private');
      const weapons = structuredClone(combat.arsenal.weapons);
      if (scenario.capturedFlags) weapons[1].def.base = fixture.arsenal[1].def.base;
      assert.deepEqual(weapons, fixture.arsenal, 'weapons are not ticked');
      assert.deepEqual(actorRigState(fixture.actor), rig, 'prediction does not pose the visible rig');
      assert.deepEqual(actorRigState(s.muzzleActor), muzzleRig, 'prediction does not pose the physical muzzle rig');
      assert.equal(s.muzzleAnimator, muzzleAnimator, 'prediction keeps its physical animator owner');
      if (scenario.kind === 'droideka') { assert.equal(fixture.deathCalls(), 0); assert.equal(s.onDeathBlast, fixture.callbacks[0]); assert.equal(s.onExploded, fixture.callbacks[1]); }
    }
    return { ...fixture, course, native, prediction, wrappers, observations, coverage,
      dispose() { for (const p of wrappers) p.dispose?.(); native?.dispose(); course.dispose(); } };
  } catch (error) { prediction?.dispose?.(); native?.dispose(); course.dispose(); throw error; }
}
