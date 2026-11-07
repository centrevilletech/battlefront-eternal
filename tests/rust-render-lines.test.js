import {sceneProperty} from "../src/rust-render/scene-changes.js";
import {prepareSceneFixture} from "./support/rust-standin.mjs";
// The host publishes domain arrays; native fixtures check immutable geometry
// and raster bytes. The backend fixture verifies the resulting GPU draws.
import test from 'node:test';
import assert from 'node:assert/strict';
import { LINE as D } from '../src/rust-render/line-layout.js';
import { RustRender } from '../src/rust-render/index.js';
import { lineSource, LINE_KIND } from '../src/line-source.js';
import * as THREE from 'three';
import { standinModule,standinThree,fakeProgram } from './support/rust-standin.mjs';
globalThis.location??={search:''};
function fixture(){
 const {BfeRenderer,memory,calls}=standinModule(),three=standinThree(calls);
 three.renderer.compile=scene=>scene.traverse(object=>{three.renderer.properties.get(object.material).currentProgram=fakeProgram([],['position']);});
 const core=prepareSceneFixture(new RustRender(three.graphics,{memory},BfeRenderer)),pass=core.byName.lines;
 const discover=()=>core.prepareScene(three.renderer,three.scene,three.camera,three.graphics.sceneTarget);
 return {core,pass,calls,discover,...three};
}
test('cables and helpers admit native descriptors once and publish one bounded view call',()=>{
 const f=fixture(),cable=lineSource(LINE_KIND.TOW,32),helper=lineSource(LINE_KIND.FLIGHT,128);f.scene.add(cable,helper);
 cable.userData.lineSource.positions.set([0,0,-1,1,0,-1]);cable.userData.lineSource.count=2;
 f.discover([cable,helper]);
 assert.deepEqual(f.calls.filter(c=>c[0]==='line_source').map(c=>c.slice(2)),[[0,32],[1,128]]);
 assert.equal(cable.geometry,undefined);assert.equal(cable.material,undefined);
 f.pass.prepare({camera:f.camera},2);
 assert.deepEqual(f.calls.filter(c=>c[0]==='line_frame').map(c=>c.slice(1)),[[2]]);
 const at=D.VIEW.LEN;assert.equal(f.core.lineInput[at+D.ROW.COUNT],2);
 assert.deepEqual(Array.from(f.core.lineData.slice(0,6)),[0,0,-1,1,0,-1]);
 cable.userData.lineSource.positions[0]=2;cable.userData.lineSource.serial++;
 f.discover([]);f.pass.prepare({camera:f.camera},0);
 assert.equal(f.core.lineData[0],2);assert.equal(f.calls.filter(c=>c[0]==='line_source').length,2);
 assert.equal(f.calls.some(c=>c[0]==='geometry_buffer_data'),false);
});
test('hidden parents suppress native line facts and placements keep authored ordering',()=>{
 const f=fixture(),line=lineSource(LINE_KIND.TOW,32),group=new THREE.Group();sceneProperty(group,"renderOrder",5);sceneProperty(line,"renderOrder",17);group.add(line);f.scene.add(group);
 f.discover([group]);f.pass.prepare({camera:f.camera},2);
 const at=D.VIEW.LEN;assert.equal(f.core.lineInput[at+D.ROW.GROUP_ORDER],5);assert.equal(f.core.lineInput[at+D.ROW.RENDER_ORDER],17);
 sceneProperty(group,"visible",false);f.discover([]);f.pass.prepare({camera:f.camera},2);assert.equal(f.core.lineInput[D.VIEW.COUNT],0);
});
