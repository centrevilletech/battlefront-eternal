import {sceneProperty} from "./rust-render/scene-changes.js";
import {Family,Mode} from './rust-render/scene.js';
import {SceneMotionHeader as H,SceneMotionEvent as E,SceneSelection as Selected,SceneMotionKind as K} from './rust-render/scene-layout.js';
import * as THREE from "three";


// Screen motion of moving bodies, for the temporal upscaler (upscaler.js).
//
// Static geometry needs no motion pass: its motion follows from the depth
// buffer and the camera's last two matrices. Soldiers, held weapons and
// vehicles move on their own, and reprojecting them as if they were static
// smears them (worst for the player's own body, which the chase camera
// carries along). Each such mesh gets a proxy that shares its buffers and
// skeleton and draws, in a scene of its own, the distance its surface moved
// on screen since the last frame. Its matrices and bones from the last frame
// are kept here.
//
// The target holds the motion in UV units (rg) and what drew it (a): 0 the
// static scene, 0.5 a tracked body, 1 the first-person weapon.
//
// The proxies draw without a depth buffer. A fragment counts only where it
// lies on the surface the scene's depth shows there; behind it, in front of
// it (a cut-out hole, a hidden first-person body) it is dropped.
const FAR = 60;

const shared = {
  currentViewProjection: { value: new THREE.Matrix4() },
  previousViewProjection: { value: new THREE.Matrix4() },
  sceneDepth: { value: null },
  depthRange: { value: new THREE.Vector2(0.5, 200) },
};

// The uniforms every motion draw reads, for objects that draw their own
// (addExtra).
export const motionUniforms = shared;

export class MotionVectors {
  constructor() {
    this.scene = new THREE.Scene();
    sceneProperty(this.scene,"matrixWorldAutoUpdate",false);
    this.proxies = new Set();
    this.extras = new Set();
    this.nextOrder=0;
    // The Rust renderer's motion pass when it draws for this one (render).
    this.drawer = null;
    this.renderInput={target:null,camera:null,viewScene:null,view:null,depth:null};
  }

  // Sky motion still consumes the displayed-frame receipt; the native history
  // owner removes this facade when it joins the environment motion inputs.
  get frame(){return this.drawer?this.drawer.core.rust.motion_history_sequence():0;}

  // A mesh that moves by itself. `far` (m): beyond it from the camera its
  // few pixels rely on the upscaler's history clipping instead.
  track(source, { far = FAR } = {}) {
    if (!source?.userData.actorAsset || source.userData.motion) return null;
    const proxy=new THREE.Object3D();sceneProperty(proxy,"boundingSphere",source.boundingSphere);
    proxy.userData.order=this.nextOrder++;
    if(source.skeleton){proxy.isSkinnedMesh=true;proxy.skeleton=source.skeleton;proxy.bindMatrix=source.bindMatrix;}
    sceneProperty(proxy,"matrixAutoUpdate",false);
    sceneProperty(proxy,"matrixWorldAutoUpdate",false);
    proxy.userData.source = source;
    proxy.userData.far = far;
    source.userData.motion = proxy;
    this.proxies.add(proxy);
    this.scene.add(proxy);
    return proxy;
  }

  // An object that draws its own motion with the proxies (sky-flyers.js's
  // ships: the far scene's, which the near scene's depth can't place). It
  // draws while visible, with the camera the proxies draw with.
  addExtra(object) {
    sceneProperty(object,"matrixAutoUpdate",false);
    this.extras.add(object);
    this.scene.add(object);
    if(this.drawer)this.drawer.queueExtra(object);
  }

  removeExtra(object) {
    this.extras.delete(object);
    this.scene.remove(object);
  }

  // Every mesh of a model.
  trackAll(object, options) {
    object.traverse((child) => { if (child.userData.actorAsset) this.track(child, options); });
  }

  // MotionVectors.prepare consumes retained native visibility/lifecycle output.
  // The native history owner retains current/previous matrices and skin bytes.
  prepare(root, camera) {
    const core=this.drawer.core,registry=core.registry;
    const count=registry.select(this.scene,camera,Family.MOTION,Mode.MOTION_PREPARE,camera.layers.mask,0,root);
    core.views();const events=core.sceneMotionEvents[H.EVENTS];
    for(let i=0;i<events;i++){
      core.views();const at=H.STRIDE+i*E.STRIDE,node=registry.nodes[core.sceneMotionEvents[at+E.SLOT]];
      if(node?.generation!==core.sceneMotionEvents[at+E.GENERATION])continue;
      if(core.sceneMotionEvents[at+E.KIND]===K.RELEASE)this.release(node.object);
      else this.drawer.historyReset(node.object);
    }
    this.drawer.historyEpoch=(this.drawer.historyEpoch||0)+1;
    let prepared=0;
    for(let i=0;i<count;i++){
      core.views();const proxy=registry.nodes[core.sceneOutput[i*Selected.STRIDE+Selected.SLOT]].object;
      if(this.drawer.historyPrepare(proxy,prepared))prepared++;
    }
    core.rust.motion_history_prepare_frame(prepared);
    return prepared;
  }

  // Commit follows the native complete-frame receipt, including empty frames.
  commit() {this.drawer.historyCommit();}

  release(proxy) {
    this.drawer.historyRelease(proxy);
    this.proxies.delete(proxy);
    this.scene.remove(proxy);
    delete proxy.userData.source.userData.motion;
  }

  // Draw the motion of everything tracked into `target` (cleared to zero:
  // static), then mark the first-person weapon as fixed on screen. `camera`
  // and `view` carry this frame's jitter, so proxies cover exactly the
  // pixels the scene drew; `current`/`previous` are the unjittered
  // view-projections the motion is measured in. The Rust renderer's motion
  // pass (rust-render/motion.js, `drawer`) must cover every tracked item.
  render(renderer, frame) {
    const target=frame.target,root=frame.root,camera=frame.camera,viewScene=frame.viewScene,view=frame.view,depth=frame.depth;
    shared.sceneDepth.value = depth;
    shared.depthRange.value.set(camera.near, camera.far);
    const prepared=this.prepare(root,camera);
    const drawn=prepared+this.drawer.core.sceneMotionEvents[H.EXTRAS];
    const locked=viewScene&&view?viewScene:null;
    const input=this.renderInput;input.target=target;input.camera=camera;input.viewScene=locked;input.view=view;input.depth=depth;
    if (!this.drawer.cover(renderer, this, input)) this.drawer.core.fail(new Error(`Incomplete Rust motion: ${this.drawer.why}`));

    return drawn;
  }


}

export const motionVectors = new MotionVectors();
