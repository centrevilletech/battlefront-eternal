import * as THREE from "three";
import { lighting } from "./materials.js";
import { particleDepth } from "./particle-depth.js";

// The particles and electric arcs of the Enhanced destruction effects
// (src/destruction.js): droids shorting out and blowing apart, and wrecked
// power-up droids smoking.
//
// Two fixed pools of instanced quads, each one draw:
//
//   hot    additive light: sparks (streaks along their flight that skip off
//          the floor), fire, flashes, the glow a blast throws on the floor,
//          and embers
//   smoke  alpha-blended: smoke lit by the map's sun and ambient and drifting
//          with the wind, flakes of dark plating, and scorch marks
//
// A particle is written once at its birth (where, when, how fast, how big)
// and the vertex shader works out where it is from its age: drag, gravity or
// buoyancy, expansion, the wind. The CPU only retires dead ones, so a frame
// uploads what was born or moved in the pool, nothing per live particle.
//
// Smoke and fire are drawn without textures: a ball eaten into by four
// octaves of noise, the noise's slope bending the ball's normal so the sun
// picks out billows. Fire is coloured by how dense and how young it is, from
// white-yellow through orange to a dull red.
//
// Arcs are jagged ribbons between two points, each end fixed in the world or
// carried by an object (a bone, a flying piece). Their shape is redrawn 30
// times a second with a new random jag, which is what makes them crackle.
// One dynamic buffer, one draw.

export const SPARK = 0;
export const FIRE = 1;
export const FLASH = 2;
export const GLOW = 3;
export const EMBER = 4;
export const SMOKE = 0;
export const CHIP = 1;
export const SCORCH = 2;

const REBASE = 600; // s: births are shifted back by this much so the clock stays precise
const WIND = new THREE.Vector3(0.55, 0, 0.32); // m/s, drifting the smoke

const NOISE = /* glsl */ `
  float dHash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
  float dNoise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(dHash(i), dHash(i + vec2(1.0, 0.0)), f.x), mix(dHash(i + vec2(0.0, 1.0)), dHash(i + vec2(1.0, 1.0)), f.x), f.y);
  }
  // Value noise and its slope (x: value, yz: gradient).
  vec3 dNoiseD(vec2 x) {
    vec2 i = floor(x), f = fract(x);
    vec2 u = f * f * (3.0 - 2.0 * f), du = 6.0 * f * (1.0 - f);
    float a = dHash(i), b = dHash(i + vec2(1.0, 0.0)), c = dHash(i + vec2(0.0, 1.0)), d = dHash(i + vec2(1.0, 1.0));
    float k = a - b - c + d;
    return vec3(a + (b - a) * u.x + (c - a) * u.y + k * u.x * u.y, du * (vec2(b - a, c - a) + k * u.yx));
  }
  // Four octaves of it, the slopes summed alongside.
  vec3 dFbmD(vec2 p) {
    vec3 sum = vec3(0.0);
    float amplitude = 0.5, frequency = 1.0;
    for (int i = 0; i < 4; i++) {
      vec3 n = dNoiseD(p * frequency + float(i) * 7.31);
      sum.x += amplitude * n.x;
      sum.yz += amplitude * frequency * n.yz;
      amplitude *= 0.5;
      frequency *= 2.07;
    }
    return sum;
  }
`;

// What both pools share: where a particle is at its age, under linear drag
// and a constant pull (gravity, or buoyancy when negative).
const MOTION = /* glsl */ `
  vec3 dMove(vec3 p0, vec3 v0, vec3 pull, float k, float t, out vec3 v) {
    k = max(k, 0.01);
    float e = exp(-k * t);
    vec3 terminal = pull / k;
    v = (v0 - terminal) * e + terminal;
    return p0 + (v0 - terminal) * (1.0 - e) / k + terminal * t;
  }
`;

const SOFT = /* glsl */ `
  uniform sampler2D sceneDepth;
  uniform float softParticles;
  uniform vec2 depthResolution;
  uniform vec2 cameraRange;
  float dSoft(float viewDepth, float reach) {
    if (softParticles < 0.5 || reach <= 0.0) return 1.0;
    float depth = texture2D(sceneDepth, gl_FragCoord.xy / depthResolution).x;
    float surface = cameraRange.x * cameraRange.y / (cameraRange.y - depth * (cameraRange.y - cameraRange.x));
    return smoothstep(0.0, reach, surface - viewDepth);
  }
`;

