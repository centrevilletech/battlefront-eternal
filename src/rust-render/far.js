import * as THREE from 'three';
import {Family,Mode,S} from './scene.js';
import {sceneProperty,sceneTransform} from './scene-changes.js';
import {ownedEffectProgram} from './effect-material.js';
import {ownsEffectSource} from './programs.js';
import {ownedWorldProgram} from './world-programs.js';
import {VIEW_FAR} from './index.js';
import {EFFECT,EFFECT_INPUTS} from './effect-layout.js';
import {nativeActorSource} from '../actor-resources.js';
import {ACTOR} from './actor-layout.js';
import {STATIC} from './static-layout.js';
import {staticFeatures} from './geometry-types.js';
import {lighting} from '../materials.js';

// FarPass retains only numeric descriptors and sparse view placements. Native
// resource owners retain topology, samplers, raster policy and cell culling.
// add_program's kinds (web/mod.rs).
const WORLD = 0, PLAIN = 3;
// The far input's layout and the uniform sources and item flags (far.rs).
const CLEAR = 0, DRAW = 4, KEEP = 5, MIPMAPS = 6, NEAR = 7, SLOTS = 8;
const SRC = { FLOATS: 0, INPUT: 1, TEXTURE: 2, TEXTURE_INPUT: 3, PROJECTION: 4, VIEW: 5, EYE: 6, MODEL: 7, MODEL_VIEW: 8, NORMAL: 9, CUBE_INPUT: 10 };
const F = { DEPTH_TEST: 4, DEPTH_WRITE: 8, COLOR_WRITE: 16, A2C: 32, TRANSPARENT: 64, PLACED: 128, CULLED: 256, BLEND: 512 };
// The frame block's members (shader.rs FRAME_MEMBERS); those three takes from
// the camera, and the object's matrices.
const FRAME_NAMES = new Set(["projectionMatrix", "viewMatrix", "cameraPosition", "time", "sunDirection", "bump", "sunColor", "shadowStrength",
  "ambient", "aaSamples", "ambientTop", "terrainShade", "ambientBottom", "farAaSamples", "characterAmbient", "cascadeNear", "fogColor",
  "cascadeFar", "fade", "resolution", "fogRange"]);
const CAMERA = { projectionMatrix: SRC.PROJECTION, viewMatrix: SRC.VIEW, cameraPosition: SRC.EYE };
const OBJECT = { modelMatrix: SRC.MODEL, modelViewMatrix: SRC.MODEL_VIEW, normalMatrix: SRC.NORMAL };
// GL uniform types the renderer sets, and their floats an element.
const SAMPLER_2D = 0x8B5E, SAMPLER_CUBE = 0x8B60;
const FLOATS = { 0x1406: 1, 0x8B50: 2, 0x8B51: 3, 0x8B52: 4, 0x1404: 1, 0x8B56: 1, 0x8B5B: 9, 0x8B5C: 16, [SAMPLER_2D]: 1, [SAMPLER_CUBE]: 1 };
const IDENTITY = new THREE.Matrix4();
// The layer a near-scene item the pass draws goes on (no camera renders it:
// graphics.js uses 1, 2 and 4, sky-probe.js 3, water.js 5; instances.js's
// flying parts this one too).
// A uniform's value as the floats three would upload, appended to `out`;
// false when it isn't one.
function floatsOf(value, out) {
  if (typeof value === "number") out.push(value);
  else if (typeof value === "boolean") out.push(value ? 1 : 0);
  else if (value?.isVector2) out.push(value.x, value.y);
  else if (value?.isVector3) out.push(value.x, value.y, value.z);
  else if (value?.isVector4 || value?.isQuaternion) out.push(value.x, value.y, value.z, value.w);
  else if (value?.isColor) out.push(value.r, value.g, value.b);
  else if (value?.isMatrix3 || value?.isMatrix4) for (const v of value.elements) out.push(v);
  else if (Array.isArray(value) || ArrayBuffer.isView(value)) { for (const v of value) if (!floatsOf(v, out)) return false; }
  else return false;
  return true;
}

