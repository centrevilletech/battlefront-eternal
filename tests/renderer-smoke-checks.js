import * as THREE from 'three';
import { Graphics } from '../src/graphics.js';
import { ParticleRenderer } from '../src/particles.js';
import { TreeCanopies } from '../src/tree-canopies.js';
import { FoliageGpuBatch } from '../src/foliage-gpu.js';
import { motionVectors } from '../src/motion-vectors.js';
import { Batch } from '../src/interface/batch.js';
import { Water } from '../src/water.js';
import { Settings } from '../src/settings.js';
import { zeroMaterial, lighting } from '../src/materials.js';

// All pixels, meshes and textures here are generated. No game loader runs.
export async function checkRenderer(backend) {
  const results = [], draws = {}, links = [], programKinds = new Set();
  const assert = (name, ok, detail) => {
    results.push({ name, ok: Boolean(ok), detail });
  };
  // Check actual driver link results and errors immediately after each draw,
  // including calls through WASM. Counters prevent a three-only false pass.
  const proto = WebGL2RenderingContext.prototype;
  const getExtension = proto.getExtension;
  let multiDraws = 0;
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
  for (const name of ['linkProgram', 'drawElements', 'drawArrays', 'drawElementsInstanced', 'drawArraysInstanced']) {
    const original = proto[name]; originals.set(name, original);
    proto[name] = function (...args) {
      const result = original.apply(this, args);
      if (name === 'linkProgram') {
        const ok = this.getProgramParameter(args[0], this.LINK_STATUS);
        links.push(ok);
        if (!ok) throw new Error(`Program link: ${this.getProgramInfoLog(args[0])}`);
      } else {
        draws[name] = (draws[name] ?? 0) + 1;
        const error = this.getError();
        if (error) throw new Error(`${name}: WebGL 0x${error.toString(16)}`);
      }
      return result;
    };
  }
  const settings = new Settings({ shadows: 'low', waterQuality: 'low', antialiasing: 'off', bloom: false, softFocus: false, renderScale: 1, upscaler: 'off' });
  const g = new Graphics(document.body, settings);
  try {
    await g.rustReady;
    assert('Rust WASM initialized', g.rust?.enabled, g.rust?.errors);
    const gl = g.renderer.getContext(), debug = gl.getExtension('WEBGL_debug_renderer_info');
    const renderer = debug && gl.getParameter(debug.UNMASKED_RENDERER_WEBGL);
    assert('Requested ANGLE backend selected', backend === 'warp' ? /Direct3D11/i.test(renderer) && /Microsoft|WARP|Basic Render/i.test(renderer) : backend === 'gl' ? /ANGLE.*OpenGL/i.test(renderer) : /SwiftShader/i.test(renderer), renderer);
    const texture = (rgba) => {
      const t = new THREE.DataTexture(new Uint8Array(rgba), 1, 1);
      t.needsUpdate = true; return t;
    };
    const white = texture([200, 150, 90, 255]), normal = texture([128, 128, 255, 100]);
    g.setMap({ post: { blur: { enabled: true, blend: .25, downsize: .25 } }, lighting: { sunDirection: [.4, -1, .2], ambient: [70, 70, 70], sunColor: [180, 180, 180] }, distance: { nearScene: [40, 40, 80, 80] } });
    g.camera.position.set(0, 0, 8); g.camera.lookAt(0, 0, 0);
    const geometry = new THREE.PlaneGeometry(1.2, 1.2); geometry.setAttribute('tangent', new THREE.Float32BufferAttribute([1,0,0,1,0,0,1,0,0,1,0,0],3)); geometry.setAttribute('binormal', new THREE.Float32BufferAttribute([0,1,0,0,1,0,0,1,0,0,1,0],3));
    const material = zeroMaterial('lit', { map: white, bumpMap: normal, specular: [20, .3, .3, .3] });
    material.defines.WORLD_SPACE = '';
    // Static buckets must have identity transforms; bake the placements.
    for (let i = 0; i < 3; i++) {
      const mesh = new THREE.Mesh(geometry.clone().translate(i * 1.5 - 1.5, 0, 0), material);
      mesh.matrixAutoUpdate = false; g.scene.add(mesh);
    }
    const skinGeometry = new THREE.PlaneGeometry(.7, .7);
    skinGeometry.setAttribute('skinIndex', new THREE.Uint16BufferAttribute(new Uint16Array(16),4));
    skinGeometry.setAttribute('skinWeight', new THREE.Float32BufferAttribute([1,0,0,0,1,0,0,0,1,0,0,0,1,0,0,0],4));
    const skinMaterial = zeroMaterial('dynamic', { map: white }); skinMaterial.defines.RIGID_SKIN = '';
    const body = new THREE.SkinnedMesh(skinGeometry, skinMaterial), bone = new THREE.Bone();
    body.add(bone); body.bind(new THREE.Skeleton([bone])); body.position.set(1.5,1,0); body.frustumCulled = false;
    const root = new THREE.Group(); root.userData.parts = { body }; root.add(body); g.scene.add(root); g.cullRoots.push(root);
    const far = new THREE.Mesh(new THREE.PlaneGeometry(20, 20), zeroMaterial('unlit', { map: texture([20, 40, 80, 255]), far: true }));
    far.position.z = -90; g.farScene.add(far);
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
    const groundMaterial = new THREE.ShaderMaterial({ defines: { TERRAIN: '', WORLD_SPACE: '', TERRAIN_LAYERS: 3 }, uniforms: { ...lighting,
      layerMap0: { value: white }, layerMap1: { value: null }, layerMap2: { value: texture([100, 180, 60, 255]) },
    }, vertexShader: 'attribute vec4 weights0; varying vec3 paint; varying vec2 vUv; void main(){paint=weights0.xyz; vUv=uv; gl_Position=projectionMatrix*viewMatrix*vec4(position,1.0);}',
    // A zero weight still leaves the middle sampler active, just as an
    // unpainted game layer does. Validation precedes fragment execution.
    fragmentShader: 'uniform sampler2D layerMap0; uniform sampler2D layerMap1; uniform sampler2D layerMap2; varying vec3 paint; varying vec2 vUv; void main(){vec4 a=texture2D(layerMap0,vUv); vec4 b=texture2D(layerMap1,vUv); vec4 c=texture2D(layerMap2,vUv); gl_FragColor=a*paint.x+b*paint.y+c*paint.z;}' });
    const ground = new THREE.Group(); ground.add(new THREE.Mesh(groundGeometry, groundMaterial)); ground.material = groundMaterial;
    const boxes = [[-3,-2,0,-1,-1,0], [99,99,0,100,100,0], [1,-2,0,3,-1,0]].map(b => new THREE.Box3(new THREE.Vector3(...b.slice(0,3)),new THREE.Vector3(...b.slice(3))));
    ground.userData.patches = { full: { index: groundGeometry.index, cells: boxes.map((box,i) => ({ start: i*3, end: i*3+3, box })) }, bounds: new THREE.Sphere(new THREE.Vector3(), 200) };
    g.scene.add(ground);
    const particles = new ParticleRenderer(g.scene).batch(white, 'additive');
    particles.begin(); particles.push(-1, 1, .5, 1, .4, .2, 1, .3); particles.end();
    const batch = new Batch(), hudScene = new THREE.Scene(), hudCamera = new THREE.OrthographicCamera(0,320,240,0,-10,10);
    batch.begin(); batch.quad(new THREE.Matrix4(), 12, 12, 48, 40, white, [0,0,1,1], [1,1,1,1]); batch.end(); hudScene.add(batch.mesh);
    const hud = { batch, scene: hudScene, camera: hudCamera };
    g.treeCanopies = new TreeCanopies(g, { texture: 'leaves', instances: [{ matrix: new THREE.Matrix4().makeTranslation(-6,2,-10).elements, bounds: [[-3,-3,-1],[3,3,1]] }] }, () => white);
    const foliageBatch = new FoliageGpuBatch(g.scene, white, [{ x:0,y:0,z:0,width:.4,height:.8,grey:1,part:0,phase:0,rate:1 }], { Alpha: 1 }, true, 1);
    const instances = [{ position: new THREE.Vector3(2,-1,1), matrix: new THREE.Matrix4(), tint: [1,1,1], sphere: null, cullDistance: Infinity, maxDistance: 100 }];
    const foliage = { kinds: new Map([[0,{ batch: foliageBatch, grass: true, instances }]]), nearEnd: () => 100, time: 0, drawer: null };
    g.scene.userData.foliage = foliage;
    for (const mesh of g.scene.children) if (mesh.isMesh && mesh.material === material) { mesh.receiveShadow = true; g.caster(mesh); }
    body.receiveShadow = true; g.caster(body);
    const phaseDraws = {}, passDraws = Array(15).fill(0);
    // Seed the formerly unpainted unit with a comparison texture. A missing
    // rebind must reject the whole terrain draw, not merely shade it wrong.
    const poison = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, poison);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.DEPTH_COMPONENT16, 1, 1, 0, gl.DEPTH_COMPONENT, gl.UNSIGNED_SHORT, null);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_COMPARE_MODE, gl.COMPARE_REF_TO_TEXTURE);
    g.renderer.resetState();
    const rust = g.rust.rust;
    g.rust.rust = new Proxy(rust, { get(target, name) {
      const fn = target[name];
      if (typeof fn !== 'function') return fn;
      return (...args) => {
        if (name === 'add_program' || name === 'add_program_lamps') programKinds.add(args[0]);
        if (name === 'frame') { gl.activeTexture(gl.TEXTURE7); gl.bindTexture(gl.TEXTURE_2D, poison); }
        const value = fn.apply(target, args);
        if (/frame$|_segment$|near_begin$/.test(name) && value?.length) {
          phaseDraws[name] = (phaseDraws[name] ?? 0) + value[3];
          for (let i = 0; i < passDraws.length; i++) passDraws[i] += value[5+i] ?? 0;
          const error = gl.getError(); if (error) throw new Error(`${name}: WebGL 0x${error.toString(16)}`);
        }
        return value;
      };
    } });
    let waterDelegated = false;
    const capture = (on) => {
      g.rust.setActive(on);
      if (on && waterDelegated) g.rust.byName.water.setActive(false);
      g.camera.updateMatrixWorld(true);
      const frustum = new THREE.Frustum().setFromProjectionMatrix(new THREE.Matrix4().multiplyMatrices(g.camera.projectionMatrix,g.camera.matrixWorldInverse));
      if (on && foliage.drawer) foliage.drawer.select(foliage,g.camera);
      else foliageBatch.update(instances,0,g.camera,new THREE.Vector3(1,0,0),new THREE.Vector3(0,1,0),new THREE.Vector3(0,0,1),frustum,100);

      g.render(0);
      g.renderer.setRenderTarget(null); g.renderer.autoClear = false;
      if (!g.rust.coverHud(g.renderer, hud)) g.renderer.render(hudScene, hudCamera);
      g.renderer.autoClear = true;
      if (!g.rust.enabled || g.rust.errors.length) throw new Error(`Rust fallback: ${JSON.stringify(g.rust.errors)}`);
      const pixels = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4);
      gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      const error = gl.getError(); if (error) throw new Error(`Frame: WebGL 0x${error.toString(16)}`);
      return pixels;
    };
    const compare = (name) => {
      // Warm both owners before comparing, then alternate on one context.
      for (let i = 0; i < 4; i++) capture(true);
      for (let i = 0; i < 3; i++) capture(false);
      const a = capture(false), a2 = capture(false);
      for (let i = 0; i < 3; i++) capture(true);
      const b = capture(true), b2 = capture(true);
      assert(`${name}: stable three frame`, a.every((v,i) => v === a2[i]));
      assert(`${name}: stable Rust frame`, b.every((v,i) => v === b2[i]));
      let max = 0, changed = 0, visible = 0;
      for (let i = 0; i < a.length; i += 4) {
        let delta = 0;
        for (let k = 0; k < 3; k++) delta = Math.max(delta, Math.abs(a[i+k] - b[i+k]));
        max = Math.max(max, delta); if (delta > 2) changed++;
        if (a[i] > 40 || a[i+1] > 40 || a[i+2] > 90) visible++;
      }
      assert(`${name}: visible fixture`, visible > 100, { visible });
      assert(`${name}: three/Rust pixels`, changed === 0, { max, changed, pixels: a.length / 4 });
    };
    compare('opaque, far, terrain, transparent, view and composite');
    lighting.bump.value = 0; compare('bump switched off'); lighting.bump.value = 1;
    settings.set('bloom', true); settings.set('softFocus', true); g.setVehicleMotion(.8,.2); compare('bloom, soft focus and speed blur');
    settings.set('bloom', false); settings.set('softFocus', false); g.setVehicleMotion(0,0); compare('post effects switched off');
    settings.set('antialiasing','fxaa'); compare('FXAA');
    settings.set('antialiasing','fxaa-fast'); compare('fast FXAA');
    settings.set('antialiasing','off');
    assert('World draws taken by Rust', g.rust.byName.world.count >= 3, g.rust.byName.world.count);
    assert('Unpainted terrain taken by Rust', groundMaterial.visible === false);
    for (const name of ['frame', 'far_frame', 'transparent_frame', 'view_frame', 'post_frame']) assert(`${name} issues Rust programs`, phaseDraws[name] > 0, phaseDraws);
    // Motion has its own target and depth input, separate from post history.
    const proxy = motionVectors.track(body);
    const motionTarget = new THREE.WebGLRenderTarget(64,64,{type:THREE.HalfFloatType,depthBuffer:false});
    const depthValues = new Float32Array(64*64).fill((g.camera.far - g.camera.near*g.camera.far/8)/(g.camera.far-g.camera.near));
    const depth = new THREE.DataTexture(depthValues,64,64,THREE.RedFormat,THREE.FloatType); depth.needsUpdate = true;
    const current = new THREE.Matrix4().multiplyMatrices(g.camera.projectionMatrix,g.camera.matrixWorldInverse);
    const previous = current.clone(); previous.elements[12] += .02;
    const motion = (on) => {
      g.rust.setActive(on);
      motionVectors.render(g.renderer,{target:motionTarget,root:g.scene,camera:g.camera,viewScene:g.viewScene,view:g.viewCamera,depth,current,previous});
      const pixels = new Uint16Array(64*64*4); g.renderer.readRenderTargetPixels(motionTarget,0,0,64,64,pixels); return pixels;
    };
    motion(false); motion(true);
    const motionThree = motion(false), motionRust = motion(true);
    assert('Motion proxy writes nonzero velocity', motionThree.some((v,i) => i % 4 === 0 && v !== 0));
    assert('Motion target matches three', motionThree.every((v,i) => v === motionRust[i]));
    motionVectors.release(proxy); motionTarget.dispose();
    // Water's far copies and transparent surface have their own programs,
    // checked after the other fixture's independent comparisons.
    const water = new Water({ data: { height:-2,cell:12,offset:[6,6],grid:1,cells:[[0,0]],look:{PatchDivisions:[2]} }, texture: () => null, graphics: g });
    g.scene.userData.water = water;
    water.update(0,g.camera);
    compare('water surface');
    waterDelegated = true;
    compare('water delegated to three');
    g.rust.byName.water.setActive(true);
    assert('Every shader rewrite kind links', [0,1,2,3,4,9].every(k => programKinds.has(k)), [...programKinds]);
    assert('All Rust passes draw', passDraws.every(n => n > 0), { names: ['world','instances','far','terrain','transparent','effects','foliage','view','post','hud','water','shadow','motion','canopies','screen'], counts: passDraws });
    assert('Multi-draw path exercised', fallback ? multiDraws === 0 : multiDraws > 0, { fallback, multiDraws });
    assert('Every compiled program links', links.length > 0 && links.every(Boolean), { count: links.length });
    return { ok: results.every(r => r.ok), backend, fallback, multiDraws, renderer, results, draws, passDraws, phaseDraws, programKinds: [...programKinds], linkedPrograms: links.length };
  } finally {
    proto.getExtension = getExtension;
    for (const [name, original] of originals) proto[name] = original;
    g.renderer.dispose();
  }
}