const HOT_VERTEX = /* glsl */ `
  attribute vec4 a0; // birth position, birth time
  attribute vec4 a1; // velocity, lifetime
  attribute vec4 a2; // size at birth, size at death, pull (g, + falls), drag
  attribute vec4 a3; // style, seed, intensity, floor height (sparks, glow) or tint (0 fire, 1 electric)
  uniform float time;
  varying vec2 vUv;
  varying vec4 vState; // age 0..1, seed, intensity, style
  varying float vTint;
  varying float vDepth;
  ${MOTION}
  void main() {
    float t = max(time - a0.w, 0.0);
    float age = clamp(t / max(a1.w, 0.001), 0.0, 1.0);
    float style = a3.x;
    vec3 v;
    vec3 p = dMove(a0.xyz, a1.xyz, vec3(0.0, -9.8 * a2.z, 0.0), a2.w, t, v);
    if (style < 0.5 && p.y < a3.w) {
      // A spark that reaches the floor skips along it, slowed.
      p.y = a3.w + (a3.w - p.y) * 0.3;
      v.y = -v.y * 0.3;
      v.xz *= 0.55;
    }
    if (style > 3.5) {
      // Embers wander as they rise.
      p.x += sin(a3.y * 7.1 + t * 2.3) * 0.12 * t;
      p.z += cos(a3.y * 5.3 + t * 1.9) * 0.12 * t;
    }
    float size = mix(a2.x, a2.y, 1.0 - (1.0 - age) * (1.0 - age));
    vec2 corner = position.xy;
    vec4 view;
    if (style > 2.5 && style < 3.5) {
      // Glow on the floor: flat on it, under the blast.
      float c = cos(a3.y * 6.2832), s = sin(a3.y * 6.2832);
      vec2 spread = vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y) * size;
      view = viewMatrix * vec4(p.x + spread.x, a3.w + 0.04, p.z + spread.y, 1.0);
    } else {
      view = viewMatrix * vec4(p, 1.0);
      if (style < 0.5) {
        // A streak behind the spark, as long as its motion over ~1/25 s.
        vec3 sv = (viewMatrix * vec4(v, 0.0)).xyz;
        float speed = length(sv.xy);
        vec2 along = speed > 1e-4 ? sv.xy / speed : vec2(0.0, 1.0);
        vec2 across = vec2(-along.y, along.x);
        float reach = clamp(length(v) * 0.03, size * 2.0, 0.45);
        view.xy += across * corner.x * size + along * (corner.y * 0.5 - 0.5) * reach;
      } else {
        float angle = a3.y * 6.2832 + t * (style < 1.5 ? 1.2 : 0.0) * (fract(a3.y * 13.7) - 0.5);
        float c = cos(angle), s = sin(angle);
        view.xy += vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y) * size;
      }
    }
    vUv = uv;
    vState = vec4(age, a3.y, a3.z, style);
    vTint = a3.w;
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
`;

const HOT_FRAGMENT = /* glsl */ `
  uniform vec2 fogRange;
  varying vec2 vUv;
  varying vec4 vState;
  varying float vTint;
  varying float vDepth;
  ${NOISE}
  ${SOFT}
  // Metal and gas as they cool: white-hot, yellow, orange, deep red.
  vec3 dHeat(float h) {
    vec3 c = mix(vec3(0.16, 0.012, 0.0), vec3(1.0, 0.2, 0.02), smoothstep(0.0, 0.3, h));
    c = mix(c, vec3(1.75, 0.68, 0.12), smoothstep(0.28, 0.62, h));
    return mix(c, vec3(2.5, 2.1, 1.45), smoothstep(0.64, 1.0, h));
  }
  void main() {
    float age = vState.x, seed = vState.y, intensity = vState.z, style = vState.w;
    vec2 q = vUv * 2.0 - 1.0;
    float r2 = dot(q, q);
    vec3 color;
    float alpha;
    float soft = 0.0;
    if (style < 0.5) {
      // Spark: a bright head fading down its tail, cooling as it flies.
      float head = vUv.y;
      alpha = pow(max(1.0 - abs(q.x), 0.0), 1.6) * smoothstep(0.0, 0.6, head) * (1.0 - smoothstep(0.7, 1.0, age));
      // Near the eye a spark's streak would cover the screen: fade it.
      alpha *= smoothstep(0.8, 2.5, vDepth);
      color = dHeat(0.88 - age * 0.75) * (0.7 + 0.5 * head);
    } else if (style < 1.5) {
      // Fire: dense billows burn white-yellow, the thin edges orange to red.
      vec3 n = dFbmD(q * 1.45 + vec2(seed * 23.0, seed * 7.0 - age * 2.4));
      float density = 1.0 - r2 * 1.1 + (n.x - 0.5) * 1.5 - age * 0.5;
      alpha = smoothstep(0.0, 0.32, density) * smoothstep(0.0, 0.05, age) * (1.0 - smoothstep(0.5, 1.0, age));
      float heat = clamp(density * 1.25 - age * 0.75 + 0.2, 0.0, 1.0);
      color = dHeat(heat) * (0.75 + 0.5 * n.x);
      soft = 0.3;
    } else if (style < 2.5) {
      // Flash: a blinding core in a soft halo, gone within a few frames.
      float core = exp(-r2 * 12.0), halo = exp(-r2 * 3.5);
      alpha = (core + halo * 0.3) * pow(1.0 - age, 2.0);
      vec3 warm = mix(vec3(1.7, 0.8, 0.3), vec3(2.6, 2.3, 1.9), core);
      vec3 cold = mix(vec3(0.45, 0.75, 2.2), vec3(2.3, 2.6, 3.0), core);
      color = mix(warm, cold, clamp(vTint, 0.0, 1.0));
      soft = 0.2;
    } else if (style < 3.5) {
      // The blast's light on the floor.
      alpha = exp(-r2 * 3.0) * pow(1.0 - age, 1.6) * smoothstep(1.0, 0.8, sqrt(r2));
      color = vec3(1.5, 0.75, 0.3);
    } else {
      // Ember: a glowing point that flickers and dims.
      float flicker = 0.55 + 0.45 * sin(seed * 40.0 + age * 31.0) * sin(seed * 13.0 + age * 17.0);
      alpha = exp(-r2 * 3.2) * flicker * smoothstep(0.0, 0.1, age) * (1.0 - smoothstep(0.55, 1.0, age));
      color = dHeat(0.78 - age * 0.45);
    }
    alpha *= intensity * dSoft(vDepth, soft) * smoothstep(0.25, 0.9, vDepth);
    alpha *= 1.0 - clamp((vDepth - fogRange.x) / max(fogRange.y - fogRange.x, 1.0), 0.0, 1.0);
    if (alpha < 0.002) discard;
    gl_FragColor = vec4(color * alpha, alpha);
  }
`;

