import * as THREE from 'three';
import { Graphics } from '../src/graphics.js';
import { ParticleRenderer } from '../src/particles.js';
import { TreeCanopies } from '../src/tree-canopies.js';
import { FoliageGpuBatch } from '../src/foliage-gpu.js';
import { motionVectors } from '../src/motion-vectors.js';
import { Batch } from '../src/interface/batch.js';
import { Water } from '../src/water.js';
import { Settings } from '../src/settings.js';
import { zeroMaterial, worldMaterial, lighting } from '../src/materials.js';
import { plainProgram } from '../src/rust-render/post.js';
import { writeScreenStep } from '../src/rust-render/screen.js';
import { ownWorldMaterial } from '../src/rust-render/world-programs.js';
import { programLayouts } from '../src/render-programs.js';

// All pixels, meshes and textures here are generated. No game loader runs.
export async function checkRenderer(backend, manifests) {
  const results = [], draws = {}, links = [], shaderFamilies = [], compiledShaders = [], programKinds = new Set(), ownedIds = new Map(), ownedDraws = new Set();
  const assert = (name, ok, detail) => {
    results.push({ name, ok: Boolean(ok), detail });
  };
  // Check actual driver link results and errors immediately after each draw,
  // including calls through WASM. Counters prevent a three-only false pass.
  const proto = WebGL2RenderingContext.prototype;
  const getExtension = proto.getExtension;
  let multiDraws = 0, alphaCoverageDraws = 0;const alphaSamples=new Set();
  const fallback = new URLSearchParams(location.search).has('noMulti');
  proto.getExtension = function (name) {
    if (name === 'WEBGL_multi_draw' && fallback) return null;
    const ext = getExtension.call(this, name);
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
  let compilingKey=null;
  for (const name of ['compileShader', 'linkProgram', 'drawElements', 'drawArrays', 'drawElementsInstanced', 'drawArraysInstanced']) {
    const original = proto[name]; originals.set(name, original);
    proto[name] = function (...args) {
      const result = original.apply(this, args);
      if(name.startsWith('draw') && this.isEnabled(this.SAMPLE_ALPHA_TO_COVERAGE) && this.getParameter(this.SAMPLES)>1){alphaCoverageDraws++;alphaSamples.add(this.getParameter(this.SAMPLES));}
      if(name==='compileShader'){
        const ok=this.getShaderParameter(args[0],this.COMPILE_STATUS);
        compiledShaders.push({key:compilingKey,ok});
        if(!ok)throw Error(`${compilingKey || 'fixture'}: shader compile: ${this.getShaderInfoLog(args[0])}`);
      } else if (name === 'linkProgram') {
        const ok = this.getProgramParameter(args[0], this.LINK_STATUS);
        links.push(ok);
        if (!ok) throw new Error(`${compilingKey || "fixture"}: program link: ${this.getProgramInfoLog(args[0])}`);
      } else {
        if(name.startsWith('draw')) draws[name] = (draws[name] ?? 0) + 1;
        const error = this.getError();
        if (error && error !== this.CONTEXT_LOST_WEBGL) throw new Error(`${name}: WebGL 0x${error.toString(16)}`);
      }
      return result;
    };
  }
  const settings = new Settings({ shadows: 'low', waterQuality: 'low', antialiasing: '2x', bloom: false, softFocus: false, renderScale: 1, upscaler: 'off' });
  const g = new Graphics(document.body, settings);
  try {
    await g.rustReady;
    assert('Rust WASM initialized', Boolean(g.rust), g.rust.errors);
    const gl = g.renderer.getContext(), debug = gl.getExtension('WEBGL_debug_renderer_info');
    const renderer = debug && gl.getParameter(debug.UNMASKED_RENDERER_WEBGL);
    assert('Requested ANGLE backend selected', backend === 'warp' ? /Direct3D11/i.test(renderer) && /Microsoft|WARP|Basic Render/i.test(renderer) : backend === 'gl' ? /ANGLE.*OpenGL/i.test(renderer) : /SwiftShader/i.test(renderer), renderer);
    // Every family is discovered by the launcher from its manifest at runtime.
    // Register before fixture draws, so the Rust cache is cold for every key.
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
    if(shaderFamilies.some(f=>f.variants.some(v=>!v.accepted)))return {ok:false,backend,renderer,results,shaderFamilies,compiledShaders};

    const texture = (rgba) => {
      const t = new THREE.DataTexture(new Uint8Array(rgba), 1, 1);
      t.needsUpdate = true; return t;
    };
    const white = texture([200, 150, 90, 255]), normal = texture([128, 128, 255, 100]);
    // A material can appear before a lazy image has uploaded. Its Rust id
    // must survive the transition from the transparent sample to real pixels.
    const pendingImage = new THREE.DataTexture(new Uint8Array([80,130,200,255]),1,1);
    g.setMap({ post: { blur: { enabled: true, blend: .25, downsize: .25 } }, lighting: { sunDirection: [.4, -1, -.2], ambient: [70, 70, 70], sunColor: [180, 180, 180] }, distance: { nearScene: [40, 40, 80, 80] } });
    g.camera.position.set(0, 0, 8); g.camera.lookAt(0, 0, 0);
    const geometry = new THREE.PlaneGeometry(1.2, 1.2); geometry.setAttribute('tangent', new THREE.Float32BufferAttribute([1,0,0,1,0,0,1,0,0,1,0,0],3)); geometry.setAttribute('binormal', new THREE.Float32BufferAttribute([0,1,0,0,1,0,0,1,0,0,1,0],3));
    // Match a captured map light-count variant with zero-intensity generated lights.
    for(let i=0;i<15;i++)g.scene.add(new THREE.PointLight(0,0,1));
    const material = worldMaterial('lit', { map: white, bumpMap: normal, specular: [20, .3, .3, .3] });
    material.defines.WORLD_SPACE = '';
    ownWorldMaterial(material,geometry);
    // Static buckets must have identity transforms; bake the placements.
    for (let i = 0; i < 3; i++) {
      const mesh = new THREE.Mesh(geometry.clone().translate(i * 1.5 - 1.5, 0, 0), material);
      mesh.matrixAutoUpdate = false; g.scene.add(mesh);
    }
    // This raised caster projects onto the visible right terrain run.
    const raised=new THREE.Mesh(geometry.clone().scale(.6,.6,1).translate(1.5,-.4,.25),material);
    raised.matrixAutoUpdate=false;g.scene.add(raised);
    const pendingMaterial=worldMaterial('unlit',{map:pendingImage,lights:false});pendingMaterial.defines.WORLD_SPACE='';ownWorldMaterial(pendingMaterial);
    const pendingMesh=new THREE.Mesh(new THREE.PlaneGeometry(.7,.7).translate(-2,2,1),pendingMaterial);
    pendingMesh.matrixAutoUpdate=false;g.scene.add(pendingMesh);
    const skinGeometry = new THREE.PlaneGeometry(.7, .7);
    skinGeometry.setAttribute('skinIndex', new THREE.Uint16BufferAttribute(new Uint16Array(16),4));
    skinGeometry.setAttribute('skinWeight', new THREE.Float32BufferAttribute([1,0,0,0,1,0,0,0,1,0,0,0,1,0,0,0],4));
    const skinMaterial = zeroMaterial('dynamic', { map: white }); skinMaterial.defines.RIGID_SKIN = '';
    const body = new THREE.SkinnedMesh(skinGeometry, skinMaterial), bone = new THREE.Bone();
    body.add(bone); body.bind(new THREE.Skeleton([bone])); body.position.set(1.5,1,0); body.frustumCulled = false;
    const root = new THREE.Group(); root.userData.parts = { body }; root.add(body); g.scene.add(root); g.cullRoots.push(root);
    const farMaterial=worldMaterial('unlit',{map:texture([20,40,80,255]),far:true,lights:false});farMaterial.defines.WORLD_SPACE='';ownWorldMaterial(farMaterial);
    const far = new THREE.Mesh(new THREE.PlaneGeometry(20,20).translate(0,0,-90),farMaterial);far.matrixAutoUpdate=false;g.farScene.add(far);
    const transparent = new THREE.Mesh(new THREE.PlaneGeometry(.7, .7), new THREE.MeshBasicMaterial({ color: 0x2277aa, transparent: true, opacity: .5 }));
    transparent.position.set(0, 1, 1); g.scene.add(transparent);
    const view = new THREE.Mesh(new THREE.PlaneGeometry(.12, .12), zeroMaterial('dynamic', { map: white }));
    view.position.set(.2, -.15, -1); g.viewScene.add(view);
    // Three terrain cells: the invisible middle one splits visible runs,
    // requiring multi-draw rather than one contiguous range.
    const groundGeometry = new THREE.BufferGeometry();
    groundGeometry.setAttribute('position', new THREE.Float32BufferAttribute([-3,-2,0,-1,-2,0,-3,-1,0, 99,99,0,100,99,0,99,100,0, 1,-2,0,3,-2,0,3,-1,0], 3));
    groundGeometry.setAttribute('uv', new THREE.Float32BufferAttribute(new Float32Array(18), 2));
    groundGeometry.setAttribute('normal', new THREE.Float32BufferAttribute(Array(9).fill([0,0,1]).flat(), 3));
    groundGeometry.setAttribute('weights0', new THREE.Float32BufferAttribute(Array(9).fill([.5,0,.5,0]).flat(),4));
    groundGeometry.setIndex([0,1,2,3,4,5,6,7,8]);
    groundGeometry.setAttribute('color',new THREE.Float32BufferAttribute(Array(9).fill([1,1,1]).flat(),3));
    groundGeometry.setAttribute('weights1',new THREE.Float32BufferAttribute(new Float32Array(36),4));
    const groundMaterial=worldMaterial('terrain',{map:white});
    Object.assign(groundMaterial.defines,{WORLD_SPACE:'',TERRAIN_LAYERS:6});
    Object.assign(groundMaterial.uniforms,{layerMap0:{value:white},layerMap1:{value:null},layerMap2:{value:texture([100,180,60,255])},layerMap3:{value:null},layerMap4:{value:null},layerMap5:{value:null}});
    ownWorldMaterial(groundMaterial,groundGeometry);
    const ground = new THREE.Group(),groundMesh=new THREE.Mesh(groundGeometry, groundMaterial);groundMesh.receiveShadow=true;ground.add(groundMesh);ground.material = groundMaterial;
    const boxes = [[-3,-2,0,-1,-1,0], [99,99,0,100,100,0], [1,-2,0,3,-1,0]].map(b => new THREE.Box3(new THREE.Vector3(...b.slice(0,3)),new THREE.Vector3(...b.slice(3))));
    ground.userData.patches = { full: { index: groundGeometry.index, cells: boxes.map((box,i) => ({ start: i*3, end: i*3+3, box })) }, bounds: new THREE.Sphere(new THREE.Vector3(), 200) };
    g.scene.add(ground);
    const cutoutMap=new THREE.DataTexture(new Uint8Array([220,80,40,128,220,80,40,255,220,80,40,0,220,80,40,192]),2,2);cutoutMap.needsUpdate=true;
    const cutoutMaterial=worldMaterial('unlit',{map:cutoutMap,lights:false,alphaTest:true,doubleSided:true});cutoutMaterial.defines.WORLD_SPACE='';
    const cutoutGeometry=new THREE.PlaneGeometry(.8,.8).translate(-1.5,-1,1);ownWorldMaterial(cutoutMaterial,cutoutGeometry);
    const cutout=new THREE.Mesh(cutoutGeometry,cutoutMaterial);cutout.matrixAutoUpdate=false;g.scene.add(cutout);
    const particles = new ParticleRenderer(g.scene).batch(white, 'additive');
    particles.begin(); particles.push(-1, 1, .5, 1, .4, .2, 1, .3); particles.end();
    const batch = new Batch(), hudScene = new THREE.Scene(), hudCamera = new THREE.OrthographicCamera(0,320,0,240,-10,10);
    batch.begin(); batch.quad(new THREE.Matrix4(), 12, 12, 48, 40, white, [0,0,1,1], [1,1,1,1]); batch.quad(new THREE.Matrix4(),88,12,120,40,pendingImage,[0,0,1,1],[1,1,1,1]); batch.end(); hudScene.add(batch.mesh);
    const hud = { batch, scene: hudScene, camera: hudCamera };
    g.treeCanopies = new TreeCanopies(g, { texture: 'leaves', instances: [{ matrix: new THREE.Matrix4().makeTranslation(-6,2,-10).elements, bounds: [[-3,-3,-1],[3,3,1]] }] }, () => white);
    const foliageBatch = new FoliageGpuBatch(white, [{ x:0,y:0,z:0,width:.4,height:.8,grey:1,part:0,phase:0,rate:1 }], { Alpha: 1 }, true);
    const instances = [{ position: new THREE.Vector3(2,-1,1), matrix: new THREE.Matrix4(), tint: [1,1,1], sphere: null, cullDistance: Infinity, maxDistance: 100 }];
    const foliage = { kinds: new Map([[0,{ batch: foliageBatch, grass: true, instances }]]), nearEnd: () => 100, time: 0, drawer: null };
    g.scene.userData.foliage = foliage;
    for (const mesh of g.scene.children) if (mesh.isMesh && mesh.material === material) { mesh.receiveShadow = true; g.caster(mesh); }
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
    g.rust.rust = new Proxy(rust, { get(target, name) {
      const fn = target[name];
      if (typeof fn !== 'function') return fn;
      return (...args) => {
        if (name === 'add_program' || name === 'add_program_lamps') programKinds.add(args[0]);
        if(name==='post_frame')for(let i=0;i<args[0];i++)ownedDraws.add(g.rust.postInput[i*(5+12*18)]);
        if(name==='hud_frame')ownedDraws.add(g.rust.hudInput[1]);
        if (name === 'frame') { gl.activeTexture(gl.TEXTURE7); gl.bindTexture(gl.TEXTURE_2D, poison); }
        const value = fn.apply(target, args);
        if(name==='add_owned_program')ownedIds.set(args[0],value);
        if (/frame$|_segment$|near_begin$/.test(name) && value?.length) {
          phaseDraws[name] = (phaseDraws[name] ?? 0) + value[3];
          for (let i = 0; i < passDraws.length; i++) passDraws[i] += value[5+i] ?? 0;
          const error = gl.getError(); if (error && error !== this.CONTEXT_LOST_WEBGL) throw new Error(`${name}: WebGL 0x${error.toString(16)}`);
        }
        return value;
      };
    } });
    g.warmup();
    const capture = () => {
      g.camera.updateMatrixWorld(true);
      if (foliage.drawer) foliage.drawer.select(foliage,g.camera);
      g.render(0);
      g.renderer.setRenderTarget(null); g.renderer.autoClear = false;
      g.rust.coverHud(g.renderer, hud);
      g.renderer.autoClear = true;
      if (g.rust.errors.length || g.renderer.battleThreeCalls) throw new Error(`Rust fallback: ${JSON.stringify(g.rust.errors)}`);
      const pixels = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4);
      gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      const error = gl.getError(); if (error) throw new Error(`Frame: WebGL 0x${error.toString(16)}`);
      return pixels;
    };
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
    const baseline=compare('opaque, far, terrain, transparent, view and composite');
    lighting.shadowStrength.value=0;const unshadowed=capture();lighting.shadowStrength.value=1;const shadowed=capture();
    const lo=new THREE.Vector3(1.8,-1.9,0).project(g.camera),hi=new THREE.Vector3(2.2,-1.75,0).project(g.camera);
    let terrainShadowPixels=0;
    for(let y=Math.ceil((lo.y+1)*.5*gl.drawingBufferHeight);y<Math.floor((hi.y+1)*.5*gl.drawingBufferHeight);y++)for(let x=Math.ceil((lo.x+1)*.5*gl.drawingBufferWidth);x<Math.floor((hi.x+1)*.5*gl.drawingBufferWidth);x++){
      const at=4*(y*gl.drawingBufferWidth+x);if([0,1,2].some(k=>shadowed[at+k]!==unshadowed[at+k]))terrainShadowPixels++;
    }
    assert('Multi-draw terrain receives an occluding mesh shadow',terrainShadowPixels>0,{terrainShadowPixels});
    lighting.bump.value = 0; compare('bump switched off'); lighting.bump.value = 1;
    settings.set('bloom', true); settings.set('softFocus', true); g.setVehicleMotion(.8,.2); const effects=compare('bloom, soft focus and speed blur');
    assert('Post settings change visible pixels',effects.some((v,i)=>i%4!==3&&v!==baseline[i]));
    settings.set('bloom', false); settings.set('softFocus', false); g.setVehicleMotion(0,0); const plain=compare('post effects switched off');
    assert('Disabling post effects reproduces the plain frame',plain.every((v,i)=>v===baseline[i]));
    settings.set('antialiasing','fxaa'); compare('FXAA');
    settings.set('antialiasing','fxaa-fast'); compare('fast FXAA');
    settings.set('antialiasing','off');
    settings.set('renderScale', .75);
    settings.set('upscaler','spatial'); compare('spatial upscale');
    // FXAA forces the explicit bilinear pass rather than the direct composite.
    settings.set('antialiasing','fxaa');
    settings.set('upscaler','bilinear'); compare('bilinear upscale with FXAA');
    settings.set('antialiasing','off');
    settings.set('upscaler','temporal');
    assert('Temporal targets supported',g.upscaling==='temporal');
    const temporalSequence=()=>{
      g.upscaler.invalidate();g.upscaler.frame=0;g.upscaler.read=0;
      let pixels;for(let i=0;i<8;i++)pixels=capture();return pixels;
    };
    const temporalA=temporalSequence(),temporalB=temporalSequence();
    assert('Temporal post sequence repeats exactly',temporalA.every((v,i)=>v===temporalB[i]));
    settings.set('upscaler','off');settings.set('renderScale',1);
    settings.set('shadows','off');compare('shadows switched off');
    settings.set('shadows','medium');compare('three cascade shadow receivers');
    settings.set('shadows','high');compare('four cascade shadow receivers');
    settings.set('shadows','low');compare('shadows restored');
    settings.set('lampLighting',true);compare('lamps enabled');
    settings.set('lampLighting',false);compare('lamps disabled');
    assert('World draws taken by Rust', g.rust.byName.world.count >= 3, g.rust.byName.world.count);
    assert('World materials have no browser shader program',[material,pendingMaterial,farMaterial,groundMaterial,cutoutMaterial].every(m=>m.vertexShader===undefined&&!g.renderer.properties.get(m).currentProgram));
    assert('Alpha-to-coverage drew on a multisampled target',alphaCoverageDraws>0,{alphaCoverageDraws,samples:[...alphaSamples]});
    assert('Unpainted terrain taken by Rust', groundMaterial.visible === false);
    for (const name of ['frame', 'far_frame', 'transparent_frame', 'view_frame', 'post_frame']) assert(`${name} issues Rust programs`, phaseDraws[name] > 0, phaseDraws);
    // Motion has its own target and depth input, separate from post history.
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
    const water = new Water({ data: { height:-2,cell:12,offset:[6,6],grid:1,cells:[[0,0]],look:{PatchDivisions:[2]} }, texture: () => null, graphics: g });
    g.scene.userData.water = water;
    water.update(0,g.camera);
    compare('water surface');
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
    tintScene.add(new THREE.Mesh(new THREE.PlaneGeometry(2,2), new THREE.ShaderMaterial({
      uniforms:{color:{value:new THREE.Vector4(.1,.2,.3,.5)}},
      vertexShader:'void main(){gl_Position=vec4(position,1.0);}',
      fragmentShader:'uniform vec4 color; void main(){gl_FragColor=color;}',
      transparent:true,depthTest:false,depthWrite:false,
    })));
    tintScene.children[0].frustumCulled=false;
    g.renderer.compile(tintScene,tintCamera);
    g.renderer.autoClear=false;
    g.rust.cover('underwater',g.renderer,tintScene,tintCamera);
    g.renderer.autoClear=true;
    // Overlapping partial writes of different lengths must survive restoration.
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
    const retainedPixels=()=>{
      g.renderer.setRenderTarget(retainedTarget);g.renderer.clear();
      const target=g.rust.byName.post.target(retainedTarget);
      writeScreenStep(g.rust,0,retainedProgram,retainedMaterial,retainedMesh,tintCamera,{target,size:[32,32],geometry:{id:retainedId,count:6}});
      g.rust.rust.screen_frame(1);g.rust.stale=true;
      const pixels=new Uint8Array(32*32*4);g.renderer.readRenderTargetPixels(retainedTarget,0,0,32,32,pixels);return pixels;
    };
    const retainedBefore=retainedPixels();
    assert('Partial-update fixture draws visible pixels',retainedBefore.some((v,i)=>i%4===0&&v===255));
    const beforeLoss = capture();
    const loss = gl.getExtension('WEBGL_lose_context');
    if (!loss) throw new Error('Context-loss extension unavailable');
    const canvas = g.renderer.domElement;
    console.log("renderer smoke: context loss");
    const lost = new Promise(resolve => canvas.addEventListener('webglcontextlost', resolve, { once: true }));
    loss.loseContext();
    await lost;
    await new Promise(resolve => setTimeout(resolve, 100));
    const restored = new Promise((resolve,reject) => {
      const timer=setTimeout(()=>reject(new Error(`Context restore timeout: ${g.rust.failure?.stack || "no renderer error"}`)),30000);
      canvas.addEventListener("webglcontextrestored",()=>{clearTimeout(timer);resolve();},{once:true});
    });
    loss.restoreContext();
    await restored;
    {const error=gl.getError();if(error)throw Error(`After restore: GL ${error}`);}
    poison = makePoison();
    {const error=gl.getError();if(error)throw Error(`After poison: GL ${error}`);}
    g.renderer.resetState();
    for (let i=0;i<4;i++) capture();
    const afterLoss = capture();
    assert('Restored Rust frame matches its generated fixture', beforeLoss.every((v,i) => v === afterLoss[i]));
    const retainedAfter=retainedPixels();
    assert('Overlapping vertex and index updates survive restoration',retainedBefore.every((v,i)=>v===retainedAfter[i]));
    retainedTarget.dispose();
    assert('Battle never invokes three draw', g.renderer.battleThreeCalls === 0);
    assert('Every shader rewrite kind links', [0,1,2,3,4,9].every(k => programKinds.has(k)), [...programKinds]);
    assert('All Rust passes draw', passDraws.every(n => n > 0), { names: ['world','instances','far','terrain','transparent','effects','foliage','view','post','hud','water','shadow','motion','canopies','screen'], counts: passDraws });
    assert('Multi-draw path exercised', fallback ? multiDraws === 0 : multiDraws > 0, { fallback, multiDraws });
    assert('Every declared Rust post and HUD program draws',Object.keys(programLayouts).filter(key=>key.startsWith('post-hud/')).every(key=>ownedIds.has(key)&&ownedDraws.has(ownedIds.get(key))), {registered:[...ownedIds.keys()],drawn:[...ownedDraws]});
    assert('Every compiled program links', links.length > 0 && links.every(Boolean), { count: links.length });
    return { ok: results.every(r => r.ok), backend, fallback, multiDraws, renderer, results, shaderFamilies, compiledShaders, draws, passDraws, phaseDraws, programKinds: [...programKinds], linkedPrograms: links.length };
  } catch (error) {
    const stopped=g.rust.byName.near.stopped;
    throw new Error(`${error.stack}\nCause: ${error.cause?.stack || "none"}\nStopped: ${JSON.stringify(stopped && {name:stopped.name,material:stopped.material?.name,uniforms:Object.keys(stopped.material?.uniforms||{}),background:stopped===g.background,layer:stopped.layers.mask})}`);
  } finally {
    proto.getExtension = getExtension;
    for (const [name, original] of originals) proto[name] = original;
    g.renderer.dispose();
  }
}
