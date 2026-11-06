// The motion pass (motion.rs): the temporal upscaler's motion of the bodies
// that move on their own (motion-vectors.js), drawn by the renderer in
// place of three's render of their proxies and of the first-person view's
// locked mask (MotionVectors.drawer).
//
// Each proxy prepare() shows this frame that three's render would list (on
// the camera's layers, in its frustum) is a record, in three's order (the
// proxies' material ids: each proxy has a material of its own): its owned
// program (Kind::Motion: what it
// reads per object from its row), its source's geometry, its matrices now
// and last frame (the ones prepare() keeps) and its bones now and last
// frame. Then each part of the first-person view three's override render
// would list (a part the view pass hides from three included) with the
// locked mask's owned program and the view's camera. Registration requires
// every proxy kind and mask program before the motion phase draws.
// The sky's ships draw their own motion (MotionVectors.addExtra: an
// instanced mesh a model part, sky-flyers.js), first, in three's order
// (render order, material, depth, id), each with its catalog-owned program
// (Kind::Plain: every uniform set per draw)
// and its ships' instance attributes written each frame; their hook (the
// flyers' still frame) runs first, as three runs it. They write only where
// the near scene drew nothing (no proxy's pixel: a proxy keeps only what
// lies on the near scene's surface), so drawing them before the proxies is
// three's order.
import * as THREE from "three";
import { arcBatches } from "../arc-lightning.js";
import { ownedEffectProgram } from "./effect-material.js";
import { motionUniforms, motionVectors } from "../motion-vectors.js";
import { writeUniform } from "./post.js";

const MOTION = 9;
const KIND_PROXY = 1, KIND_LOCKED = 2;
// motion.rs's layout: the frame's values, then the records.
const IN = { PROJECTION: 0, VIEW: 16, VIEW_PROJECTION: 32, VIEW_VIEW: 48, CURRENT: 64, PREVIOUS: 80, RANGE: 96, TARGET: 98, SIZE: 99, DEPTH: 101,
  CLEAR: 102, BONES: 103, PREVIOUS_BONES: 104, RECORDS: 106 };
const R = { KIND: 0, PROGRAM: 1, GEOMETRY: 2, FIRST: 3, COUNT: 4, SIDE: 5, BONE_AT: 6, PREVIOUS_AT: 7, WORLD: 8, PREVIOUS_WORLD: 24, BIND: 40,
  BIND_INVERSE: 56, PREVIOUS_BIND_INVERSE: 72 };
