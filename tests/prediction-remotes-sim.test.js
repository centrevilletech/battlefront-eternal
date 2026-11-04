import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { seededRandom } from '../src/weapons.js';
import { createRemotePredictionSim, createRemotePredictionBridge } from '../src/runtime/remote-prediction-sim.js';
import { predictRemote, RemotePredictionContext } from '../src/runtime/network-remote-prediction.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { actorCourse } from './fixtures/prediction-actor-cases.js';
import { remoteGun, remoteStrike, remoteCommands, remoteObservation, createRemoteFixture, remotePredictionCases, runRemotePredictionCase } from './fixtures/prediction-remote-cases.js';

test('private recon sessions match exact checkpoints, camera math, query order, shots and strikes', async () => {
  const native = createRemotePredictionSim(await loadSim()), cases = remotePredictionCases();
  let ticks = 0, shots = 0, queries = 0;
  try {
    for (const scenario of cases) {
      const a = runRemotePredictionCase(scenario, { native }), b = runRemotePredictionCase(scenario);
      assert.deepEqual(a.observation, b.observation, `${scenario.name} final and omitted state`);
      assert.deepEqual(a.events, b.events, `${scenario.name} query and partial shot order`);
      assert.deepEqual(a.shots, b.shots, `${scenario.name} shot descriptors`);
      assert.equal(a.result?.tick, b.result?.tick); assert.equal(native.activeSessions, 0);
      if (a.result) {
        assert.ok(a.result.nativeSession.steps > 0, `${scenario.name} native delegation`);
        assert.equal(a.result.nativeSession.handle, 0); assert.equal(a.result.nativeSession.disposed, true);
        ticks += a.result.nativeSession.steps;
      }
      if (['wall-crash-and-death', 'fall-below-world-floor', 'health-drain-and-filter', 'destruct-countdown-strike-and-filter'].includes(scenario.name)) assert.equal(a.observation.state.dead, true);
      if (scenario.throwOnFired) { assert.equal(a.shots.length, 1); assert.equal(a.error, a.marker); }
      if (scenario.privateLevelEdit) assert.deepEqual(a.shots[0].partial.state.weapons.map(w => w.heat),
        scenario.privateLevelEdit === 'shared' ? [.1, .3] : [0, 0], 'charge levels keep their actual field snapshots');
      if (scenario.replaceVelocity) assert.equal(a.retainedVelocities.length, 1, 'old velocity remains observable after replacement');
      shots += a.shots.length; queries += a.events.length;
    }
    assert.ok(ticks > 1200 && shots > 100 && queries > 5000);
  } finally { native.dispose(); }
});

test('cached recon restores preserve omitted weapon generators, level writes and private camera motion', async () => {
  const native = createRemotePredictionSim(await loadSim());
  const scenario = { definition: { weapons: [remoteGun({ RoundsPerClip: 90, MaxPressedTime: .3,
    charges: [{ at: .2, ShotsPerSalvo: 2 }, { at: .8, ShotsPerSalvo: 3 }] }), remoteStrike()] } };
  const run = backend => {
    const fixture = createRemoteFixture(scenario), cache = new RemotePredictionContext(), observations = [], { source, droid } = fixture;
    let previous = null;
    for (let frame = 0; frame < 24; frame++) {
      if (frame === 8) droid.networkGeneration++;
      if (frame === 16) droid.owner = { id: 24, generation: 2, team: 2 };
      const { context, predicted } = cache.acquire(source, droid); fixture.setContext(context);
      if (frame === 0 || frame === 8 || frame === 16) {
        assert.notEqual(context, previous); const weapon = predicted.arsenal.weapons[0];
        weapon.level = 73; weapon.salvoSeed = 987; weapon.salvoCount = 8; weapon.salvoRandom = seededRandom(987);
        for (let i = 0; i < 8; i++) weapon.salvoRandom();
        context.hoverTime = 123;
      } else assert.equal(context, previous);
      previous = context;
      if (context.simulationView) { context.simulationView.motion.offset.set(.03, .07, -.02); context.simulationView.motion.turn.set(.01, -.02, .03, 'ZYX'); }
      const weapon = droid.arsenal.weapons[0]; weapon.def = weapon.levels[1]; weapon.level = 99;
      Object.assign(weapon, { charging: frame % 3 !== 0, held: .08, salvosLeft: frame % 3 === 0 ? 2 : 0, salvoWait: .01, wait: 0 });
      droid.position.x += .03125; source.lookX = frame % 3; source.lookY = -(frame % 5);
      const authority = remoteObservation(source, droid); fixture.events.length = 0; fixture.shots.length = 0;
      const result = predictRemote(source, droid, remoteCommands(9, { from: frame }), frame, 1, [], {}, cache, { native: backend });
      if (backend) { assert.equal(result.nativeSession.steps, 9); assert.equal(backend.activeSessions, 0); }
      assert.deepEqual(remoteObservation(source, droid), authority);
      observations.push({ state: remoteObservation(result.context, result.droid), events: fixture.events.slice() });
    }
    cache.reset(); assert.equal(cache.value, null); return observations;
  };
  try { assert.deepEqual(run(native), run(null)); } finally { native.dispose(); }
});

