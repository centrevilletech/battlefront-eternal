import { presentationModule } from "./helpers/page-presentation.js";
import { bindInterfaceBatch } from "../src/interface/batch.js";
bindInterfaceBatch(presentationModule());
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
test('a lazy source replaces its retained Rust binding without changing the draw id',()=>{
 const {BfeRenderer,memory,calls}=standinModule({methods:{replace_sample_texture(id,owner,resource){calls.push(['replace_sample_texture',id,owner,resource]);}}});
 const {renderer,graphics}=standinThree(calls),texture=new THREE.Texture();
 const placeholder={},decoded={};renderer.properties.get(texture).__webglTexture=placeholder;
 let ready=false,resolutions=0;
 renderer.initTexture=t=>{resolutions++;const p=renderer.properties.get(t);p.__webglTexture=ready?decoded:placeholder;p.__version=t.version;};
 const core=new RustRender(graphics,{memory},BfeRenderer),id=core.texture(texture);
 for(let i=0;i<1000;i++)assert.equal(core.texture(texture),id);
 assert.equal(resolutions,1,'ordinary draws do not re-enter the resource resolver');
 texture.image={width:2,height:2};texture.needsUpdate=true;ready=true;
 assert.equal(core.texture(texture),id);assert.equal(core.textureSources[id],texture);
 assert.equal(calls.filter(c=>c[0]==='add_texture').length,1);
 assert.deepEqual(calls.find(c=>c[0]==='replace_sample_texture'),['replace_sample_texture',id,renderer.resources.gpu,0]);
 assert.equal(core.textures.has(placeholder),false);assert.equal(core.textures.get(decoded),id);
 // RenderTarget.setSize changes dimensions without incrementing Texture.version.
 // That descriptor change must bypass the draw lookup cache before a copy.
 texture.image.width=4;assert.equal(core.texture(texture),id);assert.equal(resolutions,3);
});

