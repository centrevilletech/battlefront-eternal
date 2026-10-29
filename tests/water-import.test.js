import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {Water} from '../src/water.js';
import {WaterState} from '../src/runtime/water-state.js';

test('water retains rectangular divisions and custom shore patch triangles',()=>{
  const data={cell:32,offset:[64,64],height:-72.5,cells:[[1,1,0],[2,1,1]],patches:[{
    positions:[[0,-72.5,-32],[16,-72.5,-32],[0,-72.5,-16]],uvs:[[0,0],[.5,0],[0,.5]],indices:[0,1,2],
  }]};
  const g=Water.prototype.surface.call({...data,data},2,3);
  assert.equal(g.attributes.position.count,12+3);
  assert.equal(g.index.count,36+3);
  assert.deepEqual(Array.from(g.attributes.position.array.slice(-9)),data.patches[0].positions.flat());
  assert.deepEqual(Array.from(g.attributes.uv.array.slice(-6)),data.patches[0].uvs.flat());
  assert.equal(g.attributes.position.getZ(11),0);
  assert.deepEqual(Array.from(g.index.array.slice(-3)),[12,13,14]);
});

test('native water grid is positive Z and independent of the negative-Z foliage grid',()=>{
  const w=new WaterState({height:-72.5,cell:32,offset:[1024,1024],grid:64,cells:[[33,32,0]],look:{OscillationEnable:[0]}});
  assert.equal(w.over(40,17),true);assert.equal(w.over(40,-17),false);
  const hit=new THREE.Vector3();
  assert.equal(w.crossing(new THREE.Vector3(40,-72,17),new THREE.Vector3(40,-73,17),hit),hit);
  assert.equal(hit.y,-72.5);
  assert.equal(w.crossing(new THREE.Vector3(40,-73,17),new THREE.Vector3(40,-72,17),hit),null);
  w.step(1);assert.equal(w.level,-72.5);
});
