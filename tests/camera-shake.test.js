import test from 'node:test';
import assert from 'node:assert/strict';
import {CameraFrame} from './helpers/cameras.js';
import {CameraOutput as O} from '../src/presentation/layout.js';
const STRIKE={Shake:2,ShakeLength:1,ShakeRadiusInner:10,ShakeRadiusOuter:30};
function rig(){const h=new CameraFrame();h.owner.shake.random=()=>1;return h;}
const q=(o,at)=>Array.from(o.subarray(at,at+4));
test('the camera phase measures explosion falloff at the followed origin less half its sphere',()=>{
  for(const [distance,radius,total] of [[10.5,1,4],[20.73,1.46,2],[30.5,0,0],[.00005,0,0]]){
    const h=rig();h.state.position.set(0,0,distance);h.owner.shake.explode(STRIKE,[0,0,0],{position:h.state.position,radius});h.frame(.01);h.finish();assert.equal(h.owner.output[O.SHAKE_TOTAL],total);
  }
});
test('explosion shake has two admissions, four slots, and sums before decaying',()=>{
  const h=rig(),s=h.owner.shake,target={position:h.state.position.set(0,0,20.73),radius:1.46};
  s.explode(STRIKE,[0,0,0],target);h.frame(.25);assert.equal(s.total,2);h.finish();
  h.frame(.25);assert.equal(s.total,1);h.finish();assert.equal(s.active,false);
  s.explode(STRIKE,[0,0,0],target);s.explode(STRIKE,[0,0,0],target);s.explode(STRIKE,[0,0,0],target);
  h.frame(.01);assert.equal(s.total,4);assert.equal(h.owner.output[O.REFUSED],2);
  // Clear is ordered after all pending births, even after a stalled queue.
  for(let i=0;i<2000;i++)s.add(1,1);s.clear();h.frame(.01);assert.equal(s.active,false);h.finish();
});
test('paused and death views consume no random turns while live views turn on all three axes',()=>{
  const h=rig();let draws=0;h.owner.shake.random=()=>{draws++;return 1;};h.owner.shake.add(2,1);
  h.frame(0);assert.deepEqual(q(h.owner.output,O.QUATERNION),q(h.owner.output,O.PRE_QUATERNION));assert.equal(draws,0);h.finish();
  h.frame(1/60);assert.notDeepEqual(q(h.owner.output,O.QUATERNION),q(h.owner.output,O.PRE_QUATERNION));assert.equal(draws,3);h.finish();
  h.death.active=true;h.frame(1/60);assert.deepEqual(q(h.owner.output,O.QUATERNION),q(h.owner.output,O.PRE_QUATERNION));assert.equal(draws,3);h.finish();
});
