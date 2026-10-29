// Animated props and doors, as the original game's animated prop runs them
// (src/props.js AnimatedProp): idle clips, proximity triggers, the eased
// pose, the trapdoor's collision (solid shut, gone while it opens), and a
// creature's grab (the sarlacc's tentacles): who it seizes, where it holds
// him, and his end; and a page following the world's state.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { AnimatedProp } from '../src/props.js';
import { loadClips } from '../src/anim.js';
import { listenSounds } from '../src/audio/events.js';

const BONES = ['dummyroot', 'bone_door1', 'hp_spot'];
const DT = 1 / 60;

function rig(names = BONES, at = null) {
  const root = new THREE.Group();
  if (at) root.position.copy(at);
  const bones = names.map((name) => { const b = new THREE.Object3D(); b.name = name; root.add(b); return b; });
  const byName = Object.fromEntries(bones.map((b) => [b.name.toLowerCase(), b]));
  const bind = new Map(bones.map((b) => [b, { q: b.quaternion.clone(), p: b.position.clone() }]));
  root.updateMatrixWorld(true);
  return { root, byName, bind, bones, meshes: [] };
}

// A clip of `frames` frames moving bone 1 along x from `from` to `to`.
function track(frames, from, to) {
  const out = [];
  for (let f = 0; f < frames; f++) out.push(0, 0, 0, 1, from + (to - from) * f / Math.max(frames - 1, 1), 0, 0);
  return out;
}

// idle holds the bone at x=0 (31 frames, 1 s); the action slides it to
// x=1 over a second (31 frames).
function clips(names = BONES) {
  const idle = track(31, 0, 0), act = track(31, 0, 1);
  const floats = new Float32Array([...idle, ...act, ...act]);
  const meta = { fps: 30, bones: names, clips: {
    idle: { frames: 31, flags: 0, offset: 0, tracks: [1], travel: [0, 0, 0] },
    open: { frames: 31, flags: 0, offset: idle.length, tracks: [1], travel: [0, 0, 0] },
    grab: { frames: 31, flags: 0, offset: idle.length + act.length, tracks: [1], travel: [0, 0, 0] },
  } };
  return loadClips(meta, floats.buffer, names);
}

function prop(base, action, extra = {}) {
  const desc = {
    base, matrix: new THREE.Matrix4().toArray(),
    animation: { idle: base === 'door' ? 'open' : 'idle', clips: base === 'door' ? ['open'] : ['idle', action], trigger: null,
      triggers: [{ action, node: 'hp_spot', radius: 0.5 }], ...extra },
  };
  const r = rig();
  const body = { enabled: true };
  return { p: new AnimatedProp(desc, r, body, clips(), 0), doorBone: r.bones[1], body };
}

// A soldier on foot with his feet at (x, y, z), as the props see units.
function soldier(x, y = 0, z = 0) {
  return { soldier: { position: new THREE.Vector3(x, y, z), velocity: new THREE.Vector3(), air: null, dive: 0 },
    get position() { return this.soldier.position; }, alive: true, dead: false, removed: false, generation: 1, id: 7 };
}

const near = () => [soldier(0.5)];
const far = () => [soldier(9)];

test('a trapdoor is solid shut, opens under a soldier, and is solid again after', () => {
  const { p, doorBone, body } = prop('animatedprop', 'open');
  p.step(DT, [], far(), 1);
  assert.equal(body.enabled, true);
  assert.equal(doorBone.position.x, 0);
  p.step(DT, [], near(), 2);
  assert.equal(p.names[p.slot], 'open', 'the open clip starts');
  assert.equal(body.enabled, false, 'and its collision goes with it');
  for (let i = 0; i < 45; i++) p.step(DT, [], near(), 3 + i);
  assert.ok(doorBone.position.x > 0.4, `the door bone moves, easing in (${doorBone.position.x})`);
  for (let i = 0; i < 30; i++) p.step(DT, [], far(), 50 + i);
  assert.equal(p.slot, 0, 'the clip ends back at idle');
  assert.equal(body.enabled, true, 'solid again');
  assert.ok(doorBone.position.x > 0, 'the idle eases in at a fifth a second, so the door is not back yet');
  for (let i = 0; i < 400; i++) p.step(DT, [], far(), 80 + i);
  assert.ok(doorBone.position.x < 1e-3, `idle pose in the end, no stuck action frame (${doorBone.position.x})`);
});

