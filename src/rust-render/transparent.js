import { Family } from "./scene.js";
import { ownedWorldProgram } from "./world-programs.js";
// The near walk publishes blended map buckets and object rows with their
// authored sort keys. Rust merges them with effects, water, instances and
// lines in one stable list. No JS item or callback splits that execution.
// Reflection preparation uses the same producer with its own camera packet.
// Material/source registration remains cold; culling and transform production
// still follow the scene shells until the native scene registry consumes them.
import * as THREE from "three";
import { ownedEffectProgram } from "./effect-material.js";
import { ownsEffectSource } from "./programs.js";

// add_program's kind for the map's meshes (web/mod.rs).
const WORLD = 0;
// The material values a blended bucket's program may read, set per draw
// (receiveShadow is the bucket's own). shadows: a blended bucket receives the
// cascades; taken out of three's list rather than hidden, it is still three's
// shadow pass's to draw if it casts (nothing to reveal).
const MATERIAL = new Set(["sunScale", "brightness", "ambientPull", "specular", "scroll", "receiveShadow"]);
const IDENTITY = new THREE.Matrix4();
// GL enums for three's blending and depth functions (WebGLState).
const GL = {
  ADD: 0x8006, SUBTRACT: 0x800a, REVERSE_SUBTRACT: 0x800b, MIN: 0x8007, MAX: 0x8008,
  ZERO: 0, ONE: 1, SRC_COLOR: 0x0300, ONE_MINUS_SRC_COLOR: 0x0301, SRC_ALPHA: 0x0302, ONE_MINUS_SRC_ALPHA: 0x0303,
  DST_ALPHA: 0x0304, ONE_MINUS_DST_ALPHA: 0x0305, DST_COLOR: 0x0306, ONE_MINUS_DST_COLOR: 0x0307, SRC_ALPHA_SATURATE: 0x0308,
};
const EQUATIONS = { [THREE.AddEquation]: GL.ADD, [THREE.SubtractEquation]: GL.SUBTRACT, [THREE.ReverseSubtractEquation]: GL.REVERSE_SUBTRACT, [THREE.MinEquation]: GL.MIN, [THREE.MaxEquation]: GL.MAX };
const FACTORS = {
  [THREE.ZeroFactor]: GL.ZERO, [THREE.OneFactor]: GL.ONE, [THREE.SrcColorFactor]: GL.SRC_COLOR, [THREE.OneMinusSrcColorFactor]: GL.ONE_MINUS_SRC_COLOR,
  [THREE.SrcAlphaFactor]: GL.SRC_ALPHA, [THREE.OneMinusSrcAlphaFactor]: GL.ONE_MINUS_SRC_ALPHA, [THREE.DstAlphaFactor]: GL.DST_ALPHA,
  [THREE.OneMinusDstAlphaFactor]: GL.ONE_MINUS_DST_ALPHA, [THREE.DstColorFactor]: GL.DST_COLOR, [THREE.OneMinusDstColorFactor]: GL.ONE_MINUS_DST_COLOR,
  [THREE.SrcAlphaSaturateFactor]: GL.SRC_ALPHA_SATURATE,
};
// transparent.rs OBJECT_* stores placement, colour, sampler and order keys.
const OBJ = { INDEX: 0, KEY: 1, WORLD: 5, DIFFUSE: 21, ROTATION: 25, CENTER: 26, MAP: 28, BUCKET: 37 };
const baseBeforeRender = THREE.Object3D.prototype.onBeforeRender, baseAfterRender = THREE.Object3D.prototype.onAfterRender;
const baseMaterialBeforeRender = THREE.Material.prototype.onBeforeRender;
const DEPTH_FUNCS = { [THREE.NeverDepth]: 0x0200, [THREE.LessDepth]: 0x0201, [THREE.EqualDepth]: 0x0202, [THREE.LessEqualDepth]: 0x0203, [THREE.GreaterDepth]: 0x0204, [THREE.NotEqualDepth]: 0x0205, [THREE.GreaterEqualDepth]: 0x0206, [THREE.AlwaysDepth]: 0x0207 };