// A material's blending as three's setMaterial sets it: [equation rgb,
// alpha, source rgb, destination rgb, source alpha, destination alpha], null
// for none, undefined for what the renderer doesn't set (a constant colour).
function blendOf(material, gl) {
  if (material.blending === THREE.NoBlending || (material.blending === THREE.NormalBlending && !material.transparent)) return null;
  const add = gl.FUNC_ADD, pre = material.premultipliedAlpha;
  switch (material.blending) {
    case THREE.NormalBlending: return pre ? [add, add, gl.ONE, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA] : [add, add, gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA];
    case THREE.AdditiveBlending: return pre ? [add, add, gl.ONE, gl.ONE, gl.ONE, gl.ONE] : [add, add, gl.SRC_ALPHA, gl.ONE, gl.SRC_ALPHA, gl.ONE];
    case THREE.SubtractiveBlending: return [add, add, gl.ZERO, gl.ONE_MINUS_SRC_COLOR, gl.ZERO, gl.ONE];
    case THREE.MultiplyBlending: return pre ? [add, add, gl.ZERO, gl.SRC_COLOR, gl.ZERO, gl.SRC_ALPHA] : [add, add, gl.ZERO, gl.SRC_COLOR, gl.ZERO, gl.SRC_COLOR];
    case THREE.CustomBlending: {
      const equations = { [THREE.AddEquation]: gl.FUNC_ADD, [THREE.SubtractEquation]: gl.FUNC_SUBTRACT, [THREE.ReverseSubtractEquation]: gl.FUNC_REVERSE_SUBTRACT, [THREE.MinEquation]: gl.MIN, [THREE.MaxEquation]: gl.MAX };
      const factors = { [THREE.ZeroFactor]: gl.ZERO, [THREE.OneFactor]: gl.ONE, [THREE.SrcColorFactor]: gl.SRC_COLOR, [THREE.SrcAlphaFactor]: gl.SRC_ALPHA,
        [THREE.SrcAlphaSaturateFactor]: gl.SRC_ALPHA_SATURATE, [THREE.DstColorFactor]: gl.DST_COLOR, [THREE.DstAlphaFactor]: gl.DST_ALPHA,
        [THREE.OneMinusSrcColorFactor]: gl.ONE_MINUS_SRC_COLOR, [THREE.OneMinusSrcAlphaFactor]: gl.ONE_MINUS_SRC_ALPHA,
        [THREE.OneMinusDstColorFactor]: gl.ONE_MINUS_DST_COLOR, [THREE.OneMinusDstAlphaFactor]: gl.ONE_MINUS_DST_ALPHA };
      const b = [equations[material.blendEquation], equations[material.blendEquationAlpha || material.blendEquation],
        factors[material.blendSrc], factors[material.blendDst], factors[material.blendSrcAlpha || material.blendSrc], factors[material.blendDstAlpha || material.blendDst]];
      return b.every((v) => v !== undefined) ? b : undefined;
    }
    default: return undefined;
  }
}

// three's depth functions (WebGLState).
const DEPTH = { [THREE.NeverDepth]: 0x0200, [THREE.AlwaysDepth]: 0x0207, [THREE.LessDepth]: 0x0201, [THREE.LessEqualDepth]: 0x0203,
  [THREE.EqualDepth]: 0x0202, [THREE.GreaterEqualDepth]: 0x0206, [THREE.GreaterDepth]: 0x0204, [THREE.NotEqualDepth]: 0x0205 };

class FarPass {
  constructor(core) {
    this.core = core;
    this.name = "far";
    this.phase = "far";
    this.tables=new Map();this.scratch=[];
    this.all=[];this.nextSlot=SLOTS;this.found=[];this.count=0;
    this.color=new THREE.Color();this.complete=false;this.mode='incomplete';this.unclaimed=0;
    this.nativePending = [];
  }

  // The static world no longer constructs browser geometry/material shells.
  // Frame scheduling retains only a scene node and its numeric native slot.
  admit(object, record, vertices, index, map, bump, weights = new Uint8Array(), samplers = null, source = null) {
    const core = this.core, slot = this.nextSlot;
    this.nextSlot += record.terrain ? 2 : 1;
    const item = { id: -1, slot, native: true, terrain: Boolean(record.terrain), object };
    object.userData.nativeFar = item;
    source ??= core.rust.static_source(JSON.stringify(record), vertices, index);
    if (weights.length) core.rust.static_source_weights(source, weights);
    this.nativePending.push({ item, source, tree: Boolean(record.treeCanopies), scale: record.lowResScale || 1, samplers, map, bump });
    this.all.push(item);core.registry.bind(object,Family.FAR,item);
    return item;
  }

  // Water.farPass has no browser geometry or material. Its retained native
  // item reads a complete per-view snapshot, including moving sampler ids.
  admitWater(object,source) {
    const core=this.core;source.owner.prepareResources(core);
    const slot=this.nextSlot;this.nextSlot+=17+source.values.length;
    const facts=core.rust.effect_water(source.owner.sourceId,source.nativeKind,true,source.sceneFar===false?core.shadowsOn():false,slot,object.id);
    source.center=facts.slice(1,4);source.values.set(facts.subarray?facts.subarray(4):facts.slice(4));
    const item={id:facts[0],slot,native:true,object,water:source};object.userData.nativeFar=item;this.all.push(item);core.registry.bind(object,source.sceneFar===false?Family.SEA:Family.FAR,item);
  }

