// Rust draws every battle phase. Three supplies scene updates and CPU
// descriptors. Rust owns texture storage and targets. This core
// registers those resources, publishes frame
// inputs, and treats incomplete coverage or a failed program as a fatal error.
import { INPUT as CAMERA_INPUT } from "./camera-layout.js";
import { CameraOwner } from "./cameras.js";
import { BattleFrame } from "./frame.js";
import { RetainedScene, Family } from "./scene.js";
import { isActorMaterial } from "./actors.js";
import { rendererFailure } from "./failure.js";
import { rendererModule } from "./load.js";
import { installResources } from "../render-resources.js";
import * as THREE from "three";
import { lighting } from "../materials.js";
import { packed, packPointLights } from "../point-lights.js";
import { worldPass } from "./world.js";
import { instancesPass } from "./instances.js";
import { farPass, seaPass } from "./far.js";
import { terrainPass } from "./terrain.js";
import { transparentPass } from "./transparent.js";
import { effectsPass } from "./effects.js";
import { foliagePass } from "./foliage.js";
import { viewPass } from "./view.js";
import { postPass } from "./post.js";
import { hudPass } from "./hud.js";
import { waterPass } from "./water.js";
import { nearPass } from "./near.js";
import { canopiesPass } from "./canopies.js";
import { motionPass } from "./motion.js";
import { shadowPass } from "./shadow.js";
import { cloudsPass, underwaterPass, fogPass } from "./screen.js";
import { mirrorPass } from "./mirror.js";
import { linesPass } from "./lines.js";
import { InterfacePass } from "./interface.js";
import { bindInterfaceRenderer } from "./draw-guard.js";
import { GL_TYPES } from "./geometry-types.js";
import { ownsEffectSource } from "./programs.js";

// The views a frame draws, each with a frame block of its own (core.rs
// VIEW_*). This file imports the passes, so a pass reads these inside its
// functions, not as it loads.
export const VIEW_NEAR = 0, VIEW_FAR = 1, VIEW_FIRST_PERSON = 2, VIEW_SPARE = 3, VIEW_FAR_MIRROR = 4;
// The frame block's members (shader.rs FRAME_MEMBERS), in its float layout.
const FRAME = [
  ["projectionMatrix", 0, 16], ["viewMatrix", 16, 16], ["cameraPosition", 32, 3], ["time", 35, 1],
  ["sunDirection", 36, 3], ["bump", 39, 1], ["sunColor", 40, 3], ["shadowStrength", 43, 1],
  ["ambient", 44, 3], ["aaSamples", 47, 1], ["ambientTop", 48, 3], ["terrainShade", 51, 1],
  ["ambientBottom", 52, 3], ["farAaSamples", 55, 1], ["characterAmbient", 56, 3], ["cascadeNear", 59, 1],
  ["fogColor", 60, 3], ["cascadeFar", 63, 1], ["fade", 64, 2], ["resolution", 66, 2], ["fogRange", 68, 2],
];
const NATIVE_LIGHT_FIELDS=new Set(["cascadeNear","cascadeFar","sunDirection","sunColor","ambient","ambientTop","ambientBottom","characterAmbient","fogColor","fogRange","shadowStrength"]);
// The shadow block's float layout (shader.rs SHADOW_*): per cascade its
// shadow values (8: intensity, bias, normal bias, radius, map size, pad),
// matrix (16), plane matrix (12: three columns of four) and slice (4).
const SHADOW = { CASCADES: 4 };
const LIGHT_COUNT = 70;

// Scene inputs still have JS producers. They publish named views; execution
// and dependency order live exclusively in frame_plan.rs.
const VIEWS = {far:VIEW_FAR,near:VIEW_NEAR,transparent:VIEW_NEAR,view:VIEW_FIRST_PERSON,
  hud:null,mirror:VIEW_SPARE,mirrorTransparent:VIEW_SPARE,farMirror:VIEW_FAR_MIRROR,
  probe:VIEW_FAR,motion:null,shadow:null,lines:null,image:null,skyClouds:null,underwater:null};

// three's renderer's entry points that read or change its state cache, each
// settling a deferred phase's GL state first (watchThree).
const SETTLES = ["render", "clear", "clearColor", "clearDepth", "clearStencil", "setClearColor", "setClearAlpha", "compile",
  "compileAsync", "setViewport", "setScissor", "setScissorTest", "resetState", "dispose"];

