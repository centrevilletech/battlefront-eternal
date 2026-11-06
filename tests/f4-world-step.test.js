import test from 'node:test';
import assert from 'node:assert/strict';
import {stepWorld} from '../src/f4-world-step.js';
import {f3World} from './fixtures/f3-world.js';

test('outer world order keeps the two object passes around ordnance and items',()=>{
  const calls=[],host={water:true,dead:false,playerCommand:true,ai:true,live:true,eyeView:true};
  stepWorld(host,(op,value,flag)=>{calls.push([op,value,flag]);return op==='takeSquad'&&value===2;},60);
  assert.deepEqual(calls.map(([name])=>name),['start','water','clockAxes','takeSquad','takeSquad','takeSquad','squad','takeSquad',
    'playerCount','eye','zoom','camera','cameraMatrix','ai','nativeUnits','live','ordnance','mineTargets','items','battlefield','battle','target','spawnFlush']);
  assert.deepEqual(calls.find(([name])=>name==='clockAxes'),['clockAxes',1,undefined]);
  assert.deepEqual(calls.find(([name])=>name==='camera'),['camera',0,true]);
});
test('guard reads follow callback changes and a dormant server does not chase its body',()=>{
  const calls=[],host={dead:false,playerCommand:true,server:true,removed:true,grenadeUpdate:true};let grenades=2;
  stepWorld(host,(op)=>{
    calls.push(op);if(op==='start')host.dead=true;if(op==='takeSquad')return true;
    if(op==='nextGrenade')return grenades-->0;
  },1);
  assert.equal(calls.includes('squad'),false);assert.equal(calls.includes('camera'),false);
  assert.equal(calls.filter(x=>x==='grenade').length,2);assert.equal(calls.at(-1),'spawnFlush');
});
test('the native pre-AI count publishes the viewpoint without observing its shapes',async()=>{
  const world=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:100,weapons:[]}},{passiveClasses:['soldier']});
  const unit=world.passive[0];world.phase.registerViewpoint(unit);
  const point=unit.position,tick=unit.tickCount,stamp=unit.spheresTick;
  world.phase.x.f4_viewpoint_clock();world.phase.sync();
  assert.equal(unit.tickCount,tick+1);assert.equal(unit.spheresTick,stamp);assert.equal(unit.position,point);
});
