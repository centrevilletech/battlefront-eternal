import './fixtures/native-anim.js';
import { contentURL } from './helpers/content.js';
import { stepPrediction, predictHull, registerHullFixture } from './helpers/hull-prediction.js';
// A walker's legs and feet as a page draws them (src/vehicles.js
// Vehicle.present, src/vehicle-feet.js, src/runtime/vehicle-prediction.js):
// the hull he drives is posed though no record poses it, its predicted walk
// cycle keeps up with its predicted hull, and its feet come down by the
// original game's foot rules whatever the frame rate. The retail cases need
// extracted vehicle packs (public/maps/<map>/vehicles/...).
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { CollisionWorld } from '../tools/collision-fixture.mjs';
import { Vehicles } from '../src/vehicles.js';
import {VisualFeet,cycleHeights} from '../src/presentation/feet.js';
import {FootEvent as O} from '../src/presentation/layout.js';
import { packWith } from '../tools/vehicle-packs.mjs';
import { attachPage } from './helpers/page-presentation.js';

// A retail walker on flat ground, as tests/creature-motion.test.js builds one.
function walker(name, map) {
  const pack = packWith(contentURL('maps/'), [name], map);
  if (!pack) return null;
  const dir = new URL('./', pack);
  const bin = (file) => { const b = fs.readFileSync(new URL(file, dir)); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); };
  const data = JSON.parse(fs.readFileSync(pack));
  const g = new THREE.PlaneGeometry(4000, 4000).rotateX(-Math.PI / 2);
  const collision = new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
  const system = new Vehicles({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(), collision,
    data: { ...data, blockers: null, spawns: [{ name: 'test', matrix: new THREE.Matrix4().toArray(), team: 1, post: '', classes: { 1: name } }] },
    buffer: bin('models.bin'), indexBuffer: bin('models_index.bin'), animationBuffer: bin('animations.bin'),
    texture: () => null, effects: { play() { return null; } }, ordnance: { explosion() {} } }); attachPage(system);
  const v = system.vehicles[0];
  v.driver = {}; v.seatIndex = 0;
  return { system, v };
}
const WALKERS = [['imp_walk_atat', 'hot1'], ['imp_walk_atst_snow', 'hot1'], ['rep_walk_atte', 'geo1'], ['cis_walk_spider', 'geo1'], ['all_walk_tauntaun', 'hot1']];
const available = WALKERS.filter(([name, map]) => packWith(contentURL('maps/'), [name], map));

test('every walker\'s feet come down by its class rules, at most once a cycle, the same at any frame or record rate', { skip: !available.length }, () => {
  for (const [name, map] of available) {
    if(name==='all_walk_tauntaun')continue; // Its feet have sounds but no visual stomp resource.
    const runs = [];
    // Footfalls read every tick (a 60 Hz page), every third (20 Hz records)
    // and twice a tick (a 120 Hz page drawing the same tick twice).
    for (const reads of [1, 3, .5]) {
      const { system, v } = walker(name, map);
      const feet=v.def.fx.feet, steps=feet.map(()=>[]), footfalls=new VisualFeet(), heights=cycleHeights(v,feet);
      const detection=Boolean(v.def.fx.stomp?.detection);
      let walkingFrom = null, travelled = 0, phase = v.walkPhase;
      for (let tick = 0; tick < 60 * 14; tick++) {
        stepPrediction(system, 1 / 60, v, tick < 30 ? {} : { forward: 1 });
        if (walkingFrom === null && v.walkBlend > .01) walkingFrom = tick;
        travelled += ((v.walkPhase - phase) % v.walkLength + v.walkLength) % v.walkLength;
        phase = v.walkPhase;
        const each = reads < 1 ? 2 : 1;
        if (reads >= 1 && tick % reads) continue;
        for(let k=0;k<each;k++) {
          footfalls.begin(1/60/each);footfalls.write(v);footfalls.run();
          const row=footfalls.rows.get(v);
          for(let i=0;i<row.nodes.length;i++)for(let n=0;n<footfalls.output[(row.start+i)*O.STRIDE+O.COUNT];n++)steps[i].push({tick});
        }
      }
      runs.push(steps.map((s) => s.length));
      const cycles = travelled / v.walkLength;
      steps.forEach((s, i) => {
        const scheduled=!detection&&Math.max(...heights[i])-Math.min(...heights[i])>(v.def.fx.stomp?.threshold??.15);
        assert.ok(s.length <= Math.ceil(cycles) + 1 + detection, `${name} ${feet[i].name}: ${s.length} in ${cycles.toFixed(1)} cycles`);
        if (scheduled) assert.ok(s.length >= Math.floor(cycles) - 1, `${name} ${feet[i].name}: ${s.length} in ${cycles.toFixed(1)} cycles`);

      });
      // Type 1 (the AT-AT, AT-TE and spider walker): every foot as it sets off.
      if (detection) steps.forEach((s, i) => assert.ok(s[0]?.tick - walkingFrom <= 3, `${name} ${feet[i].name} sets off`));
      system.dispose();
    }
    assert.deepEqual(runs[1], runs[0], `${name}: 20 Hz records`);
    assert.deepEqual(runs[2], runs[0], `${name}: 120 Hz frames`);
  }
});

