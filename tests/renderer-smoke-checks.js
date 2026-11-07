import {SurfaceOwner,stepSurface} from './helpers/surfaces.js';
import {actorPart} from './helpers/actor-resources.js';
import {actorAsset,glowLook,glowNode} from '../src/actor-resources.js';
import { LINE as LINE_DATA } from '../src/rust-render/line-layout.js';
import { lineSource, LINE_KIND } from '../src/line-source.js';
import { ROLE as FRAME_ROLE, STATS as FRAME_STATS, STATE as FRAME_STATE, UI as FRAME_UI } from "../src/rust-render/frame-layout.js";
import { STATE, FACTS, MODE } from "../src/rust-render/post-layout.js";
import { checkResources } from "./renderer-resource-checks.js";
import * as THREE from 'three';
import { Graphics } from '../src/graphics.js';
import { EFFECT } from '../src/rust-render/effect-layout.js';
import { squareBatches } from '../src/atmosphere.js';
import { ParticleRenderer } from '../src/particles.js';
import { TreeCanopies } from '../src/tree-canopies.js';
import { motionVectors } from '../src/motion-vectors.js';
import { Batch, loadInterfaceBatch } from '../src/interface/batch.js';
import { Water } from '../src/water.js';
import { loadEnvironment } from '../src/environment/module.js';
import { Settings } from '../src/settings.js';
import { actorMaterial, worldMaterial, lighting } from '../src/materials.js';
import { plainProgram } from '../src/rust-render/post.js';
import { writeScreenStep } from '../src/rust-render/screen.js';
import { ownWorldMaterial } from '../src/rust-render/world-programs.js';
import { ownedBasicEffect, ownedEffectMaterial } from '../src/rust-render/effect-material.js';
import { programLayouts } from '../src/render-programs.js';
import { interfaceFixture, shellInterfaceFixture } from './renderer-interface-checks.js';
import { checkOwnedActorDraws } from './actor-program-checks.js';

