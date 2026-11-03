// The transparent pass (transparent.rs): what the near scene blends, drawn
// in three's back-to-front order, as three's transparent list would. Any
// pass whose draws blend (the effects, the water...) is a source of items
// for it on the renderer's side (transparent.rs Source); this file is the
// page's side of the list and of this pass's own items.
//
// When it draws. The near phase runs at the start of the near scene's
// opaque list; three has sorted its lists by then. After it (afterPhase),
// the pass arms the transparent phase on the item three draws just before
// its transparent list (the last opaque one): that item's onAfterRender
// runs the phase, once, this frame.
//
// What three still draws. Whatever three draws itself stays in three's
// list, in three's order. The phase writes the keys three sorted those
// items by (group order, render order, z, id) into the renderer, which
// sorts every source's items by the same key and splits them into
// segments around three's own: segment 0 is drawn at once, before three's
// first item; segment k right after three draws its item k - 1 (that
// item's onAfterRender, armed after the phase). The frame's blending order
// is then the one three would have drawn everything in.
//
// This pass's own items: the map's blended buckets, and objects drawn with
// three's basic and sprite materials (the command posts' holograms, beams
// and flares). three still lists them: its culling, its keys and the game's
// own visibility stand. After the near phase, those registered are taken
// out of three's list in the near scene's own render (any other, a
// mirror's, still draws them with three), each a record of its key (and an
// object's matrix and material values), which the renderer draws in their
// place, each as three would: a double-sided one twice, back faces then
// front, each with three's own program for that side. Each is registered
// once three has drawn it as it is now, and drawn by three again when three
// would draw it with another program.
import * as THREE from "three";

// add_program's kind for the map's meshes (web/mod.rs).
const WORLD = 0;
// The material values a blended bucket's program may read, set per draw.
const MATERIAL = new Set(["sunScale", "brightness", "ambientPull", "specular", "scroll"]);
const POLICY = {
  uniforms: new Set(["map", "farMap", "bumpMap", ...MATERIAL, "zeroLights", "zeroLightCount", "localLightPositions", "localLightColors", "localLightCount"]),
  material: MATERIAL,
};
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
// three's materials whose objects this pass draws (transparent.rs OBJECT_*),
// and what their programs may read besides the frame block: the object's
// matrices, the material's values and map, a sprite's rotation and centre.
const OBJECT_KINDS = { MeshBasicMaterial: 0, SpriteMaterial: 1 };
const OBJECT_POLICY = {
  uniforms: new Set(["modelViewMatrix", "modelMatrix", "diffuse", "opacity", "map", "mapTransform", "rotation", "center", "isOrthographic"]),
  material: new Set(),
};
const OBJ = { INDEX: 0, KEY: 1, WORLD: 5, DIFFUSE: 21, ROTATION: 25, CENTER: 26, MAP: 28, BUCKET: 37 };
const baseBeforeRender = THREE.Object3D.prototype.onBeforeRender, baseAfterRender = THREE.Object3D.prototype.onAfterRender;
const baseMaterialBeforeRender = THREE.Material.prototype.onBeforeRender;
const DEPTH_FUNCS = { [THREE.NeverDepth]: 0x0200, [THREE.LessDepth]: 0x0201, [THREE.EqualDepth]: 0x0202, [THREE.LessEqualDepth]: 0x0203, [THREE.GreaterDepth]: 0x0204, [THREE.NotEqualDepth]: 0x0205, [THREE.GreaterEqualDepth]: 0x0206, [THREE.AlwaysDepth]: 0x0207 };

// A material's blending as three's setBlending sets it: [enabled, equation
// rgb, equation alpha, src rgb, dst rgb, src alpha, dst alpha], or null for
// one this pass leaves to three (constant-colour factors).
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
// this pass leaves to three (stencil, polygon offset, alpha to coverage).
export function stateOf(material) {
  if (material.stencilWrite || material.polygonOffset || material.alphaToCoverage || material.transmission > 0) return null;
  const func = DEPTH_FUNCS[material.depthFunc];
  if (func === undefined) return null;
  return { func, flags: (material.depthTest ? 1 : 0) | (material.depthWrite ? 2 : 0) | (material.colorWrite ? 4 : 0) };
}

