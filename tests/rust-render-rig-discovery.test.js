// Rigs are admitted from native source/appearance records. These checks enter
// the actual colour phase; frozen native fixtures cover removed shell readers.
import './fixtures/native-render-descriptors.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {actorAsset,actorNode,actorLook} from '../src/actor-resources.js';
import {standinModule,standinThree} from './support/rust-standin.mjs';
globalThis.location??={search:''};
const {RustRender}=await import('../src/rust-render/index.js');
function fixture(){const {BfeRenderer,memory,calls}=standinModule({lengths:{input:200+256*112,bones:8192*16},layout:{record_stride:112}}),host=standinThree(calls),core=new RustRender(host.graphics,{memory},BfeRenderer);return {core,calls,...host};}
const asset=actorAsset({kind:'plane'}),look=actorLook('dynamic');
function part(){const node=actorNode(asset,look);node.position.z=-5;return node;}
function draw(f){f.scene.updateMatrixWorld(true);f.scene.onBeforeRender(f.renderer,f.scene,f.camera,f.core.graphics.sceneTarget);f.core.publishView('near',f.renderer,f.scene,f.camera);return f.calls.filter(c=>c[0]==='frame').at(-1)[1];}
test('colour admission excludes shadow-only rigs and discovers a later ordinary rig',()=>{
 const f=fixture(),caster=part();caster.userData.shadowOnly=true;f.scene.add(caster);assert.equal(draw(f),0);assert.equal(caster.parent,f.scene);
 const body=part();f.scene.add(body);assert.equal(draw(f),1);assert.equal(body.geometry,undefined);assert.equal(body.material.isMaterial,undefined);
 body.visible=false;assert.equal(draw(f),0);body.visible=true;assert.equal(draw(f),1);
 body.material=actorLook('unlit');assert.equal(draw(f),1);assert.equal(f.calls.filter(c=>c[0]==='actor_material').length,2);
});
test('a crowded colour phase clips its later actors and keeps subsequent frames live',()=>{
 const f=fixture();for(let i=0;i<270;i++)f.scene.add(part());assert.equal(draw(f),256);assert.equal(f.core.failure,undefined);assert.ok(f.core.errors.some(v=>/capacity/.test(v)));
 while(f.scene.children.length>1)f.scene.remove(f.scene.children.at(-1));assert.equal(draw(f),1);assert.equal(f.core.failure,undefined);
});