  // StormSky's borrowed environment streams have native geometry/raster
  // descriptors. The existing far bank snapshots their dirty prefix per view.
  admitEffect(object,source){
    const core=this.core,slot=this.nextSlot,vertex=this.nextVertex??0;
    const attributes=EFFECT_INPUTS[source.nativeKind],stride=attributes.reduce((n,a)=>n+a[1],0);
    const capacity=EFFECT.STORM_LIGHTNING.CAPACITY;
    this.nextSlot+=17+source.values.length;this.nextVertex=vertex+capacity*stride;
    const id=core.rust.effect_far_mesh(source.nativeKind,core.shadowsOn(),slot,vertex,object.id);
    const item={id,slot,native:true,object,effect:source,vertex,attributes,capacity,serial:NaN};
    object.userData.nativeFar=item;this.all.push(item);core.registry.bind(object,Family.FAR,item);core.buffer=null;core.views();
  }

  // Flyer resources use a native recipe; only the environment arrays and
  // current view facts are copied into the retained far input bank.
  admitFlyer(object,source){const core=this.core,slot=this.nextSlot,capacity=Math.min(source.capacity,256);this.nextSlot+=17+ACTOR.FLYER.LEN+capacity*20;
    source.map=core.resources.staticSample(source.map);const texture=source.map==null?-1:core.texture(source.map);
    const id=core.rust.actor_flyer(nativeActorSource(core,source.asset).id,source.flags,source.fading,capacity,texture,slot,object.id,core.shadowsOn());
    const item={id,slot,native:true,object,flyer:source,capacity};object.userData.nativeFar=item;this.all.push(item);core.registry.bind(object,Family.FAR,item);core.buffer=null;core.views();}

  prepareNative() {
    const core = this.core;
    for (const cold of this.nativePending) {
      const { item, source, tree, scale, map, bump } = cold;
      const samplers = cold.samplers || new Int32Array(STATIC.SAMPLERS.LENGTH).fill(-1);
      if (!cold.samplers) {
        samplers[STATIC.SAMPLERS.MAP] = map == null ? -1 : core.texture(core.resources.staticSample(map));
        samplers[STATIC.SAMPLERS.BUMP] = bump == null ? -1 : core.texture(core.resources.staticSample(bump));
        if (map == null) samplers[STATIC.SAMPLERS.EMPTY] = core.texture(core.resources.gpu.world_fallback(false));
        if (tree) samplers[STATIC.SAMPLERS.TREE] = core.texture(core.graphics.treeCanopies.stateMap);
      }
      item.id = core.rust.static_world(source, true, scale, samplers, staticFeatures(core, true), item.object.userData.worldOrder, item.slot, item.object.id);
    }
    this.nativePending.length = 0;
  }

  register() {
    this.core.addView("farInput", Float64Array, () => this.core.rust.far_input_ptr(), () => this.core.rust.far_input_len());
    this.core.addView("farVertices", Float32Array, () => this.core.rust.far_vertices_ptr(), () => this.core.rust.far_vertices_len());
  }

  // Register new resources and write the frame inputs. Every visible item
  // needs a Rust draw; incomplete coverage fails the battle.
  draw(ctx) {
    this.frame(ctx);
  }

  // The same resource table serves main far views, reflected views and probes.
  // Only frame/camera inputs change; registration must not depend on which view
  // happened to draw an object first. Every selected item remains Rust-owned.
  frame(ctx) {
    if (this.nativePending.length) this.prepareNative();
    const { renderer, scene, camera } = ctx, core = this.core;
    this.near = false;
    this.count = 0;
    this.unclaimed = 0;
    this.camera = camera;
    this.scene = scene;
    // A sky probe's face (sky-probe.js): which one (far_probe_frame).
    const cube = camera.parent?.type === "CubeCamera" ? camera.parent : null;
    const nativeProbe=camera.userData.nativeProbe;
    ctx.face=nativeProbe?nativeProbe.face:cube?Math.max(0,cube.children.indexOf(camera)):0;
    // Its cube's mipmaps, made after the face drawn now as three's render
    // makes them (updateRenderTargetMipmap: whenever the texture asks for
    // them; CubeCamera.update holds them back until the last face): 1 + the
    // texture's id, else 0. Registered here, before the input is written
    // (registering can move the module's memory).
    const texture = cube?.renderTarget?.texture;
    this.mipmaps=nativeProbe?(nativeProbe.last?1+core.texture(nativeProbe.target.texture):0):texture?.generateMipmaps?1+core.texture(texture):0;
    const registry=core.registry,count=registry.select(scene,camera,Family.FAR,Mode.FAR);
    for(let i=0;i<count;i++){core.views();const object=registry.nodes[core.sceneOutput[i*S.STRIDE+S.SLOT]].object,item=object.userData.nativeFar||core.registry.admitIfNew(object).records[Family.FAR];
      if(!item){core.skip(object,object.material,'far resource has no native descriptor');this.unclaimed++;continue;}
      const found=this.found[this.count]??={};found.object=object;found.item=item;this.count++;
    }
    const complete = this.unclaimed === 0;

    if(complete)this.prepare(renderer,scene,camera);
    this.write(renderer, complete);
    this.complete = complete;
    this.mode = complete ? "full" : "incomplete";
  }