const STRIDE = 88, MAX_RECORDS = 256, MAX_BONES = (1024 * 32) / 4;
// motion.rs's extras: each its registration, instances, side, where its
// instances' values start, its uniforms (post.rs's layout).
const EX = { ID: 0, INSTANCES: 1, SIDE: 2, AT: 3, UNIFORMS: 4, FIRST: 5 };
const UNIFORM = 18, MAX_EXTRA_UNIFORMS = 8, EXTRA = EX.FIRST + MAX_EXTRA_UNIFORMS * UNIFORM, MAX_EXTRAS = 16;
const IDENTITY = new THREE.Matrix4().elements;

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
    // Proxy programs use the motion row; sky extras have explicit Plain
    // descriptors because their shaders consume their own instance attributes.
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
    // The sky's ships' meshes drawn this frame, in three's order, and each
    // one's registration (null: never the renderer's).
    this.extras = [];
    this.extraEntries = new Map();
    this.scratch = new Float64Array(EX.FIRST + MAX_EXTRA_UNIFORMS * UNIFORM);
    this.drawn = 0;
    this.left = 0;
    // Why coverage failed on the last frame (for diagnostics).
    this.why = null;
  }

  // The motion pass draws for motion-vectors.js from now on.
  register() {
    motionVectors.drawer = this;
  }




  // motion-vectors.js (render, after prepare): the frame's motion drawn here
  // when every proxy three would draw and every part of the view's mask is
  // one the renderer has a program for; true when it drew it. frame: the
  // target, the camera the proxies draw with (as the scene was drawn), the
  // view scene to mark fixed (or null) and its camera, the scene's depth.
  cover(renderer, motion, { target, camera, viewScene, view, depth }) {
    const core = this.core;
    if (core.contextLost) return this.decline("off", false);
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
      const source = proxy.userData.source;
      const program = ownedEffectProgram(core, proxy.material, MOTION);
      const geometry = source.geometry.index === proxy.geometry.index ? core.geometry(source.geometry, program) : null;
      if (geometry === null || geometry === undefined || records.length >= MAX_RECORDS) return this.decline(geometry === null || geometry === undefined ? "proxy: geometry" : "records");
      const record = { kind: KIND_PROXY, object: proxy, program: program.id, geometry, first: 0, count: proxy.geometry.index.count, side: proxy.material.side, bones: -1 };
      if (proxy.isSkinnedMesh) {
        const skeleton = proxy.skeleton;
        let at = this.skeletons.get(skeleton);
        if (at === undefined) {
          if (bones + skeleton.bones.length > MAX_BONES) return this.decline("bones");
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
    if (viewScene && !this.lockedParts(viewScene, view, motion.locked, records)) return this.decline(`mask: ${this.maskWhy}`);
    renderer.initRenderTarget(target);
    const targetTexture = core.renderer.properties.get(target.texture).__webglTexture ? core.texture(target.texture) : -1;
    const depthTexture = depth ? core.texture(depth) : -1;
    if (targetTexture < 0 || depthTexture < 0) return this.decline(targetTexture < 0 ? "target" : "depth");
    // The sky's ships' own motion, first.
    if (!this.extrasOf(motion, camera)) return this.decline(`extra: ${this.extraWhy}`);
    this.pending = { camera, view, target, targetTexture, depthTexture, clear: true, bones, motion };
    this.complete = false;
    try {
      core.phase("motion", renderer, null, camera, true);
    } catch (error) {
      core.fail(error, renderer);
      this.complete = false;
    }
    this.pending = null;
    if (this.complete) { this.drawn++; this.why = null; } else this.decline("phase");
    return this.complete;
  }

  decline(why, registered = true) {
    this.left++;
    this.why = why;
    if (registered) this.core.stale = true;
    return false;
  }

  // The sky's ships' meshes three's render of the frame's motion would list
  // (shown, on the camera's layers; never culled), in its order: render
  // order, material, z (its bounding sphere's centre through the camera, as
  // projectObject takes it), id. False when one isn't the renderer's (yet).
  extrasOf(motion, camera) {
    const list = this.extras, point = this.point ??= new THREE.Vector3();
    list.length = 0;
    for (const mesh of motion.extras) {
      if (!mesh.visible || !mesh.layers.test(camera.layers)) continue;
      if (mesh.frustumCulled && !this.frustum.intersectsObject(mesh)) continue;
      const entry = this.extra(mesh, camera);
      if (!entry) { this.extraWhy = entry === undefined ? "no program yet" : "unsupported"; return false; }
      if (list.length >= MAX_EXTRAS) { this.extraWhy = "too many"; return false; }
      if (mesh.boundingSphere !== undefined) { if (mesh.boundingSphere === null) mesh.computeBoundingSphere(); point.copy(mesh.boundingSphere.center); }
      else { if (mesh.geometry.boundingSphere === null) mesh.geometry.computeBoundingSphere(); point.copy(mesh.geometry.boundingSphere.center); }
      entry.z = point.applyMatrix4(mesh.matrixWorld).applyMatrix4(this.screen).z;
      list.push(mesh);
    }
    const entries = this.extraEntries;
    list.sort((a, b) => a.renderOrder - b.renderOrder || a.material.id - b.material.id || entries.get(a).z - entries.get(b).z || a.id - b.id);
    return true;
  }

  // A ship's mesh as the renderer's: three's program for its material (with
  // three's attribute locations: an instance matrix takes four), its
  // geometry's shared attributes and index, and its instance attributes
  // (the mesh's matrices, the geometry's instanced ones, a buffer each).
  // null for one the renderer can't draw as three does (blended, tested or
  // not instanced by plain float attributes).
  extra(mesh, camera) {
    let entry = this.extraEntries.get(mesh);
    if (entry !== undefined) return entry;
    const core = this.core, material = mesh.material, geometry = mesh.geometry;
    const program = ownedEffectProgram(core, material, 3);
    const attributes = program.attributes;
    entry = null;
    const perInstance = (name) => name === "instanceMatrix" || Boolean(geometry.attributes[name]?.isInstancedBufferAttribute);
    const instanced = [], layout = [];
    let ok = Boolean(program) && program.uniforms.length <= MAX_EXTRA_UNIFORMS && material.blending === THREE.NormalBlending && !material.transparent
      && !material.depthTest && !material.depthWrite && material.colorWrite;
    for (const [name, location] of ok ? attributes : []) {
      if (!perInstance(name)) continue;
      const attribute = name === "instanceMatrix" ? mesh.instanceMatrix : geometry.attributes[name];
      const size = attribute?.itemSize;
      if (!(attribute?.array instanceof Float32Array) || attribute.isInterleavedBufferAttribute || (attribute.meshPerAttribute ?? 1) !== 1 || (size > 4 && size !== 16)) { ok = false; break; }
      layout.push(location, size > 4 ? 4 : 1, size > 4 ? 4 : size);
      instanced.push({ attribute, size });
    }
    // Every uniform it reads has a value the renderer can set.
    // Extra meshes are independent of colour-pass visibility. Prepare their
    // samplers before validation and before taking shared motion-input views.
    if (program) for (const uniform of program.uniforms) if (uniform.type >= 32) {
      const texture = material.uniforms?.[uniform.name]?.value ?? material[uniform.name];
      if (texture?.isTexture) core.texture(texture);
    }
    for (let k = 0; ok && k < program.uniforms.length; k++) ok = writeUniform(core, this.scratch, 0, k, program.uniforms[k], material, mesh, camera);
    if (ok) {
      const g = core.geometry(geometry, { id: program.id, attributes: attributes.filter(([name]) => !perInstance(name)) });
      if (g !== null && g !== undefined) {
        const max = mesh.instanceMatrix.count;
        entry = { id: core.rust.motion_extra(program.id, g, max, new Uint32Array(layout)), program, instanced, max, z: 0 };
      }
    }
    this.extraEntries.set(mesh, entry);
    return entry;
  }

  // The view scene walked as three's projectObject walks it (shown, on the
  // camera's layers, in its frustum): a record for each item it would list
  // (each shown group of an object with a material per group), drawn with
  // the locked material three overrides them with. False for anything the
  // pass can't draw (a sprite, a line, a skinned part, the locked mask's
  // program not made by three yet).
  lockedParts(viewScene, view, locked, records) {
    const core = this.core;
    this.maskWhy = "program";
    const program = ownedEffectProgram(core, locked, MOTION);
    const frustum = this.viewFrustum.setFromProjectionMatrix(this.viewScreen.multiplyMatrices(view.projectionMatrix, view.matrixWorldInverse));
    const hidden = core.byName.view?.parts;
    // Shown to three, or hidden from it by the view pass (which draws it).
    const shown = (object, material) => Boolean(material) && (material.visible || hidden?.get(object)?.material === material);
    const visit = (object) => {
      if (!object.visible) return true;
      if (object.layers.test(view.layers)) {
        if (object.isSprite || object.isLine || object.isPoints || object.isSkinnedMesh || object.isInstancedMesh || object.isBatchedMesh) { this.maskWhy = object.type; return false; }
        if (object.isMesh && (!object.frustumCulled || frustum.intersectsObject(object))) {
          const material = object.material, groups = Array.isArray(material) ? object.geometry.groups.filter((g) => shown(object, material[g.materialIndex])) : shown(object, material) ? [null] : [];
          // Arc capsules expose corners, not positions. The original mask
          // received a constant zero position and produced degenerate triangles.
          // Their visible view draw remains the effects batch's responsibility.
          const emptyArcMask = arcBatches.has(object) && !object.geometry.attributes.position;
          if (groups.length && !emptyArcMask) {
            if (material.userData?.bfeShape !== undefined) object.onBeforeRender(core.renderer, viewScene, view, object.geometry, locked, null);
            this.maskWhy = "geometry";
            if (!object.geometry.index) return false;
            const geometry = core.geometry(object.geometry, program);
            if (geometry === null || geometry === undefined) return false;
            for (const group of groups) {
              const range = rangeOf(object.geometry, group);
              if (!range) continue;
              if (records.length >= MAX_RECORDS) { this.maskWhy = "records"; return false; }
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
      core.addView("motionExtras", Float64Array, () => core.rust.motion_extras_ptr(), () => core.rust.motion_extras_len());
      core.addView("motionExtraValues", Float32Array, () => core.rust.motion_extra_values_ptr(), () => core.rust.motion_extra_values_len());
      this.input = true;
    }
    // The ships' hooks first, as three runs them before each draw (the
    // flyers' still frame: once a frame).
    for (const mesh of this.extras) mesh.onBeforeRender(ctx.renderer, p.motion.scene, p.camera, mesh.geometry, mesh.material, null);
    core.views();
    this.writeExtras(core, p.camera);
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

  // The ships' meshes this frame (extrasOf), each its registration, its
  // instances (three draws the mesh's count), its side, its instance
  // attributes' values and its uniforms as three would set them.
  writeExtras(core, camera) {
    const out = core.motionExtras, values = core.motionExtraValues;
    let n = 0, at = 0;
    for (const mesh of this.extras) {
      const entry = this.extraEntries.get(mesh), o = 1 + n * EXTRA, count = Math.min(mesh.count, entry.max);
      out[o + EX.ID] = entry.id; out[o + EX.INSTANCES] = count; out[o + EX.SIDE] = mesh.material.side; out[o + EX.AT] = at;
      for (const { attribute, size } of entry.instanced) {
        if (at + count * size > values.length) { out[o + EX.INSTANCES] = 0; break; }
        values.set(attribute.array.subarray(0, count * size), at);
        at += count * size;
      }
      const uniforms = entry.program.uniforms;
      out[o + EX.UNIFORMS] = uniforms.length;
      for (let k = 0; k < uniforms.length; k++) writeUniform(core, out, o + EX.FIRST + k * UNIFORM, k, uniforms[k], mesh.material, mesh, camera);
      n++;
    }
    out[0] = n;
  }
}

export function motionPass(core) {
  return new MotionPass(core);
}
