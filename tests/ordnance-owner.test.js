import test from 'node:test';import assert from 'node:assert/strict';import {Vector3} from 'three';
import {captureOrdnanceOwner,resolveOrdnanceOwner} from '../src/ordnance-owner.js';import {OrdnanceWorld} from '../src/ordnance-world.js';
// The query runs in the native ordnance owner. A reused source object is
// excluded only while its generation still names the launching life.
test('shared cast excludes launch life but permits reused Unit new life',()=>{
 const owner={id:4,generation:1,team:1,human:true,alive:true,sphere:{center:new Vector3(3,0,0),radius:1},hitShapes:()=>[{type:'sphere',part:'body',center:new Vector3(3,0,0),radius:1}]};
 const frozen=captureOrdnanceOwner(owner),unitHit={point:new Vector3(),normal:new Vector3()};
 const world=new OrdnanceWorld({units:{list:[owner]},unitHit});
 assert.equal(world.raycastUnits(new Vector3(),new Vector3(1,0,0),10,owner,null,null,null),null);
 assert.equal(world.raycastUnits(new Vector3(),new Vector3(1,0,0),10,frozen,null,null,null),null);
 owner.generation++;
 const hit=world.raycastUnits(new Vector3(),new Vector3(1,0,0),10,frozen,null,null,null);
 assert.equal(hit.unit,owner);assert.equal(hit.distance,2);assert.deepEqual(hit.point.toArray(),[2,0,0]);
 assert.equal(captureOrdnanceOwner(frozen),frozen);
});

test('a native creation notice restores sampled metadata before identity references are decoded',async()=>{
 const {prepareOrdnanceOwner,restoreOrdnanceOwner}=await import('../src/ordnance-owner.js');
 const {Identity}=await import('../src/sim/runtime.js');
 const {KIND_OWNER_SNAPSHOT}=await import('../src/sim/gen/index.js');
 const character={},source={id:3,generation:8,team:2,human:false,character:{},alive:true,removed:false,origin:out=>out.set(1,2,3)};
 const owner=restoreOrdnanceOwner(source,{id:3,life:7,team:1,human:true,className:'recon-owner',character},prepareOrdnanceOwner());
 const identities=new Identity(KIND_OWNER_SNAPSHOT);
 identities.bind(4,2,owner);
 assert.equal(identities.at(4,2),owner);
 assert.equal(identities.slotOf(owner),4);
 assert.equal(owner.character,character);
 assert.equal(owner.team,1);
 assert.equal(owner.generation,7);
 assert.equal(owner.alive,false);
 assert.equal(owner.removed,true);
 assert.equal(resolveOrdnanceOwner(owner),null);
 assert.deepEqual(owner.origin(new Vector3()).toArray(),[1,2,3]);
});