  registerItem(object, material, threeProgram, groupOrder, camera) {
    const core = this.core, rust = core.rust, gl = core.gl;
    const mesh = object;
    if (!mesh?.isMesh || mesh.isSkinnedMesh || mesh.isBatchedMesh || mesh.morphTargetInfluences) return null;
    // An instanced mesh three culls by its instances' bounds: not here.
    const instanced = Boolean(mesh.isInstancedMesh);
    if (instanced && mesh.frustumCulled) return null;
    if (Array.isArray(material) || !(material.isShaderMaterial || ownsEffectSource(material) || material.userData?.bfeFamily === "world") || material.wireframe || material.stencilWrite || material.polygonOffset || material.clippingPlanes?.length) return null;
    // three draws a two-sided blended material in two passes.
    if (material.transparent && material.side === THREE.DoubleSide && !material.forceSinglePass) return null;
    if (sceneTransform(mesh.matrixWorld.determinant()) < 0) return null;
    const geometry = mesh.geometry;
    if (geometry.groups.length || Object.keys(geometry.morphAttributes).length) return null;
    // Unindexed (a full-screen triangle, the storm's bolt): drawn through an
    // index of the pass's own, 0, 1, 2..., over the draw range three draws
    // each frame.
    const unindexed = !geometry.index;
    if (unindexed && (instanced || !geometry.attributes.position || geometry.attributes.position.isInterleavedBufferAttribute)) return null;
    const blend = blendOf(material, gl);
    if (blend === undefined || DEPTH[material.depthFunc] === undefined) return null;
    // Its uniforms: the frame block's when every frame value it reads is
    // the shared one, else each its own.
    const active = [];
    let shared, program;
    if (ownsEffectSource(material)) {
      let root = object;
      while (root.parent) root = root.parent;
      program = ownedEffectProgram(core, material, [WORLD, PLAIN], material.side, root === core.graphics.scene && core.shadowsOn());
      shared = program.kind === "World";
      const types = { 1: gl.FLOAT, 2: gl.FLOAT_VEC2, 3: gl.FLOAT_VEC3, 4: gl.FLOAT_VEC4, 5: gl.INT, 9: gl.FLOAT_MAT3, 16: gl.FLOAT_MAT4, 32: gl.SAMPLER_2D, 33: gl.SAMPLER_3D, 34: gl.SAMPLER_2D_SHADOW, 35: gl.SAMPLER_CUBE };
      for (const u of program.uniforms) active.push({ name: u.name, type: types[u.type], size: u.size });
    } else {
      if (!threeProgram.owned) return null;
      for (const u of threeProgram.inputs) active.push({...u, name:u.name.replace(/\[0\]$/, "")});
      shared = active.every(({ name }) => !FRAME_NAMES.has(name) || name in CAMERA || material.uniforms?.[name] === lighting[name]);
      program = threeProgram;

    }
    if (!program) return null;
    const own = shared ? active.filter(({ name }) => !FRAME_NAMES.has(name)) : active;
    // A zero material's values are its own and never change (materials.js):
    // fixed at registration. Any other's are written each frame.
    const fixed = shared && material.name?.startsWith("zero:");
    let table = this.tables.get(program.id);
    if (table === undefined) {
      if (own.some((u) => FLOATS[u.type] === undefined)) table = null;
      else {
        const id = rust.far_table(program.id, own.map((u) => u.name).join(","), new Uint32Array(own.map((u) => u.type)), new Uint32Array(own.map((u) => u.size)));
        table = id === undefined ? null : { id };
      }
      this.tables.set(program.id, table);
    }
    if (!table) return null;
    // Where each comes from: the camera and the object's matrices; else the
    // material's value, fixed (a zero material's) or written each frame
    // into the input (a plain one's).
    const placed = own.some((u) => u.name in OBJECT) || !mesh.matrixWorld.equals(IDENTITY) || mesh.matrixAutoUpdate;
    const slot = this.nextSlot;
    let at = slot + 1 + (placed ? 16 : 0);
    const sources = [], dynamic = [], watched = [];
    for (let index = 0; index < own.length; index++) {
      const { name, type, size } = own[index];
      if (type === SAMPLER_CUBE) {
        // A cube map (the Enhanced sky probe: sky-probe.js), named each
        // frame; none (the Classic flyers') is never read.
        if (!(name in (material.uniforms || {}))) return null;
        dynamic.push({ name, at, sampler: true, cube: true, id: -1 });
        sources.push([index, SRC.CUBE_INPUT, null, at]);
        at += 1;
        continue;
      }
      if (name in CAMERA) { sources.push([index, CAMERA[name], null, 0]); continue; }
      if (name in OBJECT) { sources.push([index, OBJECT[name], null, 0]); continue; }
      if (name === "isOrthographic") { sources.push([index, SRC.FLOATS, [camera.isOrthographicCamera ? 1 : 0], 0]); continue; }
      if (!(name in (material.uniforms || {}))) return null;
      const value = material.uniforms[name].value;
      if (type === SAMPLER_2D) {
        if (fixed) {
          // None: three binds its empty texture (a texel of zeros) there.
          if (value === null || value === undefined) {
            const id = core.texture(this.emptyTexture());
            if (id < 0) return undefined;
            sources.push([index, SRC.TEXTURE, null, id]);
            continue;
          }
          if (!value?.isTexture && typeof value !== "number") return null;
          const id = core.texture(value);
          if (id < 0) return undefined;
          sources.push([index, SRC.TEXTURE, null, id]);
          // A data texture's contents can change (three uploads them before
          // its next draw).
          if (value.isDataTexture) watched.push(value);
        } else {
          dynamic.push({ name, at, sampler: true, id: -1 });
          sources.push([index, SRC.TEXTURE_INPUT, null, at]);
          at += 1;
        }
        continue;
      }
      const floats = [];
      const want = FLOATS[type] * Math.max(1, size);
      if (!floatsOf(value, floats) || floats.length !== want) return null;
      if (fixed) sources.push([index, SRC.FLOATS, floats, 0]);
      else {
        dynamic.push({ name, at, floats: want, sampler: false });
        sources.push([index, SRC.INPUT, null, at]);
        at += want;
      }
    }
    // An instanced mesh's per-instance attributes (its matrices, its colours,
    // its geometry's instanced ones), each written each frame after the
    // instance count; the geometry's others are the core's.
    const perInstance = (name) => instanced && (name === "instanceMatrix" || name === "instanceColor" || Boolean(geometry.attributes[name]?.isInstancedBufferAttribute));
    const instances = [];
    let countAt = -1;
    if (instanced) {
      const max = mesh.instanceMatrix.count;
      countAt = at++;
      for (const [name, location] of program.attributes) {
        if (!perInstance(name)) continue;
        const attribute = name === "instanceMatrix" ? mesh.instanceMatrix : name === "instanceColor" ? mesh.instanceColor : geometry.attributes[name];
        if (!attribute?.array || attribute.isInterleavedBufferAttribute || (attribute.meshPerAttribute ?? 1) !== 1 || !(attribute.array instanceof Float32Array)) return null;
        const size = attribute.itemSize;
        if (size > 4 && size !== 16) return null;
        instances.push({ attribute, location, columns: size > 4 ? 4 : 1, rows: size > 4 ? 4 : size, max, at, size });
        at += max * size;
      }
    }
    // An unindexed geometry's range (first index, count), written each frame;
    // the buffers of its attributes three uploads anew (DynamicDrawUsage: the
    // storm's bolt) refreshed when their versions move: their values in the
    // pass's vertices, and the slot saying they were written anew.
    const rangeAt = unindexed ? at : -1;
    if (unindexed) at += 2;
    const made = unindexed ? this.unindexedGeometry(geometry, program) : null;
    const g = made ? made.g
      : core.geometry(geometry, instanced ? { id: program.id, attributes: program.attributes.filter(([name]) => !perInstance(name)) } : program);
    if (g === null || g === undefined) return null;
    const refresh = [];
    let refreshAt = -1;
    for (const { attribute, slot } of made?.buffers ?? []) {
      if (attribute.usage !== THREE.DynamicDrawUsage) continue;
      if (refreshAt < 0) refreshAt = at++;
      const from = this.nextVertex ?? 0, floats = attribute.array.length;
      if (from + floats > core.rust.far_vertices_len()) return null;
      this.nextVertex = from + floats;
      refresh.push({ attribute, slot, at: from, floats, version: -1 });
    }
    if (at > core.rust.far_input_len()) return null;
    if (!geometry.boundingSphere) geometry.computeBoundingSphere();
    const s = geometry.boundingSphere;
    const flags = material.side | (material.depthTest ? F.DEPTH_TEST : 0) | (material.depthWrite ? F.DEPTH_WRITE : 0) | (material.colorWrite ? F.COLOR_WRITE : 0)
      | (material.alphaToCoverage ? F.A2C : 0) | (material.transparent ? F.TRANSPARENT : 0) | (placed ? F.PLACED : 0)
      | (mesh.frustumCulled ? F.CULLED : 0) | (blend ? F.BLEND : 0);
    const id = rust.far_item(program.id, table.id, g, flags, DEPTH[material.depthFunc], new Uint32Array(blend || [0, 0, 0, 0, 0, 0]),
      new Float64Array([groupOrder, object.renderOrder, material.id, mesh.id]), new Float64Array([s.center.x, s.center.y, s.center.z, s.radius]), slot);
    for (const [index, kind, floats, from] of sources) rust.far_source(id, index, kind, new Float32Array(floats || []), from);
    for (const i of instances) rust.far_instanced(id, i.location, i.columns, i.rows, i.max, countAt, i.at);
    for (const r of refresh) rust.far_refresh(id, r.slot, r.at, r.floats, refreshAt);
    if (unindexed) {
      rust.far_range_input(id, rangeAt);
    } else if (mesh.userData.cellLights) {
      // The positions alone (the map's vertices are interleaved).
      const position = geometry.attributes.position, positions = new Float32Array(position.count * 3);
      for (let i = 0; i < position.count; i++) { positions[3 * i] = position.getX(i); positions[3 * i + 1] = position.getY(i); positions[3 * i + 2] = position.getZ(i); }
      const index = geometry.index.array;
      rust.far_cells(id, positions, index instanceof Uint32Array ? index : Uint32Array.from(index), 48);
    } else {
      // Preserve each descriptor's authored index range.
      const range = geometry.drawRange, count = geometry.index.count;
      const start = Math.max(0, range.start), end = Math.min(count, range.start + range.count);
      if (start !== 0 || end !== count) rust.far_range(id, start, Math.max(0, end - start));
    }
    this.nextSlot = at;
    // Its hooks that set uniforms (the far water's range), for one whose
    // values are written each frame.
    const hooks = !fixed && [mesh.onBeforeRender !== THREE.Object3D.prototype.onBeforeRender ? "object" : null,
      material.onBeforeRender !== THREE.Material.prototype.onBeforeRender ? "material" : null].filter(Boolean);
    return { id, object, mesh, material, threeProgram, slot, placed, terrain: false,
      dynamic, watched, hooks: hooks || [], instances, countAt, rangeAt, refresh, refreshAt };
  }

