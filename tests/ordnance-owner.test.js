import test from 'node:test';import assert from 'node:assert/strict';import {Vector3} from 'three';
import {captureOrdnanceOwner,resolveOrdnanceOwner} from '../src/ordnance-owner.js';import {RemoteDroids} from '../src/remote.js';import {OrdnanceWorld} from '../src/ordnance-world.js';
test('recon strike keeps launching team/life/Character after owner respawn and team switch',()=>{
 const character={},owner={id:1,generation:2,team:1,human:true,character,alive:true};const shots=[],explosions=[];
 const remote=new RemoteDroids({scene:{add(){},remove(){}},camera:null,collision:{cast:()=>null,gather:()=>[]},lights:{flash(){}},model:()=>null,owner,fire:(def,at,dir,who)=>shots.push(who),explode:(def,at,who)=>explosions.push(who)});
 const droid=remote.launch({def:{droid:{weapons:[],explosion:{}}}},new Vector3(),new Vector3(),0,1);remote.strike(droid,{SalvoCount:2,SalvoDelay:.5,AimDistance:10,strike:{}});remote.updateStrikes(0);owner.alive=false;remote.kill(droid);owner.generation++;owner.team=2;owner.character={};owner.alive=true;remote.updateStrikes(.5);
 for(const attribution of [...shots,...explosions]){assert.equal(attribution.team,1);assert.equal(attribution.generation,2);assert.equal(attribution.character,character);assert.equal(attribution.human,true);}assert.equal(shots.length,2);assert.equal(explosions.length,1);assert.equal(resolveOrdnanceOwner(shots[0]),null);assert.equal(shots[0].networkHuman,undefined);
});
test('shared cast excludes launch life but permits reused Unit new life',()=>{
 const owner={id:4,generation:1,team:1,human:true},frozen=captureOrdnanceOwner(owner);let excluded;
 const world=new OrdnanceWorld({units:{raycast(origin,dir,far,exclude){excluded=exclude;return null;}},unitHit:{}});
 world.raycastUnits(new Vector3(),new Vector3(1,0,0),10,frozen,null,null,null);assert.equal(excluded,owner);owner.generation++;world.raycastUnits(new Vector3(),new Vector3(1,0,0),10,frozen,null,null,null);assert.equal(excluded,null);assert.equal(captureOrdnanceOwner(frozen),frozen);
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
