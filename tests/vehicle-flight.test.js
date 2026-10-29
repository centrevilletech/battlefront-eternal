import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { fixture } from './flight-fixture.js';
import { FixedClock } from '../src/runtime/fixed-clock.js';
import { VehicleCamera } from '../src/vehicles.js';
const run=(f,n,input={})=>{for(let i=0;i<n;i++)f.system.update(1/60,f.v,input);};
const airborne=()=>{const f=fixture();f.system.flightEnvironment.bounds=null;run(f,1,{takeoff:true});run(f,240);return f;};
// The X-wing's lift (TakeoffHeight 2.5, TakeoffSpeed 3, TakeoffTime 1): up
// at an eighth of TakeoffSpeed on the pad, all of it 2.5 m up; over to flight
// within TakeoffTime from there, the climb fading as the nose speed grows;
// then on to cruise at Acceleraton (30 m/s^2).
test('a takeoff lifts TakeoffHeight, goes over to flight in TakeoffTime, then speeds up to cruise; no W autolaunch',()=>{
 const f=fixture();run(f,60,{forward:1});assert.equal(f.v.flightState,'Landed');
 // Its origin rests 0.5 m over the floor: 2 m to go to TakeoffHeight.
 const start=f.v.position.y;let flying=null,over=null;assert.equal(start,.5);run(f,1,{takeoff:true});assert.equal(f.v.flightState,'TakingOff');
 for(let t=1;t<600&&f.v.airspeed<f.v.flightConfig.cruiseSpeed;t++){run(f,1);if(over===null&&f.v.gear<1)over=t;if(flying===null&&f.v.flightState==='Flying')flying={t,speed:f.v.airspeed,rise:f.v.position.y-start};}
 assert.ok(over>=60&&over<=75,`starts over to flight ${over} ticks in, 2.5 m up`);
 assert.ok(flying.t-over>=59&&flying.t-over<=61,`over to flight in TakeoffTime: ${flying.t-over} ticks`);
 assert.ok(flying.speed<6,`it leaves the takeoff slow: ${flying.speed}`);
 assert.ok(Math.abs(flying.rise-3.5)<.1,`2 m up and half TakeoffSpeed for TakeoffTime more: ${flying.rise}`);
 assert.equal(f.v.airspeed,f.v.flightConfig.cruiseSpeed);assert.equal(f.v.health,f.v.maxHealth);
});
test('speed returns to cruise, braking has minimum, boost exhausts power',()=>{const f=airborne(),c=f.v.flightConfig;run(f,180,{forward:1});assert.ok(f.v.airspeed>c.maxSpeed-.1);run(f,180);assert.ok(Math.abs(f.v.airspeed-c.cruiseSpeed)<.1);run(f,240,{forward:-1});assert.ok(f.v.airspeed>=c.minSpeed&&f.v.airspeed<c.minSpeed+.1);run(f,300,{forward:1,boost:true});assert.equal(f.v.boosting,false);assert.ok(f.v.power<20);});
test('local pitch crosses vertical and preserves unit quaternion, camera finite',()=>{const f=airborne();f.v.position.y=600;f.system.flightEnvironment.space=true;const cam=new THREE.PerspectiveCamera(),view=new VehicleCamera(cam,f.c);let min=1;for(let i=0;i<240;i++){run(f,1,{stickY:1,rollLock:true});f.v.present(.5);view.update(f.v,1/60,false);min=Math.min(min,new THREE.Vector3(0,1,0).applyQuaternion(f.v.quaternion).y);assert.ok(cam.quaternion.toArray().every(Number.isFinite));}assert.ok(min<-.99);assert.ok(Math.abs(f.v.quaternion.length()-1)<1e-12);});
test('roll lock, banked pitch, ground versus space leveling',()=>{const f=airborne();f.v.position.y=600;f.system.flightEnvironment.space=true;f.v.quaternion.setFromAxisAngle(new THREE.Vector3(0,0,1),Math.PI/2);const q=f.v.quaternion.clone();run(f,120,{rollLock:true});assert.ok(q.angleTo(f.v.quaternion)<1e-7);run(f,45,{stickY:1,rollLock:true});const forward=new THREE.Vector3(0,0,1).applyQuaternion(f.v.quaternion);assert.ok(Math.abs(forward.x)>.4);f.v.quaternion.copy(q);f.v.tiltRate=0;f.system.flightEnvironment.space=false;f.system.flightEnvironment.ceiling=2000;run(f,180);assert.ok(Math.abs(new THREE.Vector3(1,0,0).applyQuaternion(f.v.quaternion).y)<.02,'the wings level at LevelFilter');assert.ok(Math.abs(f.v.bank)<.02,'no lean without a turn');});
test('barrel, double tap loop, Immelmann spend power and finish',()=>{for(const kind of ['barrel','loop','immelmann']){const f=airborne();f.system.flightEnvironment.space=true;f.v.position.y=600;if(kind==='immelmann')run(f,1,{immelmann:true});else{run(f,1,{trick:true});run(f,1);run(f,1,kind==='loop'?{trick:true}:{right:1});if(kind==='barrel')run(f,4,{right:1});}assert.ok(f.v.power<80);run(f,120,{rollLock:true});assert.equal(f.v.trick,'');if(kind==='immelmann'){const d=new THREE.Vector3(0,0,1).applyQuaternion(f.v.quaternion);assert.ok(d.z<-.99);assert.ok(Math.abs(d.y)<.03);}}});
test('landing at full speed settles flat ground, no slowing down first',()=>{const f=airborne();assert.ok(f.v.airspeed>10);run(f,1,{takeoff:true});assert.equal(f.v.flightState,'Landing');for(let i=0;i<1200&&f.v.flightState==='Landing';i++)run(f,1);assert.equal(f.v.flightState,'Landed');assert.equal(f.v.health,f.v.maxHealth);});
test('60Hz clock identical at 30, 60, 144Hz rendering',()=>{const states=[];for(const hz of [30,60,144]){const f=fixture(),clock=new FixedClock();for(let i=0;i<hz*3;i++)clock.advance(1/hz,(dt,tick)=>f.system.update(dt,f.v,{takeoff:tick===1,stickX:tick>100?.3:0}));states.push([...f.v.position.toArray(),...f.v.quaternion.toArray(),f.v.airspeed]);}for(const s of states)assert.deepEqual(s,states[0]);});
