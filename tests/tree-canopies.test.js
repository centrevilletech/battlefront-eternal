import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {RustRender} from '../src/rust-render/index.js';
import {standinModule,standinThree} from './support/rust-standin.mjs';
import {SceneCanopyHeader as H,SceneCanopySelection as S} from '../src/rust-render/scene-layout.js';
import { TreeCanopies } from '../src/tree-canopies.js';
globalThis.location??={search:''};

const LEAVES = 912;
function fixture(count = 16, locations = null) {
  let detail = 'enhanced';
  const {BfeRenderer,memory,calls}=standinModule(),{graphics}=standinThree(calls);
  graphics.settings={get:key=>key==='treeDetail'?detail:undefined};
  const core=new RustRender(graphics,{memory},BfeRenderer);graphics.rust=core;
  const instances = Array.from({length:count},(_,i)=>({
    matrix:new THREE.Matrix4().makeTranslation(...(locations?.[i] || [0,0,-30-i])).toArray(),
    bounds:[[-4,0,-4],[4,12,4]],
  }));
  const trees = new TreeCanopies(graphics,{instances,texture:'native-leaves'},()=>new THREE.Texture());
  core.registry.registerCanopies(trees);trees.stateMap=core.resources.gpu.canopy_state(trees.groups.length,trees.states);
  const camera = new THREE.PerspectiveCamera(75,1,0.1,1000);
  camera.position.set(0,6,0);camera.updateMatrixWorld(true);
  return {trees,camera,graphics,selected:()=>{core.views();const rows=core.sceneCanopyOutput;return Array.from({length:rows[H.COUNT]},(_,i)=>rows[H.STRIDE+i*S.STRIDE+S.ID]);},setDetail:value=>{detail=value;}};
}

test('enhanced canopy budget leaves excluded originals available',()=>{
  const f=fixture();f.trees.update(f.camera);
  assert.equal(f.selected().length,12);
  assert.deepEqual(f.selected(),Array.from({length:12},(_,i)=>i));
  assert.equal(f.trees.instanceCount,12*LEAVES);
  assert.equal(f.trees.states.filter((value,i)=>i%4===0&&value===1).length,12);
  for(const group of f.trees.groups) {
    assert.equal(f.trees.states[group.index*4],f.selected().includes(group.index)?1:0);
    assert.equal(group.centers.length,LEAVES*3);
    for(let i=0;i<group.centers.length;i+=3)assert.ok(group.world.containsPoint(new THREE.Vector3(...group.centers.slice(i,i+3))));
  }
});

test('Original detail disables enhanced instances and restores every native card',()=>{
  const f=fixture();f.trees.update(f.camera);f.setDetail('original');f.trees.update(f.camera);
  assert.equal(f.trees.instanceCount,0);
  assert.equal(f.selected().length,0);
  assert.ok(f.trees.states.every(value=>value===0));
  f.setDetail('enhanced');f.trees.update(f.camera);
  assert.equal(f.trees.instanceCount,12*LEAVES);
});

test('canopy frustum selection follows stationary-camera zoom changes',()=>{
  const f=fixture(1,[[25,0,-40]]);
  f.trees.update(f.camera);assert.equal(f.selected().length,1);
  f.camera.fov=10;f.camera.updateProjectionMatrix();
  f.trees.update(f.camera);
  assert.equal(f.selected().length,0,'narrowing the projection must release a now-offscreen canopy group');
  assert.ok(f.trees.states.every(value=>value===0));
});

test('canopy placement retains raw dirty rows without Three descriptors',()=>{
  const f=fixture();f.trees.update(f.camera);
  assert.equal(f.trees.mesh,undefined);
  assert.equal(f.trees.node.parent,null);
  assert.deepEqual(f.trees.takeDirty(),[0,12*LEAVES]);
  assert.equal(f.trees.takeDirty(),null);
  f.camera.position.set(0,6,-48);f.camera.lookAt(0,6,0);f.camera.updateMatrixWorld(true);f.trees.update(f.camera);
  const dirty=f.trees.takeDirty();
  assert.ok(dirty&&dirty[0]%LEAVES===0&&dirty[1]%LEAVES===0&&dirty[1]>dirty[0]);
  assert.equal(f.trees.centers instanceof Float32Array,true);
});
