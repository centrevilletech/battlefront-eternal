import {PoseDepthSource as DS} from "./pose-layout.js";
import {admitSkeleton} from "./scene-skeletons.js";
import {sceneLayer,sceneProperty,sceneTransform} from "./scene-changes.js";
import {Family,Mode,S as Selected} from './scene.js';
import {ACTOR} from './actor-layout.js';
import {nativeActorSource,actorSkin,publishActorSkin} from '../actor-resources.js';
import {actorSample,isActorMaterial} from './actors.js';
// The shadow pass (shadow.rs): the sun's shadow cascades drawn by the renderer
// in place of three's shadow pass. graphics.js renderCascades offers it each
// cascade's pass (cover): the static cache the frame its window lands (its
// casters on the static layer), and every frame (the last cascade every other
// one) the live map: the cache's depth copied in, then the moving casters.
// A pass refuses any caster without a complete source-owned descriptor;
// graphics.js reports the refusal instead of issuing browser shadow draws.
//
// The scene walk follows WebGLShadowMap's visibility, layer and frustum
// rules. Custom depth shells name Rust programs and carry material values.
// Static casters carry native descriptor ids; Rust selects their cell ranges.
// Skeleton bones and dynamic geometry buffers retain their
// existing upload contracts; no browser shader compilation is needed.
import * as THREE from "three";
import { STATIC } from "./static-layout.js";
import { ownedProgram } from "./programs.js";
import { lighting } from "../materials.js";
import { writeUniform } from "./post.js";

const SHADOW_SIDE = { [THREE.FrontSide]: THREE.BackSide, [THREE.BackSide]: THREE.FrontSide, [THREE.DoubleSide]: THREE.DoubleSide };
// An attribute's buffer source (an interleaved one's shared buffer).
const sourceOf = (a) => (a?.isInterleavedBufferAttribute ? a.data : a);

// A source's new contents (s: its buffer's slot, -1 for the index, the
// version and size it was sent with, and how much of it from the start is
// valid) as three's WebGLAttributes sends them: its update ranges, sorted and
// merged as three merges them (adjacent ones too), else all of it, into the
// buffer as made (bufferSubData); all of it into a buffer made again when its
// array is no longer that size. The ranges stay (three clears them when it
// sends them itself), so they say what changed only when the version moved
// by one since the pass sent it: else (three sent one between, clearing its
// ranges, or the page marked it twice) it is all of it. An index without
// ranges is sent only as far as the pass's draws read it (need), the rest
// when a draw reads further (geometry): the crowd's mesh draws a tenth of its
// index. Returns how much of it from the start is valid now.
function send(rust, id, s, need = Infinity) {
  const array = s.source.array, per = array.BYTES_PER_ELEMENT;
  const view = (start, count) => new Uint8Array(array.buffer, array.byteOffset + start * per, count * per);
  const part = (start, count) => {
    if (s.slot < 0) rust.geometry_index_sub_data(id, start * per, view(start, count));
    else rust.geometry_buffer_sub_data(id, s.slot, start * per, view(start, count));
  };
  if (array.byteLength !== s.bytes) {
    if (s.slot < 0) rust.geometry_index_data(id, view(0, array.length));
    else rust.geometry_buffer_data(id, s.slot, view(0, array.length));
    return array.length;
  }
  const ranges = [], marked = s.source.version === s.version + 1 ? s.source.updateRanges || [] : [];
  for (const r of [...marked].sort((a, b) => a.start - b.start)) {
    const last = ranges[ranges.length - 1];
    if (last && r.start <= last.start + last.count + 1) last.count = Math.max(last.count, r.start + r.count - last.start);
    else ranges.push({ start: r.start, count: r.count });
  }
  if (!ranges.length) {
    const n = s.slot < 0 ? Math.min(need, array.length) : array.length;
    if (n > 0) part(0, n);
    return n;
  }
  let valid = s.valid;
  for (const { start, count } of ranges) {
    const n = Math.min(count, array.length - start);
    if (start < 0 || n <= 0) continue;
    part(start, n);
    if (start <= valid) valid = Math.max(valid, start + n);
  }
  return valid;
}