// A material's blending as three's setBlending sets it: [enabled, equation
// rgb, equation alpha, src rgb, dst rgb, src alpha, dst alpha], or null for
// an invalid or unauthored blend state, which fails battle registration.
export function blendOf(material) {
  const p = material.premultipliedAlpha;
  switch (material.blending) {
    case THREE.NoBlending: return [0, GL.ADD, GL.ADD, GL.ONE, GL.ZERO, GL.ONE, GL.ZERO];
    case THREE.NormalBlending:
      if (!material.transparent) return [0, GL.ADD, GL.ADD, GL.ONE, GL.ZERO, GL.ONE, GL.ZERO];
      return p ? [1, GL.ADD, GL.ADD, GL.ONE, GL.ONE_MINUS_SRC_ALPHA, GL.ONE, GL.ONE_MINUS_SRC_ALPHA] : [1, GL.ADD, GL.ADD, GL.SRC_ALPHA, GL.ONE_MINUS_SRC_ALPHA, GL.ONE, GL.ONE_MINUS_SRC_ALPHA];
    case THREE.AdditiveBlending: return p ? [1, GL.ADD, GL.ADD, GL.ONE, GL.ONE, GL.ONE, GL.ONE] : [1, GL.ADD, GL.ADD, GL.SRC_ALPHA, GL.ONE, GL.SRC_ALPHA, GL.ONE];
    case THREE.SubtractiveBlending: return [1, GL.ADD, GL.ADD, GL.ZERO, GL.ONE_MINUS_SRC_COLOR, GL.ZERO, GL.ONE];
    case THREE.MultiplyBlending: return p ? [1, GL.ADD, GL.ADD, GL.ZERO, GL.SRC_COLOR, GL.ZERO, GL.SRC_ALPHA] : [1, GL.ADD, GL.ADD, GL.ZERO, GL.SRC_COLOR, GL.ZERO, GL.SRC_COLOR];
    case THREE.CustomBlending: {
      const src = material.blendSrc, dst = material.blendDst, srcA = material.blendSrcAlpha || src, dstA = material.blendDstAlpha || dst;
      const equation = material.blendEquation, equationA = material.blendEquationAlpha || equation;
      if ([src, dst, srcA, dstA].some((f) => FACTORS[f] === undefined) || EQUATIONS[equation] === undefined || EQUATIONS[equationA] === undefined) return null;
      return [1, EQUATIONS[equation], EQUATIONS[equationA], FACTORS[src], FACTORS[dst], FACTORS[srcA], FACTORS[dstA]];
    }
    default: return null;
  }
}

// What a material's draw state is to the renderer: its depth function and
// flags (1 depth test, 2 depth write, 4 colour write), or null for a state
// outside the declared depth functions. Alpha to coverage is flag 8.
export function stateOf(material) {
  const func = DEPTH_FUNCS[material.depthFunc];
  if (func === undefined) return null;
  return { func, flags: (material.depthTest ? 1 : 0) | (material.depthWrite ? 2 : 0) | (material.colorWrite ? 4 : 0) | (material.alphaToCoverage ? 8 : 0) };
}

// The sides three draws a transparent material with, in order (renderObject:
// a double-sided one back then front, unless forceSinglePass), each as the
// renderer's side (0 front, 1 back, 2 both).
export function sidesOf(material) {
  if (material.transparent && material.side === THREE.DoubleSide && !material.forceSinglePass) return [1, 0];
  return [material.side === THREE.BackSide ? 1 : material.side === THREE.DoubleSide ? 2 : 0];
}

class TransparentPass {
  constructor(core) {
    this.core = core;
    this.name = "transparent";
    this.phase = "transparent";
    this.frameNo = 0;
    // Three's render list of the near scene this frame, the items it still
    // draws itself, and the hooks armed on them (object -> hook).
    this.list = null;
    this.taken = new Map();
    this.candidates = [];
    // Objects found before three listed the scene (beforeSort) that could not
    // be registered yet (a texture still loading).
    this.retry = new Set();
    this.admissions = [];
    // The objects' programs, made for this pass alone: the renderer keeps
    // the values it last set in each (transparent.rs), and the effects'
    // glow models can draw with the same three program, keeping theirs.
    // The list near.js's walk stands for (walkedList).
    this.walked = { opaque: [], transmissive: [], transparent: [], image: { object: null } };
    this.buckets = 0;
    this.objectCount = 0;
    this.takenCount = 0;
    core.addView("transparentKeys", Float64Array, () => core.rust.transparent_keys_ptr(), () => core.rust.transparent_keys_len());
    core.addView("transparentObjects", Float64Array, () => core.rust.transparent_objects_ptr(), () => core.rust.transparent_objects_len());
    this.objectStride = core.rust.transparent_object_stride();
  }

  // NearPass's walk supplies candidates. Sorting and interleaved line/copy
  // execution belong to the native list, with no later JS continuation.
  prepare(ctx) {
    this.frameNo++;this.takenCount=0;
    this.list=this.walkedList(ctx.walked || []);
    this.take(this.list.transparent,ctx);
  }

