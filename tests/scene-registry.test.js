// Mutation fixtures exercise the real CPU-only registry through its generated
// boundary. GPU registration is stood in; visibility and order remain native.
// Repeated changes during a paused frame must coalesce by qualified identity.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { RustRender } from '../src/rust-render/index.js';
import {sceneProperty,sceneTransform} from '../src/rust-render/scene-changes.js';
import { Family, Mode, publishSceneWorld } from '../src/rust-render/scene.js';
import { SceneCapacity, SceneNode, SceneWorldChange, SceneSelection, SceneFlags } from '../src/rust-render/scene-layout.js';
import { standinModule, standinThree } from './support/rust-standin.mjs';
globalThis.location ??= {search:''};

function fixture() {
  const {BfeRenderer,memory,calls}=standinModule(),host=standinThree(calls);
  const core=new RustRender(host.graphics,{memory},BfeRenderer);
  return {...host,core,registry:core.registry};
}

test('paused resource and property replacements retain one queued identity',()=>{
  const {registry,scene,camera}=fixture(),mesh=new THREE.Mesh(new THREE.BufferGeometry(),new THREE.Material());
  mesh.frustumCulled=false;scene.add(mesh);registry.select(scene,camera,0);
  // These are cold replacements, not selected draws. Empty geometry keeps the
  // fixture independent of any catalog while exercising the actual observers.
  registry.fresh.length=registry.added.length=0;
  const node=registry.admitIfNew(mesh);node.freshQueued=node.addedQueued=false;
  const first=new THREE.BufferGeometry(),second=new THREE.BufferGeometry();
  for(let i=0;i<100000;i++){sceneProperty(mesh,"geometry",i&1?first:second);sceneProperty(mesh,"visible",Boolean(i&1));sceneProperty(mesh.layers,"mask",i&1?1:2);}
  assert.equal(registry.fresh.length,1);assert.equal(registry.added.length,1);
  assert.equal(registry.changed,1);assert.equal(registry.clipped,0);
  assert.equal(registry.select(scene,camera,Family.NEAR),1);
  assert.ok(registry.fresh.length<=SceneCapacity.NODES);
});

test('cold admission beyond a map grows retained rows before selection',()=>{
  const {registry,core,scene,camera}=fixture();
  // The store's first allocation is deliberately smaller than the maximum.
  // Crossing it must refresh pointers even when WASM keeps the same heap pages.
  registry.select(scene,camera,0);
  for(let i=0;i<16000;i++)scene.add(new THREE.Object3D());
  assert.equal(registry.capacity,16384);
  assert.equal(core.sceneChanges.length,registry.capacity*SceneNode.STRIDE);
  assert.equal(registry.changed,16000);assert.equal(registry.clipped,0);
  assert.doesNotThrow(()=>registry.select(scene,camera,0));
  assert.equal(registry.shown(scene.children.at(-1)),true);
});

test('reparenting an admitted detached subtree publishes native membership',()=>{
  const {registry,scene,camera}=fixture(),detached=new THREE.Group(),child=new THREE.Object3D();
  detached.add(child);registry.admit(detached);
  registry.select(scene,camera,0);assert.equal(registry.shown(child),false);
  scene.add(detached);registry.select(scene,camera,0);
  assert.equal(registry.shown(child),true);assert.equal(registry.admitIfNew(child).main,true);
  sceneProperty(detached,"visible",false);registry.select(scene,camera,0);assert.equal(registry.shown(child),false);
  sceneProperty(detached,"visible",true);scene.remove(detached);registry.select(scene,camera,0);
  assert.equal(registry.shown(child),false);
});

test('material group changes publish sparse rows and native clipped ranges',()=>{
  const {registry,core,scene,camera}=fixture(),geometry=new THREE.BoxGeometry();
  geometry.clearGroups();geometry.addGroup(0,6,0);geometry.addGroup(6,6,1);geometry.setDrawRange(3,7);
  const materials=[new THREE.Material(),new THREE.Material()],mesh=new THREE.Mesh(geometry,materials);
  mesh.castShadow=true;mesh.frustumCulled=false;materials[1].visible=false;scene.add(mesh);
  assert.equal(registry.select(scene,camera,Family.SHADOW,Mode.SHADOW),1);
  assert.equal(registry.groupChanged,2);
  assert.deepEqual([...core.sceneOutput.subarray(SceneSelection.FIRST,SceneSelection.COUNT+1)],[3,3]);
  registry.select(scene,camera,Family.SHADOW,Mode.SHADOW);assert.equal(registry.groupChanged,0);
  sceneProperty(materials[1],"visible",true);
  assert.equal(registry.select(scene,camera,Family.SHADOW,Mode.SHADOW),2);
  assert.equal(registry.groupChanged,2);
  geometry.clearGroups();geometry.addGroup(4,2,1);
  assert.equal(registry.select(scene,camera,Family.SHADOW,Mode.SHADOW),1);
  assert.deepEqual([...core.sceneOutput.subarray(SceneSelection.FIRST,SceneSelection.COUNT+1)],[4,2]);
});