// shadow.rs's layout.
const S = { TARGET: 0, FROM: 1, SIZE: 2, DRAWS: 3, FIRST: 4 };
const D = { PROGRAM: 0, GEOMETRY: 1, START: 2, COUNT: 3, SIDE: 4, MIRRORED: 5, UNIFORMS: 6, FIRST: 7 };
const UNIFORM = 18, MAX_UNIFORMS = 8, DRAW = D.FIRST + MAX_UNIFORMS * UNIFORM, MAX_DRAWS = 256;
const SAMPLER = 32;

class ShadowPass {
  constructor(core) {
    this.core = core;
    this.name = "shadow";
    this.phase = "shadow";
    this.request = null;
    this.draws = [];
    this.done = false;
    this.input = false;
    this.geometries = new Map(); // "geometry uuid:program" -> { id, sources, index } (geometry())
    this.boneFrames = new WeakMap(); // skeleton -> the render count its bones were worked out at
    this.found = new Map(); // a pass camera's layer mask -> this frame's casters it sees (candidates)
    this.drawn = 0;
    this.left = 0;
    this.why = null;
    this.actorNodes=new WeakMap();core.addView("actorBones",Float32Array,()=>core.rust.actor_bones_ptr(),()=>core.rust.actor_bones_len());
    this.nativeNodes = new Map();
    this.nativeRoots = new Map();
    this.capacity = MAX_DRAWS;
    this.clipped = 0;
    this.reserveCommands(MAX_DRAWS);
  }

  reserveCommands(capacity) {
    this.capacity = capacity;
    this.scratch = new Float64Array(S.FIRST + capacity * DRAW);
    this.commandViews = Array.from({length:capacity + 1}, (_, n) => this.scratch.subarray(0, S.FIRST + n * DRAW));
  }

  // buildWorld publishes immutable sources while loading. GPU depth work
  // waits for native program preparation, where loss can suspend and resume.
  queueNative(roots){for(const [source,root]of roots)this.nativeRoots.set(source,root);this.nativePending=true;}
  finishNative(){if(!this.nativePending)return;this.admitNative(JSON.parse(this.core.rust.static_depth_finish()),this.nativeRoots);this.nativePending=false;}

  // Native static casters keep only scene anchors and numeric resource ids.
  // Descriptor admission, cells, uniforms and GPU restoration belong to Rust.
  admitNative(metadata, roots) {
    const core = this.core, g = core.graphics;
    this.nativeRoots = roots;
    if (!this.nativeInput) {
      core.addView("staticShadowInput", Float64Array, () => core.rust.static_shadow_input_ptr(), () => core.rust.static_shadow_input_len());
      this.nativeInput = true;
    }
    this.reserveCommands(metadata.capacity);
    core.rust.frame_shadow_reserve_frame();
    for (const row of metadata.nodes) {
      let node = this.nativeNodes.get(row.id);
      if (!node && !row.active && row.source !== null) continue;
      if (!node) {
        node = new THREE.Object3D(); node.name = `native-static-depth:${row.id}`;sceneProperty(node,"matrixAutoUpdate",false);
        node.userData.nativeShadow = { native: row.id };
        this.nativeNodes.set(row.id, node);
        g.caster(node);
      }
      const parent = row.source === null ? g.scene : roots.get(row.source);
      if (parent && node.parent !== parent) parent.add(node);
      sceneLayer(node.layers,"set",row.layer);sceneProperty(node,"visible",row.active);core.registry.bind(node,Family.SHADOW,node.userData.nativeShadow);
    }
    this.found.clear();
    for (const window of g.cascadeWindows) window.invalidate();
  }

  // Compile source-owned depth variants before the battle's first cascade.
  prepare() {
    this.finishNative();
    const graphics = this.core.graphics;
    for (const target of [...graphics.shadowMaps, ...graphics.shadowCaches]) this.core.renderer.initRenderTarget(target);
    const depths = new Set(this.core.graphics.depthMaterials.values());
    graphics.scene.traverse(object => { if (object.customDepthMaterial) depths.add(object.customDepthMaterial); });
    for (const depth of depths) {
      if (depth.userData.bfeProgram) ownedProgram(this.core, depth.userData.bfeProgram, null);
    }
  }