const SMOKE_VERTEX = /* glsl */ `
  attribute vec4 a0; // birth position, birth time
  attribute vec4 a1; // velocity, lifetime
  attribute vec4 a2; // size at birth, size at death, pull (g, + falls), drag
  attribute vec4 a3; // style, seed, opacity, floor height (chips, scorch)
  attribute vec4 a4; // tone (0 black .. 1 pale), heat, wind share, spin
  uniform float time;
  uniform vec3 wind;
  uniform vec3 sunDirection;
  varying vec2 vUv;
  varying vec4 vState; // age 0..1, seed, opacity, style
  varying vec4 vLook;  // tone, heat, seconds, spin phase
  varying vec3 vSun;   // toward the sun, in view space
  varying float vDepth;
  ${MOTION}
  void main() {
    float t = max(time - a0.w, 0.0);
    float age = clamp(t / max(a1.w, 0.001), 0.0, 1.0);
    float style = a3.x;
    vec3 v;
    vec3 p = dMove(a0.xyz, a1.xyz, vec3(0.0, -9.8 * a2.z, 0.0), a2.w, t, v);
    float size = a2.x;
    vec2 corner = position.xy;
    vec4 view;
    if (style > 1.5) {
      // A scorch mark, flat on the floor.
      float c = cos(a3.y * 6.2832), s = sin(a3.y * 6.2832);
      vec2 spread = vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y) * a2.x;
      view = viewMatrix * vec4(p.x + spread.x, a3.w + 0.03, p.z + spread.y, 1.0);
    } else {
      if (style < 0.5) {
        // Smoke: carried off by the wind as it rises, curling a little.
        p += wind * a4.z * t * smoothstep(0.0, 1.5, t);
        p.x += sin(a3.y * 11.0 + t * 0.7) * 0.15 * t;
        p.z += cos(a3.y * 7.0 + t * 0.6) * 0.15 * t;
        size = mix(a2.x, a2.y, 1.0 - pow(1.0 - age, 2.4));
      } else if (p.y < a3.w) {
        p.y = a3.w + (a3.w - p.y) * 0.25;
      }
      view = viewMatrix * vec4(p, 1.0);
      float angle = a3.y * 6.2832 + a4.w * t;
      float c = cos(angle), s = sin(angle);
      if (style > 0.5) corner.y *= 0.55 + 0.45 * abs(sin(a3.y * 9.0 + a4.w * t * 0.6));
      vec2 offset = vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y) * size;
      if (style < 0.5) {
        // Drawn out along its motion on screen: a trail or a rising wisp
        // reads as a streak, a puff come to rest stays round.
        vec2 sv = (viewMatrix * vec4(v + wind * a4.z, 0.0)).xy;
        float speed = length(sv);
        vec2 along = speed > 1e-3 ? sv / speed : vec2(0.0, 1.0);
        vec2 across = vec2(-along.y, along.x);
        offset = along * dot(offset, along) * (1.0 + min(speed * 0.45, 1.0)) + across * dot(offset, across);
      }
      view.xy += offset;
    }
    vUv = uv;
    vState = vec4(age, a3.y, a3.z, style);
    vLook = vec4(a4.x, a4.y, t, a4.w * t);
    vSun = normalize((viewMatrix * vec4(-sunDirection, 0.0)).xyz);
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
`;

