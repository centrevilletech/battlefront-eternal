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
import { CollisionWorld } from '../src/collision.js';
import { Vehicles } from '../src/vehicles.js';
import { Footfalls, footSchedule, gait } from '../src/vehicle-feet.js';
import { predictVehicle } from '../src/runtime/vehicle-prediction.js';
import { packWith } from '../tools/vehicle-packs.mjs';

const RULES = { detection: 0, threshold: .15, hydraulic: 1 };
// A 2 s cycle at 30 keys a second: planted for the first half, then a bump
// `top` metres high (a sine), over `span` seconds.
function bump(top, span = 1, length = 2) {
  const count = length * 30;
  return Float32Array.from({ length: count }, (_, k) => {
    const t = k / 30 - (length - span);
    return t > 0 ? top * Math.sin(Math.PI * t / span) : 0;
  });
}

test('type 0: a foot comes down as it falls through StompThreshold over its lowest, and lifts past HydraulicSoundHeight', () => {
  const { stomps, lifts } = footSchedule(bump(1.2), 2, RULES);
  assert.equal(stomps.length, 1, 'once a cycle');
  // The sine falls through 0.15 m at t = 1 - asin(.15 / 1.2) / pi of its second.
  const crossing = 2 - Math.asin(.15 / 1.2) / Math.PI;
  assert.ok(Math.abs(stomps[0].at - crossing) < .02, String(stomps[0].at));
  assert.equal(stomps[0].arm, null);
  assert.equal(lifts.length, 1);
  assert.ok(Math.abs(lifts[0] - (1 + Math.asin(1 / 1.2) / Math.PI)) < .02, String(lifts[0]));
  // Played backward, it comes down where it rose forward.
  const back = footSchedule(bump(1.2), 2, RULES, 1, -1);
  assert.equal(back.stomps.length, 1);
  assert.ok(Math.abs(back.stomps[0].at - (1 + Math.asin(.15 / 1.2) / Math.PI)) < .02, String(back.stomps[0].at));
  // Never higher than its threshold: never down; never past 1 m: never lifts.
  assert.deepEqual(footSchedule(bump(.1), 2, RULES), { stomps: [], lifts: [] });
});

test('type 1: a foot comes down once its fall over an update drops under 0.1 m, only after falling faster', () => {
  const rules = { ...RULES, detection: 1 };
  // 2 m in a quarter second: it falls up to 0.42 m in 1/60 s.
  const fast = footSchedule(bump(2, .25), 2, rules);
  assert.equal(fast.stomps.length, 1);
  const { at, arm } = fast.stomps[0];
  // Armed on the way down, down once it has landed (just round the end of
  // the cycle, where the bump ends).
  assert.ok(arm > 1.75 && arm < 2, String(arm));
  assert.ok(((at - arm) % 2 + 2) % 2 < .3 && (at > arm || at < .05), `${arm} ${at}`);
  // At half the animation's pace it falls only half as far an update.
  assert.equal(footSchedule(bump(2, .25), 2, rules, .5).stomps.length, 1);
  assert.equal(footSchedule(bump(.2, 1), 2, rules).stomps.length, 0, 'never 6 m/s: never armed');
});

// A retail walker on flat ground, as tests/creature-motion.test.js builds one.
function walker(name, map) {
  const pack = packWith(new URL('../public/maps/', import.meta.url), [name], map);
  if (!pack) return null;
  const dir = new URL('./', pack);
  const bin = (file) => { const b = fs.readFileSync(new URL(file, dir)); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); };
  const data = JSON.parse(fs.readFileSync(pack));
  const g = new THREE.PlaneGeometry(4000, 4000).rotateX(-Math.PI / 2);
  const collision = new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
  const system = new Vehicles({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(), collision,
    data: { ...data, blockers: null, spawns: [{ name: 'test', matrix: new THREE.Matrix4().toArray(), team: 1, post: '', classes: { 1: name } }] },
    buffer: bin('models.bin'), indexBuffer: bin('models_index.bin'), animationBuffer: bin('animations.bin'),
    texture: () => null, effects: { play() { return null; } }, ordnance: { explosion() {} } });
  const v = system.vehicles[0];
  v.driver = {}; v.seatIndex = 0;
  return { system, v };
}
const WALKERS = [['imp_walk_atat', 'hot1'], ['imp_walk_atst_snow', 'hot1'], ['rep_walk_atte', 'geo1'], ['cis_walk_spider', 'geo1'], ['all_walk_tauntaun', 'hot1']];
const available = WALKERS.filter(([name, map]) => packWith(new URL('../public/maps/', import.meta.url), [name], map));

