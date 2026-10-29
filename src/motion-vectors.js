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

const vertexShader = /* glsl */ `
  #include <common>
  #include <skinning_pars_vertex>
  uniform mat4 currentViewProjection;
  uniform mat4 previousViewProjection;
  uniform mat4 previousModel;
  #ifdef USE_SKINNING
    uniform mat4 previousBindInverse;
    uniform highp sampler2D previousBones;
    mat4 previousBone(const in float i) {
      int size = textureSize(previousBones, 0).x;
      int j = int(i) * 4;
      int x = j % size, y = j / size;
      return mat4(texelFetch(previousBones, ivec2(x, y), 0), texelFetch(previousBones, ivec2(x + 1, y), 0),
        texelFetch(previousBones, ivec2(x + 2, y), 0), texelFetch(previousBones, ivec2(x + 3, y), 0));
    }
  #endif
  varying vec4 vCurrent;
  varying vec4 vPrevious;
  varying float vDepth;
  void main() {
    #include <skinbase_vertex>
    #include <begin_vertex>
    #include <skinning_vertex>
    #include <project_vertex>
    vec4 before = vec4(position, 1.0);
    #ifdef USE_SKINNING
      // Last frame's bones, and last frame's bind inverse: an attached
      // skeleton's is the inverse of the mesh's own placement then.
      vec4 bound = bindMatrix * before;
      vec4 moved = previousBone(skinIndex.x) * bound * skinWeight.x + previousBone(skinIndex.y) * bound * skinWeight.y
        + previousBone(skinIndex.z) * bound * skinWeight.z + previousBone(skinIndex.w) * bound * skinWeight.w;
      before = previousBindInverse * moved;
    #endif
    vCurrent = currentViewProjection * modelMatrix * vec4(transformed, 1.0);
    vPrevious = previousViewProjection * previousModel * before;
    vDepth = -mvPosition.z;
  }
`;

const fragmentShader = /* glsl */ `
  uniform highp sampler2D sceneDepth;
  uniform vec2 depthRange;
  varying vec4 vCurrent;
  varying vec4 vPrevious;
  varying float vDepth;
  void main() {
    float stored = texelFetch(sceneDepth, ivec2(gl_FragCoord.xy), 0).r;
    float near = depthRange.x, far = depthRange.y;
    float surface = near * far / (far - stored * (far - near));
    if (abs(surface - vDepth) > 0.04 + 0.01 * vDepth) discard;
    vec2 now = vCurrent.xy / vCurrent.w, then = vPrevious.xy / vPrevious.w;
    gl_FragColor = vec4((now - then) * 0.5, 0.0, 0.5);
  }
`;

// The first-person weapon is drawn in a view of its own and moves with the
// eye: it keeps its place on screen.
const lockedVertexShader = /* glsl */ `
  #include <common>
  #include <skinning_pars_vertex>
  void main() {
    #include <skinbase_vertex>
    #include <begin_vertex>
    #include <skinning_vertex>
    #include <project_vertex>
  }
`;

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
    this.locked = new THREE.ShaderMaterial({
      vertexShader: lockedVertexShader,
      fragmentShader: "void main() { gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0); }",
      depthTest: false, depthWrite: false, side: THREE.DoubleSide,
    });
    this.clearColor = new THREE.Color();
  }

  // A mesh that moves by itself. `far` (m): beyond it from the camera its
  // few pixels rely on the upscaler's history clipping instead.
  track(source, { far = FAR } = {}) {
    if (!source?.isMesh || source.userData.motion) return null;
    const geometry = new THREE.BufferGeometry();
    geometry.index = source.geometry.index;
    geometry.attributes = source.geometry.attributes;
    geometry.boundingSphere = source.geometry.boundingSphere;
    const material = new THREE.ShaderMaterial({
      vertexShader, fragmentShader,
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
  // view-projections the motion is measured in.
  render(renderer, { target, root, camera, viewScene, view, depth, current, previous }) {
    shared.currentViewProjection.value.copy(current);
    shared.previousViewProjection.value.copy(previous);
    shared.sceneDepth.value = depth;
    shared.depthRange.value.set(camera.near, camera.far);
    let drawn = this.prepare(root, camera);
    for (const extra of this.extras) if (extra.visible) drawn++;
    const autoClear = renderer.autoClear, shadows = renderer.shadowMap.autoUpdate, alpha = renderer.getClearAlpha();
    renderer.getClearColor(this.clearColor);
    renderer.shadowMap.autoUpdate = false;
    renderer.setRenderTarget(target);
    renderer.setClearColor(0x000000, 0);
    renderer.clear(true, false, false);
    renderer.autoClear = false;
    if (drawn) renderer.render(this.scene, camera);
    if (viewScene && view && viewScene.children.some((child) => child.visible && !child.isLight)) {
      viewScene.overrideMaterial = this.locked;
      renderer.render(viewScene, view);
      viewScene.overrideMaterial = null;
    }
    renderer.setClearColor(this.clearColor, alpha);
    renderer.autoClear = autoClear;
    renderer.shadowMap.autoUpdate = shadows;
    this.commit();
    return drawn;
  }
}

export const motionVectors = new MotionVectors();
