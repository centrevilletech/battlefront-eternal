// Native texture ownership can change handles after asynchronous decode or a
// sampler revision. Rust records refer to logical ids and must observe those
// handle changes without rebuilding every mesh. Shared image data does not
// imply shared logical ownership: cloned textures may later diverge.
// The memory-growth fixtures check another resource boundary hazard: registering
// a sampler can detach already acquired WASM views. Register before writing
// frame/HUD/post records, then reacquire views so their values reach Rust.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {standinModule,standinThree} from './support/rust-standin.mjs';
globalThis.location ??= {search:''};
const {RustRender}=await import('../src/rust-render/index.js');

// A retained draw can outlive image decode. Its sampler id must not move when
// the transparent placeholder is replaced by the texture owner's real handle.
// Checking only the texture registry size would miss a stale bound sampler.
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

// Grow actual WebAssembly.Memory inside add_texture, detaching old views.
// The HUD group and index count prove the fresh view received its record.
// This is the failure that can otherwise leave a drawn world with an empty HUD.
test('registering a new HUD sampler cannot detach the frame inputs being written', async()=>{
 const {Batch}=await import('../src/interface/batch.js');
 const {BfeRenderer,memory,calls}=standinModule(),{renderer,graphics}=standinThree(calls);
 const backing=new WebAssembly.Memory({initial:128});memory.buffer=backing.buffer;
 const core=new RustRender(graphics,{memory},BfeRenderer),batch=new Batch(),texture=new THREE.Texture();
 batch.begin();batch.quad(new THREE.Matrix4(),0,0,10,10,texture,[0,0,1,1],[1,1,1,1]);batch.end();
 const add=core.rust.add_texture.bind(core.rust);let grew=false;
 core.rust.add_texture=(...args)=>{const id=add(...args);if(!grew){backing.grow(1);memory.buffer=backing.buffer;grew=true;}return id;};
 core.coverHud(renderer,{batch,scene:new THREE.Scene(),camera:new THREE.PerspectiveCamera()});
 core.views();
 assert.ok(grew);
 assert.equal(core.hudInput[0],1,'one group reaches Rust after resource registration grows memory');
 assert.equal(core.hudInput[46],6,'its six indices survive the refreshed shared view');
});

// The same hazard applies to a source-owned post step, not just HUD batches.
// A valid step must retain its dimensions/uniform count after sampler growth.
// The material itself deliberately carries no native shader source.
test('a post sampler is registered before its shared step record is written',async()=>{
 const {ownedFullScreenPass}=await import('../src/render-passes.js');
 const {BfeRenderer,memory,calls}=standinModule(),{renderer,graphics}=standinThree(calls);
 const backing=new WebAssembly.Memory({initial:128});memory.buffer=backing.buffer;
 const core=new RustRender(graphics,{memory},BfeRenderer),add=core.rust.add_texture.bind(core.rust);
 core.rust.add_texture=(...args)=>{const id=add(...args);backing.grow(1);memory.buffer=backing.buffer;return id;};
 const pass=ownedFullScreenPass('post-hud/copy',{map:{value:new THREE.Texture()}});
 core.post(renderer,[{pass,target:null,camera:new THREE.OrthographicCamera(-1,1,1,-1,0,1)}]);
 core.views();assert.equal(core.postInput[2],800);assert.equal(core.postInput[4],1);
});

// A pending-image placeholder can allocate even when the requested image is
// not ready. Prepare that sampler before acquiring post-input typed views.
// A successful return with a detached view would be a silent missing draw.
test('a screen pass prepares a pending image before taking shared views',async()=>{
 const {writeScreenStep}=await import('../src/rust-render/screen.js');
 const {BfeRenderer,memory,calls}=standinModule(),{renderer,graphics}=standinThree(calls);
 const backing=new WebAssembly.Memory({initial:128});memory.buffer=backing.buffer;
 const texture=new THREE.Texture();delete renderer.properties.get(texture).__webglTexture;
 renderer.initTexture=t=>{if(t!==texture)renderer.properties.get(t).__webglTexture={};};
 const core=new RustRender(graphics,{memory},BfeRenderer),add=core.rust.add_texture.bind(core.rust);
 core.rust.add_texture=(...args)=>{const id=add(...args);backing.grow(1);memory.buffer=backing.buffer;return id;};
 const material=new THREE.ShaderMaterial({uniforms:{map:{value:texture}}});
 assert.equal(writeScreenStep(core,0,{id:0,uniforms:[{name:'map',type:32}]},material,new THREE.Mesh(),new THREE.Camera()),true);
 core.views();assert.equal(core.postInput[4],1);assert.equal(core.postInput[6],32);
});


// The resource owner may initially deduplicate two clones onto one GL handle.
// Changing one sampler can split them later, so ids belong to texture objects.
// Only the changed clone's retained binding may be replaced.
test('texture clones keep independent Rust bindings when a sampler change replaces one native handle', () => {
 const {BfeRenderer,memory,calls}=standinModule({methods:{replace_sample_texture(id,handle){calls.push(['replace_sample_texture',id,handle]);}}});
 const {renderer,graphics}=standinThree(calls),first=new THREE.Texture({width:1,height:1}),second=first.clone();
 const shared={};
 renderer.properties.get(first).__webglTexture=shared;
 renderer.properties.get(second).__webglTexture=shared;
 const core=new RustRender(graphics,{memory},BfeRenderer),a=core.texture(first),b=core.texture(second);
 assert.notEqual(a,b);
 assert.equal(core.texture(first),a);
 const replacement={};second.needsUpdate=true;
 renderer.initTexture=texture=>{const props=renderer.properties.get(texture);props.__webglTexture=replacement;props.__version=texture.version;};
 assert.equal(core.texture(second),b);
 assert.equal(core.texture(first),a);
 assert.deepEqual(calls.filter(c=>c[0]==='replace_sample_texture'),[['replace_sample_texture',b,replacement]]);
});

// A filtering revision may replace handles without changing source images.
// Refresh registered samples before submitting any phase that still uses them.
// The already retained ids and geometry must remain usable across that epoch.
test('an anisotropy revision uploads and rebinds retained static texture samples before drawing', () => {
 const {BfeRenderer,memory,calls}=standinModule({methods:{replace_sample_texture(id,handle){calls.push(['replace_sample_texture',id,handle]);}}});
 const {renderer,graphics}=standinThree(calls),texture=new THREE.Texture({width:1,height:1});
 const core=new RustRender(graphics,{memory},BfeRenderer),id=core.texture(texture),replacement={};
 texture.needsUpdate=true;graphics.textureRevision=1;
 renderer.initTexture=t=>{const p=renderer.properties.get(t);p.__webglTexture=replacement;p.__version=t.version;};
 core.phase('post',renderer,new THREE.Scene(),new THREE.Camera());
 assert.equal(core.texture(texture),id);
 assert.deepEqual(calls.find(c=>c[0]==='replace_sample_texture'),['replace_sample_texture',id,replacement]);
 assert.ok(calls.findIndex(c=>c[0]==='replace_sample_texture')<calls.findIndex(c=>c[0]==='post_frame'));
});
