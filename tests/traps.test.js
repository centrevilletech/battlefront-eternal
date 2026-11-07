import './fixtures/native-anim.js';
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
import { pageProps, local } from './helpers/page-presentation.js';

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
  return t;
}

test('a trap starts unbuilt, its pad and hologram out, and belongs to its ResetTeam', () => {
  const t = trap();
  assert.equal(t.state, 'unbuilt');
  assert.equal(t.health, 0);
  assert.equal(t.healthType, 'build');
  assert.equal(t.pad.root.visible, true);
  assert.equal(t.phase, 'down');
  const live = Object.assign(Object.create(LiveObjects.prototype), { traps: [t] });
  live.attachTraps({ teamOf: (side) => ({ all: 1, imp: 2 })[side] });
  assert.equal(t.team, 1);
  // Its sphere is the pad's, on the pad.
  const at = new THREE.Vector3();
  assert.ok(Math.abs(t.sphere(at) - 0.71) < 1e-6);
  assert.ok(Math.abs(at.x - 27.5) < 1e-4 && Math.abs(at.z - 42.3) < 1e-4);
});

test('a page poses the published armed and down phases without ticking the trap', () => {
  const t = trap();
  t.show('built');
  const phase=pageProps({traps:[t]});t.adopt([2, 17, 0]);phase.present(0,22);
  assert.ok(Math.abs(local(t.rig.bones[1]).position.x - 23) < 1e-4);
  t.adopt([0, 45, 0]);phase.present(0,45);
  assert.ok(Math.abs(local(t.rig.bones[1]).position.x - 11) < 1e-4);
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
  const states = [[1,0,1,0],[5,1,5,0],[290,2,290,0],[420,3,420,1],[744,0,744,0]];
  for (const [at,phase,started,sprung] of states) {
    tick=at;world.adopt([phase,started,sprung]);
    const records=captureWorldPresentation(worldGame,tick,1).filter(r=>r.schema===TRAP_STATE_SCHEMA);
    assert.equal(records.length,1);assert.equal(records[0].id,500000);
    const [record]=stagger.update(tick,records),payload=text.decode(record.payload);
    if(payload!==sent){sent=payload;changes++;assert.equal(applyWorldPresentation(pageGame,{...record,kind:'summary'}),true);}
    assert.equal(page.phase,TRAP_PHASES[phase]);assert.equal(page.started,started);
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