test('the AT-AT and AT-ST stomp at the foot, level with the root, turned as the foot is', { skip: !available.some(([n]) => n === 'imp_walk_atat') }, async () => {
  const { VehicleEffects } = await import('../src/vehicle-effects.js');
  const played = [];
  const effects = { play: (name, position, normal, velocity, quaternion) => { played.push({ name, position: position.clone(), quaternion: quaternion?.clone() ?? null }); return null; } };
  const fx = new VehicleEffects({ effects });
  const { system, v } = walker('imp_walk_atat', 'hot1');
  for (let tick = 0; tick < 60 * 8; tick++) {
    stepPrediction(system, 1 / 60, v, tick < 30 ? {} : { forward: 1 });
    fx.update([v], 1 / 60);
  }
  assert.ok(played.length >= 6, String(played.length));
  for (const p of played) {
    assert.equal(p.name, 'bigwalkerstomp');
    assert.ok(Math.abs(p.position.y - v.root.position.y) < 1e-6);
    if (v.def.fx.feet[0].turn) assert.ok(new THREE.Vector3(0, 1, 0).applyQuaternion(p.quaternion).y > .95, 'a planted foot is upright');
  }
  system.dispose();
});

test('the hull he drives is posed on the page, and its predicted legs keep up with it', { skip: !available.some(([n]) => n === 'imp_walk_atst_snow') }, async () => {
  const a = walker('imp_walk_atst_snow', 'hot1').v, b = walker('imp_walk_atst_snow', 'hot1').v;
  const foot = (v) => v.byName.bone_l_toe.getWorldPosition(new THREE.Vector3()).sub(v.root.position);
  // No record poses his own hull: present() does, from its walk cycle, and
  // runs the page's clock for what is only drawn.
  b.sleeping = false; b.resetPresentationHistory();
  b.present(1, 1 / 60);
  const idle = foot(b);
  b.walkPhase = .6; b.walkBlend = 1;
  b.present(1, 1 / 60);
  assert.ok(foot(b).distanceTo(idle) > .5, 'the legs step');
  assert.equal(b.shownWalkPhase, .6);
  assert.ok(Math.abs(b.age - 2 / 60) < 1e-9);
  b.walkPhase = 0; b.walkBlend = 0;
  // The world's walker and his prediction of it over the same commands.
  const pending = Array.from({ length: 90 }, (_, i) => ({ tick: i + 1, sequence: i + 1, epoch: 1, generation: 1, down: ['KeyW'], actions: [], look: [], mouseScale: 1 }));
  for (let index = 0; index < pending.length; index++) {
    const input = { forward: 1, right: 0, turn: 0 };
    stepPrediction(a.system, 1 / 60, a, input);
  }
  const predicted = predictHull(b, pending, 0, 1);
  assert.ok(predicted.position.distanceTo(a.position) < 1e-8);
  assert.ok(Math.abs(predicted.walkPhase - a.walkPhase) < 1e-9 && a.walkPhase > .1);
  assert.equal(b.walkPhase, 0, 'the hull itself is left alone');
  // Drawn halfway between its last two ticks, the legs too.
  predicted.present(.5);
  const half = predicted.walkFromPhase + (a.walkPhase - predicted.walkFromPhase) / 2;
  assert.ok(Math.abs(b.shownWalkPhase - half) < 1e-9, `${b.shownWalkPhase} ${half}`);
  assert.ok(foot(b).distanceTo(idle) > .1);
});
