import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Controls } from '../src/input.js';
import { LocalLights } from '../src/local-lights.js';
import { TreeCanopies } from '../src/tree-canopies.js';
import { readFileSync } from 'node:fs';
import { RustRender } from '../src/rust-render/index.js';
import { SceneCanopyHeader as H, SceneCanopySelection as S } from '../src/rust-render/scene-layout.js';
import { standinModule, standinThree } from './support/rust-standin.mjs';
import { RenderProfiler } from '../src/render-profiler.js';
globalThis.location ??= {search:''};

test('controls retain axes, simultaneous bindings, short clicks and focus cleanup',()=>{
  const oldWindow=globalThis.window,oldDocument=globalThis.document;
  globalThis.window=new EventTarget();globalThis.document=new EventTarget();
  try {
    const controls=new Controls(new EventTarget());
    controls.hold('KeyW','ArrowUp','KeyD');assert.equal(controls.axis('forward'),1);assert.equal(controls.axis('right'),1);
    controls.down.delete('KeyW');assert.equal(controls.axis('forward'),1);
    controls.hold('KeyS');assert.equal(controls.axis('forward'),0);
    for(const [code,action] of [['Space','jump'],['ControlRight','crouch'],['KeyX','prone'],['Mouse0','firePrimary'],['Mouse1','zoom'],['Mouse2','fireSecondary'],['WheelUp','nextPrimary'],['WheelDown','previousPrimary']]) {
      controls.press(code);assert.equal(controls.take(action),true);assert.equal(controls.take(action),false);
    }
    controls.hold('Mouse0');assert.equal(controls.held('firePrimary'),true);
    controls.press('Space');window.dispatchEvent(new Event('blur'));
    assert.equal(controls.axis('forward'),0);assert.equal(controls.held('firePrimary'),false);assert.equal(controls.take('jump'),false);
  } finally {globalThis.window=oldWindow;globalThis.document=oldDocument;}
});

test('lamp selection matches full stable sort and occlusion before and after movement',()=>{
  const lights=new LocalLights();let seed=927;
  const rand=()=>((seed=Math.imul(seed,1664525)+1013904223>>>0)/4294967296);
  for(let i=0;i<250;i++)lights.add([rand()*30-15,rand()*5,rand()*30-15],[1,.5,.2],3+rand()*20);
  const object=new THREE.Object3D();
  for(let frame=0;frame<100;frame++) {
    object.position.set(rand()*20-10,1,rand()*20-10);object.updateMatrixWorld();lights.begin(true);lights.now=frame;
    const expected=lights.sources.map(source=>({source,distance:source.position.distanceTo(object.position)})).filter(x=>x.distance<x.source.radius).sort((a,b)=>a.distance/a.source.radius-b.distance/b.source.radius).slice(0,4);
    // Every second candidate is occluded; rejected candidates must not be
    // replaced by the fifth-nearest source, preserving the existing lighting.
    let calls=0;lights.collision={cast:()=>++calls%2===0};
    calls=0;const oracle=expected.filter(x=>x.distance<.4||++calls%2!==0).map(x=>x.source);
    calls=0;lights.select(object);
    assert.deepEqual(lights.cache.get(object).lights,oracle);
  }
});

test('canopy selection and every uploaded leaf match frozen view histories',async()=>{
  let enabled=true;
  const { BfeRenderer, memory, calls } = standinModule();
  const { graphics } = standinThree(calls);
  graphics.settings = {get:()=>enabled?'enhanced':'original'};
  const core = new RustRender(graphics, {memory}, BfeRenderer);
  graphics.rust = core; graphics.rustReady = Promise.resolve(core);
  core.resources.gpu.canopy_state = () => 1;
  core.resources.gpu.update_canopy_state = () => {};
  const frozen = JSON.parse(readFileSync(new URL('./fixtures/canopy-selection.json',import.meta.url)));
  const instances=Array.from({length:20},(_,i)=>({matrix:new THREE.Matrix4().makeTranslation((i%5-2)*25,0,-30-Math.floor(i/5)*40).toArray(),bounds:[[-4,0,-4],[4,12,4]]}));
  const trees=new TreeCanopies(graphics,{instances,texture:'leaves'},()=>new THREE.Texture());
  await graphics.rustReady;
  const camera=new THREE.PerspectiveCamera(75,1,.1,1000);

  for(let frame=0;frame<80;frame++) {
    enabled=frame%19!==0;camera.position.set(Math.sin(frame*.2)*50,6,Math.cos(frame*.1)*20);camera.rotation.y=Math.sin(frame*.13)*.9;camera.fov=frame%2?50:80;camera.updateProjectionMatrix();camera.updateMatrixWorld();
    const expected = frozen.frames[frame].ids.map(id=>trees.groups[id]);
    trees.update(camera);
    const selected = Array.from({length:core.sceneCanopyOutput[H.COUNT]},(_,i)=>core.sceneCanopyOutput[H.STRIDE+i*S.STRIDE+S.ID]);
    assert.deepEqual(selected,frozen.frames[frame].ids,`selected sources at frame ${frame}`);
    assert.equal(trees.stateVersion,frozen.frames[frame].version,`state publication at frame ${frame}`);
    assert.equal(trees.instanceCount,expected.length*912);
    for(const [field,size] of [['centers',3],['sizes',2],['shades',3]]) {
      assert.deepEqual(trees[field].slice(0,expected.length*912*size),new Float32Array(expected.flatMap(g=>g[field])));
    }
    for(const g of trees.groups)assert.equal(trees.states[g.index*4],expected.includes(g)?1:0);
  }
  enabled=true;trees.update(camera);
  const stateVersion=trees.stateVersion;
  // Distinct camera objects/projections are used for near and far scenes.
  const far=camera.clone();far.far=3000;far.updateProjectionMatrix();far.updateMatrixWorld();trees.update(far);
  assert.equal(trees.stateVersion,stateVersion);
});

test('GPU profiling polls sparsely, never reads unavailable results, and discards disjoint samples',()=>{
  let statusReads=0,available=false,disjoint=false,deleted=0;
  const gl={QUERY_RESULT_AVAILABLE:1,QUERY_RESULT:2,getExtension:()=>({GPU_DISJOINT_EXT:3,TIME_ELAPSED_EXT:4}),getParameter:()=>{statusReads++;return disjoint;},createQuery:()=>({}),beginQuery(){},endQuery(){},deleteQuery(){deleted++;},getQueryParameter(q,key){if(key===1)return available;assert.ok(available);return 2e6;}};
  const profiler=new RenderProfiler({getContext:()=>gl,domElement:new EventTarget()});
  for(let i=0;i<32;i++){profiler.begin();profiler.end();}
  assert.equal(statusReads,4);assert.equal(profiler.pending.length,4);assert.equal(profiler.gpu.length,0);
  available=true;profiler.begin();profiler.end();assert.equal(profiler.gpu.length,4);assert.equal(deleted,4);
  disjoint=true;for(let i=0;i<8;i++){profiler.begin();profiler.end();}
  assert.equal(profiler.pending.length,0);assert.equal(profiler.gpu.length,0);
});
