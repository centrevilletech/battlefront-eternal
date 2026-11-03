// The motion pass (motion.rs): the temporal upscaler's motion of the bodies
// that move on their own (motion-vectors.js), drawn by the renderer in
// place of three's render of their proxies and of the first-person view's
// locked mask (MotionVectors.drawer).
//
// Each proxy prepare() shows this frame that three's render would list (on
// the camera's layers, in its frustum) is a record, in three's order (the
// proxies' material ids: each proxy has a material of its own): three's
// program for its material as three compiled it (Kind::Motion: what it
// reads per object from its row), its source's geometry, its matrices now
// and last frame (the ones prepare() keeps) and its bones now and last
// frame. Then each part of the first-person view three's override render
// would list (a part the view pass hides from three included) with the
// locked mask's program and the view's camera. The renderer draws them when
// it has a program for every one (three has drawn a proxy of each kind,
// and the mask, once); otherwise three draws the frame's motion as before.
// The sky's ships, which draw their own motion (MotionVectors.addExtra),
// stay three's: drawn first, into the target three clears, where the near
// scene drew nothing (no proxy's pixel: a proxy keeps only what lies on the
// near scene's surface), so the order is three's.
import * as THREE from "three";
import { motionUniforms, motionVectors } from "../motion-vectors.js";

const MOTION = 9;
const KIND_PROXY = 1, KIND_LOCKED = 2;
// motion.rs's layout: the frame's values, then the records.
const IN = { PROJECTION: 0, VIEW: 16, VIEW_PROJECTION: 32, VIEW_VIEW: 48, CURRENT: 64, PREVIOUS: 80, RANGE: 96, TARGET: 98, SIZE: 99, DEPTH: 101,
  CLEAR: 102, BONES: 103, PREVIOUS_BONES: 104, RECORDS: 106 };
const R = { KIND: 0, PROGRAM: 1, GEOMETRY: 2, FIRST: 3, COUNT: 4, SIDE: 5, BONE_AT: 6, PREVIOUS_AT: 7, WORLD: 8, PREVIOUS_WORLD: 24, BIND: 40,
  BIND_INVERSE: 56, PREVIOUS_BIND_INVERSE: 72 };
const STRIDE = 88, MAX_RECORDS = 256, MAX_BONES = (1024 * 32) / 4;
// What a motion program reads besides the frame's values the pass sets
// (the projection, the view-projections now and last frame, the depth
// range): its row's matrices, both frames' bones, the scene's depth.
const POLICY = {
  uniforms: new Set(["modelMatrix", "modelViewMatrix", "normalMatrix", "bindMatrix", "bindMatrixInverse", "boneTexture", "previousModel", "previousBindInverse",
    "previousBones", "currentViewProjection", "previousViewProjection", "sceneDepth", "depthRange"]),
  material: new Set(),
};
const IDENTITY = new THREE.Matrix4().elements;
// Whether three compiled a program with skinning (the locked mask's
// program is made per object it overrides).
const skinnedPrograms = new WeakMap();
function skinned(gl, threeProgram) {
  let is = skinnedPrograms.get(threeProgram);
  if (is === undefined) {
    const vs = gl.getAttachedShaders(threeProgram.program).find((s) => gl.getShaderParameter(s, gl.SHADER_TYPE) === gl.VERTEX_SHADER);
    is = /^\s*#define USE_SKINNING\b/m.test(vs ? gl.getShaderSource(vs) : "");
    skinnedPrograms.set(threeProgram, is);
  }
  return is;
}

// three's index range for an item (renderBufferDirect, triangles): the
// geometry's draw range within the group's and the index.
function rangeOf(geometry, group) {
  const range = geometry.drawRange, count = geometry.index.count;
  let first = range.start, last = range.start + range.count;
  if (group) { first = Math.max(first, group.start); last = Math.min(last, group.start + group.count); }
  first = Math.max(first, 0); last = Math.min(last, count);
  return last > first && last !== Infinity ? [first, last - first] : null;
}

class MotionPass {
  constructor(core) {
    this.core = core;
    this.name = "motion";
    this.phase = "motion";
    // Its programs are its own (Kind::Motion), apart from any other pass's.
    this.programs = new Map();
    // A program three made for a proxy of each kind (rigid, skinned): every
    // proxy of a kind compiles to it (one material each, made alike).
    this.kinds = [null, null];
    this.input = false;
    this.pending = null;
    this.complete = false;
    this.frustum = new THREE.Frustum();
    this.screen = new THREE.Matrix4();
    this.viewFrustum = new THREE.Frustum();
    this.viewScreen = new THREE.Matrix4();
    this.proxies = [];
    this.records = [];
    this.skeletons = new Map();
    this.drawn = 0;
    this.left = 0;
  }

  // The motion pass draws for motion-vectors.js from now on.
  register() {
    motionVectors.drawer = this;
  }