// The renderer with each registration call it is given kept (typed arrays
// copied), so another build of it can be given the same scene. The phases'
// calls and the memory layout's are not registration.
function recorder(rust, recording) {
  return new Proxy(rust, {
    get(target, name) {
      const value = target[name];
      if (typeof value !== "function") return value;
      if (/frame$|_segment$|_(ptr|len)$|^(records_at|record_stride|rebuild|replace_sample_texture)$/.test(String(name))) return value.bind(target);
      return (...args) => {
        const result = value.apply(target, args);
        recording.calls.push([name, name === "add_texture" ? [null] : args.map((a) => (ArrayBuffer.isView(a) ? a.slice() : a?.constructor?.name === "WebGLTexture" ? null : a)), result]);
        return result;
      };
    },
  });
}

// A geometry's groups with consecutive ranges of one material merged: a
// soldier's two ranges with the same material draw as one.
const mergedGroups = new WeakMap();
function groupsOf(geometry) {
  let groups = mergedGroups.get(geometry);
  if (groups) return groups;
  groups = [];
  for (const g of geometry.groups) {
    const last = groups[groups.length - 1];
    if (last && last.materialIndex === g.materialIndex && last.start + last.count === g.start) last.count += g.count;
    else groups.push({ start: g.start, count: g.count, materialIndex: g.materialIndex });
  }
  mergedGroups.set(geometry, groups);
  return groups;
}

export async function createRustRender(graphics) {
  const { module, wasm } = await rendererModule();
  await installResources(graphics.renderer);
  return new RustRender(graphics, wasm, module.BfeRenderer);
}

export class RustRender {
  constructor(graphics, wasm, BfeRenderer) {
    this.graphics = graphics;
    this.renderer = graphics.renderer;
    this.resources = graphics.renderer.resources;
    this.gl = this.renderer.getContext();
    this.wasm = wasm;
    this.rust = new BfeRenderer(this.gl);
    // ?rustRecord: every call made to the renderer, and each frame's inputs,
    // kept for a replay outside the browser (rust/bfe-native).
    this.recording = new URLSearchParams(location.search).has("rustRecord") ? { calls: [], textures: [], frames: [], keepFrames: 0 } : null;
    if (this.recording) this.rust = recorder(this.rust, this.recording);
    this.buffer = null;
    this.viewSpecs = [];
    this.geometries = new Map(); // "geometry uuid:program" -> Rust geometry
    this.textureSources = [];
    this.nativeSamples = new Map();
    this.sampleTextures = new WeakMap();
    this.textureRevision = graphics.textureRevision ?? 0;
    this.resourceEpoch = 0;
    this.textures = new Map(); // WebGLTexture -> Rust texture
    this.farTexture = null;
    this.phaseStats = {};
    this.phaseMs = {};

    this.errors = [];
    // Scene objects no pass claims, by name, type and material key: each is
    // skipped for the frame and reported once (skip()).
    this.skipped = new Set();
    // The cascades' maps last given to the renderer.
    this.shadowMapCount = 0;
    this.shadowMapIds = new Int32Array(SHADOW.CASCADES).fill(-1);
    this.shadowMapData = new Uint32Array(SHADOW.CASCADES);
    this.shadowMapViews = Array.from({length:SHADOW.CASCADES+1},(_,n)=>this.shadowMapData.subarray(0,n));
    this.shadowView = false;
    // Retained lamp sources and their indices supply owned actor programs.
    this.lampSources = 0;
    this.lampIndex = new Map();
    this.lampOut = { indices: [-1, -1, -1, -1], count: 0 };
    // The renderer's inputs: the frame block's values, then the passes'
    // records (f64), where they start and a record's size (instances.rs);
    // and the bones of the rigs drawn (f32).
    this.recordsAt = this.rust.records_at();
    this.stride = this.rust.record_stride();
    this.addView("input", Float64Array, () => this.rust.input_ptr(), () => this.rust.input_len());
    this.addView("bones", Float32Array, () => this.rust.bones_ptr(), () => this.rust.bones_len());
    // Phases drawn between three's renders reset three's state once, when
    // three is next used.
    this.stale = false;
    this.settleThree = null;
    this.watchThree();
    this.contextLost = false;
    this.renderer.domElement?.addEventListener?.("webglcontextlost", (event) => {
      event.preventDefault();
      this.contextLost = true;
      this.stale = false;
    });
    this.renderer.domElement?.addEventListener?.("webglcontextrestored", () => {
      try { this.rebuild(); } catch (error) { this.fail(error); }
    });
    // The passes' work before three lists a render of the near scene
    // (beforeSort): hooked first, so that what hooks in after it
    // (particle-depth.js's copier) sees what they registered.
    this.passes = [];
    this.registry = new RetainedScene(this);
    this.sceneContext = {renderer:this.renderer,scene:graphics.scene,camera:graphics.camera};
    // The passes, in the renderer's order.
    this.phases = Object.fromEntries(Object.keys(VIEWS).map((name) => [name, []]));
    // A mirror draws the near phase's passes, then the transparent phase's;
    // its far scene, the far phase's.
    this.phases.mirror = this.phases.near;
    this.phases.mirrorTransparent = this.phases.transparent;
    this.phases.farMirror = this.phases.far;
    this.phases.probe = this.phases.far;
    this.byName = {};
    this.byName.post = postPass(this);
    for (const make of [worldPass, instancesPass, farPass, terrainPass, transparentPass, effectsPass, foliagePass, viewPass, hudPass, waterPass, nearPass, canopiesPass, seaPass, motionPass, shadowPass, cloudsPass, underwaterPass, mirrorPass, fogPass, linesPass]) this.addPass(make(this));
    this.interface = new InterfacePass(this);
    bindInterfaceRenderer(this.renderer, this.interface);
    this.cameras = new CameraOwner(this);
    this.frame = new BattleFrame(this);
    for (const scene of [graphics.scene, graphics.farScene, graphics.viewScene]) if (scene) this.registry.admit(scene);
  }

