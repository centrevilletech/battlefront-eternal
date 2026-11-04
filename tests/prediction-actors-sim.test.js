import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { MovementKernel } from '../src/movement-kernel.js';
import { InfantryPrediction } from '../src/runtime/infantry-prediction.js';
import { DroidekaPrediction } from '../src/runtime/droideka-prediction.js';
import { createActorPredictionSim, createActorPredictionBridge } from '../src/runtime/actor-prediction-sim.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { actorCourse, createPredictionActor, actorPredictionCases, actorInput, actorObservation, runActorPredictionCase } from './fixtures/prediction-actor-cases.js';

test('private actors match exact correction bytes, recoil, shield ordering, root travel and quiet death', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), coverage = {};
  let ticks = 0;
  for (const scenario of actorPredictionCases()) {
    let actual, expected;
    try {
      actual = runActorPredictionCase(scenario, { rays, makeNative: world => createActorPredictionSim(instance, world),
        after({ prediction, fixture }) { assert.ok(prediction.nativeSession.handle); assert.ok(prediction.nativeSession.steps > 0);
          if (scenario.kind === 'soldier') assert.equal(fixture.soldier.mover, -1, 'private session never registers an authoritative mover'); } });
      expected = runActorPredictionCase(scenario);
      assert.deepEqual(actual.observations, expected.observations, scenario.name);
      ticks += actual.observations.length;
      for (const [key, value] of Object.entries(actual.coverage)) if (typeof value === 'number') coverage[key] = (coverage[key] || 0) + value;
      if (scenario.pattern === 'unfold') assert.ok(actual.observations.every(o => o.state.mode === 12), 'low ceiling prevents unfolding');
      if (scenario.pattern === 'reverse') assert.ok(actual.observations.filter(o => o.state.reverse).length > 50, 'reverse root motion exercised');
      if (scenario.capturedFlags) { assert.equal(actual.observations[1].shield.on, true); assert.equal(actual.observations[18].shield.on, true); }
    } finally { actual?.dispose(); expected?.dispose(); }
  }
  assert.ok(ticks > 3500);
  for (const key of ['jump', 'fall', 'thrown', 'jet', 'prone', 'crouch', 'dive', 'lock', 'shield', 'stepBlocked']) assert.ok(coverage[key] > 20, `${key} exercised`);
});

test('native actor correction preserves seeded collision scratch until the JS solver overwrites it', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), scenario = actorPredictionCases().find(c => c.seededScratch);
  let a, b;
  try {
    a = runActorPredictionCase(scenario, { rays, makeNative: world => createActorPredictionSim(instance, world) }); b = runActorPredictionCase(scenario);
    assert.deepEqual(a.observations, b.observations);
    assert.deepEqual(a.observations[0].ingressScratch, [3, [.125, -.25, 0], true]);
    assert.equal(a.observations[0].state.contactCount, 0);
    assert.ok(a.coverage.stepBlocked > 100);
    for (let tick = 23; tick < scenario.ticks; tick += 23) assert.equal(a.observations[tick].ingressScratch[0], tick + 3);
  } finally { a?.dispose(); b?.dispose(); }
});

test('actor outputs survive interleaved owners, same-owner guards and memory growth after native publication', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), bridge = bridgeFor(instance), wire = createWireBridge(bridge);
  const course = actorCourse(rays), reference = actorCourse(), scenario = { kind: 'soldier', pattern: 'jet', definition: { JetType: 'hover', JetFuelRechargeRate: .1 } };
  const droidCase = { kind: 'droideka', pattern: 'droideka' }, a = createPredictionActor(scenario, course), b = createPredictionActor(droidCase, course);
  const ra = createPredictionActor(scenario, reference), rb = createPredictionActor(droidCase, reference), traces = [];
  let p, q, nested = false, once = false, grew = 0;
  const native = createActorPredictionBridge(bridge, course.world, { trace(event) {
    traces.push(structuredClone(event));
    wire.encodeWordDelta(new Uint8Array([1, 3, 7, 15]));
    if (event.op === 1 && !nested) {
      nested = true;
      try {
        assert.throws(() => p.step(actorInput(scenario, 0), 1 / 60), /busy/);
        if (!once) { once = true; q.step(actorInput(droidCase, 0), 1 / 60, true); }
        if (grew < 3) { instance.exports.memory.grow(1); grew++; }
      } finally { nested = false; }
    }
  } });
  p = new InfantryPrediction(a.soldier, a.combat, undefined, { native }); q = new DroidekaPrediction(b.soldier, b.combat, undefined, { native });
  const rp = new InfantryPrediction(ra.soldier, ra.combat), rq = new DroidekaPrediction(rb.soldier, rb.combat);
  try {
    for (let tick = 0; tick < 90; tick++) {
      p.step(actorInput(scenario, tick), 1 / 60); rp.step(actorInput(scenario, tick), 1 / 60);
      if (tick === 0) rq.step(actorInput(droidCase, 0), 1 / 60, true);
      assert.deepEqual(actorObservation(a.soldier, p.kick), actorObservation(ra.soldier, rp.kick));
      assert.deepEqual(actorObservation(b.soldier, q.kick), actorObservation(rb.soldier, rq.kick));
    }
    assert.ok(once && grew === 3 && traces.length > 90);
    for (const t of traces) { assert.ok(Array.isArray(t.input)); assert.ok(Array.isArray(t.output)); }
  } finally { p.dispose(); q.dispose(); native.dispose(); course.dispose(); reference.dispose(); }
});