test('every walker\'s feet come down by its class rules, at most once a cycle, the same at any frame or record rate', { skip: !available.length }, () => {
  for (const [name, map] of available) {
    const runs = [];
    // Footfalls read every tick (a 60 Hz page), every third (20 Hz records)
    // and twice a tick (a 120 Hz page drawing the same tick twice).
    for (const reads of [1, 3, .5]) {
      const { system, v } = walker(name, map);
      const g = gait(v), feet = v.def.fx.feet, steps = feet.map(() => []), footfalls = new Footfalls();
      let walkingFrom = null, travelled = 0, phase = v.walkPhase;
      for (let tick = 0; tick < 60 * 14; tick++) {
        system.update(1 / 60, v, tick < 30 ? {} : { forward: 1 });
        if (walkingFrom === null && v.walkBlend > .01) walkingFrom = tick;
        travelled += ((v.walkPhase - phase) % v.walkLength + v.walkLength) % v.walkLength;
        phase = v.walkPhase;
        const each = reads < 1 ? 2 : 1;
        if (reads >= 1 && tick % reads) continue;
        for (let k = 0; k < each; k++) footfalls.update(v, feet, 1 / 60 / each, (i, point, step) => {
          if (step === 'down') steps[i].push({ tick, height: point.y - v.root.position.y });
        });
      }
      runs.push(steps.map((s) => s.length));
      const cycles = travelled / v.walkLength;
      steps.forEach((s, i) => {
        const scheduled = g.forward[i].stomps.length;
        assert.ok(s.length <= Math.ceil(cycles) + 1 + g.detection, `${name} ${feet[i].name}: ${s.length} in ${cycles.toFixed(1)} cycles`);
        if (scheduled) assert.ok(s.length >= Math.floor(cycles) - 1, `${name} ${feet[i].name}: ${s.length} in ${cycles.toFixed(1)} cycles`);
        // A machine's foot comes down at the ground (type 0: within its
        // threshold and an update's fall of it).
        if (!v.creature && feet[i].bottom) for (const step of s.slice(g.detection ? 1 : 0)) assert.ok(Math.abs(step.height) < .25, `${name} ${feet[i].name} at ${step.height}`);
      });
      // Type 1 (the AT-AT, AT-TE and spider walker): every foot as it sets off.
      if (g.detection) steps.forEach((s, i) => assert.ok(s[0]?.tick - walkingFrom <= 3, `${name} ${feet[i].name} sets off`));
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
    system.update(1 / 60, v, tick < 30 ? {} : { forward: 1 });
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
  const { CommandControls } = await import('../src/runtime/commands.js');
  const { networkVehicleInput } = await import('../src/runtime/network-vehicle.js');
  const a = walker('imp_walk_atst_snow', 'hot1').v, b = walker('imp_walk_atst_snow', 'hot1').v;
  const foot = (v) => v.byName.bone_l_toe.getWorldPosition(new THREE.Vector3()).sub(v.root.position);
  // No record poses his own hull: present() does, from its walk cycle, and
  // runs the page's clock for what is only drawn.
  b.sleeping = false; b.captureTick();
  b.present(1, 1 / 60);
  const idle = foot(b);
  b.walkPhase = .6; b.walkBlend = 1;
  b.present(1, 1 / 60);
  assert.ok(foot(b).distanceTo(idle) > .5, 'the legs step');
  assert.equal(b.shownWalkPhase, .6);
  assert.ok(Math.abs(b.age - 2 / 60) < 1e-9);
  b.walkPhase = 0; b.walkBlend = 0;
  // The world's walker and his prediction of it over the same commands.
  const controls = new CommandControls();
  const pending = Array.from({ length: 90 }, (_, i) => ({ tick: i + 1, sequence: i + 1, epoch: 1, generation: 1, down: ['KeyW'], actions: [], look: [], mouseScale: 1 }));
  for (const c of pending) {
    controls.accept(c);
    const input = networkVehicleInput(a, 0, controls, c);
    a.captureTick(); a.tick(1 / 120, input); a.tick(1 / 120, input); a.walk(1 / 60);
  }
  const predicted = predictVehicle(b, pending, 0, 1);
  assert.ok(predicted.position.distanceTo(a.position) < 1e-8);
  assert.ok(Math.abs(predicted.walkPhase - a.walkPhase) < 1e-9 && a.walkPhase > .1);
  assert.equal(b.walkPhase, 0, 'the hull itself is left alone');
  // Drawn halfway between its last two ticks, the legs too.
  predicted.present(.5);
  const half = predicted.walkFromPhase + (a.walkPhase - predicted.walkFromPhase) / 2;
  assert.ok(Math.abs(b.shownWalkPhase - half) < 1e-9, `${b.shownWalkPhase} ${half}`);
  assert.ok(foot(b).distanceTo(idle) > .1);
});
