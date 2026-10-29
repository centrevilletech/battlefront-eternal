import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { existsSync, readFileSync } from 'node:fs';
import { Clip, loadClips } from '../src/anim.js';
import { PlayerCombat } from '../src/combat.js';
import { FirstPersonView } from '../src/firstperson.js';
import { prepareMelee } from '../src/heroes.js';
import { WeaponPrediction } from '../src/runtime/weapon-prediction.js';
import { Arsenal } from '../src/weapons.js';

// The first-person arms while the player fires, as the page drives them: the
// page's combat says which weapon is shown (its authority copy of the
// arsenal), and the shots it predicts come from WeaponPrediction's own copy
// (src/runtime/weapon-prediction.js), other objects of the same classes.

const weapon = (name, bank, model, extra = {}) => ({ class: name, base: 'cannon', channel: 0, AnimationBank: bank, HighResGeometry: model,
  RoundsPerClip: 50, ammo: 4, ShotDelay: .2, ReloadTime: 2, InitialSalvoDelay: 0, SalvoCount: 1, ShotsPerSalvo: 1, SalvoDelay: 0,
  ordnance: { base: 'bolt', Velocity: 100, LifeSpan: 1 }, ...extra });
const RIFLE = weapon('rep_weap_inf_rifle', 'rifle', 'rep_1st_weap_inf_rifle', { TriggerSingle: 0 });
const PISTOL = weapon('rep_weap_inf_pistol', 'pistol', 'rep_1st_weap_inf_pistol', { TriggerSingle: 1, RoundsPerClip: 0 });

// A clip holding hp_weapons (track 1) at x for its whole length: x names the
// clip the arms are in.
const MARK = { _1st_handsdown: -1, _1st_rifle_idle: 0, _1st_rifle_shoot: 1, _1st_tool_idle: 2, _1st_tool_shoot: 3 };
function marker(name, seconds) {
  const frames = Math.round(seconds * 30) + 1, data = new Float32Array(frames * 7);
  for (let f = 0; f < frames; f++) data.set([0, 0, 0, 1, MARK[name], 0, 0], f * 7);
  return new Clip(name, { frames, tracks: [1], offset: 0, flags: 0, travel: [0, 0, 0] }, data, 30);
}
const identity = new THREE.Matrix4().toArray();
const RIG = { model: 'arms', bones: [{ name: 'dummyroot', parent: -1, local: identity }, { name: 'hp_weapons', parent: 0, local: identity }] };

function page({ rig = RIG, clips = null } = {}) {
  clips ||= new Map(Object.keys(MARK).map((name) => [name, marker(name, name.includes('shoot') ? .5 : name.includes('idle') ? 3.3 : 0)]));
  const models = new Map([[RIFLE.HighResGeometry, []], [PISTOL.HighResGeometry, []]]);
  const view = new FirstPersonView({ rig, parts: new Map(), clips, texture: () => null, weaponModels: models, firePoints: {} });
  const soldier = { posture: 'stand', air: null, dive: 0, aimYaw: 0, aimPitch: 0, velocity: new THREE.Vector3() };
  const combat = Object.create(PlayerCombat.prototype);
  Object.assign(combat, { arsenal: prepareMelee(new Arsenal([RIFLE, PISTOL], { random: () => .5 })), soldier, zoomStep: 0, dead: false });
  const unit = { id: 1, generation: 1, alive: true, soldier, combat };
  const camera = new THREE.PerspectiveCamera();
  camera.updateMatrixWorld();
  // The page's combat shows the authority's weapon (updateHeld as each
  // correction lands), whichever arsenal the prediction has swapped in.
  const authority = combat.arsenal;
  const args = () => ({ active: true, camera, soldier, shown: authority.current(0), zoomed: false });
  // Into first person and raised: handsdown, then idle after 1 s.
  for (let i = 0; i < 90; i++) view.update(1 / 60, args());
  return { view, unit, combat, args };
}

// Held fire for `seconds` at 60 frames a second, a tick each frame, as the
// page does: the prediction steps its private arsenal and the arms play each
// round it fires (predicted-shots.js), then the frame draws them with the
// authority's weapon shown. Returns each frame's clip and model.
function fire({ view, unit, args }, seconds, { press = ['firePrimary'] } = {}) {
  const prediction = new WeaponPrediction(), frames = [];
  for (let tick = 1; tick <= seconds * 60; tick++) {
    prediction.begin(unit, { tick: tick - 1 });
    prediction.step({ actions: tick === 1 ? press : [], down: ['Mouse0'] }, tick, (shot) => view.play('shoot', shot.weapon));
    view.update(1 / 60, args());
    const held = [...view.weapons].filter(([, w]) => w.group.visible).map(([name]) => name);
    frames.push({ tick, slot: view.slot, clip: view.clip?.name, x: view.hand.position.x, held, at: view.hand.getWorldPosition(new THREE.Vector3()) });
  }
  prediction.restore();
  return frames;
}

