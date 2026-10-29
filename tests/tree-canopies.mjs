import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { TreeCanopies } from '../src/tree-canopies.js';

const LEAVES = 912;
function fixture(count = 16, locations = null) {
  let detail = 'enhanced';
  const graphics = {scene:new THREE.Scene(),farScene:new THREE.Scene(),settings:{get:()=>detail}};
  const instances = Array.from({length:count},(_,i)=>({
    matrix:new THREE.Matrix4().makeTranslation(...(locations?.[i] || [0,0,-30-i])).toArray(),
    bounds:[[-4,0,-4],[4,12,4]],
  }));
  const trees = new TreeCanopies(graphics,{instances,texture:'native-leaves'},()=>new THREE.Texture());
  const camera = new THREE.PerspectiveCamera(75,1,0.1,1000);
  camera.position.set(0,6,0);camera.updateMatrixWorld(true);
  return {trees,camera,graphics,setDetail:value=>{detail=value;}};
}

function cards(rectangles) {
  const position=[],index=[];
  for(const [x,y,z,width,height] of rectangles) {
    const n=position.length/3;
    position.push(x-width/2,y,z,x+width/2,y,z,x-width/2,y+height,z,x+width/2,y+height,z);
    index.push(n,n+1,n+2,n+2,n+1,n+3);
  }
  const geometry=new THREE.BufferGeometry();
  geometry.setAttribute('position',new THREE.Float32BufferAttribute(position,3));geometry.setIndex(index);
  const material=new THREE.ShaderMaterial({uniforms:{},vertexShader:'varying vec3 vWorld; void main() { vWorld=position; gl_Position=vec4(position,1.0); }',fragmentShader:'varying vec3 vWorld; void main() { gl_FragColor=vec4(1.0); }'});
  return new THREE.Mesh(geometry,material);
}

test('enhanced canopy budget leaves excluded originals available',()=>{
  const f=fixture();f.trees.update(f.camera);
  assert.equal(f.trees.active.length,12);
  assert.equal(f.trees.mesh.geometry.instanceCount,12*LEAVES);
  assert.equal(f.trees.states.filter((value,i)=>i%4===0&&value===1).length,12);
  for(const group of f.trees.groups) {
    assert.equal(f.trees.states[group.index*4],f.trees.active.includes(group)?1:0);
    assert.equal(group.centers.length,LEAVES*3);
    for(let i=0;i<group.centers.length;i+=3)assert.ok(group.world.containsPoint(new THREE.Vector3(...group.centers.slice(i,i+3))));
  }
});

test('Original detail disables enhanced instances and restores every native card',()=>{
  const f=fixture();f.trees.update(f.camera);f.setDetail('original');f.trees.update(f.camera);
  assert.equal(f.trees.mesh.geometry.instanceCount,0);
  assert.equal(f.trees.active.length,0);
  assert.ok(f.trees.states.every(value=>value===0));
  f.setDetail('enhanced');f.trees.update(f.camera);
  assert.equal(f.trees.mesh.geometry.instanceCount,12*LEAVES);
});

test('giant bush mask preserves small foliage and unmatched islands without moving source vertices',()=>{
  const f=fixture(1);
  const mesh=cards([[0,0,-30,7,8],[0,1,-29,1,1],[40,0,-30,8,8]]);
  const beforePosition=Array.from(mesh.geometry.attributes.position.array);
  const beforeIndex=Array.from(mesh.geometry.index.array);
  f.trees.adapt(mesh,{texture:'yav_prop_bush1'});
  const ids=Array.from(mesh.geometry.attributes.treeGroup.array);
  assert.deepEqual(ids.slice(0,4),[0,0,0,0]);
  assert.ok(ids.slice(4).every(id=>id===-1));
  assert.equal(mesh.userData.treeCanopyPieces,1);
  assert.deepEqual(Array.from(mesh.geometry.attributes.position.array),beforePosition);
  assert.deepEqual(Array.from(mesh.geometry.index.array),beforeIndex);
  assert.equal(mesh.material.uniforms.treeStates.value,f.trees.stateMap);
});

test('ordinary materials receive no canopy mask or shader changes',()=>{
  const f=fixture(1),mesh=cards([[0,0,-30,7,8]]);
  const fragment=mesh.material.fragmentShader;
  f.trees.adapt(mesh,{texture:'stone_wall'});
  assert.equal(mesh.geometry.attributes.treeGroup,undefined);
  assert.equal(mesh.material.fragmentShader,fragment);
});

test('canopy frustum selection follows stationary-camera zoom changes',()=>{
  const f=fixture(1,[[25,0,-40]]);
  f.trees.update(f.camera);assert.equal(f.trees.active.length,1);
  f.camera.fov=10;f.camera.updateProjectionMatrix();
  f.trees.update(f.camera);
  assert.equal(f.trees.active.length,0,'narrowing the projection must release a now-offscreen canopy group');
  assert.ok(f.trees.states.every(value=>value===0));
});
