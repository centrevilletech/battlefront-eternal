import {nativeActorSource} from '../actor-resources.js';
import {ACTOR} from './actor-layout.js';
// EffectsPass publishes live presentation rows into effects.rs. Native
// descriptors own batch/model geometry, shader variants, raster state, sampler
// identities and uniform layouts. No live path reads Three resource shells.
// The near and reflected views share the renderer's transparent sort stream.
//
// ParticleRenderer, PageEffects, SkyFlyers and the environmental producers
// supply retained arrays. Their changed prefixes are copied into fixed native
// banks, then one phase submits every batch, ribbon, glow and shield model.
// Native skin resources likewise serve view and depth draws without Texture
// objects or version scans. Scene membership and matrices keep their current
// publishers until the retained scene registry consumes their pose identities.
//
// The loading screen warms catalog variants between GPU settling points.
// beforeSort discovers new semantic sources, including a glow first seen by a
// reflected camera, before that view's packet is captured. Cold admission may
// allocate; the frame publisher reuses its arrays and clips excess records.
//
// Volumes and fountain spray retain their own targets. Their composites enter
// the same sorted list as particles, and native commands preserve the placement
// of colour/depth copies. First-person arcs and glow models publish through
// this owner too, using the separate view camera and its ordering boundary.
import { FACTS as F } from "./frame-layout.js";
import * as THREE from "three";
import { EFFECT as BATCH, EFFECT_INPUTS } from "./effect-layout.js";
import { effectProgramKeys } from "./effect-material.js";
import { ownedProgram } from "./programs.js";
import { particleRenderers } from "../particles.js";
import { squareBatches } from "../atmosphere.js";
import { classicLightning } from "../lightning.js";
import { arcBatches } from "../arc-lightning.js";
import { particleDepth } from "../particle-depth.js";

// Capture policy is presentation input. Geometry, shaders, raster state and
// material value layouts are compiled by effect_resources.rs at admission.
const KINDS = {
 waterShore:{beforeRender:true},oceanSpray:{beforeRender:true,copies:"soft"},
 cpu:{copies:"soft"},analytic:{copies:"soft"},square:{copies:"soft"},snow:{copies:"soft"},
 blaster:{beforeRender:true,copies:"soft"},sniperStreaks:{beforeRender:true,copies:"soft"},sniperFlares:{},
 arc:{beforeRender:true,copies:b=>b.soft?"soft":null},arcView:{beforeRender:true,into:true},
 trail:{copies:"soft"},wake:{copies:"soft"},pool:{copies:"soft"},
 flame:{beforeRender:true,depth:true,copies:"always"},
 rain:{static:true,copies:b=>b.values[BATCH.RAIN.ENHANCED]>.5?"soft":null},
 rainSheets:{copies:"soft"},impacts:{copies:"soft"},
 wisps:{static:true,beforeRender:true,depth:true,copies:"always"},
 spray:{static:true,beforeRender:true,depth:true,copies:"always"},
 jets:{static:true,depth:true,copies:"always"},volume:{into:true},
 volumeComposite:{depth:true,copies:"always"},jetComposite:{depth:true,copies:"always"},
};

const LIGHTNING=EFFECT_INPUTS[BATCH.LIGHTNING.ID];
// The frame's inputs (effects.rs IN_*, OWNER_*, DRAW_*).
const IN_DRAWS = 0, IN_OWNERS = 1, IN_SOFT = 2;
const OWNER_AT = 8, OWNER_STRIDE = 4, MAX_OWNERS = 8;
const DRAW_AT = OWNER_AT + MAX_OWNERS * OWNER_STRIDE, DRAW_STRIDE = 12, DRAW_KEY = 6, MAX_DRAWS = 512;
const KIND_BATCH = 0, KIND_MESH = 1, KIND_MODEL = 2, KIND_COPY = 3;
// Floats of an emitter row (effects-gpu.js).
const EMITTER_FLOATS = 12;
const center = new THREE.Vector4();

// Batch identity is published by the producer, independent of its arrays.
function kindOf(batch) {
  return batch.family || (batch.nativeKind === BATCH.BLASTER.ID ? "blaster" : "cpu");
}

// src[from..to) into data from `at`, without a view for a short run; where
// it ends.
function copy(data, at, src, from, to) {
  const n = to - from;
  for (let i = 0; i < n; i++) data[at + i] = src[from + i];
  return at + n;
}
// The same for 32-bit indices (kept apart: one array type each, for the JIT).
function copyBits(bits, at, src, from, to) {
  const n = to - from;
  for (let i = 0; i < n; i++) bits[at + i] = src[from + i];
  return at + n;
}

// A record's copies (KINDS' copies, SHADER_COPIES): "soft", "always" or null.
function copiesOf(record) {
  const copies = record.copies;
  return typeof copies === "function" ? copies(record.batch) : copies;
}

