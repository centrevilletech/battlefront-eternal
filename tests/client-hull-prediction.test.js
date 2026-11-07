import {replayOutcomes} from './helpers/replay-outcomes.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { attachF2 } from '../src/f2-sim.js';
import { ClientHullContent } from '../src/runtime/client-hull-content.js';
import { Unit } from '../src/units.js';
import { hullFixture, hullCommand } from './fixtures/client-hull.js';
import { createClientPrediction } from '../src/runtime/client-prediction.js';
import { captureGroundCorrection, restoreGroundCorrection } from '../src/runtime/network-entity-ground.js';
import { captureFlightCorrection, restoreFlightCorrection } from '../src/runtime/network-entity-flight.js';

// Frozen before native hull replay replaced predictVehicle. Each digest pins
// IEEE correction bytes and interpolation/gait endpoints, rather than a twin.
// The advanced corpus also exercises suspension, wheels, legs and target locks.
const corpora = [
  ['hulls', '9d5d9dc0750a535ad5ddcb7edc58631b9ef78cb0231307aed84b4aca1c8369da', false],
  ['hulls-continuation', '71d81c396f4cf1a02c610131294ee5f25b77cb28c3d5f567229285522cac32d9', true],
  ['hulls-advanced', 'ff644cc68a00a0ee51de955988bbd2b47cd7b1d2a792cfd06ab768365b3ec7c0', false],
];
for (const [name, digest, continuation] of corpora) {
  const text = gunzipSync(readFileSync(new URL(`./fixtures/prediction-reference/${name}.json.gz`, import.meta.url))).toString();
  assert.equal(createHash('sha256').update(text).digest('hex'), digest);
  const records = text.trimEnd().split('\n').map(line => JSON.parse(line));
  for (const kind of new Set(records.map(row => row.kind))) test(`${name}: ${kind} reproduces frozen correction and checkpoint bytes`, async () => {
    const instance = await loadSim(), kernel = await loadRayKernel(instance);
    const f = hullFixture(kind, kernel), { system, vehicle: v } = f;
    v.resetPresentationHistory();
    const bridge = bridgeFor(instance, { classes: { Unit, Vehicle: f.Vehicle } });
    const map = { killY: -300, deathRegions: [], scripted: [] };
    // Fresh replay also exercises the actual page: content plus replicas,
    // without a canonical F2 fleet or its world lifecycle in this instance.
    if (continuation) attachF2(system, bridge, map, f, Unit);
    else system.predictionContent = new ClientHullContent(system, bridge, map, f);
    const session = createClientPrediction(instance, system.collision);
    const lease = { vehicle: v, index: 0 }, unit = { id: 1, generation: 1, seat: lease, alive: true, combat: { dead: false } };
    let pending = [], held = [], authority = 0;
    try {
      for (const row of records.filter(row => row.kind === kind)) {
        const c = hullCommand(row.tick);
        if (kind === 'flyer-targets' && [60, 120].includes(row.tick)) c.actions.push('targetLock');
        pending.push(c);
        // Forced restores check the full replay; continuations check native
        // byte eligibility and command retirement against the frozen window.
        if (!continuation) session.reset();
        const before = v.position.toArray(), matrix = v.body.matrix.toArray(), enabled = v.body.enabled;
        session.replaySources([],replayOutcomes(pending),held);
        session.mounted(unit, [], pending, authority, held, v, 1, v.flying ? captureFlightCorrection(v) : captureGroundCorrection(v), v.flying ? 2 : 10);
        const predicted = session.vehicle;
        const correction = v.flying ? captureFlightCorrection(predicted) : captureGroundCorrection(predicted);
        assert.equal(Buffer.from(session.correction()).toString('base64'), row.bytes, `${kind} native checkpoint ${row.tick}`);
        assert.equal(Buffer.from(correction).toString('base64'), row.bytes, `${kind} presentation ${row.tick}`);
        assert.deepEqual([predicted.walkPhase, predicted.walkBlend, predicted.walkFromPhase, predicted.walkFromBlend], row.walk);
        assert.deepEqual(predicted.wheels.map(w => [w.phase, w.previous, w.tickFrom]), row.wheels);
        assert.deepEqual(v.position.toArray(), before, 'authority pose is private');
        assert.deepEqual(v.body.matrix.toArray(), matrix, 'authority collider never moves');
        assert.equal(v.body.enabled, enabled, 'query flags cannot escape the private view');
        if (row.tick % 12 === 0) {
          if (v.flying) restoreFlightCorrection(v, correction); else restoreGroundCorrection(v, correction);
          authority = row.tick; held = c.down; pending = [];
        }
      }
    } finally { session.dispose(); system.dispose(); }
  });
}