  // graphics.js renderCascades: one cascade's pass (light: the cascade's; its
  // shadow's map is drawn into; from: the target whose depth is copied in
  // first, the cache, or null to clear), drawn here when every caster three
  // would draw is one the renderer can draw; true when it was.
  cover(renderer, light, scene, camera, from) {
    const core = this.core;

    const shadow = light.shadow;
    if (!shadow?.map?.depthTexture || shadow.mapSize.x !== shadow.mapSize.y || (from && !from.depthTexture)) return false;
    renderer.initRenderTarget(shadow.map);
    if (from) renderer.initRenderTarget(from);
    this.request = { renderer, light, scene, camera, from };
    this.done = false;
    try {
      core.publishView("shadow", renderer, scene, camera);
    } finally {
      this.request = null;
    }
    if (this.done) { sceneProperty(shadow,"needsUpdate",false); this.drawn++; } else this.left++;
    return this.done;
  }


  // The phase: the walk, registration, and the pass's input.
  draw() {
    const req = this.request;
    if (!req) return;
    this.finishNative();
    const core = this.core, { renderer, light, scene, camera, from } = req, shadow = light.shadow;
    // As three's pass does for the light: its camera placed, its frustum.
    core.cameras.shadow(light);
    const shadowCamera = shadow.camera, frustum = shadow.getFrustum();
    this.draws.length = 0;
    this.why = null;
    this.publishCasters(scene, camera, shadowCamera, renderer);
    const target = this.why ? -1 : core.texture(shadow.map.depthTexture);
    const source = from ? core.texture(from.depthTexture) : -1;
    // The pass is written into a buffer of the page's first: a value's texture
    // registered on the way (core.texture) can grow the renderer's memory,
    // which detaches every view of it; the view is taken once all is written.
    const out = this.scratch;
    let n = 0;
    const ok = () => {
      if (this.why || target < 0 || (from && source < 0)) return false;
      out[S.TARGET] = target; out[S.FROM] = source; out[S.SIZE] = shadow.mapSize.x;
      const count = Math.min(this.draws.length, this.capacity);
      this.clipped += this.draws.length - count;
      for (let i = 0; i < count; i++) {
        const d = this.draws[i], o = S.FIRST + n * DRAW;
        if(d.actor!==undefined){
          out[o+D.PROGRAM]=ACTOR.KIND.ACTOR_DEPTH;out[o+D.GEOMETRY]=d.actor;const a=o+D.FIRST,r=ACTOR.DEPTH,node=d.object;
          const source=core.registry.admitIfNew(node);out[a+DS.SLOT]=source.slot;out[a+DS.GENERATION]=source.generation;out[a+DS.ROLE]=core.cameras.query(shadowCamera);
          if(node.skeleton)publishActorSkin(core,node.skeleton,core.rust.scene_stamp_frame(false));
          n++;continue;
        }
        if (d.native !== undefined) {
          out[o + D.PROGRAM] = -1;out[o + D.GEOMETRY] = d.native;out[o + D.UNIFORMS] = 0;
          n++;continue;
        }
        const { program } = d;
        out[o + D.PROGRAM] = program.id; out[o + D.GEOMETRY] = d.geometry; out[o + D.START] = d.start; out[o + D.COUNT] = d.count;
        out[o + D.SIDE] = d.side; out[o + D.MIRRORED] = d.mirrored ? 1 : 0;
        if (program.uniforms.length > MAX_UNIFORMS) { this.why = `${program.uniforms.length} uniforms`; return false; }
        out[o + D.UNIFORMS] = program.uniforms.length;
        for (let k = 0; k < program.uniforms.length; k++) {
          if (!this.uniform(out, o + D.FIRST + k * UNIFORM, k, program.uniforms[k], d, shadowCamera)) { this.why = `no value for ${program.uniforms[k].name}`; return false; }
        }
        n++;
      }
      out[S.DRAWS] = n;
      return true;
    };
    const written = ok();
    if (!this.input) { core.addView("casterInput", Float64Array, () => core.rust.caster_input_ptr(), () => core.rust.caster_input_len()); this.input = true; }
    core.views();
    const input = core.casterInput;
    if (!written) {
      this.why ||= "textures";
      input[S.TARGET] = -1; input[S.DRAWS] = 0;
      return;
    }
    input.set(this.commandViews[n]);
    core.rust.scene_actor_depth_sources_frame?.();core.rust.actor_depth_frame();
    if (this.nativeInput) {
      core.rust.camera_static_shadow_frame(light.userData.nativeCascade);
      core.staticShadowInput[STATIC.SHADOW.TIME] = lighting.time.value;
    }
    this.done = true;
  }