  // The list three would have made of the near scene, as near.js walked it:
  // the far scene's image alone opaque, and the walk's items.
  walkedList(items) {
    this.walked.opaque[0] = this.walked.image;
    this.walked.image.object = this.core.graphics.background;
    this.walked.transparent = items;
    return this.walked;
  }

  // Whether this pass draws an item three would list in the near scene's
  // next render (take()'s test, before that frame's near phase).
  // Ownership must be known before the near walk accepts the frame. A material
  // revision can invalidate its old program; compile/register the new one before
  // using its previous record. Failure must remain visible to the required drawer.
  takes(item) {
    const ownedMaterial=item.material;
    if (ownsEffectSource(ownedMaterial)) {
      let entry=this.taken.get(item.object);
      if(!entry || entry.material!==ownedMaterial || entry.version!==ownedMaterial.version){
        entry=this.registerObject(item,true);if(!entry)return false;this.taken.set(item.object,entry);
      }
      return !item.group;
    }
    if(ownedMaterial.userData.bfeFamily === "world") {
      let entry=this.taken.get(item.object);
      if(!entry || entry.material!==ownedMaterial || entry.version!==ownedMaterial.version){
        entry=this.registerBucket(item);if(!entry)return false;this.taken.set(item.object,entry);
      }
      return !item.group;
    }
    return false;
  }

  // three's render list for the near scene being drawn (core.nearList).



  // beforeSort admits source-owned effects added since the previous view.
  // Keep pending textures until their resource owner publishes storage.
  beforeSort(renderer, scene, camera, target, added) {
    const graphics = this.core.graphics;
    if (!added || scene !== graphics.scene) return;
    if (!added.length && !this.retry.size) return;
    try {
      const found = this.admissions;
      found.length = 0;
      for (const object of this.retry) found.push(object);
      this.retry.clear();
      // RetainedScene supplies each changed node once, including descendants.
      // Traversing each admitted ancestor would revisit the same cold subtree.
      for (const o of added) {
        if(!(o.isMesh||o.isSprite))continue;
        const material = o.material;
        if (!(o.isMesh || o.isSprite) || !material || Array.isArray(material) || material.userData?.bfeShape === undefined || !material.transparent || this.taken.has(o)) continue;
        // Not a glow model's part (the effects pass's: its group keeps its
        // material), nor one whose vertices change (the Classic lightning's):
        // this pass keeps an object's geometry as it first finds it.
        if (o.parent?.userData?.material === material || changing(o.geometry)) continue;
        found.push(o);
      }
      if (!found.length) return;
      // Only what registered: anything else is looked at as three lists it
      // (a texture still loading, say).
      for (const object of found) {
        if (this.taken.has(object)) continue;
        const entry = this.registerObject({ object, material: object.material, geometry: object.geometry, group: null }, true);
        if (entry) this.taken.set(object, entry);
        // Retry after the resource owner has admitted its pending sample.
        else if (entry === undefined && object.parent) this.retry.add(object);
      }
    } catch (error) {
      this.core.fail(error);
    }
  }

  // take writes retained catalog draws in the near list's selected order.
  // Material replacement invalidates a record without a browser program cache.
  take(items, ctx) {
    const core = this.core, renderer = core.renderer, stride = this.objectStride;
    core.views();
    let records = core.transparentObjects;
    let kept = 0, n = 0;
    this.candidates.length = 0;
    for (const item of items) {
      let entry = this.taken.get(item.object);
      // Another material since (the game swapped it): looked at again.
      if (entry && entry.material !== item.material) { this.taken.delete(item.object); entry = undefined; }
      else if (entry && item.group) entry = null;
      else if (entry && entry.material.userData.bfeFamily === "world") {
        if (entry.version !== entry.material.version) { this.taken.delete(item.object); entry = undefined; }
      }
      if (entry === undefined && item.material.userData?.bfeProgram) {
        entry=this.registerObject(item,true);if(!entry)throw Error("Owned transparent effect failed to register");this.taken.set(item.object,entry);
      } else if (entry === undefined && this.candidate(item)) {
        if (item.material.userData.bfeFamily === "world") { entry = this.registerBucket(item); if (entry !== undefined) this.taken.set(item.object, entry); }
        else this.candidates.push(item);
      }
      if (entry && n < 512) {
        core.views();records=core.transparentObjects;
        const o = 1 + n * stride;
        if (entry.bucket) {
          records[o] = entry.id; records[o + OBJ.BUCKET] = 1;
          records[o + OBJ.KEY] = item.groupOrder; records[o + OBJ.KEY + 1] = item.renderOrder; records[o + OBJ.KEY + 2] = item.z; records[o + OBJ.KEY + 3] = item.id;
        } else this.writeObject(records, o, entry, item, ctx);
        n++;
        continue;
      }
      items[kept++] = item;
    }
    items.length = kept;
    core.views();core.transparentObjects[0] = n;
    this.takenCount = n;
  }


