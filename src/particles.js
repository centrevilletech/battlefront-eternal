import {sceneProperty} from "./rust-render/scene-changes.js";
import * as THREE from "three";
import { EFFECT } from "./rust-render/effect-layout.js";
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

const ATTRIBUTES = { iPosition: 3, iColor: 4, iShape: 4, iAxis: 3, iAxis2: 3, iFrame: 4 };

const ATTRIBUTE_NAMES=Object.keys(ATTRIBUTES);

class Batch {
  constructor(texture, blending, profile = "particle", capacity = 256) {
    this.count = 0;
    this.capacity = 0;
    this.arrays = {};
    // Batch's authored profile and blend are cold facts. Rust owns its
    // geometry and material; the anchor carries only scene order and layers.
    this.nativeKind = EFFECT.CPU.ID;
    this.variant = (blending === "additive" ? 1 : 0) | (profile === "bolt" ? 2 : 0);
    this.texture = texture;
    this.values = new Float64Array(EFFECT.CPU.LENGTH);
    this.mesh = new THREE.Object3D();
    sceneProperty(this.mesh,"frustumCulled",false);
    sceneProperty(this.mesh,"renderOrder",10);
    // Only the first three arguments: the fourth is the geometry, which
    // capture() would read as "required" and blit depth even when disabled.
    this.mesh.onBeforeRender = (renderer, scene, camera) => particleDepth.capture(renderer, scene, camera);
    this.grow(Math.max(capacity, 16384));
  }

  grow(capacity) {
    for (const [name, size] of Object.entries(ATTRIBUTES)) {
      const array = new Float32Array(capacity * size);
      if (this.arrays[name]) array.set(this.arrays[name]);
      this.arrays[name] = array;
    }
    this.capacity = capacity;
  }

  begin() {
    this.count = 0;
  }

  // One quad. `shape` is BILLBOARD, STREAK, or FLAT; `axis` and `axis2` are
  // arrays of three (or null); `frame` is [u, v, du, dv] (or null for the
  // whole texture). A streak's axis2[0] is its tail's share of the width.
  push(x, y, z, r, g, b, a, size, rotation = 0, shape = BILLBOARD, length = 0, axis = null, axis2 = null, frame = null) {
    if(this.count===this.capacity){this.dropped=(this.dropped||0)+1;return;}
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

  // `count` quads the ordnance phase drew, from one batch's region of its
  // output (attribute arrays in ATTRIBUTES order), appended as push() would.
  load(source, from, count) {
    const available=Math.min(count,this.capacity-this.count);
    this.dropped=(this.dropped||0)+count-available;count=available;
    const names = ATTRIBUTE_NAMES;
    for (let k = 0; k < names.length; k++) {
      const size = ATTRIBUTES[names[k]];
      this.arrays[names[k]].set(source[k].subarray(from * size, (from + count) * size), this.count * size);
    }
    this.count += count;
  }

  // The renderer reads the active prefix directly. No attribute versions or
  // secondary Three upload ranges are published by Batch.end.
  end() {
    sceneProperty(this.mesh,"visible",this.count > 0);
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