test('fresh wrappers reuse a body owner while disposal, corrections and class changes stay isolated', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), course = actorCourse(rays), native = createActorPredictionSim(instance, course.world);
  const scenario = { kind: 'soldier', pattern: 'walk' }, actor = createPredictionActor(scenario, course), handles = [];
  let kick, handle, last;
  try {
    for (let frame = 0; frame < 80; frame++) {
      const p = new InfantryPrediction(actor.soldier, actor.combat, kick, { native });
      actor.soldier.position.x += .015625; actor.soldier.maxSpeed = frame % 2 ? 9 : 7;
      p.step(actorInput(scenario, frame), 1 / 60);
      assert.ok(p.nativeSession.steps > 0); handle ??= p.nativeSession.handle; assert.equal(p.nativeSession.handle, handle);
      kick = p.kick; handles.push(p.nativeSession); p.dispose(); p.dispose(); assert.equal(p.nativeSession.handle, 0); last = p;
    }
    assert.deepEqual(actor.combat.kick, actor.kick); assert.ok(handles.every(h => h.handle === 0));
    assert.throws(() => last.step(actorInput(scenario, 0), 1 / 60), /disposed/);
    const p = new InfantryPrediction(actor.soldier, actor.combat, kick, { native }); p.step(actorInput(scenario, 0), 1 / 60);
    native.dispose(); native.dispose(); assert.equal(p.nativeSession.handle, 0);
    assert.throws(() => p.step(actorInput(scenario, 0), 1 / 60), /disposed/); p.dispose();
  } finally { native.dispose(); course.dispose(); }
});

test('custom recoil, foreign worlds and unknown actor states retain the existing JavaScript path', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), course = actorCourse(rays), foreign = actorCourse();
  const native = createActorPredictionSim(instance, course.world), scenario = { kind: 'soldier', pattern: 'walk' };
  try {
    const actor = createPredictionActor(scenario, course); let custom = 0;
    actor.combat.updateKick = function (dt) { this.kick.extra = ++custom; this.soldier.aimPitch += dt; };
    const p = new InfantryPrediction(actor.soldier, actor.combat, undefined, { native }); p.step(actorInput(scenario, 0), 1 / 60);
    assert.equal(custom, 1); assert.equal(p.nativeSession.steps, 0); assert.equal(actor.combat.kick.extra, 'retained'); p.dispose();
    const other = createPredictionActor(scenario, foreign), q = new InfantryPrediction(other.soldier, other.combat, undefined, { native });
    q.step(actorInput(scenario, 0), 1 / 60); assert.equal(q.nativeSession.steps, 0); q.dispose();
    const unusual = createPredictionActor(scenario, course); unusual.soldier.air = 'swimming';
    const r = new InfantryPrediction(unusual.soldier, unusual.combat, undefined, { native }); r.step(actorInput(scenario, 0), 1 / 60); assert.equal(r.nativeSession.steps, 0); r.dispose();
  } finally { native.dispose(); course.dispose(); foreign.dispose(); }
});