  // Whether three's item is one this pass could draw: one of the map's
  // blended buckets, or an object with a material it knows.
  candidate(item) {
    const { object, material } = item;
    if (item.group || Array.isArray(object.material)) return false;
    if (material.userData?.bfeShape !== undefined) return object.parent?.userData?.material !== material && object.geometry?.attributes.position?.usage !== THREE.DynamicDrawUsage;
    return Boolean(object.isMesh && object.parent === this.core.graphics.scene && !object.matrixAutoUpdate && object.matrix.equals(IDENTITY)
      && material.defines && "WORLD_SPACE" in material.defines && !("TERRAIN" in material.defines) && (material.isShaderMaterial || material.userData.bfeFamily === "world"));
  }

  writeObject(r, o, entry, item, ctx) {
    const object = item.object, material = entry.material;
    if (material.userData?.bfeShape !== undefined && object.onBeforeRender !== baseBeforeRender) {
      object.onBeforeRender(ctx.renderer, ctx.scene, ctx.camera, item.geometry, material, item.group);
    }
    // A first-use depth copy can allocate a target in the hook above. Import
    // the current view after that cold boundary; earlier rows survive growth.
    this.core.views();r=this.core.transparentObjects;
    r[o + OBJ.INDEX] = entry.id; r[o + OBJ.BUCKET] = 0;
    r[o + OBJ.KEY] = item.groupOrder; r[o + OBJ.KEY + 1] = item.renderOrder; r[o + OBJ.KEY + 2] = item.z; r[o + OBJ.KEY + 3] = item.id;
    r.set(object.matrixWorld.elements, o + OBJ.WORLD);
    const c = material.color;
    r[o + OBJ.DIFFUSE] = c.r; r[o + OBJ.DIFFUSE + 1] = c.g; r[o + OBJ.DIFFUSE + 2] = c.b; r[o + OBJ.DIFFUSE + 3] = material.opacity;
    r[o + OBJ.ROTATION] = material.rotation ?? 0;
    r[o + OBJ.CENTER] = object.center?.x ?? 0; r[o + OBJ.CENTER + 1] = object.center?.y ?? 0;
    // refreshTransformUniform uses the native f64 UV publication directly.
    const map = material.map;
    if (map) this.core.cameras.uv(map,r,o+OBJ.MAP);
  }

  // Copy commands use the same group/render/depth/id keys as blended draws.
  // Depth capture and fountain colour capture are resource operations, not
  // dummy meshes. Their explicit keys preserve ordering without native callbacks.
  // The Rust segment cursor resumes immediately after each command.
  draw(ctx) {
    const core = this.core;
    for (const item of this.candidates) {
      if (this.taken.has(item.object)) continue;
      const entry = item.material.userData?.bfeShape !== undefined ? this.registerObject(item) : this.registerBucket(item);
      if (entry !== undefined) this.taken.set(item.object, entry);
    }
    this.candidates.length = 0;
    core.views();

  }


  // passesOf preserves back/front order while selecting declared sources.
  // Material state can change at admission without a compiled browser cache.
  passesOf(material) {
    const core = this.core, sides = sidesOf(material);
    if (material.userData?.bfeFamily === "world") return { sides, programs:sides.map(side=>ownedWorldProgram(core,material,core.graphics.scene,core.graphics.camera,side)),current:null };
    if (ownsEffectSource(material)) return { sides, programs:sides.map(side=>ownedEffectProgram(core,material,WORLD,side)),current:null };
    return null;
  }

