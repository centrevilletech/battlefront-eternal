import { contentURL } from './helpers/content.js';
import test from 'node:test';
import { existsSync, readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { Controls } from '../src/input.js';
import { ActionQueue } from '../src/runtime/action-queue.js';
import { CommandControls } from '../src/runtime/commands.js';
import { f3World } from './fixtures/f3-world.js';

// The same queued KeyG command reaches native IntentControls. Repeated
// ticks retain the selected item until another discrete edge arrives.
test('G cycles native secondary items without switching the primary', async() => {
  const page=Object.assign(Object.create(Controls.prototype),{queued:new ActionQueue(),down:new Set(),looks:[],human:[],sequence:0});
  const controls=new CommandControls();
  const world=await f3World({body:{class:'body',base:'soldier',weapons:[
    {class:'rifle',base:'cannon',channel:0,RoundsPerClip:10},
    {class:'grenade',base:'grenade',channel:1,RoundsPerClip:1},
    {class:'mine',base:'dispenser',channel:1,RoundsPerClip:1},
  ]}});
  const unit=world.spawn('body',0,0),primary=unit.combat.arsenal.current(0);
  function step(code){
    if(code)page.press(code);
    assert.ok(controls.accept(page.command(page.sequence+1,1,1)));
    unit.intent.nextSecondary=controls.take('nextSecondary');
    world.phase.phase(1/60);
  }
  step('KeyG');assert.equal(unit.combat.arsenal.current(1).name,'mine');
  step();assert.equal(unit.combat.arsenal.current(1).name,'mine');
  step('KeyG');assert.equal(unit.combat.arsenal.current(1).name,'grenade');
  assert.equal(unit.combat.arsenal.current(0),primary);
});

// Content loadouts exercise the same phase as the synthetic fixture. The
// host translates queued input; all bank, throw and return timing is native.
async function fixture(def) {
  const world = await f3World({ body: { ...def, class: 'body' } });
  const unit = world.spawn('body', 0, 0), shots = [];
  // Real bodies receive named gameplay streams at attachment. Grenade
  // spread must use that native binding, never the removed legacy draw.
  unit.combat.random = world.ordnance.random;
  for (const weapon of unit.combat.arsenal.weapons) weapon.random = world.ordnance.random;
  const page = Object.assign(Object.create(Controls.prototype), {
    queued: new ActionQueue(), down: new Set(), looks: [], human: [], sequence: 0,
  });
  const controls = new CommandControls();
  unit.combat.onFired = shot => shots.push(shot.weapon);
  return { combat: unit.combat, shots, step(code) {
    if (code) page.press(code);
    assert.ok(controls.accept(page.command(page.sequence + 1, 1, 1)));
    unit.intent.nextSecondary = controls.take('nextSecondary');
    unit.intent.buttons[1] = controls.take('fireSecondary') ? 1 : 0;
    world.phase.phase(1 / 60);
    unit.combat.updateHeld();
  } };
}
const unitsPath = contentURL('sides/rep/units.json');
test('G cycles and fires both secondary items in the native clone trooper loadout', { skip: !existsSync(unitsPath) }, async () => {
  const def = JSON.parse(readFileSync(unitsPath)).units.rep_inf_clone_trooper;
  const { combat, shots, step } = await fixture(def);
  const [first, second] = combat.arsenal.channels[1];
  assert.ok(first && second);
  for (const expected of [second, first]) {
    step('KeyG');
    assert.ok(combat.arsenal.current(1) === expected, 'the secondary identity stays stable');
    step('Mouse2');
    for (let i = 0; i < 180; i++) step();
    assert.ok(shots.at(-1) === expected, 'the selected secondary fires');
  }
  assert.equal(shots.length, 2);
});
test('native jet trooper throws its sole secondary with right-click or Alt', { skip: !existsSync(unitsPath) }, async () => {
  const def = JSON.parse(readFileSync(unitsPath)).units.rep_inf_jet_trooper;
  for (const code of ['Mouse2', 'AltLeft', 'AltRight']) {
    const { combat, shots, step } = await fixture(def);
    const grenade = combat.arsenal.current(1);
    assert.equal(combat.arsenal.channels[1].length, 1);
    assert.equal(grenade.def.base, 'grenade');
    step('KeyG');
    assert.ok(combat.arsenal.current(1) === grenade, 'the sole secondary stays selected');
    step(code);
    assert.ok(combat.shown === grenade, 'the throw displays the grenade');
    for (let i = 0; i < 120; i++) step();
    assert.equal(shots.length, 1);
    assert.ok(shots[0] === grenade, 'the thrown weapon keeps its identity');
    assert.ok(combat.shown === combat.arsenal.current(0), 'the primary returns after the throw');
  }
});