  // Off (this pass alone): three draws the motion.
  setActive(on) {
    this.on = on;
  }

  restore() {
    if (motionVectors.drawer === this) motionVectors.drawer = null;
  }

  // motion-vectors.js (render, after prepare): the frame's motion drawn here
  // when every proxy three would draw and every part of the view's mask is
  // one the renderer has a program for; true when it drew it. frame: the
  // target, the camera the proxies draw with (as the scene was drawn), the
  // view scene to mark fixed (or null) and its camera, the scene's depth.
  cover(renderer, motion, { target, camera, viewScene, view, depth }) {
    const core = this.core;
    if (!core.enabled || !core.active || core.lost || this.on === false || !core.supported()) return false;
    const records = this.records;
    records.length = 0;
    this.skeletons.clear();
    let bones = 0;
    // The proxies three's render would list, in its order.
    this.screen.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.frustum.setFromProjectionMatrix(this.screen);
    const proxies = this.proxies;
    proxies.length = 0;
    for (const proxy of motion.proxies) {
      if (!proxy.visible || !proxy.layers.test(camera.layers)) continue;
      if (proxy.frustumCulled && !this.frustum.intersectsObject(proxy)) continue;
      proxies.push(proxy);
    }
    proxies.sort((a, b) => a.renderOrder - b.renderOrder || a.material.id - b.material.id || a.id - b.id);
    for (const proxy of proxies) {
      const kind = proxy.isSkinnedMesh ? 1 : 0, source = proxy.userData.source;
      // Its own program, or (until three has drawn it) its kind's.
      const threeProgram = core.renderer.properties.get(proxy.material).currentProgram ?? this.kinds[kind];
      if (threeProgram) this.kinds[kind] ??= threeProgram;
      const program = threeProgram && core.program(threeProgram, MOTION, proxy.material, POLICY, this.programs);
      // The source's buffers, which the proxy shares (one registration for
      // every proxy of a model).
      const geometry = source.geometry.index === proxy.geometry.index && program ? core.geometry(source.geometry, program) : null;
      if (geometry === null || geometry === undefined || records.length >= MAX_RECORDS) { this.left++; return false; }
      const record = { kind: KIND_PROXY, object: proxy, program: program.id, geometry, first: 0, count: proxy.geometry.index.count, side: proxy.material.side, bones: -1 };
      if (proxy.isSkinnedMesh) {
        const skeleton = proxy.skeleton;
        let at = this.skeletons.get(skeleton);
        if (at === undefined) {
          if (bones + skeleton.bones.length > MAX_BONES) { this.left++; return false; }
          at = bones;
          bones += skeleton.bones.length;
          this.skeletons.set(skeleton, at);
        }
        record.bones = at;
      }
      records.push(record);
    }
    // The view's parts three's override render would list, a part the view
    // pass hides from three among them.
    if (viewScene && !this.lockedParts(viewScene, view, motion.locked, records)) { this.left++; return false; }
    const targetTexture = core.renderer.properties.get(target.texture).__webglTexture ? core.texture(target.texture) : -1;
    const depthTexture = depth && core.renderer.properties.get(depth).__webglTexture ? core.texture(depth) : -1;
    if (targetTexture < 0 || depthTexture < 0) { this.left++; return false; }
    // The sky's ships, three's: into the target three clears.
    let clear = true;
    for (const extra of motion.extras) {
      if (!extra.visible) continue;
      motion.drawExtras(renderer, camera, target);
      clear = false;
      break;
    }
    this.pending = { camera, view, target, targetTexture, depthTexture, clear, bones };
    this.complete = false;
    try {
      core.phase("motion", renderer, null, camera, true);
    } catch (error) {
      core.fail(error, renderer);
      this.complete = false;
    }
    this.pending = null;
    if (this.complete) this.drawn++;
    else this.left++;
    return this.complete;
  }

