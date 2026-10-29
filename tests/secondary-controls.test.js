import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { Controls } from '../src/input.js';
import { ActionQueue } from '../src/runtime/action-queue.js';
import { CommandControls } from '../src/runtime/commands.js';
import { PlayerCombat } from '../src/combat.js';
import { Arsenal } from '../src/weapons.js';

function fixture(loadout) {
  const page = Object.assign(Object.create(Controls.prototype), {
    queued: new ActionQueue(), down: new Set(), looks: [], human: [], sequence: 0,
  });
  const world = new CommandControls(), shots = [];
  const combat = Object.assign(Object.create(PlayerCombat.prototype), {
    arsenal: new Arsenal(loadout), soldier: { posture: 'stand', air: 'jet', dive: 0, lock: 0 },
    animator: { setBank() {}, weaponAction() {}, endWeaponAction() {}, fire() {} },
    held: new Map(), seen: new Map(),
    aim() {}, updateGlow() {}, updateKick() {}, updateFlashes() {}, fire(shot) { shots.push(shot); },
  });
  function step(code) {
    if (code) page.press(code);
    assert.ok(world.accept(page.command(page.sequence + 1, 1, 1)));
    combat.update(1 / 60, world, 0);
  }
  return { combat, shots, step };
}

test('G cycles secondary items through world commands without switching the primary', () => {
  const { combat, step } = fixture([
    { class: 'rifle', channel: 0 },
    { class: 'grenade', base: 'grenade', channel: 1 },
    { class: 'mine', base: 'dispenser', channel: 1 },
  ]);
  const primary = combat.arsenal.current(0);
  step('KeyG');
  assert.equal(combat.arsenal.current(1).def.class, 'mine');
  step();
  assert.equal(combat.arsenal.current(1).def.class, 'mine', 'one press advances once');
  step('KeyG');
  assert.equal(combat.arsenal.current(1).def.class, 'grenade', 'wraps to the first secondary');
  assert.equal(combat.arsenal.current(0), primary);
});

const unitsPath = new URL('../public/sides/rep/units.json', import.meta.url);
test('G cycles and fires both secondary items in the retail clone trooper loadout', { skip: !existsSync(unitsPath) }, () => {
  const loadout = JSON.parse(readFileSync(unitsPath)).units.rep_inf_clone_trooper.weapons;
  const { combat, shots, step } = fixture(loadout);
  const [first, second] = combat.arsenal.channels[1];
  assert.ok(first && second);
  for (const expected of [second, first]) {
    step('KeyG');
    assert.equal(combat.arsenal.current(1), expected);
    step('Mouse2');
    for (let i = 0; i < 180; i++) step();
    assert.equal(shots.at(-1).weapon, expected);
  }
  assert.equal(shots.length, 2);
});

test('jet trooper throws its sole secondary with right-click or Alt, including in flight', { skip: !existsSync(unitsPath) }, () => {
  const loadout = JSON.parse(readFileSync(unitsPath)).units.rep_inf_jet_trooper.weapons;
  for (const code of ['Mouse2', 'AltLeft', 'AltRight']) {
    const { combat, shots, step } = fixture(loadout);
    const grenade = combat.arsenal.current(1);
    assert.equal(combat.arsenal.channels[1].length, 1);
    assert.equal(grenade.def.base, 'grenade');
    step('KeyG');
    assert.equal(combat.arsenal.current(1), grenade);
    step(code);
    assert.equal(combat.shown, grenade, 'throw displays the grenade');
    for (let i = 0; i < 120; i++) step();
    assert.equal(shots.length, 1, 'a short click throws once');
    assert.equal(shots[0].weapon, grenade);
    assert.equal(combat.shown, combat.arsenal.current(0), 'returns to the primary after throwing');
  }
});