// What the pass asks of the scene's depth for a batch of a kind (KINDS'
// copies; a shader model's by its mesh's name), as for its records.
export function kindCopies(kind, batch = null) {
  const copies = KINDS[kind]?.copies ?? null;
  return typeof copies === "function" ? copies(batch) : copies;
}

// The copy a draw not asked for takes now (drawFrame): the strongest of
// those seen so far ("always" over "soft"), or null.
function lateOf(record, late) {
  const mode = copiesOf(record);
  return late === "always" || !mode ? late : mode;
}

// The order of the nearest Group above an object, as three's projectObject
// passes it down (0 without one).
function groupOrderOf(object) {
  for (let o = object.parent; o; o = o.parent) if (o.isGroup) return o.renderOrder;
  return 0;
}

class EffectsPass {
  constructor(core) {
    this.core = core;
    this.name = "effects";
    this.phase = "transparent";
    this.records = []; // registered batches
    this.byBatch = new Map(); // batch -> its registered record
    this.viewBatches = new WeakMap();
    this.owners = new Map(); // GpuEffects -> { id }
    this.ownerList=[];
    this.meshes = new Map(); // the lightning's material -> its record, or null
    this.shaderMeshes = new Map(); // the death camera's trace -> its record, or null
    this.glows = new Map(); // a glow model's part -> its registered record
    this.glowList = []; // the records, in registration order
    // This frame's batches and glow models drawn (kept, not made each frame).
    this.drawn = [];
    this.glowing = [];
    this.traced = [];
    // The loading screen links the variants the settings use, one at a time
    // (warmEffectPrograms). A shadow setting changed later queues the other
    // set, linked one program per frame in beforeSort, never all at once.
    this.shadows = core.shadowsOn();
    this.linkQueue = [];
    this.ownedInitial = true;
    this.collectOwned = (root) => root.traverse((mesh) => {
      if(mesh.userData.effectModel?.family!=="ocean"&&mesh.userData.effectModel&&!this.glows.has(mesh))this.registerNativeModel(mesh.userData.effectModel);
      if(mesh.userData.effectMesh&&!this.shaderMeshes.has(mesh))this.registerDynamic(mesh.userData.effectMesh);
      const batch=mesh.userData.effectBatch;
      if(batch && batch.family!=="arcView" && !this.byBatch.has(batch) && !this.waiting.has(batch)) this.waiting.set(batch,()=>this.registerBatch(batch,batch.family,null));
    });
    this.batchCount = -1;
    this.waiting = new Map(); // batch -> its registration, run in beforeSort once its geometry is ready
    this.frame = 0;this.clipped=0;this.nativeCapacity=1<<20;
    // What the measurement tools read: last frame's draws and instances.
    this.draws = 0;
    this.instances = 0;
    // The scene's depth asked for before the first of the frame's draws that
    // take it, where three would take it (particle-depth.js requester).
    this.depthKey = new Float64Array(6);
    this.requested = [];
    this.unrequest = core.graphics?.scene ? particleDepth.requester(core.graphics.scene, (camera) => {
      this.requestDepth(camera);
    }) : null;
    core.addView("fxInput", Float64Array, () => core.rust.fx_input_ptr(), () => core.rust.fx_input_len());
    core.addView("fxData", Float32Array, () => core.rust.fx_data_ptr(), () => core.rust.fx_data_len());
    core.addView("fxBits", Uint32Array, () => core.rust.fx_data_ptr(), () => core.rust.fx_data_len());
    core.addView("fxMatrices", Float64Array, () => core.rust.fx_matrices_ptr(), () => core.rust.fx_matrices_len());
  }

  // Register new source-owned meshes before the scene is listed. Textures
  // still loading remain pending; no browser compilation is requested.
  beforeSort(renderer, scene, camera, target, added) {
    const graphics = this.core.graphics;
    if (scene !== graphics.scene || (camera !== graphics.camera || target !== graphics.sceneTarget) && camera.userData?.mirror !== true) return;
    try {
      if (this.core.shadowsOn() !== this.shadows) {
        this.shadows = !this.shadows;
        this.linkQueue = effectProgramKeys(this.core).filter((key) => !this.core.ownedPrograms?.has(key)).reverse();
      }
      const queued = this.linkQueue.pop();
      if (queued) ownedProgram(this.core, queued, null);
      const collect = this.collectOwned;
      if (this.ownedInitial) { collect(scene); this.ownedInitial = false; }
      else for (const root of added || []) collect(root);

    } catch (error) {
      this.core.errors.push(`effects before sort failed: ${String(error?.stack || error).slice(0, 400)}`);
      throw error;
    }
  }

