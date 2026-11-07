import test from 'node:test';
import assert from 'node:assert/strict';
import { Session } from 'node:inspector/promises';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/sim/wasm/ray-kernel.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { Unit } from '../src/units.js';
import { hullFixture } from './fixtures/client-hull.js';
import { ClientHullContent } from '../src/sim/wasm/runtime/client-hull-content.js';
import { createClientPrediction } from '../src/sim/wasm/runtime/client-prediction.js';
import { MountedWeaponPrediction } from '../src/runtime/mounted-weapon-prediction.js';
import { Weapon } from '../src/weapons.js';
import { captureGroundCorrection, GROUND_CORRECTION_SCHEMA } from '../src/runtime/network-entity-ground.js';
import { captureFlightCorrection, FLIGHT_CORRECTION_SCHEMA } from '../src/runtime/network-entity-flight.js';

// Exercise the page's resource-only instance, not a canonical world fixture.
// Native allocator counters independently cover one and 120 pending ticks.
// A battle retains its fleet, including hulls the player previously boarded.
// Keep those shapes alive too: collecting an earlier fixture invalidates V8's
// compiled property loads during the next fixture's sampling interval.
const battles = [];
test.after(() => { for (const battle of battles) battle.dispose(); });
for (const kind of ['springs', 'wheeled', 'walker-legs', 'flyer-targets']) test(`warm ${kind} page frames create no sampled JavaScript objects`, async () => {
  const instance = await loadSim(), f = hullFixture(kind, await loadRayKernel(instance)), v = f.vehicle;
  battles.push(f.system);
  v.resetPresentationHistory();
  if (v.flying) { v.flightState = 'Flying'; v.position.y = 100; }
  f.system.predictionContent = new ClientHullContent(f.system,
    bridgeFor(instance, { classes: { Unit, Vehicle: f.Vehicle } }),
    { killY: -300, deathRegions: [], scripted: [] }, f);
  const prediction = createClientPrediction(instance, f.system.collision);
  f.system.predictionContent.adopt(v);
  // The synthetic rig has no muzzle bones; its separately borrowed arsenal
  // exercises the same frame owner without altering immutable pose topology.
  // Include the real borrowed arsenal and its restore/publish work. Empty
  // hulls alone cannot prove that a piloted weapon retains frame storage.
  v.seats[0].weapons.push({ state: new Weapon({ class: 'seat-cannon', base: 'cannon', channel: 0,
    RoundsPerClip: 6, ammo: 5, ShotDelay: .1, ReloadTime: .3, ShotsPerSalvo: 1 }, { random: () => .5 }),
    barrel: 0, aimers: [], muzzles: [{}] });
  const mounted = new MountedWeaponPrediction(prediction);
  const unit = { id: 1, generation: 1, seat: { vehicle: v, index: 0 }, alive: true, combat: { dead: false } };
  const held = ['KeyW', 'Mouse0'], command = { tick: 1, generation: 1, sequence: 1, epoch: 1,
    down: held, actions: [], look: [[.25, -.1]], mouseScale: 1, flightSettings: { sensitivity: 1 } }, pending = [command];
  const bytes = v.flying ? captureFlightCorrection(v) : captureGroundCorrection(v);
  const schema = v.flying ? FLIGHT_CORRECTION_SCHEMA : GROUND_CORRECTION_SCHEMA;
  const frame = () => {
    command.tick++; command.sequence++;
    mounted.predict(unit, pending, command.tick - 1, held, v, 1, bytes, schema);
  };
  const profiler = new Session(); profiler.connect();
  try {
    // Real page frames yield between calls. Give V8 the same opportunity to
    // install its compiled hot loops before sampling retained frame storage.
    for (let block = 0; block < 100; block++) {
      for (let i = 0; i < 100; i++) frame();
      await new Promise(resolve => setTimeout(resolve, 2));
    }
    const options = { samplingInterval: 1, includeObjectsCollectedByMajorGC: true, includeObjectsCollectedByMinorGC: true };
    // The profiler allocates stack metadata on its first observation too.
    // Warm that separately so its own bookkeeping cannot look like game work.
    // Sampling changes V8's allocation-stack metadata and can trigger another
    // optimization tier. Let those compiled loops settle between intervals.
    for (let interval = 0; interval < 5; interval++) {
      await profiler.post('HeapProfiler.startSampling', options);
      for (let i = 0; i < 1000; i++) frame();
      await profiler.post('HeapProfiler.stopSampling');
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    await profiler.post('HeapProfiler.startSampling', options);
    for (let i = 0; i < 1000; i++) frame();
    const { profile } = await profiler.post('HeapProfiler.stopSampling');
    const allocations = [];
    function visit(node) {
      if (node.selfSize && /\/src\/(?:runtime\/|sim\/gen\/)/.test(node.callFrame.url)) allocations.push({ function: node.callFrame.functionName, line: node.callFrame.lineNumber + 1, bytes: node.selfSize });
      for (const child of node.children ?? []) visit(child);
    }
    visit(profile.head);
    assert.deepEqual(allocations, []);
    assert.equal(prediction.counters, undefined, 'play has no diagnostic counter updates');
  } finally { profiler.disconnect(); mounted.dispose(); prediction.dispose(); }
});