const SMOKE_FRAGMENT = /* glsl */ `
  uniform vec3 sunColor;
  uniform vec3 ambient;
  uniform vec3 fogColor;
  uniform vec2 fogRange;
  varying vec2 vUv;
  varying vec4 vState;
  varying vec4 vLook;
  varying vec3 vSun;
  varying float vDepth;
  ${NOISE}
  ${SOFT}
  void main() {
    float age = vState.x, seed = vState.y, opacity = vState.z, style = vState.w;
    vec2 q = vUv * 2.0 - 1.0;
    float r2 = dot(q, q);
    vec3 color;
    float alpha;
    if (style < 0.5) {
      // A ball of smoke eaten into by noise; the noise's slope bends its
      // normal, so the sun picks out billows and the far side falls into
      // the ambient. It thins and pales as it spreads.
      vec2 p = q * (1.2 + 0.3 * age) + vec2(seed * 37.0, seed * 11.0 - vLook.z * 0.1);
      vec3 n = dFbmD(p * 1.55);
      float density = 1.0 - r2 + (n.x - 0.47) * 1.3 - age * 0.25;
      alpha = smoothstep(0.0, 0.45, density) * smoothstep(0.0, 0.07, age) * pow(1.0 - age, 1.2);
      vec3 normal = normalize(vec3(q * 1.05 - n.yz * 0.28, sqrt(max(1.0 - r2, 0.0)) + 0.3));
      float lit = dot(normal, vSun) * 0.5 + 0.5;
      vec3 base = mix(vec3(0.06, 0.058, 0.056), vec3(0.54, 0.535, 0.525), clamp(vLook.x + age * 0.35, 0.0, 1.0));
      color = base * (ambient * (0.8 + 0.35 * n.x) + sunColor * (0.1 + 1.05 * lit * lit));
      color *= mix(0.72, 1.12, smoothstep(0.15, 0.85, n.x));
      // Fire beneath lights young smoke from below, for its first moments.
      color += vec3(1.0, 0.36, 0.07) * vLook.y * exp(-age * 10.0) * smoothstep(0.35, -0.9, q.y) * smoothstep(0.0, 0.5, density);
      alpha *= opacity * dSoft(vDepth, 0.45);
    } else if (style < 1.5) {
      // A flake of plating: ragged and dark, glinting as it turns.
      float n = dNoise(q * 2.4 + seed * 50.0);
      alpha = step(max(abs(q.x), abs(q.y)) + (n - 0.5) * 0.7, 0.72) * (1.0 - smoothstep(0.8, 1.0, age));
      float glint = pow(max(sin(vLook.w * 1.7 + seed * 30.0), 0.0), 12.0);
      color = vec3(0.1, 0.1, 0.11) * (ambient + sunColor) + sunColor * glint * 0.8;
      alpha *= opacity;
    } else {
      // A scorch mark: soot streaked out from the middle, fading slowly.
      float r = sqrt(r2);
      float streaks = dNoise(vec2(atan(q.y, q.x) * 3.2 + seed * 40.0, r * 2.0));
      float n = dNoise(q * 3.5 + seed * 19.0);
      alpha = smoothstep(1.0, 0.25, r + (streaks - 0.5) * 0.5 + (n - 0.5) * 0.3);
      alpha *= opacity * (1.0 - smoothstep(0.6, 1.0, age)) * smoothstep(0.0, 0.02, age);
      // Black soot, browner where it is streaked thin.
      color = mix(vec3(0.03, 0.028, 0.026), vec3(0.11, 0.075, 0.05), smoothstep(0.35, 0.9, streaks) * smoothstep(0.2, 0.8, r));
    }
    alpha *= smoothstep(0.25, 0.9, vDepth);
    float fog = clamp((vDepth - fogRange.x) / max(fogRange.y - fogRange.x, 1.0), 0.0, 1.0);
    color = mix(color, fogColor, fog);
    if (alpha < 0.003) discard;
    gl_FragColor = vec4(color, alpha);
  }
`;