// Grow actual WebAssembly.Memory inside add_texture, detaching old views.
// The HUD group and index count prove the fresh view received its record.
// This is the failure that can otherwise leave a drawn world with an empty HUD.
test('registering a new HUD sampler cannot detach the frame inputs being written', async()=>{
 const {Batch}=await import('../src/interface/batch.js');
 const {BfeRenderer,memory,calls}=standinModule(),{renderer,graphics}=standinThree(calls);
 const backing=new WebAssembly.Memory({initial:512});memory.buffer=backing.buffer;
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

// Native post facts must survive every cold target/sampler registration. A
// detached view here would retain dimensions but silently lose the camera.
test('native post facts survive resource registration and retain program bindings',async()=>{
 const {Upscaler}=await import('../src/upscaler.js');
 const {FACTS:F,RESOURCE:R,PROGRAM:P}=await import('../src/rust-render/post-layout.js');
 const {BfeRenderer,memory,calls}=standinModule(),{renderer,graphics}=standinThree(calls);
 const target=()=>new THREE.WebGLRenderTarget(800,600,{depthBuffer:false});
 Object.assign(graphics,{quarterTarget:target(),bloomTarget:target(),bloomScratch:target(),postTarget:target(),aaTarget:target(),
  renderSize:new THREE.Vector2(800,600),outputSize:new THREE.Vector2(800,600),postRevision:0,vehicleMotion:0,vehicleTurn:0,blur:null,
  upscaler:new Upscaler({extensions:{has:()=>true}})});
 const backing=new WebAssembly.Memory({initial:512});memory.buffer=backing.buffer;
 const core=new RustRender(graphics,{memory},BfeRenderer),add=core.rust.add_texture.bind(core.rust);
 core.rust.add_texture=(...args)=>{const id=add(...args);backing.grow(1);memory.buffer=backing.buffer;return id;};
 const camera=new THREE.PerspectiveCamera();camera.position.set(1,2,3);camera.updateMatrixWorld();
 core.byName.post.begin(camera);core.views();
 assert.equal(core.postFacts[F.INPUT_WIDTH],800);assert.equal(core.postFacts[F.INPUT_HEIGHT],600);
 assert.equal(core.postFacts[F.WORLD+12],1);assert.equal(core.postFacts[F.WORLD+13],2);assert.equal(core.postFacts[F.WORLD+14],3);
 assert.equal(core.postFacts[F.PROGRAMS+P.FXAA],core.ownedPrograms.get('post-hud/fxaa').id);
 assert.equal(core.postFacts[F.RESOURCES+R.MOTION*R.STRIDE+R.WIDTH],1);
 const facts=core.postFacts,stats=core.postStats,registrations=calls.filter(c=>c[0]==='add_texture').length;
 core.byName.post.prepare();core.byName.post.begin(camera);core.byName.post.prepare();
 assert.equal(core.postFacts,facts);assert.equal(core.postStats,stats);
 assert.equal(calls.filter(c=>c[0]==='add_texture').length,registrations);
 assert.equal(calls.filter(c=>c[0]==='battle_frame'||c[0]==='frame_aux_frame').length,0,'publication issues no draw');
 core.rust.battle_frame=()=>false;core.rust.frame_error_frame=()=> 'Synthetic post refusal';
 core.frame.begin(true);assert.throws(()=>core.frame.execute(),/Synthetic post refusal/,'a rejected native frame stays visible');
});

// A pending-image placeholder can allocate even when the requested image is
// not ready. Prepare that sampler before acquiring post-input typed views.
// A successful return with a detached view would be a silent missing draw.
test('a screen pass prepares a pending image before taking shared views',async()=>{
 const {writeScreenStep}=await import('../src/rust-render/screen.js');
 const {BfeRenderer,memory,calls}=standinModule(),{renderer,graphics}=standinThree(calls);
 const backing=new WebAssembly.Memory({initial:512});memory.buffer=backing.buffer;
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
 const {BfeRenderer,memory,calls}=standinModule({methods:{replace_sample_texture(id,owner,resource){calls.push(['replace_sample_texture',id,owner,resource]);}}});
 const {renderer,graphics}=standinThree(calls),first=new THREE.Texture({width:1,height:1}),second=first.clone();
 const shared={};
 renderer.properties.get(first).__webglTexture=shared;
 renderer.properties.get(second).__webglTexture=shared;
 const core=new RustRender(graphics,{memory},BfeRenderer),a=core.texture(first),b=core.texture(second);
 assert.notEqual(a,b);
 assert.equal(core.texture(first),a);
 const replacement={};second.needsUpdate=true;
 renderer.initTexture=texture=>{const props=renderer.properties.get(texture);if(props.__version!==texture.version)props.__webglTexture=replacement;props.__version=texture.version;};
 assert.equal(core.texture(second),b);
 assert.equal(core.texture(first),a);
 assert.deepEqual(calls.filter(c=>c[0]==='replace_sample_texture'),[['replace_sample_texture',b,renderer.resources.gpu,renderer.resources.textureId(second)]]);
});

// A filtering revision may replace handles without changing source images.
// Refresh registered samples before submitting any phase that still uses them.
// The already retained ids and geometry must remain usable across that epoch.
test('an anisotropy revision uploads and rebinds retained static texture samples before drawing', () => {
 const {BfeRenderer,memory,calls}=standinModule({methods:{replace_sample_texture(id,owner,resource){calls.push(['replace_sample_texture',id,owner,resource]);}}});
 const {renderer,graphics}=standinThree(calls),texture=new THREE.Texture({width:1,height:1});
 const core=new RustRender(graphics,{memory},BfeRenderer),id=core.texture(texture),replacement={};
 texture.needsUpdate=true;graphics.textureRevision=1;
 renderer.initTexture=t=>{const p=renderer.properties.get(t);p.__webglTexture=replacement;p.__version=t.version;};
 core.publishView('image',renderer,new THREE.Scene(),new THREE.Camera());
 assert.equal(core.texture(texture),id);
 assert.deepEqual(calls.find(c=>c[0]==='replace_sample_texture'),['replace_sample_texture',id,renderer.resources.gpu,renderer.resources.textureId(texture)]);
 assert.ok(calls.findIndex(c=>c[0]==='replace_sample_texture')<calls.findIndex(c=>c[0]==='frame_aux_frame'));
});
