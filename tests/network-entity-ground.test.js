import test from 'node:test';import assert from 'node:assert/strict';import * as THREE from 'three';
import {fixture} from './flight-fixture.js';
import {CollisionWorld} from '../src/collision.js';
import {Vehicles} from '../src/vehicles.js';
import {predictionVehicle} from '../src/runtime/vehicle-prediction.js';
import {captureGroundCorrection,restoreGroundCorrection,groundCorrectionBytes} from '../src/runtime/network-entity-ground.js';
function ground(kind){
 const shape=(w,h,d,y=0)=>{const g=new THREE.BoxGeometry(w,h,d).translate(0,y,0);return {positions:g.attributes.position.array,indices:g.index.array,polygons:new Uint32Array(g.index.count/3)};};
 const floor=shape(20000,2,20000,-1),hull=shape(2,2,4,1),collision=new CollisionWorld(floor.positions,floor.indices,floor.polygons);
 const name=kind==='wheeled'?'test_hailfire':kind;
 const def={model:'hull',base:kind==='walker'||kind==='creature'?'walker':'hover',settings:{MaxHealth:1000,ForwardSpeed:12,ReverseSpeed:6,StrafeSpeed:kind==='hover'||kind==='springs'?5:0,HealthType:kind==='creature'?'animal':'vehicle'},seats:[{name:'BODY',settings:{},weapons:[]}],explosion:null};
 // A hover on springs (src/vehicle-hover.js): its pitch and roll rates and
 // ground factor are state the correction carries.
 if(kind==='springs'){Object.assign(def.settings,{GravityScale:4,Traction:20,VelocitySpring:5,VelocityDamp:2.5,OmegaXSpring:3,OmegaXDamp:3,OmegaZSpring:5,OmegaZDamp:1.5,LevelSpring:2,LevelDamp:1});
  def.springs=[{at:[-.8,.4,1],radius:1,length:.5,factors:[1,1,-1,1,-1,1]},{at:[.8,.4,1],radius:1,length:.5,factors:[1,1,-1,1,1,1]},{at:[0,.8,-1],radius:1.4,length:.5,factors:[1,1,2,1,0,1]}];}
 const data={models:{hull:{bones:[],parts:[]}},collision:{hull},classes:{[name]:def},spawns:[{name:'pad',matrix:new THREE.Matrix4().toArray(),team:1,post:'cp',respawnTime:2,classes:{1:name}}]};
 const system=new Vehicles({scene:new THREE.Scene(),data,buffer:new ArrayBuffer(0),indexBuffer:new ArrayBuffer(0),collision,texture:()=>null,camera:new THREE.PerspectiveCamera(),posts:{posts:[{desc:{name:'cp'},team:1}]},ordnance:{explosion(){}},effects:{play(){return null;}}});
 const v=system.vehicles[0];v.driver={};v.seatIndex=0;v.captureTick();return predictionVehicle(v);
}
test('ground correction resumes exact 120Hz hover, spring, wheel, walker and creature motion',()=>{
 for(const kind of ['hover','springs','wheeled','walker','creature']){
  const a=ground(kind);let b;
  for(let tick=1;tick<=240;tick++){
   const input={forward:tick<150?1:-1,right:tick>40?.4:0,turn:tick>90?.2:0,brake:tick>200};
   a.captureTick();a.tick(1/120,input);a.tick(1/120,input);
   if([10,80,155,210].includes(tick)){
    b=ground(kind);for(const x of Object.values(b))if(x?.isVector3)x.set(11,-42,7);
    const body=b.body.matrix.toArray();restoreGroundCorrection(b,captureGroundCorrection(a));assert.deepEqual(b.body.matrix.toArray(),body);assert.deepEqual(b.tickFrom,a.position);
   }else if(b){b.captureTick();b.tick(1/120,input);b.tick(1/120,input);}
   if(b)assert.deepEqual(captureGroundCorrection(b),captureGroundCorrection(a),`${kind} tick ${tick}`);
  }
 }
 // Eleven numbers (a hover's tilt and roll rates and ground factor among
 // them), position, velocity, quaternion, two flags and six seat fields.
 assert.equal(groundCorrectionBytes,224);
});
test('ground correction validates before mutation and requires matching body kind',()=>{
 const a=ground('hover'),before=captureGroundCorrection(a),bad=before.slice();new DataView(bad.buffer).setFloat64(0,Infinity,true);
 assert.throws(()=>restoreGroundCorrection(a,bad));assert.deepEqual(captureGroundCorrection(a),before);assert.throws(()=>captureGroundCorrection(fixture().v));
});