  // The steam's volumes into their target, as the composite's onBeforeRender
  // would have three draw them (environment-volume.js renderVolumes): the
  // scene's depth taken and the target sized by the volumes' prepare(), then
  // the renderer's own call (fx_into_frame), after which three binds the
  // frame's target again. False when three is to draw them (their composite's
  // onBeforeRender).
  drawVolumes(ctx, record) {
    const core = this.core, renderer = ctx.renderer, volume = record.source, camera = ctx.camera;
    let ready = false;
    record.prepareContext=ctx;
    record.prepareVolume ||= () => { const c=record.prepareContext; record.ready=record.source.prepare(c.renderer,c.scene,c.camera); };
    particleDepth.early(record.prepareVolume); ready=record.ready;
    if (!ready) return true;
    // (None: the target cleared all the same, as three's render of the
    // empty volumes' scene would leave it.)
    const b = record.batch, count = Math.max(0, b.count), source = renderer.getRenderTarget();
    const target = volume.target;
    if(target.nativeTarget===undefined && !renderer.properties.get(target.texture).__webglTexture) renderer.initRenderTarget(target);
    const texture = core.texture(target.texture);
    if (texture < 0) return false;
    const depth = particleDepth.map.value ? core.texture(particleDepth.map.value) : -1;
    const need = count * record.stride + record.valueCount;
    core.views();
    if (need > core.fxData.length) { this.clipped++; return true; }
    const data = core.fxData;
    let at = 0;
    for (let k = 0; k < record.attributes.length; k++) at = copy(data, at, b.arrays[record.attributes[k][0]], 0, count * record.sizes[k]);
    at=copy(data,at,b.values,0,record.valueCount);
    this.writeSoft(depth);
    core.frame.volume(record.id,texture,target,count,b.capacity,at);
    record.capacity = b.capacity;
    renderer.resetState();
    renderer.setRenderTarget(source);
    return true;
  }

  // The first-person arc uses the same owned batch program and upload as
  // the near arcs, at its position among the view's transparent draws.
  viewBatch(mesh) {
    let record = this.viewBatches.get(mesh);
    if (!record) {
      const arc = arcBatches.get(mesh);
      if (!arc) throw Error("Unknown first-person arc batch");
      record = this.registerBatch(arc, "arcView", null);
      if (!record) throw Error("First-person arc batch failed to register");
      this.viewBatches.set(mesh, record);
    }
    return record;
  }

  writeViewBatch(record, camera, at) {
    const core = this.core, batch = record.batch, count = Math.min(batch.count,batch.capacity);
    record.mesh.onBeforeRender(core.renderer, core.graphics.viewScene, camera, batch.geometry, batch.material, null);
    let need = at + count * record.stride + (record.valueCount || 0);
    for (const [, size] of record.values) need += size;
    core.views();
    if (need > core.fxData.length) {this.clipped++;return at;}
    at = copy(core.fxData, at, batch.data, 0, count * record.stride);
    return copy(core.fxData,at,batch.values,0,record.valueCount);
  }

  // The frame's soft particle values for the renderer (effects.rs IN_SOFT):
  // whether soft particles are on, the depth's size, the camera's range, the
  // scene's depth (registered texture, -1 none).
  writeSoft(depth) {
    const input = this.core.fxInput;
    input[IN_SOFT] = particleDepth.enabled.value;
    input[IN_SOFT + 1] = particleDepth.resolution.value.x; input[IN_SOFT + 2] = particleDepth.resolution.value.y;
    input[IN_SOFT + 3] = particleDepth.cameraRange.value.x; input[IN_SOFT + 4] = particleDepth.cameraRange.value.y;
    input[IN_SOFT + 5] = depth;
  }

  // Retire detached glow anchors without allocating another render list.
  // First-person sources share this cache and remain while in the view scene.
  listed(ctx) { this.retireModels(ctx.scene); }
  retireModels(scene) {
    if (this.frame % 600) return;
    for(let i=this.glowList.length-1;i>=0;i--){const record=this.glowList[i];let root=record.mesh;while(root.parent)root=root.parent;
      if(root===scene||root===this.core.graphics.viewScene)continue;this.glows.delete(record.mesh);this.glowList.splice(i,1);
    }
  }

  // Stats inspect current scene owners, without retaining old map batches.
  // The native sniper counter includes event and GPU-prefix clipping.
  sniperDropped(){
    const renderers=particleRenderers.get(this.core.graphics.scene);let count=0;
    if(renderers)for(let i=0;i<renderers.length;i++)for(const batch of renderers[i].batches.values())count+=batch.droppedCount||0;
    return count;
  }

