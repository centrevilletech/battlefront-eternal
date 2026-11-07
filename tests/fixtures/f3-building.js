import { Matrix4, Object3D } from 'three';
import { Trap } from '../../src/traps.js';
import { Building } from '../../src/props.js';

// Synthetic static geometry exercises the owned phase without installed game
// files. Its collision, when requested, is a separate ordinary world body.
export function building(name, x, z, {health=100, explosion=null, body=null} = {}) {
  const matrix = new Matrix4().makeTranslation(x,0,z).toArray();
  const model = {sphere:[0,1,0,.5],box:[[-.25,0,-.25],[.25,2,.25]]};
  const looks = {built:{root:new Object3D(),model},destroyed:{root:new Object3D(),model}};
  return new Building({class:name,base:'armedbuilding',settings:{MaxHealth:health},matrix,explosion},
    looks,{built:body,destroyed:null,unbuilt:null},null);
}

export function buildingBody(collision, object) {
  const x = object.position.x, z = object.position.z;
  const body = collision.addBody(new Float32Array([x-2,-1,z,x+2,-1,z,x,3,z]),
    new Uint32Array([0,1,2]),new Uint32Array([0]),object);
  object.bodies.built = body;
  return body;
}

// A trap has independent F4 lifecycle storage and keeps its build pad as the
// service point, rather than the model origin used by ordinary buildings.
export function trap(name,x,z) {
  const matrix=new Matrix4().makeTranslation(x,0,z).toArray();
  const object=new Trap({class:name,base:'trap',settings:{MaxHealth:100},matrix,team:1,
    trap:{pad:{at:[1,0,0]}}},{built:{root:new Object3D(),bones:[],byName:{}},pad:null},{},null);
  object.team=1;
  return object;
}
