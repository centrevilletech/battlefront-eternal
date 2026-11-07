import { loadEnvironment } from '../src/environment/module.js';
await loadEnvironment();
// Asset construction must register the required Rust foliage classes without
// keeping a selectable CPU population. The fixture checks retained card and
// placement inputs rather than comparing against a deleted JS selector.
// A missing renderer is an initialization error, not permission to draw in Three.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Foliage } from '../src/foliage.js';
function build(def, instances) {
  return new Foliage(new THREE.Scene(), {classes:{plant:def},instances:instances.map(i=>({class:'plant',...i}))}, {drawer:{select(){}},texture:()=>null,nearEnd:()=>100});
}
const matrix=(x=0,y=0,z=-10)=>new THREE.Matrix4().makeTranslation(x,y,z).toArray();

// The whole texture is one part, so mirroring must not switch atlas quadrants.
// Inspecting authored cards catches an accidental resource-construction change.
test('single-image grass retains the whole atlas and alternating mirrors',()=>{
  const f=build({base:'grasspatch',NumParts:1,NumParticles:4,MinSize:1,MaxSize:1,Alpha:1},[{matrix:matrix()}]);
  const uv=f.kinds.get('plant').batch.geometry.attributes.uv.array;
  assert.deepEqual([...uv.slice(0,16)],[0,1,1,1,0,0,1,0,1,1,0,1,1,0,0,0]);
});

// Distance/tint are retained inputs to the Rust selector, not a JS selection.
// They must survive import even though the old CPU drawer has been deleted.
test('painted grass retains the distance rule and imported tint consumed by Rust',()=>{
  const f=build({base:'grasspatch',NumParticles:1,MinSize:1,MaxSize:1,MaxDistance:20},[{matrix:matrix(0,0,-50)},{matrix:matrix(0,0,-50),terrain:true,distance:120,tint:[.4,.5,.6]}]);
  const kind=f.kinds.get('plant');
  assert.deepEqual(kind.instances[1].tint,[.4,.5,.6]);
  assert.equal(kind.instances[0].maxDistance,20);
  assert.equal(kind.instances[1].maxDistance,0);
  assert.equal(kind.instances[1].cullDistance,177.46410161513776);
});

// The authored up axis preserves slope while patch yaw is deliberately ignored.
// A generic full-matrix import would change the visible billboard orientation.
test('painted bushes discard placement yaw while retaining slope',()=>{
  const m=new THREE.Matrix4().makeRotationY(1.2).multiply(new THREE.Matrix4().makeRotationX(.3)).setPosition(0,0,-60);
  const f=build({base:'leafpatch',NumParticles:1,MinSize:1,MaxSize:1,Radius:1,Height:1},[{matrix:m.toArray(),terrain:true,distance:30}]);
  const inst=f.kinds.get('plant').instances[0];
  assert.equal(inst.matrix.elements[2],0);
  assert.ok(Math.abs(inst.matrix.elements[6])>.01);
  assert.equal(inst.maxDistance,30);
});
