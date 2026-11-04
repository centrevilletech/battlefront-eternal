import test from 'node:test';
import assert from 'node:assert/strict';
import { Weapon } from '../src/weapons.js';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { WeaponPrediction } from '../src/runtime/weapon-prediction.js';
import { MountedWeaponPrediction } from '../src/runtime/mounted-weapon-prediction.js';
import { createWeaponPredictionSim, createWeaponPredictionBridge } from '../src/runtime/weapon-prediction-sim.js';
import { captureWeaponState } from '../src/runtime/network-weapon-state.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { weaponDefinition, createWeaponUnit, arsenalObservation, weaponObservation, weaponPredictionCases, runWeaponPredictionCase,
  mountedPredictionCases, runMountedPredictionCase } from './fixtures/prediction-weapon-cases.js';

test('native private weapons match seeded shots, checkpoints, selection, blocking and melee', async () => {
  const native = createWeaponPredictionSim(await loadSim());
  let samples = 0, shots = 0;
  for (const scenario of weaponPredictionCases()) {
    let a, b;
    try {
      a = runWeaponPredictionCase(scenario, { native, after({ prediction, operation }) {
        if (['begin', 'step'].includes(operation.op)) assert.ok(prediction.nativeSession?.handle, `${scenario.name} delegated`);
      } });
      b = runWeaponPredictionCase(scenario);
      assert.deepEqual(a.observations, b.observations, scenario.name);
      samples += a.observations.length; shots += a.observations.reduce((n, o) => n + o.shots.length, 0);
      assert.equal(a.unit.combat.arsenal, a.authority, `${scenario.name} authority restored`);
    } finally { a?.prediction.dispose(); b?.prediction.restore(); b?.prediction.dispose?.(); }
  }
  assert.ok(samples >= 900); assert.ok(shots >= 140);
});

test('native mounted replay matches duplicate entries, owners, barrels, gaps and lease policies', async () => {
  const native = createWeaponPredictionSim(await loadSim());
  for (const scenario of mountedPredictionCases()) {
    let a, b;
    try {
      a = runMountedPredictionCase(scenario, { native, afterPredict(prediction) {
        const eligible = !scenario.released && !scenario.dead && scenario.alive !== false && (!scenario.flying || scenario.flightState === 'Flying');
        assert.equal(!!prediction.nativeSession?.handle, eligible, `${scenario.name} delegation`);
      } });
      b = runMountedPredictionCase(scenario);
      for (const key of ['before', 'ticks', 'observations', 'restored']) assert.deepEqual(a[key], b[key], `${scenario.name} ${key}`);
      for (const weapon of a.weapons) { assert.equal(typeof weapon.random, 'function'); assert.equal(typeof weapon.admitShot, 'function'); }
    } finally { a?.prediction.dispose(); b?.prediction.restore(); b?.prediction.dispose?.(); }
  }
});

test('prediction callbacks own their outputs across nested bridge calls and memory growth', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), wire = createWireBridge(bridge), traces = [];
  let grew = false;
  const reenter = () => { wire.encodeWordDelta(new Uint8Array([1, 9, 7, 3])); if (!grew) { grew = true; instance.exports.memory.grow(1); } };
  const native = createWeaponPredictionBridge(bridge, { trace(event) { traces.push(structuredClone(event)); reenter(); } });
  const scenario = weaponPredictionCases()[0], checkpoints = [], mountedCalls = [];
  let a, b, c, d;
  try {
    a = runWeaponPredictionCase(scenario, { native, onShot({ prediction }) { reenter(); checkpoints.push(prediction.checkpoint()); } });
    b = runWeaponPredictionCase(scenario);
    assert.deepEqual(a.observations, b.observations);
    c = runMountedPredictionCase(mountedPredictionCases()[2], { native, ownerRead(i, value) { mountedCalls.push(['read', i, value]); reenter(); },
      ownerWrite(i, value) { mountedCalls.push(['barrel', i, value]); reenter(); }, towCall(i) { mountedCalls.push(['tow', i]); reenter(); } });
    const expectedCalls = [];
    d = runMountedPredictionCase(mountedPredictionCases()[2], { ownerRead: (i, value) => expectedCalls.push(['read', i, value]),
      ownerWrite: (i, value) => expectedCalls.push(['barrel', i, value]), towCall: i => expectedCalls.push(['tow', i]) });
    assert.deepEqual(c.observations, d.observations); assert.deepEqual(c.restored, d.restored); assert.deepEqual(mountedCalls, expectedCalls);
    assert.ok(checkpoints.length > 10); assert.ok(traces.length > 200); assert.ok(grew);
    for (const trace of traces) { assert.ok(Array.isArray(trace.input)); assert.ok(Array.isArray(trace.output)); }
  } finally { for (const r of [a, b, c, d]) { r?.prediction.restore(); r?.prediction.dispose?.(); } }
});