  // The scene and texture owners keep their CPU resources. Rust replays its
  // retained resource descriptions into the restored context, preserving ids.
  rebuild() {
    this.stale = false;
    const renderer = this.renderer, ids = new Uint32Array(this.textureSources.length);
    this.graphics.warmup();
    this.textures.clear();
    this.sampleTextures = new WeakMap();
    for (let id = 0; id < this.textureSources.length; id++) {
      const texture = this.textureSources[id];
      if (typeof texture === "number") { ids[id] = texture; continue; }
      ids[id] = this.resources.owns(texture) ? this.resources.textureId(texture) : this.resources.gpu.retired_sample();
      const handle = renderer.properties.get(texture).__webglTexture;
      this.textures.set(handle,id);
      this.sampleTextures.set(texture,{id,handle,version:this.resources.owns(texture)?texture.version:-1,source:texture.source,sourceVersion:texture.source.version,revision:this.resources.revision,width:texture.image?.width,height:texture.image?.height,depth:texture.image?.depth});
    }
    this.rust.rebuild(this.gl,this.resources.gpu,ids);
    this.buffer = null;
    this.views();
    this.shadowMapCount = -1;
    this.byName.post.revision = -1;
    this.byName.hud.full = true;
    this.byName.water.full = true;
    for (const window of this.graphics.cascadeWindows || []) window.invalidate();
    this.graphics.upscaler?.invalidate?.();
    renderer.resetState();
    this.contextLost = false;
  }

  addPass(pass) {
    if (!this.phases[pass.phase]) throw new Error(`rust-render: pass ${pass.name} has no phase ${pass.phase}`);
    this.passes.push(pass);
    this.phases[pass.phase].push(pass);
    this.byName[pass.name] = pass;
    pass.register?.();
  }

  // Whether the frame has shadow receivers' inputs to give (C2: the cascades'
  // block and maps).
  shadowsOn() {
    return this.renderer.shadowMap.enabled && Boolean(this.graphics.csm) && lighting.cascadeMaps.value.length > 0;
  }

  // Transparent candidates retained as roots enter the battle scene.
  nearList(renderer, scene) {
    return this.registry.items;
  }

  // A typed view of the renderer's memory, kept as this[name]: a Type over
  // len() elements from the byte offset ptr(). views() makes them again
  // whenever the memory has grown, which detaches the old ones: registering
  // anything can.
  addView(name, Type, ptr, len) {
    this.viewSpecs.push({ name, Type, ptr, len });
    this.buffer = null;
  }

