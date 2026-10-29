import { SeatInput } from '../src/seats.js';
import * as THREE from 'three';
import test from 'node:test';
import assert from 'node:assert/strict';
import { fixture } from './flight-fixture.js';
import { predictVehicle, predictionVehicle } from '../src/runtime/vehicle-prediction.js';
import { networkVehicleInput } from '../src/runtime/network-vehicle.js';
import { CommandControls } from '../src/runtime/commands.js';
const command = tick => ({ tick, sequence:tick, epoch:1, generation:1, down:['KeyW','ShiftLeft'], actions:tick===1?['takeoff']:[], look:tick>80?[[.4,.1]]:[], flightSettings:{sensitivity:1,invert:false}, mouseScale:1 });
const vectors = v => Object.fromEntries(Object.entries(v).filter(([,x])=>x?.isVector3||x?.isQuaternion||x?.isMatrix4).map(([k,x])=>[k,x.toArray()]));

test('network flyer replay matches authoritative flight and never changes source or collider', () => {
  const authoritative = fixture(), client = fixture();
  const controls = new CommandControls();
  const pending = Array.from({length:100},(_,i)=>command(i+1));
  for (const c of pending) {
    controls.accept(c);
    const input=networkVehicleInput(authoritative.v,0,controls,c);
    authoritative.v.tick(1/60,input);
  }
  const before=vectors(client.v), body=client.v.body.matrix.toArray();
  const predicted=predictVehicle(client.v,pending,0,1);
  assert.deepEqual(predicted.position.toArray(),authoritative.v.position.toArray());
  assert.deepEqual(predicted.quaternion.toArray(),authoritative.v.quaternion.toArray());
  assert.equal(predicted.power,authoritative.v.power);
  assert.deepEqual(vectors(client.v),before);
  assert.deepEqual(client.v.body.matrix.toArray(),body);
  assert.equal(client.v.flightState,'Landed');
  predicted.present(.5);
  assert.ok(client.v.root.position.distanceTo(client.v.position)>1);
});

test('network seat aim uses desktop mouse scaling and respects seat limits', () => {
  const {v}=fixture();v.flying=false;
  const controls=new CommandControls();
  const c={...command(1),look:[[2048,-2048]],down:[],actions:[]};controls.accept(c);
  networkVehicleInput(v,0,controls,c);
  assert.equal(v.seats[0].aimYaw,-1);
  assert.ok(v.seats[0].pitch<=v.seats[0].view.pitchLimits[1]);
});

test('0–200ms RTT, jitter and 5% snapshot loss preserve replayed pilot motion', () => {
  // Reliable inputs are retained until acknowledged; lost snapshots delay the
  // baseline without deleting those inputs. Exercise the same replay path.
  for (const ground of [false,true]) {
    const {v}=fixture();
    if(ground){v.flying=false;v.repulsor=true;v.rotation=new THREE.Euler();}
    const controls=new CommandControls(), snapshots=[];
    for(let tick=1;tick<=150;tick++) {
      const c=command(tick);controls.accept(c);
      const input=networkVehicleInput(v,0,controls,c);
      if(v.flying)v.tick(1/60,input);else{v.tick(1/120,input);v.tick(1/120,input);}
      if(tick%3===0)snapshots.push({tick, vehicle:predictionVehicle(v)});
    }
    for(const rtt of [0,50,100,200]) {
      for(let target=30;target<=150;target+=6) {
        const halfTrip=rtt/2000*60;
        const baseline=snapshots.filter((s,i)=>i%20!==0&&s.tick+halfTrip+(i%4)*.3<=target-2).at(-1);
        if(!baseline)continue;
        const pending=Array.from({length:target-baseline.tick},(_,i)=>command(baseline.tick+i+1));
        const predicted=predictVehicle(baseline.vehicle,pending,baseline.tick,1,command(baseline.tick).down);
        const actual=snapshots.find(s=>s.tick===target).vehicle;
        assert.ok(predicted.position.distanceTo(actual.position)<1e-8,`${ground?'ground':'air'} RTT${rtt} tick${target}`);
        assert.ok(predicted.quaternion.angleTo(actual.quaternion)<1e-7);
        assert.ok(predicted.predictionPath.size<=24);
      }
    }
  }
});

