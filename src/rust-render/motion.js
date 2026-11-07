import {Family,Mode,S} from './scene.js';
import {ACTOR} from './actor-layout.js';
import {nativeActorSource} from '../actor-resources.js';
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
import { motionUniforms, motionVectors } from "../motion-vectors.js";

const FLYER_STREAMS=["matrices","previous","shown"];
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
    this.records = [];
    this.skeletons = new Map();
    // The sky's ships' meshes drawn this frame, in three's order, and each
    // one's registration (null: never the renderer's).
    this.extras = [];
    this.extraEntries = new Map();
    this.pendingExtras=new Set();
    this.scratch = new Float64Array(EX.FIRST + MAX_EXTRA_UNIFORMS * UNIFORM);
    this.drawn = 0;
    this.left = 0;
    // Why coverage failed on the last frame (for diagnostics).
    this.why = null;
  }

  // The motion pass draws for motion-vectors.js from now on.
  register() {
    motionVectors.drawer = this;this.core.registry.admit(motionVectors.scene,null,true,Family.MOTION|Family.EXTRA);
    for(const object of motionVectors.extras)this.queueExtra(object);
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
    const registry=core.registry,selected=registry.select(motion.scene,camera,Family.MOTION,Mode.PROXY);
    for(let i=0;i<selected;i++){
      core.views();const proxy=registry.nodes[core.sceneOutput[i*S.STRIDE+S.SLOT]].object;
      const source = proxy.userData.source;
      let descriptor=proxy.userData.nativeMotion;
      if(descriptor===undefined){descriptor=core.rust.actor_motion(nativeActorSource(core,source.userData.actorAsset).id,!!source.skeleton,false);proxy.userData.nativeMotion=descriptor;}
      if(descriptor===undefined||records.length>=MAX_RECORDS)return this.decline('actor motion capacity');
      const record=proxy.userData.motionRow??={kind:KIND_PROXY,object:proxy,bones:-1};record.descriptor=descriptor;record.bones=-1;
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
      core.publishView("motion", renderer, null, camera);
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
  queueExtra(object){this.pendingExtras.add(object);}
  extrasOf(motion, camera) {
    // SkyFlyers has populated its instance matrices by this first view. Cold
    // bounds must use that publication, rather than the empty birth buffers.
    for(const object of this.pendingExtras)this.extra(object,camera);
    this.pendingExtras.clear();
    const core=this.core,registry=core.registry,list=this.extras;list.length=0;
    const count=registry.select(motion.scene,camera,Family.EXTRA,Mode.EXTRA);
    for(let i=0;i<count;i++){core.views();const mesh=registry.nodes[core.sceneOutput[i*S.STRIDE+S.SLOT]].object;
      const entry=this.extra(mesh,camera);if(!entry){this.extraWhy='unsupported';return false;}
      if(list.length>=MAX_EXTRAS){this.extraWhy='too many';return false;}list.push(mesh);
    }
    return true;
  }

  // A ship's mesh as the renderer's: three's program for its material (with
  // three's attribute locations: an instance matrix takes four), its
  // geometry's shared attributes and index, and its instance attributes
  // (the mesh's matrices, the geometry's instanced ones, a buffer each).
  // null for one the renderer can't draw as three does (blended, tested or
  // not instanced by plain float attributes).
  extra(mesh,camera){let entry=this.extraEntries.get(mesh);if(entry!==undefined)return entry;const source=mesh.userData.flyerMotion;if(!source)return null;const core=this.core;
    const row=core.rust.actor_flyer_motion(nativeActorSource(core,source.asset).id,source.capacity,source.arrays.matrices),meta=ACTOR.MOTION_META;mesh.boundingSphere.center.fromArray(row,meta.BOUNDS);mesh.boundingSphere.radius=row[meta.BOUNDS+3];entry={id:row[meta.HANDLE],source,max:Math.min(source.capacity,256),z:0};this.extraEntries.set(mesh,entry);const node=core.registry.admitIfNew(mesh);if(node)core.registry.touch(node);core.buffer=null;core.views();return entry;
  }
  // Locked view masks share the actor's native geometry and a fixed program.
  lockedParts(viewScene,view,locked,records){
    const core=this.core,registry=core.registry,count=registry.select(viewScene,view,Family.VIEW,Mode.LOCKED);
    for(let i=0;i<count;i++){core.views();const node=registry.nodes[core.sceneOutput[i*S.STRIDE+S.SLOT]].object;
      if(!node.userData.actorAsset)continue;
      let descriptor=node.userData.lockedMotion;if(descriptor===undefined){descriptor=core.rust.actor_motion(nativeActorSource(core,node.userData.actorAsset).id,false,true);node.userData.lockedMotion=descriptor;}
      if(descriptor===undefined||records.length>=MAX_RECORDS){this.maskWhy='locked actor capacity';return false;}
      const row=node.userData.lockedRow??={kind:KIND_LOCKED,object:node,bones:-1};row.descriptor=descriptor;records.push(row);
    }
    return true;
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
      bones.set(skeleton.actorPoseSkin||skeleton.boneMatrices,at*16);
      const before = motionVectors.bones(skeleton);
      previous.set(before.data,at*16);
    }
    input[IN.BONES] = p.bones; input[IN.PREVIOUS_BONES] = p.bones;
    const records = this.records;
    for (let r = 0; r < records.length; r++) {
      const record = records[r], object = record.object, o = IN.RECORDS + r * STRIDE;
      input[o + R.KIND] = record.kind; input[o + R.PROGRAM] = ACTOR.KIND.RETAINED; input[o + R.GEOMETRY] = record.descriptor;

      input[o + R.BONE_AT] = Math.max(0, record.bones); input[o + R.PREVIOUS_AT] = Math.max(0, record.bones);
      input.set(object.matrixWorld.elements, o + R.WORLD);
      if (record.kind === KIND_PROXY) {
        input.set(object.userData.previousModel.elements, o + R.PREVIOUS_WORLD);
        input.set(object.isSkinnedMesh ? object.bindMatrix.elements : IDENTITY, o + R.BIND);
        input.set(object.isSkinnedMesh ? object.bindMatrixInverse.elements : IDENTITY, o + R.BIND_INVERSE);
        input.set(object.userData.previousBindInverse.elements, o + R.PREVIOUS_BIND_INVERSE);
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
      out[o+EX.ID]=entry.id;out[o+EX.INSTANCES]=count;out[o+EX.AT]=at;out[o+EX.UNIFORMS]=0;out[o+EX.FIRST]=entry.source.owner.still??1;
      for(const name of FLYER_STREAMS){const data=entry.source.arrays[name],size=name==='shown'?1:16;if(at+count*size>values.length){out[o+EX.INSTANCES]=0;break;}for(let j=0;j<count*size;j++)values[at+j]=data[j];at+=count*size;}
      n++;
    }
    out[0] = n;
  }
}

export function motionPass(core) {
  return new MotionPass(core);
}
