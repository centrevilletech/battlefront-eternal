import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Controls } from '../src/input.js';
import { LocalLights } from '../src/local-lights.js';
import { TreeCanopies } from '../src/tree-canopies.js';
import { RenderProfiler } from '../src/render-profiler.js';
import { drawFramebuffer } from '../src/render-targets.js';

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

test('canopy selection and every uploaded leaf match the original equations',()=>{
  let enabled=true;
  const graphics={scene:new THREE.Scene(),farScene:new THREE.Scene(),settings:{get:()=>enabled?'enhanced':'original'}};
  const instances=Array.from({length:20},(_,i)=>({matrix:new THREE.Matrix4().makeTranslation((i%5-2)*25,0,-30-Math.floor(i/5)*40).toArray(),bounds:[[-4,0,-4],[4,12,4]]}));
  const trees=new TreeCanopies(graphics,{instances,texture:'leaves'},()=>new THREE.Texture());
  const camera=new THREE.PerspectiveCamera(75,1,.1,1000),frustum=new THREE.Frustum(),matrix=new THREE.Matrix4();
  const attributes=trees.mesh.geometry.attributes;
  for(let frame=0;frame<80;frame++) {
    enabled=frame%19!==0;camera.position.set(Math.sin(frame*.2)*50,6,Math.cos(frame*.1)*20);camera.rotation.y=Math.sin(frame*.13)*.9;camera.fov=frame%2?50:80;camera.updateProjectionMatrix();camera.updateMatrixWorld();
    frustum.setFromProjectionMatrix(matrix.multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse));
    const expected=enabled?trees.groups.filter(g=>frustum.intersectsSphere(g.sphere)&&g.sphere.center.distanceTo(camera.position)-g.sphere.radius<220).sort((a,b)=>a.sphere.center.distanceToSquared(camera.position)-b.sphere.center.distanceToSquared(camera.position)).slice(0,12):[];
    trees.update(camera);assert.deepEqual(trees.active,expected);assert.equal(trees.mesh.geometry.instanceCount,expected.length*912);
    for(const [name,field] of [['leafCenter','centers'],['leafSize','sizes'],['leafShade','shades']]) {
      assert.deepEqual(attributes[name].array.slice(0,expected.length*912*attributes[name].itemSize),new Float32Array(expected.flatMap(g=>g[field])));
    }
    for(const g of trees.groups)assert.equal(trees.states[g.index*4],expected.includes(g)?1:0);
  }
  enabled=true;trees.update(camera);
  const versions=Object.values(attributes).map(a=>a.version),stateVersion=trees.stateMap.version;
  // Distinct camera objects/projections are used for near and far scenes.
  const far=camera.clone();far.far=3000;far.updateProjectionMatrix();far.updateMatrixWorld();trees.update(far);
  assert.deepEqual(Object.values(attributes).map(a=>a.version),versions);assert.equal(trees.stateMap.version,stateVersion);
});

test('framebuffer lookup follows single-sample and both MSAA paths without GL queries',()=>{
  const resolved={},multisample={};const properties={__webglFramebuffer:resolved};
  const renderer={properties:{get:()=>properties}};
  assert.equal(drawFramebuffer(renderer,{samples:0}),resolved);
  assert.equal(drawFramebuffer(renderer,{samples:4}),resolved); // render-to-texture extension
  properties.__webglMultisampledFramebuffer=multisample;
  assert.equal(drawFramebuffer(renderer,{samples:4}),multisample);
  assert.equal(drawFramebuffer(renderer,{samples:0}),resolved);
});

test('GPU profiling polls sparsely, never reads unavailable results, and discards disjoint samples',()=>{
  let statusReads=0,available=false,disjoint=false,deleted=0;
  const gl={QUERY_RESULT_AVAILABLE:1,QUERY_RESULT:2,getExtension:()=>({GPU_DISJOINT_EXT:3,TIME_ELAPSED_EXT:4}),getParameter:()=>{statusReads++;return disjoint;},createQuery:()=>({}),beginQuery(){},endQuery(){},deleteQuery(){deleted++;},getQueryParameter(q,key){if(key===1)return available;assert.ok(available);return 2e6;}};
  const profiler=new RenderProfiler({getContext:()=>gl});
  for(let i=0;i<32;i++){profiler.begin();profiler.end();}
  assert.equal(statusReads,4);assert.equal(profiler.pending.length,4);assert.equal(profiler.gpu.length,0);
  available=true;profiler.begin();profiler.end();assert.equal(profiler.gpu.length,4);assert.equal(deleted,4);
  disjoint=true;for(let i=0;i<8;i++){profiler.begin();profiler.end();}
  assert.equal(profiler.pending.length,0);assert.equal(profiler.gpu.length,0);
});

