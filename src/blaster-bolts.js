import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from "three";
import { texturePixels } from "./texture-pixels.js";
import { lighting } from "./materials.js";
import { particleDepth } from "./particle-depth.js";

// Enhanced blaster bolts: each one is a glowing capsule of plasma between its
// head and tail, shaded per pixel from the closest distance between the view
// ray and that segment. A white-hot core sits inside a saturated sheath and a
// wide soft halo, so a bolt reads the same side-on, end-on and in the
// distance, where its width is held to about a pixel and its brightness
// thinned to match. The 2004 textured strips remain the Classic look
// (ordnance.js); the game's own sizes and colors drive both.
//
// One instanced draw for every bolt and beam. The quad is fitted on the image
// plane around both projected end discs, so it costs little more than the
// bolt's own outline; fragments write the depth of their closest approach,
// so walls hide the part of a bolt behind them.

export const boltLook = { enhanced: false };

const QUAD = new Float32Array([-1, -1, 1, -1, 1, 1, -1, 1]);
const QUAD_INDEX = [0, 1, 2, 0, 2, 3];
const ATTRIBUTES = { iHead: 3, iTail: 3, iColor: 4, iSize: 4 };





// A ParticleRenderer batch of its own kind (see ParticleRenderer.custom).
export class BlasterBatch {
  constructor(capacity = 64) {
    this.count = 0;
    this.capacity = 0;
    this.arrays = {};
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.setAttribute("corner", new THREE.BufferAttribute(QUAD, 2));
    this.geometry.setIndex(QUAD_INDEX);
    this.material = ownedEffectMaterial('effects/blaster', {
      uniforms: {
        fade: lighting.fade, time: lighting.time, pixelAngle: { value: 0.002 },
        sceneDepth: particleDepth.map, softParticles: particleDepth.enabled,
        depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
      },
      transparent: true,
      depthWrite: false,
      blending: THREE.CustomBlending,
      blendEquation: THREE.AddEquation,
      blendSrc: THREE.OneFactor,
      blendDst: THREE.OneMinusSrcAlphaFactor,
      side: THREE.DoubleSide,
      forceSinglePass: true,
    });
    this.mesh = new THREE.Mesh(this.geometry, this.material);
    this.mesh.name = "blaster-bolts";
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = 11;
    const size = new THREE.Vector2();
    this.mesh.onBeforeRender = (renderer, scene, camera) => {
      particleDepth.capture(renderer, scene, camera);
      if (camera.isPerspectiveCamera) {
        // The pixels being drawn: under an upscaler, the 3D frame's.
        const target = renderer.getRenderTarget();
        const height = target ? target.height : renderer.getDrawingBufferSize(size).y;
        this.material.uniforms.pixelAngle.value = (2 * Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2)) / Math.max(height, 1) / (camera.zoom || 1);
      }
    };
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

  // One bolt from its head back to its tail. `rgb` is the sheath colour with
  // its largest channel at 1; `brightness` scales the whole bolt.
  push(head, tail, rgb, brightness, core, glow, seed = 0, taper = 0.6) {
    if (this.count === this.capacity) this.grow(this.capacity * 2);
    const i = this.count++;
    const { iHead, iTail, iColor, iSize } = this.arrays;
    iHead[i * 3] = head.x; iHead[i * 3 + 1] = head.y; iHead[i * 3 + 2] = head.z;
    iTail[i * 3] = tail.x; iTail[i * 3 + 1] = tail.y; iTail[i * 3 + 2] = tail.z;
    iColor[i * 4] = rgb[0]; iColor[i * 4 + 1] = rgb[1]; iColor[i * 4 + 2] = rgb[2]; iColor[i * 4 + 3] = brightness;
    iSize[i * 4] = core; iSize[i * 4 + 1] = glow; iSize[i * 4 + 2] = seed; iSize[i * 4 + 3] = taper;
  }

  // `count` bolts the ordnance phase drew (head, tail, colour, size arrays),
  // appended as push() would.
  load(source, count) {
    while (this.count + count > this.capacity) this.grow(this.capacity * 2);
    const names = Object.keys(ATTRIBUTES);
    for (let k = 0; k < names.length; k++) {
      const size = ATTRIBUTES[names[k]];
      this.arrays[names[k]].set(source[k].subarray(0, count * size), this.count * size);
    }
    this.count += count;
  }

  end() {
    this.geometry.instanceCount = this.count;
    // Drawn by another renderer (external: src/rust-render/effects.js reads
    // its arrays), it is shown to three never and uploads it nothing.
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

// The sheath colour of a LaserGlowColor: its hue at full strength, so a dim
// authored alpha doesn't make the bolt grey.
export function sheathColor(rgba = [255, 255, 255, 255]) {
  const peak = Math.max(rgba[0], rgba[1], rgba[2], 1);
  return [rgba[0] / peak, rgba[1] / peak, rgba[2] / peak];
}

// Deeper, film-like saturation for a sheath colour; the strongest channel
// stays at 1, so the hue is kept and pale textures still read as coloured.
export function vivid(rgb, power = 1.6) {
  return rgb.map((c) => Math.pow(Math.max(c, 0), power));
}

// The hue a laser texture shows: guns without a glow showed only their core
// texture, so its colour is what the bolt looked like. Texels are weighted by
// saturation, which skips the near-white middle and the dark fringe.
// Undefined until the pixels are readable; null when the texture is grey.
const hues = new WeakMap();
export function textureHue(texture) {
  if (hues.has(texture)) return hues.get(texture);
  const pixels = texturePixels(texture);
  if (!pixels) return undefined;
  const data = pixels.data;
  const sum = [0, 0, 0];
  let weight = 0;
  for (let i = 0; i < data.length; i += 4) {
    const peak = Math.max(data[i], data[i + 1], data[i + 2]);
    if (peak < 48 || data[i + 3] < 32) continue;
    const saturation = 1 - Math.min(data[i], data[i + 1], data[i + 2]) / peak, w = saturation * saturation * peak;
    sum[0] += (data[i] / peak) * w; sum[1] += (data[i + 1] / peak) * w; sum[2] += (data[i + 2] / peak) * w;
    weight += w;
  }
  const hue = weight > 1 ? sheathColor(sum) : null;
  hues.set(texture, hue);
  return hue;
}