// The old window could accept a correction older than 60 ticks after one
// long replay. Its wheel phase proves continuation, since ground wire omits it.
test('delayed exact ground correction keeps the entire preceding replay history', async () => {
  const text = readFileSync(new URL('./fixtures/prediction-reference/hull-delayed.json', import.meta.url), 'utf8');
  assert.equal(createHash('sha256').update(text).digest('hex'), '96422a0d8fed4f1833e8406d81bac5a72c6254c624883167a8b648044347f57a');
  const frozen = JSON.parse(text), instance = await loadSim(), kernel = await loadRayKernel(instance);
  const f = hullFixture('wheeled', kernel), v = f.vehicle; v.resetPresentationHistory();
  attachF2(f.system, bridgeFor(instance, { classes: { Unit, Vehicle: f.Vehicle } }),
    { killY: -300, deathRegions: [], scripted: [] }, f, Unit);
  const session = createClientPrediction(instance, f.system.collision);
  const unit = { id: 1, generation: 1, seat: { vehicle: v, index: 0 } };
  const pending = Array.from({ length: 101 }, (_, i) => hullCommand(i + 1));
  try {
    session.mounted(unit, [], pending.slice(0, 100), 0, [], v, 1, captureGroundCorrection(v), 10);
    assert.deepEqual(session.vehicle.wheels.map(w => [w.phase, w.previous, w.tickFrom]), frozen.first.wheels);
    restoreGroundCorrection(v, Uint8Array.from(Buffer.from(frozen.authority, 'base64')));
    session.mounted(unit, [], pending.slice(20), 20, pending[19].down, v, 1, captureGroundCorrection(v), 10);
    assert.equal(session.result.continued, true); assert.equal(session.result.ticks, 1);
    assert.equal(Buffer.from(session.correction()).toString('base64'), frozen.last.bytes);
    assert.deepEqual(session.vehicle.wheels.map(w => [w.phase, w.previous, w.tickFrom]), frozen.last.wheels);
  } finally { session.dispose(); f.system.dispose(); }
});

// Full checkpoints and compact authority bodies restore the same active
// solver state. Exercise each conditional transition/trick branch in CI.
test('native compact flight correction matches the full checkpoint through maneuvers', async () => {
  const { captureCompactFlight, restoreCompactFlight, COMPACT_FLIGHT_SCHEMA } = await import('../src/runtime/network-entity-flight-compact.js');
  const instance = await loadSim(), f = hullFixture('flyer', await loadRayKernel(instance)), v = f.vehicle;
  v.resetPresentationHistory();
  attachF2(f.system, bridgeFor(instance, { classes: { Unit, Vehicle: f.Vehicle } }),
    { killY: -300, deathRegions: [], scripted: [] }, f, Unit);
  const full = createClientPrediction(instance, f.system.collision), compact = createClientPrediction(instance, f.system.collision);
  const unit = { id: 1, generation: 1, seat: { vehicle: v, index: 0 } };
  let pending = [], authority = 0, held = [];
  try {
    for (let tick = 1; tick <= 420; tick++) {
      const command = hullCommand(tick);
      if ([160, 166].includes(tick)) command.actions.push('trick');
      if (tick === 260) command.actions.push('immelmann');
      pending.push(command);
      const wire = captureCompactFlight(v);
      restoreCompactFlight(v, wire);
      full.reset(); compact.reset();
      full.mounted(unit, [], pending, authority, held, v, 1, captureFlightCorrection(v), 2);
      compact.mounted(unit, [], pending, authority, held, v, 1, wire, COMPACT_FLIGHT_SCHEMA);
      assert.deepEqual(compact.correction(), full.correction(), `compact replay tick ${tick}`);
      if (tick % 12 === 0) {
        restoreCompactFlight(v, captureCompactFlight(full.vehicle));
        authority = tick; held = command.down; pending = [];
      }
    }
  } finally { full.dispose(); compact.dispose(); f.system.dispose(); }
});

// A passenger/infantry detour can rebuild the arsenal without a new hull life.
// Cold actor setup clears the native hull too; reboarding must configure it.
test('a replacement lease reconfigures the same live hull', async () => {
  const instance = await loadSim(), f = hullFixture('hover', await loadRayKernel(instance)), v = f.vehicle;
  v.resetPresentationHistory();
  f.system.predictionContent = new ClientHullContent(f.system,
    bridgeFor(instance, { classes: { Unit, Vehicle: f.Vehicle } }),
    { killY: -300, deathRegions: [], scripted: [] }, f);
  const session = createClientPrediction(instance, f.system.collision);
  const unit = { id: 1, generation: 1, seat: { vehicle: v, index: 0 } }, pending = [hullCommand(1)], bytes = captureGroundCorrection(v);
  try {
    session.mounted(unit, [], pending, 0, [], v, 1, bytes, 10);
    const before = session.correction();
    unit.seat = { vehicle: v, index: 0 };
    session.mounted(unit, [], pending, 0, [], v, 1, bytes, 10);
    assert.deepEqual(session.correction(), before);
  } finally { session.dispose(); f.system.dispose(); }
});