test('recon callbacks can reenter another session and the shared collision kernel across memory growth', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), rays = await loadRayKernel(instance), wire = createWireBridge(bridge), course = actorCourse(rays), reference = actorCourse();
  const traces = [], frozen = []; let grows = 0, nested = false, nestedResult;
  const native = createRemotePredictionBridge(bridge, { trace(event) {
    traces.push(event); frozen.push(structuredClone(event));
    if (traces.length % 17 === 0 && grows < 3) { instance.exports.memory.grow(1); grows++; }
  } });
  const scenario = { ticks: 36, mutate: true, definition: { weapons: [remoteGun({ ShotsPerSalvo: 3 }), remoteStrike()] } };
  const child = { ticks: 18, pattern: 'drive', water: true };
  try {
    const actual = runRemotePredictionCase(scenario, { native, collision: course.world,
      onQuery() { wire.encodeWordDelta(new Uint8Array([1, 7, 9, 3])); },
      onFired() {
        if (!nested) { nested = true; nestedResult = runRemotePredictionCase(child, { native }); }
        course.world.ground(0, 0, 10);
      } });
    const expected = runRemotePredictionCase(scenario, { collision: reference.world, onFired() { reference.world.ground(0, 0, 10); } });
    const expectedNested = runRemotePredictionCase(child);
    assert.deepEqual(actual.observation, expected.observation); assert.deepEqual(actual.events, expected.events);
    assert.deepEqual(nestedResult.observation, expectedNested.observation); assert.deepEqual(nestedResult.events, expectedNested.events);
    assert.deepEqual(traces, frozen, 'callback records keep owned values after memory growth');
    assert.equal(grows, 3); assert.equal(native.activeSessions, 0); assert.ok(new Set(traces.map(t => t.session)).size >= 2);
  } finally { native.dispose(); course.dispose(); reference.dispose(); }
});

test('a throwing foreign query retains its partial mutations and emits no later callback', async () => {
  const native = createRemotePredictionSim(await loadSim());
  const run = backend => {
    const marker = new Error('recon gather stopped'), cache = new RemotePredictionContext(); let armed = false, calls = 0;
    const fixture = createRemoteFixture({}, { onQuery({ name, context }) {
      if (!armed || name !== 'gather') return;
      calls++; const droid = context.droids[0]; droid.health -= .125; droid.position.x += .0625; throw marker;
    } });
    const { source, droid } = fixture, { context, predicted } = cache.acquire(source, droid); fixture.setContext(context);
    const authority = remoteObservation(source, droid); fixture.events.length = 0; armed = true;
    assert.throws(() => predictRemote(source, droid, remoteCommands(5), 0, 1, [], {}, cache, { native: backend }), error => error === marker);
    assert.equal(calls, 1); assert.equal(fixture.events.at(-1)[0], 'gather'); assert.equal(fixture.effects(), 0);
    assert.deepEqual(remoteObservation(source, droid), authority);
    if (backend) assert.equal(backend.activeSessions, 0);
    return { state: remoteObservation(context, predicted), events: fixture.events };
  };
  try { assert.deepEqual(run(native), run(null)); } finally { native.dispose(); }
});

test('completed and empty recon replays release native sessions and permit reuse after callback failure', async () => {
  const native = createRemotePredictionSim(await loadSim()), fixture = createRemoteFixture({}), cache = new RemotePredictionContext();
  try {
    for (let i = 0; i < 40; i++) {
      const pending = i % 3 === 0 ? [] : remoteCommands(2, { generation: i % 3 === 1 ? 1 : 99 });
      const result = predictRemote(fixture.source, fixture.droid, pending, 0, 1, [], {}, cache, { native });
      assert.equal(result.nativeSession.steps, i % 3 === 1 ? 2 : 0);
      assert.equal(result.nativeSession.handle, 0); assert.equal(native.activeSessions, 0);
    }
    runRemotePredictionCase({ ticks: 3, throwOnFired: 1 }, { native });
    assert.equal(native.activeSessions, 0);
    const a = runRemotePredictionCase({ ticks: 3 }, { native }), b = runRemotePredictionCase({ ticks: 3 });
    assert.deepEqual(a.observation, b.observation);
    native.dispose(); native.dispose(); assert.equal(native.activeSessions, 0);
    assert.throws(() => predictRemote(fixture.source, fixture.droid, remoteCommands(1), 0, 1, [], {}, cache, { native }), /disposed/);
  } finally { native.dispose(); }
});