const ARC_VERTEX = /* glsl */ `
  attribute vec3 tangent;
  attribute vec4 meta; // side (-1, 1), along 0..1, half width (m), intensity
  uniform vec2 resolution;
  varying vec4 vMeta;
  varying float vDepth;
  void main() {
    vec4 view = viewMatrix * vec4(position, 1.0);
    vec3 along = (viewMatrix * vec4(tangent, 0.0)).xyz;
    vec3 side = cross(along, normalize(view.xyz));
    float length2 = dot(side, side);
    side = length2 > 1e-10 ? side * inversesqrt(length2) : vec3(1.0, 0.0, 0.0);
    // Never thinner than a couple of pixels, so a far arc still crackles.
    float pixel = 2.0 / (projectionMatrix[1][1] * max(resolution.y, 1.0));
    float width = max(meta.z, -view.z * pixel * 2.2);
    view.xyz += side * meta.x * width;
    vMeta = meta;
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
`;

const ARC_FRAGMENT = /* glsl */ `
  uniform vec2 fogRange;
  varying vec4 vMeta;
  varying float vDepth;
  void main() {
    float x = vMeta.x;
    float core = exp(-x * x * 26.0);
    float glow = exp(-x * x * 3.2);
    float ends = smoothstep(0.0, 0.08, vMeta.y) * smoothstep(1.0, 0.92, vMeta.y);
    float strength = vMeta.w * ends * smoothstep(0.2, 0.7, vDepth);
    strength *= 1.0 - clamp((vDepth - fogRange.x) / max(fogRange.y - fogRange.x, 1.0), 0.0, 1.0);
    // White-hot along the core, blue in the glow around it.
    vec3 color = (core * vec3(2.3, 2.6, 3.0) + glow * 0.55 * vec3(0.22, 0.48, 1.45)) * strength;
    float alpha = (core + glow * 0.55) * strength;
    if (alpha < 0.002) discard;
    gl_FragColor = vec4(color, alpha);
  }
`;

const depthUniforms = () => ({
  sceneDepth: particleDepth.map, softParticles: particleDepth.enabled,
  depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
});

// A fixed pool of instanced quads whose records live in their attribute
// arrays: births are appended, deaths swap in the last record.
class Pool {
  constructor(scene, { name, capacity, attributes, vertexShader, fragmentShader, uniforms, blending, premultiplied, renderOrder }) {
    this.capacity = capacity;
    this.count = 0;
    this.dirty = Infinity; // first record changed since the last upload
    this.emitted = 0;
    this.dropped = 0;
    const plane = new THREE.PlaneGeometry(2, 2);
    const geometry = new THREE.InstancedBufferGeometry();
    geometry.index = plane.index;
    geometry.attributes.position = plane.attributes.position;
    geometry.attributes.uv = plane.attributes.uv;
    this.arrays = [];
    this.attributes = [];
    for (let i = 0; i < attributes; i++) {
      const attribute = new THREE.InstancedBufferAttribute(new Float32Array(capacity * 4), 4).setUsage(THREE.DynamicDrawUsage);
      geometry.setAttribute(`a${i}`, attribute);
      this.attributes.push(attribute);
      this.arrays.push(attribute.array);
    }
    geometry.instanceCount = 0;
    this.material = new THREE.ShaderMaterial({
      name, uniforms, vertexShader, fragmentShader,
      transparent: true, depthWrite: false, depthTest: true, toneMapped: false,
      blending, premultipliedAlpha: premultiplied,
      // Quads laid flat on the floor wind either way round seen from above.
      side: THREE.DoubleSide,
    });
    this.mesh = new THREE.Mesh(geometry, this.material);
    this.mesh.name = name;
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = renderOrder;
    this.mesh.visible = false;
    this.mesh.onBeforeRender = (renderer, scene, camera) => particleDepth.capture(renderer, scene, camera);
    scene.add(this.mesh);
  }

  // A free record's index, or -1 when the pool is full (the particle is
  // dropped: the ones already flying matter more than one more).
  take() {
    if (this.count >= this.capacity) {
      this.dropped++;
      return -1;
    }
    const i = this.count++;
    if (i < this.dirty) this.dirty = i;
    this.emitted++;
    return i;
  }

  set(attribute, i, x, y, z, w) {
    const a = this.arrays[attribute], k = i * 4;
    a[k] = x; a[k + 1] = y; a[k + 2] = z; a[k + 3] = w;
  }

  update(time) {
    const birth = this.arrays[0], flow = this.arrays[1];
    for (let i = this.count - 1; i >= 0; i--) {
      if (time - birth[i * 4 + 3] < flow[i * 4 + 3]) continue;
      const last = --this.count;
      if (i !== last) {
        for (const array of this.arrays) array.copyWithin(i * 4, last * 4, last * 4 + 4);
        if (i < this.dirty) this.dirty = i;
      }
    }
    if (this.dirty < this.count) {
      for (const attribute of this.attributes) {
        // A range not yet uploaded (frames stepped without a draw) is
        // merged in, not replaced: three clears the ranges once it uploads.
        const pending = attribute.updateRanges[0];
        const start = Math.min(this.dirty * 4, pending ? pending.start : Infinity);
        attribute.clearUpdateRanges();
        attribute.addUpdateRange(start, this.count * 4 - start);
        attribute.needsUpdate = true;
      }
    }
    this.dirty = Infinity;
    this.mesh.geometry.instanceCount = this.count;
    this.mesh.visible = this.count > 0;
  }