  // three's renderObject, for one cascade: each caster the pass camera's
  // layers see (candidates) in the cascade's frustum, in scene order.
  publishCasters(scene,camera,shadowCamera,renderer){
    const core=this.core,registry=core.registry,count=registry.select(scene,shadowCamera,Family.SHADOW,Mode.SHADOW,camera.layers.mask,this.capacity);
    this.clipped+=registry.lastClipped;
    for(let i=0;i<count;i++){
      core.views();const at=i*Selected.STRIDE,node=registry.nodes[core.sceneOutput[at+Selected.SLOT]],object=node.object;
      if(this.why)return;
      if(object.userData.actorAsset){this.addActor(object,shadowCamera,renderer);continue;}
      if(object.userData.nativeShadow){this.draws.push(object.userData.nativeShadow);continue;}
      if(!object.isMesh){core.skip(object,object.material,'a line or points caster');continue;}
      if(object.isSkinnedMesh)this.bones(object,renderer);
      const group=core.sceneOutput[at+Selected.GROUP],mi=core.sceneOutput[at+Selected.MATERIAL_INDEX];
      this.add(object,object.geometry,group<0?object.material:object.material[mi],group<0?null:object.geometry.groups[group],camera,shadowCamera,renderer);
    }
  }

  // Actor depth slots and resources are cold native descriptors. A live view
  // reads only pose matrices and the retained skin transport, never materials.
  addActor(node,camera,renderer){
    const look=Array.isArray(node.material)?node.material[0]:node.material;if(!isActorMaterial(look))return;
    let record=this.actorNodes.get(node);
    if(!record||record.appearance!==node.material||record.skeleton!==node.skeleton){
      const core=this.core;admitSkeleton(core.registry,core.registry.admitIfNew(node));const source=nativeActorSource(core,node.userData.actorAsset),skin=node.skeleton?actorSkin(core,node.skeleton):null;
      if(node.skeleton&&!skin){this.clipped++;return;}
      const id=core.rust.actor_depth(source.id,JSON.stringify(look.authored),skin?skin.id:-1,actorSample(core,look));if(id===undefined){this.clipped++;return;}
      record={actor:id,object:node,appearance:node.material,skin,skeleton:node.skeleton};this.actorNodes.set(node,record);core.buffer=null;core.views();
    }
    this.draws.push(record);
  }

  // A caster's draw: its depth material as three's getDepthMaterial dresses it
  // for this material, its program, geometry and range.
  add(object, geometry, material, group, camera, shadowCamera, renderer) {
    if (this.why) return;
    const depth = object.customDepthMaterial;
    if (!depth) { this.core.skip(object, material, "no custom depth material"); return; }
    sceneProperty(depth,"visible",material.visible);
    depth.wireframe = material.wireframe;
    depth.side = material.shadowSide ?? SHADOW_SIDE[material.side];
    depth.alphaMap = material.alphaMap;
    depth.alphaTest = material.alphaTest;
    if(object.userData.fabricWind)depth.userData.bfeProgram=depth.alphaTest>0 ? "actors/depth-fabric-cutout" : "actors/depth-fabric";
    depth.map = material.map;
    if (object.isInstancedMesh || object.isBatchedMesh || object.morphTargetInfluences?.length || depth.wireframe) { this.core.skip(object, material, "caster kind no pass draws"); return; }
    const program = ownedProgram(this.core, depth.userData.bfeProgram, null);
    // Cutout casters can be met before their colour pass, especially during
    // preparation: register the depth material's maps here rather than
    // assuming another pass already published their sampler ids.
    for (const uniform of program.uniforms) if (uniform.type === SAMPLER && uniform.name !== "boneTexture") {
      const texture = depth.uniforms?.[uniform.name]?.value ?? depth[uniform.name];
      if (texture?.isTexture) this.core.texture(texture);
    }
    // renderBufferDirect's range: the draw range within the group, the index.
    const index = geometry.index;
    if (!index) { this.core.skip(object, material, "no index"); return; }
    let start = geometry.drawRange.start, end = geometry.drawRange.start + geometry.drawRange.count;
    if (group) { start = Math.max(start, group.start); end = Math.min(end, group.start + group.count); }
    start = Math.max(start, 0); end = Math.min(end, index.count);
    const g = this.geometry(geometry, program, end > start ? end : 0);
    if (g === null) { this.core.skip(object, material, "geometry the renderer cannot hold"); return; }
    if (end - start > 0 && end - start !== Infinity) {
      this.draws.push({ program, geometry: g, start, count: end - start, side: depth.side, mirrored: this.core.cameras.mirrored(object), object, depth });
    }
  }

