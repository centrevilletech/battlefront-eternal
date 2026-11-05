import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { LOCKSTEP_START_SRC } from '../tools/renderer-corpus/battle.mjs';

test('capture starts at the same worker tick despite a raced initial tick', async () => {
 const align=vm.runInNewContext(`(${LOCKSTEP_START_SRC})`);
 for(const first of [0,1,4]){
  let tick=first,frames=0;const b={worldClock:{tick},client:{host:{
   async call(method){if(method==='advance')tick++;return {tick};},drainEntities(){b.worldClock.tick=tick;}
  }},async lockstep(dt,draw){assert.equal(dt,0);assert.equal(draw,false);frames++;}};
  assert.equal(await align(b),60);assert.equal(b.worldClock.tick,60);assert.equal(frames,1);
 }
});