// The sides three draws a transparent material with, in order (renderObject:
// a double-sided one back then front, unless forceSinglePass), each as the
// renderer's side (0 front, 1 back, 2 both).
export function sidesOf(material) {
  if (material.transparent && material.side === THREE.DoubleSide && !material.forceSinglePass) return [1, 0];
  return [material.side === THREE.BackSide ? 1 : material.side === THREE.DoubleSide ? 2 : 0];
}

// three's program for a material drawn with `side` (0 front, 1 back, 2
// both: a single pass, as three draws a double-sided ShaderMaterial, which
// forces one): the one it last drew with for that side, among the
// material's programs. They differ by their FLIP_SIDED and DOUBLE_SIDED
// defines alone.
export function programForSide(renderer, material, side) {
  const props = renderer.properties.get(material), current = props.currentProgram;
  if (!current) return null;
  const gl = renderer.getContext();
  const vertexOf = (program) => {
    const [vs] = gl.getAttachedShaders(program.program).filter((s) => gl.getShaderParameter(s, gl.SHADER_TYPE) === gl.VERTEX_SHADER);
    return vs ? gl.getShaderSource(vs) : null;
  };
  const fits = (source) => /^#define FLIP_SIDED$/m.test(source) === (side === 1) && /^#define DOUBLE_SIDED$/m.test(source) === (side === 2);
  const strip = (source) => source.replace(/^#define (FLIP|DOUBLE)_SIDED\n/gm, "");
  const currentSource = vertexOf(current);
  if (currentSource === null) return null;
  if (fits(currentSource)) return current;
  const base = strip(currentSource);
  for (const program of props.programs?.values() || []) {
    if (program === current) continue;
    const source = vertexOf(program);
    if (source !== null && fits(source) && strip(source) === base) return program;
  }
  return null;
}

class TransparentPass {
  constructor(core) {
    this.core = core;
    this.name = "transparent";
    this.phase = "transparent";
    this.active = true;
    this.frameNo = 0;
    // Three's render list of the near scene this frame, the items it still
    // draws itself, and the hooks armed on them (object -> hook).
    this.list = null;
    this.three = [];
    this.hooks = new Map();
    this.segmentStats = [];
    // What this pass draws in three's place: three's object -> { bucket,
    // id, material, program (three's, front pass), frame (registered) }, or
    // null for one left to three. Candidates: items seen in three's list
    // this frame, registered in the transparent phase.
    this.taken = new Map();
    this.candidates = [];
    // The objects' programs, made for this pass alone: the renderer keeps
    // the values it last set in each (transparent.rs), and the effects'
    // glow models can draw with the same three program, keeping theirs.
    this.objectPrograms = new Map();
    // The list near.js's walk stands for (walkedList).
    this.walked = { opaque: [], transmissive: [], transparent: [], image: { object: null } };
    this.buckets = 0;
    this.objectCount = 0;
    this.takenCount = 0;
    core.addView("transparentKeys", Float64Array, () => core.rust.transparent_keys_ptr(), () => core.rust.transparent_keys_len());
    core.addView("transparentObjects", Float64Array, () => core.rust.transparent_objects_ptr(), () => core.rust.transparent_objects_len());
    this.objectStride = core.rust.transparent_object_stride();
  }

  // After the near phase (the near scene's own render: never a mirror's):
  // take what this pass draws out of three's transparent list, then arm the
  // transparent phase on the item three draws just before that list. After
  // the transparent phase: arm each later segment on the item three draws
  // just before it. When the near scene is drawn in place of three's render
  // (near.js), its walk's items stand for three's list, all of them this
  // pass's, and the transparent phase follows at once, as deferred as the
  // near phase.
  afterPhase(name, ctx) {
    if (name === "near") {
      this.disarm();
      this.frameNo++;
      this.list = null;
      this.takenCount = 0;
      this.list = ctx.walked ? this.walkedList(ctx.walked) : this.threeList(ctx.renderer, ctx.scene);
      if (!this.list) return;
      // Off (a comparison): three draws every item of its list, and the
      // other sources' items still draw around them.
      if (this.active) this.take(this.list.transparent);
      else this.takeNone();
      const before = this.list.transmissive.length ? this.list.transmissive : this.list.opaque;
      const last = before[before.length - 1];
      const start = () => this.core.phase("transparent", ctx.renderer, ctx.scene, ctx.camera, ctx.defer);
      // The far scene's image drawn last: the transparent list follows now.
      if (last.object === this.core.graphics.background) start();
      else this.arm(last, start);
    } else if (name === "transparent") {
      const core = this.core, rust = core.rust;
      const n = rust.transparent_segments_len(), segments = new Uint32Array(core.wasm.memory.buffer, rust.transparent_segments_ptr(), n);
      this.segmentStats.length = 0;
      for (let k = 1; k + 1 < n && k <= this.three.length; k++) {
        if (segments[k + 1] > segments[k]) { const segment = k; this.arm(this.three[k - 1], () => this.drawSegment(segment)); }
      }
    }
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
  takes(item) {
    const entry = this.taken.get(item.object);
    if (!this.active || !entry || entry.material !== item.material || item.group) return false;
    const props = this.core.renderer.properties.get(entry.material);
    return props.currentProgram === entry.program && props.__version === entry.material.version && entry.frame <= this.frameNo;
  }

  // three's render list for the near scene being drawn: the one whose
  // opaque list holds the far scene's image (lists are kept per render depth).
  threeList(renderer, scene) {
    const background = this.core.graphics.background;
    for (let depth = 0; depth < 4; depth++) {
      const list = renderer.renderLists.get(scene, depth);
      if (!list.opaque.length) return null;
      if (list.opaque.some((item) => item.object === background)) return list;
    }
    return null;
  }

  // fn right after three draws `item` (its object's onAfterRender for that
  // material and group) in the near scene's own pass, once. An object's hook
  // is taken off once all of its items have run, or at the next frame's
  // near phase whatever happened.
  arm(item, fn) {
    const { graphics } = this.core;
    let hook = this.hooks.get(item.object);
    if (!hook) {
      const object = item.object;
      hook = { object, own: Object.prototype.hasOwnProperty.call(object, "onAfterRender"), original: object.onAfterRender, entries: [], left: 0 };
      const hooks = this.hooks;
      object.onAfterRender = function (renderer, scene, camera, geometry, material, group) {
        hook.original.call(this, renderer, scene, camera, geometry, material, group);
        if (camera !== graphics.camera || scene !== graphics.scene || renderer.getRenderTarget() !== graphics.sceneTarget) return;
        for (const entry of hook.entries) {
          if (entry.done || entry.material !== material || entry.group !== group) continue;
          entry.done = true;
          if (--hook.left === 0) { restore(hook); hooks.delete(object); }
          entry.fn();
          break;
        }
      };
      this.hooks.set(object, hook);
    }
    hook.entries.push({ material: item.material, group: item.group, fn, done: false });
    hook.left++;
  }

  disarm() {
    for (const hook of this.hooks.values()) restore(hook);
    this.hooks.clear();
  }

  // What this pass draws, out of three's transparent list (in place: three's
  // loop over it has not started), each a record for the renderer with
  // three's key. Taken only while three's program for it is the one it was
  // registered with, and three has drawn it as it is now (it still does in
  // other renders, a mirror's); else three draws it, and it is registered
  // again if its program changed.
  take(items) {
    const core = this.core, renderer = core.renderer, stride = this.objectStride;
    core.views();
    const records = core.transparentObjects;
    let kept = 0, n = 0;
    this.candidates.length = 0;
    for (const item of items) {
      let entry = this.taken.get(item.object);
      // Another material since (the game swapped it): looked at again.
      if (entry && entry.material !== item.material) { this.taken.delete(item.object); entry = undefined; }
      else if (entry && item.group) entry = null;
      else if (entry) {
        const props = renderer.properties.get(entry.material);
        if (props.currentProgram !== entry.program) { this.taken.delete(item.object); entry = undefined; }
        else if (props.__version !== entry.material.version || entry.frame >= this.frameNo) entry = null;
      }
      if (entry === undefined && this.candidate(item)) this.candidates.push(item);
      if (entry && n < 512) {
        const o = 1 + n * stride;
        if (entry.bucket) {
          records[o] = entry.id; records[o + OBJ.BUCKET] = 1;
          records[o + OBJ.KEY] = item.groupOrder; records[o + OBJ.KEY + 1] = item.renderOrder; records[o + OBJ.KEY + 2] = item.z; records[o + OBJ.KEY + 3] = item.id;
        } else this.writeObject(records, o, entry, item);
        n++;
        continue;
      }
      items[kept++] = item;
    }
    items.length = kept;
    records[0] = n;
    this.takenCount = n;
  }

  takeNone() {
    this.core.views();
    this.core.transparentObjects[0] = 0;
    this.candidates.length = 0;
  }

  // Whether three's item is one this pass could draw: one of the map's
  // blended buckets, or an object with a material it knows.
  candidate(item) {
    const { object, material } = item;
    if (item.group || Array.isArray(object.material)) return false;
    if (OBJECT_KINDS[material.type] !== undefined) return true;
    return Boolean(object.isMesh && object.parent === this.core.graphics.scene && !object.matrixAutoUpdate && object.matrix.equals(IDENTITY)
      && material.defines && "WORLD_SPACE" in material.defines && !("TERRAIN" in material.defines) && material.isShaderMaterial);
  }

  writeObject(r, o, entry, item) {
    const object = item.object, material = entry.material;
    r[o + OBJ.INDEX] = entry.id; r[o + OBJ.BUCKET] = 0;
    r[o + OBJ.KEY] = item.groupOrder; r[o + OBJ.KEY + 1] = item.renderOrder; r[o + OBJ.KEY + 2] = item.z; r[o + OBJ.KEY + 3] = item.id;
    r.set(object.matrixWorld.elements, o + OBJ.WORLD);
    const c = material.color;
    r[o + OBJ.DIFFUSE] = c.r; r[o + OBJ.DIFFUSE + 1] = c.g; r[o + OBJ.DIFFUSE + 2] = c.b; r[o + OBJ.DIFFUSE + 3] = material.opacity;
    r[o + OBJ.ROTATION] = material.rotation ?? 0;
    r[o + OBJ.CENTER] = object.center?.x ?? 0; r[o + OBJ.CENTER + 1] = object.center?.y ?? 0;
    // refreshTransformUniform: the map's matrix, brought up to date first.
    const map = material.map;
    if (map) { if (map.matrixAutoUpdate) map.updateMatrix(); r.set(map.matrix.elements, o + OBJ.MAP); }
  }

  // The transparent phase, before the renderer's call: what three listed
  // this frame that this pass has not seen, registered (three draws it this
  // frame: from the next one it is taken), then three's own remaining
  // items' keys.
  draw(ctx) {
    const core = this.core;
    for (const item of this.candidates) {
      if (this.taken.has(item.object)) continue;
      const entry = OBJECT_KINDS[item.material.type] !== undefined ? this.registerObject(item) : this.registerBucket(item);
      if (entry !== undefined) this.taken.set(item.object, entry);
    }
    this.candidates.length = 0;
    core.views();
    const keys = core.transparentKeys, items = this.list?.transparent || [], three = this.three;
    three.length = 0;
    for (const item of items) {
      if (!item.object.layers.test(ctx.camera.layers)) continue;
      const o = 4 * three.length;
      if (o + 4 > keys.length) break;
      keys[o] = item.groupOrder; keys[o + 1] = item.renderOrder; keys[o + 2] = item.z; keys[o + 3] = item.id;
      three.push(item);
    }
    ctx.three = three.length;
  }

  drawSegment(k) {
    const core = this.core, renderer = core.renderer, target = renderer.getRenderTarget();
    if (!core.enabled || !core.active) return;
    this.segmentStats.push(core.rust.transparent_segment(k));
    renderer.resetState();
    renderer.setRenderTarget(target);
  }

  // Three's passes for a material (transparent.rs), each its program for
  // the renderer, or null: [{ side, program }], three's program for the
  // front pass (the one it draws last) first among `three`. cache: where
  // the programs are kept (core.program).
  passesOf(material, policy, cache = undefined) {
    const core = this.core, renderer = core.renderer;
    const sides = sidesOf(material);
    const three = sides.map((side) => programForSide(renderer, material, side));
    if (three.some((p) => !p)) return null;
    const programs = three.map((p) => core.program(p, WORLD, material, policy, cache));
    if (programs.some((p) => !p) || programs.some((p) => p.attributes.join() !== programs[0].attributes.join())) return null;
    return { sides, programs, current: renderer.properties.get(material).currentProgram };
  }

  // A map's blended bucket three drew as it is now: { bucket, id, material,
  // program, frame }, null for one left to three, or undefined to look
  // again later.
  registerBucket(item) {
    const core = this.core, renderer = core.renderer, rust = core.rust;
    const { object: mesh, material } = item, geometry = mesh.geometry;
    const props = renderer.properties.get(material);
    if (!props.currentProgram || props.__version !== material.version) return undefined;
    if (geometry.groups.length || Object.keys(geometry.morphAttributes).length || geometry.drawRange.start !== 0 || geometry.drawRange.count < (geometry.index?.count ?? 0)) return null;
    if (mesh.onBeforeRender !== baseBeforeRender || mesh.onAfterRender !== baseAfterRender) return null;
    const blend = blendOf(material), state = stateOf(material);
    if (!blend || !state) return null;
    const passes = this.passesOf(material, POLICY);
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
        const v = material.uniforms[name]?.value;
        const values = typeof v === "number" ? [v] : v?.isVector4 ? [v.x, v.y, v.z, v.w] : v?.isVector3 ? [v.x, v.y, v.z] : v?.isVector2 ? [v.x, v.y] : null;
        if (values) rust.transparent_bucket_uniform(id, pass, name, new Float32Array(values), false);
      }
    });
    this.buckets++;
    return { bucket: true, id, material, program: passes.current, frame: this.frameNo };
  }

  // An object three drew as it is now, with a material this pass draws:
  // { bucket: false, id, material, program, frame }, null for one left to
  // three, or undefined to look again later.
  registerObject(item) {
    const core = this.core, renderer = core.renderer, rust = core.rust;
    const { object, material, geometry } = item;
    const props = renderer.properties.get(material);
    if (!props.currentProgram || props.__version !== material.version) return undefined;
    const kind = OBJECT_KINDS[material.type];
    if (object.isSkinnedMesh || object.isInstancedMesh || object.isBatchedMesh || object.morphTargetInfluences?.length || (!object.isMesh && !object.isSprite)) return null;
    if (object.onBeforeRender !== baseBeforeRender || object.onAfterRender !== baseAfterRender || material.onBeforeRender !== baseMaterialBeforeRender) return null;
    if (material.alphaMap || material.alphaTest > 0 || material.alphaHash || material.vertexColors || material.envMap || material.lightMap || material.aoMap || material.specularMap || material.wireframe) return null;
    const blend = blendOf(material), state = stateOf(material);
    if (!blend || !state || !geometry.index || Object.keys(geometry.morphAttributes).length) return null;
    const passes = this.passesOf(material, OBJECT_POLICY, this.objectPrograms);
    if (!passes) return null;
    const g = core.geometry(geometry, passes.programs[0]);
    if (g === null) return null;
    let map = -1;
    if (passes.programs[0].names.has("map")) { map = core.texture(material.map); if (map < 0) return null; }
    const start = geometry.drawRange.start, count = Math.min(geometry.drawRange.count, geometry.index.count - start);
    const id = rust.transparent_object(kind, g, start, count, new Uint32Array(blend), state.func, state.flags);
    passes.sides.forEach((side, i) => rust.transparent_object_pass(id, passes.programs[i].id, side));
    if (map >= 0) rust.transparent_object_texture(id, map);
    this.objectCount++;
    return { bucket: false, id, material, program: passes.current, frame: this.frameNo };
  }

  // Off: three draws everything in its list again (alone, for a comparison
  // in one battle, or with the whole renderer).
  setActive(on) {
    this.active = on;
    this.disarm();
  }

  restore() {
    this.disarm();
  }
}

function restore(hook) {
  if (hook.own) hook.object.onAfterRender = hook.original;
  else delete hook.object.onAfterRender;
}

export function transparentPass(core) {
  return new TransparentPass(core);
}