  // The view scene walked as three's projectObject walks it (shown, on the
  // camera's layers, in its frustum): a record for each item it would list
  // (each shown group of an object with a material per group), drawn with
  // the locked material three overrides them with. False for anything the
  // pass can't draw (a sprite, a line, a skinned part, the locked mask's
  // program not made by three yet).
  lockedParts(viewScene, view, locked, records) {
    const core = this.core;
    const threeProgram = core.renderer.properties.get(locked).currentProgram;
    if (!threeProgram || skinned(core.gl, threeProgram)) return false;
    const program = core.program(threeProgram, MOTION, locked, POLICY, this.programs);
    if (!program) return false;
    const frustum = this.viewFrustum.setFromProjectionMatrix(this.viewScreen.multiplyMatrices(view.projectionMatrix, view.matrixWorldInverse));
    const hidden = core.byName.view?.parts;
    // Shown to three, or hidden from it by the view pass (which draws it).
    const shown = (object, material) => Boolean(material) && (material.visible || hidden?.get(object)?.material === material);
    const visit = (object) => {
      if (!object.visible) return true;
      if (object.layers.test(view.layers)) {
        if (object.isSprite || object.isLine || object.isPoints || object.isSkinnedMesh || object.isInstancedMesh || object.isBatchedMesh) return false;
        if (object.isMesh && (!object.frustumCulled || frustum.intersectsObject(object))) {
          const material = object.material, groups = Array.isArray(material) ? object.geometry.groups.filter((g) => shown(object, material[g.materialIndex])) : shown(object, material) ? [null] : [];
          if (groups.length) {
            if (!object.geometry.index) return false;
            const geometry = core.geometry(object.geometry, program);
            if (geometry === null || geometry === undefined) return false;
            for (const group of groups) {
              const range = rangeOf(object.geometry, group);
              if (!range) continue;
              if (records.length >= MAX_RECORDS) return false;
              records.push({ kind: KIND_LOCKED, object, program: program.id, geometry, first: range[0], count: range[1], side: locked.side, bones: -1 });
            }
          }
        }
      }
      for (const child of object.children) if (!visit(child)) return false;
      return true;
    };
    return visit(viewScene);
  }

  // The phase: the frame's values, the records and their bones written for
  // motion.rs.
  draw(ctx) {
    const p = this.pending;
    if (!p) return;
    const core = this.core;
    if (!this.input) {
      core.addView("motionInput", Float64Array, () => core.rust.motion_input_ptr(), () => core.rust.motion_input_len());
      core.addView("motionBones", Float32Array, () => core.rust.motion_bones_ptr(), () => core.rust.motion_bones_len());
      core.addView("motionPrevious", Float32Array, () => core.rust.motion_previous_ptr(), () => core.rust.motion_previous_len());
      this.input = true;
    }
    core.views();
    const input = core.motionInput, bones = core.motionBones, previous = core.motionPrevious;
    input.set(p.camera.projectionMatrix.elements, IN.PROJECTION);
    input.set(p.camera.matrixWorldInverse.elements, IN.VIEW);
    input.set((p.view ?? p.camera).projectionMatrix.elements, IN.VIEW_PROJECTION);
    input.set((p.view ?? p.camera).matrixWorldInverse.elements, IN.VIEW_VIEW);
    input.set(motionUniforms.currentViewProjection.value.elements, IN.CURRENT);
    input.set(motionUniforms.previousViewProjection.value.elements, IN.PREVIOUS);
    const range = motionUniforms.depthRange.value;
    input[IN.RANGE] = range.x; input[IN.RANGE + 1] = range.y;
    input[IN.TARGET] = p.targetTexture; input[IN.SIZE] = p.target.width; input[IN.SIZE + 1] = p.target.height;
    input[IN.DEPTH] = p.depthTexture; input[IN.CLEAR] = p.clear ? 1 : 0;
    // Each skeleton's bones once: now (three's, as it worked them out in
    // the near scene's render), and last frame's (prepare()'s copy).
    for (const [skeleton, at] of this.skeletons) {
      bones.set(skeleton.boneMatrices, at * 16);
      const before = motionVectors.bones(skeleton);
      previous.set(before.data.subarray(0, before.length), at * 16);
    }
    input[IN.BONES] = p.bones; input[IN.PREVIOUS_BONES] = p.bones;
    const records = this.records;
    for (let r = 0; r < records.length; r++) {
      const record = records[r], object = record.object, o = IN.RECORDS + r * STRIDE;
      input[o + R.KIND] = record.kind; input[o + R.PROGRAM] = record.program; input[o + R.GEOMETRY] = record.geometry;
      input[o + R.FIRST] = record.first; input[o + R.COUNT] = record.count; input[o + R.SIDE] = record.side;
      input[o + R.BONE_AT] = Math.max(0, record.bones); input[o + R.PREVIOUS_AT] = Math.max(0, record.bones);
      input.set(object.matrixWorld.elements, o + R.WORLD);
      if (record.kind === KIND_PROXY) {
        const u = object.material.uniforms;
        input.set(u.previousModel.value.elements, o + R.PREVIOUS_WORLD);
        input.set(object.isSkinnedMesh ? object.bindMatrix.elements : IDENTITY, o + R.BIND);
        input.set(object.isSkinnedMesh ? object.bindMatrixInverse.elements : IDENTITY, o + R.BIND_INVERSE);
        input.set(u.previousBindInverse.value.elements, o + R.PREVIOUS_BIND_INVERSE);
      } else {
        input.set(object.matrixWorld.elements, o + R.PREVIOUS_WORLD);
        input.set(IDENTITY, o + R.BIND); input.set(IDENTITY, o + R.BIND_INVERSE); input.set(IDENTITY, o + R.PREVIOUS_BIND_INVERSE);
      }
    }
    ctx.records = records.length;
    this.complete = true;
  }
}

export function motionPass(core) {
  return new MotionPass(core);
}
