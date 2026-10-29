// Acceptance reproduction using the actual shared captured-post hull lifecycle.
// Run explicitly: node --test tests/vehicle-lifecycle-regression.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles } from '../src/vehicles.js';
import { Units } from '../src/registry.js';
import { captureEntityVehicle } from '../src/runtime/network-entity-capture.js';
import { captureGroundCorrection, restoreGroundCorrection } from '../src/runtime/network-entity-ground.js';
import { EntityGameState } from '../src/runtime/network-entity-game.js';

function box(x,y,z,w,h,d) {
  const geometry=new THREE.BoxGeometry(w,h,d).translate(x,y,z);
  return {positions:geometry.attributes.position.array,indices:geometry.index.array,polygons:new Uint32Array(geometry.index.count/3)};
}
function world() {
  const floor=box(0,-1,0,200,2,200),collision=new CollisionWorld(floor.positions,floor.indices,floor.polygons);
  const hull=box(0,1,0,2,2,4),post={desc:{name:'cp'},team:1},units=new Units();
  const definition={model:'tank',label:'Test tank',settings:{MaxHealth:1000,ForwardSpeed:20,ReverseSpeed:8,StrafeSpeed:4,Acceleration:10,Deceleration:10,SetAltitude:.5},seats:[{name:'BODY',settings:{},weapons:[]}],explosion:null};
  const data={models:{tank:{bones:[],parts:[]}},collision:{tank:hull},classes:{tank:definition,enemy:definition},spawns:[{name:'pad',matrix:new THREE.Matrix4().toArray(),team:1,post:'cp',respawnTime:2,classes:{1:'tank',2:'enemy'}}]};
  const vehicles=new Vehicles({scene:new THREE.Scene(),data,buffer:new ArrayBuffer(0),indexBuffer:new ArrayBuffer(0),collision,texture:()=>null,camera:new THREE.PerspectiveCamera(),posts:{posts:[post]},ordnance:{explosion(){}},effects:{play(){return null}},units});
  return {vehicles,post,units,playerUnit:{seat:null}};
}

test('authoritative captured-post hull respawn creates the corresponding client variant',()=>{
  const authority=world(),client=world();
  const adapter={game:client,registry:{classes:new Map([[1,'tank'],[2,'enemy']])},presentation:{bindVehicle(){},unbind(){}},
    slots:new Map(),ownsMountAim:EntityGameState.prototype.ownsMountAim,hull:EntityGameState.prototype.hull,destroyHull:EntityGameState.prototype.destroyHull};
  const original=authority.vehicles.vehicles[0];
  EntityGameState.prototype.vehicle.call(adapter,captureEntityVehicle(original,{tick:1,classId:1}));
  original.damage(Infinity);
  EntityGameState.prototype.remove.call(adapter,{id:original.aiTarget.id,generation:original.aiTarget.generation});
  authority.post.team=0;authority.vehicles.updateSpawns(3);
  assert.equal(authority.vehicles.vehicles.length,1,'neutral post does not create a hull');
  authority.post.team=2;authority.vehicles.updateSpawns(.1);
  const replacement=authority.vehicles.slots[0].vehicle;
  assert.equal(replacement.name,'enemy');
  assert.notEqual(replacement.aiTarget.id,original.aiTarget.id,'actual shared Vehicles lifecycle creates a new class variant/identity');
  const record=captureEntityVehicle(replacement,{tick:190,classId:2});
  console.log('vehicle-lifecycle-reproduction',JSON.stringify({old:{id:original.aiTarget.id,generation:original.aiTarget.generation,alive:original.alive},record,clientClasses:client.vehicles.vehicles.map(v=>v.name)}));
  // The mount record supplies this authoritative slot fact; the stable
  // adapter ignores it. Packet-order coverage lives in the mount tests.
  adapter.slots.set(record.id,{index:authority.vehicles.slots.indexOf(replacement.slot),generation:record.generation});
  assert.doesNotThrow(()=>EntityGameState.prototype.vehicle.call(adapter,record));
  const remote=client.vehicles.vehicles.find(v=>v.aiTarget.id===record.id);
  assert.equal(remote.name,'enemy');assert.equal(remote.aiTarget.generation,record.generation);
  assert.equal(remote.alive,true);assert.equal(remote.body.enabled,true);
  assert.deepEqual(remote.tickFrom.toArray(),record.position,'new hull initializes shared presentation history');
  assert.doesNotThrow(()=>restoreGroundCorrection(remote,captureGroundCorrection(replacement)),
    'boarding a newly created semantic hull accepts its exact ground correction');
});
