import test from 'node:test';
import assert from 'node:assert/strict';
import {Soldier} from '../src/soldier.js';
import {CollisionWorld} from '../src/collision.js';
import {captureCompactSoldier, restoreCompactSoldier} from '../src/runtime/network-entity-soldier-compact.js';
const world = () => new CollisionWorld(new Float32Array([-100,0,-100,100,0,-100,100,0,100,-100,0,100]),new Uint32Array([0,2,1,0,3,2]),new Uint32Array([0,1]));
const make = w => { const s=new Soldier(w,{});s.spawn(0,0,0,0);s.diveTime=.4;s.getupTime=.2;return s; };
test('compact human state resumes exact movement across jump, posture and dive boundaries', () => {
 const w=world(), a=make(w); let b;
 for(let tick=1;tick<=420;tick++) {
  const input={forward:tick<300?1:-1,right:tick%90<45?.4:-.4,turn:0,jump:[50,150,260].includes(tick),crouch:tick===120,prone:tick===200};
  a.lookRaw(.3,-.1);a.tick(input,1/60);
  if([20,51,121,151,201,261,330].includes(tick)) {
   b=make(w); for(const value of Object.values(b))if(value?.isVector3)value.set(99,-31,25);
   b.groundAdjustment=37;restoreCompactSoldier(b,captureCompactSoldier(a));assert.equal(b.groundAdjustment,37);assert.deepEqual(b.previous,a.position);
  } else if(b) { b.lookRaw(.3,-.1);b.tick(input,1/60); }
  if(b)assert.deepEqual(captureCompactSoldier(b),captureCompactSoldier(a),`tick ${tick}`);
 }
});
test('settled offset and dive source are conditional and malformed state is atomic', () => {
 const a=make(world()),b=make(world());assert.equal(captureCompactSoldier(a).length,124);
 a.settled.x+=.01;a.dive=.2;a.diveFrom='prone';const payload=captureCompactSoldier(a);assert.equal(payload.length,149);
 restoreCompactSoldier(b,payload);assert.deepEqual(captureCompactSoldier(b),payload);
 const bad=payload.slice();bad[bad.length-1]=9;assert.throws(()=>restoreCompactSoldier(b,bad));assert.deepEqual(captureCompactSoldier(b),payload);
 a.contactCount=1;assert.throws(()=>captureCompactSoldier(a),/completed collision/);
});
test('actual lateral dive preserves original posture through completion',()=>{
 const w=world(),a=make(w);a.posture='crouch';a.stateTime=1;let b,observed=false;
 for(let tick=1;tick<=80;tick++){
  const input={forward:0,right:1,turn:0,jump:tick===1};a.tick(input,1/60);
  if(tick===3){assert.equal(a.dive,1);assert.equal(a.diveFrom,'crouch');observed=true;b=make(w);restoreCompactSoldier(b,captureCompactSoldier(a));}
  else if(b)b.tick(input,1/60);
  if(b)assert.deepEqual(captureCompactSoldier(b),captureCompactSoldier(a),`dive tick ${tick}`);
 }
 assert.ok(observed);assert.equal(a.dive,0);
});
test('conditional settled presence preserves signed zero exactly',()=>{
 const a=make(world()),b=make(world());a.position.x=0;a.settled.x=-0;
 const payload=captureCompactSoldier(a);assert.equal(payload.length,148);restoreCompactSoldier(b,payload);assert.ok(Object.is(b.settled.x,-0));
});