  batches() {
    const scene = this.core.graphics.scene;
    const renderers = particleRenderers.get(scene) || [], haze = squareBatches.get(scene) || [];
    let n = haze.length;
    for (const r of renderers) n += r.batches.size;
    if (n !== this.batchCount) {
      this.batchCount = n;
      for (const r of renderers) {
        for (const batch of r.batches.values()) {
          if (this.byBatch.has(batch) || this.waiting.has(batch)) continue;
          // The Enhanced sniper's trace: two batches of its own.
          if (batch.streaks?.data && batch.flares?.data) {
            this.byBatch.set(batch, null);
            for (const [part, kind] of [[batch.streaks, "sniperStreaks"], [batch.flares, "sniperFlares"]]) this.waiting.set(part, () => this.registerBatch(part, kind, r));
            continue;
          }
          const kind = kindOf(batch);
          if (!kind) this.byBatch.set(batch, null);
          else this.waiting.set(batch, () => this.registerBatch(batch, kind, r));
        }
      }
      for (const batch of haze) if (!this.byBatch.has(batch) && !this.waiting.has(batch)) this.waiting.set(batch, () => this.registerBatch(batch, "square", null));
    }
    // Those waiting for three to draw them first (the Enhanced bolts' batch
    // never is in Classic): a look at their program each frame.
    for (const [batch, register] of this.waiting) {
      const record = register();
      if (record === undefined) continue;
      this.byBatch.set(batch, record);
      this.waiting.delete(batch);
    }
  }

  // The texture asset may still be loading; admission otherwise has no
  // dependency on a browser geometry or material descriptor.
  registerBatch(batch, kind) { return this.registerNativeBatch(batch, kind); }

  // Batch, SquareBatch and BlasterBatch now provide only domain arrays and
  // a scene anchor. The cold Rust compiler owns every GPU descriptor.
  registerNativeBatch(batch, kind) {
    const core = this.core, spec = KINDS[kind];
    const sample = batch.texture ? core.renderer.resources.staticSample(batch.texture) : -1;
    const texture = sample >= 0 ? core.texture(sample) : -1;
    let samplers=new Int32Array(0);
    if(batch.prepareResources){
      const samples=batch.prepareResources(core.renderer.resources);samplers=new Int32Array(samples.length);
      for(let i=0;i<samples.length;i++)samplers[i]=samples[i]>=0?core.texture(samples[i]):-1;
    }
    if(batch.waterOwner){batch.waterOwner.prepareResources(core);batch.values.set(core.rust.water_values(batch.waterOwner.sourceId,batch.nativeKind));}
    const owner=kind==="analytic"?this.owner(batch.owner):-1;
    if(owner>=MAX_OWNERS){batch.external=true;batch.mesh.visible=false;this.clipped++;return null;}
    const id = core.rust.effect_batch(batch.nativeKind, batch.variant, core.shadowsOn(), texture, batch.capacity, owner, samplers);
    const interleaved = Boolean(batch.data);
    const attributes=interleaved||core.rust.effect_fixed(id)?[]:EFFECT_INPUTS[batch.nativeKind];
    const key = new Float64Array(6); this.keyOf(batch.mesh, key);
    const record = { batch, kind, id, native: true, mesh: batch.mesh, attributes,
      sizes: attributes.map(([,size])=>size), stride: interleaved ? batch.stride : attributes.reduce((n,[,size])=>n+size,0), values: [], samplers: [],
      valueCount: batch.values.length, multiplier: core.rust.effect_multiplier(id), jets:batch.jetsOwner||null, key, beforeRender: Boolean(spec.beforeRender), static: Boolean(spec.static), versioned: batch.serial !== undefined,
      interleaved, depth: Boolean(spec.depth), copies: batch.soft===false ? null : spec.copies, requested: false, hiddenBy: batch.visibility || "external", synced: false, capacity: batch.capacity, from: this.frame };
    this.nativeCapacity+=batch.capacity*record.stride+record.valueCount;
    core.rust.fx_reserve(Math.max(1<<22,this.nativeCapacity));
    // Cold admission can move the Vec inside already reserved WASM pages.
    // Buffer identity alone cannot detect that change (EffectsPass.views).
    core.buffer=null;core.views();
    record.prepareContext=null;
    record.prepareVolume=()=>{const c=record.prepareContext;record.ready=record.source.prepare(c.renderer,c.scene,c.camera);};
    record.before=()=>{const c=this.context;record.mesh.onBeforeRender(c.renderer,c.scene,c.camera);};
    record.prepareJets=()=>{const c=this.context;record.jets.prepare(c.renderer,c.camera);};
    if(spec.into)return record;
    if(batch.volumeBatch){record.volume=this.registerNativeBatch(batch.volumeBatch,"volume");record.volume.source=batch;}
    if(!batch.visibility){batch.external = true;batch.mesh.visible = false;}
    this.records.push(record); return record;
  }

  // An owner of analytic batches (effects-gpu.js GpuEffects), registered once.
  owner(gpu) {
    let o = this.owners.get(gpu);
    if (!o) {
      const tracks=this.core.texture(gpu.prepareResources(this.core.renderer.resources));
      o={id:this.core.rust.fx_add_owner(tracks,gpu.trackRows),gpu};
      this.ownerList.push(o);
      this.owners.set(gpu, o);
    }
    return o.id;
  }

