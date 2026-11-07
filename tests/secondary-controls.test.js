import test from 'node:test';
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