  views() {
    if (this.buffer === this.wasm.memory.buffer) return;
    // Query every address before acquiring views: a lazy pass may allocate
    // during its pointer getter and grow the shared resource/renderer heap.
    for (const spec of this.viewSpecs) { spec.address=spec.ptr();spec.length=spec.len(); }
    this.buffer = this.wasm.memory.buffer;
    for (const spec of this.viewSpecs) this[spec.name] = new spec.Type(this.buffer,spec.address,spec.length);
  }

  texture(texture,refresh=false) {
    // Native resource clients pass sampler ids. Their descriptors and retained
    // restoration bytes live in GpuResources, never renderer.properties.
    if (typeof texture === "number") {
      const known = this.nativeSamples.get(texture);
      if (known !== undefined) {
        if (refresh) this.rust.replace_sample_texture(known, this.resources.gpu, texture);
        return known;
      }
      const id = this.rust.add_texture(this.resources.gpu, texture);
      this.nativeSamples.set(texture, id);
      this.textureSources[id] = texture;
      return id;
    }
    if (!texture) return -1;
    const known = this.sampleTextures.get(texture);
    // Resource generations are resolved once per descriptor change. Ordinary
    // draw lookups only compare scalar versions; no image/sampler scan runs.
    if(!refresh && known && known.revision===this.resources.revision && known.version===texture.version && known.source===texture.source && known.sourceVersion===texture.source.version && known.width===texture.image?.width && known.height===texture.image?.height && known.depth===texture.image?.depth)return known.id;
    const resource = this.resources.textureId(texture);
    const handle = this.resources.gpu.texture(resource);
    if(known){
      if(known.handle!==handle)this.replaceTexture(known.id,texture,known.handle,handle,resource);
      known.revision=this.resources.revision;known.version=texture.version;known.source=texture.source;known.sourceVersion=texture.source.version;known.width=texture.image?.width;known.height=texture.image?.height;known.depth=texture.image?.depth;
      return known.id;
    }
    // Clones can initially share a native handle and later acquire different
    // sampler settings. Keep their logical ids independent from the outset.
    const id = this.rust.add_texture(this.resources.gpu,resource);
    this.textureSources[id] = texture;
    this.textures.set(handle, id);
    if (this.recording) this.recording.textures[id] = { handle, width: texture.image?.width ?? texture.mipmaps?.[0]?.width ?? texture.width ?? 0, height: texture.image?.height ?? texture.mipmaps?.[0]?.height ?? texture.height ?? 0 };
    this.sampleTextures.set(texture,{id,handle,version:texture.version,source:texture.source,sourceVersion:texture.source.version,revision:this.resources.revision,width:texture.image?.width,height:texture.image?.height,depth:texture.image?.depth});
    return id;
  }

  replaceTexture(id,texture,previous,handle,resource){
    this.rust.replace_sample_texture(id,this.resources.gpu,resource ?? this.resources.textureId(texture));
    if(this.textures.get(previous)===id)this.textures.delete(previous);
    this.textures.set(handle,id);
    this.textureSources[id]=texture;
    this.sampleTextures.set(texture,{id,handle,version:texture.version,source:texture.source,sourceVersion:texture.source.version,revision:this.resources.revision,width:texture.image?.width,height:texture.image?.height,depth:texture.image?.depth});
    if(this.recording)this.recording.textures[id]={handle,width:texture.image?.width??0,height:texture.image?.height??0};
    this.stale=true;
  }


  // A geometry's groups, consecutive ranges of one material merged.
  groups(geometry) {
    return groupsOf(geometry);
  }

