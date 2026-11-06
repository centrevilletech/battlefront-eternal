import test from 'node:test';
import assert from 'node:assert/strict';
import { f3World } from './fixtures/f3-world.js';
import { weaponDefinition } from './fixtures/client-weapons.js';
import { BlockHeaderLayout as B } from '../src/sim/gen/boundary.js';
import { F3PublicationMirror, syncF3Publication, sameF3UnitMirror, sameMoveInput } from '../src/sim/gen/units.js';
import { sameF3CombatView } from '../src/sim/gen/combat.js';

// Exercise the complete native phase and its masked reader. Compare with the
// encoded native row after movement, weapon clocks, scalar commands and reuse;
// the reference reader observes bytes rather than running another simulation.
test('sparse unit publications preserve the complete native body and combat row', async () => {
  const world = await f3World({ unit: {
    class: 'unit', base: 'soldier', MaxHealth: 300, weapons: [
      weaponDefinition('rifle', 0, { RoundsPerClip: 12, ammo: 5, ShotDelay: .1, ReloadTime: .2,
        ordnance: { class: 'rifle-bolt', base: 'bolt', Velocity: 60, LifeSpan: .1, Damage: 0 } }),
      weaponDefinition('pistol', 0, { RoundsPerClip: 4, ammo: 3, ShotDelay: .2,
        ordnance: { class: 'pistol-bolt', base: 'bolt', Velocity: 60, LifeSpan: .1, Damage: 0 } }),
    ],
  } });
  const unit = world.spawn('unit', 0, 0), binding = world.phase.bindings[unit.id];
  const reference = new F3PublicationMirror();
  const previous=unit.soldier.previous,settled=unit.soldier.settled,avoidance=unit.soldier.avoidance;
  for (const weapon of unit.combat.arsenal.weapons) weapon.random = world.ordnance.random;
  const check = () => {
    const words = binding.words;
    assert.equal(words, world.phase.bridge.f, "publication decodes retained native storage directly");
    syncF3Publication(world.phase.bridge, words, binding.wordsAt + B.words, binding.wordsAt + words[binding.wordsAt + B.heap], reference);
    assert.equal(unit.soldier.previous,previous);assert.equal(unit.soldier.settled,settled);assert.equal(unit.soldier.avoidance,avoidance);
    assert.equal(sameF3UnitMirror(binding.view, reference.body), null);
    assert.equal(sameMoveInput(unit.input, reference.controls.input), null);
    // A diagnostic observes every camera vector as well as every weapon.
    void unit.combat.origin; void unit.combat.target; void unit.combat.aimDir;
    void unit.combat.shotDir; void unit.combat.left; void unit.combat.up; void unit.combat.pivot;
    // A diagnostic observes every weapon; gameplay getters usually need one.
    for (const weapon of unit.combat.arsenal.weapons) void weapon.w;
    assert.equal(sameF3CombatView(unit.combat.state, reference.combat), null);
  };
  // The aimer borrows only its declared movement facts. Ordinary correction
  // fields publish at completion rather than requiring a second full decode.
  let moved = null;
  unit.onMoved = () => { moved = {
    position: unit.position.toArray(), velocity: unit.velocity.toArray(),
    yaw: unit.soldier.aimYaw, pitch: unit.soldier.aimPitch,
    posture: unit.soldier.posture, tick: unit.tickCount,
  }; };
  check();
  for (let tick = 0; tick < 90; tick++) {
    unit.intent.forward = tick < 30 ? 1 : 0;
    unit.intent.crouch = tick === 35;
    unit.intent.buttons[0] = tick < 50 ? 1 : 0;
    if (tick === 55) unit.combat.arsenal.select(1);
    if (tick === 70) unit.combat.reset();
    world.phase.phase(1 / 60); check();
    assert.deepEqual(unit.position.toArray(), moved.position);
    assert.deepEqual(unit.velocity.toArray(), moved.velocity);
    assert.equal(unit.soldier.aimYaw, moved.yaw);
    assert.equal(unit.soldier.aimPitch, moved.pitch);
    assert.equal(unit.soldier.posture, moved.posture);
    assert.equal(unit.tickCount, moved.tick);
  }
  unit.combat.damage(1000); check();
  unit.spawn(3, 0, 2, .4); check();
  world.phase.phase(1 / 60); check();
});