test('network wheeled pilot steering matches local replay across held keyboard turns',()=>{
  const a=fixture().v,b=fixture().v;
  for(const v of [a,b]){v.flying=false;v.repulsor=false;v.wheeled=true;v.strafeSpeed=0;v.rotation=new THREE.Euler();}
  a.driver=null;a.seats[0].lease={human:false,unit:{networkHuman:true},input:{turn:0,pitch:0}};
  const controls=new CommandControls();
  const pending=Array.from({length:60},(_,i)=>({...command(i+1),down:['KeyW','KeyD'],actions:[],look:[]}));
  for(const c of pending){controls.accept(c);const input=networkVehicleInput(a,0,controls,c);a.tick(1/120,input);a.tick(1/120,input);}
  const predicted=predictVehicle(b,pending,0,1);
  assert.ok(Math.abs(a.yaw)>.1);
  assert.ok(predicted.position.distanceTo(a.position)<1e-8);
  assert.ok(Math.abs(predicted.yaw-a.yaw)<1e-8);
});


test('stationary network gunner preserves desktop mouse rate range while bots stay bounded',()=>{
  const lease={turret:{},unit:{networkHuman:true},intent:{turn:12,pitch:-20,buttons:[0,0]},was:[0,0]};
  const input=new SeatInput().latch(lease);
  assert.equal(input.turn,12);assert.equal(input.pitch,-16);
  lease.unit.networkHuman=false;input.latch(lease);assert.equal(input.turn,1);assert.equal(input.pitch,-1);
});

test('rolling predicted flight bases preserve authority identity and bounded scratch prototypes',()=>{
 const source=fixture().v,before=vectors(source),body=source.body.matrix.toArray();
 const commands=Array.from({length:180},(_,i)=>command(i+1));
 const full=predictVehicle(source,commands,0,1);
 let rolling=source;
 for(const c of commands){
  rolling=predictVehicle(rolling,[c],c.tick-1,1,c.tick>1?command(c.tick-1).down:[]);
  assert.equal(rolling.predictionSource,source);
  assert.equal(Object.getPrototypeOf(rolling.system),source.system);
  assert.equal(Object.getPrototypeOf(rolling.system.collision),source.system.collision);
  assert.equal(rolling.body,source.body);
 }
 assert.deepEqual(rolling.position.toArray(),full.position.toArray());
 assert.deepEqual(rolling.quaternion.toArray(),full.quaternion.toArray());
 assert.deepEqual(vectors(source),before);assert.deepEqual(source.body.matrix.toArray(),body);
});

test('an acknowledged pilot with no pending input keeps the latest authoritative pose',()=>{
 const source=fixture().v,before=vectors(source);
 const predicted=predictVehicle(source,[],42,1);
 assert.ok(predicted);
 assert.equal(predicted.predictionTick,42);
 assert.deepEqual(predicted.position.toArray(),source.position.toArray());
 assert.equal(predicted.predictionPath.size,0);
 assert.deepEqual(vectors(source),before);
});


test('predicted collision queries share the callback owner scratch before an authoritative gather',()=>{
 const source=fixture().v,body=source.body.matrix.toArray(),predicted=predictionVehicle(source);
 const polygons=predicted.system.collision.gather(new THREE.Vector3(0,0,0),.5);
 assert.ok(polygons.some(p=>p.triangles.length));
 assert.equal(predictionVehicle(source).system.collision,predicted.system.collision);
 assert.deepEqual(source.body.matrix.toArray(),body);
});


test('network pilot replay is independent of target-dependent steering assist', () => {
  const authority=fixture(),local=fixture(),controls=new CommandControls();
  authority.system.flightAssists=true;local.system.flightAssists=true;
  const pending=Array.from({length:180},(_,i)=>command(i+1));
  for(const c of pending){
    // Authority sees a changing target while the client has no received actor.
    authority.v.flightAssistId=c.tick%9<6?123:-1;
    controls.accept(c);
    authority.v.tick(1/60,networkVehicleInput(authority.v,0,controls,c));
  }
  const predicted=predictVehicle(local.v,pending,0,1);
  assert.deepEqual(predicted.position.toArray(),authority.v.position.toArray());
  assert.deepEqual(predicted.quaternion.toArray(),authority.v.quaternion.toArray());
  assert.equal(predicted.yawRate,authority.v.yawRate);
  assert.equal(authority.system.flightAssists,true,'weapon/target assist setting is retained');
});

test('single-player flight retains target-dependent steering assist', () => {
  const assisted=fixture(),unassisted=fixture();
  for(const f of [assisted,unassisted]){
    f.system.flightAssists=true;
    for(let tick=0;tick<200;tick++)f.v.tick(1/60,{takeoff:tick===0});
    assert.equal(f.v.flightState,'Flying');
  }
  assisted.v.flightAssistId=123;unassisted.v.flightAssistId=-1;
  assisted.v.tick(1/60,{stickX:.4});unassisted.v.tick(1/60,{stickX:.4});
  assert.ok(Math.abs(assisted.v.yawRate)<Math.abs(unassisted.v.yawRate));
});