  // Rust's copy of a geometry's attributes (those the program reads, at its
  // locations) and index.
  geometry(geometry, program) {
    const key = `${geometry.uuid}:${program.id}`;
    let id = this.geometries.get(key);
    if (id !== undefined) return id;
    id = null;
    const attributes = [];
    for (const [name, location] of program.attributes) {
      const a = geometry.attributes[name];
      const array = a?.isInterleavedBufferAttribute ? a.data.array : a?.array;
      if (!array || !(array.constructor.name in GL_TYPES)) { this.errors.push(`attribute ${name}`); this.geometries.set(key, null); return null; }
      attributes.push([a, location, array]);
    }
    if (!geometry.index) { this.geometries.set(key, null); return null; }
    id = this.rust.add_geometry();
    // Each buffer once (interleaved attributes share theirs).
    const buffers = new Map();
    for (const [a, location, array] of attributes) {
      const source = a.isInterleavedBufferAttribute ? a.data : a;
      let slot = buffers.get(source);
      if (slot === undefined) { slot = this.rust.geometry_buffer(id, new Uint8Array(array.buffer, array.byteOffset, array.byteLength)); buffers.set(source, slot); }
      const bytes = array.BYTES_PER_ELEMENT;
      const stride = a.isInterleavedBufferAttribute ? a.data.stride * bytes : 0, offset = a.isInterleavedBufferAttribute ? a.offset * bytes : 0;
      this.rust.geometry_pointer(id, slot, location, a.itemSize, GL_TYPES[array.constructor.name], a.normalized, stride, offset);
    }
    const index = geometry.index.array;
    this.rust.geometry_index(id, new Uint8Array(index.buffer, index.byteOffset, index.byteLength), index.BYTES_PER_ELEMENT === 2 ? 0x1403 : 0x1405);
    for (const group of groupsOf(geometry)) this.rust.geometry_group(id, group.start, group.count);
    this.geometries.set(key, id);
    return id;
  }

  // The frame's shared values, as three would upload them, for a view's
  // frame block (renderer.rs upload_frame). The views drawn over the far
  // scene's image read it too (farMap); it is taken first, so that its
  // texture is registered before anything a pass registers.
  writeFrame(scene, camera, view) {
    // The far scene's views (the eye's, a water mirror's) read neither the far
    // image nor shadows nor lamps.
    const far = view === VIEW_FAR || view === VIEW_FAR_MIRROR;
    if (!far) {
      // The far image the materials read (lighting.farMap: the far target,
      // or a mirror's own far image while it draws).
      const image = lighting.farMap.value ?? this.graphics.farTarget.texture;
      const far = this.renderer.properties.get(image).__webglTexture;
      if (far !== this.farTexture) { this.farTexture = far; this.rust.set_far_map(this.texture(image)); }
    }
    // The Enhanced inputs first: they may register the cascades' maps and send
    // the lamp table, which can grow the renderer's memory (every view is made
    // again then): the frame block's view is taken after them.
    if (!far && view !== VIEW_FIRST_PERSON) this.writeShadow();
    if (!far) this.writeLamps();
    this.views();
    const input = this.input;
    const nativeRole=this.cameras.shaderRole(camera);
    this.rust.camera_shader_frame(nativeRole);

    for (const [name, at, size] of FRAME) {
      if (at < 35 || NATIVE_LIGHT_FIELDS.has(name)) continue;
      const v = lighting[name].value;
      if (size === 1) input[at] = v;
      else { input[at] = v.x; input[at + 1] = v.y; if (size === 3) input[at + 2] = v.z; }
    }
    this.rust.camera_global_shader_frame();
    let pack = packed(scene, camera);
    // A nested view can recycle an old pair's slot. The packet needs this
    // pair's current generation, never the replacement's numeric light row.
    if(pack.native!==undefined&&!this.cameras.packValid(pack))pack=packPointLights(scene,camera,this.cameras);
    if(pack.native!==undefined)this.rust.camera_pack_shader_frame(pack.native);
    else input[LIGHT_COUNT]=0;

  }