  // Every birth moved back by `by` seconds, with the clock.
  rebase(by) {
    const birth = this.arrays[0];
    for (let i = 0; i < this.count; i++) birth[i * 4 + 3] -= by;
    if (this.count) this.dirty = 0;
  }

  clear() {
    this.count = 0;
    this.dirty = Infinity;
    this.mesh.geometry.instanceCount = 0;
    this.mesh.visible = false;
  }

  dispose() {
    this.mesh.removeFromParent();
    this.mesh.geometry.dispose();
    this.material.dispose();
  }
}

const SEGMENTS = 12; // per arc
const BRANCH_SEGMENTS = 5;
const BRANCHES = 2;
const POINTS_PER_ARC = SEGMENTS + 1 + BRANCHES * (BRANCH_SEGMENTS + 1);
const FLICKER = 30; // new jags a second

// A seeded random number in [0, 1) from two integers, without state.
function jitter(seed, n) {
  let h = Math.imul(seed ^ Math.imul(n, 0x9e3779b1), 0x85ebca6b);
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

// An arc's end: a fixed point, or a point carried by an object.
class Anchor {
  constructor() {
    this.object = null;
    this.local = new THREE.Vector3();
  }

  set(object, point) {
    this.object = object;
    this.local.copy(point);
    return this;
  }

  resolve(out) {
    out.copy(this.local);
    if (this.object) out.applyMatrix4(this.object.matrixWorld);
    return out;
  }
}

class Arcs {
  constructor(scene, capacity) {
    this.capacity = capacity;
    this.arcs = Array.from({ length: capacity }, () => ({
      live: false, age: 0, life: 0, width: 0, intensity: 0, seed: 0, reach: 0, a: new Anchor(), b: new Anchor(),
    }));
    this.live = 0;
    this.spawned = 0;
    const vertices = capacity * POINTS_PER_ARC * 2;
    this.positions = new Float32Array(vertices * 3);
    this.tangents = new Float32Array(vertices * 3);
    this.meta = new Float32Array(vertices * 4);
    const quads = capacity * (SEGMENTS + BRANCHES * BRANCH_SEGMENTS);
    this.indices = new Uint16Array(quads * 6);
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(this.positions, 3).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute("tangent", new THREE.BufferAttribute(this.tangents, 3).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute("meta", new THREE.BufferAttribute(this.meta, 4).setUsage(THREE.DynamicDrawUsage));
    geometry.setIndex(new THREE.BufferAttribute(this.indices, 1).setUsage(THREE.DynamicDrawUsage));
    geometry.setDrawRange(0, 0);
    this.material = new THREE.ShaderMaterial({
      name: "destruction-arcs",
      uniforms: { resolution: lighting.resolution, fogRange: lighting.fogRange },
      vertexShader: ARC_VERTEX, fragmentShader: ARC_FRAGMENT,
      transparent: true, depthWrite: false, depthTest: true, toneMapped: false,
      blending: THREE.AdditiveBlending, premultipliedAlpha: true, side: THREE.DoubleSide,
    });
    this.mesh = new THREE.Mesh(geometry, this.material);
    this.mesh.name = "destruction-arcs";
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = 12;
    this.mesh.visible = false;
    scene.add(this.mesh);
    this.points = Array.from({ length: SEGMENTS + 1 }, () => new THREE.Vector3());
    this.from = new THREE.Vector3();
    this.to = new THREE.Vector3();
    this.dir = new THREE.Vector3();
    this.u = new THREE.Vector3();
    this.w = new THREE.Vector3();
    this.tangent = new THREE.Vector3();
    this.branchPoints = Array.from({ length: BRANCH_SEGMENTS + 1 }, () => new THREE.Vector3());
  }

  // An arc from a to b (each an object or null, and a point in its frame or
  // the world) for `life` seconds. Returns false when every arc is busy.
  add(aObject, a, bObject, b, life, width, intensity) {
    if (this.live >= this.capacity) return false;
    let arc = null;
    for (const candidate of this.arcs) if (!candidate.live) { arc = candidate; break; }
    arc.live = true;
    arc.age = 0;
    arc.life = life;
    arc.width = width;
    arc.intensity = intensity;
    arc.seed = (Math.random() * 0xffffffff) >>> 0;
    arc.a.set(aObject, a);
    arc.b.set(bObject, b);
    this.live++;
    this.spawned++;
    return true;
  }

  update(dt, time) {
    this.vertex = 0;
    this.index = 0;
    const frame = Math.floor(time * FLICKER);
    for (const arc of this.arcs) {
      if (!arc.live) continue;
      arc.age += dt;
      if (arc.age >= arc.life) {
        arc.live = false;
        this.live--;
        continue;
      }
      const from = arc.a.resolve(this.from), to = arc.b.resolve(this.to);
      const length = from.distanceTo(to);
      if (length < 0.02) continue;
      const fade = Math.min(1, 3 * (1 - arc.age / arc.life)) * (0.6 + 0.4 * jitter(arc.seed, frame + 11));
      const dir = this.dir.subVectors(to, from).divideScalar(length);
      this.u.set(dir.y, dir.z, -dir.x).cross(dir).normalize();
      this.w.crossVectors(dir, this.u);
      const jag = length * 0.16;
      const points = this.points;
      for (let i = 0; i <= SEGMENTS; i++) {
        const s = i / SEGMENTS;
        const envelope = Math.sin(Math.PI * s);
        const ju = (jitter(arc.seed, frame * 97 + i * 2) - 0.5) * 2 * jag * envelope;
        const jw = (jitter(arc.seed, frame * 97 + i * 2 + 1) - 0.5) * 2 * jag * envelope;
        points[i].lerpVectors(from, to, s).addScaledVector(this.u, ju).addScaledVector(this.w, jw);
      }
      this.strip(points, SEGMENTS, arc.width, arc.intensity * fade);
      // A branch or two off the inside points, forking away and fading.
      for (let k = 0; k < BRANCHES; k++) {
        if (jitter(arc.seed, frame * 31 + 50 + k) > 0.55) continue;
        const at = 2 + Math.floor(jitter(arc.seed, frame * 31 + 60 + k) * (SEGMENTS - 4));
        const reach = length * (0.2 + 0.25 * jitter(arc.seed, frame * 31 + 70 + k));
        const branch = this.branchPoints;
        this.tangent.subVectors(points[at + 1], points[at - 1]).normalize()
          .addScaledVector(this.u, (jitter(arc.seed, k * 7 + 80) - 0.5) * 2.4)
          .addScaledVector(this.w, (jitter(arc.seed, k * 7 + 81) - 0.5) * 2.4).normalize();
        for (let i = 0; i <= BRANCH_SEGMENTS; i++) {
          const s = i / BRANCH_SEGMENTS;
          branch[i].copy(points[at]).addScaledVector(this.tangent, reach * s)
            .addScaledVector(this.u, (jitter(arc.seed, frame * 53 + k * 13 + i) - 0.5) * reach * 0.35 * s)
            .addScaledVector(this.w, (jitter(arc.seed, frame * 59 + k * 13 + i) - 0.5) * reach * 0.35 * s);
        }
        this.strip(branch, BRANCH_SEGMENTS, arc.width * 0.6, arc.intensity * fade * 0.7);
      }
    }
    const geometry = this.mesh.geometry;
    const { vertex, index } = this;
    geometry.setDrawRange(0, index);
    this.mesh.visible = index > 0;
    if (!index) return;
    for (const name of ["position", "tangent", "meta"]) {
      const attribute = geometry.getAttribute(name);
      attribute.clearUpdateRanges();
      attribute.addUpdateRange(0, vertex * attribute.itemSize);
      attribute.needsUpdate = true;
    }
    geometry.index.clearUpdateRanges();
    geometry.index.addUpdateRange(0, index);
    geometry.index.needsUpdate = true;
  }

  // One strand of points as a ribbon: two vertices a point, a quad a segment.
  strip(points, segments, width, intensity) {
    let vertex = this.vertex, index = this.index;
    const base = vertex;
    for (let i = 0; i <= segments; i++) {
      const p = points[i];
      const before = points[Math.max(i - 1, 0)], after = points[Math.min(i + 1, segments)];
      this.tangent.subVectors(after, before);
      for (let side = -1; side <= 1; side += 2) {
        const v = vertex++;
        this.positions[v * 3] = p.x;
        this.positions[v * 3 + 1] = p.y;
        this.positions[v * 3 + 2] = p.z;
        this.tangents[v * 3] = this.tangent.x;
        this.tangents[v * 3 + 1] = this.tangent.y;
        this.tangents[v * 3 + 2] = this.tangent.z;
        this.meta[v * 4] = side;
        this.meta[v * 4 + 1] = i / segments;
        this.meta[v * 4 + 2] = width;
        this.meta[v * 4 + 3] = intensity;
      }
    }
    for (let i = 0; i < segments; i++) {
      const a = base + i * 2;
      this.indices[index++] = a;
      this.indices[index++] = a + 1;
      this.indices[index++] = a + 2;
      this.indices[index++] = a + 1;
      this.indices[index++] = a + 3;
      this.indices[index++] = a + 2;
    }
    this.vertex = vertex;
    this.index = index;
  }

  clear() {
    for (const arc of this.arcs) arc.live = false;
    this.live = 0;
    this.mesh.geometry.setDrawRange(0, 0);
    this.mesh.visible = false;
  }

  dispose() {
    this.mesh.removeFromParent();
    this.mesh.geometry.dispose();
    this.material.dispose();
  }
}

// The pools and arcs together, with emitters in plain terms. Positions and
// velocities are read, never kept.
export class DestructionFx {
  constructor(scene, { hot = 1536, smoke = 1536, arcs = 48 } = {}) {
    this.time = 0;
    this.clock = { value: 0 };
    this.hot = new Pool(scene, {
      name: "destruction-hot", capacity: hot, attributes: 4,
      vertexShader: HOT_VERTEX, fragmentShader: HOT_FRAGMENT,
      uniforms: { time: this.clock, fogRange: lighting.fogRange, ...depthUniforms() },
      blending: THREE.AdditiveBlending, premultiplied: true, renderOrder: 11,
    });
    this.smoke = new Pool(scene, {
      name: "destruction-smoke", capacity: smoke, attributes: 5,
      vertexShader: SMOKE_VERTEX, fragmentShader: SMOKE_FRAGMENT,
      uniforms: {
        time: this.clock, wind: { value: WIND.clone() },
        sunDirection: lighting.sunDirection, sunColor: lighting.sunColor, ambient: lighting.ambient,
        fogColor: lighting.fogColor, fogRange: lighting.fogRange, ...depthUniforms(),
      },
      blending: THREE.NormalBlending, premultiplied: false, renderOrder: 9,
    });
    this.arcs = new Arcs(scene, arcs);
  }

  get draws() {
    return Number(this.hot.mesh.visible) + Number(this.smoke.mesh.visible) + Number(this.arcs.mesh.visible);
  }

  // How much of a pool is still free, past a reserve kept for blasts and
  // wrecks: 1 with room to spare, falling to 0 as it fills. Lesser emitters
  // (trails, wisps) scale their rate by it.
  room(pool) {
    return THREE.MathUtils.clamp((1 - pool.count / pool.capacity - 0.25) / 0.35, 0, 1);
  }

  // style: SPARK, FIRE, FLASH, GLOW or EMBER. size0, size1: its radius at
  // birth and death (a spark's half width); pull: gravity's share (negative
  // rises); extra: the floor's height for sparks and glows, or a flash's
  // tint (0 fire, 1 electric).
  hotParticle(style, position, velocity, life, size0, size1, pull, drag, intensity, extra = -1e4, delay = 0) {
    const i = this.hot.take();
    if (i < 0) return;
    this.hot.set(0, i, position.x, position.y, position.z, this.time + delay);
    this.hot.set(1, i, velocity.x, velocity.y, velocity.z, life);
    this.hot.set(2, i, size0, size1, pull, drag);
    this.hot.set(3, i, style, Math.random(), intensity, extra);
  }

  // style: SMOKE, CHIP or SCORCH. tone: 0 oily black .. 1 pale grey; heat:
  // fire light on its underside while young; wind: its share of the wind;
  // floor: the height chips skip off and scorch marks lie on.
  smokeParticle(style, position, velocity, life, size0, size1, pull, drag, opacity, { tone = 0.3, heat = 0, wind = 1, spin = 0, floor = -1e4, delay = 0 } = {}) {
    const i = this.smoke.take();
    if (i < 0) return;
    this.smoke.set(0, i, position.x, position.y, position.z, this.time + delay);
    this.smoke.set(1, i, velocity.x, velocity.y, velocity.z, life);
    this.smoke.set(2, i, size0, size1, pull, drag);
    this.smoke.set(3, i, style, Math.random(), opacity, floor);
    this.smoke.set(4, i, tone, heat, wind, spin);
  }

  arc(aObject, a, bObject, b, life, width = 0.012, intensity = 1) {
    return this.arcs.add(aObject, a, bObject, b, life, width, intensity);
  }

  update(dt) {
    this.time += dt;
    if (this.time > REBASE) {
      this.time -= REBASE;
      this.hot.rebase(REBASE);
      this.smoke.rebase(REBASE);
    }
    this.clock.value = this.time;
    this.hot.update(this.time);
    this.smoke.update(this.time);
    this.arcs.update(dt, this.time);
  }

  clear() {
    this.hot.clear();
    this.smoke.clear();
    this.arcs.clear();
  }

  dispose() {
    this.hot.dispose();
    this.smoke.dispose();
    this.arcs.dispose();
  }
}