test('native pose publication skips transform-only bones and retains descriptor facts',()=>{
  const {registry,core,scene,camera}=fixture(),bone=new THREE.Bone(),mesh=new THREE.Mesh(new THREE.BoxGeometry(),new THREE.Material());
  mesh.frustumCulled=false;bone.add(mesh);scene.add(bone);
  registry.select(scene,camera,Family.NEAR);
  const world=new Float64Array(mesh.matrixWorld.elements);world[12]=2;
  publishSceneWorld(bone,world,0);
  assert.equal(registry.changed,0);
  publishSceneWorld(mesh,world,0);
  assert.equal(registry.changed,0);assert.equal(registry.worldChanged,1);
  const node=registry.admitIfNew(mesh);
  assert.equal(node.propertiesDirty,false);
  registry.select(scene,camera,Family.NEAR);
  assert.equal(core.sceneOutput[SceneSelection.SLOT],node.slot);
  assert.equal(core.sceneWorldChanges[SceneWorldChange.WORLD+12],2);
  publishSceneWorld(mesh,world,0);assert.equal(registry.changed,0);
  world[13]=-0;publishSceneWorld(mesh,world,0);
  assert.equal(registry.worldChanged,1);assert.equal(registry.changed,0);registry.select(scene,camera,Family.NEAR);
  assert.ok(Object.is(core.sceneWorldChanges[SceneWorldChange.WORLD+13],-0));
  // A later authored change must survive coalescing with a pose-only row.
  world[12]=3;publishSceneWorld(mesh,world,0);sceneProperty(mesh,"visible",false);
  assert.equal(registry.changed,1);assert.equal(node.propertiesDirty,true);
  assert.equal(registry.select(scene,camera,Family.NEAR),0);
  sceneProperty(bone,"visible",false);sceneProperty(mesh,"visible",true);
  assert.equal(registry.select(scene,camera,Family.NEAR),0);
});

test('stationary matrix dirty flags do not republish static descriptor rows',()=>{
  const {registry,core,scene,camera}=fixture(),mesh=new THREE.Mesh(new THREE.BoxGeometry(),new THREE.Material());
  mesh.frustumCulled=false;scene.add(mesh);mesh.updateMatrixWorld();registry.select(scene,camera,Family.NEAR);
  const flush=registry.flush;let published=0,flushes=0;
  registry.flush=function(){flushes++;published=flush.call(this);return published;};
  for(let i=0;i<100;i++){
    mesh.updateMatrixWorld();
    assert.equal(registry.admitIfNew(mesh).propertiesDirty,false);
    assert.equal(registry.select(scene,camera,Family.NEAR),1);assert.equal(published,0);assert.equal(flushes,0);
  }
  sceneTransform(mesh.position.set(3,0,0));mesh.updateMatrixWorld();
  assert.equal(registry.select(scene,camera,Family.NEAR),1);assert.equal(published,0);assert.equal(registry.lastWorldChanged,1);assert.equal(flushes,0);
  assert.equal(core.sceneWorldChanges[SceneWorldChange.WORLD+12],3);
  sceneProperty(mesh,"visible",false);mesh.updateMatrixWorld();
  assert.equal(registry.select(scene,camera,Family.NEAR),0);assert.equal(published,1);assert.equal(registry.lastWorldChanged,0);assert.equal(flushes,1);
});

