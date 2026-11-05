import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { fixture, FIXTURE_SPEEDS, FIXTURE_HANDLING } from './flight-fixture.js';
test('a landing starts wherever it is asked for: a post under a wing lets it down, one under its fuselage holds it until he lifts off',async()=>{
 const {CollisionWorld}=await import('../tools/collision-fixture.mjs');const {stepPrediction}=await import('./helpers/hull-prediction.js');const stepFlight=(v,dt,input)=>stepPrediction(v.system,dt,v,input);
 const solid=(...boxes)=>{const pos=[],idx=[];for(const [x,y,z,w,h,d] of boxes){const g=new THREE.BoxGeometry(w,h,d).translate(x,y,z),base=pos.length/3;pos.push(...g.attributes.position.array);idx.push(...[...g.index.array].map(i=>i+base));}return new CollisionWorld(new Float32Array(pos),new Uint32Array(idx),new Uint32Array(idx.length/3));};
 const floor=[0,-1,0,20000,2,20000],wing=[.9,3,0,.4,6,.4],nose=[0,3,1.9,.4,6,.4];// under the hull's right edge; under its nose
 // A craft that hovers (MidSpeed 0, as the LAAT/i and the MAF do), so it
 // comes straight down over the posts.
 const hovering=world=>fixture({...FIXTURE_SPEEDS,MinSpeed:'-15.0',MidSpeed:'0.0',...FIXTURE_HANDLING},'ship',{},undefined,world);
 const hover=f=>{f.v.flightState='Flying';f.v.gear=0;f.v.position.set(0,10,0);f.v.quaternion.identity();f.v.airspeed=0;f.v.velocity.set(0,0,0);f.v.syncBody();};
 const land=(f,n)=>{for(let i=0;i<n&&f.v.flightState==='Landing';i++)stepFlight(f.v,1/60,{});};
 // Under a wing: the 2004 game's craft meet the ground with a sphere about
 // the fuselage; the post goes into the wing.
 const f=hovering(solid(floor,wing));stepFlight(f.v,1/60,{});hover(f);
 stepFlight(f.v,1/60,{takeoff:true});assert.equal(f.v.flightState,'Landing');
 land(f,1200);assert.equal(f.v.flightState,'Landed');assert.ok(Math.abs(f.v.position.y-(-f.v.bounds.min.y+.15))<1e-6,`on the floor (${f.v.position.y})`);
 // Under the fuselage: it stops on the post's top, still landing.
 const g=hovering(solid(floor,nose));stepFlight(g.v,1/60,{});hover(g);
 stepFlight(g.v,1/60,{takeoff:true});land(g,600);
 assert.equal(g.v.flightState,'Landing');assert.equal(g.v.flightWarning,'Landing path blocked');
 assert.ok(g.v.position.y+g.v.bounds.min.y>5.8,`held on the post's top (${g.v.position.y})`);
 stepFlight(g.v,1/60,{takeoff:true});assert.equal(g.v.flightState,'TakingOff','late in a landing the key lifts it off again');
});