  // An unindexed geometry's copy for the renderer: each attribute the
  // program reads (one buffer each, its slot there), and an index 0, 1,
  // 2... over every vertex. { g, buffers: [{ attribute, slot }] }, or null.
  // Non-indexed sky geometry still needs a retained VAO and explicit vertices.
  // The renderer must preserve its triangles rather than hand it back to Three.
  // Registration is cold; the resulting geometry id is reused by every view.
  unindexedGeometry(geometry, program) {
    const rust = this.core.rust, position = geometry.attributes.position, count = position.count;
    const buffers = [];
    for (const [name, location] of program.attributes) {
      const a = geometry.attributes[name];
      if (!a || a.isInterleavedBufferAttribute || !(a.array instanceof Float32Array) || a.count !== count) return null;
      buffers.push({ attribute: a, location });
    }
    const g = rust.add_geometry();
    for (const b of buffers) {
      const array = b.attribute.array;
      b.slot = rust.geometry_buffer(g, new Uint8Array(array.buffer, array.byteOffset, array.byteLength));
      rust.geometry_pointer(g, b.slot, b.location, b.attribute.itemSize, 0x1406, b.attribute.normalized, 0, 0);
    }
    const index = new Uint32Array(count);
    for (let i = 0; i < count; i++) index[i] = i;
    rust.geometry_index(g, new Uint8Array(index.buffer), 0x1405);
    return { g, buffers };
  }


