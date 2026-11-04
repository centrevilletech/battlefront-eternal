// Synthetic source-oracle cases for rust/sim/tests/ai_combat_twin.rs.
// Run under the reference Node with one output filename argument, then
// set BFE_AI_COMBAT_TWIN to that file for the native test. No game assets
// are loaded. Numbers retain all 64 bits, including signed zero and NaN.
import fs from 'node:fs';
import { Vector3 } from 'three';
import { ClassicRandom } from '../src/ai/random.js';
import { Aimer, ballisticLead, filterDirection, skillCurves } from '../src/ai/classic/aim.js';
import { PriorityHeap, matchupPriority } from '../src/ai/classic/perception.js';
import { pressButton, registerAIData, triggerFromCounts, weaponAI } from '../src/ai/classic/weapons-ai.js';
import { AIM_PROFILES, EnhancedAimer } from '../src/ai/enhanced/aim.js';

if (!process.versions.v8.startsWith('13.6.')) throw Error('Use the reference V8 13.6');
if (!process.argv[2]) throw Error('Pass an output filename');
const rng = new ClassicRandom(0x45ab913);
const n = (scale = 1) => (rng.uniform() * 2 - 1) * scale;
const vector = (scale = 1) => new Vector3(n(scale), n(scale), n(scale));
const plain = (v) => [v.x, v.y, v.z];
const output = { node: process.version, v8: process.versions.v8, heaps: [], filter: [], aimer: [], ballistic: [], weapons: [], buttons: [], priorities: [], enhanced: [] };