test('a movement-kernel-backed actor keeps the same retained JS scratch fields', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), a = actorCourse(rays), b = actorCourse(rays);
  a.world.movement = new MovementKernel(a.world); b.world.movement = new MovementKernel(b.world);
  const scenario = actorPredictionCases().find(c => c.seededScratch), actual = createPredictionActor(scenario, a), expected = createPredictionActor(scenario, b);
  const native = createActorPredictionSim(instance, a.world), p = new InfantryPrediction(actual.soldier, actual.combat, undefined, { native }), q = new InfantryPrediction(expected.soldier, expected.combat);
  try {
    for (let tick = 0; tick < 90; tick++) {
      p.step(actorInput(scenario, tick), 1 / 60); q.step(actorInput(scenario, tick), 1 / 60);
      assert.ok(p.nativeSession.steps > 0); assert.deepEqual(actorObservation(actual.soldier, p.kick), actorObservation(expected.soldier, q.kick));
    }
    assert.equal(actual.soldier.contactCount, 7); assert.deepEqual(actual.soldier.contactMean.toArray(), [.25, -.5, 1.5]);
  } finally { p.dispose(); native.dispose(); a.dispose(); b.dispose(); }
});

test('private actor clips observe in-place data and track changes between steps', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), course = actorCourse(rays), reference = actorCourse();
  const scenario = { kind: 'droideka', pattern: 'reverse' }, a = createPredictionActor(scenario, course), b = createPredictionActor(scenario, reference);
  const native = createActorPredictionSim(instance, course.world), p = new DroidekaPrediction(a.soldier, a.combat, undefined, { native }), q = new DroidekaPrediction(b.soldier, b.combat);
  try {
    for (let tick = 0; tick < 120; tick++) {
      if (tick % 10 === 0) for (const actor of [a, b]) for (const [name, clip] of actor.clips) if (name.includes('foot')) {
        clip.data[6] += .125; clip.data[clip.data.length - 1] += .25; clip.tracks[0] = tick % 20 === 0 ? -1 : 0;
      }
      p.step(actorInput(scenario, tick), 1 / 60); q.step(actorInput(scenario, tick), 1 / 60);
      assert.equal(p.nativeSession.steps, tick + 1, 'edited ordinary clips continue natively');
      assert.deepEqual(actorObservation(a.soldier, p.kick), actorObservation(b.soldier, q.kick), `clip edit tick ${tick}`);
    }
  } finally { p.dispose(); native.dispose(); course.dispose(); reference.dispose(); }
});

test('custom clip accessors and track methods retain JavaScript call order', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance);
  for (const change of ['data-getter', 'track-indexOf']) {
    const run = useNative => {
      const course = actorCourse(useNative ? rays : null), scenario = { kind: 'droideka', pattern: 'reverse' }, actor = createPredictionActor(scenario, course), calls = [];
      for (const [name, clip] of actor.clips) if (name.includes('foot')) {
        if (change === 'data-getter') {
          const data = clip.data;
          Object.defineProperty(clip, 'data', { configurable: true, get() { calls.push(name); return data; } });
        } else {
          const indexOf = clip.tracks.indexOf;
          clip.tracks.indexOf = function (...args) { calls.push([name, ...args]); return indexOf.apply(this, args); };
        }
      }
      const native = useNative ? createActorPredictionSim(instance, course.world) : null;
      const p = new DroidekaPrediction(actor.soldier, actor.combat, undefined, { native }), observations = [];
      try {
        for (let tick = 0; tick < 30; tick++) {
          p.step(actorInput(scenario, tick), 1 / 60); observations.push(actorObservation(actor.soldier, p.kick));
          if (native) assert.equal(p.nativeSession.steps, 0);
        }
        assert.ok(calls.length > 0); return { observations, calls };
      } finally { p.dispose(); native?.dispose(); course.dispose(); }
    };
    assert.deepEqual(run(true), run(false), change);
  }
});

test('disposal during collision preparation happens before native actor entry', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), course = actorCourse(rays), scenario = { kind: 'soldier', pattern: 'walk' };
  const actor = createPredictionActor(scenario, course), native = createActorPredictionSim(instance, course.world);
  const p = new InfantryPrediction(actor.soldier, actor.combat, undefined, { native }), before = actorObservation(actor.soldier, p.kick);
  let calls = 0;
  Object.defineProperty(course.world, 'rayOrder', { configurable: true, get() { calls++; native.dispose(); return course.world.bodiesVersion; } });
  try {
    assert.throws(() => p.step(actorInput(scenario, 0), 1 / 60), /disposed/);
    assert.equal(calls, 1); assert.equal(p.nativeSession.steps, 0); assert.equal(p.nativeSession.handle, 0);
    assert.deepEqual(actorObservation(actor.soldier, p.kick), before);
  } finally { p.dispose(); native.dispose(); course.dispose(); }
});