test('triggers wait for the idle to blend half in, and each plays its own action once', () => {
  const { p } = prop('animatedprop', 'grab');
  let tick = 1;
  p.step(DT, [], far(), tick++);
  p.step(DT, [], near(), tick++);
  assert.equal(p.names[p.slot], 'grab');
  // The action lasts its frames at 30 a second (31 frames: 1.033 s).
  let steps = 0;
  while (p.slot !== 0 && steps < 200) { p.step(DT, [], near(), tick++); steps++; }
  assert.ok(steps >= 61 && steps <= 63, `the grab ran ${steps} steps`);
  // Back at idle with the soldier still there: not again until the idle's
  // weight reaches 0.5, at a fifth a second (2.5 s).
  let again = 0;
  while (p.slot === 0 && again < 400) { p.step(DT, [], near(), tick++); again++; }
  assert.ok(again >= 149 && again <= 152, `the next grab came after ${again} steps`);
});

test('the reach is the box round the node that meets his collision sphere', () => {
  const { p } = prop('animatedprop', 'grab');
  // His sphere: 1.278 m round a point 1.048 m above his feet; the node at
  // the origin with a 0.5 m reach: a box 1.778 m each way.
  p.step(DT, [], [soldier(1.7)], 1);
  assert.equal(p.slot, 1, '1.7 m to the side is in reach');
  const { p: q } = prop('animatedprop', 'grab');
  q.step(DT, [], [soldier(1.9)], 1);
  assert.equal(q.slot, 0, '1.9 m is not');
  const { p: r } = prop('animatedprop', 'grab');
  r.step(DT, [], [soldier(1.2, 0, 1.2)], 1);
  assert.equal(r.slot, 1, 'a box, not a sphere: 1.7 m away on the diagonal still is');
});

test('IdleDelay holds the idle on its last frame between loops', () => {
  const { p } = prop('animatedprop', 'grab', { idleDelay: 0.5, fromStart: true });
  let tick = 1;
  assert.equal(p.time, 0, 'fromStart: the first frame');
  const times = [];
  for (let i = 0; i < 120; i++) { p.step(DT, [], far(), tick++); times.push(p.time); }
  const held = times.filter((t, i) => i > 0 && t === times[i - 1]).length;
  assert.ok(held >= 29 && held <= 31, `held ${held} steps at the end of the loop`);
});

// A creature: a rig with its grab node on a bone the grab clip swings, and
// the grab itself (hp_active 1.5 grab human_rifle_thrown_flail 2.5).
const BODY = ['DummyRoot', 'bone_root', 'bone_pelvis', 'bone_neck', 'bone_head', 'bone_l_thigh', 'bone_l_calf', 'bone_l_foot', 'bone_r_thigh', 'bone_r_calf', 'bone_r_foot'];
function body() {
  const bones = BODY.map((name) => { const b = new THREE.Object3D(); b.name = name; return b; });
  const by = Object.fromEntries(bones.map((b) => [b.name, b]));
  by.DummyRoot.add(by.bone_root); by.bone_root.add(by.bone_pelvis); by.bone_pelvis.add(by.bone_neck); by.bone_neck.add(by.bone_head);
  for (const side of ['l', 'r']) { by.bone_pelvis.add(by[`bone_${side}_thigh`]); by[`bone_${side}_thigh`].add(by[`bone_${side}_calf`]); by[`bone_${side}_calf`].add(by[`bone_${side}_foot`]); }
  by.bone_root.position.set(0, 1, 0);
  // His clip: the root bone 1 m up, the rest at bind.
  const frames = 26, floats = [];
  for (let f = 0; f < frames; f++) floats.push(0, 0, 0, 1, 0, 0, 0);
  for (let f = 0; f < frames; f++) floats.push(0, 0, 0, 1, 0, 1, 0);
  const flail = loadClips({ fps: 30, bones: BODY, clips: { human_rifle_thrown_flail: { frames, flags: 4, offset: 0, tracks: [0, 1], travel: [0, 0, 0] } } },
    new Float32Array(floats).buffer, BODY);
  return { bones, bindLengths: [], rootBind: new THREE.Vector3(), clips: flail };
}