// All pixels, meshes and textures here are generated. No game loader runs.
// This is a Rust-only driver test, not a parity comparison with Three.
// Owned post/HUD shells intentionally have no browser shader source, so a
// native draw would test an unsupported path and can fail before Rust links.
// Keep fixture construction synthetic so the Windows runner needs no game files.
export async function checkRenderer(backend, manifests) {
  await loadInterfaceBatch();
  // Water's surface owner instantiates the environment module, which the game
  // compiles while a battle loads; the harness has no battle, so load it here.
  await loadEnvironment();
  const results = [], draws = {}, links = [], shaderFamilies = [], compiledShaders = [], programKinds = new Set(), ownedIds = new Map(), ownedDraws = new Set();
  const assert = (name, ok, detail) => {
    results.push({ name, ok: Boolean(ok), detail });
  };
  // Check actual driver link results and errors immediately after each draw,
  // including calls through WASM. Counters prevent a three-only false pass.
  const proto = WebGL2RenderingContext.prototype;
  const getExtension = proto.getExtension;
  let multiDraws = 0, multiAvailable = false, alphaCoverageDraws = 0;
  const alphaSamples = new Set();
  // Both modes still draw through Rust. Withholding WEBGL_multi_draw exercises
  // the required repeated-draw implementation on a backend without that extension.
  // Extension availability is reported rather than assumed from the platform.
  const fallback = new URLSearchParams(location.search).has('noMulti');
  proto.getExtension = function (name) {
    if (name === 'WEBGL_multi_draw' && fallback) return null;
    const ext = getExtension.call(this, name);
    if (name === 'WEBGL_multi_draw' && ext) multiAvailable = true;
    if (name === 'WEBGL_multi_draw' && ext && !ext.__smokeWrapped) {
      const draw = ext.multiDrawElementsWEBGL.bind(ext);
      ext.multiDrawElementsWEBGL = (...args) => {
        multiDraws++; draw(...args);
        const error = this.getError();
        if (error) throw new Error(`multiDrawElementsWEBGL: WebGL 0x${error.toString(16)}`);
      };
      ext.__smokeWrapped = true;
    }
    return ext;
  };
  const originals = new Map();
  let compilingKey=null,checkShadowAttachments=false,shadowAttachments=0;
  const shadowHandles=new Set(),depthPrograms=new Set();
  let observeShadow=null, observeSquares=null;
  // Hook the context prototype before initialization so Rust's WASM imports
  // and the remaining native compiler both pass through the same diagnostics.
  // The current catalog key tags even a failure inside a driver's compile call.
  for (const name of ['texImage2D','texSubImage2D','texImage3D','texSubImage3D','compressedTexImage2D','renderbufferStorage','renderbufferStorageMultisample','framebufferTexture2D','blitFramebuffer','compileShader', 'linkProgram', 'drawElements', 'drawArrays', 'drawElementsInstanced', 'drawArraysInstanced']) {
    const original = proto[name]; originals.set(name, original);
    proto[name] = function (...args) {
      const result = original.apply(this, args);
      if(name==='compileShader'){
        const ok=this.getShaderParameter(args[0],this.COMPILE_STATUS);
        compiledShaders.push({key:compilingKey,ok});
        if(!ok)throw Error(`${compilingKey || 'fixture'}: shader compile: ${this.getShaderInfoLog(args[0])}`);
      } else if (name === 'linkProgram') {
        const ok = this.getProgramParameter(args[0], this.LINK_STATUS);
        links.push(ok);if(compilingKey?.startsWith('actors/depth-'))depthPrograms.add(args[0]);
        if (!ok) throw new Error(`${compilingKey || "fixture"}: program link: ${this.getProgramInfoLog(args[0])}`);
      } else {
        if(name.startsWith('draw') && this.isEnabled(this.SAMPLE_ALPHA_TO_COVERAGE) && this.getParameter(this.SAMPLES)>1){alphaCoverageDraws++;alphaSamples.add(this.getParameter(this.SAMPLES));}
        if(name.startsWith('draw')){observeShadow?.(this);observeSquares?.(this);}
        if(name.startsWith('draw')) draws[name] = (draws[name] ?? 0) + 1;
        const error = this.getError();
        if (error && error !== this.CONTEXT_LOST_WEBGL) {
          console.log('renderer smoke: gl-error '+JSON.stringify({name,error,stack:new Error().stack.split('\n').slice(0,6),args:args.map(a=>typeof a==='number'?a:a?.buffer?{type:a.constructor.name,length:a.length,bytes:a.byteLength,offset:a.byteOffset}:a?.constructor?.name),unpack:[this.UNPACK_ROW_LENGTH,this.UNPACK_SKIP_ROWS,this.UNPACK_SKIP_PIXELS,this.UNPACK_ALIGNMENT].map(p=>this.getParameter(p))}));
          throw new Error(`${name}: WebGL 0x${error.toString(16)}`);
        }
      }
      return result;
    };
  }
  // Start from a deterministic low-effects frame, then change one setting at
  // a time. Off/on/off comparisons below detect both ignored settings and
  // state that remains accidentally enabled after a material revision.
  const settings = new Settings({ shadows: 'low', waterQuality: 'low', antialiasing: 'off', bloom: false, softFocus: false, renderScale: 1, upscaler: 'off', terrainDetail: 'original' });
  const g = new Graphics(document.body, settings);
  try {
    await g.rustReady;
    assert('Rust WASM initialized', Boolean(g.rust), g.rust.errors);
    const gl = g.renderer.getContext(), debug = gl.getExtension('WEBGL_debug_renderer_info');
    const renderer = debug && gl.getParameter(debug.UNMASKED_RENDERER_WEBGL);
    assert('Requested ANGLE backend selected', backend === 'warp' ? /Direct3D11/i.test(renderer) && /Microsoft|WARP|Basic Render/i.test(renderer) : backend === 'gl' ? /ANGLE.*OpenGL/i.test(renderer) : /SwiftShader/i.test(renderer), renderer);
    const coldResources=await checkResources(g.renderer,assert);
    // Every family is discovered by the launcher from its manifest at runtime.
    // Register before fixture draws, so the Rust cache is cold for every key.
    // Register before any fixture draws so each manifest key reaches a cold
    // Rust program cache. The manifest list comes from disk at launch time.
    // Stream progress before compilation: a driver hang must identify its variant.
    for(const {family,programs} of manifests){
      const started=performance.now(),variants=[];
      for(const {key} of programs){
        console.log(`renderer smoke: variant-start ${JSON.stringify({key,fallback})}`);
        const start=performance.now(),linkStart=links.length,shaderStart=compiledShaders.length;compilingKey=key;
        try {
          const id=g.rust.rust.add_owned_program(key);
          programKinds.add({World:0,Skinned:1,Rigid:2,Plain:3,Effect:4,Motion:9}[programLayouts[key].kind]);
          const error=gl.getError();if(error)throw Error(`WebGL 0x${error.toString(16)}`);
          variants.push({key,id,ms:performance.now()-start,accepted:true,shaders:compiledShaders.length-shaderStart,links:links.length-linkStart,cached:links.length===linkStart});
        } catch(error){variants.push({key,ms:performance.now()-start,accepted:false,error:String(error.message||error)});}
        finally {compilingKey=null;}
        console.log(`renderer smoke: variant-result ${JSON.stringify({...variants.at(-1),fallback})}`);
      }
      const row={family,fallback,compileMs:variants.filter(v=>!v.cached).reduce((sum,v)=>sum+v.ms,0),ms:performance.now()-started,variants};shaderFamilies.push(row);
      for(const variant of variants)assert(`${variant.key}: compiles and links`,variant.accepted,variant);
      console.log(`renderer smoke: family-result ${JSON.stringify(row)}`);
    }
    g.rust.stale=true;
    assert('All manifest families were checked',shaderFamilies.length>0);
    // Every WebGL upload goes through a typed-array view of WebAssembly memory. When the externref table
    // is full, making that view's JS value grows memory and detaches the view, and the GL call reads an
    // empty buffer (core.rs spare_view_slot). Register enough geometries for the table (each holds a
    // vertex array and buffers) to grow several times, and check that every buffer arrived whole.
    {
      const P=WebGL2RenderingContext.prototype,bufferData=P.bufferData,short=[];let uploads=0;
      P.bufferData=function(target,data,...rest){if(typeof data!=='number'){uploads++;if(!data || data.byteLength===0)short.push(target);}return bufferData.call(this,target,data,...rest);};
      try {
        const program={id:g.rust.rust.add_owned_program('actors/depth-solid'),attributes:[['position',0]]};
        for(let i=0;i<4000;i++){
          const geometry=new THREE.BufferGeometry();
          geometry.setAttribute('position',new THREE.BufferAttribute(new Float32Array(9+(i%7)*3),3));geometry.setIndex([0,1,2]);
          if(g.rust.geometry(geometry,program)==null)short.push('geometry '+i);
        }
      } finally { P.bufferData=bufferData; }
      assert('Uploads arrive whole while the externref table grows',uploads>=8000 && short.length===0,{uploads,short:short.slice(0,5)});
    }
    if(shaderFamilies.some(f=>f.variants.some(v=>!v.accepted)))return {ok:false,backend,renderer,results,shaderFamilies,compiledShaders};

    // One-pixel textures provide known output colours without decoding assets.
    // They also make blank frames distinguishable from a correctly sampled HUD.
    // No network or prepared-content request is necessary for these samplers.
    const texture = (rgba) => {
      const t = new THREE.DataTexture(new Uint8Array(rgba), 1, 1);
      t.needsUpdate = true; return t;
    };
    const white = texture([200, 150, 90, 255]), normal = texture([128, 128, 255, 100]);
    // A material can appear before a lazy image has uploaded. Its Rust id
    // must survive the transition from the transparent sample to real pixels.
    // Delay this sampler's native handle deliberately. The battle must retain a
    // transparent placeholder under the same Rust id and later adopt the upload.
    // A missing first-use handle cannot justify skipping or redrawing in Three.
    const pendingImage = new THREE.DataTexture(new Uint8Array([80,130,200,255]),1,1);
    g.setMap({ post: { blur: { enabled: true, blend: .25, downsize: .25 } }, lighting: { sunDirection: [.4, -1, -.2], ambient: [70, 70, 70], sunColor: [180, 180, 180] }, distance: { nearScene: [40, 40, 80, 80] } });
    g.camera.position.set(0, 0, 8); g.camera.lookAt(0, 0, 0);
    const geometry = new THREE.PlaneGeometry(1.2, 1.2); geometry.setAttribute('tangent', new THREE.Float32BufferAttribute([1,0,0,1,0,0,1,0,0,1,0,0],3)); geometry.setAttribute('binormal', new THREE.Float32BufferAttribute([0,1,0,0,1,0,0,1,0,0,1,0],3));
    // Match a captured map light-count variant with zero-intensity generated lights.
    for(let i=0;i<15;i++)g.scene.add(new THREE.PointLight(0,0,1));
    const material = worldMaterial('lit', { map: white, bumpMap: normal, specular: [20, .3, .3, .3] });
    material.defines.WORLD_SPACE = '';
    ownWorldMaterial(material,geometry);
    const nativeSources = new Map();
    const nativeWorld = (mesh, record, order = mesh.material.id) => {
      const attrs = ["position","normal","uv",...Object.keys(mesh.geometry.attributes).filter(n=>!["position","normal","uv"].includes(n))];
      const vertices = [];
      for(let v=0;v<mesh.geometry.attributes.position.count;v++)for(const name of attrs){const attribute=mesh.geometry.attributes[name];for(let k=0;k<attribute.itemSize;k++)vertices.push(attribute.getComponent(v,k));}
      const authored={...record,vertexCount:mesh.geometry.attributes.position.count,stride:attrs.reduce((n,key)=>n+mesh.geometry.attributes[key].itemSize,0),attributes:attrs};
      const node=new THREE.Object3D();node.name=mesh.name;node.userData.worldOrder=order;
      const source=g.rust.byName.world.admit(node,authored,Float32Array.from(vertices),Uint32Array.from(mesh.geometry.index.array),mesh.material.map,mesh.material.uniforms.bumpMap?.value);
      nativeSources.set(source,node);
      node.userData.nativeDepthSource=g.rust.rust.static_depth_add(source,g.rust.texture(g.rust.resources.staticSample(mesh.material.map)));
      g.scene.add(node);
      return node;
    };
    // Static buckets must have identity transforms; bake the placements.
    for (let i = 0; i < 3; i++) {
      const mesh = new THREE.Mesh(geometry.clone().translate(i * 1.5 - 1.5, 0, 0), material);
      mesh.matrixAutoUpdate = false; nativeWorld(mesh,{flags:0x21,bump:"normal",specular:[20,.3,.3,.3]});
    }
    // This raised caster projects onto the visible right terrain run.
    const raised=new THREE.Mesh(geometry.clone().scale(.6,.6,1).translate(1.5,-.9,.25),material);
    raised.matrixAutoUpdate=false;nativeWorld(raised,{flags:0x21,bump:"normal",specular:[20,.3,.3,.3]});
    const pendingMaterial=worldMaterial('unlit',{map:pendingImage,lights:false});pendingMaterial.defines.WORLD_SPACE='';ownWorldMaterial(pendingMaterial);
    const pendingMesh=new THREE.Mesh(new THREE.PlaneGeometry(.7,.7).translate(-2,2,1),pendingMaterial);
    pendingMesh.matrixAutoUpdate=false;nativeWorld(pendingMesh,{flags:0});
    const skinGeometry = new THREE.PlaneGeometry(.7, .7);
    skinGeometry.setAttribute('skinIndex', new THREE.Uint16BufferAttribute(new Uint16Array(16),4));
    skinGeometry.setAttribute('skinWeight', new THREE.Float32BufferAttribute([1,0,0,0,1,0,0,0,1,0,0,0,1,0,0,0],4));
    const skinMaterial = actorMaterial('dynamic', { map: white }); skinMaterial.authored.rigidSkin=true;
    const body = actorPart(skinGeometry,skinMaterial,true), bone = new THREE.Bone();
    body.add(bone); body.bind(new THREE.Skeleton([bone])); body.position.set(1.5,1,0); body.frustumCulled = false;
    const root = new THREE.Group(); root.userData.parts = { body }; root.add(body); g.scene.add(root); g.cullRoots.push(root);
    const farMaterial=worldMaterial('unlit',{map:texture([20,40,80,255]),far:true,lights:false});farMaterial.defines.WORLD_SPACE='';ownWorldMaterial(farMaterial);
    const far = new THREE.Mesh(new THREE.PlaneGeometry(20,20).translate(0,0,-90),farMaterial);far.matrixAutoUpdate=false;g.farScene.add(far);
    // Owned basic effects are the effects pass's; the transparent pass's own draws are
    // blended world surfaces, which this scene has none of.
    const transparent = new THREE.Mesh(new THREE.PlaneGeometry(.7, .7), ownedBasicEffect('basic', { color: 0x2277aa, map: white, transparent: true, opacity: .5 }));
    transparent.position.set(0, 1, 1); g.scene.add(transparent);
    const view = actorPart(new THREE.PlaneGeometry(.12,.12),actorMaterial('dynamic',{map:white}));
    view.position.set(.2, -.15, -1); g.viewScene.add(view);
    // This upright synthetic receiver uses Original terrain lighting; its
    // normal cannot describe a heightfield slope. Unused boundary vertices
    // fix a two-by-two native terrain grid. The two
    // visible triangles straddle its z boundary; the offscreen middle cell
    // separates their index spans, requiring the actual multi-draw path.
    const ground = new THREE.Group(); ground.userData.worldOrder = 10000;
    const groundPositions = [-3,-2,-.1,-1,-2,-.1,-3,-1,-.1, 99,99,-.1,100,99,-.1,99,100,-.1, 1,-2,.1,3,-2,.1,3,-1,.1, -128,0,-128,128,0,128];
    const groundVertices = Float32Array.from(Array.from({length:11},(_,i)=>[...groundPositions.slice(i*3,i*3+3),0,0,1,0,0,.7,.6,.5,1]).flat());
    const groundRecord = {terrain:true,vertexCount:11,stride:12,attributes:["position","normal","uv","color"],layers:["base","empty1","empty2","empty3","green"]};
    const emptySample = g.rust.texture(g.renderer.resources.gpu.world_fallback(false));
    const neutralSample = g.rust.texture(g.renderer.resources.gpu.world_fallback(true));
    const groundSamplers = Int32Array.of(g.rust.texture(white),-1,g.rust.texture(white),neutralSample,g.rust.texture(white),emptySample,emptySample,emptySample,g.rust.texture(texture([100,180,60,255])),emptySample,emptySample,emptySample,-1,emptySample);
    g.rust.byName.terrain.admit(ground,groundRecord,groundVertices,Uint32Array.of(0,1,2,3,4,5,6,7,8),Uint8Array.from(Array(11).fill([128,0,0,0,128,0,0,0]).flat()),groundSamplers);
    g.scene.add(ground);
    const cutoutMap=new THREE.DataTexture(new Uint8Array([220,80,40,128,220,80,40,255,220,80,40,0,220,80,40,192]),2,2);cutoutMap.needsUpdate=true;
    const cutoutMaterial=worldMaterial('unlit',{map:cutoutMap,lights:false,alphaTest:true,doubleSided:true});cutoutMaterial.defines.WORLD_SPACE='';
    const cutoutGeometry=new THREE.PlaneGeometry(.8,.8).translate(-1.5,-1,1);ownWorldMaterial(cutoutMaterial,cutoutGeometry);
    const cutout=new THREE.Mesh(cutoutGeometry,cutoutMaterial);cutout.matrixAutoUpdate=false;const cutoutNode=nativeWorld(cutout,{flags:2});
    const particles = new ParticleRenderer(g.scene).batch(white, 'additive');
    particles.begin(); particles.push(-1, 1, .5, 1, .4, .2, 1, .3); particles.end();
    // SquareBatch's planar instance records and trailing live facts must keep
    // distinct offsets, even after earlier effect owners occupy the packet.
    const haze={nativeKind:EFFECT.SQUARE.ID,variant:0,texture:white,capacity:5,count:5,
      values:Float64Array.of(.25,.5,.75,1,1,0,0,0,1,0),
      arrays:{center:Float32Array.from([0,1,0,.1,1,1,0,.1,2,1,0,.1,3,1,0,.1,4,1,0,.1]),spin:new Float32Array(10)},mesh:new THREE.Object3D()};
    haze.mesh.name='fixture-haze';haze.mesh.frustumCulled=false;haze.mesh.renderOrder=10;
    g.scene.add(haze.mesh);squareBatches.set(g.scene,[haze]);
    let squareDraws=0,squareValues=true;
    observeSquares=context=>{
      const program=context.getParameter(context.CURRENT_PROGRAM);
      if(!(program instanceof WebGLProgram)||context.getAttribLocation(program,'center')<0||context.getAttribLocation(program,'spin')<0)return;
      squareDraws++;
      for(const [name,expected] of [['tint',[.25,.5,.75,1]],['axisU',[1,0,0]],['axisV',[0,1,0]]]){
        const actual=context.getUniform(program,context.getUniformLocation(program,name));
        squareValues&&=actual?.length===expected.length&&actual.every((v,i)=>v===expected[i]);
      }
    };
    const batch = new Batch(), hudScene = new THREE.Scene(), hudCamera = new THREE.OrthographicCamera(0,320,0,240,-10,10);
    batch.begin(); batch.quad(new THREE.Matrix4(), 12, 12, 48, 40, white, [0,0,1,1], [1,1,1,1]); batch.quad(new THREE.Matrix4(),88,12,120,40,pendingImage,[0,0,1,1],[1,1,1,1]); batch.end(); hudScene.add(batch.mesh);
    const hud = { batch, scene: hudScene, camera: hudCamera };
    g.treeCanopies = new TreeCanopies(g, { texture: 'leaves', instances: [{ matrix: new THREE.Matrix4().makeTranslation(-6,2,-10).elements, bounds: [[-3,-3,-1],[3,3,1]] }] }, () => white);
    const instances = [{ position: new THREE.Vector3(2,-1,1), matrix: new THREE.Matrix4(), tint: [1,1,1], sphere: null, cullDistance: Infinity, maxDistance: 100 }];
    const foliage = { kinds: new Map([[0,{ map: white, def: { Alpha: 1 }, cards: [{ x:0,y:0,z:0,width:.4,height:.8,grey:1,part:0,phase:0,rate:1 }], grass: true, instances }]]), nearEnd: () => 100, time: 0, drawer: null };
    g.scene.userData.foliage = foliage;
    g.rust.byName.shadow.admitNative(JSON.parse(g.rust.rust.static_depth_finish()),nativeSources);
    body.receiveShadow = true; g.caster(body);
    const phaseDraws = {}, passDraws = Array(15).fill(0);
    // Seed the formerly unpainted unit with a comparison texture. A missing
    // rebind must reject the whole terrain draw, not merely shade it wrong.
    const makePoison = () => {
    const texture = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.DEPTH_COMPONENT16, 1, 1, 0, gl.DEPTH_COMPONENT, gl.UNSIGNED_SHORT, null);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_COMPARE_MODE, gl.COMPARE_REF_TO_TEXTURE);
      return texture;
    };
    let poison = makePoison();
    g.renderer.resetState();
    const rust = g.rust.rust;
    observeShadow=context=>{if(checkShadowAttachments && depthPrograms.has(context.getParameter(context.CURRENT_PROGRAM))){
      const actual=context.getFramebufferAttachmentParameter(context.DRAW_FRAMEBUFFER,context.DEPTH_ATTACHMENT,context.FRAMEBUFFER_ATTACHMENT_OBJECT_NAME);
      assert('Shadow draw attaches the current depth storage',shadowHandles.has(actual));shadowAttachments++;
    }};

    g.rust.rust = new Proxy(rust, { get(target, name) {
      const fn = target[name];
      if (typeof fn !== 'function') return fn;
      return (...args) => {
        if (name === 'add_program' || name === 'add_program_lamps') programKinds.add(args[0]);
        if(name==='post_frame')for(let i=0;i<args[0];i++)ownedDraws.add(g.rust.postInput[i*(5+12*18)]);
        if(name==='hud_frame')ownedDraws.add(g.rust.hudInput[1]);
        if (name === 'battle_frame') {
          gl.activeTexture(gl.TEXTURE7);gl.bindTexture(gl.TEXTURE_2D,poison);
          if(checkShadowAttachments){shadowHandles.clear();for(const target of [...g.shadowMaps,...g.shadowCaches])shadowHandles.add(g.renderer.properties.get(target.depthTexture).__webglTexture);}
        }
        let value = fn.apply(target, args);
        if(name==='battle_frame' || name==='frame_aux_frame'){
          g.rust.views();const roles=name==='battle_frame'?Array.from({length:FRAME_ROLE.COUNT},(_,i)=>i):[args[1]];
          for(const role of roles){const at=role*FRAME_STATS.LEN;for(let i=0;i<passDraws.length;i++)passDraws[i]+=g.rust.frameStats[at+FRAME_STATS.PASS_DRAWS+i];
            const key=role===FRAME_ROLE.NEAR?'frame':role===FRAME_ROLE.FAR?'far_frame':role===FRAME_ROLE.TRANSPARENT?'transparent_frame':role===FRAME_ROLE.VIEW?'view_frame':role===FRAME_ROLE.POST?'post_chain_frame':null;
            if(key)phaseDraws[key]=(phaseDraws[key]||0)+g.rust.frameStats[at+FRAME_STATS.PROGRAM_SWITCHES];
          }
          if(name==='battle_frame'){for(let i=0;i<g.rust.postState[STATE.STEPS];i++)ownedDraws.add(g.rust.postInput[i*221]);ownedDraws.add(g.rust.hudInput[1]);}
          else if(args[1]>=FRAME_ROLE.HUD0&&args[1]<=FRAME_ROLE.HUD7)ownedDraws.add(g.rust.hudInput[1]);
        }
        if (name === 'post_chain_frame') {
          g.rust.views(); value = g.rust.postStats;
          for (let i = 0; i < g.rust.postState[STATE.STEPS]; i++) ownedDraws.add(g.rust.postInput[i*(5+12*18)]);
        }
        if(name==='add_owned_program')ownedIds.set(args[0],value);
        if(name==='hud_resources_frame'){g.rust.views();ownedIds.set('post-hud/hud',g.rust.hudInput[1]);}
        if (/frame$|_segment$|near_begin$/.test(name) && value?.length) {
          phaseDraws[name] = (phaseDraws[name] ?? 0) + value[3];
          for (let i = 0; i < passDraws.length; i++) passDraws[i] += value[5+i] ?? 0;
          const error = gl.getError(); if (error && error !== this.CONTEXT_LOST_WEBGL) throw new Error(`${name}: WebGL 0x${error.toString(16)}`);
        }
        return value;
      };
    } });
    g.warmup();
    // Every comparison draws the required battle path and then reads its canvas.
    // Draw counts, renderer errors and GL status reject an empty successful call.
    // Known UI colour assertions catch a correctly sized but incorrectly bound target.
    const capture = () => {
      g.camera.updateMatrixWorld(true);
      if (foliage.drawer) foliage.drawer.select(foliage,g.camera);
      g.rust.frame.begin(true);
      g.rust.frame.mirror(g.scene.userData.water,g.camera);
      g.prepareFrame(0);
      g.rust.frame.underwater(g.scene.userData.water,g.camera);
      g.renderer.autoClear=false;g.rust.coverHud(g.renderer,hud);g.renderer.autoClear=true;
      g.rust.frame.execute();g.profiler.end();
      assert("Native view packets fit their retained stores",g.rust.frameState[FRAME_STATE.CLIPPED]===0);
      g.renderer.setRenderTarget(null);
      if (g.rust.errors.length || g.renderer.battleThreeCalls) throw new Error(`Rust fallback: ${JSON.stringify(g.rust.errors)}`);
      const pixels = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4);
      gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      const error = gl.getError(); if (error) throw new Error(`Frame: WebGL 0x${error.toString(16)}`);
      return pixels;
    };
    // Warm the same configuration before collecting two independent draws.
    // Exact repetition is expected because fixtures have no moving wall-clock input.
    // Visibility and known colour are checked as well as equality: two blank images
    // must never be accepted as stable rendered output.
    const compare = (name) => {
      console.log(`renderer smoke: ${name}`);
      for (let i = 0; i < 4; i++) capture();
      const a = capture(), b = capture();
      assert(`${name}: stable Rust frame`, a.every((v,i) => v === b[i]));
      let visible = 0;
      for (let i = 0; i < a.length; i += 4) if (a[i] > 40 || a[i+1] > 40 || a[i+2] > 90) visible++;
      assert(`${name}: visible fixture`, visible > 100, { visible });
      // The HUD uses an unlit one-pixel texture, without post processing.
      const x = Math.floor(gl.drawingBufferWidth * 30 / 320);
      const y = Math.floor(gl.drawingBufferHeight * (240 - 25) / 240);
      const at = 4 * (y * gl.drawingBufferWidth + x);
      assert(`${name}: generated HUD colour`, [200,150,90].every((v,k) => Math.abs(a[at+k]-v) <= 1), [...a.slice(at,at+4)]);
      return a;
    };
    // Settings checks compare against actual pixels, not only uniform values.
    // Returning to the baseline tests that shader/material revisions and target
    // changes do not leave stale enabled state in a retained Rust program.
    const baseline=compare('opaque, far, terrain, transparent, view and composite');
    const terrainShadowPixels=()=>{
      // Lighting uniforms are native readbacks. Publish the fixture toggle
      // through the same owner as battle environment and settings changes.
      g.setEnvironment({shadowStrength:0});const unshadowed=capture();g.setEnvironment({shadowStrength:1});const shadowed=capture();
      const lo=new THREE.Vector3(1.8,-1.9,.1).project(g.camera),hi=new THREE.Vector3(2.2,-1.75,.1).project(g.camera);
      let pixels=0;
      for(let y=Math.ceil((lo.y+1)*.5*gl.drawingBufferHeight);y<Math.floor((hi.y+1)*.5*gl.drawingBufferHeight);y++)for(let x=Math.ceil((lo.x+1)*.5*gl.drawingBufferWidth);x<Math.floor((hi.x+1)*.5*gl.drawingBufferWidth);x++){
        const at=4*(y*gl.drawingBufferWidth+x);if([0,1,2].some(k=>shadowed[at+k]!==unshadowed[at+k]))pixels++;
      }
      return pixels;
    };
    // Replace a cutout's material through the same raw-source API used by
    // loading. Both colour and depth must return to the exact original pixels.
    const cutoutRows=[];
    for(let v=0;v<cutoutGeometry.attributes.position.count;v++)for(const name of ['position','normal','uv']){const a=cutoutGeometry.attributes[name];for(let k=0;k<a.itemSize;k++)cutoutRows.push(a.getComponent(v,k));}
    const replaceCutout=flags=>g.rust.byName.world.admit(cutoutNode,{flags,vertexCount:4,stride:8,attributes:['position','normal','uv']},Float32Array.from(cutoutRows),Uint32Array.from(cutoutGeometry.index.array),cutoutMap,null);
    const depthReference=cutoutNode.userData.nativeDepthSource;
    replaceCutout(0);compare('native material replacement to opaque');
    replaceCutout(2);const replacement=compare('native material replacement restored');
    assert('Material replacement preserves depth reference and original pixels',depthReference===cutoutNode.userData.nativeDepthSource&&replacement.every((v,i)=>v===baseline[i]));
    const shadowPixels=terrainShadowPixels();
    assert('Multi-draw terrain receives an occluding mesh shadow',shadowPixels>0,{terrainShadowPixels:shadowPixels});
    // Settings can rebuild shadow storage after a battle has cached its FBOs.
    // Also resize the same descriptors: immutable depth changes its GL handle
    // while retained sampler/FBO ids must keep addressing the replacement.
    const resizeShadows=size=>{
      g.csm.shadowMapSize=size;
      for(let i=0;i<g.csm.lights.length;i++){
        g.csm.lights[i].shadow.mapSize.set(size,size);
        g.shadowMaps[i].setSize(size,size);g.shadowCaches[i].setSize(size,size);
        g.cascadeWindows[i].invalidate();
      }
      g.projection='';
    };
    const resizedDepth=g.shadowMaps[0].depthTexture,depthId=g.rust.texture(resizedDepth),depthHandle=g.renderer.properties.get(resizedDepth).__webglTexture;
    const shadowSize=g.csm.shadowMapSize;
    checkShadowAttachments=true;resizeShadows(shadowSize*2);capture();
    assert('Mid-frame shadow resize replaces storage while retaining its sampler id',g.rust.texture(resizedDepth)===depthId&&g.renderer.properties.get(resizedDepth).__webglTexture!==depthHandle);
    {const pixels=terrainShadowPixels();assert('Resized maps still draw shadowed receivers',pixels>0&&shadowAttachments>0,{pixels,shadowAttachments});}
    resizeShadows(shadowSize);capture();checkShadowAttachments=false;
    lighting.bump.value = 0; compare('bump switched off'); lighting.bump.value = 1;
    settings.set('bloom', true); settings.set('softFocus', true); g.setVehicleMotion(.8,.2); const effects=compare('bloom, soft focus and speed blur');
    assert('Post settings change visible pixels',effects.some((v,i)=>i%4!==3&&v!==baseline[i]));
    settings.set('bloom', false); settings.set('softFocus', false); g.setVehicleMotion(0,0); const plain=compare('post effects switched off');
    assert('Disabling post effects reproduces the plain frame',plain.every((v,i)=>v===baseline[i]));
    settings.set('antialiasing','fxaa'); const fxaa=compare('FXAA');
    assert('FXAA changes fixture edge pixels',fxaa.some((v,i)=>i%4!==3&&v!==plain[i]));
    settings.set('antialiasing','fxaa-fast'); const fastFxaa=compare('fast FXAA');
    assert('Fast FXAA changes fixture edge pixels',fastFxaa.some((v,i)=>i%4!==3&&v!==plain[i]));
    settings.set('antialiasing','off');
    const noFxaa=compare('FXAA switched off');
    assert('Disabling FXAA restores the plain frame',noFxaa.every((v,i)=>v===plain[i]));
    settings.set('renderScale', .75);
    settings.set('upscaler','spatial'); compare('spatial upscale');
    // FXAA forces the explicit bilinear pass rather than the direct composite.
    settings.set('antialiasing','fxaa');
    settings.set('upscaler','bilinear'); compare('bilinear upscale with FXAA');
    settings.set('antialiasing','off');
    settings.set('upscaler','temporal');
    assert('Temporal targets supported',g.upscaling==='temporal');
    const temporalSequence=()=>{
      g.upscaler.reset();
      let pixels;for(let i=0;i<8;i++)pixels=capture();return pixels;
    };
    const temporalA=temporalSequence(),temporalB=temporalSequence();
    assert('Temporal post sequence repeats exactly',temporalA.every((v,i)=>v===temporalB[i]));
    settings.set('upscaler','off');settings.set('renderScale',1);
    settings.set('shadows','off');compare('shadows switched off');
    settings.set('shadows','medium');checkShadowAttachments=true;compare('three cascade shadow receivers');
    assert('Settings shadow-resolution change still draws shadows',terrainShadowPixels()>0);
    checkShadowAttachments=false;
    settings.set('shadows','high');compare('four cascade shadow receivers');
    settings.set('shadows','low');compare('shadows restored');
    settings.set('lampLighting',true);compare('lamps enabled');
    settings.set('lampLighting',false);compare('lamps disabled');
    const sampleId=g.rust.texture(white),sampleCount=g.rust.textureSources.length;
    g.textures.add(white);g.filter(white);
    settings.set('anisotropy',2);compare('anisotropy changed');
    assert('Sampler changes retain Rust texture ids',g.rust.texture(white)===sampleId&&g.rust.textureSources.length===sampleCount);
    settings.set('anisotropy',1);compare('anisotropy restored');
    assert('World draws taken by Rust', g.rust.byName.world.count >= 3, g.rust.byName.world.count);
    settings.set('antialiasing','4x');compare('four-sample alpha to coverage');
    const coveredShadowPixels=terrainShadowPixels();
    assert('Multisampled multi-draw terrain receives an occluding mesh shadow',coveredShadowPixels>0,{terrainShadowPixels:coveredShadowPixels,samples:g.sceneTarget.samples});
    assert('Alpha-to-coverage drew on a four-sample target',alphaCoverageDraws>0&&alphaSamples.has(4),{alphaCoverageDraws,samples:[...alphaSamples]});
    settings.set('antialiasing','off');compare('multisampling switched off');
    assert('Native terrain has no browser geometry or material', ground.children.length === 0 && ground.material === undefined);
    for (const name of ['frame', 'far_frame', 'transparent_frame', 'view_frame', 'post_chain_frame']) assert(`${name} issues Rust programs`, phaseDraws[name] > 0, phaseDraws);
    // Motion has its own target and depth input, separate from post history.
    // Use real motion tracking with synthetic depth and two distinct view matrices.
    // Nonzero velocity proves the shader did work; exact repetition detects stale
    // previous-frame data or target state without requiring any animated game asset.
    const proxy = motionVectors.track(body);
    const motionTarget = new THREE.WebGLRenderTarget(64,64,{type:THREE.HalfFloatType,depthBuffer:false});
    const depthValues = new Float32Array(64*64).fill((g.camera.far - g.camera.near*g.camera.far/8)/(g.camera.far-g.camera.near));
    const depth = new THREE.DataTexture(depthValues,64,64,THREE.RedFormat,THREE.FloatType); depth.needsUpdate = true;
    const current = new THREE.Matrix4().multiplyMatrices(g.camera.projectionMatrix,g.camera.matrixWorldInverse);
    const previous = current.clone(); previous.elements[12] += .02;
    const motion = () => {
      motionVectors.render(g.renderer,{target:motionTarget,root:g.scene,camera:g.camera,viewScene:g.viewScene,view:g.viewCamera,depth,current,previous});
      const pixels = new Uint16Array(64*64*4); g.renderer.readRenderTargetPixels(motionTarget,0,0,64,64,pixels); return pixels;
    };
    motion(); motion();
    const motionFirst = motion(), motionRust = motion();
    assert('Motion proxy writes nonzero velocity', motionFirst.some((v,i) => i % 4 === 0 && v !== 0));
    assert('Motion target is stable', motionFirst.every((v,i) => v === motionRust[i]));
    motionVectors.release(proxy); motionTarget.dispose();
    // Water's far copies and transparent surface have their own programs,
    // checked after the other fixture's independent comparisons.
    // The small generated pool supplies the actual Water and underwater paths.
    // Its data is intentionally sufficient for reflection and fog construction,
    // so this checks the battle renderer integration rather than an isolated shader.
    const water = new Water({surfaceOwner:new SurfaceOwner(undefined,true), data: { height:-2,cell:12,offset:[6,6],grid:2,cells:[[0,0]],look:{PatchDivisions:[2]} }, texture: () => null, graphics: g });
    g.scene.userData.water = water;
    stepSurface(water,0,g.camera);
    compare('water surface');
    const cameraAt=g.camera.position.clone(),cameraRotation=g.camera.quaternion.clone();
    const basin=water.cellBounds[0].getCenter(new THREE.Vector3());
    // A glow can be created between frames and first appear in a reflection.
    const lateGlowMaterial=glowLook(white,0xffffff,.7);lateGlowMaterial.basic=true;
    const lateGlow=glowNode(actorAsset({kind:'plane',width:1,height:1}),lateGlowMaterial),lateGlowGroup=new THREE.Group();
    lateGlowGroup.userData.material=lateGlowMaterial;lateGlowGroup.add(lateGlow);g.scene.add(lateGlowGroup);
    lateGlow.position.set(basin.x,water.level+1,basin.z);lateGlow.frustumCulled=false;
    settings.set('waterQuality','ultra');
    g.camera.position.set(basin.x,water.level+3,basin.z+6);g.camera.lookAt(basin.x,water.level+1,basin.z);g.camera.updateMatrixWorld(true);
    stepSurface(water,0,g.camera);const mirrors=g.rust.byName.mirrorCover.drawn;
    compare('Ultra reflection with a new glow');
    assert('Ultra reflection discovers a new glow before the main view',Boolean(g.rust.byName.effects.glows.get(lateGlow)));
    assert('Ultra reflection draws the complete near scene',g.rust.byName.mirrorCover.drawn>mirrors);
    settings.set('waterQuality','high');
    g.camera.position.set(basin.x,water.level-1,basin.z);
    g.camera.lookAt(basin.x+1,water.level-1,basin.z);g.camera.updateMatrixWorld(true);
    stepSurface(water,0,g.camera);
    compare('High underwater composite');
    assert('High underwater fog issues a Rust composite',g.rust.frameStats[(FRAME_ROLE.FOG+1)*FRAME_STATS.LEN-1]>0);
    settings.set('waterQuality','low');
    g.camera.position.copy(cameraAt);g.camera.quaternion.copy(cameraRotation);g.camera.updateMatrixWorld(true);stepSurface(water,0,g.camera);

    // After readiness changes, all retained draws must still sample the same id.
    // Reallocating the logical id would leave previously registered program rows
    // pointing at the transparent placeholder despite a successful native upload.
    const pendingId=g.rust.texture(pendingImage);
    pendingImage.needsUpdate=true;
    const uploaded=capture();
    assert('Lazy image keeps its Rust texture id',g.rust.texture(pendingImage)===pendingId);
    const sample=(x,y)=>Array.from(uploaded.slice(4*(y*gl.drawingBufferWidth+x),4*(y*gl.drawingBufferWidth+x)+3));
    const hudPixel=sample(Math.floor(gl.drawingBufferWidth*104/320),Math.floor(gl.drawingBufferHeight*(240-25)/240));
    assert('HUD sees the uploaded lazy image',[80,130,200].every((v,k)=>Math.abs(hudPixel[k]-v)<=1),hudPixel);
    const projected=new THREE.Vector3(-2,2,1).project(g.camera);
    const worldPixel=sample(Math.floor((projected.x+1)*.5*gl.drawingBufferWidth),Math.floor((projected.y+1)*.5*gl.drawingBufferHeight));
    assert('World binding sees the uploaded lazy image',[80,130,200].every((v,k)=>Math.abs(worldPixel[k]-v)<=1),worldPixel);
    const tintScene = new THREE.Scene(), tintCamera = new THREE.OrthographicCamera(-1,1,1,-1,0,1);
    // The Low water's tint is a catalog program like every other screen material.
    tintScene.add(new THREE.Mesh(new THREE.PlaneGeometry(2,2), ownedEffectMaterial('effects/water-underwater-tint', {
      uniforms:{color:{value:new THREE.Vector4(.1,.2,.3,.5)}},
      transparent:true,depthTest:false,depthWrite:false,visible:true,
    })));
    tintScene.children[0].frustumCulled=false;
    g.renderer.autoClear=false;
    g.rust.cover('underwater',g.renderer,tintScene,tintCamera);
    g.renderer.autoClear=true;
    // Overlapping partial writes of different lengths must survive restoration.
    // A direct retained geometry isolates journal byte ordering from scene updates.
    // Overlapping VBO/EBO writes include shorter suffix-preserving ranges; replay
    // must reproduce the final contents, not merely the latest call per offset.
    const retainedGeometry=new THREE.PlaneGeometry(2,2);
    const retainedMaterial=new THREE.ShaderMaterial({
      vertexShader:'void main(){gl_Position=vec4(position,1.0);}',
      fragmentShader:'void main(){gl_FragColor=vec4(1.0,.25,.125,1.0);}',
      depthTest:false,depthWrite:false,
    });
    const retainedMesh=new THREE.Mesh(retainedGeometry,retainedMaterial),retainedScene=new THREE.Scene();retainedScene.add(retainedMesh);
    g.renderer.compile(retainedScene,tintCamera);
    const retainedProgram=plainProgram(g.rust,retainedMaterial,'position:0');
    const retainedId=g.rust.geometry(retainedGeometry,{id:retainedProgram.id,attributes:[['position',0]]});
    const bytes=a=>new Uint8Array(a.buffer,a.byteOffset,a.byteLength);
    g.rust.rust.geometry_buffer_sub_data(retainedId,0,0,bytes(Float32Array.of(-.7,1,0,.5,1,0)));
    g.rust.rust.geometry_buffer_sub_data(retainedId,0,0,bytes(Float32Array.of(-.5,1,0)));
    g.rust.rust.geometry_index_sub_data(retainedId,0,bytes(Uint16Array.of(0,2,1,1,2,3)));
    g.rust.rust.geometry_index_sub_data(retainedId,0,bytes(Uint16Array.of(0,1,2)));
    const retainedTarget=new THREE.WebGLRenderTarget(32,32,{depthBuffer:false});
    // Draw the same retained ids before and after loss. Rebuilding a separate
    // fixture would conceal a broken logical-id contract or lost buffer contents.
    // The small target makes exact recovery independent of the full post chain.
    const retainedPixels=()=>{
      g.renderer.setRenderTarget(retainedTarget);g.renderer.clear();
      const target=g.rust.byName.post.target(retainedTarget);
      writeScreenStep(g.rust,0,retainedProgram,retainedMaterial,retainedMesh,tintCamera,{target,size:[32,32],geometry:{id:retainedId,count:6}});
      g.rust.rust.screen_frame(1);g.rust.stale=true;
      const pixels=new Uint8Array(32*32*4);g.renderer.readRenderTargetPixels(retainedTarget,0,0,32,32,pixels);return pixels;
    };
    const retainedBefore=retainedPixels();
    assert('Partial-update fixture draws visible pixels',retainedBefore.some((v,i)=>i%4===0&&v===255));
    // Real cables and debug segments retain only live points on the page.
    // Native descriptors must still draw, update and survive recovery.
    const withoutLines=capture(),lineGroup=new THREE.Group();g.scene.add(lineGroup);
    const lineObjects=[];
    for(let i=0;i<2;i++){
      const object=lineSource(i,128),source=object.userData.lineSource;
      const positions=[-1,-.7-i*.15,-2,1,-.7-i*.15,-2,0,-.3-i*.15,-2];
      source.positions.set(positions);source.count=3;
      for(let at=0;at<positions.length;at+=3)new THREE.Vector3().fromArray(source.positions,at).applyMatrix4(g.camera.matrixWorld).toArray(source.positions,at);
      lineGroup.add(object);lineObjects.push(object);
    }
    const withLines=compare('tow cable and flight helper topologies');
    assert('Rust line draws change visible pixels',withLines.some((v,i)=>i%4!==3&&v!==withoutLines[i]));
    const cable=lineObjects[0],source=cable.userData.lineSource,id=g.rust.byName.lines.records.get(cable).id;
    source.positions[0]+=.3;source.serial++;
    const movedLines=compare('dynamic tow cable positions');
    assert('Dynamic line updates change visible pixels',movedLines.some((v,i)=>i%4!==3&&v!==withLines[i]));
    assert('Dynamic cables keep the retained native descriptor',g.rust.byName.lines.records.get(cable).id===id);
    assert('Cables and helpers retain no browser geometry or material',lineObjects.every(object=>!object.geometry&&!object.material&&g.rust.byName.lines.owns(object)));
    assert('Cables and helpers issue native line draws',g.rust.frameStats[(FRAME_ROLE.LINES2+1)*FRAME_STATS.LEN-1]===lineObjects.length);
    // Hostile line counts and ids clip in the native owner, including a
    // publication larger than any map can create. The next frame still draws.
    const clippedLines=g.rust.rust.line_clipped();g.rust.views();
    g.rust.lineInput[LINE_DATA.VIEW.COUNT]=Infinity;
    g.rust.lineInput[LINE_DATA.VIEW.LEN+LINE_DATA.ROW.SOURCE]=-1;
    g.rust.rust.line_frame(2);
    assert('Native line publication clips hostile counts and ids',g.rust.rust.line_clipped()>clippedLines);
    compare('line publication resumes after clipping');
    // A slow callback can prepare more overlay batches than a view admits.
    // Keep the earlier packets, clip the remainder and leave history invalid;
    // the next ordinary frame must still draw without a browser exception.
    const resumed=new THREE.Object3D();g.scene.add(resumed);
    g.rust.frame.begin(true);g.prepareFrame(0);
    const completed=g.rust.frameState[FRAME_STATE.FRAME];
    g.rust.frame.uiStage(FRAME_UI.OVERLAY);
    for(let i=0;i<128;i++)g.rust.coverHud(g.renderer,hud);
    g.rust.frame.execute();
    assert('Full overlay stores clip instead of throwing',g.rust.frameState[FRAME_STATE.CLIPPED]===1);
    assert('Clipped frames retain their completion stamp',g.rust.frameState[FRAME_STATE.FRAME]===completed);
    resumed.position.x=3;
    capture();
    assert('The next frame refreshes transforms after clipping',resumed.matrixWorld.elements[12]===3);
    g.scene.remove(resumed);
    // Keep every live resource and logical id across the forced loss. The browser
    // invalidates the old GL objects; only a reconstruction can reproduce this frame.
    // Comparing after several warm draws also lets invalidated histories settle.
    const ui = interfaceFixture(g, assert);
    const beforeLoss = capture();
    const loss = gl.getExtension('WEBGL_lose_context');
    if (!loss) throw new Error('Context-loss extension unavailable');
    const canvas = g.renderer.domElement;
    console.log("renderer smoke: context loss");
    const lost = new Promise(resolve => canvas.addEventListener('webglcontextlost', resolve, { once: true }));
    // Loss is observable natively before the browser dispatches its event.
    // A pending temporal chain must not advance either bank or phase then.
    g.rust.postFacts[FACTS.MODE] = MODE.TEMPORAL;
    g.rust.rust.post_chain_begin_frame();
    const lostRead=g.upscaler.read,lostFrame=g.upscaler.frame;
    loss.loseContext();
    g.rust.rust.post_chain_frame();
    assert('A lost post frame never commits history',g.upscaler.read===lostRead&&g.upscaler.frame===lostFrame&&!g.upscaler.valid);
    await lost;
    // Loading and window changes can create/resize descriptors while drawing
    // is suspended. They must be realized by restoration, never by native GL.
    const duringLoss=new THREE.WebGLRenderTarget(16,8,{depthBuffer:false});
    g.renderer.initRenderTarget(duringLoss);duringLoss.setSize(8,4);g.renderer.initRenderTarget(duringLoss);
    await new Promise(resolve => setTimeout(resolve, 100));
    const restored = new Promise((resolve,reject) => {
      const timer=setTimeout(()=>reject(new Error(`Context restore timeout: ${g.rust.failure?.stack || "no renderer error"}`)),30000);
      canvas.addEventListener("webglcontextrestored",()=>{clearTimeout(timer);resolve();},{once:true});
    });
    // The loss event must have finished before restoration is requested.
    // Calling restoreContext in its event microtask is rejected by WebGL, which
    // would test an illegal harness sequence instead of renderer recovery.
    loss.restoreContext();
    await restored;
    {const error=gl.getError();if(error)throw Error(`After restore: GL ${error}`);}
    // The descriptor fixture released its Three Source before loss. Only the
    // native sampler retains its restoration bytes and stable logical handle.
    {
      const sampler=g.renderer.resources.gpu.texture(coldResources.native),fb=gl.createFramebuffer(),rgba=new Uint8Array(4);
      gl.bindFramebuffer(gl.FRAMEBUFFER,fb);gl.framebufferTexture2D(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,sampler,0);
      gl.readPixels(0,0,1,1,gl.RGBA,gl.UNSIGNED_BYTE,rgba);gl.deleteFramebuffer(fb);
      assert('Native sampler restores exact bytes without a Three Source',rgba.every((v,i)=>v===[31,73,149,227][i]));
      g.renderer.setRenderTarget(null);
    }
    poison = makePoison();
    {const error=gl.getError();if(error)throw Error(`After poison: GL ${error}`);}
    g.renderer.resetState();
    for (let i=0;i<4;i++) capture();
    // Restoration must preserve visible output and the partial-update fixture.
    // A successful context event alone does not prove programs, VAOs, indices,
    // textures, target handles and shared CPU inputs were all reconstructed.
    const afterLoss = capture();
    assert('Restored Rust frame matches its generated fixture', beforeLoss.every((v,i) => v === afterLoss[i]));
    const retainedAfter=retainedPixels();
    assert('Overlapping vertex and index updates survive restoration',retainedBefore.every((v,i)=>v===retainedAfter[i]));
    g.renderer.setRenderTarget(duringLoss);gl.clearColor(1,0,0,1);gl.clear(gl.COLOR_BUFFER_BIT);
    const recoveredTarget=new Uint8Array(8*4*4);g.renderer.readRenderTargetPixels(duringLoss,0,0,8,4,recoveredTarget);
    assert('Targets published and resized during loss rebuild correctly',recoveredTarget.every((v,i)=>v===[255,0,0,255][i%4]));
    g.renderer.setRenderTarget(null);duringLoss.dispose();
    const uiRestored = ui.pixels();
    assert('Restored interface quads, offscreen target and skinned model match',ui.before.every((v,i)=>v===uiRestored[i]));
    ui.dispose();
    retainedTarget.dispose();
    assert('Battle never invokes three draw', g.renderer.battleThreeCalls === 0);
    assert('Every shader rewrite kind links', [0,1,2,3,4,9].every(k => programKinds.has(k)), [...programKinds]);
    // The transparent pass draws blended world surfaces, which the fixture has none of
    // (its blended mesh is an owned effect, drawn by the effects pass).
    const passNames = ['world','instances','far','terrain','transparent','effects','foliage','view','post','hud','water','shadow','motion','canopies','screen'];
    assert('Native haze preserves trailing tint and camera axes',squareDraws>0&&squareValues,{squareDraws});
    assert('All Rust passes draw', passDraws.every((n, i) => n > 0 || passNames[i] === 'transparent' || passNames[i] === 'water'), { names: passNames, counts: passDraws });
    assert('Available Rust draw path exercised', fallback || !multiAvailable ? multiDraws === 0 : multiDraws > 0, { fallback, multiAvailable, multiDraws });
    assert('Every declared Rust post and HUD program draws',Object.keys(programLayouts).filter(key=>key.startsWith('post-hud/')).every(key=>ownedIds.has(key)&&ownedDraws.has(ownedIds.get(key))), {registered:[...ownedIds.keys()],drawn:[...ownedDraws]});
    // Count actual successful links, including reconstructed programs. Program
    // registration without a driver link, or a swallowed compile failure, cannot
    // stand in for backend coverage even when another pass produced visible pixels.
    assert('Every compiled program links', links.length > 0 && links.every(Boolean), { count: links.length });
    await shellInterfaceFixture(assert);
    const actorDraws = await checkOwnedActorDraws();
    assert('Owned actor bodies, rigs, held and view parts, shock and dissolve draw', actorDraws.ok, actorDraws);
// Return all family and phase evidence, including the backend/draw mode,
// so CI cannot confuse a successful subset with full driver coverage.
    return { ok: results.every(r => r.ok), backend, fallback, multiAvailable, multiDraws, renderer, results, shaderFamilies, compiledShaders, draws, passDraws, phaseDraws, programKinds: [...programKinds], linkedPrograms: links.length };
  } catch (error) {
    const stopped=g.rust ? g.rust.byName.near.stopped : null;
    throw new Error(`${error.stack}\nCause: ${error.cause?.stack || "none"}\nStopped: ${JSON.stringify(stopped && {name:stopped.name,material:stopped.material?.name,uniforms:Object.keys(stopped.material?.uniforms||{}),background:stopped===g.background,layer:stopped.layers.mask})}`);
  } finally {
    proto.getExtension = getExtension;
    for (const [name, original] of originals) proto[name] = original;
    g.renderer.dispose();
  }
}
