import * as THREE from "three";
import { lighting } from "./materials.js";
import { ownedEffectMaterial } from "./rust-render/effect-material.js";
import { particleDepth } from "./particle-depth.js";

// Batched particle drawing. Each batch is one texture and one blend mode, drawn
// as instanced quads of three shapes:
//
//   billboard  faces the camera, turned by its rotation
//   streak     runs back from its head along a direction, facing the camera
//              across it (laser bolts, sparks); u runs head to tail, and the
//              width can taper toward the tail
//   flat       lies along two world axes (shockwaves, decals)
//
// Each quad can show one cell of a flipbook texture. Like the near scene, the
// particles fade out between the fade start and end. The particles' own
// behavior lives in the effect runtime; this only draws what it is handed.

export const BILLBOARD = 0;
export const STREAK = 1;
export const FLAT = 2;

const QUAD = new Float32Array([-1, -1, 1, -1, 1, 1, -1, 1]);
const QUAD_INDEX = [0, 1, 2, 0, 2, 3];

const ATTRIBUTES = { iPosition: 3, iColor: 4, iShape: 4, iAxis: 3, iAxis2: 3, iFrame: 4 };

class Batch {
  constructor(texture, blending, profile = "particle", capacity = 256) {
    this.count = 0;
    this.capacity = 0;
    this.arrays = {};
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.setAttribute("corner", new THREE.BufferAttribute(QUAD, 2));
    this.geometry.setIndex(QUAD_INDEX);
    this.material = ownedEffectMaterial(profile === "bolt" ? "effects/particle-bolt" : "effects/particle-cpu", {
      uniforms: {
        map: { value: texture }, fade: lighting.fade,
        sceneDepth: particleDepth.map, softParticles: particleDepth.enabled,
        depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
      },
      transparent: true,
      depthWrite: false,
      blending: blending === "additive" ? THREE.AdditiveBlending : THREE.NormalBlending,
      // A streak's quad winds either way round depending on its direction.
      side: THREE.DoubleSide,
      forceSinglePass: true,
    });
    this.mesh = new THREE.Mesh(this.geometry, this.material);
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = 10;
    // Only the first three arguments: the fourth is the geometry, which
    // capture() would read as "required" and blit depth even when disabled.
    this.mesh.onBeforeRender = (renderer, scene, camera) => particleDepth.capture(renderer, scene, camera);
    this.grow(capacity);
  }

  grow(capacity) {
    for (const [name, size] of Object.entries(ATTRIBUTES)) {
      const array = new Float32Array(capacity * size);
      if (this.arrays[name]) array.set(this.arrays[name]);
      this.arrays[name] = array;
      this.geometry.setAttribute(name, new THREE.InstancedBufferAttribute(array, size).setUsage(THREE.DynamicDrawUsage));
    }
    // three keeps the instance limit of the buffers it first bound until the
    // geometry is disposed: grown, the new instances would not be drawn.
    delete this.geometry._maxInstanceCount;
    this.capacity = capacity;
  }

  begin() {
    this.count = 0;
  }

  // One quad. `shape` is BILLBOARD, STREAK, or FLAT; `axis` and `axis2` are
  // arrays of three (or null); `frame` is [u, v, du, dv] (or null for the
  // whole texture). A streak's axis2[0] is its tail's share of the width.
  push(x, y, z, r, g, b, a, size, rotation = 0, shape = BILLBOARD, length = 0, axis = null, axis2 = null, frame = null) {
    if (this.count === this.capacity) this.grow(this.capacity * 2);
    const i = this.count++;
    const { iPosition, iColor, iShape, iAxis, iAxis2, iFrame } = this.arrays;
    iPosition[i * 3] = x;
    iPosition[i * 3 + 1] = y;
    iPosition[i * 3 + 2] = z;
    iColor[i * 4] = r;
    iColor[i * 4 + 1] = g;
    iColor[i * 4 + 2] = b;
    iColor[i * 4 + 3] = a;
    iShape[i * 4] = size;
    iShape[i * 4 + 1] = rotation;
    iShape[i * 4 + 2] = length;
    iShape[i * 4 + 3] = shape;
    iAxis[i * 3] = axis ? axis[0] : 0;
    iAxis[i * 3 + 1] = axis ? axis[1] : 0;
    iAxis[i * 3 + 2] = axis ? axis[2] : 1;
    iAxis2[i * 3] = axis2 ? axis2[0] : 1;
    iAxis2[i * 3 + 1] = axis2 ? axis2[1] : 0;
    iAxis2[i * 3 + 2] = axis2 ? axis2[2] : 0;
    iFrame[i * 4] = frame ? frame[0] : 0;
    iFrame[i * 4 + 1] = frame ? frame[1] : 0;
    iFrame[i * 4 + 2] = frame ? frame[2] : 1;
    iFrame[i * 4 + 3] = frame ? frame[3] : 1;
  }

  end() {
    this.geometry.instanceCount = this.count;
    // A batch another renderer draws (external: src/rust-render/effects.js
    // reads its arrays) is shown to three never, and uploads it nothing.
    this.mesh.visible = this.count > 0 && !this.external;
    if (!this.count || this.external) return;
    for (const name of Object.keys(ATTRIBUTES)) {
      const attribute = this.geometry.getAttribute(name);
      attribute.clearUpdateRanges();
      attribute.addUpdateRange(0, this.count * attribute.itemSize);
      attribute.needsUpdate = true;
    }
  }
}

// The particle renderers drawing into each scene (keyed by the scene at the
// root of theirs), for a renderer that draws their batches itself
// (src/rust-render/effects.js).
export const particleRenderers = new WeakMap();

export class ParticleRenderer {
  constructor(scene) {
    this.scene = scene;
    this.batches = new Map();
    this.lookup = new Map();
    let root = scene;
    while (root.parent) root = root.parent;
    if (!particleRenderers.has(root)) particleRenderers.set(root, []);
    particleRenderers.get(root).push(this);
  }

  // The batch for a texture and blend mode ("additive" or "normal").
  // Looked up per particle per frame: nested maps, no key strings.
  batch(texture, blending = "additive", profile = "particle") {
    let byBlend = this.lookup.get(texture);
    if (!byBlend) this.lookup.set(texture, (byBlend = new Map()));
    let byProfile = byBlend.get(blending);
    if (!byProfile) byBlend.set(blending, (byProfile = new Map()));
    let batch = byProfile.get(profile);
    if (!batch) {
      batch = new Batch(texture, blending, profile);
      byProfile.set(profile, batch);
      this.batches.set(`${texture.uuid}:${blending}:${profile}`, batch);
      this.scene.add(batch.mesh);
    }
    return batch;
  }

  // A batch of another kind with the same begin/push/end life and a `mesh`
  // (blaster-bolts.js), kept with the rest so it is reset and uploaded with them.
  custom(key, make) {
    let batch = this.batches.get(key);
    if (!batch) {
      batch = make();
      this.batches.set(key, batch);
      this.scene.add(batch.mesh);
    }
    return batch;
  }

  begin() {
    for (const batch of this.batches.values()) batch.begin();
  }

  end() {
    for (const batch of this.batches.values()) batch.end();
  }
}