test('native bone readbacks retain authored state and child lifecycle without accessors',()=>{
  const {registry,scene,camera}=fixture(),bone=new THREE.Bone(),mesh=new THREE.Mesh(new THREE.BoxGeometry(),new THREE.Material());
  bone.visible=false;bone.add(mesh);mesh.frustumCulled=false;
  scene.add(bone);assert.equal(registry.select(scene,camera,Family.NEAR),0);
  assert.equal(Object.getOwnPropertyDescriptor(bone,'visible')?.get,undefined);
  assert.equal(Object.getOwnPropertyDescriptor(bone,'matrixWorldNeedsUpdate')?.get,undefined);
  // Authored hidden bones still prune draws; reparenting an attached mesh
  // publishes the topology mutation without changing that immutable policy.
  scene.add(mesh);assert.equal(registry.select(scene,camera,Family.NEAR),1);
  bone.add(mesh);assert.equal(registry.select(scene,camera,Family.NEAR),0);
});

test('steady scene preparation keeps resource admission cold and view preparation live',()=>{
  const {registry,core,renderer,scene,camera}=fixture();let admissions=0,views=0;
  const listed=core.byName.effects.listed;
  core.byName.effects.listed=function(...args){admissions++;return listed.apply(this,args);};
  const before=core.byName.lines.beforeSort;
  core.byName.lines.beforeSort=function(...args){views++;return before.apply(this,args);};
  core.prepareScene(renderer,scene,camera,null);
  const cold=admissions;assert.ok(cold>0);
  core.prepareScene(renderer,scene,camera,null);assert.equal(admissions,cold);assert.equal(views,2);
  scene.add(new THREE.Group());core.prepareScene(renderer,scene,camera,null);
  assert.equal(admissions,cold+1);assert.equal(views,3);
  assert.equal(registry.fresh.length,0);
});

test('dirty matrix subtrees solve once between writes and preserve hidden placement',()=>{
  const {registry,scene}=fixture(),parent=new THREE.Group(),child=new THREE.Object3D();
  parent.add(child);scene.add(parent);
  sceneTransform(parent.position.set(2,0,0));sceneTransform(child.position.set(3,0,0));
  scene.updateMatrixWorld();assert.equal(child.matrixWorld.elements[12],5);
  const updates=registry.worlds.updates,visits=registry.worlds.visits;
  for(let i=0;i<100;i++){scene.updateMatrixWorld();child.getWorldPosition(new THREE.Vector3());}
  assert.equal(registry.worlds.updates,updates);assert.equal(registry.worlds.visits,visits);
  sceneProperty(parent,'visible',false);sceneTransform(child.position.set(7,0,0));
  scene.updateMatrixWorld();assert.equal(child.matrixWorld.elements[12],5);
  sceneProperty(parent,'visible',true);scene.updateMatrixWorld();
  assert.equal(child.matrixWorld.elements[12],9);
  // Matrix/visibility reads retain plain properties on every hot shell.
  for(const object of [parent,child])for(const key of ['visible','matrixWorldNeedsUpdate','renderOrder','frustumCulled'])
    assert.equal(Object.getOwnPropertyDescriptor(object,key)?.get,undefined);
});

test('parent-only world getters leave unrelated dirty subtrees deferred',()=>{
  const {registry,scene}=fixture(),first=new THREE.Group(),second=new THREE.Group();
  const a=new THREE.Object3D(),b=new THREE.Object3D();first.add(a);second.add(b);scene.add(first,second);
  scene.updateMatrixWorld();const updates=registry.worlds.updates;
  sceneTransform(b.position.set(4,0,0));a.getWorldPosition(new THREE.Vector3());
  assert.equal(registry.worlds.updates,updates);assert.equal(b.matrixWorld.elements[12],0);
  scene.updateMatrixWorld();assert.equal(b.matrixWorld.elements[12],4);
  assert.equal(registry.worlds.updates,updates+1);
});

test('named world getters retain a native roots lazy placement boundary',()=>{
  const {scene}=fixture(),root=new THREE.Object3D(),child=new THREE.Object3D();root.add(child);
  let stale=true,placed=0;
  const place=()=>{if(!stale)return;stale=false;placed++;root.matrixWorld.makeTranslation(7,0,0);publishSceneWorld(root,root.matrixWorld.elements,0);};
  root.updateMatrixWorld=root.updateWorldMatrix=place;root.matrixAutoUpdate=root.matrixWorldAutoUpdate=false;
  scene.add(root);child.getWorldPosition(new THREE.Vector3());
  assert.equal(placed,1);assert.equal(child.matrixWorld.elements[12],7);
  child.getWorldPosition(new THREE.Vector3());assert.equal(placed,1);
});