  // Cascades publish the receiver block directly at the original shadow
  // boundaries. Mirrors consume the preceding publication; the near scene
  // consumes this frame's. The host supplies only live sampler identities.
  writeShadow() {
    const on = this.shadowsOn();
    const maps = lighting.cascadeMaps.value, count = on ? Math.min(maps.length,SHADOW.CASCADES) : 0;
    let changed = count !== this.shadowMapCount, valid = true;
    // writeShadow runs for several views. Compare numeric sampler identities
    // in retained storage; only a changed binding crosses into Rust.
    for(let i=0;i<count;i++){
      const id=this.texture(maps[i]);
      if(this.shadowMapIds[i]!==id)changed=true;
      this.shadowMapIds[i]=id;this.shadowMapData[i]=id;
      if(id<0)valid=false;
    }
    this.shadowMapCount=count;
    if(changed)this.rust.set_shadow_maps(this.shadowMapViews[valid?count:0]);
    if (!on) return;
    if (!this.shadowView) { this.addView("shadowInput", Float64Array, () => this.rust.shadow_input_ptr(), () => this.rust.shadow_input_len()); this.shadowView = true; }
    this.views();
    const lights=this.graphics.csm.lights,out=this.cameraInput;
    for(let i=0;i<Math.min(lights.length,SHADOW.CASCADES);i++){
      const shadow=lights[i].shadow,at=CAMERA_INPUT.SHADOW_METADATA+i*6;
      out[at]=shadow.intensity??1;out[at+1]=shadow.bias;out[at+2]=shadow.normalBias;out[at+3]=shadow.radius;out[at+4]=shadow.mapSize.x;out[at+5]=shadow.mapSize.y;
    }
    this.rust.camera_shadow_shader_frame();

  }

  // The lamp table (shader.rs LAMP_*): every lamp source of the map (two
  // texels each: position and radius, colour), given again when the sources
  // grow (they do as the self-lit textures arrive, early in the battle).
  writeLamps() {
    const local = this.graphics.localLights;
    if (!local || !this.graphics.settings.get("lampLighting")) return;
    const sources = local.sources;
    if (sources.length === this.lampSources) return;
    const data = new Float32Array(8 * sources.length);
    for (let i = 0; i < sources.length; i++) {
      const s = sources[i];
      data.set([s.position.x, s.position.y, s.position.z, s.radius, s.color.x, s.color.y, s.color.z, 0], 8 * i);
      this.lampIndex.set(s, i);
    }
    this.lampSources = sources.length;
    if (sources.length) this.rust.set_lamps(data);
  }

  // The lamps a pass's draw of `object` has this frame (four lamp table
  // indices, -1 for none, and a count), as three's onBeforeRender of a
  // DYNAMIC zero material selects them (local-lights.js select, with its
  // cache and occlusion rays); viewModel: a first-person part's. The object
  // returned is reused.
  lamps(object, viewModel = false) {
    const out = this.lampOut, lights = this.graphics.localLights?.select(object, viewModel) ?? [];
    out.count = Math.min(4, lights.length);
    for (let k = 0; k < 4; k++) out.indices[k] = k < out.count ? this.lampIndex.get(lights[k]) ?? -1 : -1;
    return out;
  }


  // Sparse lifecycle admission precedes each view publication. The observer
  // has already captured topology at child/property mutation boundaries; only
  // newly admitted resources are inspected here, never the live scene tree.
  prepareScene(renderer, scene, camera, target) {
    const registry=this.registry;
    registry.admitIfNew(scene,scene===this.graphics.scene?0:scene===this.graphics.farScene?Family.FAR:Family.VIEW);
    const fresh=registry.fresh,freshCount=fresh.length;
    const list = registry.items, admitted=registry.admission;admitted.length=freshCount;
    for(let i=0;i<freshCount;i++)admitted[i]=fresh[i];
    for (let i = 0; i < freshCount; i++) {
      const node = fresh[i], object = node.object;
      node.freshQueued = false;
      this.byName.far.admitAnchor(object);
      if(node.scope&Family.VIEW)this.byName.view.admit(object);
      if (!node.main) continue;
      if(object.userData.actorWhole||object.userData.shadowOnly){registry.claim(object);continue;}
      this.byName.instances.admit(object);
      if(object===this.graphics.background){registry.claim(object);continue;}
      if (!(object.isMesh || object.isSprite)) continue;
      const material = object.material;
      if (!material || Array.isArray(material)) continue;
      if (object.userData.shadowOnly || isActorMaterial(material)) continue;
      if (material.transparent && !node.listed) { list.transparent.push(node.item); node.listed = true; }
    }
    fresh.copyWithin(0, freshCount); fresh.length -= freshCount;
    if (scene === this.graphics.scene) {
      const transparent = this.byName.transparent;
      // registerItems admitted only changed resources. Retain that boundary:
      // unchanged views must not rescan every transparent/effect candidate.
      // Pending textures and effects already have their pass-owned retry sets.
      if (freshCount) {
        const context=this.sceneContext;context.renderer=renderer;context.scene=scene;context.camera=camera;
        this.byName.effects.listed(context);
        for (let i = 0; i < list.transparent.length; i++) {
          const item = list.transparent[i];
          if (!item.material || transparent.taken.has(item.object) || !transparent.candidate(item)) continue;
          const entry = item.material.userData.bfeFamily === "world" || item.material.isShaderMaterial && "WORLD_SPACE" in (item.material.defines || {}) ? transparent.registerBucket(item) : transparent.registerObject(item, true);
          if (entry) transparent.taken.set(item.object, entry);
        }
      }
      for (const pass of this.passes) pass.beforeSort?.(renderer, scene, camera, target, registry.added);
      for(let i=0;i<registry.added.length;i++){const node=registry.admitIfNew(registry.added[i]);if(node)node.addedQueued=false;}
      registry.added.length = 0;
      // Admission resolves the remaining generic colour owner. The selector
      // therefore emits complete native draw ids without a second JS filter.
      for(let i=0;i<admitted.length;i++){
        const node=admitted[i],object=node.object;
        if(!node.main || !(node.families&Family.NEAR))continue;
        const material=object.material,entry=transparent.taken.get(object);
        if(material?.transparent && !(material.transmission>0) && entry){registry.bind(object,Family.NEAR,entry,entry.id);continue;}
        registry.claim(object);
        if(ownsEffectSource(material) || transparent.retry.has(object))continue;
        this.skip(object,material,"unclaimed scene resource");
      }
    }
  }

