import test from 'node:test';
import assert from 'node:assert/strict';
import { Arsenal } from '../src/weapons.js';
import { prepareMelee } from '../src/heroes.js';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { PredictionArsenal } from '../src/runtime/prediction-arsenal.js';
import { PredictionFrame } from '../src/runtime/prediction-frame.js';
import { captureWeaponState, decodeWeaponState, applyValidatedWeaponState, WEAPON_STATE_NUMBERS, WEAPON_STATE_FLAGS } from '../src/runtime/network-weapon-state.js';
import { captureCompactSoldier, restoreCompactSoldier, COMPACT_SOLDIER_LENGTHS } from '../src/runtime/network-entity-soldier-compact.js';
import { captureDroidekaState, DROIDEKA_STATE_LENGTHS } from '../src/runtime/network-droideka-state.js';
import { actorCourse, createPredictionActor } from './fixtures/client-actors.js';
import { createWeaponUnit, weaponDefinition } from './fixtures/client-weapons.js';

// The existing wire capture/read functions remain a reference for the presentation
// copy's exact field set. No game files or alternate runtime path are needed.
test('retained arsenal view matches wire snapshots and keeps authority isolated', () => {
  const unit = createWeaponUnit([
    weaponDefinition('charge', 0, { charges: [{ at: .5, ShotDelay: .25 }] }),
    weaponDefinition('blade', 1, { base: 'melee', ordnance: undefined, swings: [{ duration: .2, hit: .05 }, { duration: .3, hit: .1 }] }),
  ]);
  const authority = unit.combat.arsenal, view = new PredictionArsenal();
  const expected = prepareMelee(new Arsenal(authority.weapons.map(w => w.levels[0]), { random: () => .5 }));
  let privateArsenal, selected, snapshot, meleeScratch;
  for (let frame = 0; frame < 30; frame++) {
    for (const w of authority.weapons) {
      for (let n = 0; n < WEAPON_STATE_NUMBERS.length; n++) w[WEAPON_STATE_NUMBERS[n]] = frame * .03125 + n;
      for (let n = 0; n < WEAPON_STATE_FLAGS.length; n++) w[WEAPON_STATE_FLAGS[n]] = Boolean((frame + n) % 2);
      w.reserve = frame % 2 ? Infinity : 12;
      w.yawSpreadOverride = frame % 2 ? null : -0;
      w.def = w.levels[frame % w.levels.length];
    }
    const melee = authority.weapons[1].melee;
    melee.time = frame * .01; melee.index = frame % 2;
    melee.swing = frame % 3 ? { duration: .2 } : null;
    melee.active = false; // captureState derives active from swing.
    authority.selected[0] = frame % 2; authority.selected[1] = 1 - frame % 2;
    unit.combat.zoomStep = frame % 3;
    for (let i = 0; i < authority.weapons.length; i++) {
      applyValidatedWeaponState({ weapon: expected.weapons[i], state: decodeWeaponState(captureWeaponState(authority.weapons[i])) });
      if (authority.weapons[i].melee) expected.weapons[i].melee.restoreState(authority.weapons[i].melee.captureState());
    }
    expected.selected[0] = authority.selected[0]; expected.selected[1] = authority.selected[1];
    view.begin(unit, { epoch: 1, tick: frame });
    assert.equal(unit.combat.arsenal, view.arsenal);
    for (let i = 0; i < expected.weapons.length; i++) {
      assert.deepEqual(captureWeaponState(view.arsenal.weapons[i]), captureWeaponState(expected.weapons[i]));
      assert.deepEqual(view.arsenal.weapons[i].melee?.captureState(), expected.weapons[i].melee?.captureState());
    }
    assert.deepEqual(view.arsenal.selected, expected.selected);
    if (frame === 0) {
      privateArsenal = view.arsenal; selected = view.arsenal.selected;
      snapshot = view.saved; meleeScratch = view.meleeStates[1];
    } else {
      assert.equal(view.arsenal, privateArsenal); assert.equal(view.arsenal.selected, selected);
      assert.equal(view.saved, snapshot); assert.equal(view.meleeStates[1], meleeScratch);
    }
    view.arsenal.weapons[0].clip = -999; unit.combat.zoomStep = -10;
    view.restore(); view.restore();
    assert.equal(unit.combat.arsenal, authority);
    assert.equal(unit.combat.zoomStep, frame % 3);
    assert.notEqual(authority.weapons[0].clip, -999);
  }
  view.dispose();
});