function creature(at = new THREE.Vector3()) {
  const names = ['root', 'bone_arm', 'hp_active', 'hp_hotspot'];
  const r = rig(names, at);
  r.byName.bone_arm.add(r.byName.hp_active);
  r.byName.hp_hotspot.position.set(0, 0, 5);
  r.root.updateMatrixWorld(true);
  // The arm sweeps from x=0 to x=1 over the grab; the idle holds it at 0.
  const idle = track(31, 0, 0), sweep = track(91, 0, 1);
  const set = loadClips({ fps: 30, bones: names, clips: {
    idle: { frames: 31, flags: 0, offset: 0, tracks: [1], travel: [0, 0, 0] },
    grab: { frames: 91, flags: 0, offset: idle.length, tracks: [1], travel: [0, 0, 0] },
  } }, new Float32Array([...idle, ...sweep]).buffer, names);
  const desc = { base: 'animatedprop', matrix: new THREE.Matrix4().makeTranslation(at.x, at.y, at.z).toArray(), animation: {
    idle: 'idle', clips: ['idle', 'grab'], triggers: [{ action: 'grab', node: 'hp_hotspot', radius: 5 }],
    attach: [{ node: 'hp_active', radius: 1.5, action: 'grab', clip: 'human_rifle_thrown_flail', window: 2.5 }],
    killSound: 'sarlacc_belch', deathCamera: true } };
  return { p: new AnimatedProp(desc, r, null, set, 0), r };
}

function victim(x, y = 0, z = 0) {
  const unit = soldier(x, y, z);
  unit.animator = body();
  unit.rootScale = 1;
  unit.combat = { mounted: false, deaths: 0, damage(amount, onShield, source = null, attacker = null) { this.amount = amount; this.attacker = attacker; unit.alive = false; unit.dead = true; } };
  unit.remove = () => { unit.removed = true; unit.alive = false; unit.held = null; };
  unit.letGo = () => { unit.held = null; unit.combat.mounted = false; };
  return unit;
}

test('a tentacle seizes the first living soldier at its node, holds him there, and swallows him at the end', () => {
  const sounds = [];
  const unlisten = listenSounds((type, ...args) => sounds.push([type, ...args]));
  try {
    const { p, r } = creature();
    const man = victim(0, 0, 0.5);
    let tick = 1;
    // He stands within the hotspot's 5 m: the grab plays, and he is within
    // 1.5 m of hp_active (at the origin at first).
    p.step(DT, [], [man], tick++);
    assert.equal(p.names[p.slot], 'grab');
    assert.equal(p.victim?.unit, man, 'seized at once');
    assert.equal(man.held.prop, p);
    assert.equal(man.combat.mounted, true, 'his weapons wait');
    // Held: his root bone on the node, so his feet 1 m under it, swinging
    // with the arm.
    const node = new THREE.Vector3();
    for (let i = 0; i < 60; i++) {
      p.step(DT, [], [man], tick++);
      r.byName.hp_active.getWorldPosition(node);
      assert.ok(man.position.distanceTo(node.clone().setY(node.y - 1)) < 1e-4, `on the node (${man.position.toArray()} vs ${node.toArray()})`);
    }
    assert.ok(man.position.x > 0.1, `carried with the arm (${man.position.x})`);
    // To the grab's end (91 frames at 30 a second: 3.03 s), then gone: no
    // one credited, the belch at the prop.
    while (p.slot !== 0 && tick < 400) p.step(DT, [], [man], tick++);
    assert.equal(p.victim, null);
    assert.equal(man.removed, true, 'his body is gone at once');
    assert.equal(man.combat.amount, Infinity);
    assert.equal(man.combat.attacker, null, 'no one is credited');
    assert.deepEqual(sounds.find((s) => s[1] === 'sarlacc_belch')?.slice(0, 2), ['sound', 'sarlacc_belch']);
  } finally { unlisten(); }
});

test('a grab takes no one past its window, no body, and no one seated', () => {
  const { p } = creature();
  const corpse = victim(0, 0, 0.5);
  corpse.alive = false; corpse.dead = true;
  let tick = 1;
  p.step(DT, [], [corpse], tick++);
  assert.equal(p.names[p.slot], 'grab', 'a body lying there sets the grab off');
  assert.equal(p.victim, null, 'but is not seized');
  // A living soldier who walks up after the window (2.5 s) is not taken.
  while (p.time <= 2.5) p.step(DT, [], [], tick++);
  const late = victim(0.9, 0, 0);
  p.step(DT, [], [late], tick++);
  assert.equal(p.victim, null, 'too late');
  const { p: q } = creature();
  const rider = victim(0, 0, 0.5);
  rider.seat = { index: 0 };
  q.step(DT, [], [rider], 1);
  assert.equal(q.victim, null, 'a seated soldier is not on foot');
});

