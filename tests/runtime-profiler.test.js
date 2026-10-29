import assert from 'node:assert/strict';
import test from 'node:test';
import { RuntimeProfiler } from '../src/runtime-profiler.js';

test('exclusive CPU phases, simulation steps and clamped visual time remain distinct', () => {
  let time = 100;
  const profile = new RuntimeProfiler({ now: () => time, timeOrigin: 0 });
  profile.begin(.1, .15);
  time += 1; profile.phase('simulation'); profile.step(1 / 60); profile.step(1 / 60);
  time += 5; profile.phase('presentation');
  time += 2; profile.phase('render');
  time += 3; profile.end(true);
  const { metrics: m } = profile.report();
  assert.equal(m.cpuMs.total, 11);
  assert.equal(m.input.total + m.simulation.total + m.presentation.total + m.render.total, 11);
  assert.equal(m.steps.total, 2);
  assert.ok(Math.abs(m.simulatedMs.total - 1000 / 30) < 1e-9);
  assert.ok(Math.abs(m.visualClampMs.total - 50) < 1e-9);
});

test('input timestamp waits for a drawn frame and is never counted twice', () => {
  let time = 100;
  const profile = new RuntimeProfiler({ now: () => time, timeOrigin: 1700000000000 });
  profile.noteInput({ timeStamp: 1700000000090 });
  profile.begin(1 / 60); time = 110; profile.end(false);
  assert.equal(profile.report().pendingInputs, 1);
  assert.equal(profile.report().metrics.inputToSubmitMs.samples, 0);
  profile.begin(1 / 60); time = 115; profile.end(true);
  profile.begin(1 / 60); time = 120; profile.end(true);
  const { metrics: m } = profile.report();
  assert.equal(m.inputCount.total, 1);
  assert.equal(m.eventDispatchDelayMs.max, 10);
  assert.equal(m.inputToSubmitMs.max, 25);
  assert.equal(m.inputToSubmitMs.samples, 1);
});

test('recording uses a bounded ring and reset excludes old samples', () => {
  let time = 0;
  const profile = new RuntimeProfiler({ capacity: 2, now: () => time, timeOrigin: 0 });
  for (const duration of [100, 2, 3]) { profile.begin(.01); time += duration; profile.end(true); }
  assert.equal(profile.report().overwritten, 1);
  assert.equal(profile.report().metrics.cpuMs.max, 3);
  profile.reset();
  assert.equal(profile.report().retained, 0);
  assert.equal(profile.report().metrics.cpuMs.max, null);
});