  // What a sampler three is given no texture for reads: three's empty
  // texture, one texel of zeros.
  emptyTexture() {
    if (!this.empty) {
      this.empty = new THREE.DataTexture(new Uint8Array(4), 1, 1);
      sceneProperty(this.empty,"needsUpdate",true);
    }
    return this.empty;
  }

  // What this frame's items read that can register anything (and so grow
  // the module's memory): the hooks that set their values (the far water's
  // range), the textures they read now, the terrain's level.
  // Prepare programs/textures before writing views into WASM memory: creating
  // a retained resource may grow the heap and detach an earlier typed view.
  // No draw is needed to initialise these compiler and texture dependencies.
  prepare(renderer, scene, camera) {
    const core = this.core;
    for (let i = 0; i < this.count; i++) {
      const item = this.found[i].item;
      if (!item || item.native) continue;
      for (const hook of item.hooks) {
        if (hook === "object") item.mesh.onBeforeRender(renderer, scene, camera, item.mesh.geometry, item.material, null);
        else item.material.onBeforeRender(renderer, scene, camera, item.mesh.geometry, item.mesh, null);
      }
      for (const texture of item.watched) this.upload(texture);
      const uniforms = item.material.uniforms;
      for (const d of item.dynamic) {
        if (!d.sampler) continue;
        const value = uniforms[d.name].value;
        if (value?.isTexture) this.upload(value);
        // None: three's empty texture, as for a fixed one (a cube sampler
        // given none is never read: none).
        d.id = d.cube ? (value?.isTexture || typeof value === "number" ? core.texture(value) : -1) : core.texture(value?.isTexture || typeof value === "number" ? value : this.emptyTexture());
      }
    }
  }