  // Publish one retained semantic view. Native execution consumes its sealed
  // packet after all required scene and presentation inputs are available.
  publishView(name, renderer, scene, camera, walked = null, values = null) {
    if (this.contextLost) return;
    if (this.resources.failure) throw this.resources.failure;
    const revision=this.graphics.textureRevision ?? 0;
    if(revision!==this.textureRevision || this.resources.gpu.epoch()!==this.resourceEpoch){
      this.textureRevision=revision;
      for(const texture of this.textureSources)if(typeof texture === "number" || this.resources.owns(texture))this.texture(texture,true);
      this.resourceEpoch=this.resources.gpu.epoch();
    }
    if (VIEWS[name] !== null) this.writeFrame(scene,camera,VIEWS[name]);
    const ctx=this.frame.context(name,scene,camera,walked,values);
    for(const pass of this.phases[name])pass.draw?.(ctx);
    if(ctx.priming)return ctx;
    // The native-client capture explicitly requests a finite near-view input
    // sample. Ordinary view publication retains its allocation-free path.
    if(name==="near"&&this.recording?.keepFrames>0){
      this.recording.keepFrames--;this.views();
      this.recording.frames.push({records:ctx.records,input:this.input.slice(0,this.recordsAt+ctx.records*this.stride),bones:this.bones.slice(0,ctx.bones*16)});
    }
    this.frame.publish(name,ctx);
    return ctx;
  }

  // Three's state cache, out of date after a deferred phase, made good before
  // three is next used: every entry point of three's renderer that uses it
  // (render, targets, clears, reads, uploads, compiles, viewport) settles
  // first. Settling resets three's state and binds again the target three
  // believes bound (a phase leaves three's own idea of it unchanged).
  watchThree() {
    const renderer = this.renderer;
    if (renderer.__rustSettles) return;
    renderer.__rustSettles = true;
    const resetState = renderer.resetState?.bind(renderer), setRenderTarget = renderer.setRenderTarget?.bind(renderer), getRenderTarget = renderer.getRenderTarget?.bind(renderer);
    this.settleThree = () => {
      if (!this.stale || this.frame?.active) return;
      this.stale = false;
      const target = getRenderTarget();
      resetState();
      setRenderTarget(target);
    };
    for (const name of SETTLES) {
      const f = renderer[name];
      if (typeof f !== "function") continue;
      const bridge = this;
      renderer[name] = function (...args) {
        bridge.settleThree?.();
        return f.apply(this, args);
      };
    }
  }

  settle() {
    this.settleThree?.();
  }

