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

test('closing a recon bridge during a nested shot lets active steps finish and releases every session', async () => {
  const instance = await loadSim(), native = createRemotePredictionSim(instance), other = createRemotePredictionSim(instance);
  const sessions = [], create = native.create;
  native.create = (...args) => { const session = create(...args); sessions.push(session); return session; };
  const idleFixture = createRemoteFixture({}), idleCache = new RemotePredictionContext();
  const idleState = idleCache.acquire(idleFixture.source, idleFixture.droid);
  const idle = native.create(idleState.context, idleState.predicted);
  let child, independent, closed = 0;
  try {
    const outer = runRemotePredictionCase({ ticks: 1 }, { native, onFired() {
      child = runRemotePredictionCase({ ticks: 1 }, { native, onFired() {
        assert.equal(native.activeSessions, 3);
        native.dispose(); native.dispose(); closed++;
        assert.equal(idle.handle, 0);
        assert.equal(native.activeSessions, 2, 'busy owners retain handles until their steps unwind');
        for (const session of sessions.slice(1)) assert.notEqual(session.handle, 0);
        assert.throws(() => native.create(idleState.context, idleState.predicted), /bridge is disposed/);
        assert.throws(() => sessions[1].step(remoteCommands(1)[0], {}), /bridge is disposed/);
        independent = runRemotePredictionCase({ ticks: 1 }, { native: other });
      } });
      assert.equal(native.activeSessions, 1, 'the outer step still owns its suspended continuation');
      assert.equal(child.result.nativeSession.handle, 0);
    } });
    const reference = runRemotePredictionCase({ ticks: 1 });
    for (const result of [outer, child, independent]) {
      assert.deepEqual(result.observation, reference.observation);
      assert.deepEqual(result.events, reference.events);
      assert.equal(result.result.nativeSession.steps, 1);
    }
    assert.equal(closed, 1); assert.equal(native.activeSessions, 0);
    for (const session of sessions) { assert.equal(session.handle, 0); assert.equal(session.disposed, true); }
    const reused = runRemotePredictionCase({ ticks: 2 }, { native: other });
    assert.deepEqual(reused.observation, runRemotePredictionCase({ ticks: 2 }).observation);
  } finally { native.dispose(); other.dispose(); }
});

test('deferred recon disposal preserves callback-thrown values and partial state', async () => {
  const instance = await loadSim();
  const observe = (native, marker) => {
    const cache = new RemotePredictionContext(); let calls = 0;
    const fixture = createRemoteFixture({}, { onFired() { calls++; native?.dispose(); throw marker; } });
    const { source, droid } = fixture, { context, predicted } = cache.acquire(source, droid);
    fixture.setContext(context); fixture.events.length = 0;
    let caught = Symbol('no callback error');
    try { predictRemote(source, droid, remoteCommands(3), 0, 1, [], {}, cache, { native }); }
    catch (error) { caught = error; }
    assert.equal(caught, marker); assert.equal(calls, 1);
    assert.equal(fixture.events.at(-1)[0], 'fired', 'no later foreign callback runs after the throw');
    return { state: remoteObservation(context, predicted), events: fixture.events };
  };
  for (const marker of [new Error('closed from shot'), null, undefined]) {
    const native = createRemotePredictionSim(instance), sessions = [], create = native.create;
    native.create = (...args) => { const session = create(...args); sessions.push(session); return session; };
    try {
      assert.deepEqual(observe(native, marker), observe(null, marker));
      assert.equal(native.activeSessions, 0);
      assert.equal(sessions.length, 1); assert.equal(sessions[0].handle, 0);
    } finally { native.dispose(); }
  }
  const other = createRemotePredictionSim(instance);
  try { assert.equal(runRemotePredictionCase({ ticks: 1 }, { native: other }).result.nativeSession.steps, 1); }
  finally { other.dispose(); }
});

test('disposing a busy recon session retires it after the step without closing its bridge', async () => {
  const native = createRemotePredictionSim(await loadSim()), create = native.create;
  let session, disposed = 0;
  native.create = (...args) => (session = create(...args));
  try {
    const result = runRemotePredictionCase({ ticks: 1 }, { native, onFired() {
      session.dispose(); session.dispose(); disposed++;
      assert.notEqual(session.handle, 0); assert.equal(native.activeSessions, 1);
      assert.throws(() => session.step(remoteCommands(1)[0], {}), /already running/);
    } });
    assert.equal(disposed, 1); assert.equal(result.result.nativeSession.handle, 0);
    assert.equal(native.activeSessions, 0);
    assert.deepEqual(result.observation, runRemotePredictionCase({ ticks: 1 }).observation);
    assert.equal(runRemotePredictionCase({ ticks: 2 }, { native }).result.nativeSession.steps, 2);
  } finally { native.dispose(); }
});

test('recon projection callbacks see the original pose and stop immediately on throw or pilot exit', async () => {
  let mutatedTrace = 0;
  const native = createRemotePredictionSim(await loadSim(), { trace(event) {
    if (event.output.length) { event.output[0] = 99999; mutatedTrace++; }
    if (event.input.length) event.input[0] = NaN;
  } });
  const run = (backend, throwing) => {
    const fixture = createRemoteFixture({}), cache = new RemotePredictionContext(), { source, droid } = fixture;
    const { context, predicted } = cache.acquire(source, droid); fixture.setContext(context);
    const authority = remoteObservation(source, droid), camera = context.aimCamera, update = camera.updateProjectionMatrix;
    const marker = new Error('recon projection stopped'); let calls = 0;
    fixture.events.length = 0;
    camera.updateProjectionMatrix = function () {
      calls++; fixture.events.push(['projection', this.fov, this.aspect, this.position.toArray(), this.quaternion.toArray(), context.active]);
      if (throwing && calls === 2) throw marker;
      return update.call(this);
    };
    const commands = remoteCommands(7);
    commands[0].actions = ['use']; commands[0].firstPerson = false;
    for (let i = 0; i < commands.length; i++) commands[i].aspect = 1.2 + i * .1;
    let result;
    if (throwing) {
      assert.throws(() => predictRemote(source, droid, commands, 0, 1, [], {}, cache, { native: backend }), error => error === marker);
      assert.equal(fixture.events.at(-1)[0], 'projection');
    } else {
      result = predictRemote(source, droid, commands, 0, 1, [], {}, cache, { native: backend });
      assert.equal(result.context.active, false); assert.equal(camera.aspect, commands.at(-1).aspect);
      if (backend) assert.equal(result.nativeSession.steps, 7);
    }
    assert.equal(calls, 2, 'restore and the last piloted frame each update projection once');
    assert.deepEqual(remoteObservation(source, droid), authority); if (backend) assert.equal(backend.activeSessions, 0);
    return { state: remoteObservation(context, predicted), events: fixture.events };
  };
  try {
    for (const throwing of [false, true]) assert.deepEqual(run(native, throwing), run(null, throwing));
    assert.ok(mutatedTrace > 10, 'diagnostic copies cannot change native continuation outputs');
  } finally { native.dispose(); }
});