  // Register Classic lightning before its first draw; Enhanced lightning
  // belongs to its separate effect batch.
  lightning() {
    const lightning = classicLightning.get(this.core.graphics.scene);
    if (!lightning || lightning.isEnhanced()) return null;
    if(!lightning.map)return null;
    let record=this.meshes.get(lightning.map);
    if(!record){
      const core=this.core,sample=core.renderer.resources.lightningSample(lightning.map),texture=core.texture(sample);
      const id=core.rust.effect_mesh(lightning.nativeKind,core.shadowsOn(),texture,lightning.positions.length/3,lightning.indices.length,lightning.map.matrix.elements);
      core.buffer=null;core.views();
      record={id,native:true,mesh:lightning.mesh,attributes:LIGHTNING,values:[],source:lightning};this.meshes.set(lightning.map,record);
    }
    lightning.external=true;lightning.mesh.visible=false;
    const pair=this.classicPair ||= {lightning:null,record:null};pair.lightning=lightning;pair.record=record;return pair;
  }

  // Dynamic ribbons expose only used streams and counts. Cold native admission
  // owns topology, material defaults and the fixed DeathTrace line stream.
  registerDynamic(source) {
    const core=this.core,mesh=source.mesh;
    const attributes=EFFECT_INPUTS[source.nativeKind];
    const vertices=source.positions.length/3,indices=source.indexCapacity??source.indices?.length??6;
    const texture=source.texture?core.texture(core.resources.staticSample(source.texture)):-1;
    const id=core.rust.effect_mesh(source.nativeKind,core.shadowsOn(),texture,vertices,indices,new Float64Array(0));
    core.buffer=null;core.views();
    const record={id,mesh,source,attributes,stride:attributes.reduce((sum,a)=>sum+a[1],0),values:[],valueCount:source.values.length,native:true,from:this.frame};
    record.before=()=>source.prepare?.(this.context.camera);
    this.shaderMeshes.set(mesh,record);
    if(!source.visibility){source.external=true;mesh.visible=false;}
    return record;
  }

  prepareGlow(source,camera,scene){
    const row=source.nativeKind===BATCH.ACTOR_BASIC.ID?BATCH.ACTOR_BASIC:BATCH.ACTOR_GLOW,v=source.values,look=source.owner;
    if(source.prepareActor){source.prepareActor(camera);return;}
    source.mesh.onBeforeRender(this.core.renderer,scene,camera);v.set(camera.projectionMatrix.elements,row.PROJECTION_MATRIX);look.color.toArray(v,row.DIFFUSE);v[row.OPACITY]=look.opacity;
  }
  viewModel(mesh){return this.glows.get(mesh)||this.registerNativeModel(mesh.userData.effectModel,true);}
  writeViewModel(record,camera,at,matrixAt){
    this.prepareGlow(record.source,camera,this.core.graphics.viewScene);const core=this.core;core.views();
    if(at+record.valueCount>core.fxData.length||matrixAt+16>core.fxMatrices.length){this.clipped++;return at;}
    core.fxMatrices.set(record.mesh.matrixWorld.elements,matrixAt);for(let i=0;i<record.valueCount;i++)core.fxData[at+i]=record.data[i];return at+record.valueCount;
  }

  // Basin models publish only authored streams on first admission. Procedural
  // tier topology, shader slots, raster state and sampler policy stay native.
  registerNativeModel(source,view=false) {
    const core=this.core,owner=source.owner,mesh=source.mesh;
    if(source.family==='actorEffect'){
      owner.map=source.gradient?core.resources.gpu.actor_gradient():core.resources.staticSample(owner.map);
      if(source.repeat&&owner.map!=null)owner.map=core.resources.gpu.repeat_sample(owner.map);const texture=owner.map==null?-1:core.texture(owner.map),asset=nativeActorSource(core,source.asset);
      const facts=core.rust.actor_effect(asset.id,source.nativeKind,core.shadowsOn(),view,texture,JSON.stringify(source.authored||{}));if(!facts.length){this.clipped++;return null;}source.center=Array.from(facts.slice(ACTOR.MODEL.CENTER,ACTOR.MODEL.CENTER+3));source.values.set(facts.subarray(ACTOR.MODEL.LEN));
      const record={native:true,source,mesh,model:facts[ACTOR.MODEL.HANDLE],valueCount:source.values.length,values:[],shader:false,data:source.values};
      record.before=()=>this.prepareGlow(source,this.context.camera,this.context.scene);this.glows.set(mesh,record);this.glowList.push(record);core.buffer=null;core.views();return record;
    }
    if(source.family==='water'){
      owner.prepareResources(core);
      const facts=core.rust.effect_water(owner.sourceId,source.nativeKind,false,core.shadowsOn(),0,mesh.id);
      source.center=facts.slice(1,4);source.values.set(facts.subarray?facts.subarray(4):facts.slice(4));
      const record={native:true,source,mesh,model:facts[0],valueCount:source.values.length,values:[],shader:false,data:source.values};
      record.before=()=>source.prepare(this.context.camera);
      this.glows.set(mesh,record);this.glowList.push(record);return;
    }
    const samples=owner.prepareResources(core.resources),samplers=new Int32Array(samples.length);
    for(let i=0;i<samples.length;i++)samplers[i]=samples[i]>=0?core.texture(samples[i]):-1;
    const geometry=mesh.geometry,position=geometry?.attributes.position;
    const vertices=new Float32Array(position?position.count*3:0);
    if(position)for(let i=0;i<position.count;i++){vertices[i*3]=position.getX(i);vertices[i*3+1]=position.getY(i);vertices[i*3+2]=position.getZ(i);}
    const index=geometry?.index?.array,indices=index?new Uint32Array(index):new Uint32Array(0);
    const facts=core.rust.effect_basin(source.tier,owner.tierData,vertices,indices,index instanceof Uint32Array,core.shadowsOn(),samplers);
    core.buffer=null;core.views();
    source.center=facts.slice(1);
    const record={native:true,source,mesh,model:facts[0],valueCount:owner.values.length,values:[],shader:false,data:owner.values};
    record.before=()=>owner.capture(this.context.renderer,this.context.camera);
    this.glows.set(mesh,record);this.glowList.push(record);
  }