test('a predicted round plays the shoot clip with the weapon still in the hand', () => {
  const arms = page();
  assert.equal(arms.view.slot, 'idle');
  const frames = fire(arms, 2);
  const lowered = frames.filter((f) => f.slot === 'handsdown');
  assert.deepEqual(lowered, [], `the arms lowered on ${lowered.length} of ${frames.length} frames`);
  for (const f of frames) {
    assert.equal(f.clip, '_1st_rifle_shoot', JSON.stringify(f));
    assert.ok(Math.abs(f.x - MARK._1st_rifle_shoot) < 1e-6, JSON.stringify(f));
    assert.deepEqual(f.held, [RIFLE.HighResGeometry], JSON.stringify(f));
  }
  // Let go, the arms finish the shot and idle, never lowered.
  const slots = new Set();
  for (let i = 0; i < 60; i++) { arms.view.update(1 / 60, arms.args()); slots.add(arms.view.slot); }
  assert.ok(!slots.has('handsdown'), [...slots].join());
  assert.equal(arms.view.slot, 'idle');
});

test('a single-shot weapon plays shoot each press and never lowers between them', () => {
  const arms = page();
  arms.combat.arsenal.next(0);
  for (let i = 0; i < 120; i++) arms.view.update(1 / 60, arms.args());
  assert.equal(arms.view.clip?.name, '_1st_tool_idle');
  for (let shot = 0; shot < 3; shot++) {
    const frames = fire(arms, .4);
    assert.ok(frames.every((f) => f.slot !== 'handsdown'), `shot ${shot}: ${JSON.stringify(frames.find((f) => f.slot === 'handsdown'))}`);
    assert.equal(frames[0].clip, '_1st_tool_shoot');
    assert.ok(frames.every((f) => f.held.join() === PISTOL.HighResGeometry));
  }
});

test('a weapon switch still lowers the old weapon and raises the new one', () => {
  const arms = page();
  arms.combat.arsenal.next(0);
  arms.view.update(1 / 60, arms.args());
  assert.equal(arms.view.slot, 'handsdown');
  assert.equal(arms.view.holding.name, RIFLE.class, 'the old weapon is lowered');
  for (let i = 0; i < 25; i++) arms.view.update(1 / 60, arms.args());
  assert.equal(arms.view.holding.name, PISTOL.class);
  for (let i = 0; i < 2; i++) arms.view.update(1 / 60, arms.args());
  assert.equal(arms.view.slot, 'idle');
  assert.equal(arms.view.clip.name, '_1st_tool_idle');
});

// The retail arms and clips: the grip stays where it idles through a burst
// of rifle fire. The shoot clip moves it by at most 0.16 m; lowered
// (handsdown) it is 0.8 m away, beside the eye inside the near plane.
const rigFile = new URL('../public/sides/rep/firstperson/rep_1st_trooper.json', import.meta.url);
const clipFile = new URL('../public/common/anims_fp.json', import.meta.url);
test('retail clone trooper arms keep the rifle in view through held fire', { skip: !existsSync(rigFile) || !existsSync(clipFile) }, () => {
  const rig = JSON.parse(readFileSync(rigFile));
  const bytes = readFileSync(new URL('../public/common/anims_fp.bin', import.meta.url));
  const clips = loadClips(JSON.parse(readFileSync(clipFile)), bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength), rig.bones.map((b) => b.name));
  const arms = page({ rig, clips });
  const at = () => arms.view.hand.getWorldPosition(new THREE.Vector3());
  const idle = at();
  const frames = fire(arms, 2);
  assert.ok(frames.every((f) => f.slot === 'shoot' && f.clip === '_1st_rifle_shoot'), JSON.stringify(frames.find((f) => f.slot !== 'shoot')));
  const moved = Math.max(...frames.map((f) => f.at.distanceTo(idle)));
  assert.ok(moved < .3, `the rifle moved ${moved.toFixed(3)} m from where it idles`);
});
