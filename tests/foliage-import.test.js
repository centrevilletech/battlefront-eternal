import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Foliage } from '../src/foliage.js';

function build(gpu, def, instances) {
  return new Foliage(new THREE.Scene(), {classes:{plant:def},instances:instances.map(i=>({class:'plant',...i}))},
    {gpu,texture:()=>null,nearEnd:()=>100});
}
const matrix = (x=0,y=0,z=-10) => new THREE.Matrix4().makeTranslation(x,y,z).toArray();

test('single-image tall grass uses the whole image and alternating mirrors on both paths',()=>{
  const def={base:'grasspatch',NumParts:1,NumParticles:4,MinSize:1,MaxSize:1,Alpha:1};
  for(const gpu of [false,true]) {
    const f=build(gpu,def,[{matrix:matrix()}]);f.update(0,new THREE.PerspectiveCamera(70,1,.1,200));
    const uvs=f.kinds.get('plant').batch.geometry.attributes.uv.array;
    assert.deepEqual(Array.from(uvs.slice(0,8)),[0,1,1,1,0,0,1,0]);
    assert.deepEqual(Array.from(uvs.slice(8,16)),[1,1,0,1,1,0,0,0]);
  }
});

test('painted and placed grass retain separate distance rules and imported tint',()=>{
  const def={base:'grasspatch',NumParticles:1,MinSize:1,MaxSize:1,MaxDistance:20};
  for(const gpu of [false,true]) {
    const f=build(gpu,def,[{matrix:matrix(0,0,-50)}, {matrix:matrix(0,0,-50),terrain:true,distance:120,tint:[.4,.5,.6]}]);
    f.update(0,new THREE.PerspectiveCamera(70,1,.1,200));
    const kind=f.kinds.get('plant');assert.equal(kind.batch.count,1);
    assert.deepEqual(kind.instances[1].tint,[.4,.5,.6]);
    assert.equal(kind.instances[1].maxDistance,0);
  }
});

test('painted bushes use the layer distance and discard placement yaw while retaining slope',()=>{
  const m=new THREE.Matrix4().makeRotationY(1.2).multiply(new THREE.Matrix4().makeRotationX(.3)).setPosition(0,0,-60);
  for(const gpu of [false,true]) {
    const f=build(gpu,{base:'leafpatch',NumParticles:1,MinSize:1,MaxSize:1,Radius:1,Height:1},
      [{matrix:m.toArray(),terrain:true,distance:30}]);
    f.update(0,new THREE.PerspectiveCamera(70,1,.1,200));
    const kind=f.kinds.get('plant');assert.equal(kind.batch.count,0);
    const basis=kind.instances[0].matrix.elements;
    assert.equal(basis[2],0); assert.ok(Math.abs(basis[6])>.01);
  }
});

test('radial grass bottoms are clipped with matching CPU/GPU texture coordinates',()=>{
  const def={base:'grasspatch',NumParts:1,NumParticles:12,MinSize:1,MaxSize:1,RadiusFadeMin:0,RadiusFadeMax:3};
  const original=Math.random;
  function make(gpu){let seed=7;Math.random=()=>((seed=Math.imul(seed,1664525)+1013904223>>>0)/4294967296);try{return build(gpu,def,[{matrix:matrix()}]);}finally{Math.random=original;}}
  const cpu=make(false),gpu=make(true),camera=new THREE.PerspectiveCamera(70,1,.1,200);
  cpu.update(0,camera);gpu.update(0,camera);
  const a=cpu.kinds.get('plant').batch.geometry.attributes.uv.array;
  const b=gpu.kinds.get('plant').batch.geometry.attributes.uv.array;
  assert.deepEqual(a,b);assert.ok(a[1]<1&&a[1]>0);
});