  // Keep the scene's matrix/light hooks and MSAA resolve around a required
  // Rust phase. Incomplete ownership stops drawing with the missing pass.
  cover(name, renderer, scene, camera) {
    if (this.contextLost) return true;

    const target = renderer.getRenderTarget();
    try {
      if (scene.matrixWorldAutoUpdate) scene.updateMatrixWorld();
      scene.onBeforeRender(renderer, scene, camera, target);
      this.prepareScene(renderer, scene, camera, target);
      const passes = this.phases[name];
      for (const pass of passes) pass.complete = false;
      this.publishView(name, renderer, scene, camera);
      const complete = passes.length > 0 && passes.every((pass) => pass.complete === true);
      if (complete) {
        this.resolve(renderer.getRenderTarget());
        if (scene.isScene) scene.onAfterRender(renderer, scene, camera);
      }
      if (!complete) throw new Error(`Incomplete Rust phase ${name}: ${passes.filter((p) => !p.complete).map((p) => p.name).join(", ")}`);
      return true;
    } catch (error) {
      this.fail(error, renderer);
    }
  }

  // An object no Rust pass can claim is not drawn this frame and is reported
  // once (name, type, material key) through the error list and the console.
  // Nothing else draws it and the frame goes on: a missing shader variant or
  // a rejected program stays a hard failure (fail()), a scene object that
  // nobody owns does not stop the picture and HUD for the rest of the battle.
  skip(object, material, why) {
    const material0 = Array.isArray(material) ? material[0] : material;
    const key = material0?.userData?.bfeProgram || material0?.type || "none";
    const text = `Not drawn: ${object.name || "(unnamed)"} ${object.type} material ${key}: ${why}`;
    if (this.skipped.has(text)) return;
    this.skipped.add(text);
    this.errors.push(text);
    console.error(text);
  }

  // Keep the failure visible and stop this battle. No other drawer takes over.
  fail(error, renderer = this.renderer) {
    const failure = rendererFailure(renderer, error);
    this.failure = failure;
    this.errors.push(failure.message);
    const hud = globalThis.document?.querySelector("#hud");
    if (hud) hud.textContent = failure.message;
    throw failure;
  }

  // A multisampled target's samples into its texture, as three's
  // updateMultisampleRenderTarget blits them (r170, one colour texture; the
  // depth too where the target resolves it), with its state cache kept.
  // With three's state stale (a deferred phase), the binds are made directly
  // (three's cache is reset before its next use anyway).
  resolve(target) {
    if (!this.frame?.active) this.resources.resolve(target);
  }

  // Draw every near-scene item through Rust; incomplete ownership is fatal.
  coverNear(renderer, scene, camera) {
    if (this.contextLost) return true;
    if (!this.byName.near.cover(renderer, scene, camera)) this.fail(new Error(`Incomplete Rust near scene: ${this.byName.near.stopped?.name || this.byName.near.stopped?.type || "configuration"}`));
    return true;
  }

  // graphics.js renderCascades: Rust draws every caster in this cascade.
  coverShadow(renderer, light, scene, camera, from) {
    if (this.contextLost) return true;
    try { if (!this.byName.shadow.cover(renderer, light, scene, camera, from)) this.fail(new Error(`Incomplete Rust shadow: ${this.byName.shadow.why}`)); return true; } catch (error) { this.fail(error, renderer); }
  }

  // Draw the complete reflected scene through Rust.
  coverMirror(renderer, scene, camera) {
    if (this.contextLost) return true;
    const pass=this.byName.mirrorCover;
    if (!pass.cover(renderer, scene, camera)) this.fail(new Error(`Incomplete Rust mirror scene: ${pass.stopped?.name || pass.stopped?.type || "configuration"}`));
    return true;
  }

  // hud.js: the battle HUD's batch, drawn by the HUD pass; true when it was.
  coverHud(renderer, hud) {
    if (this.contextLost) return true;
    try { if (!this.byName.hud.cover(renderer, hud)) this.fail(new Error(`Incomplete Rust HUD: ${this.byName.hud.why || "batch"}`)); return true; } catch (error) { this.fail(error, renderer); }
  }

  // The near phase's stats and the time its renderer call took, as the
  // measurement tools read them, and what they read of the passes.
  get stats() { return this.phaseStats.near ?? null; }
  get frameMs() { return this.phaseMs.frame; }
  get worldCount() { return this.byName.world.count; }
  get worldPending() { return this.byName.world.pending; }
  get bodyRecords() { return this.byName.instances.bodyRecords; }
  get meshRecords() { return this.byName.instances.meshRecords; }

}
