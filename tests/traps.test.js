// Endor's Ewok log traps (src/traps.js), as the original game's EntityTrap
// runs them: unbuilt at first, built by a repair weapon's BuildingBuild,
// hauled up, set off by a large vehicle of TriggerTeam's side, crushing
// every large thing in its box DeathTime into the swing, unbuilt again
// once the swing has played; shot down, it falls; its state travels.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Trap, sideOfTeams, TRAP_PHASES } from '../src/traps.js';
import { LiveObjects } from '../src/props.js';
import { loadClips } from '../src/anim.js';

const DT = 1 / 60;
const BONES = ['dummyroot', 'bone_r_tree', 'bone_l_tree', 'hp_active'];
const SETTINGS = {
  trigger: 'trigger', build: 'reset', triggerTeam: 'imp', resetTeam: 'all', radius: 3, minRadius: 4, deathTime: 0.666,
  node: null, offset: [0, 0, 0], deathEffect: 'explosion', deathEffectOffset: [0, 7.5, 0], sound: null,
  pad: { model: 'com_prop_buildzone', at: [17.5, 0, 22.3], hologram: [0, 0.25, 0] },
};

function rig(names = BONES) {
  const root = new THREE.Group();
  const bones = names.map((name) => { const b = new THREE.Object3D(); b.name = name; root.add(b); return b; });
  return { root, bones, byName: Object.fromEntries(bones.map((b) => [b.name, b])), model: { sphere: [0, 0.08, 0, 0.71] } };
}

// trigger: 161 frames, the right log from x=23 (cocked) to 11 (at rest);
// reset: 141 frames, 11 back to 23.
function clips() {
  const track = (frames, from, to) => {
    const out = [];
    for (let f = 0; f < frames; f++) out.push(0, 0, 0, 1, from + (to - from) * f / (frames - 1), 6, 0);
    return out;
  };
  const trigger = track(161, 23, 11), reset = track(141, 11, 23);
  const meta = { fps: 30, bones: BONES, clips: {
    trigger: { frames: 161, flags: 0, offset: 0, tracks: [1], travel: [0, 0, 0] },
    reset: { frames: 141, flags: 0, offset: trigger.length, tracks: [1], travel: [0, 0, 0] },
  } };
  return loadClips(meta, new Float32Array([...trigger, ...reset]).buffer, BONES);
}

function trap(settings = SETTINGS) {
  const desc = { base: 'trap', class: 'end_weap_treesmash', model: 'end_weap_treesmash', matrix: new THREE.Matrix4().makeTranslation(10, -3.5, 20).toArray(),
    settings: { MaxHealth: '1000.0' }, team: 0, trap: settings };
  const t = new Trap(desc, { built: rig(), pad: rig(['com_prop_buildzone', 'hp_hologram']) }, {}, null, clips());
  t.sideOf = (team) => ({ 1: 'all', 2: 'imp', 3: 'ewk' })[team] ?? null;
  return t;
}

// A hull as the trap sees one: its model's sphere (AT-ST 5.53 m, 4.24 m
// up; speeder bike 1.94 m), its team, and setHealth.
function hull(x, z, team = 2, radius = 5.53, up = 4.24) {
  return {
    alive: true, team, dying: 0, position: new THREE.Vector3(x, -3.5, z), root: { visible: true }, lastAttacker: 'someone',
    blastSphere(out) { out.copy(this.position); out.y += up; return radius; },
    setHealth(v) { if (v <= 0) { this.alive = false; this.dying = 3; } },
  };
}

function build(t, tick = 1) {
  // The Rebels' fusion cutter: BuildingBuild 30 a tick.
  while (t.state !== 'built') t.setHealth(t.health + 30);
  t.step(DT, tick, []);
  return tick;
}

function runTo(t, phase, vehicles = [], from = 1, limit = 2000) {
  let tick = from;
  while (t.phase !== phase && tick < from + limit) t.step(DT, ++tick, vehicles);
  assert.equal(t.phase, phase);
  return tick;
}

