import * as THREE from "three";
import { ownedEffectMaterial } from "./rust-render/effect-material.js";

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
    this.scene.matrixWorldAutoUpdate = false;
    this.proxies = new Set();
    this.extras = new Set();
    this.skeletons = new Map();
    this.frame = 0;
    this.eye = new THREE.Vector3();
    this.at = new THREE.Vector3();
    this.was = new THREE.Vector3();
    this.locked = ownedEffectMaterial("effects/motion-locked", {
      depthTest: false, depthWrite: false, side: THREE.DoubleSide,
    });
    // The Rust renderer's motion pass when it draws for this one (render).
    this.drawer = null;
  }

  // A mesh that moves by itself. `far` (m): beyond it from the camera its
  // few pixels rely on the upscaler's history clipping instead.
  track(source, { far = FAR } = {}) {
    if (!source?.isMesh || source.userData.motion) return null;
    const geometry = new THREE.BufferGeometry();
    geometry.index = source.geometry.index;
    geometry.attributes = source.geometry.attributes;
    geometry.boundingSphere = source.geometry.boundingSphere;
    const material = ownedEffectMaterial(source.isSkinnedMesh ? "effects/motion-skinned" : "effects/motion-rigid", {
      uniforms: {
        ...shared, previousModel: { value: new THREE.Matrix4() }, previousBindInverse: { value: new THREE.Matrix4() },
        previousBones: { value: null },
      },
      depthTest: false, depthWrite: false, side: THREE.DoubleSide,
    });
    const proxy = source.isSkinnedMesh ? new THREE.SkinnedMesh(geometry, material) : new THREE.Mesh(geometry, material);
    if (source.isSkinnedMesh) {
      proxy.bind(source.skeleton, source.bindMatrix);
      proxy.boundingSphere = source.boundingSphere;
    }
    proxy.matrixAutoUpdate = false;
    proxy.matrixWorldAutoUpdate = false;
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
    object.matrixAutoUpdate = false;
    this.extras.add(object);
    this.scene.add(object);
  }

  removeExtra(object) {
    this.extras.delete(object);
    this.scene.remove(object);
  }

  // Every mesh of a model.
  trackAll(object, options) {
    object.traverse((child) => { if (child.isMesh) this.track(child, options); });
  }

  // Drawn by the main camera this frame: shown all the way up to the scene,
  // and drawn on its default layer (a first-person body is not).
  shown(source, root) {
    if (!source.layers.isEnabled(0)) return false;
    for (let object = source; object; object = object.parent) {
      if (!object.visible) return false;
      if (object === root) return true;
    }
    return null;
  }

  // The bones as they were last frame, in three's bone-texture layout (four
  // texels a bone, rows as wide as the texture).
  bones(skeleton) {
    const length = skeleton.boneMatrices.length;
    let entry = this.skeletons.get(skeleton);
    if (!entry || entry.length !== length) {
      entry?.texture.dispose();
      const texels = Math.ceil(length / 4);
      const width = Math.max(4, Math.ceil(Math.sqrt(texels)));
      const height = Math.ceil(texels / width);
      const data = new Float32Array(width * height * 4);
      const texture = new THREE.DataTexture(data, width, height, THREE.RGBAFormat, THREE.FloatType);
      entry = { data, texture, length, frame: -1 };
      this.skeletons.set(skeleton, entry);
    }
    return entry;
  }

  // Before drawing: which proxies draw, where they are, and where they were.
  prepare(root, camera) {
    this.eye.setFromMatrixPosition(camera.matrixWorld);
    let drawn = 0;
    for (const proxy of this.proxies) {
      const source = proxy.userData.source;
      const shown = this.shown(source, root);
      if (shown === null) {
        // Detached: a soldier's pooled model is reattached later; one gone
        // for good is let go.
        proxy.visible = false;
        if (++proxy.userData.detached > 600) this.release(proxy);
        continue;
      }
      proxy.userData.detached = 0;
      const far = proxy.userData.far;
      proxy.visible = shown && this.at.setFromMatrixPosition(source.matrixWorld).distanceToSquared(this.eye) < far * far;
      if (!proxy.visible) { proxy.userData.seen = -1; continue; }
      drawn++;
      proxy.matrixWorld.copy(source.matrixWorld);
      const previous = proxy.material.uniforms.previousModel.value;
      // First drawn now, or moved too far to be motion (a respawn, a pooled
      // soldier's new life): no motion this frame.
      const fresh = proxy.userData.seen !== this.frame - 1
        || this.was.setFromMatrixPosition(previous).distanceToSquared(this.at) > TELEPORT * TELEPORT;
      if (fresh) previous.copy(source.matrixWorld);
      if (source.isSkinnedMesh) {
        proxy.bindMatrixInverse.copy(source.bindMatrixInverse);
        if (fresh) proxy.material.uniforms.previousBindInverse.value.copy(source.bindMatrixInverse);
        const entry = this.bones(source.skeleton);
        proxy.material.uniforms.previousBones.value = entry.texture;
        if (fresh || entry.frame !== this.frame - 1) {
          source.skeleton.update();
          entry.data.set(source.skeleton.boneMatrices);
          entry.texture.needsUpdate = true;
        }
      }
    }
    return drawn;
  }

  // After drawing: this frame becomes the last one.
  commit() {
    for (const proxy of this.proxies) {
      if (!proxy.visible) continue;
      const source = proxy.userData.source;
      proxy.material.uniforms.previousModel.value.copy(source.matrixWorld);
      proxy.userData.seen = this.frame;
      if (source.isSkinnedMesh) {
        proxy.material.uniforms.previousBindInverse.value.copy(source.bindMatrixInverse);
        const entry = this.bones(source.skeleton);
        if (entry.frame !== this.frame) {
          entry.data.set(source.skeleton.boneMatrices);
          entry.texture.needsUpdate = true;
          entry.frame = this.frame;
        }
      }
    }
    this.frame++;
  }

  release(proxy) {
    this.proxies.delete(proxy);
    this.scene.remove(proxy);
    proxy.material.dispose();
    // The buffers are the source's: release only this wrapper's bindings.
    proxy.geometry.index = null;
    proxy.geometry.attributes = {};
    proxy.geometry.dispose();
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
    let drawn = this.prepare(root, camera);
    for (const extra of this.extras) if (extra.visible) drawn++;
    // The view scene to mark fixed, when it shows anything.
    const locked = viewScene && view && viewScene.children.some((child) => child.visible && !child.isLight) ? viewScene : null;
    if (!this.drawer.cover(renderer, this, { target, camera, viewScene: locked, view, depth })) this.drawer.core.fail(new Error(`Incomplete Rust motion: ${this.drawer.why}`));
    this.commit();
    return drawn;
  }


}

export const motionVectors = new MotionVectors();