  // Shown as three would show it: in the scene, it and its parents visible,
  // in the camera's layers.
  shown(mesh, camera) {
    const scene = this.core.graphics.scene;
    if (!mesh.layers.test(camera.layers)) return false;
    let o = mesh.parent;
    for (; o && o !== scene; o = o.parent) if (!o.visible) return false;
    return o === scene;
  }

  // As three starts the frame's render of the near scene, before it sorts
  // it (particle-depth.js requester): the copy of the scene's depth asked
  // for before each of the frame's draws whose onBeforeRender would take it
  // in three's render (copiesOf), the draws drawFrame will make. Each
  // record's `requested` says whether the copy will be there before it.
  requestDepth(camera) {
    const core = this.core, requested = this.requested;
    for (const record of requested) record.requested = false;
    requested.length = 0;

    for (const record of this.records) {
      const b = record.batch, mode = copiesOf(record);
      if (!mode || b.count <= 0 || !b.mesh.visible && (record.hiddenBy === "material" || record.hiddenBy === "anchor") || !this.shown(record.mesh, camera)) continue;
      if (particleDepth.request(record.key, mode === "always")) { record.requested = true; requested.push(record); }
    }

  }

  // three's sort key for an object (but z, which the renderer works out):
  // group order, render order, its bounding sphere's centre through its
  // world matrix, its id; into out from `at`.
  keyOf(object, out, at = 0) {
    const geometry=object.geometry,c=object.userData.effectModel?.center;
    if(c)center.set(c[0],c[1],c[2],1);
    else if(geometry){if(!geometry.boundingSphere)geometry.computeBoundingSphere();center.copy(geometry.boundingSphere.center);}
    else center.set(0,0,0,1);
    center.applyMatrix4(object.matrixWorld);
    out[at] = groupOrderOf(object); out[at + 1] = object.renderOrder;
    out[at + 2] = center.x; out[at + 3] = center.y; out[at + 4] = center.z; out[at + 5] = object.id;
  }

  draw(ctx) {
    this.drawFrame(ctx);
  }