test('a trap starts unbuilt, its pad and hologram out, and belongs to its ResetTeam', () => {
  const t = trap();
  assert.equal(t.state, 'unbuilt');
  assert.equal(t.health, 0);
  assert.equal(t.healthType, 'build');
  assert.equal(t.pad.root.visible, true);
  assert.equal(t.phase, 'down');
  const live = Object.assign(Object.create(LiveObjects.prototype), { traps: [t] });
  live.attachTraps({ sideOf: t.sideOf, teamOf: (side) => ({ all: 1, imp: 2 })[side], effect: null });
  assert.equal(t.team, 1);
  // Its sphere is the pad's, on the pad.
  const at = new THREE.Vector3();
  assert.ok(Math.abs(t.sphere(at) - 0.71) < 1e-6);
  assert.ok(Math.abs(at.x - 27.5) < 1e-4 && Math.abs(at.z - 42.3) < 1e-4);
});

test('built, it is hauled up (BuildAnimation) and then stands cocked', () => {
  const t = trap();
  let tick = build(t);
  assert.equal(t.state, 'built');
  assert.equal(t.health, 1000);
  assert.equal(t.pad.root.visible, false);
  assert.equal(t.phase, 'reset');
  const start = tick;
  tick = runTo(t, 'armed', [], tick);
  // 141 frames at 30 a second.
  assert.ok(Math.abs((tick - start + 1) * DT - 141 / 30) < 2 * DT, `${(tick - start) * DT}`);
  // A page draws the cocked pose: the swing's first frame.
  t.present(tick + 5);
  assert.ok(Math.abs(t.rig.bones[1].position.x - 23) < 1e-4);
});

test('only a large vehicle of TriggerTeam inside its box sets it off', () => {
  const t = trap();
  let tick = runTo(t, 'armed', [], build(t));
  const bike = hull(10, 20, 2, 1.94, 0.4);
  const rebelWalker = hull(10, 20, 1);
  const farWalker = hull(10 + 8.6, 20, 2);
  for (let i = 0; i < 30; i++) t.step(DT, ++tick, [bike, rebelWalker, farWalker]);
  assert.equal(t.phase, 'armed', 'a speeder bike is too small, the Rebels\' walker is no Imperial, the far one is out of reach');
  const walker = hull(10 + 8.4, 20, 2);
  t.step(DT, ++tick, [walker]);
  assert.equal(t.phase, 'swing');
});

test('DeathTime into the swing it crushes every large thing in its box, no one credited, gone at once', () => {
  const t = trap();
  const effects = [];
  t.onEffect = (name, at) => effects.push([name, at]);
  let tick = runTo(t, 'armed', [], build(t));
  const walker = hull(12, 20, 2);
  const rebel = hull(9, 21, 1);
  const bike = hull(10, 20, 2, 1.94, 0.4);
  const vehicles = [walker, rebel, bike];
  const start = tick + 1;
  let crushedAt = null;
  while (t.phase !== 'down') {
    t.step(DT, ++tick, vehicles);
    if (crushedAt === null && !walker.alive) crushedAt = tick;
  }
  assert.ok(Math.abs((crushedAt - start + 1) * DT - 0.666) <= DT, `crushed ${(crushedAt - start + 1) * DT} s in`);
  assert.equal(walker.alive, false);
  assert.equal(rebel.alive, false, 'whatever its side');
  assert.equal(bike.alive, true, 'smaller than MinEnemyRadius');
  assert.equal(walker.dying, 0, 'no death animation');
  assert.equal(walker.root.visible, false);
  assert.equal(walker.lastAttacker, null);
  assert.equal(effects.length, 1);
  assert.equal(effects[0][0], 'explosion');
  assert.deepEqual(effects[0][1].toArray(), [10, 4, 20]);
  // The swing played (161 frames): unbuilt, the pad back, the logs at rest.
  assert.ok(Math.abs((tick - start + 1) * DT - 161 / 30) < 2 * DT);
  assert.equal(t.state, 'unbuilt');
  assert.equal(t.health, 0);
  assert.equal(t.pad.root.visible, true);
  t.present(tick + 10);
  assert.ok(Math.abs(t.rig.bones[1].position.x - 11) < 1e-4);
  // And it is built again the same way.
  build(t, tick + 1);
  assert.equal(t.phase, 'reset');
});