test('a parent getter defers children while preserving their pending world version',()=>{
  const {scene}=fixture(),parent=new THREE.Group(),first=new THREE.Object3D(),second=new THREE.Object3D();
  parent.add(first,second);scene.add(parent);scene.updateMatrixWorld();
  sceneTransform(parent.position.set(6,0,0));parent.getWorldPosition(new THREE.Vector3());
  assert.equal(parent.matrixWorld.elements[12],6);assert.equal(first.matrixWorld.elements[12],0);
  scene.updateMatrixWorld();assert.equal(first.matrixWorld.elements[12],6);assert.equal(second.matrixWorld.elements[12],6);
});

test('dirty child words retain siblings across word boundaries and cold reparenting',()=>{
  const {scene,registry}=fixture(),parent=new THREE.Group(),other=new THREE.Group();scene.add(parent,other);
  const children=Array.from({length:97},()=>new THREE.Object3D());parent.add(...children);scene.updateMatrixWorld();
  const updates=registry.worlds.updates;
  for(const index of [0,31,32,63,64,96])sceneTransform(children[index].position.set(index+1,0,0));
  scene.updateMatrixWorld();assert.equal(registry.worlds.updates,updates+6);
  for(const index of [0,31,32,63,64,96])assert.equal(children[index].matrixWorld.elements[12],index+1);
  parent.remove(children[1]);other.add(children[31]);
  sceneTransform(children[32].position.set(300,0,0));scene.updateMatrixWorld();
  assert.equal(children[32].matrixWorld.elements[12],300);assert.equal(children[31].parent,other);
});

// Selection reuses published banks only when no writer or camera bit changed.
// A native pose travels through its own compact, generation-qualified block.
test('unchanged queries skip publication and selection without hiding real writes',()=>{
  const {registry,core,scene,camera}=fixture(),mesh=new THREE.Mesh(new THREE.BoxGeometry(),new THREE.Material());
  mesh.frustumCulled=false;scene.add(mesh);registry.select(scene,camera,Family.NEAR);
  const select=core.rust.scene_select;let calls=0,flushes=0;
  core.rust.scene_select=(...args)=>{calls++;return select(...args);};
  const flush=registry.flush;registry.flush=function(){flushes++;return flush.call(this);};
  for(let i=0;i<20;i++)assert.equal(registry.select(scene,camera,Family.NEAR),1);
  assert.equal(calls,0);assert.equal(flushes,0);
  const world=new Float64Array(mesh.matrixWorld.elements);world[12]=2;publishSceneWorld(mesh,world,0);
  assert.equal(registry.select(scene,camera,Family.NEAR),1);assert.equal(calls,1);assert.equal(flushes,0);
  assert.equal(registry.lastWorldChanged,1);assert.equal(core.sceneWorldChanges[SceneWorldChange.WORLD+12],2);
  camera.projectionMatrix.elements[0]=2;registry.select(scene,camera,Family.NEAR);assert.equal(calls,2);
  sceneProperty(mesh,'visible',false);assert.equal(registry.select(scene,camera,Family.NEAR),0);
  assert.equal(calls,3);assert.equal(flushes,1);
});

// A descriptor published later in a preparation boundary owns its final pose.
// Cold reserve may relocate the pending pose bank without discarding its rows.
test('compact poses survive growth and later authored writes use the final matrix',()=>{
  const {registry,core,scene,camera}=fixture(),mesh=new THREE.Mesh(new THREE.BoxGeometry(),new THREE.Material());
  mesh.frustumCulled=false;scene.add(mesh);registry.select(scene,camera,Family.NEAR);
  const world=new Float64Array(mesh.matrixWorld.elements);world[12]=2;publishSceneWorld(mesh,world,0);
  core.rust.scene_reserve(16384);core.buffer=null;core.views();
  assert.equal(registry.select(scene,camera,Family.NEAR),1);
  const key=new Float64Array(6);registry.key(mesh,key);assert.equal(key[2],2);
  world[12]=3;publishSceneWorld(mesh,world,0);sceneProperty(mesh,'renderOrder',7);
  world[12]=4;publishSceneWorld(mesh,world,0);
  assert.equal(registry.select(scene,camera,Family.NEAR),1);registry.key(mesh,key);
  assert.equal(key[2],4);assert.equal(core.sceneOutput[SceneSelection.RENDER_ORDER],7);
});