test('private arsenal rebuilds on each identity boundary and preserves non-checkpoint state', () => {
  const unit = createWeaponUnit([weaponDefinition('one')]), view = new PredictionArsenal();
  view.begin(unit, { epoch: 1, tick: 4 });
  let previous = view.arsenal;
  previous.weapons[0].salvoSeed = 123; previous.weapons[0].salvoCount = 7;
  previous.weapons[0].bottomless = true;
  view.presentedThrough = 10;
  view.begin(unit, { epoch: 1, tick: 5 });
  assert.equal(view.arsenal, previous);
  assert.equal(view.presentedThrough, 10);
  assert.equal(view.arsenal.weapons[0].salvoSeed, 123);
  assert.equal(view.arsenal.weapons[0].salvoCount, 7);
  assert.equal(view.arsenal.weapons[0].bottomless, true, 'keep the original checkpoint field set');
  for (const change of ['epoch', 'identity', 'generation', 'arsenal']) {
    view.restore();
    if (change === 'identity') unit.id++;
    if (change === 'generation') unit.generation++;
    if (change === 'arsenal') unit.combat.arsenal = createWeaponUnit([weaponDefinition('replacement')]).combat.arsenal;
    view.begin(unit, { epoch: 2, tick: 11 });
    assert.notEqual(view.arsenal, previous, change);
    previous = view.arsenal;
  }
  view.dispose(); assert.equal(unit.combat.arsenal, view.source);
});

test('page body checkpoint views keep exact wire bytes across every optional tail', async () => {
  const instance = await loadSim(), course = actorCourse(await loadRayKernel(instance)), frame = new PredictionFrame();
  try {
    const soldier = createPredictionActor({ kind: 'soldier' }, course).soldier;
    for (const settled of [false, true]) for (const diving of [false, true]) {
      soldier.settled.copy(soldier.position); if (settled) soldier.settled.x += .125;
      soldier.dive = diving ? 1 : 0; soldier.diveFrom = 'prone';
      const bytes = frame.captureBody(soldier, false);
      assert.ok(COMPACT_SOLDIER_LENGTHS.includes(bytes.length));
      assert.deepEqual(bytes, captureCompactSoldier(soldier));
      assert.equal(frame.captureBody(soldier, false), bytes);
    }
    const saved = frame.captureBody(soldier, false), position = soldier.position.clone();
    soldier.position.x += 10;
    restoreCompactSoldier(soldier, saved);
    assert.deepEqual(soldier.position, position, 'publication cannot mutate the saved checkpoint');
    const droid = createPredictionActor({ kind: 'droideka' }, course).soldier, shield = droid.shield;
    for (const haveShield of [false, true]) for (const haveGun of [false, true]) {
      droid.shield = haveShield ? shield : null;
      droid.lastGun = haveGun ? { index: 0, barrel: 0 } : undefined;
      const bytes = frame.captureBody(droid, true);
      assert.ok(DROIDEKA_STATE_LENGTHS.includes(bytes.length));
      assert.deepEqual(bytes, captureDroidekaState(droid));
      assert.equal(frame.captureBody(droid, true), bytes);
    }
  } finally { course.dispose(); }
});

test('page fact rows preserve roster order and overwrite recycled fire outcomes', () => {
  const frame = new PredictionFrame(), crowd = frame.crowd, edges = frame.fireEdges;
  const own = { soldier: {}, alive: true }, living = { soldier: {}, alive: true }, dead = { soldier: {}, dead: true };
  frame.captureCrowd([own, { soldier: {}, alive: true, seat: {} }, dead, living,
    { soldier: {}, alive: true, held: {} }, { soldier: {}, alive: true, removed: true }], own);
  assert.deepEqual(frame.crowd, [dead, living]);
  const outcomes = new Map([[1, { sequence: 4, edge: { kind: 'fire', button: 'firePrimary', pressed: true, id: 7 } }]]);
  frame.captureEdges(outcomes); const row = edges[0];
  assert.deepEqual(row, { sequence: 4, kind: 'fire', button: 'firePrimary', pressed: true, id: 7 });
  outcomes.clear(); outcomes.set(2, { sequence: 5, edge: { kind: 'action', id: 8 } });
  frame.captureEdges(outcomes);
  assert.equal(edges[0], row); assert.equal(row.pressed, undefined); assert.equal(row.button, undefined);
  assert.equal(row.kind, 'action'); assert.equal(row.sequence, 5);
  frame.clear();
  assert.equal(frame.crowd, crowd); assert.equal(frame.fireEdges, edges);
  assert.equal(crowd.length, 0); assert.equal(edges.length, 0);
  assert.equal(frame.options.fireEdges, edges); assert.equal(frame.options.crowd, crowd);
});
