// Tow cables and flight helpers are real battle drawables, including helpers
// whose geometry starts empty. These tests inspect retained Rust registrations
// and updates without claiming to rasterize. The backend smoke checks pixels.
// Transparent keys must match the bundled renderer, whose Vector4 clip-depth
// ordering intentionally differs from ordinary perspective-divided NDC.
// A replaced BufferGeometry must not allocate another Rust geometry every frame.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { LINE as V, LIMIT as L } from "../src/rust-render/frame-layout.js";
import {RustRender} from '../src/rust-render/index.js';
import {standinModule,standinThree,fakeProgram} from './support/rust-standin.mjs';
globalThis.location ??= {search:""};

function fixture(){
  const {BfeRenderer,memory,calls}=standinModule(),three=standinThree(calls);
  const core=new RustRender(three.graphics,{memory},BfeRenderer);
  const pass=core.byName.lines;
  const discover=objects=>pass.beforeSort(three.renderer,three.scene,three.camera,three.graphics.sceneTarget,objects);
  const line=(kind=THREE.Line,empty=false)=>{
    const geometry=new THREE.BufferGeometry();
    if(!empty)geometry.setAttribute('position',new THREE.Float32BufferAttribute([0,0,-1,1,0,-1,1,1,-1],3));
    const object=new kind(geometry,new THREE.LineBasicMaterial());object.frustumCulled=false;
    three.renderer.properties.get(object.material).currentProgram=fakeProgram([],['position']);three.scene.add(object);return object;
  };
  return {core,calls,pass,discover,line,...three};
}
// Update both a reused position array and a replaced helper geometry.
// Neither path may recreate the retained geometry or change its topology.
test('tow strips, helper segments and loops retain topology and upload dynamic positions without replacing the Rust geometry',()=>{
  const f=fixture(),objects=[f.line(),f.line(THREE.LineSegments),f.line(THREE.LineLoop)];f.discover(objects);
  assert.deepEqual(f.calls.filter(c=>c[0]==='geometry_primitive').map(c=>c[2]),[3,1,2]);
  const first=f.pass.records.get(objects[0]).geometry;f.calls.length=0;
  objects[0].geometry.attributes.position.array[0]=2;objects[0].geometry.attributes.position.needsUpdate=true;
  objects[1].geometry=new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(),new THREE.Vector3(0,1,0)]);
  f.discover([]);
  assert.equal(f.pass.records.get(objects[0]).geometry,first);
  assert.equal(f.calls.filter(c=>c[0]==='geometry_buffer_data').length,2);
  assert.equal(f.calls.filter(c=>c[0]==='add_geometry').length,0);
  f.pass.prepare({renderer:f.renderer,scene:f.scene,camera:f.camera},2);
  assert.ok(f.calls.some(c=>c[0]==='frame_lines_publish_frame'&&c[1]===2&&c[2]===3));
  assert.equal(f.core.frameLines[2*L.LINES*V.LEN+V.STATE+9],3);
});
// Claiming an empty helper must not lose its later data or ignore visibility.
// The ancestor check matches projectObject rather than only the leaf flag.
test('an initially empty flight helper becomes drawable when its positions arrive; hidden ancestors suppress it',()=>{
  const f=fixture(),object=f.line(THREE.LineSegments,true);f.discover([object]);assert.equal(f.pass.owns(object),true);assert.equal(f.pass.opaque.length,0);
  object.geometry.setAttribute('position',new THREE.Float32BufferAttribute([0,0,-1,1,0,-1],3));f.discover([]);assert.equal(f.pass.opaque.length,1);
  const group=new THREE.Group();f.scene.add(group);group.add(object);group.visible=false;f.discover([]);assert.equal(f.pass.opaque.length,0);
});
// Group/render ordering precedes the depth key for every blended source.
// Keeping material visibility proves ownership did not depend on hiding a draw.
test('transparent lines join the blended command stream at the authored group and render order',()=>{
  const f=fixture(),object=f.line();object.material.transparent=true;object.renderOrder=17;
  const group=new THREE.Group();group.renderOrder=5;group.add(object);f.scene.add(group);f.discover([group]);
  assert.equal(f.pass.opaque.length,0);assert.equal(f.pass.transparent.length,1);
  assert.deepEqual([...f.pass.transparent[0].key].slice(0,2),[5,17]);
  assert.equal(object.material.visible,true);
  f.pass.prepare({renderer:f.renderer,scene:f.scene,camera:f.camera},2);
  const at=2*L.LINES*V.LEN;
  assert.equal(f.core.frameLines[at+V.TRANSPARENT],1);
  assert.deepEqual([...f.core.frameLines.subarray(at+V.KEY,at+V.KEY+2)],[5,17]);
  assert.ok(f.calls.some(c=>c[0]==='frame_lines_publish_frame'&&c[2]===1));
});

// The bundled renderer changed this contract from conventional NDC sorting.
// A non-unit W makes an accidental perspective division observable here.
test('line depth keys use the bundled renderer clip-space sphere centre without division',()=>{
  const f=fixture(),object=f.line();object.material.transparent=true;object.position.z=-8;f.discover([object]);
  // WebGLRenderer.projectObject uses Vector4 for mesh and line bounds.
  const center=new THREE.Vector4().copy(object.geometry.boundingSphere.center).applyMatrix4(object.matrixWorld).applyMatrix4(f.pass.screen);
  assert.equal(f.pass.transparent[0].key[2],center.z);
  assert.notEqual(f.pass.transparent[0].key[2],center.z/center.w);
});