test('a swing that catches nothing plays no effect', () => {
  const t = trap();
  const effects = [];
  t.onEffect = (name) => effects.push(name);
  let tick = runTo(t, 'armed', [], build(t));
  t.damage(1000);
  runTo(t, 'down', [], tick);
  assert.equal(effects.length, 0);
});

test('shot down while built it falls, even while it is being hauled up', () => {
  const t = trap();
  let tick = build(t);
  for (let i = 0; i < 20; i++) t.step(DT, ++tick, []);
  t.damage(400);
  assert.equal(t.health, 600);
  t.damage(700);
  assert.equal(t.health, 0);
  assert.equal(t.state, 'built', 'it is no wreck: it falls');
  assert.equal(t.phase, 'reset');
  tick = runTo(t, 'swing', [], tick);
  runTo(t, 'down', [], tick);
  assert.equal(t.state, 'unbuilt');
});

test("a trap's phase travels as a world record, at once when it changes", async () => {
  const { captureWorldPresentation, applyWorldPresentation, TRAP_STATE_SCHEMA } = await import('../src/runtime/entity-world-state.js');
  const { WorldFieldStagger } = await import('../src/runtime/snapshot-records.js');
  const world = trap(), page = trap();
  const worldGame = { live: { traps: [world], buildings: [], animated: [] } };
  const pageGame = { live: { traps: [page], buildings: [], animated: [] } };
  const stagger = new WorldFieldStagger();
  const text = new TextDecoder();
  let sent = null, changes = 0;
  let tick = 1;
  const walker = hull(10, 20, 2);
  for (; tick < 900; tick++) {
    if (tick === 5) while (world.state !== 'built') world.setHealth(world.health + 30);
    world.step(DT, tick, tick > 400 ? [walker] : []);
    const records = captureWorldPresentation(worldGame, tick, 1).filter((r) => r.schema === TRAP_STATE_SCHEMA);
    assert.equal(records.length, 1);
    assert.equal(records[0].id, 500000);
    const [record] = stagger.update(tick, records);
    const payload = text.decode(record.payload);
    if (payload !== sent) { sent = payload; changes++; assert.equal(applyWorldPresentation(pageGame, { ...record, kind: 'summary' }), true); }
    if (tick === 420) {
      assert.equal(page.phase, 'swing');
      assert.equal(page.started, world.started);
    }
  }
  assert.equal(changes, 5, 'down, reset, armed, swing, down: each at once');
  assert.equal(page.phase, 'down');
  assert.deepEqual(TRAP_PHASES, ['down', 'reset', 'armed', 'swing']);
  assert.throws(() => applyWorldPresentation(pageGame, { kind: 'summary', id: 500000, schema: TRAP_STATE_SCHEMA, generation: 1, payload: new TextEncoder().encode('[0,7,1,0]') }), /Invalid trap state/);
});

test("a team's side is its classes' prefix", () => {
  const sideOf = sideOfTeams({ 1: { classes: [['all_inf_soldierjungle', 11]] }, 2: { classes: [['imp_inf_storm_trooper', 11]] }, 3: { classes: [['ewk_inf_trooper', 4]] } });
  assert.equal(sideOf(1), 'all');
  assert.equal(sideOf(2), 'imp');
  assert.equal(sideOf(3), 'ewk');
  assert.equal(sideOf(4), null);
});