  registerBucket(item) {
    const core = this.core, rust = core.rust;
    const { object: mesh, material } = item, geometry = mesh.geometry;
    if (geometry.groups.length || Object.keys(geometry.morphAttributes).length || geometry.drawRange.start !== 0 || geometry.drawRange.count < (geometry.index?.count ?? 0)) return null;
    if (mesh.onBeforeRender !== baseBeforeRender || mesh.onAfterRender !== baseAfterRender) return null;
    const blend = blendOf(material), state = stateOf(material);
    if (!blend || !state) return null;
    const passes = this.passesOf(material);
    if (!passes) { core.errors.push(`blended ${mesh.name}: program`); return null; }
    const g = core.geometry(geometry, passes.programs[0]);
    if (g === null) return null;
    if (!geometry.boundingBox) geometry.computeBoundingBox();
    const bb = geometry.boundingBox;
    const pointLit = Boolean(material.userData.pointLit) && passes.programs.some((p) => p.names.has("zeroLightCount"));
    const id = rust.transparent_bucket(g, new Uint32Array(blend), state.func, state.flags, pointLit, new Float64Array([bb.min.x, bb.min.y, bb.min.z, bb.max.x, bb.max.y, bb.max.z]));
    const map = core.texture(material.uniforms.map?.value);
    if (map >= 0) rust.transparent_bucket_texture(id, 0, map);
    if (passes.programs.some((p) => p.names.has("bumpMap"))) { const bump = core.texture(material.uniforms.bumpMap?.value); if (bump >= 0) rust.transparent_bucket_texture(id, 2, bump); }
    passes.sides.forEach((side, i) => {
      const program = passes.programs[i], pass = rust.transparent_bucket_pass(id, program.id, side);
      for (const name of program.names) {
        if (!MATERIAL.has(name)) continue;
        if (name === "receiveShadow") { rust.transparent_bucket_uniform(id, pass, name, new Float32Array([mesh.receiveShadow ? 1 : 0]), true); continue; }
        const v = material.uniforms[name]?.value;
        const values = typeof v === "number" ? [v] : v?.isVector4 ? [v.x, v.y, v.z, v.w] : v?.isVector3 ? [v.x, v.y, v.z] : v?.isVector2 ? [v.x, v.y] : null;
        if (values) rust.transparent_bucket_uniform(id, pass, name, new Float32Array(values), false);
      }
    });
    this.buckets++;
    const entry={ bucket: true, id, material, sourceGeometry:geometry, program: passes.current, version: material.version, frame: this.frameNo - (material.userData.bfeFamily === "world" ? 1 : 0) };
    core.registry.bind(mesh,Family.NEAR,entry,id);return entry;
  }

  // An owned basic mesh or sprite (bfeShape names which): it has a catalog
  // program from the moment it is made, so nothing waits on a draw.
  registerObject(item, retryLoading = false) {
    const core = this.core, rust = core.rust;
    const { object, material, geometry } = item;
    const kind = material.userData?.bfeShape;
    if (kind === undefined || !material.userData?.bfeProgram) return null;
    if (object.isSkinnedMesh || object.isInstancedMesh || object.isBatchedMesh || object.morphTargetInfluences?.length || (!object.isMesh && !object.isSprite)) return null;
    if ((object.onBeforeRender !== baseBeforeRender && material.userData?.bfeShape === undefined) || object.onAfterRender !== baseAfterRender || material.onBeforeRender !== baseMaterialBeforeRender) return null;
    if (material.alphaMap || material.alphaTest > 0 || material.alphaHash || material.vertexColors || material.envMap || material.lightMap || material.aoMap || material.specularMap || material.wireframe) return null;
    const blend = blendOf(material), state = stateOf(material);
    if (!blend || !state || !geometry.index || Object.keys(geometry.morphAttributes).length) return null;
    const passes = this.passesOf(material);
    if (!passes) return null;
    const g = core.geometry(geometry, passes.programs[0]);
    if (g === null) return null;
    let map = -1;
    // (Built without a draw, its texture may still be loading: looked at again.)
    if (passes.programs[0].names.has("map")) { map = core.texture(material.map); if (map < 0) return retryLoading ? undefined : null; }
    const start = geometry.drawRange.start, count = Math.min(geometry.drawRange.count, geometry.index.count - start);
    const id = rust.transparent_object(kind, g, start, count, new Uint32Array(blend), state.func, state.flags);
    passes.sides.forEach((side, i) => rust.transparent_object_pass(id, passes.programs[i].id, side));
    if (map >= 0) rust.transparent_object_texture(id, map);
    this.objectCount++;
    const entry={ bucket: false, id, material, sourceGeometry:geometry, program: passes.current, frame: this.frameNo, version: material.version };
    core.registry.bind(object,Family.NEAR,entry,id);return entry;
  }


}

// Whether a geometry's buffers are made to change (three's usage hint).
function changing(geometry) {
  const usage = (a) => (a.isInterleavedBufferAttribute ? a.data.usage : a.usage);
  return Object.values(geometry.attributes).some((a) => usage(a) !== THREE.StaticDrawUsage) || Boolean(geometry.index && geometry.index.usage !== THREE.StaticDrawUsage);
}

export function transparentPass(core) {
  return new TransparentPass(core);
}