test('a page following the world draws the creature and whom it holds where the world has them', () => {
  const { p: world } = creature();
  const { p: page, r: pageRig } = creature();
  const man = victim(0, 0, 0.5);
  const units = { byId: (id) => (id === man.id ? man : null) };
  let tick = 1;
  const pageMan = victim(0, 0, 0.5);
  const pageUnits = { byId: (id) => (id === man.id ? pageMan : null) };
  for (; tick < 100; tick++) {
    world.step(DT, [], [man], tick);
    assert.equal(page.adopt(world.state(), pageUnits), true);
    page.follow(DT, tick);
    page.presentHeld(tick);
  }
  assert.equal(page.victim?.unit, pageMan, 'the page holds whom the world holds');
  assert.ok(pageMan.held.frame, 'and places him');
  const drawn = new THREE.Vector3().setFromMatrixPosition(pageMan.held.frame);
  assert.ok(drawn.distanceTo(man.position) < 0.02, `drawn at ${drawn.toArray()}, held at ${man.position.toArray()}`);
  const a = new THREE.Vector3(), b = new THREE.Vector3();
  world.rig.byName.hp_active.getWorldPosition(a); pageRig.byName.hp_active.getWorldPosition(b);
  assert.ok(a.distanceTo(b) < 0.02, 'the page poses the creature as the world does');
  // Between ticks the page draws the creature, and him, between them: a
  // frame at 120 Hz lands halfway.
  const at = (t) => { page.follow(DT / 2, t); page.presentHeld(t); return new THREE.Vector3().setFromMatrixPosition(pageMan.held.frame); };
  const before = at(tick - 1), between = at(tick - 0.5), after = at(tick);
  assert.ok(between.distanceTo(before.clone().lerp(after, 0.5)) < 0.01 + before.distanceTo(after) * 0.25,
    `halfway between ticks (${before.toArray()} ${between.toArray()} ${after.toArray()})`);
  assert.ok(between.distanceTo(before) > 0 && between.distanceTo(after) > 0, 'not held to either tick');
  // The world's swallow: the page lets go.
  while (world.slot !== 0) { world.step(DT, [], [man], tick); page.adopt(world.state(), pageUnits); tick++; }
  assert.equal(page.victim, null);
  assert.equal(pageMan.held, null);
});

test("a creature's state travels as a world record, at once when it acts", async () => {
  const { captureWorldPresentation, applyWorldPresentation, CREATURE_STATE_SCHEMA } = await import('../src/runtime/entity-world-state.js');
  const { WorldFieldStagger } = await import('../src/runtime/snapshot-records.js');
  const { p: world } = creature();
  const { p: page } = creature();
  const man = victim(0, 0, 0.5);
  const pageMan = victim(0, 0, 0.5);
  const worldGame = { live: { animated: [world], buildings: [] } };
  const pageGame = { live: { animated: [page], buildings: [] }, units: { byId: (id) => (id === man.id ? pageMan : null) } };
  const stagger = new WorldFieldStagger();
  const text = new TextDecoder();
  let sent = null, changes = 0;
  // Far off; at the hotspot (the grab, out of the arm's reach); then at the arm.
  man.position.set(0, 0, 20);
  for (let tick = 1; tick < 120; tick++) {
    if (tick === 10) man.position.set(0, 0, 5);
    if (tick === 30) man.position.set(0.5, 0, 0.5);
    world.step(DT, [], [man], tick);
    const records = captureWorldPresentation(worldGame, tick, 1).filter((r) => r.schema === CREATURE_STATE_SCHEMA);
    assert.equal(records.length, 1);
    const [record] = stagger.update(tick, records);
    const payload = text.decode(record.payload);
    if (payload !== sent) { sent = payload; changes++; assert.equal(applyWorldPresentation(pageGame, { ...record, kind: 'summary' }), true); }
  }
  assert.equal(changes, 3, 'its idle, its grab, its seizure: each taken at once, nothing between');
  assert.equal(page.victim?.unit, pageMan);
  assert.equal(page.names[page.slot], 'grab');
  assert.throws(() => applyWorldPresentation(pageGame, { kind: 'summary', id: 400000, schema: CREATURE_STATE_SCHEMA, generation: 1, payload: new TextEncoder().encode('[0,7,1,0,0,0,-1,0]') }), /Invalid creature state/);
});