for (const capacity of [1, 2, 3, 7, 112, 200]) {
  const heap = new PriorityHeap(capacity, 20000), ops = [];
  for (let i = 0; i < 1200; i++) {
    const op = i % 127 === 126 ? 'clear' : rng.int(0, 3) === 0 ? 'pop' : 'insert';
    const priority = i % 7 === 0 ? 55 : Math.trunc(n(19000)) + (i % 3) * 0.25;
    let result = null;
    if (op === 'clear') heap.clear();
    else if (op === 'pop') result = heap.pop();
    else result = heap.insert(priority, i);
    ops.push({ op, priority, item: i, result, count: heap.count, dropped: heap.dropped,
      priorityAfter: Array.from(heap.priority.subarray(0, heap.count + 1)), items: heap.items.slice(1, heap.count + 1) });
  }
  output.heaps.push({ capacity, ops });
}
for (let i = 0; i < 2048; i++) {
  const d = vector().normalize(), q = vector().normalize(), v = vector(5), before = v.clone();
  const dt = [1 / 60, 1 / 12, 0, -1 / 60, 0.5, NaN][i % 6], A = n(25), D = n(5), out = new Vector3();
  filterDirection(d, q, v, dt, A, D, out);
  output.filter.push({ d: plain(d), q: plain(q), v: plain(before), dt, A, D, out: plain(out), velocity: plain(v) });
}
for (let track = 0; track < 8; track++) {
  const aimer = new Aimer(), steps = [];
  for (let i = 0; i < 128; i++) {
    const desired = i % 31 === 0 ? new Vector3(0, 1, 0) : vector().normalize();
    const dt = i % 29 === 0 ? 0 : 1 / (30 + i % 5), curves = skillCurves(1 + i % 20);
    const def = { base: ['cannon', 'launcher', 'grenade', 'melee'][i % 4], MaxRange: 30 + i, ZoomMax: 1 + i % 8 };
    const weapon = { def, spreadScale: -1, sniperScale: -1 }, hero = i % 19 === 0;
    const out = aimer.update(desired, dt, curves, weapon, hero);
    steps.push({ desired: plain(desired), dt, curves, def, hero, out: plain(out), velocity: plain(aimer.velocity), started: aimer.started,
      keptCurves: { ...aimer.curves }, spread: weapon.spreadScale, sniper: weapon.sniperScale });
  }
  output.aimer.push(steps);
}
for (let i = 0; i < 2048; i++) {
  const p = vector(400), v = vector(80), origin = vector(20), speed = [0, -1, 0.0001, 1, 30, 100, 400, Infinity][i % 8];
  const gravity = [0, -9.81, 9.81, -0, 0.001][i % 5], out = new Vector3();
  const valid = ballisticLead(p, v, origin, speed, gravity, out);
  output.ballistic.push({ p: plain(p), v: plain(v), origin: plain(origin), speed, gravity, out: plain(out), valid });
}
for (let i = 0; i < 256; i++) {
  const def = { class: `synthetic${i}`, base: ['cannon', 'launcher', 'grenade', 'melee'][i % 4],
    AnimationBank: ['pistol', 'rifle', 'grenade', 'launcher', 'melee', 'tool', 'unknown'][i % 7],
    MinRange: i % 3 ? '3.125 meters' : '', OptimalRange: i % 5 ? 12.25 : 0, MaxRange: i % 7 ? '73.5' : null,
    LockOnRange: i % 11 ? '8' : -3, ExtremeRange: i % 13 ? 100 : 'broken', TriggerSingle: i % 2,
    ZoomMax: 1 + i % 4, BarrageMin: '2', BarrageMax: '6.5', BarrageDelay: i / 3,
    CanTargetEnemy: i % 2, CanTargetFriend: i % 3, CanTargetNeutral: i % 5 };
  const ai = { minRange: i / 10, optimalRange: i + 1, maxRange: 50.125, implicitRange: false,
    relationMask: i, categoryMask: -33, barrageMin: 1, barrageMax: 4, barrageDelay: 1.75, zoomMax: 4, action: 2 };
  const registered = i % 3 === 0 ? { ai } : null;
  if (registered) registerAIData({ weapons: { [def.class]: registered } });
  if (i % 4 === 0) def.ai = { ...ai, maxRange: 80.25, categoryMask: 0 };
  output.weapons.push({ def, registered, expected: { ...weaponAI(def) } });
}
const button = { count: 0, code: 0 };
for (let i = 0; i < 2048; i++) {
  const down = rng.int(0, 4) < 3, previous = button.count;
  pressButton(button, down);
  output.buttons.push({ down, count: button.count, code: button.code, trigger: { ...triggerFromCounts(previous, button.count) } });
}
for (let source = -1; source <= 10; source++) for (let target = -1; target <= 10; target++) output.priorities.push({ source, target, expected: matchupPriority(source, target) });
for (const tier of [1, 2, 3]) for (const skill of [0.9, 1, 1.1]) {
  const random = new ClassicRandom(0x91e7 + tier), aimer = new EnhancedAimer(random, AIM_PROFILES[tier], skill), steps = [];
  let calls = 0; const next = random.next.bind(random); random.next = () => { calls++; return next(); };
  for (let i = 0; i < 256; i++) {
    const desired = i % 31 === 0 ? new Vector3(0, 1, 0) : vector().normalize(), time = i / 20;
    // Losing/reacquiring, changing generations, and targets without ids.
    const target = i % 17 < 2 ? null : { id: i % 13 === 0 ? undefined : Math.trunc(i / 19), generation: i % 23 < 3 ? 2 : 1 };
    aimer.observe(target, time);
    const dt = i % 29 === 0 ? 0 : 1 / 20, distance = Math.abs(n(150)), speedFraction = rng.uniform(), scoped = i % 3 === 0, hero = i % 37 === 0;
    const out = aimer.update(desired, dt, { distance, speedFraction, scoped, now: time, hero });
    steps.push({ target, desired: plain(desired), time, dt, distance, speedFraction, scoped, hero, out: plain(out),
      started: aimer.started, error: [...aimer.error], bias: [...aimer.bias], key: aimer.key, lastKey: aimer.lastKey,
      lostAt: aimer.lostAt, acquiredAt: aimer.acquiredAt, readyAt: aimer.readyAt, reacting: aimer.reacting(time), randomState: random.state, calls });
  }
  output.enhanced.push({ tier, skill, seed: 0x91e7 + tier, steps });
}
const bytes = new DataView(new ArrayBuffer(8));
const encoded = JSON.stringify(output, (_, v) => {
  if (typeof v !== 'number') return v;
  bytes.setFloat64(0, v, false);
  return { $f64: bytes.getBigUint64(0, false).toString(16).padStart(16, '0') };
});
fs.writeFileSync(process.argv[2], encoded + '\n');
console.log('7200 heap operations, 2048 direction filters, 1024 Aimer steps, 2048 ballistic leads, 256 weapon records, 2048 trigger steps, 144 priorities, 2304 Enhanced aim steps');
