// A crowded cascade must retain its bounded prefix and report every dropped
// submission without throwing or hiding the static admission counter.
import test from 'node:test';
import assert from 'node:assert/strict';
import { shadowPass } from '../src/rust-render/shadow.js';
import { Graphics } from '../src/graphics.js';

test('over-capacity cascades keep drawing and expose lifetime clipping in stats',()=>{
  const core={cameras:{shadow(){}},texture:()=>0,addView(){},views(){},rust:{static_depth_clipped:()=>7,actor_depth_frame(){},camera_clipped:()=>true,camera_pack_rejected:()=>3}};
  const pass=shadowPass(core),capacity=pass.capacity;
  core.casterInput=new Float64Array(pass.scratch.length);
  pass.request={light:{shadow:{camera:{},getFrustum:()=>null,map:{depthTexture:{}},mapSize:{x:1024}}}};
  let count=capacity+9000;
  pass.walk=()=>{for(let i=0;i<count;i++)pass.draws.push({native:i});};
  pass.draw();
  assert.equal(pass.done,true);assert.equal(core.casterInput[3],capacity);assert.equal(pass.clipped,9000);
  count=capacity+3;pass.draw();assert.equal(pass.clipped,9003);
  count=1;pass.draw();assert.equal(core.casterInput[3],1);assert.equal(pass.clipped,9003);
  const graphics={renderSize:{x:640,y:360},rust:{byName:{shadow:pass,effects:{sniperDropped:()=>11}},rust:core.rust}};
  const text=Graphics.prototype.statsText.call(graphics);
  assert.match(text,/SHADOW CLIPPED 9010/);assert.match(text,/CAMERA CLIPPED/);assert.match(text,/LIGHT PACK REJECTED 3/);assert.match(text,/SNIPER CLIPPED 11/);
});
