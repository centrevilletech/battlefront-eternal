import * as THREE from "three";
import { lighting } from "./materials.js";
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

const attributes = /* glsl */ `
  attribute vec2 corner;
  attribute vec3 iPosition;
  attribute vec4 iColor;
  attribute vec4 iShape; // half size, rotation, streak length, shape
  attribute vec3 iAxis; // streak direction, or the flat quad's first axis
  attribute vec3 iAxis2; // the flat quad's second axis; x is a streak's tail width
  attribute vec4 iFrame; // flipbook cell: uv offset, uv scale
`;

// Analytic effects share the exact quad placement, depth fade and blending
// with CPU particles. Only the source of their draw values differs.
export function particleVertexShader(inputs = attributes, initialize = "") {
  return /* glsl */ `
  ${inputs}
  varying vec2 vUv;
  varying vec4 vColor;
  varying float vDepth;
  varying float vSoftDistance;
  void main() {
    ${initialize}
    vColor = iColor;
    // Keep flat decals/shockwaves attached; soften volumetric sprites only.
    vSoftDistance = iShape.w > 1.5 ? 0.0 : clamp(iShape.x * 0.6, 0.04, 0.6);
    float size = iShape.x;
    vec2 uv = corner * 0.5 + 0.5;
    vec4 mv;
    if (iShape.w > 1.5) {
      vec3 p = iPosition + iAxis * (corner.x * size) + iAxis2 * (corner.y * size);
      mv = modelViewMatrix * vec4(p, 1.0);
    } else if (iShape.w > 0.5) {
      float head = corner.y * 0.5 + 0.5;
      vec3 dir = normalize(iAxis + vec3(0.0, 1e-6, 0.0));
      mv = modelViewMatrix * vec4(iPosition - dir * ((1.0 - head) * iShape.z), 1.0);
      vec3 along = normalize((modelViewMatrix * vec4(dir, 0.0)).xyz);
      vec3 side = normalize(cross(along, normalize(mv.xyz)) + vec3(1e-6, 0.0, 0.0));
      mv.xyz += side * corner.x * size * mix(iAxis2.x, 1.0, head);
      uv = vec2(1.0 - head, corner.x * 0.5 + 0.5);
    } else {
      mv = modelViewMatrix * vec4(iPosition, 1.0);
      float c = cos(iShape.y);
      float s = sin(iShape.y);
      mv.xy += vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y) * size;
    }
    vUv = iFrame.xy + uv * iFrame.zw;
    vDepth = -mv.z;
    gl_Position = projectionMatrix * mv;
  }
`;
}

export const particleFragmentShader = /* glsl */ `
  uniform sampler2D map;
  uniform vec2 fade;
  uniform sampler2D sceneDepth;
  uniform float softParticles;
  uniform vec2 depthResolution;
  uniform vec2 cameraRange;
  varying vec2 vUv;
  varying vec4 vColor;
  varying float vDepth;
  varying float vSoftDistance;
  void main() {
    vec4 color = texture2D(map, vUv) * vColor;
    #ifdef BOLT_CORE
      // The authored texture's own colour, as the stock game drew it, a
      // little brighter so thin shots survive minification and soft focus.
      // (A white-hot centre here made every bolt read white from afar.)
      color.rgb *= 1.4;
    #endif
    float near = clamp((fade.y - vDepth) / max(fade.y - fade.x, 1.0), 0.0, 1.0);
    color.a *= near * near;
    if (softParticles > 0.5 && vSoftDistance > 0.0) {
      float depth = texture2D(sceneDepth, gl_FragCoord.xy / depthResolution).x;
      float surface = cameraRange.x * cameraRange.y / (cameraRange.y - depth * (cameraRange.y - cameraRange.x));
      color.a *= smoothstep(0.0, vSoftDistance, surface - vDepth);
    }
    if (color.a < 0.002) discard;
    gl_FragColor = color;
  }
`;

const ATTRIBUTES = { iPosition: 3, iColor: 4, iShape: 4, iAxis: 3, iAxis2: 3, iFrame: 4 };

class Batch {
  constructor(texture, blending, profile = "particle", capacity = 256) {
    this.count = 0;
    this.capacity = 0;
    this.arrays = {};
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.setAttribute("corner", new THREE.BufferAttribute(QUAD, 2));
    this.geometry.setIndex(QUAD_INDEX);
    this.material = new THREE.ShaderMaterial({
      uniforms: {
        map: { value: texture }, fade: lighting.fade,
        sceneDepth: particleDepth.map, softParticles: particleDepth.enabled,
        depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
      },
      vertexShader: particleVertexShader(),
      fragmentShader: particleFragmentShader,
      defines: profile === "bolt" ? { BOLT_CORE: 1 } : {},
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

export class ParticleRenderer {
  constructor(scene) {
    this.scene = scene;
    this.batches = new Map();
    this.lookup = new Map();
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