// Unrelated weapon clocks have no foreign reader between two aimers. The
// outer phase must still publish every completed body's final correction.
test('ordinary unit ticks publish together while aimers borrow movement facts', async () => {
  const world = await f3World({ soldier: {
    class: 'soldier', base: 'soldier', MaxHealth: 300, weapons: [],
  } });
  const units = [world.spawn('soldier', 0, 0), world.spawn('soldier', 12, 0), world.spawn('soldier', 24, 0)];
  let aims = 0;
  const ticks = units.map(unit => unit.tickCount + 1);
  for (const [index, unit] of units.entries()) unit.onMoved = () => {
    aims++;
    assert.equal(unit.tickCount, ticks[index]);
    assert.equal(world.phase.counters.unitRows, 0, 'ordinary clocks wait for the outer reader');
  };
  world.phase.counters.reset();
  world.phase.phase(1 / 60);
  assert.equal(aims, 3);
  assert.equal(world.phase.counters.unitRows, 3);
  for (const [index, unit] of units.entries()) assert.equal(unit.tickCount, ticks[index]);
});

// Separation can undo an aimer's intermediate move exactly. A zero final
// position delta must still restore the borrowed public vector in place.
test('separation restores borrowed aimer positions against an unchanged baseline', async () => {
  const world = await f3World({ soldier: {
    class: 'soldier', base: 'soldier', MaxHealth: 300, weapons: [],
  } });
  const first = world.spawn('soldier', 0, 0), second = world.spawn('soldier', 0, 1.14);
  first.intent.forward = 1; second.intent.forward = -1;
  const reference = new F3PublicationMirror();
  for (let tick = 0; tick < 30; tick++) {
    world.phase.phase(1 / 60);
    for (const unit of [first, second]) {
      const binding = world.phase.bindings[unit.id], words = binding.words;
      syncF3Publication(world.phase.bridge, words, binding.wordsAt + B.words, binding.wordsAt + words[binding.wordsAt + B.heap], reference);
      assert.deepEqual(unit.position.toArray(), reference.body.position.toArray());
      assert.equal(sameF3UnitMirror(binding.view, reference.body), null);
    }
  }
});

// A weapon can remain unobserved while several native ticks change it. Its
// next ordinary getter must read the latest complete cache without an export.
test('unobserved weapon mirrors refresh on demand after multiple native ticks', async () => {
  const world = await f3World({ soldier: {
    class: 'soldier', base: 'soldier', MaxHealth: 300,
    weapons: [weaponDefinition('rifle', 0), weaponDefinition('spare', 0)],
  } });
  const unit = world.spawn('soldier', 0, 0), spare = unit.combat.arsenal.weapons[1];
  spare.heat = .8;
  const row = spare.w;
  for (let tick = 0; tick < 12; tick++) world.phase.phase(1 / 60);
  assert.equal(row.heat, .8, 'an unobserved row retains its previous decoded value');
  const before = world.phase.counters.wasmCalls, version = spare.mirrorVersion;
  assert.equal(spare.def, spare.levels[0]);
  assert.equal(spare.mirrorVersion, version, 'definition reads do not decode the complete weapon');
  assert.ok(Math.abs(spare.heat - .776) < 1e-14);
  assert.equal(row.heat, .8, 'scalar observations leave unrelated mirror stores untouched');
  assert.equal(spare.w, row, 'the mirror retains its object identity');
  assert.equal(world.phase.counters.wasmCalls, before, 'a getter reads shared memory only');
});

// Camera vectors keep their identity while unobserved native movement and
// aim updates accumulate. Reading them never enters the module again.
test('combat basis vectors observe retained native rows without an export', async () => {
  const world = await f3World({ soldier: {
    class: 'soldier', base: 'soldier', MaxHealth: 300, weapons: [],
  } });
  const unit = world.spawn('soldier', 0, 0), combat = unit.combat;
  const origin = combat.origin, binding = world.phase.bindings[unit.id];
  unit.intent.forward = .5;
  for (let tick = 0; tick < 12; tick++) world.phase.phase(1 / 60);
  const reference = new F3PublicationMirror(), words = binding.words;
  syncF3Publication(world.phase.bridge, words, binding.wordsAt + B.words,
    binding.wordsAt + words[binding.wordsAt + B.heap], reference);
  const before = world.phase.counters.wasmCalls;
  assert.equal(combat.origin, origin);
  assert.deepEqual(origin.toArray(), reference.combat.origin.toArray());
  assert.deepEqual(combat.aimDir.toArray(), reference.combat.aimDir.toArray());
  assert.equal(world.phase.counters.wasmCalls, before);
});
