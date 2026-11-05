import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {standinModule,standinThree} from './support/rust-standin.mjs';
globalThis.location ??= {search:''};
const {RustRender}=await import('../src/rust-render/index.js');

test('a lazy image keeps its Rust texture id when its transparent sample is replaced', () => {
 const {BfeRenderer,memory,calls}=standinModule({methods:{replace_sample_texture(id,handle){calls.push(['replace_sample_texture',id,handle]);}}});
 const {renderer,graphics}=standinThree(calls),image=new THREE.Texture();
 const props=renderer.properties.get(image);delete props.__webglTexture;
 let ready=false;
 renderer.initTexture=texture=>{if(texture===image&&!ready)return;const p=renderer.properties.get(texture);p.__webglTexture??={};p.__version=texture.version;};
 const core=new RustRender(graphics,{memory},BfeRenderer),id=core.texture(image);
 assert.ok(id>=0);assert.equal(core.texture(image),id);assert.equal(core.pendingTextures.length,1);
 const pixel=core.textureSources[id],oldHandle=renderer.properties.get(pixel).__webglTexture;
 assert.deepEqual([...pixel.image.data],[0,0,0,0]);
 image.image={width:2,height:2};image.needsUpdate=true;ready=true;
 core.refreshTextures();
 assert.equal(core.texture(image),id);assert.equal(core.textureSources[id],image);assert.equal(core.pendingTextures.length,0);
 assert.deepEqual(calls.find(c=>c[0]==='replace_sample_texture'),['replace_sample_texture',id,props.__webglTexture]);
 assert.equal(core.textures.has(oldHandle),false);
});
