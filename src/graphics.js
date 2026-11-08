import {sceneLayer,sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import { prepareActors } from "./rust-render/actors.js";
import { BattleCanvas, contextLimits } from "./rust-render/browser.js";
import { loadRustRenderer } from "./rust-render/load.js";
import * as THREE from "three";
import { ownedFullScreenPass } from "./render-passes.js";
import { ParticleDepth } from "./particle-depth.js";
import { FACTS as F, STATE as S, ROLE as R } from "./rust-render/frame-layout.js";
import { CAMERA_ROLE, shadowShells } from "./rust-render/cameras.js";
import { RenderProfiler } from "./render-profiler.js";
import { LocalLights } from "./local-lights.js";
import { boltLook } from "./blaster-bolts.js";
import { sniperLook } from "./sniper-trace.js";
import { UPSCALERS, Upscaler } from "./upscaler.js";
import { pageGpu, rendererName } from "./gpu-info.js";
import { lighting, setFog, setLighting } from "./materials.js";
import { ANTIALIASING, RENDER_SCALE_MIN, SHADOW_PRESETS, VIEW_DISTANCES } from "./settings.js";

// The frame, laid out like the 2004 game's. Everything is in gamma space, as
// Direct3D 9 drew it: textures are not decoded from sRGB, light adds and
// multiplies on the stored values, and nothing is converted on the way out.
//
//   1. The far scene: the sky dome around the camera, then every object's
//      low-detail model and the low-res terrain, in the far-scene fog.
//   2. The near scene over it, in full detail. It fades into the far scene's
//      image between the near scene's fade start and end (150 to 200 m on
//      Bespin at the game's longest view distance) and is cut off there. In
//      first person the weapon in view is drawn last, in a depth range of its
//      own, so it never sinks into walls.
//   3. The map's soft-focus blur: 70% the frame, 30% a quarter-size copy.
//      Optional bloom is added separately; FXAA, when selected, runs last.
//   4. With an upscaler (upscaler.js), steps 1-3 run at the render scale's
//      resolution and the result is rebuilt to the window's, where the HUD
//      is then drawn sharp (the bilinear one is the composite itself,
//      drawn to the window). Without one, the whole canvas shrinks instead.
//
// Shadows are this port's own: cascaded shadow maps of the sun, where the
// original had stencil shadow volumes. Each cascade is a hardware depth map
// (cascades.rs): the map's static casters are cached per cascade
// window, and only the moving casters are drawn each frame.
THREE.ColorManagement.enabled = false;

// The shadow pass's depth material: the transform and, for cut-out casters,
// the alpha test, and nothing else. Three's own writes a packed depth colour
// and samples every caster's texture; the hardware depth attachment is all
// the receivers read.

export class Graphics {
  constructor(container, settings) {
    this.settings = settings;
    this.renderer = new BattleCanvas();
    this.profiler = new RenderProfiler(this.renderer);
    // Which GPU draws, and whether Chromium has it drawing in software
    // (gpu-info.js): VIDEO OPTIONS and the stats line say so.
    pageGpu({ renderer: () => rendererName(this.renderer.getContext()) });
    this.renderer.setSize(window.innerWidth, window.innerHeight);
    container.appendChild(this.renderer.domElement);

    this.scene = new THREE.Scene();
    // Every renderer.render() would re-walk the scene and recompose every
    // matrix (thousands of bones), and water renders this scene twice a
    // frame. The native frame requests one world publication instead.
    sceneProperty(this.scene,"matrixWorldAutoUpdate",false);
    this.sceneFrame = -1;
    this.frameCount = 0;
    // Soldiers' roots (game.js, each frame) that the near scene's own pass
    // may leave out while out of view (cullOffView), and those left out.
    this.cullRoots = [];
    // The battle's native unit placement, given the scene about to be
    // updated: the deferred soldiers this update will reach are placed in
    // one call before the walk (game.js installs it).
    this.placeUnits = null;
    this.farScene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(60, window.innerWidth / window.innerHeight, 0.5, 200);
    this.farCamera = new THREE.PerspectiveCamera();
    // The first-person weapon and its own camera, which follows the eye.
    this.viewScene = new THREE.Scene();
    this.viewScene.userData.isViewScene = true;
    this.viewCamera = new THREE.PerspectiveCamera(60, window.innerWidth / window.innerHeight, 0.3, 10);
    // Layer 1 holds what casts shadows without being seen (the player's own
    // body in first person); layer 2 the map's static casters, drawn only
    // into the cascade caches.
    this.hiddenLayer = 1;
    this.staticLayer = 2;
    // Layer 4 holds what only the eye's own far camera draws, and not the
    // mirrors that render the far scene too (water.js's far water). Layer 3
    // is the sky probe's (sky-probe.js), 5 the water mirror's sky pass.
    this.eyeLayer = 4;
    sceneLayer(this.farCamera.layers,"enable",this.eyeLayer);
    this.shadowCamera = new THREE.PerspectiveCamera();
    this.staticShadowCamera = new THREE.PerspectiveCamera();
    this.cascadeCamera = new THREE.PerspectiveCamera();
    this.cascadeWindows = [];
    this.shadowCaches = [];
    this.shadowMaps = [];
    this.shadowPasses = { static: 0, dynamic: 0 };
    const limits = contextLimits(this.renderer);
    this.maxAnisotropy = Math.max(1, limits.maxAnisotropy);
    this.supportedSamples = limits.samples;
    this.textures = new Set();
    this.textureRevision = 0;
    this.casters = new Set();
    this.distantCasters = new Set();
    this.casterAt = new THREE.Vector3();
    this.depthMaterials = new Map();
    this.localLights = new LocalLights();
    this.sky = null;
    // Every model of the sky (world.js): { mesh, follow: [s, offset] | null,
    // layer }, layer 0 for the dome's parts and 1 on for the models on its
    // horizon.
    this.skyLayers = [];

    // The map's distances (the engine's defaults until a map sets them):
    // NearSceneRange is the fade start and end at the shortest and longest
    // view distance; FarSceneRange is the far scene's reach.
    this.nearScene = [72, 300, 120, 500];
    this.farReach = [1500, 3000];
    this.csm = null;

    // Render targets: the far scene's image, the near scene drawn over it,
    // and the quarter-size copy for the blur.
    const target = () => new THREE.WebGLRenderTarget(1, 1, {
      type: THREE.UnsignedByteType, depthBuffer: true, resolveDepthBuffer: false,
    });
    this.farTarget = target();
    this.sceneTarget = target();
    lighting.farMap.value = this.farTarget.texture;

    // The far scene's image behind everything in the near scene. It is drawn
    // last of the opaque objects, at the far plane, depth tested: it fills
    // only the samples nothing opaque has covered (their depth still the
    // clear's), as drawing it first and everything over it did, without
    // writing the rest of the screen twice. On a slow integrated GPU that
    // full-screen pass was bandwidth: about 3 ms a frame at 1600x900 with two
    // samples; drawn last, it costs only the samples the near scene leaves
    // to it. Every opaque material of the near scene writes depth, so none
    // is covered by it; only a fragment on the far plane itself (its depth
    // the clear's, the scene there wholly dissolved into this image) is now
    // under it rather than over it: a few pixels at the horizon, a few
    // levels at most.
    this.background = ownedFullScreenPass("effects/far-image", { map: { value: this.farTarget.texture } });
    this.background.material.depthTest = true;
    sceneProperty(this.background,"renderOrder",1e9);
    this.scene.add(this.background);
    // Scene shells supply authored resource facts to the required
    // battle renderer. Loading errors propagate to the loading screen.
    this.rust = null;
    // Diagnostic snapshots are requested explicitly. No browser render-info
    // object is retained or updated while drawing the frame.
    Object.defineProperty(this,"frameInfo",{get:()=>this.rust?JSON.parse(this.rust.rust.renderer_diagnostics_frame()):null});
    this.playerRoot = null;
    this.rustReady = loadRustRenderer(this).then((rust) => {
      this.rust=rust;
      // A context lost during creation has no capability facts yet. Resolve
      // settings from restored facts before any asset or preparation can draw.
      if(!limits.ready){const facts=contextLimits(this.renderer);this.maxAnisotropy=Math.max(1,facts.maxAnisotropy);this.supportedSamples=facts.samples;this.applyScale();this.applyAntialiasing();}
      return rust;
    });
    // Construction starts the fetch before preparation awaits it. Attach a
    // rejection handler now without consuming the promise's failure.
    this.rustReady.catch(() => {});
    this.particleDepth = new ParticleDepth(this.renderer);
    this.postTarget = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false });
    // FXAA's output when an upscaler still follows it.
    this.aaTarget = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false });
    this.upscaler = new Upscaler(this.renderer);
    // The 3D frame's resolution, and the window's (the canvas).
    this.renderSize = new THREE.Vector2(1, 1);
    this.outputSize = new THREE.Vector2(1, 1);
    this.renderScale = 1;
    this.upscaling = "off";
    // Source/sampler/target descriptors remain host resources; Rust chooses
    // every post program, uniform and history transition from scalar facts.
    this.blur = null; this.vehicleMotion = 0; this.vehicleTurn = 0;
    this.postRevision = 0;
    this.quarterTarget = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false });
    this.bloomTarget = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false });
    this.bloomScratch = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false });

    this.upscaler.sharpness = Number(settings.get("upscaleSharpness") ?? 0.4) || 0;
    this.applyScale();
    this.applyAntialiasing();
    this.applyShadows();
    this.applyDistance();
    this.applyBump();
    settings.onChange((key) => {
      if (key === "renderScale" || key === "upscaler") { this.applyScale(); this.applyAntialiasing(); }
      if (key === "upscaleSharpness") this.upscaler.sharpness = Number(this.settings.get("upscaleSharpness")) || 0;
      if (key === "antialiasing") this.applyAntialiasing();
      if (key === "shadows") this.applyShadows();
      if (key === "lampLighting") for(const scene of [this.scene,this.viewScene,this.farScene]) scene.traverse(object=>{
        for(const material of [].concat(object.material||[]))sceneProperty(material,"needsUpdate",true);
      });
      if (key === "viewDistance") this.applyDistance();
      if (key === "bumpMapping") this.applyBump();
      if (key === "anisotropy") {for (const texture of this.textures) this.filter(texture);this.textureRevision++;}
    });
    window.addEventListener("resize", () => this.resize());
  }

  // The map's lights, distances, and post effect.
  setMap(map) {
    setLighting(map);
    if (map.distance?.nearScene?.length === 4) this.nearScene = map.distance.nearScene;
    if (map.distance?.farScene?.length === 2) this.farReach = map.distance.farScene;
    setFog(map);
    this.blur = map.post?.blur?.enabled ? map.post.blur : null;
    this.resizeTargets();
    this.applyShadows();
    this.applyDistance();
  }

  // Runtime environment controls for future time-of-day/weather systems.
  // Inputs use the map format: RGB 0..255, a world-space ray direction, and
  // fog distances in world units. Unspecified fields retain their values.
  // This deliberately does not rebuild the CSM or change material programs.
  setEnvironment(facts={}) {
    // Cold authored commands can precede module admission. Keep the newest
    // value per field until the native owner is available.
    Object.assign(this.environmentFacts||(this.environmentFacts={}),facts);
    if(this.rust)this.rust.cameras.environment(facts);
  }

  // A texture that follows the filtering setting.
  texture(texture) {
    this.textures.add(texture);
    this.filter(texture);
    return texture;
  }

  filter(texture) {
    const wanted = Math.max(1, Math.min(Number(this.settings.get("anisotropy")) || 1, this.maxAnisotropy));
    if (texture.anisotropy !== wanted) {
      texture.anisotropy = wanted;
      // A texture still loading picks the setting up when it uploads.
      if (texture.image) sceneProperty(texture,"needsUpdate",true);
    }
  }

  // An object that casts shadows.
  // `far` (m): beyond it from the camera the object casts no shadow (live
  // props and vehicles: many separate parts, each a draw in every cascade,
  // for a few blurred texels far away).
  caster(object, { far = 0 } = {}) {
    this.casters.add(object);
    sceneProperty(object,"castShadow",Boolean(this.csm));
    object.traverse((mesh) => this.depthMaterial(mesh));
    if (far > 0) { object.userData.shadowFar = far; this.distantCasters.add(object); }
    return object;
  }

  // Distance-limited casters, once per frame before the shadow pass.
  limitShadows() {
    if (!this.csm || !this.distantCasters.size) return;
    const eye = this.camera.position, at = this.casterAt;
    for (const object of this.distantCasters) {
      if (!object.parent || !this.casters.has(object)) { this.distantCasters.delete(object); continue; }
      const far = object.userData.shadowFar;
      sceneProperty(object,"castShadow",at.setFromMatrixPosition(object.matrixWorld).distanceToSquared(eye) < far * far);
    }
  }

  // warmupPaced links each owned family under the loading screen. Lines also
  // prewarm their finite variants: a cable or debug helper can appear later.
  async warmupPaced(settle) {
    await this.rustReady;
    const before = this.frameInfo?.programs ?? 0, started = performance.now();
    await this.rust.prewarm(1,settle);
    await this.rust.prewarm(2,settle);
    await this.rust.byName.post.warm(settle);
    const rest = (await this.rust.prepare(()=>this.warmup())).programs;
    return { programs: (this.frameInfo?.programs ?? 0) - before, rest, ms: performance.now() - started };
  }

  // Native source admission links world, foliage and canopy variants while
  // two loading frames establish dynamic inputs. Incoming battle records
  // must never wait behind that preparation.
  async prepareRenderer(settle, draw) {
    await this.rustReady;
    for (let i = 0; i < 2; i++) {
      await this.rust.prepare(()=>{this.render(0);draw();});
      await settle();
    }
    // Preserve the existing scene programs' admission identities. Utility
    // variants link after preparation has discovered world and HUD resources,
    // while the loading screen still owns the context and before play begins.
    await this.rust.prewarm(3,settle);
  }

  // warmup prepares source-owned actor and shadow resources, including hidden
  // pooled casters. Program counts describe the native catalog, not Three.
  warmup() {
    const before = this.frameInfo?.programs ?? 0, started = performance.now();
    const renderer = this.renderer, target = renderer.getRenderTarget();
    this.rust.byName.post.programs();
    // warmup keeps setRenderTarget's cold admission order. Samplers and
    // attachments settle before shadow targets and the first scene draw.
    try {
      renderer.setRenderTarget(this.sceneTarget);
      prepareActors(this.rust);
      renderer.setRenderTarget(this.farTarget);
      if (this.csm) this.rust.byName.shadow.prepare();
    } finally { renderer.setRenderTarget(target); }
    return { programs: (this.frameInfo?.programs ?? 0) - before, ms: performance.now() - started };
  }

  // Every caster names a source-owned depth variant. The hardware depth
  // attachment is the only output its receivers read.
  // Alpha-cut materials keep one each, with their own texture.
  depthMaterial(mesh) {
    if (!mesh.isMesh || mesh.customDepthMaterial) return;
    const kind = mesh.isSkinnedMesh ? "skinned" : mesh.isInstancedMesh ? "instanced" : "plain";
    const source = Array.isArray(mesh.material) ? mesh.material[0] : mesh.material;
    const cut = Boolean(source && source.alphaTest > 0 && (source.map || source.alphaMap));
    const key = `${kind}:${cut ? source.uuid : "solid"}`;
    let depth = this.depthMaterials.get(key);
    if (!depth) {
      depth = new THREE.Material();
      depth.name = `depth:${key}`;
      depth.uniforms = cut ? { map: { value: source.alphaMap || source.map }, cut: { value: source.alphaTest } } : {};
      depth.colorWrite = false;
      depth.userData.bfeProgram = `actors/depth-${mesh.isSkinnedMesh ? "skinned" : "solid"}${cut ? "-cutout" : ""}`;
      if (cut && !mesh.isSkinnedMesh) depth.userData.bfeProgram = "actors/depth-cutout";
      this.depthMaterials.set(key, depth);
    }
    mesh.customDepthMaterial = depth;
  }

  // The render scale (and adaptive resolution) shrink the 3D frame, which an
  // upscaler rebuilds at the canvas's full resolution; with the upscaler
  // off the canvas itself shrinks and the browser stretches it, HUD and all.
  applyScale() {
    // Adaptive resolution stops at the options' lowest render scale.
    const base = Number(this.settings.get("renderScale")) || 1;
    const scale = Math.max(Math.min(base, RENDER_SCALE_MIN), base * (this.adaptiveScale ?? 1));
    const native = Math.min(window.devicePixelRatio, 2);
    let mode = this.settings.get("upscaler");
    if (!UPSCALERS[mode]) mode = "temporal";
    if (mode === "temporal" && !this.upscaler.floats) mode = "spatial";
    if (mode !== this.upscaling) this.upscaler.invalidate();
    this.upscaling = mode;
    this.renderScale = mode === "off" ? 1 : scale;
    this.renderer.setPixelRatio(mode === "off" ? native * scale : native);
    this.resizeTargets();
  }

  // Drop the temporal history: the view jumped (respawn, perspective).
  resetHistory() {
    this.upscaler.invalidate();
  }

  setAdaptiveScale(scale) {
    scale = Math.max(.5, Math.min(1, scale));
    if (scale === (this.adaptiveScale ?? 1)) return;
    this.adaptiveScale = scale;
    this.applyScale();
  }

  // Multisampling for the near scene; the far scene's too, unless the
  // temporal upscaler is on. Its jittered history antialiases the far
  // scene's few silhouettes (the horizon, roofs against the sky) on its own,
  // while four samples a pixel would cost that pass's storage and a
  // resolve on every frame.
  applyAntialiasing() {
    const samples = ANTIALIASING[this.settings.get("antialiasing")] ?? 4;
    const actual = samples > 0 ? (this.supportedSamples.filter((n) => n <= samples).at(-1) ?? this.supportedSamples[0] ?? 0) : 0;
    const far = this.upscaling === "temporal" ? 0 : actual;
    for (const [target, count] of [[this.farTarget, far], [this.sceneTarget, actual]]) {
      if (target.samples !== count) {
        target.samples = count;
        target.dispose();
      }
    }
    lighting.aaSamples.value = actual;
    lighting.farAaSamples.value = far;
  }

  applyBump() {
    lighting.bump.value = this.settings.get("bumpMapping") ? 1 : 0;
  }

  // How far the near scene reaches: the original's longest view distance,
  // or this port's longer ones.
  applyDistance() {
    const scale = VIEW_DISTANCES[this.settings.get("viewDistance")]?.scale ?? 1;
    const [, start, , end] = this.nearScene;
    lighting.fade.value.set(start * scale, end * scale);
    this.camera.far = end * scale;
    this.camera.updateProjectionMatrix();
    this.farCamera.near = 2;
    this.farCamera.far = this.farReach[1] * scale;
  }

  // Cascaded shadow maps from the sun, or none.
  applyShadows() {
    if (this.csm) {
      // CSM.dispose only releases shader hooks; LightShadow owns the GPU maps.
      for (const light of this.csm.lights) light.shadow.dispose();
      for (const cache of this.shadowCaches) cache.dispose();
      this.csm.remove();
      this.csm.dispose();
      this.csm = null;
    }
    this.cascadeWindows = [];
    this.shadowCaches = [];
    this.shadowMaps = [];
    lighting.cascadeMaps.value = [];
    lighting.cascadePlanes.value = [];
    const preset = SHADOW_PRESETS[this.settings.get("shadows")];
    this.shadowsEnabled = Boolean(preset);
    if (preset) {
      this.csm = shadowShells(this,preset);
      for (const light of this.csm.lights) {
        // Receiver-plane filtering handles slopes without moving vertices
        // along interpolated character normals (which caused moving smears).
        light.shadow.normalBias = 0;
        // The live map (what the receivers sample) and the static cache.
        light.shadow.map = this.shadowTarget(preset.size);
        light.shadow.map.texture.name = `${light.name}.shadowMap`;
        this.shadowMaps.push(light.shadow.map);
        this.shadowCaches.push(this.shadowTarget(preset.size));
      }
      lighting.cascadeMaps.value = this.shadowMaps.map((target) => target.depthTexture);
      lighting.cascadePlanes.value = this.shadowMaps.map(() => new THREE.Matrix3());
    }
    this.rust?.cameras.configureShadows();
    for (const object of this.casters) sceneProperty(object,"castShadow",Boolean(preset));
    // Materials recompile for the new shadow setup on their next draw.
    this.scene.traverse((object) => {
      const materials = Array.isArray(object.material) ? object.material : [object.material];
      for (const material of materials) if (material) sceneProperty(material,"needsUpdate",true);
    });
  }

  resize() {
    this.camera.aspect = window.innerWidth / window.innerHeight;
    this.camera.updateProjectionMatrix();
    this.renderer.setSize(window.innerWidth, window.innerHeight);
    this.resizeTargets();
  }

  resizeTargets() {
    const size = this.renderer.getDrawingBufferSize(this.outputSize);
    const width = Math.max(1, Math.round(size.x * this.renderScale));
    const height = Math.max(1, Math.round(size.y * this.renderScale));
    this.renderSize.set(width, height);
    this.farTarget.setSize(width, height);
    this.sceneTarget.setSize(width, height);
    this.postTarget.setSize(width, height);
    this.aaTarget.setSize(width, height);
    this.upscaler.resize(width, height, Math.max(1, size.x), Math.max(1, size.y));
    this.bloomTarget.setSize(Math.max(1, Math.ceil(width / 4)), Math.max(1, Math.ceil(height / 4)));
    this.bloomScratch.setSize(this.bloomTarget.width, this.bloomTarget.height);
    this.postRevision++;
    if (this.settings.get("softParticles")) this.particleDepth.prepare(width, height);
    const factor = this.blur?.downsize || 0.25;
    this.quarterTarget.setSize(Math.max(1, Math.round(width * factor)), Math.max(1, Math.round(height * factor)));
    lighting.resolution.value.set(width, height);
  }

  // A cascade map: a hardware depth texture the receivers sample with the
  // GPU's own comparison and bilinear filtering (sampler2DShadow). Three's
  // render targets always carry a colour texture; one byte a texel is the
  // least it can be, and nothing writes it.
  shadowTarget(size) {
    const depthTexture = new THREE.DepthTexture(size, size, THREE.UnsignedIntType);
    depthTexture.compareFunction = THREE.LessEqualCompare;
    depthTexture.minFilter = depthTexture.magFilter = THREE.LinearFilter;
    return new THREE.WebGLRenderTarget(size, size, {
      depthTexture, format: THREE.RedFormat, type: THREE.UnsignedByteType,
      minFilter: THREE.NearestFilter, magFilter: THREE.NearestFilter, generateMipmaps: false, stencilBuffer: false,
    });
  }

  // Where each cascade's window lies this frame (shadow-cascades.js): held
  // where it was while it still covers the cascade's slice of the view, else
  // landed on the slice as three's CSM would. The light is only moved when a
  // window lands, so a held cascade keeps its matrices and its maps.
  // The shadow pass: the static casters into a cascade's cache the frame its
  // window lands; every frame the cache's depth copied into the live map and
  // the moving casters drawn over it. Three's pass clears the map before it
  // draws; the copy stands in for that clear.
  renderCascades(scene, camera) {
    const { renderer, csm } = this;
    const core=this.rust,frame=core.frame;
    core.views();core.frameFacts[F.SHADOW_COUNT]=csm.lights.length;
    for(let i=0;i<csm.lights.length;i++){core.frameFacts[F.SHADOW_DIRTY+i]=+this.cascadeWindows[i].dirty;core.frameFacts[F.SHADOW_MOVED+i]=+this.cascadeWindows[i].moved;}
    core.rust.frame_select_frame();core.views();
    const dynamicCamera = this.shadowCamera, staticCamera = this.staticShadowCamera;
    core.cameras.copy(dynamicCamera,CAMERA_ROLE.MAIN,CAMERA_ROLE.SHADOW_DYNAMIC);
    sceneLayer(dynamicCamera.layers,"enable",this.hiddenLayer);
    core.cameras.copy(staticCamera,CAMERA_ROLE.MAIN,CAMERA_ROLE.SHADOW_STATIC);
    sceneLayer(staticCamera.layers,"set",this.staticLayer);
    for (let i = 0; i < csm.lights.length; i++) {
      const light = csm.lights[i], window = this.cascadeWindows[i], live = this.shadowMaps[i], cache = this.shadowCaches[i];
      if (core.frameState[S.SHADOW_STATIC+i]) {
        frame.role=R.SHADOW_STATIC0+i;
        light.shadow.map = cache;
        // Draw cached static depth through the required Rust shadow pass.
        this.rust.coverShadow(renderer, light, scene, staticCamera, null);
        light.shadow.map = live;

        this.shadowPasses.static++;
      }
      if (core.frameState[S.SHADOW_DYNAMIC+i]) {
        frame.role=R.SHADOW_DYNAMIC0+i;
        this.rust.coverShadow(renderer, light, scene, dynamicCamera, cache);
        this.shadowPasses.dynamic++;
      }

      // How this cascade's matrix (refreshed by the pass) moves plane normals.

    }
  }

  setVehicleMotion(amount, turn) {
    this.vehicleMotion = amount; this.vehicleTurn = turn;
  }

  // Direct scene clients use the same native battle schedule as game.frame.
  // The game begins collection earlier so sky updates join this transaction.
  render(time=0) {
    this.rust.frame.begin(true);
    this.rust.frame.mirror(this.scene.userData.water,this.camera);
    this.prepareFrame(time);
    this.rust.frame.underwater(this.scene.userData.water,this.camera);
    this.rust.frame.execute();this.profiler.end();
  }

  prepareFrame(time = 0) {
    if (this.rust.contextLost) return;
// A prior fatal renderer error remains visible. Re-entering frame rendering
// must not hide it by clearing the canvas or selecting a different drawer.
    if (this.rust.failure) throw this.rust.failure;
    const { renderer, camera } = this;
    this.profiler.begin();
    const temporal = this.upscaling === "temporal";
    lighting.time.value = time;
    this.localLights.begin(Boolean(this.settings.get("lampLighting")));
    // Ordnance pushes next frame's bolts in this look.
    boltLook.enhanced = this.settings.get("boltEffects") !== "classic";
    sniperLook.enhanced = this.settings.get("sniperEffects") !== "classic";
    lighting.terrainShade.value = this.settings.get("terrainDetail") === "original" ? 0 : 1;
    this.rust.cameras.main(camera);
    this.rust.byName.post.begin(camera);
    if (this.csm) {
      this.rust.frame.prepareWorlds();this.limitShadows();this.rust.cameras.cascades();
    }
    const far=this.farCamera;
    this.rust.cameras.derived(far,CAMERA_ROLE.FAR);
    if (temporal) this.upscaler.shift(far);
    this.rust.cameras.sky(this.skyLayers,CAMERA_ROLE.MAIN);
    this.rust.frame.target(this.farTarget);
    this.rust.cover("far", renderer, this.farScene, far);

    // 2. The near scene over it, then the first-person weapon.
    this.rust.frame.placement();
    this.rust.frame.prepareWorlds();
    if (temporal) this.upscaler.shift(camera);
    this.rust.frame.target(this.sceneTarget);
    if (this.csm) this.rust.frame.shadowProducer();
    this.particleDepth.begin(this.sceneTarget, camera, Boolean(this.settings.get("softParticles")));
    // Every visible item must belong to a Rust pass.
    this.rust.coverNear(renderer, this.scene, camera);
    // The temporal upscaler reprojects by the scene's depth, taken before
    // the weapon's own depth range replaces it.
    this.rust.frame.finalDepth=true;
    if (temporal) this.particleDepth.capture(camera, true);
    this.rust.frame.finalDepth=false;
    this.rust.views();this.rust.frameFacts[F.TEMPORAL]=+temporal;
    this.rust.frame.binding(R.DEPTH,this.particleDepth.target);
    let viewShown=false;
    for(const child of this.viewScene.children)if(child.visible&&!child.isLight){viewShown=true;break;}
    this.rust.views();this.rust.frameFacts[F.VIEW_SHOWN]=+viewShown;
    if (viewShown) {
      const view = this.viewCamera;
      this.rust.cameras.derived(view,CAMERA_ROLE.VIEW);
      if (temporal) this.upscaler.shift(view);
      this.rust.cover("view", renderer, this.viewScene, view);
    }
    if (temporal) {
      const motion=this.rust.frame.motionInput;
      motion.viewScene=viewShown?this.viewScene:null;motion.depth=this.particleDepth.target.depthTexture;
      this.rust.frame.motionProducer();
      this.upscaler.restore();
    }

    this.rust.byName.post.prepare();
    this.rust.frame.restorePlacement();
  }

  // The stats line's part: the 3D view's size (adaptive resolution moves
  // it), and a warning when the GPU draws or composites in software.
  statsText() {
    const gpu = pageGpu();
    const warning = gpu.software ? "  SOFTWARE GPU" : gpu.softwareCompositing || gpu.readback ? "  SOFTWARE COMPOSITING" : "";
    const shadow = this.rust?.byName.shadow;
    const clipped = shadow ? shadow.clipped + this.rust.rust.static_depth_clipped() : 0;
    const native=this.rust?.rust,cameras=native?+native.camera_clipped():0,packs=native?native.camera_pack_rejected():0,sniper=this.rust?this.rust.byName.effects.sniperDropped():0;
    return `3D ${this.renderSize.x}x${this.renderSize.y}${warning}${clipped ? `  SHADOW CLIPPED ${clipped}` : ""}${cameras?"  CAMERA CLIPPED":""}${packs?`  LIGHT PACK REJECTED ${packs}`:""}${sniper?`  SNIPER CLIPPED ${sniper}`:""}`;
  }

  diagnostics() {
    return {
      resolution: [this.sceneTarget.width, this.sceneTarget.height],
      output: [this.outputSize.x, this.outputSize.y],
      upscaler: this.upscaling,
      antialiasing: this.settings.get("antialiasing"),
      samples: this.sceneTarget.samples,
      farSamples: this.farTarget.samples,
      postAA: this.upscaling === "temporal" ? "temporal" : this.settings.get("antialiasing") === "fxaa-fast" ? "fxaa-fast" : this.settings.get("antialiasing") !== "off" ? "fxaa" : "off",
      supportedSamples: this.supportedSamples,
      anisotropy: Math.max(1, Math.min(Number(this.settings.get("anisotropy")) || 1, this.maxAnisotropy)),
      maxAnisotropy: this.maxAnisotropy,
      cascades: this.csm?.lights.length ?? 0,
      shadowSize: this.csm?.shadowMapSize ?? 0,
      shadowTexelSizes: this.csm?.lights.map((light) => (light.shadow.camera.right - light.shadow.camera.left) / this.csm.shadowMapSize) ?? [],
      bloom: Boolean(this.settings.get("bloom")),
      softFocus: Boolean(this.blur && this.settings.get("softFocus")),
      softParticles: Boolean(this.settings.get("softParticles")),
      particleDepthCopies: this.particleDepth.copies,
      frame: JSON.parse(this.rust.rust.renderer_diagnostics_frame()),
      shadows: this.shadowInfo,
      shadowPasses: { ...this.shadowPasses },
      // Lifetime counters distinguish dropped pass submissions from source
      // admission limits. Neither condition interrupts a displayed frame.
      cameraCapacity: { clipped: this.rust.rust.camera_clipped(), evicted: this.rust.rust.camera_evicted() },
      lightPackCapacity: { evicted: this.rust.rust.camera_pack_evicted(), rejected: this.rust.rust.camera_pack_rejected() },
      sniperClipped: this.rust.byName.effects.sniperDropped(),
      resourceCapacity: JSON.parse(this.rust.rust.resource_diagnostics()),
      shadowClipped: { draws: this.rust.byName.shadow.clipped, sources: this.rust.rust.static_depth_clipped() },
      timings: this.profiler.report(),
      lampLights: { sources: this.localLights.sources.length, occlusionRays: this.localLights.rays },
      // Native geometry counters remain; texture/target storage belongs to Rust.
      memory: { resources: JSON.parse(this.renderer.resources.gpu.memory()) },
      gpu: pageGpu(),
    };
  }
}
