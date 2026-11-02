// The transparent pass (transparent.rs): what the near scene blends, drawn
// in three's back-to-front order, as three's transparent list would. Any
// pass whose draws blend (the effects, the water...) is a source of items
// for it on the renderer's side (transparent.rs Source); this file is the
// page's side of the list and of this pass's own items, the map's blended
// buckets.
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
// The map's blended buckets: each is registered the first time three has
// drawn it (which gives its material a program), then hidden from three. A
// double-sided one is two draws, as three makes them (back faces, then
// front), each with three's own program for that side.
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

// three's program for a material drawn with `side` (0 front, 1 back): the
// one it last drew with for that side, among the material's programs. A
// side's program differs from the other's by its FLIP_SIDED define.
function programForSide(renderer, material, side) {
  const props = renderer.properties.get(material), current = props.currentProgram;
  if (!current) return null;
  const gl = renderer.getContext();
  const vertexOf = (program) => {
    const [vs] = gl.getAttachedShaders(program.program).filter((s) => gl.getShaderParameter(s, gl.SHADER_TYPE) === gl.VERTEX_SHADER);
    return gl.getShaderSource(vs);
  };
  const flipped = (source) => /^#define FLIP_SIDED$/m.test(source);
  const strip = (source) => source.replace(/^#define (FLIP|DOUBLE)_SIDED\n/gm, "");
  const want = side === 1;
  const currentSource = vertexOf(current);
  if (flipped(currentSource) === want && !/^#define DOUBLE_SIDED$/m.test(currentSource)) return current;
  const base = strip(currentSource);
  for (const program of props.programs?.values() || []) {
    if (program === current) continue;
    const source = vertexOf(program);
    if (flipped(source) === want && !/^#define DOUBLE_SIDED$/m.test(source) && strip(source) === base) return program;
  }
  return null;
}

class TransparentPass {
  constructor(core) {
    this.core = core;
    this.name = "transparent";
    this.phase = "transparent";
    this.pending = null;
    this.buckets = [];
    this.count = 0;
    // Three's render list of the near scene this frame, the items it still
    // draws itself, and the hooks armed on them (object -> hook).
    this.list = null;
    this.three = [];
    this.hooks = new Map();
    this.segmentStats = [];
    core.addView("transparentKeys", Float64Array, () => core.rust.transparent_keys_ptr(), () => core.rust.transparent_keys_len());
  }

  // After the near phase: arm the transparent phase on the item three draws
  // just before its transparent list. After the transparent phase: arm each
  // later segment on the item three draws just before it.
  afterPhase(name, ctx) {
    if (name === "near") {
      this.disarm();
      this.list = this.threeList(ctx.renderer, ctx.scene);
      if (!this.list) return;
      const before = this.list.transmissive.length ? this.list.transmissive : this.list.opaque;
      const last = before[before.length - 1];
      const start = () => this.core.phase("transparent", ctx.renderer, ctx.scene, ctx.camera);
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

  // The transparent phase, before the renderer's call: this pass's buckets
  // (registered, shown), then three's own remaining items' keys.
  draw(ctx) {
    const core = this.core;
    for (const b of this.buckets) {
      let shown = true;
      for (let o = b.mesh; o && o !== ctx.scene; o = o.parent) if (!o.visible) { shown = false; break; }
      if (shown !== b.shown) { b.shown = shown; core.rust.transparent_bucket_shown(b.id, shown); }
    }
    // Registered now, hidden from three now, but in three's list this frame:
    // drawn from the next frame (shown above), not twice in this one.
    if (!this.pending || this.pending.length) this.registerBuckets();
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

  // The map's blended buckets in the near scene the renderer can draw.
  registerBuckets() {
    const core = this.core, scene = core.graphics.scene, renderer = core.renderer, rust = core.rust;
    if (!this.pending) {
      this.pending = scene.children.filter((o) => o.isMesh && !o.matrixAutoUpdate && o.matrix.equals(IDENTITY) && !Array.isArray(o.material)
        && o.material.defines && "WORLD_SPACE" in o.material.defines && !("TERRAIN" in o.material.defines) && o.material.transparent);
    }
    this.pending = this.pending.filter((mesh) => {
      const material = mesh.material, geometry = mesh.geometry, props = renderer.properties.get(material);
      // Not drawn by three yet as it is now (no program, or one for an older version of it).
      if (!props.currentProgram || props.__version !== material.version) return true;
      if (geometry.groups.length || Object.keys(geometry.morphAttributes).length || geometry.drawRange.start !== 0 || geometry.drawRange.count < (geometry.index?.count ?? 0)) return false;
      const blend = blendOf(material), state = stateOf(material);
      if (!blend || !state) return false;
      const sides = sidesOf(material);
      const programs = sides.map((side) => { const p = programForSide(renderer, material, side); return p && core.program(p, WORLD, material, POLICY); });
      if (programs.some((p) => !p)) { core.errors.push(`blended ${mesh.name}: program`); return false; }
      const g = core.geometry(geometry, programs[0]);
      if (g === null) return false;
      if (programs.some((p) => p.attributes.join() !== programs[0].attributes.join())) { core.errors.push(`blended ${mesh.name}: attributes`); return false; }
      if (!geometry.boundingSphere) geometry.computeBoundingSphere();
      if (!geometry.boundingBox) geometry.computeBoundingBox();
      const s = geometry.boundingSphere, bb = geometry.boundingBox;
      const pointLit = Boolean(material.userData.pointLit) && programs.some((p) => p.names.has("zeroLightCount"));
      const id = rust.transparent_bucket(g, new Uint32Array(blend), state.func, state.flags, pointLit, new Float64Array([mesh.id, mesh.renderOrder]),
        new Float64Array([s.center.x, s.center.y, s.center.z, s.radius, bb.min.x, bb.min.y, bb.min.z, bb.max.x, bb.max.y, bb.max.z]));
      const map = core.texture(material.uniforms.map?.value);
      if (map >= 0) rust.transparent_bucket_texture(id, 0, map);
      if (programs.some((p) => p.names.has("bumpMap"))) { const bump = core.texture(material.uniforms.bumpMap?.value); if (bump >= 0) rust.transparent_bucket_texture(id, 2, bump); }
      sides.forEach((side, i) => {
        const pass = rust.transparent_bucket_pass(id, programs[i].id, side);
        for (const name of programs[i].names) {
          if (!MATERIAL.has(name)) continue;
          const v = material.uniforms[name]?.value;
          const values = typeof v === "number" ? [v] : v?.isVector4 ? [v.x, v.y, v.z, v.w] : v?.isVector3 ? [v.x, v.y, v.z] : v?.isVector2 ? [v.x, v.y] : null;
          if (values) rust.transparent_bucket_uniform(id, pass, name, new Float32Array(values), false);
        }
      });
      material.visible = false;
      rust.transparent_bucket_shown(id, false);
      this.buckets.push({ id, mesh, shown: false });
      this.count++;
      return false;
    });
  }

  setActive(on) {
    this.disarm();
    for (const b of this.buckets) if (b.mesh.material) b.mesh.material.visible = !on;
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