test('callback exceptions preserve the same restore and presentation sequence', async () => {
  const native = createWeaponPredictionSim(await loadSim());
  const make = backend => {
    const unit = createWeaponUnit([weaponDefinition('shot', 0, { ShotsPerSalvo: 3 })]), prediction = new WeaponPrediction({ native: backend });
    const authority = unit.combat.arsenal, before = arsenalObservation(authority, 0);
    prediction.begin(unit, { tick: 0 });
    let calls = 0;
    assert.throws(() => prediction.step({ actions: ['firePrimary'], down: ['Mouse0'] }, 1, () => { calls++; throw Error('shot observer failed'); }, [71, null]), /shot observer failed/);
    const after = arsenalObservation(prediction.arsenal, unit.combat.zoomStep), through = prediction.presentedThrough;
    prediction.restore(); assert.equal(unit.combat.arsenal, authority); assert.deepEqual(arsenalObservation(authority, 0), before);
    prediction.dispose?.(); return { after, through, calls };
  };
  assert.deepEqual(make(native), make(null));

  const mounted = backend => {
    const weapon = new Weapon(weaponDefinition('mounted')), originalRandom = weapon.random, originalAdmission = weapon.admitShot;
    const before = Array.from(captureWeaponState(weapon)); let barrel = 0, throwOnce = true;
    const entry = { state: weapon, muzzles: [{}], get barrel() { return barrel; }, set barrel(v) { barrel = v; if (throwOnce) { throwOnce = false; throw Error('barrel observer failed'); } } };
    const unit = { alive: true, generation: 3, combat: { dead: false }, seat: { index: 0, vehicle: { seats: [{ weapons: [entry] }] } } };
    const prediction = new MountedWeaponPrediction({ native: backend });
    assert.throws(() => prediction.predict(unit, [{ sequence: 1, tick: 1, generation: 3, actions: ['firePrimary'], down: [], look: [] }], 0, [], {}), /barrel observer failed/);
    assert.deepEqual(Array.from(captureWeaponState(weapon)), before); assert.equal(weapon.random, originalRandom); assert.equal(weapon.admitShot, originalAdmission);
    const out = { barrel, tick: prediction.tick, weapon: weaponObservation(weapon) }; prediction.dispose?.(); return out;
  };
  assert.deepEqual(mounted(native), mounted(null));
});

test('disposing prediction owners releases native sessions and restores authority after memory growth', async () => {
  const instance = await loadSim(), native = createWeaponPredictionSim(instance), unit = createWeaponUnit([weaponDefinition('lifecycle')]);
  const authority = unit.combat.arsenal, baseline = arsenalObservation(authority, 0), handles = [];
  for (let i = 0; i < 40; i++) {
    const prediction = new WeaponPrediction({ native }); prediction.begin(unit, { epoch: i + 1, tick: 0 });
    const handle = prediction.nativeSession; handles.push(handle); assert.ok(handle.handle);
    prediction.step({ actions: ['firePrimary'], down: ['Mouse0'] }, 1, () => {}, [i + 1, null]);
    if (i % 7 === 0) instance.exports.memory.grow(1);
    prediction.dispose(); prediction.dispose(); assert.equal(handle.handle, 0);
    assert.equal(unit.combat.arsenal, authority); assert.deepEqual(arsenalObservation(authority, 0), baseline);
  }
  const prediction = new WeaponPrediction({ native }); prediction.begin(unit, { tick: 0 }); const handle = prediction.nativeSession;
  native.dispose(); native.dispose(); assert.equal(handle.handle, 0);
  assert.throws(() => prediction.step({ actions: [], down: [] }, 1), /disposed/);
  prediction.dispose(); assert.equal(unit.combat.arsenal, authority);
  assert.deepEqual(arsenalObservation(authority, 0), baseline); assert.ok(handles.every(h => h.handle === 0));
});