  // Cold descriptor admission is shared by mirror/probe/main queries. No
  // camera walk, hidden-layer rewrite or browser compiler discovers resources.
  admitAnchor(object){
    if(object.userData.nativeFar)return;
    if(object.isMesh&&(ownsEffectSource(object.material)||object.material?.userData?.bfeFamily==="world")){
      const known=this.core.registry.admitIfNew(object);if(!(known.scope&Family.FAR)||known.records[Family.FAR])return;
      let group=known.parent;while(group&&!group.object.isGroup)group=group.parent;
      const g=this.core.graphics,program=object.material.userData.bfeFamily==="world"?ownedWorldProgram(this.core,object.material,g.farScene,g.farCamera):null;
      const item=this.registerItem(object,object.material,program,group?.object.renderOrder||0,g.farCamera);
      if(item){this.all.push(item);this.core.registry.bind(object,Family.FAR,item);}return;
    }
    const source=object.userData.effectModel;
    if(source?.far){if(source.family==='water'||source.family==='ocean')this.admitWater(object,source);else this.admitEffect(object,source);}
    else if(source?.family==='ocean')this.admitWater(object,source);
    else if(object.userData.flyerSource)this.admitFlyer(object,object.userData.flyerSource);
  }
  adoptNear(){const sea=this.core.graphics.scene.userData.water?.oceanSurface?.near;if(sea)this.admitAnchor(sea);}
  drawNear(ctx){
    const {renderer,scene,camera}=ctx,core=this.core;this.near=true;this.count=0;this.camera=camera;this.scene=scene;this.mipmaps=0;
    const count=core.registry.select(scene,camera,Family.SEA,Mode.SEA);
    for(let i=0;i<count;i++){core.views();const object=core.registry.nodes[core.sceneOutput[i*S.STRIDE+S.SLOT]].object;
      const found=this.found[this.count]??={};found.object=object;found.item=object.userData.nativeFar||core.registry.admitIfNew(object).records[Family.FAR];this.count++;
    }
    this.write(renderer,this.count>0);
  }

