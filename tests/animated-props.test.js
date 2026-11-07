import './fixtures/native-anim.js';
// Page presentation follows published animation and hold records. Trigger,
// body placement and death behavior are covered by the native live fixtures.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { AnimatedProp } from '../src/props.js';
import { loadClips } from '../src/anim.js';
import { pageProps } from './helpers/page-presentation.js';

const DT = 1 / 60;

function rig(names, at = null) {
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

// A soldier on foot with his feet at (x, y, z), as the props see units.
function soldier(x, y = 0, z = 0) {
  return { soldier: { position: new THREE.Vector3(x, y, z), velocity: new THREE.Vector3(), air: null, dive: 0 },
    get position() { return this.soldier.position; }, alive: true, dead: false, removed: false, generation: 1, id: 7 };
}


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
  unit.combat = { mounted: false };
  unit.letGo = () => { unit.held = null; unit.combat.mounted = false; };
  return unit;
}

test('a page draws a held creature and soldier from published clip and hold records', () => {
  const {p:page}=creature(),man=victim(0,0,0.5),units={byId:id=>id===man.id?man:null};
  assert.equal(page.adopt([1,30,0,man.id,man.generation,0,30],units),true);
  const phase=pageProps({animated:[page]});
  phase.present(DT,90);
  assert.equal(page.victim.unit,man);assert.ok(man.held.frame);
  const at=t=>{phase.present(DT/2,t);return new THREE.Vector3().setFromMatrixPosition(man.held.frame);};
  const before=at(90),between=at(90.5),after=at(91);
  assert.ok(between.distanceTo(before.clone().lerp(after,.5))<.01);
  assert.ok(between.distanceTo(before)>0 && between.distanceTo(after)>0);
  assert.equal(page.adopt([0,220,0,0,0,-1,0],units),true);
  assert.equal(page.victim,null);assert.equal(man.held,null);
});

test("a creature's published state travels at each action and retains the held facade", async () => {
  const {captureWorldPresentation,applyWorldPresentation,CREATURE_STATE_SCHEMA}=await import('../src/runtime/entity-world-state.js');
  const {WorldFieldStagger}=await import('../src/runtime/snapshot-records.js');
  const {p:world}=creature(),{p:page}=creature(),man=victim(0,0,.5),pageMan=victim(0,0,.5);
  const worldUnits={byId:id=>id===man.id?man:null},pageUnits={byId:id=>id===man.id?pageMan:null};
  const worldGame={live:{animated:[world],buildings:[]}},pageGame={live:{animated:[page],buildings:[]},units:pageUnits};
  const stagger=new WorldFieldStagger(),text=new TextDecoder();let sent=null,changes=0;
  const states=[[0,1,0,0,0,-1,0],[1,10,0,0,0,-1,0],[1,10,0,man.id,man.generation,0,30]];
  for(let i=0;i<states.length;i++){
    world.adopt(states[i],worldUnits);
    const records=captureWorldPresentation(worldGame,i+1,1).filter(r=>r.schema===CREATURE_STATE_SCHEMA);
    assert.equal(records.length,1);const [record]=stagger.update(i+1,records),payload=text.decode(record.payload);
    if(payload!==sent){sent=payload;changes++;assert.equal(applyWorldPresentation(pageGame,{...record,kind:'summary'}),true);}
  }
  assert.equal(changes,3);assert.equal(page.victim.unit,pageMan);assert.equal(page.names[page.slot],'grab');
  assert.throws(()=>applyWorldPresentation(pageGame,{kind:'summary',id:400000,schema:CREATURE_STATE_SCHEMA,generation:1,payload:new TextEncoder().encode('[0,7,1,0,0,0,-1,0]')}),/Invalid creature state/);
});
