import test from 'node:test';import assert from 'node:assert/strict';import {Vector3} from 'three';
import {captureOrdnanceOwner,resolveOrdnanceOwner} from '../src/ordnance-owner.js';import {RemoteDroids} from '../src/remote.js';import {Ordnance} from '../src/ordnance.js';
test('recon strike keeps launching team/life/Character after owner respawn and team switch',()=>{
 const character={},owner={id:1,generation:2,team:1,human:true,character,alive:true};const shots=[],explosions=[];
 const remote=new RemoteDroids({scene:{add(){},remove(){}},camera:null,collision:{cast:()=>null,gather:()=>[]},lights:{flash(){}},model:()=>null,owner,fire:(def,at,dir,who)=>shots.push(who),explode:(def,at,who)=>explosions.push(who)});
 const droid=remote.launch({def:{droid:{weapons:[],explosion:{}}}},new Vector3(),new Vector3(),0,1);remote.strike(droid,{SalvoCount:2,SalvoDelay:.5,AimDistance:10,strike:{}});remote.updateStrikes(0);owner.alive=false;remote.kill(droid);owner.generation++;owner.team=2;owner.character={};owner.alive=true;remote.updateStrikes(.5);
 for(const attribution of [...shots,...explosions]){assert.equal(attribution.team,1);assert.equal(attribution.generation,2);assert.equal(attribution.character,character);assert.equal(attribution.human,true);}assert.equal(shots.length,2);assert.equal(explosions.length,1);assert.equal(resolveOrdnanceOwner(shots[0]),null);assert.equal(shots[0].networkHuman,undefined);
});
test('shared cast excludes launch life but permits reused Unit new life',()=>{
 const owner={id:4,generation:1,team:1,human:true},frozen=captureOrdnanceOwner(owner);let excluded;
 const ordnance={units:{raycast(origin,dir,far,exclude){excluded=exclude;return null;}},unitHit:{}};
 Ordnance.prototype.castUnits.call(ordnance,new Vector3(),new Vector3(1,0,0),10,frozen);assert.equal(excluded,owner);owner.generation++;Ordnance.prototype.castUnits.call(ordnance,new Vector3(),new Vector3(1,0,0),10,frozen);assert.equal(excluded,null);assert.equal(captureOrdnanceOwner(frozen),frozen);
});