// Ownership validation can run in multiple views without a scene mutation.
// Changing the subtree claim must still publish its native pruning flag.
test('repeated ownership claims retain clean rows while subtree changes publish',()=>{
  const {registry,core,scene,camera}=fixture(),mesh=new THREE.Mesh(new THREE.BoxGeometry(),new THREE.Material());
  mesh.frustumCulled=false;scene.add(mesh);registry.select(scene,camera,Family.NEAR);
  registry.claim(mesh);assert.equal(registry.select(scene,camera,Family.NEAR),0);
  for(let i=0;i<100;i++)registry.claim(mesh);
  assert.equal(registry.changed,0);
  registry.claim(mesh,true);assert.equal(registry.changed,1);registry.select(scene,camera,Family.NEAR);
  assert.ok(core.sceneChanges[SceneNode.FLAGS]&SceneFlags.CLAIMED_TREE);
  registry.claim(mesh,true);assert.equal(registry.changed,0);
  registry.claim(mesh,false);assert.equal(registry.changed,1);registry.select(scene,camera,Family.NEAR);
  assert.equal(core.sceneChanges[SceneNode.FLAGS]&SceneFlags.CLAIMED_TREE,0);
});

// Sea contributes no ordinary water draw row, but its camera facts remain the
// culling input consumed by WaterPass.shown before the transparent packet.
test('sea preparation preserves culling facts for subsequent water consumers',()=>{
  const {registry,scene,camera}=fixture(),mesh=new THREE.Mesh(new THREE.BoxGeometry(),new THREE.Material());
  scene.add(mesh);registry.bind(mesh,Family.WATER,{});scene.updateMatrixWorld();
  assert.equal(registry.select(scene,camera,Family.SEA,Mode.FAR),0);
  assert.equal(registry.shown(mesh,true),true);
  sceneTransform(mesh.position.set(10,0,0));scene.updateMatrixWorld();
  assert.equal(registry.select(scene,camera,Family.SEA,Mode.FAR),0);
  assert.equal(registry.shown(mesh,true),false);
});

test('native actor anchors retain visibility and bounds without mesh resources',()=>{
  const {registry,core,scene,camera}=fixture(),actor=new THREE.Object3D();
  actor.userData.actorAsset={};actor.material=[{actorAppearance:true}];
  actor.boundingSphere=new THREE.Sphere(new THREE.Vector3(0,0,-2),.25);
  actor.castShadow=true;scene.add(actor);registry.bind(actor,Family.INSTANCE,{handle:7},7);
  const node=registry.admitIfNew(actor);
  assert.equal(actor.geometry,undefined);assert.equal(actor.isMesh,undefined);
  assert.equal(registry.select(scene,camera,Family.INSTANCE,Mode.INSTANCE),1);
  assert.equal(core.sceneOutput[SceneSelection.SLOT],node.slot);
  assert.equal(core.sceneOutput[SceneSelection.RESOURCE],7);
  assert.equal(node.groupCount,0);
  assert.equal(registry.select(scene,camera,Family.SHADOW,Mode.SHADOW),1);
  sceneProperty(actor,'visible',false);
  assert.equal(registry.select(scene,camera,Family.INSTANCE,Mode.INSTANCE),0);
  sceneProperty(actor,'visible',true);scene.remove(actor);
  assert.equal(registry.select(scene,camera,Family.INSTANCE,Mode.INSTANCE),0);
});

test('native effect and line anchors publish authored keys without geometry shells',()=>{
  const {registry,core,scene,camera}=fixture(),group=new THREE.Group(),effect=new THREE.Object3D(),line=new THREE.Object3D();
  group.renderOrder=6;group.add(effect,line);scene.add(group);
  effect.userData.effectModel={center:[1,2,-3]};effect.frustumCulled=false;
  line.userData.lineSource={count:2};line.frustumCulled=false;line.renderOrder=9;
  registry.bind(effect,Family.EFFECT,{});registry.bind(line,Family.LINE,{},11);
  assert.equal(registry.select(scene,camera,Family.LINE,Mode.VIEW),1);
  assert.equal(core.sceneOutput[SceneSelection.RESOURCE],11);
  assert.equal(core.sceneOutput[SceneSelection.GROUP_ORDER],6);
  assert.equal(core.sceneOutput[SceneSelection.RENDER_ORDER],9);
  const key=new Float64Array(6);registry.key(effect,key);
  assert.deepEqual([...key],[6,0,1,2,-3,effect.id]);
  assert.equal(effect.geometry,undefined);assert.equal(line.material,undefined);
  sceneProperty(group,'visible',false);
  assert.equal(registry.select(scene,camera,Family.LINE,Mode.VIEW),0);
  assert.equal(registry.shown(effect),false);
});