  drawFrame(ctx) {
    this.context=ctx;
    const core = this.core;
    this.frame++;
    this.batches();
    this.retireModels(ctx.scene);
    const bolt = this.lightning();
    // Textures three uploaded anew, or made anew (a new handle: registered
    // again); looked at when their version moved. Registering may move the
    // renderer's memory: views after.
    // Soft particles: the scene's depth, taken where three would take it,
    // before the first of the frame's draws whose onBeforeRender would ask
    // for it: by particle-depth.js's copier, asked for as three started the
    // render (requestDepth). Their onBeforeRender, called here ahead of their
    // turn, only learn that it will be there; one not asked for (shown since)
    // takes it now, as those that render with it at once do (a copy of their
    // own: the copier's still follows). Bound for the frame's draws when
    // anything drawn reads it.
    const soft = particleDepth.enabled.value > 0.5;
    let reads = soft, late = null;
    // What three would have drawn this frame, registered before it (three
    // drew what was registered this frame itself), and the floats it needs.
    const camera = ctx.camera, drawn = this.drawn, glowing = this.glowing;
    drawn.length = glowing.length = 0;
    let need = 0, jets = null;
    for (const record of this.records) {
      const b = record.batch;
      if (b.count <= 0 || !b.mesh.visible && (record.hiddenBy === "material" || record.hiddenBy === "anchor") || !this.shown(record.mesh, camera)) continue;
      if(drawn.length>=MAX_DRAWS){this.clipped++;continue;}
      drawn.push(record);
      if(record.volume)this.drawVolumes(ctx,record.volume);
      if (record.depth) reads = true;
      if (!record.requested) late = lateOf(record, late);
      // The fountains' jets: their values set, and the target the frame so
      // far goes into sized, once a frame, before their samplers.
      if (record.jets && !jets) { jets = record; particleDepth.ahead(record.prepareJets, record.requested); }
      need += b.count * record.stride;
      need += record.valueCount;
      // Values its onBeforeRender sets, as three would call it.
      if (record.beforeRender) particleDepth.ahead(record.before, record.requested);

    }
    const lit = bolt && bolt.lightning.index > 0 && this.shown(bolt.lightning.mesh, camera) ? bolt : null;
    if (lit) need += lit.lightning.vertex * 9 + lit.lightning.index;
    // The shader meshes shown (the death camera's trace: never culled).
    const traced = this.traced;
    traced.length = 0;
    for (const record of this.shaderMeshes.values()) {
      if (!record) continue;
      const source = record.source;
      if (!(source.index > 0 && (!source.visibility || record.mesh.visible)) || !this.shown(record.mesh, camera)) continue;
      record.before();
      record.vertices = source.vertex;
      record.indices = source.index;
      traced.push(record);
      need += record.vertices*record.stride+(source.indices?record.indices:0)+record.valueCount;
    }
    // Glow models and shields shown (three culls them by their spheres: the
    // renderer does). A shield's onBeforeRender runs first, as three's would
    // (it sets whether the camera is inside it); its texture follows three's.
    for (const record of this.glowList) {
      const source=record.source;
      if (!(source.family!=="water"&&source.visibility!=="anchor"?source.active:record.mesh.visible) || !this.shown(record.mesh,camera))continue;
      glowing.push(record);record.before();need+=record.valueCount;
    }

    if (late) particleDepth.capture(ctx.renderer, ctx.scene, camera, late === "always");
    // The frame's requests used (a mirror's render asks for none).
    if (camera === core.graphics.camera && ctx.renderer.getRenderTarget() === core.graphics.sceneTarget) {
      for (const record of this.requested) record.requested = false;
      this.requested.length = 0;
    }
    const depthCopy = particleDepth.sortedCopy(ctx.renderer, camera);
    // The resource owner allocates a target on first use. The ordered copy below is
    // Rust's, so nothing else would make the depth snapshot before it blits into it.
    if (depthCopy && !ctx.renderer.properties.get(depthCopy.target.depthTexture).__webglTexture) ctx.renderer.initRenderTarget(depthCopy.target);
    const depth = reads && particleDepth.map.value ? core.texture(particleDepth.map.value) : -1;
    for(let i=0;i<this.ownerList.length;i++){const gpu=this.ownerList[i].gpu;need+=Math.min(gpu.count,gpu.emitterRows)*EMITTER_FLOATS;}
    core.views();
    if (need > core.fxData.length) { this.clipped++; core.fxInput[IN_DRAWS]=0; return; }
    const input = core.fxInput, data = core.fxData, bits = core.fxBits, matrices = core.fxMatrices;
    let at = 0, owners = 0, draws = 0, instances = 0;
    for(let i=0;i<this.ownerList.length;i++){
      const o=this.ownerList[i],gpu=o.gpu;
      if(owners>=MAX_OWNERS){this.clipped++;break;}
      const rows = gpu.emitterRows, written = Math.min(gpu.count, rows);
      const e = OWNER_AT + owners++ * OWNER_STRIDE;
      input[e] = o.id; input[e + 1] = rows; input[e + 2] = written; input[e + 3] = at;
      at = copy(data, at, gpu.data, 0, written * EMITTER_FLOATS);
    }
    for (const record of drawn) {
      if(draws>=MAX_DRAWS){this.clipped++;break;}
      const b = record.batch, count = b.count, attributes = record.attributes;
      // The batch's arrays (made anew as it grows).
      // An analytic batch's (or a destruction pool's) instances from the first
      // that changed since it was last read (effects-gpu.js unread; the pool's
      // dirty range), when the renderer holds the rest; all of them otherwise.
      let from = 0;
      if (record.kind === "analytic" || record.kind === "pool") {
        if (record.synced && record.capacity === b.capacity) from = Math.min(Math.max(b.unread ?? 0, 0), count);
        b.unread = Infinity;
      } else if (record.static) {
        // Fixed instances: those it did not have yet.
        if (record.synced && record.capacity === b.capacity) from = Math.min(record.uploaded, count);
        record.uploaded = Math.max(from === 0 ? 0 : record.uploaded, count);
      } else if (record.versioned) {
        // Instances rewritten as they change: none when none did.
        const version = b.serial;
        if (record.synced && record.capacity === b.capacity && version === record.dataVersion) from = count;
        record.dataVersion = version;
      }
      const start = at, sizes = record.sizes;
      if (record.interleaved) at = copy(data, at, b.arrays.interleaved, from * record.stride, count * record.stride);
      else for (let k = 0; k < attributes.length; k++) at = copy(data, at, b.arrays[attributes[k][0]], from * sizes[k], count * sizes[k]);
      at = copy(data, at, b.values, 0, record.valueCount);
      const e = DRAW_AT + draws++ * DRAW_STRIDE;
      input[e] = KIND_BATCH; input[e + 1] = record.id; input[e + 2] = count; input[e + 3] = from; input[e + 4] = start; input[e + 5] = b.capacity;
      input.set(record.key, e + DRAW_KEY);
      record.synced = true;
      record.capacity = b.capacity;
      instances += count * (record.multiplier || 1);
    }
    if(lit&&draws>=MAX_DRAWS)this.clipped++;
    if (lit && draws<MAX_DRAWS) {
      const lightning=lit.lightning,record=lit.record;
      const vertices = lightning.vertex, indices = lightning.index, start = at;
      for(const [name,size]of record.attributes)at=copy(data,at,lightning.arrays[name],0,vertices*size);
      at=copyBits(bits,at,lightning.indices,0,indices);
      const e = DRAW_AT + draws++ * DRAW_STRIDE;
      input[e] = KIND_MESH; input[e + 1] = record.id; input[e + 2] = vertices; input[e + 3] = indices; input[e + 4] = start; input[e + 5] = 0;
      this.keyOf(lightning.mesh, input, e + DRAW_KEY);
    }
    for (const record of traced) {
      if(draws>=MAX_DRAWS){this.clipped++;break;}
      const mesh=record.mesh,vertices=record.vertices,indices=record.indices,start=at;
      for(const [name,size]of record.attributes)at=copy(data,at,record.source.arrays[name],0,vertices*size);
      if(record.source.indices)at=copyBits(bits,at,record.source.indices,0,indices);
      at=copy(data,at,record.source.values,0,record.valueCount);
      const e = DRAW_AT + draws++ * DRAW_STRIDE;
      input[e] = KIND_MESH; input[e + 1] = record.id; input[e + 2] = vertices; input[e + 3] = indices; input[e + 4] = start; input[e + 5] = 0;
      this.keyOf(mesh, input, e + DRAW_KEY);
    }
    // The fountain copier used to be a colorless three draw between scene
    // items. Publish its key alongside the jets so Rust copies at that point.
    if (jets && ctx.renderer.getRenderTarget() === core.graphics.sceneTarget && draws < MAX_DRAWS) {
      const record = jets;
      const target = record.jets.behind, e = DRAW_AT + draws++ * DRAW_STRIDE;
      input[e] = KIND_COPY; input[e + 1] = core.texture(target.texture);
      input[e + 2] = target.width; input[e + 3] = target.height; input[e + 4] = 0; input[e + 5] = 0;
      this.keyOf(record.mesh, input, e + DRAW_KEY);
      input[e + DRAW_KEY + 1] = 9 - 0.025;
      input[e + DRAW_KEY + 2] = 0; input[e + DRAW_KEY + 3] = 0; input[e + DRAW_KEY + 4] = 0;
    }
    let models = 0;
    for (const record of glowing) {
      if(draws>=MAX_DRAWS){this.clipped++;break;}
      const mesh=record.mesh,m=models++*16;
      matrices.set(mesh.matrixWorld.elements, m);
      const start = at;
      at=copy(data,at,record.data,0,record.valueCount);
      const e = DRAW_AT + draws++ * DRAW_STRIDE;
      input[e] = KIND_MODEL; input[e + 1] = record.model; input[e + 2] = m; input[e + 3] = start; input[e + 4] = 0; input[e + 5] = 0;
      this.keyOf(mesh, input, e + DRAW_KEY);
    }
    if (depthCopy) {
      if(draws<MAX_DRAWS){
      const e = DRAW_AT + draws++ * DRAW_STRIDE, m = depthCopy.copier.matrixWorld.elements;
      input[e] = KIND_COPY; input[e + 1] = core.texture(depthCopy.target.depthTexture);
      input[e + 2] = depthCopy.source.width; input[e + 3] = depthCopy.source.height;
      input[e + 4] = 1; input[e + 5] = 0;
      input[e + DRAW_KEY] = depthCopy.group.renderOrder; input[e + DRAW_KEY + 1] = depthCopy.copier.renderOrder;
      input[e + DRAW_KEY + 2] = m[12]; input[e + DRAW_KEY + 3] = m[13]; input[e + DRAW_KEY + 4] = m[14]; input[e + DRAW_KEY + 5] = depthCopy.copier.id;
      }else this.clipped++;
    }
    // The ordered copies read the framebuffer bound when Rust reaches them, so the
    // resource owner's current target is bound for real before that frame runs.
    if (depthCopy || jets) ctx.renderer.setRenderTarget(ctx.renderer.getRenderTarget());
    input[IN_DRAWS] = draws;
    input[IN_OWNERS] = draws ? owners : 0;
    this.writeSoft(depth);
    ctx.fxWords=at;
    if(depthCopy){core.views();core.frameFacts[F.DEPTH_SORTED]=1;}
    this.draws = draws;
    this.instances = instances;
  }


}

export function effectsPass(core) {
  return new EffectsPass(core);
}
