import test from 'node:test';
import assert from 'node:assert/strict';
import {Soldier} from '../src/soldier.js';

test('raw infantry displacement is independent of batching and previous tick length',()=>{
 const states=[];
 for(const hz of [30,60,144,240])for(const pieces of [1,9]) {
   const soldier=new Soldier({},{});soldier.tickLength=1/hz;
   for(let i=0;i<pieces;i++)soldier.lookRaw(900/pieces,0);
   states.push(soldier.aimYaw);
 }
 for(const yaw of states)assert.ok(Math.abs(yaw-states[0])<1e-12);
});

test('world command controls use the same stock axes and held triggers as the page', async () => {
  const { CommandControls } = await import('../src/runtime/commands.js');
  const controls = new CommandControls();
  controls.down.add('ArrowRight');
  assert.equal(controls.axis('right'), 1);
  assert.equal(controls.axis('turn'), 0);
  controls.down.add('ArrowLeft');
  assert.equal(controls.axis('right'), 0);
  controls.down.add('AltLeft');
  assert.equal(controls.held('fireSecondary'), true);
  controls.clear();
  assert.equal(controls.held('fireSecondary'), false);
});