  // A skinned caster's bones, worked out once a frame as three's shadow pass
  // does (objects.update), and its bone texture sent as three would bind it.
  bones(object, renderer) {
    const skeleton = object.skeleton, frame = this.core.rust.scene_stamp_frame(false);
    if (this.boneFrames.get(skeleton) !== frame) { skeleton.update(); this.boneFrames.set(skeleton, frame); }
    if (skeleton.boneTexture === null) skeleton.computeBoneTexture();
    this.core.resources.textureId(skeleton.boneTexture);
  }

  geometry(geometry, program, need = Infinity) {
    const core = this.core, key = `${geometry.uuid}:${program.id}`;
    let entry = this.geometries.get(key);
    if (!entry) {
      const id = core.geometry(geometry, { id: program.id, attributes: program.attributes });
      if (id === null || id === undefined) return null;
      // Its buffers' slots as core.geometry made them: each source once, in
      // the program's attribute order, the index last; each with the version
      // and size it was sent with.
      const sources = [];
      for (const [name] of program.attributes) {
        const source = sourceOf(geometry.attributes[name]);
        if (source && !sources.some((s) => s.source === source)) sources.push({ name, source, slot: sources.length, version: source.version, bytes: source.array.byteLength });
      }
      const index = geometry.index;
      entry = { id, sources, index: { source: index, slot: -1, version: index.version, bytes: index.array.byteLength, valid: index.array.length } };
      this.geometries.set(key, entry);
      return id;
    }
    if (geometry.index !== entry.index.source) return null;
    for (const s of entry.sources) if (sourceOf(geometry.attributes[s.name]) !== s.source) return null;
    for (const s of entry.sources) {
      if (s.source.version === s.version) continue;
      send(core.rust, entry.id, s);
      s.version = s.source.version;
      s.bytes = s.source.array.byteLength;
    }
    const x = entry.index, array = x.source.array;
    if (x.source.version !== x.version) {
      x.valid = send(core.rust, entry.id, x, need);
      x.version = x.source.version;
      x.bytes = array.byteLength;
    }
    if (need > x.valid && x.valid < array.length) {
      // A draw reading further than what was sent of this version.
      const end = Math.min(need, array.length), per = array.BYTES_PER_ELEMENT;
      core.rust.geometry_index_sub_data(entry.id, x.valid * per, new Uint8Array(array.buffer, array.byteOffset + x.valid * per, (end - x.valid) * per));
      x.valid = end;
    }
    return entry.id;
  }

  // A draw's uniform as three's renderBufferDirect would set it for the
  // shadow camera: the object's skinning (bind matrices, bone texture), its
  // normal matrix as it stands (three doesn't work it out for a shadow), the
  // banners' clock (the frame's time), else as a full-screen pass's are.
  uniform(out, o, slot, uniform, d, camera) {
    const { name, type } = uniform, object = d.object;
    let value;
    if (name === "bindMatrix") value = object.bindMatrix;
    else if (name === "bindMatrixInverse") value = object.bindMatrixInverse;
    else if (name === "normalMatrix") value = object.normalMatrix;
    else if (name === "fabricTime") value = lighting.time.value;
    else if (name === "boneTexture") {
      const id = this.core.texture(object.skeleton?.boneTexture);
      if (id < 0) return false;
      out[o] = slot; out[o + 1] = SAMPLER; out[o + 2] = id;
      return true;
    } else return writeUniform(this.core, out, o, slot, uniform, d.depth, object, camera);
    out[o] = slot; out[o + 1] = type;
    if (typeof value === "number") { out[o + 2] = value; return true; }
    if (value?.isMatrix4 || value?.isMatrix3) { for (let k = 0; k < value.elements.length; k++) out[o + 2 + k] = value.elements[k]; return true; }
    return false;
  }
}

export function shadowPass(core) {
  return new ShadowPass(core);
}
