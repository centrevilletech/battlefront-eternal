import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { fixture, FIXTURE_SPEEDS, FIXTURE_HANDLING } from './flight-fixture.js';
import { VehicleDriver } from '../src/vehicles.js';
function mounted() {
 const f=fixture();f.v.driver=null;
 const soldier={position:new THREE.Vector3(),previous:new THREE.Vector3(),settled:new THREE.Vector3(),velocity:new THREE.Vector3(),aimYaw:0,aimPitch:0,spawn(x,y,z,yaw){this.position.set(x,y,z);this.velocity.set(0,0,0);this.aimYaw=yaw;}};
 const actor={root:new THREE.Group()},animator={gunner(){},reset(){}},combat={dead:false,mounted:false,team:1};
 const unit={id:7,generation:1,alive:true,soldier,actor,animator,combat};combat.owner=unit;
 const driver=new VehicleDriver({vehicles:f.system,soldier,combat,actor,animator,camera:f.system.camera,collision:f.c,hud:{message(){}},presentation:{reset(){}}});
 assert.ok(driver.enter(f.v));return {...f,driver,unit,soldier,combat};
}
test('destruction severs both seat links and clears copied ship motion without relying on a game callback',()=>{
 const f=mounted(),lease=f.driver.lease;f.v.flightState='Flying';f.v.velocity.set(90,25,10);f.driver.sync();assert.ok(f.soldier.velocity.length()>90);
 f.driver.hideHull(f.v);f.v.damage(Infinity);
 assert.equal(f.driver.vehicle,null);assert.equal(f.driver.lease,null);assert.equal(f.unit.seat,null);assert.equal(f.v.seats[0].lease,null);assert.equal(f.v.driver,null);assert.equal(f.v.humanDriver,null);assert.equal(lease.released,true);
 assert.equal(f.combat.mounted,false);assert.equal(f.soldier.velocity.length(),0);assert.equal(f.soldier.air,null);assert.equal(f.driver.hiddenHull,null);assert.equal(f.actor?.root?.rotation?.z||0,0);
 f.driver.release();f.v.spawn(1);assert.ok(f.driver.enter(f.v));assert.ok(f.driver.exit());assert.equal(f.unit.seat,null);assert.equal(f.combat.mounted,false);
});
test('exit airborne bails out beside the craft, at rest; landed he steps out; stale local lease is repaired',()=>{
 const f=mounted();f.v.flightState='Flying';f.v.position.set(0,120,0);f.v.velocity.set(0,0,60);f.v.syncBody();f.driver.sync();
 assert.equal(f.driver.exit(),true);assert.equal(f.unit.seat,null);assert.equal(f.v.seats[0].lease,null);assert.equal(f.combat.mounted,false);
 assert.ok(Math.abs(f.soldier.position.y-120)<1e-9,'left in the air at the craft\'s height, not set on the ground below');
 assert.ok(Math.hypot(f.soldier.position.x-f.v.position.x,f.soldier.position.z-f.v.position.z)>f.v.radius,'clear of the hull');
 assert.equal(f.soldier.velocity.length(),0,'at rest, as the original game\'s exit leaves him');
 const g=mounted();g.v.flightState='Landed';g.driver.lease=null;assert.equal(g.driver.exit(),true);assert.equal(g.unit.seat,null);assert.equal(g.v.seats[0].lease,null);assert.equal(g.combat.mounted,false);
 assert.ok(Math.abs(g.soldier.position.y)<1.5,'landed, he steps out onto the floor');
});
test('a landing starts wherever it is asked for: a post under a wing lets it down, one under its fuselage holds it until he lifts off',async()=>{
 const {CollisionWorld}=await import('../src/collision.js');const {stepFlight}=await import('../src/flight/model.js');
 const solid=(...boxes)=>{const pos=[],idx=[];for(const [x,y,z,w,h,d] of boxes){const g=new THREE.BoxGeometry(w,h,d).translate(x,y,z),base=pos.length/3;pos.push(...g.attributes.position.array);idx.push(...[...g.index.array].map(i=>i+base));}return new CollisionWorld(new Float32Array(pos),new Uint32Array(idx),new Uint32Array(idx.length/3));};
 const floor=[0,-1,0,20000,2,20000],wing=[.9,3,0,.4,6,.4],nose=[0,3,1.9,.4,6,.4];// under the hull's right edge; under its nose
 // A craft that hovers (MidSpeed 0, as the LAAT/i and the MAF do), so it
 // comes straight down over the posts.
 const hovering=()=>fixture({...FIXTURE_SPEEDS,MinSpeed:'-15.0',MidSpeed:'0.0',...FIXTURE_HANDLING});
 const hover=f=>{f.v.flightState='Flying';f.v.gear=0;f.v.position.set(0,10,0);f.v.quaternion.identity();f.v.airspeed=0;f.v.velocity.set(0,0,0);f.v.syncBody();};
 const land=(f,n)=>{for(let i=0;i<n&&f.v.flightState==='Landing';i++)stepFlight(f.v,1/60,{});};
 // Under a wing: the 2004 game's craft meet the ground with a sphere about
 // the fuselage; the post goes into the wing.
 const f=hovering();stepFlight(f.v,1/60,{});f.system.collision=solid(floor,wing);hover(f);
 stepFlight(f.v,1/60,{takeoff:true});assert.equal(f.v.flightState,'Landing');
 land(f,1200);assert.equal(f.v.flightState,'Landed');assert.ok(Math.abs(f.v.position.y-(-f.v.bounds.min.y+.15))<1e-6,`on the floor (${f.v.position.y})`);
 // Under the fuselage: it stops on the post's top, still landing.
 const g=hovering();stepFlight(g.v,1/60,{});g.system.collision=solid(floor,nose);hover(g);
 stepFlight(g.v,1/60,{takeoff:true});land(g,600);
 assert.equal(g.v.flightState,'Landing');assert.equal(g.v.flightWarning,'Landing path blocked');
 assert.ok(g.v.position.y+g.v.bounds.min.y>5.8,`held on the post's top (${g.v.position.y})`);
 stepFlight(g.v,1/60,{takeoff:true});assert.equal(g.v.flightState,'TakingOff','late in a landing the key lifts it off again');
});
