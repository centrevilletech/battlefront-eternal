import {sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import {Family,Mode,shareSceneWorld} from './rust-render/scene.js';
import {SceneMotionHeader as H,SceneMotionEvent as E,SceneSelection as Selected,SceneCapacity,SceneMotionKind as K} from './rust-render/scene-layout.js';
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
const TELEPORT = 8;

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
    this.skeletons = new Map();
    this.frame = 0;
    this.prepared=new Array(SceneCapacity.NODES).fill(null);this.preparedCount=0;
    this.at = new THREE.Vector3();
    this.was = new THREE.Vector3();
    this.nextOrder=0;
    // The Rust renderer's motion pass when it draws for this one (render).
    this.drawer = null;
  }

  // A mesh that moves by itself. `far` (m): beyond it from the camera its
  // few pixels rely on the upscaler's history clipping instead.
  track(source, { far = FAR } = {}) {
    if (!source?.userData.actorAsset || source.userData.motion) return null;
    const proxy=new THREE.Object3D();sceneProperty(proxy,"boundingSphere",source.boundingSphere);
    proxy.userData.order=this.nextOrder++;proxy.userData.previousModel=new THREE.Matrix4();proxy.userData.previousBindInverse=new THREE.Matrix4();
    if(source.skeleton){proxy.isSkinnedMesh=true;proxy.skeleton=source.skeleton;proxy.bindMatrix=source.bindMatrix;proxy.bindMatrixInverse=new THREE.Matrix4().copy(source.bindMatrixInverse);}
    sceneProperty(proxy,"matrixAutoUpdate",false);
    sceneProperty(proxy,"matrixWorldAutoUpdate",false);
    proxy.userData.source = source;
    proxy.userData.far = far;
    proxy.userData.seen = -1;
    proxy.userData.detached = 0;
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

  // The bones as they were last frame, in three's bone-texture layout (four
  // texels a bone, rows as wide as the texture).
  bones(skeleton) {
    const length = skeleton.bones.length*16;
    let entry = this.skeletons.get(skeleton);
    if (!entry || entry.length !== length) {
      const data=new Float32Array(length);entry={data,length,frame:-1};
      this.skeletons.set(skeleton, entry);
    }
    return entry;
  }

  // Before drawing: which proxies draw, where they are, and where they were.
  prepare(root, camera) {
    const core=this.drawer.core,registry=core.registry;
    const count=registry.select(this.scene,camera,Family.MOTION,Mode.MOTION_PREPARE,camera.layers.mask,0,root);
    core.views();const events=core.sceneMotionEvents[H.EVENTS];
    for(let i=0;i<events;i++){const at=H.STRIDE+i*E.STRIDE,node=registry.nodes[core.sceneMotionEvents[at+E.SLOT]];if(node?.generation!==core.sceneMotionEvents[at+E.GENERATION])continue;if(core.sceneMotionEvents[at+E.KIND]===K.RELEASE)this.release(node.object);else node.object.userData.seen=-1;}
    this.preparedCount=count;
    for(let i=0;i<count;i++){
      core.views();const proxy=registry.nodes[core.sceneOutput[i*Selected.STRIDE+Selected.SLOT]].object;this.prepared[i]=proxy;
      const source=proxy.userData.source;this.at.setFromMatrixPosition(source.matrixWorld);
      sceneTransform(proxy.matrixWorld.copy(source.matrixWorld));shareSceneWorld(proxy,source);
      const previous = proxy.userData.previousModel;
      // First drawn now, or moved too far to be motion (a respawn, a pooled
      // soldier's new life): no motion this frame.
      const fresh = proxy.userData.seen !== this.frame - 1
        || this.was.setFromMatrixPosition(previous).distanceToSquared(this.at) > TELEPORT * TELEPORT;
      if (fresh) previous.copy(source.matrixWorld);
      if (source.isSkinnedMesh) {
        proxy.bindMatrixInverse.copy(source.bindMatrixInverse);
        if (fresh) proxy.userData.previousBindInverse.copy(source.bindMatrixInverse);
        const entry = this.bones(source.skeleton);
        if (fresh || entry.frame !== this.frame - 1) {
          source.skeleton.update();
          entry.data.set(source.skeleton.actorPoseSkin||source.skeleton.boneMatrices);
        }
      }
    }
    return count;
  }

  // After drawing: this frame becomes the last one.
  commit() {
    for(let i=0;i<this.preparedCount;i++){
      const proxy=this.prepared[i];
      const source = proxy.userData.source;
      proxy.userData.previousModel.copy(source.matrixWorld);
      proxy.userData.seen = this.frame;
      if (source.isSkinnedMesh) {
        proxy.userData.previousBindInverse.copy(source.bindMatrixInverse);
        const entry = this.bones(source.skeleton);
        if (entry.frame !== this.frame) {
          entry.data.set(source.skeleton.actorPoseSkin||source.skeleton.boneMatrices);
          entry.frame = this.frame;
        }
      }
    }
    this.frame++;
  }

  release(proxy) {
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
  render(renderer, { target, root, camera, viewScene, view, depth, current, previous }) {
    shared.currentViewProjection.value.copy(current);
    shared.previousViewProjection.value.copy(previous);
    shared.sceneDepth.value = depth;
    shared.depthRange.value.set(camera.near, camera.far);
    const prepared=this.prepare(root,camera);
    const drawn=prepared+this.drawer.core.sceneMotionEvents[H.EXTRAS];
    const locked=viewScene&&view?viewScene:null;
    if (!this.drawer.cover(renderer, this, { target, camera, viewScene: locked, view, depth })) this.drawer.core.fail(new Error(`Incomplete Rust motion: ${this.drawer.why}`));

    return drawn;
  }


}

export const motionVectors = new MotionVectors();
