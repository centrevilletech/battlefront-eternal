import * as THREE from 'three';
import { Vehicles, Vehicle } from '../../src/vehicles.js';
import { Unit } from '../../src/units.js';
import { bridgeFor } from '../../src/sim/runtime.js';
import { attachF2 } from '../../src/f2-sim.js';

// Real native fleet construction over synthetic geometry. No map pack, actor
// model or retail content is required to exercise incoming combat and leases.
export function fleetFixture({positions = [0,5], health = 100, explosion = null, height = 0, weapon = null, reserve = null, respawnTime = 5, base = 'hover', legs = null, neutralPad = -1} = {}) {
  return ({instance,collision,units,ordnance}) => {
    const box = new THREE.BoxGeometry(2,2,2).translate(0,1,0);
    const shape = {positions:Array.from(box.attributes.position.array),
      indices:Array.from(box.index.array), polygons:Array(box.index.count/3).fill(0)};
    const def = {model:'tank',base,settings:{MaxHealth:health,ForwardSpeed:12,TurnRate:1},
      seats:[{name:'BODY',settings:{},weapons:weapon ? [{def:weapon,aimers:[]}] : []}],explosion};
    if(legs)def.fx={legs};
    const data = {models:{tank:{bones:[],parts:[],sphere:[0,1,0,1]}}, collision:{tank:shape},
      classes:{tank:def},spawns:positions.map((at,i)=>({name:`pad${i}`,team:1,post:i === neutralPad ? 'neutral' : undefined,respawnTime,
        matrix:new THREE.Matrix4().makeTranslation(Array.isArray(at)?at[0]:at,height,Array.isArray(at)?at[1]:0).toArray(),classes:{1:'tank'}}))};
    const buffer = new ArrayBuffer(0), indexBuffer = new ArrayBuffer(0);
    const vehicles = new Vehicles({scene:new THREE.Scene(),camera:new THREE.PerspectiveCamera(),
      data,buffer,indexBuffer,collision,units,ordnance,
      posts:{posts:[{desc:{name:'neutral'},team:0}]},texture(){},effects:{play(){}}});
    vehicles.clock = ()=>vehicles.time;
    // A pickup fixture prepares a depleted weapon before its sole adoption.
    if (reserve !== null) for (const v of vehicles.vehicles) for (const seat of v.seats) for (const w of seat.weapons) w.state.reserve = reserve;
    attachF2(vehicles,bridgeFor(instance,{classes:{Unit,Vehicle}}),
      {killY:-100,deathRegions:[],scripted:[]},{data,buffer,indexBuffer},Unit);
    return vehicles;
  };
}
