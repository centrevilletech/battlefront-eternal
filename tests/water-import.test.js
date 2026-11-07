import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {Water} from '../src/water.js';
import {WaterState} from '../src/runtime/water-state.js';

test('native water grid is positive Z and independent of the negative-Z foliage grid',()=>{
  const w=new WaterState({height:-72.5,cell:32,offset:[1024,1024],grid:64,cells:[[33,32,0]],look:{OscillationEnable:[0]}});
  assert.equal(w.over(40,17),true);assert.equal(w.over(40,-17),false);
  const hit=new THREE.Vector3();
  assert.equal(w.crossing(new THREE.Vector3(40,-72,17),new THREE.Vector3(40,-73,17),hit),hit);
  assert.equal(hit.y,-72.5);
  assert.equal(w.crossing(new THREE.Vector3(40,-73,17),new THREE.Vector3(40,-72,17),hit),null);
  assert.equal(w.level,-72.5);
});