test('static color culling multi-draws only visible runs, in their original order',async()=>{
  const {partitionShadowCaster}=await import('../src/shadow-casters.js');
  const {mergeGeometries}=await import('three/addons/utils/BufferGeometryUtils.js');
  // Boxes in source order hop between distant cells, so visible runs are
  // separated by hidden ones (one first-to-last range would draw them all).
  const order=Array.from({length:100},(_,i)=>(i*37)%100);
  const source=mergeGeometries(order.map(i=>new THREE.BoxGeometry(2,2,2).translate((i%10-5)*40,0,-Math.floor(i/10)*40)));
  const original=source.index.array.slice();
  const mesh=new THREE.Mesh(source,new THREE.MeshBasicMaterial());
  partitionShadowCaster(mesh,()=>{},1);mesh.updateMatrixWorld(true);
  const draws=[];
  const multi={multiDrawElementsWEBGL:(mode,counts,countsOffset,type,offsets,offsetsOffset,n)=>draws.push({counts:[...counts.slice(0,n)],offsets:[...offsets.slice(0,n)],type})};
  const info={render:{triangles:0}};
  const gl={TRIANGLES:4,UNSIGNED_INT:5125,UNSIGNED_SHORT:5123};
  const renderer={extensions:{has:name=>name==='WEBGL_multi_draw',get:()=>multi},getContext:()=>gl,info};
  const bytes=original.BYTES_PER_ELEMENT;
  const camera=new THREE.PerspectiveCamera(60,1,.5,150);
  const matrix=new THREE.Matrix4(),frustum=new THREE.Frustum(),box=new THREE.Box3(),point=new THREE.Vector3();
  let multiFrames=0;
  for(let frame=0;frame<30;frame++) {
    camera.position.set(Math.sin(frame*.4)*100,5,20);camera.rotation.y=Math.sin(frame*.2);camera.updateMatrixWorld();
    draws.length=0;
    mesh.onBeforeRender(renderer,null,camera,mesh.geometry,mesh.material,null);
    mesh.onAfterRender(renderer,null,camera,mesh.geometry,mesh.material,null);
    let selected;
    if(draws.length){
      multiFrames++;
      assert.equal(mesh.geometry.drawRange.count,0);assert.equal(draws.length,1);
      const {counts,offsets,type}=draws[0];assert.equal(type,bytes===4?5125:5123);
      selected=[];let last=-1;
      for(let k=0;k<counts.length;k++){const start=offsets[k]/bytes;assert.ok(start>last);last=start+counts[k];selected.push(...original.slice(start,start+counts[k]));}
    } else { const {start,count}=mesh.geometry.drawRange;selected=[...original.slice(start,start+count)]; }
    // Every drawn triangle is an original triangle, in original order.
    let cursor=0;
    for(let i=0;i<selected.length;i+=3) {
      while(cursor<original.length && original[cursor]!==selected[i])cursor+=3;
      assert.ok(cursor<original.length);assert.deepEqual(selected.slice(i,i+3),[...original.slice(cursor,cursor+3)]);cursor+=3;
    }
    // Every triangle touching the frustum is drawn.
    frustum.setFromProjectionMatrix(matrix.multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse).multiply(mesh.matrixWorld));
    const kept=new Set();for(let i=0;i<selected.length;i+=3)kept.add(`${selected[i]},${selected[i+1]},${selected[i+2]}`);
    for(let i=0;i<original.length;i+=3) {
      box.makeEmpty();for(let j=0;j<3;j++)box.expandByPoint(point.fromBufferAttribute(source.attributes.position,original[i+j]));
      if(frustum.intersectsBox(box))assert.ok(kept.has(`${original[i]},${original[i+1]},${original[i+2]}`));
    }
    assert.ok(selected.length<original.length);
  }
  assert.ok(multiFrames>0);
  assert.deepEqual(source.index.array,original);
});

test('static color culling preserves source indices and triangle order across views and movement',async()=>{
  const {partitionShadowCaster}=await import('../src/shadow-casters.js');
  const {mergeGeometries}=await import('three/addons/utils/BufferGeometryUtils.js');
  const source=mergeGeometries(Array.from({length:100},(_,i)=>new THREE.BoxGeometry(2,2,2).translate((i%10-5)*40,0,-Math.floor(i/10)*40)));
  const original=source.index.array.slice();
  const mesh=new THREE.Mesh(source,new THREE.MeshBasicMaterial());
  partitionShadowCaster(mesh,()=>{},1);mesh.updateMatrixWorld(true);
  // Views draw ranges of the unchanged source buffer: no copies, no uploads.
  assert.equal(mesh.geometry,source);assert.deepEqual(source.index.array,original);
  const cameras=Array.from({length:3},()=>new THREE.PerspectiveCamera(60,1,.5,150));
  const matrix=new THREE.Matrix4(),frustum=new THREE.Frustum(),box=new THREE.Box3(),point=new THREE.Vector3();
  for(let frame=0;frame<45;frame++) {
    const camera=cameras[frame%3];camera.position.set(Math.sin(frame*.4)*100,5,20);camera.rotation.y=Math.sin(frame*.2);camera.updateMatrixWorld();
    mesh.position.x=frame%5;mesh.updateMatrixWorld(true);
    mesh.onBeforeRender(null,null,camera,mesh.geometry,mesh.material,null);
    const {start,count}=mesh.geometry.drawRange;
    const selected=mesh.geometry.index.array.slice(start,start+count);
    assert.ok(count<original.length);
    // Every selected triangle is an original triangle in its original order.
    let cursor=0;
    for(let i=0;i<selected.length;i+=3) {
      while(cursor<original.length && original[cursor]!==selected[i])cursor+=3;
      assert.ok(cursor<original.length);assert.deepEqual(selected.slice(i,i+3),original.slice(cursor,cursor+3));cursor+=3;
    }
    // Every triangle whose own bounds touch the frustum must survive the
    // coarser cell rejection, including near-plane-straddling triangles.
    frustum.setFromProjectionMatrix(matrix.multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse).multiply(mesh.matrixWorld));
    const kept=new Set();for(let i=0;i<selected.length;i+=3)kept.add(`${selected[i]},${selected[i+1]},${selected[i+2]}`);
    for(let i=0;i<original.length;i+=3) {
      box.makeEmpty();for(let j=0;j<3;j++)box.expandByPoint(point.fromBufferAttribute(source.attributes.position,original[i+j]));
      if(frustum.intersectsBox(box))assert.ok(kept.has(`${original[i]},${original[i+1]},${original[i+2]}`));
    }
    mesh.geometry.index.clearUpdateRanges();
  }
  assert.deepEqual(source.index.array,original);
});