  // This frame's input: whether the renderer draws, the clear colour, then
  // each item's slots.
  // Write current values into the registered sources, preserving sampler ids.
  // The native texture owner can replace a handle after upload/filter changes;
  // Rust must observe that replacement without allocating a different item.
  // Frame writes never choose a different rendering implementation.
  write(renderer, draw) {
    const core = this.core;
    core.views();
    const input = core.farInput;
    input[DRAW] = draw ? 1 : 0;
    if (!draw) return;
    // WebGLBackground's clear (none with autoClear off: a water mirror's
    // far scene drawn over its sky): the colour (a render target's: the
    // working space), premultiplied by its alpha as the context is.
    input[KEEP] = this.near || !renderer.autoClear ? 1 : 0;
    input[MIPMAPS] = this.mipmaps;
    input[NEAR] = this.near ? 1 : 0;
    renderer.getClearColor(this.color);
    const alpha = renderer.getClearAlpha(), pre = renderer.getContextAttributes()?.premultipliedAlpha !== false ? alpha : 1;
    input[CLEAR] = this.color.r * pre; input[CLEAR + 1] = this.color.g * pre; input[CLEAR + 2] = this.color.b * pre; input[CLEAR + 3] = alpha;
    for (const item of this.all) input[item.slot] = 0;
    const floats=this.scratch;
    for (let i = 0; i < this.count; i++) {
      const item = this.found[i].item;
      if (!item) continue;
      const slot = item.slot;
      input[slot] = item.id===undefined?0:1;
      if (item.native) {
        if(item.flyer){const source=item.flyer,row=ACTOR.FLYER,v=source.values;v[row.COUNT]=Math.min(item.object.count,item.capacity);v[row.PROBE]=source.owner.probe?.texture??-1;lighting.sunDirection.value.toArray(v,row.SUN_DIRECTION);lighting.sunColor.value.toArray(v,row.SUN_COLOR);lighting.ambient.value.toArray(v,row.AMBIENT);input.set(item.object.matrixWorld.elements,slot+1);input.set(v,slot+17);let at=slot+17+row.LEN;for(let j=0;j<v[row.COUNT]*16;j++)input[at+j]=source.arrays.matrices[j];at+=item.capacity*16;for(let j=0;j<v[row.COUNT]*4;j++)input[at+j]=source.arrays.looks[j];}
        if(item.effect){
          const source=item.effect;input.set(item.object.matrixWorld.elements,slot+1);input.set(source.values,slot+17);
          const row=EFFECT.STORM_LIGHTNING;input[slot+17+row.UPLOAD]=0;input[slot+17+row.FIRST]=0;input[slot+17+row.COUNT]=Math.min(source.count,item.capacity);
          if(item.serial!==source.serial){let at=item.vertex;for(let k=0;k<item.attributes.length;k++){const name=item.attributes[k][0],size=item.attributes[k][1],array=source.arrays[name];for(let j=0;j<item.capacity*size;j++)core.farVertices[at+j]=array[j];at+=item.capacity*size;}input[slot+17+row.UPLOAD]=1;item.serial=source.serial;}
        }
        if(item.water){item.water.prepare(this.camera);input.set(item.object.matrixWorld.elements,slot+1);input.set(item.water.values,slot+17);}
        if (item.terrain) input[slot + 1] = core.graphics.settings.get("farTerrain") === "reduced" ? 1 : 0;
        continue;
      }
      if (item.placed) input.set(item.mesh.matrixWorld.elements, slot + 1);
      if (item.instances.length) {
        const count = Math.min(item.mesh.count, item.instances[0].max);
        input[item.countAt] = count;
        for (const i of item.instances) input.set(i.attribute.array.subarray(0, count * i.size), i.at);
      }
      if (item.rangeAt >= 0) {
        // renderBufferDirect's range of an unindexed geometry.
        const range = item.mesh.geometry.drawRange, vertices = item.mesh.geometry.attributes.position.count;
        const start = Math.max(0, range.start), end = Math.min(vertices, range.start + range.count);
        input[item.rangeAt] = start; input[item.rangeAt + 1] = Math.max(0, end - start);
      }
      if (item.refreshAt >= 0) {
        input[item.refreshAt] = 0;
        if (item.refresh.some((r) => r.attribute.version !== r.version)) {
          for (const r of item.refresh) { core.farVertices.set(r.attribute.array, r.at); r.version = r.attribute.version; }
          input[item.refreshAt] = 1;
        }
      }
      const uniforms = item.material.uniforms;
      for (const d of item.dynamic) {
        if (d.sampler) { input[d.at] = d.id; continue; }
        floats.length = 0;
        floatsOf(uniforms[d.name].value, floats);
        for (let k = 0; k < d.floats; k++) input[d.at + k] = floats[k] ?? 0;
      }
    }
  }


  // A texture's new contents, uploaded as three would before a draw.
  upload(texture) {
    const renderer = this.core.renderer, properties = renderer.properties.get(texture);
    if (texture.version > 0 && properties.__version !== texture.version) renderer.initTexture(texture);
  }
}

export function farPass(core) {
  return new FarPass(core);
}

// The far pass's items from the near scene (FarPass.drawNear), a pass of the
// near phase (and so a mirror's) of their own.
export function seaPass(core) {
  const far = () => core.byName.far;
  return {
    name: "sea",
    phase: "near",
    beforeSort: (renderer, scene, camera) => { if (scene === core.graphics.scene) far().adoptNear(scene, camera); },
    draw: (ctx) => far().drawNear(ctx),
  };
}
