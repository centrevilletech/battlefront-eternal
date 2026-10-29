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

const vertexShader = /* glsl */ `
  attribute vec2 corner;
  attribute vec3 iHead;
  attribute vec3 iTail;
  attribute vec4 iColor; // rgb sheath colour (max channel 1), a brightness
  attribute vec4 iSize;  // core radius, glow radius, seed, tail taper
  uniform float pixelAngle; // image-plane units per pixel at unit depth
  varying vec2 vPlane;
  varying vec3 vHead;
  varying vec3 vTail;
  varying vec4 vColor;
  varying vec4 vSize;
  varying float vEnergy;
  varying float vCoreShare;
  void main() {
    vec3 h = (modelViewMatrix * vec4(iHead, 1.0)).xyz;
    vec3 t = (modelViewMatrix * vec4(iTail, 1.0)).xyz;
    const float NEAR = 0.06;
    if (h.z > -NEAR && t.z > -NEAR) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
    if (h.z > -NEAR) h = mix(t, h, (t.z + NEAR) / (t.z - h.z));
    if (t.z > -NEAR) t = mix(h, t, (h.z + NEAR) / (h.z - t.z));
    // Hold the halo to a couple of pixels far away and thin its brightness to
    // match, so a distant shot stays visible without growing brighter.
    float nearest = min(-h.z, -t.z);
    float glow = max(iSize.y, nearest * pixelAngle * 2.5);
    float core = max(iSize.x, nearest * pixelAngle * 0.75);
    vEnergy = clamp(iSize.y / glow, 0.08, 1.0);
    vCoreShare = clamp(iSize.x / core, 0.0, 1.0);
    vec2 hp = h.xy / -h.z, tp = t.xy / -t.z;
    float reach = glow * 3.2;
    float rh = reach / -h.z, rt = reach / -t.z, r = max(rh, rt);
    vec2 dir = tp - hp;
    float len = length(dir);
    dir = len > 1e-7 ? dir / len : vec2(1.0, 0.0);
    vec2 across = vec2(-dir.y, dir.x);
    float along = corner.x * 0.5 + 0.5;
    vec2 p = mix(hp - dir * rh, tp + dir * rt, along) + across * r * corner.y;
    vPlane = p;
    vHead = h;
    vTail = t;
    vColor = iColor;
    vSize = vec4(core, glow, iSize.z, iSize.w);
    // All four corners share one depth, so the image-plane position
    // interpolates linearly across the quad.
    float depth = nearest;
    gl_Position = projectionMatrix * vec4(p * depth, -depth, 1.0);
  }
`;

const fragmentShader = /* glsl */ `
  uniform mat4 projectionMatrix;
  uniform vec2 fade;
  uniform float time;
  uniform sampler2D sceneDepth;
  uniform float softParticles;
  uniform vec2 depthResolution;
  uniform vec2 cameraRange;
  varying vec2 vPlane;
  varying vec3 vHead;
  varying vec3 vTail;
  varying vec4 vColor;
  varying vec4 vSize;
  varying float vEnergy;
  varying float vCoreShare;
  void main() {
    vec3 ray = normalize(vec3(vPlane, -1.0));
    vec3 span = vTail - vHead;
    float rv = dot(ray, span), rh = dot(ray, vHead), vv = dot(span, span);
    float den = vv - rv * rv;
    float s = den > 1e-6 ? clamp((rv * rh - dot(span, vHead)) / den, 0.0, 1.0) : 0.0;
    vec3 closest = vHead + span * s;
    float along = dot(ray, closest);
    if (along <= 0.0) discard;
    float d2 = max(dot(closest, closest) - along * along, 0.0);
    float d = sqrt(d2);
    // The tail thins and dims, as if the bolt were smeared by its speed.
    float core = vSize.x * mix(1.0, vSize.w, s);
    float glow = vSize.y * mix(1.0, 0.7, s);
    float body = mix(1.0, 0.3, s * s);
    float axial = vv > 1e-8 ? abs(rv) / sqrt(vv) : 1.0;
    // A core widened to stay a pixel wide far away would turn a distant bolt
    // into a white line; fade it in proportion so the colour carries.
    float hot = (1.0 - smoothstep(core * 0.5, core * 1.15, d)) * vCoreShare;
    float sheath = exp(-d2 / (glow * glow) * 2.6) * (1.0 + 0.5 * axial);
    float halo = exp(-d / glow * 2.2) * 0.3;
    float flicker = 0.93 + 0.07 * sin(time * 97.0 + vSize.z * 40.0);
    // Light adds up as energy and saturates per channel: the core goes white
    // while the sheath around it keeps the bolt's hue instead of clipping.
    vec3 energy = mix(vColor.rgb, vec3(1.0), 0.75) * hot * 2.0 + vColor.rgb * (sheath * 1.6 + halo);
    vec3 color = 1.0 - exp(-energy * vColor.a * body * flicker * vEnergy);
    vec3 point = ray * along;
    float depth = -point.z;
    float near = clamp((fade.y - depth) / max(fade.y - fade.x, 1.0), 0.0, 1.0);
    color *= near * near;
    if (softParticles > 0.5) {
      float stored = texture2D(sceneDepth, gl_FragCoord.xy / depthResolution).x;
      float surface = cameraRange.x * cameraRange.y / (cameraRange.y - stored * (cameraRange.y - cameraRange.x));
      color *= smoothstep(0.0, max(glow * 0.8, 0.05), surface - depth);
    }
    if (max(color.r, max(color.g, color.b)) < 0.002) discard;
    vec4 clip = projectionMatrix * vec4(point, 1.0);
    gl_FragDepth = clamp(clip.z / clip.w * 0.5 + 0.5, 0.0, 1.0);
    // Premultiplied: the light adds, and the sheath also covers part of what
    // is behind it, so a bolt keeps its hue in front of bright ground or sky.
    gl_FragColor = vec4(color, max(color.r, max(color.g, color.b)) * 0.55);
  }
`;

// A ParticleRenderer batch of its own kind (see ParticleRenderer.custom).
export class BlasterBatch {
  constructor(capacity = 64) {
    this.count = 0;
    this.capacity = 0;
    this.arrays = {};
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.setAttribute("corner", new THREE.BufferAttribute(QUAD, 2));
    this.geometry.setIndex(QUAD_INDEX);
    this.material = new THREE.ShaderMaterial({
      uniforms: {
        fade: lighting.fade, time: lighting.time, pixelAngle: { value: 0.002 },
        sceneDepth: particleDepth.map, softParticles: particleDepth.enabled,
        depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
      },
      vertexShader,
      fragmentShader,
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
    this.mesh.onBeforeRender = (renderer, scene, camera, ...rest) => {
      particleDepth.capture(renderer, scene, camera, ...rest);
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

  end() {
    this.geometry.instanceCount = this.count;
    this.mesh.visible = this.count > 0;
    if (!this.count) return;
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
