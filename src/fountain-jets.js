import * as THREE from 'three';
import { lighting } from './materials.js';
import { particleDepth } from './particle-depth.js';
import { drawFramebuffer } from './render-targets.js';

// A fountain in Enhanced: the stock plume's size and timing, drawn as water
// lit by the scene. The stock jet (effects.json `waterfountain`, its water1
// layer) leaves the nozzle at 11.6-13 m/s and loses 45 m/s over a 3 s key,
// 15 m/s², so its water stands 4.5-5.6 m, and its sprites, ten times larger
// by then, show the plume to about 7.5 m. Here the water leaves at that
// speed, 12.5 m/s, under ordinary gravity, so it rises 8 m and opens into an
// umbrella that falls back into the inner tier, which fills to its lip and
// overflows into the outer pool (fountain-water.js draws the raised pool).
//
//   column  the jet itself: glassy where it leaves the water, a churning
//           white column above, its width steady; a dome of heaving foam
//           where it meets the pool
//   ropes   the umbrella: ropes of water arcing out of the crown in bundles,
//           more of them close around the jet, and fine strands, falling
//           into the inner tier; and a sparse outer ring of long arcs that
//           clear the tier and land in the outer pool. Each is a ribbon along
//           the path of a stream whose direction, reach and thickness wander
//           slowly, so it sways but holds together; it thins as it speeds
//           up, swells and necks, and breaks into drops.
//   film    the overflow, a sheet pouring over the tier's lip and running
//           down its sloping face into the outer pool
//   clump   aerated water boiling up the column and churning in the crown
//   froth   spray thrown off where the jet breaks at the crown
//   skirt   spray thrown up where the water lands
//   spray   fine spray torn from the ropes, falling with them
//   splash  where a stream strikes a pool: a crown, a spike, droplets
//   drop    drops shaken off the streams and thrown from the crown
//   mist    a faint low haze drifting downwind
//   ring    ripples spreading on the pools from the strikes
//   foam    foam riding outward from where the water lands
// Everything is placed analytically from the time on the GPU.
//
// Clear water shows the scene behind it, bent, softened and faintly tinted
// (a copy of the frame taken just before the jets draw, read through its
// mipmaps), and the sky and square at grazing angles; backlit, the sun bent
// through it brightens its sides. Aerated water is lit like cloud: sunward
// faces bright, its shade lit by the sky, shadowed by the plume between it
// and the sun, glowing through its thin parts against the sun. Spray is lit
// by the shape it hangs in, and laid on as light (compositeFragment). Glints
// go on top; highlights roll off rather than clip.
//
// The soft spray (mist, froth, skirt, spray) is drawn at half the frame's
// width and height and laid over it with depth-aware upsampling, as the
// steam volumes are: a quarter of the fragments. Twelve draws for all five
// fountains, far to near: what lies on the pools; the overflow beyond the
// jets' axes from the eye; then beyond the axes, the soft spray (two draws:
// into its target, then over the frame), the ropes, the drops, splashes and
// aerated water; the columns; before the axes the same in reverse, the
// overflow before the ropes. Each draw holds only the fountains in view.
// Nothing is allocated per frame.
const APEX = 8.0;
export const JET_APEX = APEX;
// Theed's fountain prop: around the nozzle, a 12-sided inner tier, its
// corners facing 0°, 30°... (the props are turned 180°, which keeps them
// there). Its lip stands 4 m out to the corners and `rise` above the water
// the basin mesh draws; its outer face slopes 45° down into the outer pool,
// meeting the water `foot` out; the outer pool lies `outer` from that level.
export const TIER = { lip: 4.0, sides: 12, rise: 0.797, foot: 4.82, outer: -0.05 };
// Ropes, fine strands and outer arcs per fountain, the bundles the ropes and
// strands gather in, the points along each, and how long their water falls.
const ROPES = 170, STRANDS = 160, ARCS = 28, BUNDLES = 7, STRIP = 22, ROPE_LIFE = 1.45;
// Particles per fountain and their lives, by kind (the order of the shaders'
// constants). A splash is eight particles: crown, spike, puff, droplets.
const COUNTS = [24, 300, 340, 640, 8 * 44, 700, 320, 36, 72, 80];
const LIVES = [6.0, 1.7, 1.8, ROPE_LIFE, 0.5, 1.4, 2.45, 0.7, 1.1, 3.4];
const SPRAY = 3, SPLASH = 4, CLUMP = 6, RING = 8;
// How much of what lies behind the soft spray it hides, for its density.
const SPRAY_HIDES = 0.3;
const FOUNTAINS = 5;

// Detail for aerated water, drawn once per page into two 128x128 cells: soft
// masses, fibrous along their motion, their edges torn into holes and wisps.
// RG is the surface normal, B its thickness, A a density that the shader
// thresholds, so the same mass can thin away.
let atlasPixels = null;
export function sprayAtlas() {
  const texture = new THREE.DataTexture(atlasPixels ||= foamPixels(), 256, 128);
  texture.generateMipmaps = true;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.magFilter = THREE.LinearFilter;
  texture.needsUpdate = true;
  return texture;
}
function foamPixels() {
  const cell = 128, data = new Uint8Array(256 * cell * 4);
  let state = 0x2545f491;
  const random = () => ((state = Math.imul(state, 1664525) + 1013904223 >>> 0) / 4294967296);
  const lattice = Float32Array.from({ length: 256 }, random);
  const smooth = t => { t = Math.min(Math.max(t, 0), 1); return t * t * (3 - 2 * t); };
  const noise = (x, y) => {
    const ix = Math.floor(x), iy = Math.floor(y);
    let fx = x - ix, fy = y - iy;
    fx = fx * fx * (3 - 2 * fx); fy = fy * fy * (3 - 2 * fy);
    const a = lattice[((iy & 15) << 4) | (ix & 15)], b = lattice[((iy & 15) << 4) | ((ix + 1) & 15)];
    const c = lattice[(((iy + 1) & 15) << 4) | (ix & 15)], d = lattice[(((iy + 1) & 15) << 4) | ((ix + 1) & 15)];
    return (a + (b - a) * fx) * (1 - fy) + (c + (d - c) * fx) * fy;
  };
  const fbm = (x, y) => noise(x, y) * .52 + noise(x * 2.03 + 5.2, y * 2.03 + 1.3) * .3 + noise(x * 4.1 + 9.7, y * 4.1 + 3.1) * .18;
  const height = new Float32Array(cell * cell), density = new Float32Array(cell * cell);
  for (let c = 0; c < 2; c++) {
    for (let y = 0; y < cell; y++) for (let x = 0; x < cell; x++) {
      const u = (x + .5) / cell * 2 - 1, v = (y + .5) / cell * 2 - 1, i = y * cell + x;
      const wu = u + .25 * (noise(u * 2.5 + c * 5, v * 2.5) - .5), wv = v + .25 * (noise(u * 2.5 + 9, v * 2.5 + c * 5) - .5);
      const d = Math.hypot(wu / .7, wv), body = smooth((.86 - d + (fbm(u * 2.2 + c * 7.3, v * 1.4 + c * 3.1) - .5) * .9) / .55);
      if (body <= 0) { height[i] = density[i] = 0; continue; }
      const fiber = fbm(u * 6.5 + c * 3.1, v * 1.7 + c * 7.3), fine = fbm(u * 16 + c * 1.7, v * 5 + c * 4.3);
      const torn = 1 - (1 - smooth((fbm(u * 4.5 + c * 2.3, v * 3.1 + c * 9.1) - .32) / .25)) * smooth((d - .35) / .4);
      height[i] = body * torn * (.12 * Math.sqrt(Math.max(0, 1 - d * d)) + .05 * fiber + .015 * fine);
      density[i] = body * torn * (.25 + .85 * fiber) + (fine - .5) * .25 * body;
    }
    const scale = cell / 4, last = cell - 1;
    for (let y = 0; y < cell; y++) for (let x = 0; x < cell; x++) {
      const row = y * cell, nx = -(height[row + Math.min(x + 1, last)] - height[row + Math.max(x - 1, 0)]) * scale;
      const ny = -(height[Math.min(y + 1, last) * cell + x] - height[Math.max(y - 1, 0) * cell + x]) * scale;
      const length = Math.hypot(nx, ny, 1), o = (y * 256 + c * cell + x) * 4;
      data[o] = Math.round((nx / length * .5 + .5) * 255);
      data[o + 1] = Math.round((ny / length * .5 + .5) * 255);
      data[o + 2] = Math.round(Math.min(height[row + x] / .18, 1) * 255);
      data[o + 3] = Math.round(Math.min(Math.max(density[row + x], 0), 1) * 255);
    }
  }
  return data;
}

// Each draw holds one block of instances per fountain in view; `visible`
// lists their slots, so fountains out of view cost nothing.
const VISIBLE = /* glsl */`
  uniform float visible[5];
  int fountain(int block) { return int(visible[gl_InstanceID / block] + 0.5); }
`;

const HASH = /* glsl */`
  float hash1(float p) { p = fract(p * 0.1031); p *= p + 33.33; p *= p + p; return fract(p); }
  float noise1(float x) { float i = floor(x), f = fract(x); f = f * f * (3.0 - 2.0 * f); return mix(hash1(i), hash1(i + 1.0), f); }
  vec4 hash4(float p) {
    vec4 q = fract(vec4(p) * vec4(.1031, .1030, .0973, .1099));
    q += dot(q, q.wzxy + 33.33);
    return fract((q.xxyz + q.yzzw) * q.zywx);
  }
`;
const NOISE3 = /* glsl */`
  float hash3(vec3 p) { p = fract(p * 0.1031); p += dot(p, p.zyx + 31.32); return fract((p.x + p.y) * p.z); }
  float noise3(vec3 p) {
    vec3 i = floor(p), f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(mix(hash3(i), hash3(i + vec3(1.0, 0.0, 0.0)), f.x), mix(hash3(i + vec3(0.0, 1.0, 0.0)), hash3(i + vec3(1.0, 1.0, 0.0)), f.x), f.y),
      mix(mix(hash3(i + vec3(0.0, 0.0, 1.0)), hash3(i + vec3(1.0, 0.0, 1.0)), f.x), mix(hash3(i + vec3(0.0, 1.0, 1.0)), hash3(i + 1.0), f.x), f.y), f.z);
  }
`;

// The map's light, as the water sees it.
const SKY = /* glsl */`
  uniform vec3 sunDirection, sunColor, ambient, ambientTop, ambientBottom;
  // The sun, a little less yellow than the map's (tuned for its lightmaps).
  vec3 sunLight() { return mix(sunColor, vec3(dot(sunColor, vec3(0.3333))), 0.35); }
  vec3 skyAbove() { return ambientTop * 1.3 + vec3(0.1, 0.13, 0.27); }
  // Light from the sky, and from the warm square below, on a face toward N.
  vec3 skyFill(vec3 N) { return mix(mix(ambient, ambientTop, 0.5) * 1.4 + vec3(0.06, 0.08, 0.14), ambientBottom * 0.85, 0.3 * max(-N.y, 0.0)); }
  // What a mirror shows: sky above, the square's walls toward the horizon, its floor below.
  vec3 skyReflect(vec3 R) {
    vec3 above = skyAbove(), around = mix(above, ambientBottom, 0.3), below = ambientBottom * 0.6;
    return R.y > 0.0 ? mix(around, above, smoothstep(0.0, 0.6, R.y)) : mix(around, below, smoothstep(0.0, 0.4, -R.y));
  }
  // Sunlight scattered toward the eye by spray and thin water, strongest
  // looking toward the sun (Henyey-Greenstein, g = 0.45; 1 is even
  // scattering), held below the sun's own glare.
  float forward(vec3 V) {
    float c = dot(-V, -sunDirection);
    return min(0.7975 / pow(1.2025 - 0.9 * c, 1.5), 2.5);
  }
  // Bright water rolls off toward a light grey-white: only glints, drawn
  // over this, reach white.
  vec3 soften(vec3 c) {
    float m = max(c.r, max(c.g, c.b));
    return m > 0.8 ? c * ((0.8 + 0.15 * (1.0 - exp(-(m - 0.8) / 0.15))) / m) : c;
  }
  // Spray, lit as the shape it hangs in (normal N): the sun on that side, the
  // sky in its shade, and the sun scattered through it toward the eye.
  vec3 sprayLight(vec3 N, vec3 V, float lit) {
    float facing = clamp(dot(N, -sunDirection) * 0.6 + 0.4, 0.0, 1.0);
    return soften(skyFill(N) * 0.9 + sunLight() * lit * (0.65 * facing * facing + 0.6 * forward(V)));
  }
`;

// How much sunlight reaches a point of the plume (relative to its nozzle)
// through the plume itself: the umbrella, a shell of ropes with gaps between
// them widening from the crown (eight steps along the ray toward the sun),
// and exactly through the jet (a cylinder) and the dense crown (a ball).
const SUN_THROUGH = /* glsl */`
  float sunThrough(vec3 p) {
    vec3 L = -sunDirection;
    float reach = max(APEX + 0.4 - p.y, 0.0) / max(L.y, 0.2), depth = 0.0;
    for (int k = 0; k < 8; k++) {
      vec3 q = p + L * reach * (float(k) + 0.5) / 8.0;
      float r = length(q.xz), R = 0.6 + 1.45 * sqrt(max(2.0 * (APEX * 0.9 - q.y) / 9.81, 0.0));
      depth += 0.9 * exp(-pow((r - R) / 0.3, 2.0)) * step(q.y, APEX * 0.95) * reach / 8.0;
    }
    float l2 = max(dot(L.xz, L.xz), 1e-4), tc = -dot(p.xz, L.xz) / l2, rc = 0.15 + 0.05 * clamp(p.y + L.y * tc, 0.0, APEX);
    float miss = length(p.xz + L.xz * tc), half_ = sqrt(max(rc * rc - miss * miss, 0.0)) / sqrt(l2);
    if (p.y + L.y * tc < APEX * 0.8) depth += 2.5 * clamp(tc + half_, 0.0, 2.0 * half_);
    vec3 c = vec3(0.0, APEX * 0.88, 0.0) - p;
    float tb = dot(c, L), across = sqrt(max(1.0 - (dot(c, c) - tb * tb), 0.0));
    depth += 0.8 * clamp(tb + across, 0.0, 2.0 * across);
    return exp(-0.7 * depth);
  }
`;

// A stream of the umbrella, for its water leaving the crown at time born:
// the way it falls, and its start (distance out, outward speed, height,
// upward speed). These wander slowly, so neighbouring water follows almost
// the same arc and the stream sways but holds together. Ropes (numbers below
// ROPES) leave the crown's top, two in five of them falling close around
// the jet; fine strands leave its edge; both mostly gather in bundles
// (sheets that tore into ropes), leaving gaps between them. Arcs (from
// ROPES + STRANDS), evenly spaced, are thrown far out to land in the outer
// pool.
const ROPE = /* glsl */`
  void ropeStart(float j, float salt, float born, out vec2 dir, out vec4 start) {
    float n = j * 1.913 + salt;
    vec4 h = hash4(j * 3.17 + salt * 11.0);
    vec4 w = 0.5 + 0.5 * sin(born * vec4(0.43, 0.53, 0.71, 0.61) + n * vec4(0.9, 1.7, 3.1, 2.3));
    if (j >= ROPES + STRANDS) {
      float theta = (j - ROPES - STRANDS + 0.5 + 0.6 * (h.x - 0.5)) / ARCS * TAU + salt + 0.08 * sin(born * 0.5 + n);
      dir = vec2(cos(theta), sin(theta));
      start = vec4(0.6 + 0.4 * w.x, 3.2 + 0.6 * w.y, APEX * (0.84 + 0.08 * w.z), 0.6 + 0.8 * w.w);
      return;
    }
    float fine = step(ROPES, j), bundle = floor(h.x * BUNDLES), reach = 0.85 + 0.3 * hash1(bundle * 7.1 + salt);
    float theta = h.y < 0.8 ? (bundle + hash1(bundle * 5.3 + salt)) / BUNDLES * TAU + (h.z - 0.5) * 0.45 : h.z * TAU;
    theta += salt + 0.2 * sin(born * 0.37 + n * 2.1) + 0.1 * sin(born * 0.91 + n * 5.3);
    dir = vec2(cos(theta), sin(theta));
    float close = step(h.w, 0.4) * (1.0 - fine);
    start = fine > 0.5 ? vec4(0.35 + 0.8 * w.x, (0.6 + 1.2 * w.y) * reach, APEX * (0.76 + 0.18 * w.z), 0.8 * w.w)
      : vec4(0.3 + 0.7 * w.x, mix((1.0 + 0.9 * w.y) * reach, 0.25 + 0.4 * w.y, close), APEX * (0.84 + 0.12 * w.z), 0.3 + 0.6 * w.w);
  }
  vec3 ropeAt(vec2 dir, vec4 start, float age) {
    float d = start.x + start.y * age;
    return vec3(dir.x * d, start.z + start.w * age - 0.5 * G * age * age, dir.y * d) + wind * 0.3 * age * age;
  }
  // Which pool a stream falls into: the tier's (inner) or the outer one.
  bool ropeOuter(float j) { return j >= ROPES + STRANDS; }
  // Where a stream's water that left the crown at born meets its pool.
  vec3 ropeLanding(float j, float salt, float born, float level, out vec2 dir) {
    vec4 start;
    ropeStart(j, salt, born, dir, start);
    return ropeAt(dir, start, (start.w + sqrt(start.w * start.w + 2.0 * G * max(start.z - level, 0.0))) / G);
  }
  // Its own look: radius (m), opacity, where it breaks into drops (as a
  // share of its fall), how aerated.
  vec4 ropeLook(float j, float salt) {
    vec4 h = hash4(j * 5.11 + salt * 7.3 + 3.0);
    if (j >= ROPES + STRANDS) return vec4(0.012 + 0.012 * h.x, 0.7 + 0.3 * h.y, 0.55 + 0.35 * h.z, 0.45 + 0.4 * h.w);
    return j < ROPES ? vec4(0.011 * exp(1.5 * h.x), 0.6 + 0.4 * h.y, 0.3 + 0.6 * h.z, 0.4 + 0.6 * h.w)
      : vec4(0.004 + 0.008 * h.x, 0.5 + 0.4 * h.y, 0.15 + 0.45 * h.z, 0.25 + 0.45 * h.w);
  }
`;

// Water lit by the scene, premultiplied (see the top of this file). Without
// this frame's copy, clear water lets the real scene through instead.
const WATER = /* glsl */`
  uniform sampler2D sceneColor;
  uniform vec2 sceneResolution;
  uniform float hasScene;
  // lit: sunlight reaching here through the plume; aerated: 0 glassy, 1
  // white; thick: the water's depth through here, 0 at its edge; bend: how
  // far it displaces the scene behind; blur: the mipmap level that scene is
  // read from; ripple: a small tilt of the surface; spark: a facet of it
  // turning the sun to the eye right here.
  vec4 water(vec3 N, vec3 V, float lit, float aerated, float thick, float alpha, float bend, float blur, vec3 ripple, float spark) {
    vec3 L = -sunDirection, sun = sunLight();
    float NV = max(dot(N, V), 0.0), NL = dot(N, L), F = 0.02 + 0.98 * pow(1.0 - NV, 5.0), phase = forward(V);
    // Like cloud: bright toward the sun, the sky in its shade, and the sun
    // carried on through its thin parts toward the eye.
    vec3 scatter = skyFill(N) + sun * lit * (0.62 * max(NL, 0.0) + 0.14 * (NL * 0.5 + 0.5) + 0.8 * phase * (1.0 - 0.5 * thick));
    // Clear: the sky and square reflected, strongest at grazing angles; the
    // scene behind, bent, blurred and tinted; backlit, the sun bent through
    // its sides to the eye.
    vec3 clear = skyReflect(reflect(-V, N)) * F + sun * lit * phase * 0.45 * pow(1.0 - NV, 1.5);
    // (Mostly white water hides what lies behind it: skip reading it.)
    if (hasScene > 0.5 && aerated < 0.85) {
      vec2 shift = (viewMatrix * vec4(N, 0.0)).xy * bend;
      vec3 behind = textureLod(sceneColor, clamp(gl_FragCoord.xy / sceneResolution + shift, 0.002, 0.998), blur).rgb;
      clear += (behind * vec3(0.84, 0.95, 0.97) + vec3(0.0, 0.015, 0.025)) * (1.0 - F);
    } else if (hasScene > 0.5) clear += skyFill(N) * 0.6 * (1.0 - F);
    // Aerated water is translucent where it thins; layers of it come to its
    // own colour, never past it. Glints go on top in full: they are small,
    // and should reach white.
    float clearOpacity = hasScene > 0.5 ? 1.0 : min(F + 0.12, 1.0), whiteOpacity = 0.88 + 0.12 * thick;
    vec3 H = normalize(L + V), R = normalize(N + ripple);
    float spec = pow(max(dot(N, H), 0.0), 220.0) * 1.5 + pow(max(dot(R, H), 0.0), 500.0) * 5.0
      + spark * 4.0 * smoothstep(0.2, 0.8, dot(R, H));
    vec3 color = mix(soften(clear), soften(scatter) * whiteOpacity, aerated) + sun * spec * lit * (1.0 - 0.6 * aerated);
    return vec4(color * alpha, alpha * mix(clearOpacity, whiteOpacity, aerated));
  }
`;
const SOFT = /* glsl */`
  // Frame pixels per pixel drawn: 2 for the half-size soft spray.
  uniform vec2 fragScale;
  // Soft where it meets the scene, faded with the near scene's far fade and
  // right in front of the eye.
  float softly(float depth, float reach) {
    float a = clamp((fade.y - depth) / max(fade.y - fade.x, 1.0), 0.0, 1.0) * smoothstep(0.4, 1.2, depth);
    if (hasDepth < 0.5) return a;
    float z = texture2D(sceneDepth, gl_FragCoord.xy * fragScale / depthResolution).r;
    float surface = cameraRange.x * cameraRange.y / (cameraRange.y - z * (cameraRange.y - cameraRange.x));
    return a * smoothstep(0.0, reach, surface - depth);
  }
`;

// The jet itself, and the dome of heaving foam where it meets the pool. Its
// outline holds steady; only its surface churns: streaks fine around the
// jet and long along it, riding up at the water's own speed (they are noise
// in the time each part left the nozzle).
const TOP = APEX * 0.8, RINGS = 40, DOME = 8, AROUND = 24;
function columnGeometry() {
  const sweep = [], index = [];
  const part = (kind, rings) => {
    const base = sweep.length / 3;
    for (let k = 0; k <= rings; k++) for (let j = 0; j <= AROUND; j++) sweep.push(kind, k / rings, j / AROUND);
    for (let k = 0; k < rings; k++) for (let j = 0; j < AROUND; j++) {
      const a = base + k * (AROUND + 1) + j, b = a + AROUND + 1;
      index.push(a, b, a + 1, a + 1, b, b + 1);
    }
  };
  part(0, RINGS);
  part(1, DOME);
  return { sweep: new Float32Array(sweep), index };
}

const columnVertex = /* glsl */`
  attribute vec3 sweep; // part (0 the jet, 1 the dome), along it from the bottom, around
  uniform vec4 sources[5];
  uniform float levels[5];
  uniform float time;
  uniform vec3 sunDirection;
  varying vec3 vWorld, vNormal, vFlow;
  varying float vY, vAbove, vAlpha, vLit, vPart, vDepth;
  const float G = 9.81, APEX = ${APEX.toFixed(2)}, TOP = ${TOP.toFixed(2)}, TAU = 6.2831853;
  ${VISIBLE}
  ${SUN_THROUGH}
  void main() {
    int i = fountain(1);
    vec4 origin = sources[i];
    float launch = sqrt(2.0 * G * APEX), turn = sweep.z * TAU, level = levels[i] - origin.y;
    vec2 around = vec2(cos(turn), sin(turn));
    vec3 local, normal;
    if (sweep.x < 0.5) {
      // From the pool's surface up.
      float y = mix(level, TOP, sweep.y);
      float radius = 0.1 + 0.06 * y + 0.01 * sin(turn * 3.0 + y * 1.7);
      local = vec3(around.x * radius, y, around.y * radius);
      normal = vec3(around.x, -0.05, around.y);
      vFlow = vec3(around, time - (launch - sqrt(max(launch * launch - 2.0 * G * y, 0.0))) / G);
      vY = y;
    } else {
      // Heaving up around the jet's foot and spreading over the pool.
      float t = sweep.y, heave = 0.05 * sin(time * 5.3 + turn * 3.0) + 0.04 * sin(time * 7.9 - turn * 5.0 + t * 4.0);
      float radius = mix(0.95, 0.12, t);
      local = vec3(around.x * radius, level + (0.18 + heave) * sin(t * 1.5708), around.y * radius);
      normal = vec3(around.x * cos(t * 1.5708), 1.0, around.y * cos(t * 1.5708));
      vFlow = vec3(local.xz * 3.0, time * 1.6);
      vY = t;
    }
    vec3 world = origin.xyz + local;
    vWorld = world;
    vNormal = normalize(normal);
    vAbove = local.y - level;
    vAlpha = origin.w;
    vLit = 0.25 + 0.75 * sunThrough(local);
    vPart = sweep.x;
    vec4 view = viewMatrix * vec4(world, 1.0);
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
`;

const columnFragment = /* glsl */`
  uniform sampler2D sceneDepth;
  uniform vec2 depthResolution, cameraRange, fade;
  uniform float hasDepth, time;
  varying vec3 vWorld, vNormal, vFlow;
  varying float vY, vAbove, vAlpha, vLit, vPart, vDepth;
  const float TOP = ${TOP.toFixed(2)};
  ${NOISE3}
  ${SKY}
  ${WATER}
  ${SOFT}
  void main() {
    vec3 V = normalize(cameraPosition - vWorld), axis = normalize(vNormal);
    float facing = abs(dot(axis, V)), aerated, cover;
    vec3 N;
    if (vPart < 0.5) {
      // Streaks: fine around the jet, long along it; one layer rides with
      // the water, one slips along it, and fine bubbles churn through both.
      float a = noise3(vec3(vFlow.xy * 6.0, vFlow.z * 9.0)), b = noise3(vec3(vFlow.xy * 17.0 + 3.0, vFlow.z * 5.0 - time * 1.3));
      float c = noise3(vec3(vFlow.xy * 34.0 + 7.0, vFlow.z * 23.0));
      vec3 tangent = normalize(cross(vec3(0.0, 1.0, 0.0), axis) + vec3(1e-4, 0.0, 0.0));
      N = normalize(axis + tangent * (b - 0.5) * 0.9 + vec3(0.0, c - 0.5, 0.0) * 0.7);
      // Glassy where it leaves the water; above, churning white water.
      aerated = smoothstep(0.15, 0.9, vAbove + (a - 0.5) * 0.5) * (0.85 + 0.15 * c);
      cover = smoothstep(0.02, 0.3, facing + (a - 0.5) * 0.4);
      cover *= mix(1.0, smoothstep(0.15, 0.5, 0.6 * a + 0.4 * b), 0.5 * aerated);
      cover *= (1.0 - smoothstep(0.6 * TOP, TOP, vY + (b - 0.5) * 1.2)) * 0.95;
    } else {
      // Foam boiling on the dome, torn open to the water beneath, thinning
      // to nothing at its edge.
      float a = noise3(vFlow), b = noise3(vFlow * 2.3 + 7.0), c = noise3(vFlow * 5.1 + 3.0);
      N = normalize(axis + vec3(a - 0.5, 0.0, b - 0.5) * 1.2 + vec3(c - 0.5, 0.0, a - 0.5) * 0.6);
      aerated = 0.55 + 0.45 * smoothstep(0.3, 0.7, b * 0.6 + c * 0.4);
      cover = smoothstep(0.0, 0.5, vY) * mix(0.0, 0.7, smoothstep(0.45, 0.8, a * 0.5 + b * 0.3 + c * 0.2));
    }
    cover *= vAlpha * softly(vDepth, 0.15);
    if (cover < 0.002) discard;
    // The jet is the brightest water in the fountain: little of the
    // umbrella's shade reaches it.
    gl_FragColor = water(N, V, max(vLit, 0.8), aerated, facing, cover, 0.06, 1.2, vec3(0.0), 0.0);
  }
`;

// The umbrella's ropes and strands: one ribbon each, through points along
// the path its water has taken since leaving the crown, so it bends and
// shades smoothly. Points crowd toward the crown, where it bends most.
function ribbonGeometry() {
  const strip = [], index = [];
  for (let k = 0; k <= STRIP; k++) strip.push(k / STRIP, -1, k / STRIP, 1);
  for (let k = 0; k < STRIP; k++) { const a = k * 2; index.push(a, a + 1, a + 2, a + 1, a + 3, a + 2); }
  return { strip: new Float32Array(strip), index };
}

const ribbonVertex = /* glsl */`
  attribute vec2 strip; // along it from the crown, across it
  attribute float rope; // its number
  uniform vec4 sources[5];
  uniform float levels[5], lows[5]; // the tier's pool and the outer pool
  uniform float time, side;
  uniform vec3 wind, sunDirection;
  uniform vec2 depthResolution;
  varying vec3 vWorld, vSide, vAlong, vFlow;
  varying vec4 vRope;
  varying float vAcross, vHalf, vSpeed, vFall, vLit, vCover, vDepth, vLevel, vPixel;
  const float G = 9.81, APEX = ${APEX.toFixed(2)}, TAU = 6.2831853, ROPES = ${ROPES.toFixed(1)}, STRANDS = ${STRANDS.toFixed(1)};
  const float ARCS = ${ARCS.toFixed(1)}, BUNDLES = ${BUNDLES.toFixed(1)}, LIFE = ${ROPE_LIFE.toFixed(2)};
  uniform float perFountain;
  ${VISIBLE}
  ${HASH}
  ${ROPE}
  ${SUN_THROUGH}
  void main() {
    int i = fountain(int(perFountain));
    vec4 source = sources[i];
    float j = rope, salt = float(i) * 2.17, pool = ropeOuter(j) ? lows[i] : levels[i], level = pool - source.y;
    // The whole ribbon in one half, by the way it falls now.
    vec2 dir; vec4 start;
    ropeStart(j, salt, time, dir, start);
    if ((dot(dir, cameraPosition.xz - source.xz) >= 0.0 ? 1.0 : 0.0) != side) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
    float age = LIFE * pow(strip.x, 1.5), born = time - age;
    ropeStart(j, salt, born, dir, start);
    vec3 p = ropeAt(dir, start, age);
    // Along the path: where water that left a moment earlier is now.
    vec2 dir1; vec4 start1;
    ropeStart(j, salt, born - 0.01, dir1, start1);
    vec3 along = ropeAt(dir1, start1, age + 0.01) - p;
    along = length(along) > 1e-6 ? normalize(along) : vec3(0.0, -1.0, 0.0);
    vec3 center = source.xyz + p, toEye = normalize(cameraPosition - center);
    vec3 across = cross(along, toEye);
    across = length(across) > 1e-5 ? normalize(across) : vec3(1.0, 0.0, 0.0);
    float depth = -(viewMatrix * vec4(center, 1.0)).z;
    float pixel = 2.0 * max(depth, 0.1) / (projectionMatrix[1][1] * depthResolution.y);
    vec4 look = ropeLook(j, salt);
    // Wide enough for the stream, its waver and its drops, and at least a pixel.
    float halfWidth = max(2.4 * look.x, pixel);
    vec3 world = center + across * strip.y * halfWidth;
    vWorld = world;
    vSide = across;
    vAlong = along;
    vFlow = vec3(born, j, salt);
    vRope = look;
    vAcross = strip.y;
    vHalf = halfWidth;
    vSpeed = length(vec3(dir.x * start.y, start.w - G * age, dir.y * start.y) + wind * 0.6 * age);
    vFall = clamp((start.z - p.y) / max(start.z - level, 1.0), 0.0, 1.0);
    vLit = 0.25 + 0.75 * sunThrough(world - source.xyz);
    vCover = smoothstep(0.0, 0.05, age);
    vPixel = pixel;
    vLevel = pool;
    vec4 view = viewMatrix * vec4(world, 1.0);
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
`;

const ribbonFragment = /* glsl */`
  uniform sampler2D sceneDepth;
  uniform vec2 depthResolution, cameraRange, fade;
  uniform float hasDepth, time;
  varying vec3 vWorld, vSide, vAlong, vFlow;
  varying vec4 vRope;
  varying float vAcross, vHalf, vSpeed, vFall, vLit, vCover, vDepth, vLevel, vPixel;
  const float TAU = 6.2831853;
  ${HASH}
  ${SKY}
  ${WATER}
  ${SOFT}
  void main() {
    float f = vFlow.x, j = vFlow.y, salt = vFlow.z + j * 1.7, fall = vFall, px = vPixel;
    // It carries the same water as it speeds up, so it thins; it swells and
    // necks as its water passes, and wavers.
    float r = vRope.x * sqrt(1.5 / max(vSpeed, 1.5)) * (0.8 + 0.4 * noise1(f * 4.0 + salt));
    float across = vAcross * vHalf + 0.5 * r * (noise1(f * 17.0 - time * 1.7 + salt * 5.3) - 0.5);
    // Past its break point it beads and parts into drops (Plateau-Rayleigh):
    // a bead every few diameters, each the water that left the crown in that
    // moment, so the drops ride with it; some beads come to nothing.
    float A = smoothstep(vRope.z, vRope.z + 0.25, fall);
    float cellF = 0.75 * vRope.x, w = f / cellF + hash1(j * 3.3 + salt);
    float cell = floor(w), u = w - cell - 0.5, along = u * cellF * vSpeed;
    float rb = min(1.9 * r * (0.6 + 0.8 * hash1(cell * 1.37 + salt)), 0.009) * smoothstep(0.0, 0.35, A) * step(0.15, hash1(cell * 2.9 + salt * 1.3));
    // A drop, smeared over a sixtieth of a second.
    float halfLen = rb + vSpeed / 120.0;
    float stream = r * (1.0 + 0.7 * A * cos(TAU * u)) * (1.0 - smoothstep(0.55, 0.95, A));
    // How much of this pixel each covers, for the pixel's own width.
    float inStream = clamp((min(across + 0.5 * px, stream) - max(across - 0.5 * px, -stream)) / px, 0.0, 1.0);
    float e = length(vec2(along / max(halfLen, 1e-5), across / max(rb, 1e-5)));
    float inDrop = rb > 1e-5 ? (1.0 - smoothstep(1.0 - 0.5 * px / rb, 1.0 + 0.5 * px / rb, e)) * min(1.0, 2.0 * rb / px) : 0.0;
    float cover = max(inStream, inDrop) * vCover * vRope.y;
    // Faded softly into the pool and where it passes behind the bowl.
    cover *= smoothstep(vLevel - 0.05, vLevel + 0.25, vWorld.y) * softly(vDepth, 0.3);
    if (cover < 0.002) discard;
    // Round across, like a rod of water; a drop is round all over.
    bool drop = inDrop > inStream;
    float nx = clamp(across / max(drop ? rb : stream, 1e-5), -1.0, 1.0), ny = drop ? clamp(along / halfLen, -1.0, 1.0) : 0.0;
    vec3 V = normalize(cameraPosition - vWorld);
    vec3 N = normalize(vSide * nx + vAlong * ny * 0.6 + V * sqrt(max(1.0 - nx * nx - ny * ny * 0.36, 0.04)));
    // White where air churns in it, in patches, and all through at the
    // crown; its drops are clear. Far off, bubbles and glints too small to
    // draw average into white.
    float aerated = clamp(vRope.w * (0.55 + 0.8 * noise1(f * 6.0 + salt * 2.1)) + 0.7 * (1.0 - smoothstep(0.0, 0.2, fall)), 0.0, 1.0);
    if (drop) aerated *= 0.35;
    aerated = max(aerated, 0.55 * smoothstep(0.015, 0.06, px));
    // Ripples riding with the water catch the sun.
    vec3 ripple = (vSide * (noise1(f * 140.0 + across / max(r, 1e-3) + salt) - 0.5) + vAlong * (noise1(f * 97.0 + salt * 3.0) - 0.5)) * 2.6;
    gl_FragColor = water(N, V, vLit, aerated, sqrt(max(1.0 - nx * nx, 0.0)), cover, 0.04, 2.0, ripple, 0.0);
    // Glints: here and there a facet of the tumbling surface turns the sun to
    // the eye for a moment. Each is a point a pixel and a half across,
    // however far, more of them looking toward the sun.
    vec2 g = vec2(f / 0.035, (vAcross + 1.0) * 2.0), gi = floor(g), gp = (fract(g) - 0.5) * vec2(0.035 * vSpeed, 0.5 * vHalf) / px;
    float chance = (0.04 + 0.08 * forward(V)) * vLit * vLit;
    float glint = step(1.0 - chance, hash1(gi.x * 7.13 + gi.y * 1.7 + floor(time * 14.0 + hash1(gi.x) * 3.0) * 0.37 + salt));
    gl_FragColor.rgb += sunLight() * 2.5 * glint * (1.0 - smoothstep(0.4, 1.2, length(gp))) * min(cover * 3.0, 1.0);
  }
`;

// The overflow: a sheet pouring over the tier's lip and running down its
// sloping outer face into the outer pool, picking up speed, a little apart
// from the stone. Each side of the tier is its own strip, so that a whole
// side falls in one half, beyond or before the jet's axis from the eye.
const FILM_ACROSS = 8, FILM_DOWN = 10;
function filmGeometry() {
  const film = [], index = [];
  for (let side = 0; side < TIER.sides; side++) {
    const base = film.length / 3;
    for (let k = 0; k <= FILM_DOWN; k++) for (let j = 0; j <= FILM_ACROSS; j++) film.push(side, j / FILM_ACROSS, k / FILM_DOWN);
    for (let k = 0; k < FILM_DOWN; k++) for (let j = 0; j < FILM_ACROSS; j++) {
      const a = base + k * (FILM_ACROSS + 1) + j, b = a + FILM_ACROSS + 1;
      index.push(a, b, a + 1, a + 1, b, b + 1);
    }
  }
  return { film: new Float32Array(film), index };
}

const filmVertex = /* glsl */`
  attribute vec3 film; // side of the tier, along it, down its face from the lip
  uniform vec4 sources[5];
  uniform float levels[5], lows[5];
  uniform float time, side;
  uniform vec3 sunDirection;
  varying vec3 vWorld, vNormal, vDown;
  varying vec2 vFlow;
  varying float vS, vLit, vDepth;
  const float G = 9.81, APEX = ${APEX.toFixed(2)}, TAU = 6.2831853, SIDES = ${TIER.sides.toFixed(1)};
  const float LIP = ${TIER.lip.toFixed(2)}, FOOT = ${TIER.foot.toFixed(2)};
  ${VISIBLE}
  ${SUN_THROUGH}
  void main() {
    int i = fountain(1);
    vec4 origin = sources[i];
    float middle = (film.x + 0.5) * TAU / SIDES;
    vec2 out_ = vec2(cos(middle), sin(middle));
    if ((dot(out_, cameraPosition.xz - origin.xz) >= 0.0 ? 1.0 : 0.0) != side) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
    // The lip's stone lies just under the tier's pool; the face meets the
    // outer pool at its foot.
    float top = levels[i] - 0.01 - origin.y, bottom = lows[i] - origin.y;
    float a0 = film.x * TAU / SIDES, a1 = a0 + TAU / SIDES;
    vec2 along = mix(vec2(cos(a0), sin(a0)), vec2(cos(a1), sin(a1)), film.y) * mix(LIP, FOOT, film.z);
    // Out and up from the face, and down it.
    float run = (FOOT - LIP) * cos(3.14159265 / SIDES), drop = top - bottom;
    vec3 normal = normalize(vec3(out_ * drop, run).xzy), down = normalize(vec3(out_ * run, -drop).xzy);
    vec3 local = vec3(along.x, mix(top, bottom, film.z), along.y) + normal * 0.018;
    // The water leaves the lip slowly and speeds down the face (gravity
    // along the slope, less the stone's drag): when the water here left it.
    float slope = length(vec2(run, drop)), accel = G * drop / slope * 0.6, v0 = 0.5;
    float tau = (sqrt(v0 * v0 + 2.0 * accel * film.z * slope) - v0) / accel;
    vec3 world = origin.xyz + local;
    vWorld = world;
    vNormal = normal;
    vDown = down;
    vFlow = vec2((film.x + film.y) * LIP * TAU / SIDES, time - tau);
    vS = film.z;
    vLit = 0.25 + 0.75 * sunThrough(local);
    vec4 view = viewMatrix * vec4(world, 1.0);
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
`;

const filmFragment = /* glsl */`
  uniform sampler2D sceneDepth;
  uniform vec2 depthResolution, cameraRange, fade;
  uniform float hasDepth, time;
  varying vec3 vWorld, vNormal, vDown;
  varying vec2 vFlow;
  varying float vS, vLit, vDepth;
  ${NOISE3}
  ${SKY}
  ${WATER}
  ${SOFT}
  void main() {
    // Rivulets: fine across the face, long down it, riding down with the
    // water (born is when the water here left the lip; it runs at 1-3 m/s).
    float u = vFlow.x, born = vFlow.y;
    float a = noise3(vec3(u * 9.0, born * 1.5, 0.0)), b = noise3(vec3(u * 26.0 + 3.0, born * 3.0, 5.0)), c = noise3(vec3(u * 60.0, born * 6.0 - time * 0.5, 9.0));
    vec3 V = normalize(cameraPosition - vWorld), across = normalize(cross(vNormal, vDown));
    // Over the lip the sheet curls, smooth and glassy; below it ripples.
    float curl = 1.0 - smoothstep(0.0, 0.14, vS);
    vec3 N = normalize(mix(vNormal + vDown * (a - 0.5) * 0.5 + across * (b - 0.5) * 0.6, vec3(0.0, 1.0, 0.0) + vNormal * 0.4, curl));
    // It thins into rivulets low down, and churns white where it meets the pool.
    float cover = mix(0.9, 0.55 + 0.45 * smoothstep(0.35, 0.6, 0.6 * a + 0.4 * b), smoothstep(0.2, 0.8, vS));
    float aerated = clamp(0.05 + 0.35 * smoothstep(0.7, 0.9, b * 0.6 + c * 0.4) * smoothstep(0.2, 0.7, vS) + 0.9 * smoothstep(0.7, 0.95, vS) * (0.6 + 0.4 * a), 0.0, 1.0);
    vec3 ripple = (across * (c - 0.5) * 1.6 + vDown * (b - 0.5) * 0.6) * 1.4;
    cover *= softly(vDepth, 0.006) * (1.0 - smoothstep(0.96, 1.0, vS));
    if (cover < 0.002) discard;
    gl_FragColor = water(N, V, vLit, aerated, 0.4, cover, 0.025, 1.0, ripple, 0.0);
  }
`;

// The soft spray, drawn at half size, laid over the frame: each pixel takes
// the four nearest drawn pixels weighted by distance, and by how close their
// opaque depth is to its own, so spray never bleeds across an edge onto
// whatever stands in front of it. Fine drops scatter what lies behind them
// mostly onward, so spray adds light far more than it hides: it is laid on
// as light (screened: brightening dark things, hardly sunlit ones, never
// past white), dimming what lies behind only a little (SPRAY_HIDES).
const compositeFragment = /* glsl */`
  uniform sampler2D sprayMap, sceneDepth;
  uniform vec2 sprayResolution, depthResolution, cameraRange;
  uniform float enabled;
  float linearDepth(float z) { return cameraRange.x * cameraRange.y / (cameraRange.y - z * (cameraRange.y - cameraRange.x)); }
  void main() {
    vec2 uv = gl_FragCoord.xy / depthResolution;
    if (enabled < 0.5 || texture2D(sprayMap, uv).a < 0.002) discard;
    vec2 pixel = uv * sprayResolution - 0.5, base = floor(pixel), f = fract(pixel);
    float depth = linearDepth(texture2D(sceneDepth, uv).r), total = 0.0;
    vec4 color = vec4(0.0);
    for (int y = 0; y < 2; y++) for (int x = 0; x < 2; x++) {
      vec2 offset = vec2(float(x), float(y)), at = (base + offset + 0.5) / sprayResolution;
      vec2 weight = mix(1.0 - f, f, offset);
      float w = weight.x * weight.y * exp(-abs(depth - linearDepth(texture2D(sceneDepth, at).r)) * 4.0 / max(1.0, depth * 0.02)) + 1e-4;
      color += texture2D(sprayMap, at) * w;
      total += w;
    }
    color /= total;
    gl_FragColor = vec4(min(color.rgb, vec3(1.0)), color.a * ${SPRAY_HIDES.toFixed(2)});
  }
`;

const vertexShader = /* glsl */`
  attribute vec2 corner;
  attribute vec3 seed; // kind, phase within its life, salt (a splash's: its event, and part in eighths)
  uniform vec4 sources[5]; // nozzle; w is 1 while the fountain is drawn
  uniform float levels[5], lows[5]; // the tier's pool and the outer pool
  uniform float time, side;
  uniform vec3 wind;
  uniform vec2 depthResolution;
  varying vec2 vUv;
  varying vec4 vCell; // atlas cell: offset, size; size 0 is a drop, -1 a splash's crown
  varying vec4 vLook; // coverage, erosion (a ring's radius), how aerated (a ring's width), kind
  varying vec2 vFlow; // a core streak: when its water left the nozzle; a salt
  varying vec3 vWorld, vSide, vUp, vTint;
  varying vec2 vLight; // sunlight reaching it through the plume, glint
  varying float vLevel, vDepth;
  const float G = 9.81, APEX = ${APEX.toFixed(2)}, TAU = 6.2831853, ROPES = ${ROPES.toFixed(1)}, STRANDS = ${STRANDS.toFixed(1)};
  const float ARCS = ${ARCS.toFixed(1)}, BUNDLES = ${BUNDLES.toFixed(1)};
  // The tier's lip and the overflow's foot, and the outer pool's wall at the water.
  const float LIP = ${TIER.lip.toFixed(2)}, FOOT = ${TIER.foot.toFixed(2)}, WALL = 6.2;
  const int MIST = 0, FROTH = 1, SKIRT = 2, SPRAY = 3, SPLASH = 4, DROP = 5, CLUMP = 6, CORE = 7, RING = 8, FOAM = 9;
  // How a particle faces: along its motion, the eye (turned by spin), the eye but upright, or flat on the pool.
  const int ALONG = 0, EYE = 1, UPRIGHT = 2, FLAT = 3;
  ${HASH}
  ${SKY}
  ${ROPE}
  ${SUN_THROUGH}
  uniform float perFountain;
  ${VISIBLE}
  // A stream to follow, for u in [0, 1): an outer arc for the first share
  // of u, one of the ropes and strands for the rest.
  float pick(float u, float share) {
    return u < share ? ROPES + STRANDS + floor(u / share * ARCS) : floor((u - share) / (1.0 - share) * (ROPES + STRANDS));
  }
  void main() {
    int i = fountain(int(perFountain)), kind = int(seed.x + 0.5);
    vec4 source = sources[i];
    float lives[10] = float[10](${LIVES.map(l => l.toFixed(2)).join(', ')});
    float life = lives[kind];
    // level: the tier's pool, low: the outer pool, pool: the one this
    // particle belongs to (all from the nozzle).
    float launch = sqrt(2.0 * G * APEX), level = levels[i] - source.y, low = lows[i] - source.y, pool = level, salt = float(i) * 2.17;
    float clock = time + fract(seed.y + salt) * life, cycle = floor(clock / life), age = clock - cycle * life, t = age / life;
    // A splash's parts share its event (r): where it strikes, how hard.
    vec4 r = hash4((kind == SPLASH ? floor(seed.z) : seed.z) * 97.13 + cycle * 3.71 + salt), s = hash4(seed.z * 41.7 + cycle * 1.93 + 17.3 + salt);
    vec2 dir = vec2(cos(r.x * TAU), sin(r.x * TAU));
    vec3 p, v = vec3(0.0), shape = vec3(0.0, 1.0, 0.0);
    vec2 size;
    // trail: seconds of its motion it is drawn along; cap: the most that adds.
    float coverage, erode = 0.0, aerated = 0.0, trail = 0.0, cap = 0.0, glint = 0.0, spin = 0.0, born = 0.0;
    int facing = ALONG;
    bool drop = false;
    vec4 cell = vec4(0.0), mass = vec4(step(0.5, s.x) * 0.5, 0.0, 0.5, 1.0);
    if (kind == CORE) {
      // Streaks of water racing up the jet's surface.
      float up = launch * (0.95 + 0.06 * r.y), lat = 0.2 * r.z, d = 0.08 + 0.03 * r.w + lat * age;
      p = vec3(dir.x * d, up * age - 0.5 * G * age * age, dir.y * d);
      v = vec3(dir.x * lat, up - G * age, dir.y * lat);
      size = vec2(0.025 + 0.02 * s.y, 0.0);
      trail = 1.0 / 25.0; cap = 0.35;
      coverage = 0.75 * smoothstep(0.0, 0.04, age) * (1.0 - smoothstep(0.5 * life, life, age));
      aerated = 0.3 + 0.6 * smoothstep(level + 0.2, level + 1.2, p.y);
      born = time - age;
      glint = 1.0;
    } else if (kind == CLUMP) {
      // Aerated water boiling up the column and churning in the crown.
      float up = launch * (0.86 + 0.13 * r.y), lat = 0.6 * pow(r.z, 1.5), apex = up / G, rise = age / apex;
      float d = 0.06 * r.w + lat * (1.0 - exp(-1.3 * age)) / 1.3 + (0.4 + 0.6 * s.w) * max(age - 0.5 * apex, 0.0);
      p = vec3(dir.x * d, up * age - 0.5 * G * age * age, dir.y * d);
      p.xz += wind.xz * max(age - 0.5 * apex, 0.0) * 0.3;
      v = vec3(0.0, up - G * age, 0.0);
      size = vec2(mix(0.1, 0.5, smoothstep(0.0, 1.0, rise)) * (1.0 + 0.3 * smoothstep(1.0, 1.6, rise)) * (0.75 + 0.5 * s.y));
      facing = EYE;
      spin = (s.z - 0.5) * 0.6;
      trail = 0.04; cap = 0.25;
      coverage = mix(0.45, 0.7, smoothstep(0.6, 1.0, rise)) * smoothstep(0.0, 0.05, age) * smoothstep(0.1, 0.35, rise) * (1.0 - smoothstep(1.1, 1.6, rise));
      erode = 0.1 + 0.25 * smoothstep(0.7, 1.2, rise);
      aerated = 0.95;
      cell = mass;
    } else if (kind == DROP) {
      if (r.y < 0.65) {
        // Shaken off a stream, with a kick from the water around it.
        float j = pick(s.x, 0.15), a0 = 0.05 + 0.75 * s.y;
        if (ropeOuter(j)) pool = low;
        vec2 rd; vec4 st; ropeStart(j, salt, time - age - a0, rd, st);
        vec3 v0 = vec3(rd.x * st.y, st.w - G * a0, rd.y * st.y) + vec3(dir.x, 0.0, dir.y) * (0.3 + 0.9 * r.z) + vec3(0.0, 0.2 + 0.8 * r.w, 0.0);
        p = ropeAt(rd, st, a0) + v0 * age;
        v = v0;
      } else {
        // Thrown from the crown.
        float lat = 0.6 + 1.6 * r.w, up = 0.5 + 1.5 * r.z;
        p = vec3(dir.x * (0.2 + lat * age), APEX * 0.88 + up * age, dir.y * (0.2 + lat * age));
        v = vec3(dir.x * lat, up, dir.y * lat);
      }
      p.y -= 0.5 * G * age * age;
      v.y -= G * age;
      size = vec2(0.0025 + 0.0035 * s.z, 0.0);
      trail = 1.0 / 60.0; cap = 0.07;
      coverage = smoothstep(0.0, 0.08, age);
      drop = true;
      // Some catch the sun, flashing as they wobble and turn.
      glint = step(0.5, s.w) * pow(0.5 + 0.5 * sin(time * (7.0 + 9.0 * s.z) + s.w * 40.0), 10.0);
    } else if (kind == SPRAY) {
      // Fine spray torn from a rope: drag holds it back a little and lets it
      // drift out, so it falls with the rope as a soft veil of streaks,
      // smeared along its fall, widening.
      float j = floor(s.x * ROPES);
      vec2 rd; vec4 st; ropeStart(j, salt, time - age, rd, st);
      p = ropeAt(rd, st, age) + vec3(rd.x, 0.0, rd.y) * (0.1 + 0.3 * r.z) * age + vec3(0.0, 1.2 * r.w * age * age, 0.0);
      v = vec3(rd.x * st.y, st.w - G * age, rd.y * st.y);
      size = vec2(mix(0.06, 0.2, t) * (0.8 + 0.4 * s.y), 0.0);
      trail = 0.1; cap = 0.7;
      coverage = 0.22 * smoothstep(0.0, 0.17, t) * (1.0 - smoothstep(0.65, 1.0, t));
      shape = normalize(vec3(p.x, 0.25, p.z));
      cell = mass;
    } else if (kind == FROTH) {
      // Spray thrown off where the jet stalls and breaks at the crown, hanging
      // around it and settling out over the umbrella.
      float out_ = 0.3 + 1.0 * r.z, d = 0.2 + 1.2 * r.y + out_ * age;
      p = vec3(dir.x * d, APEX * (0.74 + 0.24 * s.x) + 0.6 * r.w * age - 0.9 * age * age, dir.y * d) + wind * age * 0.4;
      size = vec2(mix(0.35, 1.0, t) * (0.7 + 0.6 * s.y));
      facing = EYE;
      spin = s.z * TAU + age * 0.3;
      coverage = 0.34 * pow(sin(3.14159 * t), 1.2);
      shape = normalize(vec3(p.x, (p.y - APEX * 0.75) * 0.9, p.z));
      cell = mass;
    } else if (kind == SKIRT) {
      // Spray thrown up where the water lands, hanging over the pool.
      float j = pick(s.x, 0.25);
      if (ropeOuter(j)) pool = low;
      vec2 rd; vec3 at = ropeLanding(j, salt, time - age - 1.25, pool, rd);
      // A metre at most: up to 1.4 m/s, slowed as it spreads.
      float up = 0.5 + 0.9 * r.w, out_ = 0.15 + 0.5 * r.z;
      p = vec3(at.x + rd.x * out_ * age, pool + 0.05 + max(up * age - 0.5 * age * age, 0.0), at.z + rd.y * out_ * age) + wind * age * 0.3;
      size = vec2(mix(0.15, 0.55, t) * (0.7 + 0.6 * s.y));
      facing = EYE;
      spin = s.z * TAU;
      coverage = 0.24 * pow(sin(3.14159 * t), 1.2);
      shape = normalize(vec3(p.x, 0.7, p.z));
      cell = mass;
    } else if (kind == MIST) {
      // A faint haze over the pools, drifting downwind, never above a metre or so.
      float d = 1.0 + 3.5 * r.z;
      p = vec3(dir.x * d, (d > LIP ? low : level) + 0.3 + 0.6 * r.w + 0.08 * age, dir.y * d) + wind * age * 0.8;
      size = vec2(mix(0.9, 1.7, t) * (0.8 + 0.4 * s.y));
      facing = EYE;
      spin = s.z * TAU + age * 0.1;
      coverage = 0.07 * pow(sin(3.14159 * t), 1.5);
      shape = normalize(vec3(p.x, 0.6, p.z));
      cell = mass;
    } else if (kind == SPLASH) {
      // A drop striking the pool where a stream lands: a crown thrown up
      // around it, a spike from its middle as the crown falls, a puff of
      // white water, and droplets from the crown's rim.
      int part = int(fract(seed.z) * 8.0 + 0.5);
      float j = pick(r.y, 0.3);
      if (ropeOuter(j)) pool = low;
      vec2 rd; vec3 at = ropeLanding(j, salt, time - age - 1.25, pool, rd);
      if (part == 0) {
        float width = (0.04 + 0.06 * r.z) * (0.5 + t), height = (0.05 + 0.09 * r.w) * sin(3.14159 * min(t * 1.4, 1.0));
        p = vec3(at.x, pool + 0.5 * height, at.z);
        size = vec2(width, 0.5 * height);
        facing = UPRIGHT;
        coverage = 0.8 * step(0.005, height);
        erode = t;
        aerated = 0.25;
        cell = vec4(0.0, 0.0, -1.0, 0.0);
      } else if (part == 1) {
        float rise = 1.0 + 1.0 * r.w, a = max(age - 0.12, 0.0), h = max(rise * a - 0.5 * G * a * a, 0.0);
        p = vec3(at.x, pool + 0.5 * h, at.z);
        v = vec3(0.0, 1.0, 0.0);
        size = vec2(0.005 + 0.004 * r.z, 0.5 * h);
        coverage = 0.8 * step(0.01, h);
        glint = 0.6;
      } else if (part == 2) {
        p = vec3(at.x, pool + 0.04 + 0.12 * sin(3.14159 * t), at.z);
        size = vec2(mix(0.06, 0.22, sqrt(t)) * (0.8 + 0.5 * r.z));
        facing = EYE;
        spin = s.w * TAU;
        coverage = 0.6 * smoothstep(0.0, 0.1, t) * (1.0 - smoothstep(0.4, 1.0, t));
        erode = 0.25 + 0.4 * t;
        aerated = 0.9;
        cell = mass;
      } else {
        float a = (float(part) - 3.0) / 5.0 * TAU + r.z * TAU + (s.x - 0.5) * 0.8, up = 1.3 + 1.0 * s.y, speed = 0.3 + 0.4 * s.z;
        vec2 out_ = vec2(cos(a), sin(a));
        p = vec3(at.x + out_.x * (0.04 + speed * age), pool + up * age - 0.5 * G * age * age, at.z + out_.y * (0.04 + speed * age));
        v = vec3(out_.x * speed, up - G * age, out_.y * speed);
        size = vec2(0.003 + 0.003 * s.w, 0.0);
        trail = 1.0 / 60.0; cap = 0.05;
        coverage = step(pool, p.y);
        drop = true;
        glint = step(0.4, s.x) * 0.9;
      }
    } else if (kind == RING) {
      // A ripple spreading from a strike.
      float j = pick(r.y, 0.3);
      if (ropeOuter(j)) pool = low;
      vec2 rd; vec3 at = ropeLanding(j, salt, time - age - 1.25, pool, rd);
      float radius = 0.04 + 0.5 * pow(t, 0.6);
      p = vec3(at.x, pool + 0.01, at.z);
      size = vec2(radius + 0.08);
      facing = FLAT;
      erode = radius / size.x;
      aerated = (0.025 + 0.035 * t) / size.x;
      coverage = (1.0 - t) * (1.0 - t) * 0.9;
    } else {
      // Foam riding outward from where the water lands, thinning as its
      // bubbles burst: in the tier out to its lip, in the outer pool from
      // the arcs' landings and from the overflow's foot toward the wall.
      float j = pick(r.y, 0.45);
      bool outer = ropeOuter(j);
      if (outer) pool = low;
      vec2 rd; vec3 at = ropeLanding(j, salt, time - age - 1.25, pool, rd);
      if (outer && s.x < 0.5) at.xz = rd * FOOT * (0.94 + 0.04 * s.y);
      vec2 radial = normalize(at.xz + vec2(1e-4, 0.0));
      float limit = outer ? WALL : LIP - 0.15, reach = min(length(at.xz) + (0.12 + 0.25 * r.z) * age, limit);
      p = vec3(radial.x * reach, pool + 0.012, radial.y * reach);
      size = vec2(mix(0.12, 0.45, sqrt(t)) * (0.7 + 0.6 * s.y));
      facing = FLAT;
      spin = s.z * TAU + age * (s.w - 0.5) * 0.4;
      erode = 0.15 + 0.6 * t;
      coverage = 0.9 * smoothstep(0.0, 0.08, t) * (1.0 - smoothstep(0.6, 1.0, t)) * (1.0 - smoothstep(limit - 0.15, limit, reach));
    }
    // Only this draw's half, beyond or before the axis; what lies on the
    // pool is drawn in one.
    vec2 eye = cameraPosition.xz - source.xz;
    if (side > -0.5 && (dot(p.xz, eye) >= 0.0 ? 1.0 : 0.0) != side) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
    vec3 center = source.xyz + p, toEye = normalize(cameraPosition - center);
    vec3 right = vec3(viewMatrix[0][0], viewMatrix[1][0], viewMatrix[2][0]);
    vec3 up = vec3(viewMatrix[0][1], viewMatrix[1][1], viewMatrix[2][1]);
    float depth = -(viewMatrix * vec4(center, 1.0)).z;
    // Metres per pixel there.
    float pixel = 2.0 * max(depth, 0.1) / (projectionMatrix[1][1] * depthResolution.y);
    vec3 axisX, axisY;
    vec2 extent = vec2(size.x, size.y > 0.0 ? size.y : size.x);
    float seen = 0.0;
    if (facing == ALONG) {
      // Along its motion as the eye sees it, smeared by its speed.
      vec3 across = v - dot(v, toEye) * toEye;
      seen = length(across);
      axisY = seen > 1e-4 ? across / seen : up;
      axisX = normalize(cross(axisY, toEye));
      extent.y += min(0.5 * trail * seen, cap);
    } else if (facing == UPRIGHT) {
      axisY = vec3(0.0, 1.0, 0.0);
      axisX = normalize(cross(axisY, toEye) + vec3(1e-5, 0.0, 0.0));
    } else if (facing == FLAT) {
      axisX = vec3(cos(spin), 0.0, sin(spin));
      axisY = vec3(-sin(spin), 0.0, cos(spin));
    } else {
      float c = cos(spin), sn = sin(spin);
      axisX = right * c + up * sn;
      axisY = up * c - right * sn;
      extent.y += min(0.5 * trail * abs(v.y), cap);
    }
    // Nothing thinner than about a pixel: drawn that wide and fainter. Drops
    // never grow past a few pixels, however near.
    vec2 shown = max(extent, vec2(0.7 * pixel));
    if (drop) shown = min(shown, vec2(1.5, 9.0) * pixel);
    float area = min(1.0, extent.x * extent.y / (shown.x * shown.y));
    coverage *= area;
    // A drop passes on how much of what is drawn it fills, so its glint keeps its light.
    if (drop) erode = area;
    vec3 world = center + axisX * corner.x * shown.x + axisY * corner.y * shown.y;
    vUv = corner * 0.5 + 0.5;
    vCell = cell;
    vLook = vec4(coverage * source.w, erode, aerated, float(kind));
    // Water further along its motion left earlier.
    vFlow = vec2(born - corner.y * shown.y / max(seen, 1e-3), salt + seed.z * 13.0);
    vWorld = world;
    vSide = axisX;
    vUp = axisY;
    float lit = 0.25 + 0.75 * sunThrough(world - source.xyz);
    vLight = vec2(lit, glint);
    // Spray and foam are lit here, by the shape they hang in; a ripple's
    // crest shows the sky.
    vTint = kind == RING ? skyAbove() : sprayLight(shape, toEye, lit);
    vLevel = pool + source.y;
    vec4 view = viewMatrix * vec4(world, 1.0);
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
`;

const fragmentShader = /* glsl */`
  uniform sampler2D atlas, foamMap, sceneDepth;
  uniform vec2 depthResolution, cameraRange, fade;
  uniform float hasDepth, hasFoam, time;
  varying vec2 vUv;
  varying vec4 vCell, vLook;
  varying vec2 vFlow;
  varying vec3 vWorld, vSide, vUp, vTint;
  varying vec2 vLight;
  varying float vLevel, vDepth;
  const int MIST = 0, FROTH = 1, SKIRT = 2, SPRAY = 3, SPLASH = 4, DROP = 5, CLUMP = 6, CORE = 7, RING = 8, FOAM = 9;
  ${HASH}
  ${SKY}
  ${WATER}
  ${SOFT}
  void main() {
    int kind = int(vLook.w + 0.5);
    vec2 q = vUv * 2.0 - 1.0;
    vec3 V = normalize(cameraPosition - vWorld);
    float alpha = vLook.x;
    if (kind == RING) {
      // A bright crest, and a darker trough inside it.
      float d = length(q), crest = exp(-pow((d - vLook.y) / vLook.z, 2.0)), trough = exp(-pow((d - vLook.y + 1.8 * vLook.z) / vLook.z, 2.0));
      alpha *= softly(vDepth, 0.02);
      float a = alpha * (0.5 * crest + 0.06 * trough);
      if (a < 0.002) discard;
      gl_FragColor = vec4(vTint * alpha * 0.5 * crest, a);
      return;
    }
    if (kind == FOAM) {
      float lace = hasFoam > 0.5 ? texture2D(foamMap, vUv).a : texture2D(atlas, vec2(vUv.x * 0.5, vUv.y)).a;
      float a = alpha * smoothstep(vLook.y, vLook.y + 0.25, lace) * (1.0 - smoothstep(0.55, 1.0, length(q)));
      if (a < 0.002) discard;
      a *= softly(vDepth, 0.02);
      gl_FragColor = vec4(vTint * a, a);
      return;
    }
    // Faded into the pool: splashes only at the surface itself.
    alpha *= kind == SPLASH ? smoothstep(vLevel - 0.03, vLevel + 0.01, vWorld.y) : smoothstep(vLevel - 0.05, vLevel + 0.3, vWorld.y);
    if (kind <= SPRAY) {
      // Spray: soft, uneven, lit by the shape it hangs in (how it is laid
      // over the frame: compositeFragment).
      float r2 = dot(q, q);
      if (r2 > 1.0) discard;
      vec4 t = texture2D(atlas, vCell.xy + vUv * vCell.zw, 3.0);
      float a = alpha * exp(-3.0 * r2) * (0.35 + 0.9 * t.a);
      if (a < 0.002) discard;
      a *= softly(vDepth, 0.8);
      gl_FragColor = vec4(vTint * (0.85 + 0.1 * t.b) * a, a);
      return;
    }
    vec3 n;
    float cover, thick, aerated = vLook.z;
    if (vCell.z < -0.5) {
      // A splash's crown: a thin sheet flaring as it rises, its rim breaking
      // into fingers that throw off beads.
      float x = q.x, y = vUv.y, hw = 0.45 + 0.5 * y, top = 0.72 + 0.12 * sin(x * 11.0 + vFlow.y) + 0.08 * sin(x * 27.0 + vFlow.y * 2.3);
      float sheet = (1.0 - smoothstep(hw - 0.1, hw, abs(x))) * (1.0 - smoothstep(top - 0.06, top, y));
      float rim = exp(-pow((abs(x) - hw + 0.08) / 0.08, 2.0)), k = floor((x + 1.0) * 5.0);
      float bead = 1.0 - smoothstep(0.05, 0.09, length(vec2((x - (k + 0.5) / 5.0 + 1.0) * 1.2, y - top - 0.15 * vLook.y)));
      cover = max(sheet * (0.2 + 0.8 * rim), bead);
      n = vec3(clamp(x / hw, -1.0, 1.0) * 0.8, 0.2, 0.6);
      thick = 0.3;
    } else if (kind == CORE) {
      // A streak of water, round across, swelling and wavering with the flow.
      float n1 = noise1(vFlow.x * 9.0 + vFlow.y), n2 = noise1(vFlow.x * 23.0 - time * 3.1 + vFlow.y * 1.7);
      float half_ = mix(0.55, 0.95, n1), x = q.x + 0.15 * (n2 - 0.5);
      cover = (1.0 - smoothstep(half_ * 0.55, half_, abs(x))) * (1.0 - smoothstep(0.6, 1.0, abs(q.y)));
      float nx = clamp(x / half_, -1.0, 1.0);
      n = vec3(nx * 0.85, 0.3 * (n2 - 0.5), sqrt(max(1.0 - nx * nx * 0.72, 0.0)));
      thick = 1.0 - abs(nx);
    } else if (vCell.z < 0.01) {
      // A drop: a small sphere, drawn along its motion.
      float r2 = dot(q, q);
      if (r2 > 1.0) discard;
      n = vec3(q, sqrt(1.0 - r2));
      cover = 1.0 - smoothstep(0.3, 1.0, r2);
      thick = n.z;
    } else {
      // A mass of aerated water.
      vec4 t = texture2D(atlas, vCell.xy + vUv * vCell.zw);
      n = vec3((t.rg * 2.0 - 1.0) * 0.5, 0.0);
      n.z = sqrt(max(1.0 - dot(n.xy, n.xy), 0.0));
      thick = t.b;
      cover = smoothstep(vLook.y, vLook.y + 0.4, t.a);
    }
    alpha *= cover;
    if (alpha < 0.002) discard;
    alpha *= softly(vDepth, kind == SPLASH ? 0.05 : 0.3);
    if (alpha < 0.002) discard;
    vec3 N = normalize(vSide * n.x + vUp * n.y + V * n.z);
    if (vCell.z > -0.5 && vCell.z < 0.01 && kind != CORE) {
      // Drops are clear and faint, sky-tinted. Where one glints, the sun's
      // reflection in it lights its whole pixel, however small the drop.
      float glint = vLight.y * vLight.x * thick, a = alpha * 0.25;
      vec3 light = sunLight() * 2.5 * glint * (0.6 + 0.4 * forward(V)) * alpha * min(1.0 / max(vLook.y, 1e-3), 6.0);
      gl_FragColor = vec4(skyReflect(reflect(-V, N)) * a + min(light, vec3(1.0)), a);
    } else gl_FragColor = water(N, V, vLight.x, aerated, thick, alpha, 0.04, 2.0, vec3(0.0), 0.0);
  }
`;

export class FountainJets {
  // The jets share the fountains' sources with the basin splashes (spray);
  // `graphics` gives the frame they draw into; `foam` is the stock foam
  // texture, if there is one; `tier`, whether the fountains stand in Theed's
  // two-tier basins (the overflow and the outer arcs need them).
  constructor(scene, spray, graphics, { foam = null, tier = false } = {}) {
    this.atlas = sprayAtlas();
    this.levels = new Float32Array(FOUNTAINS);
    this.lows = new Float32Array(FOUNTAINS);
    this.visible = new Float32Array(FOUNTAINS);
    this.graphics = graphics;
    // The frame so far (the scene, the basins' water), copied once just
    // before the first jet draws; the jets read it blurred, through its mipmaps.
    this.behind = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false, generateMipmaps: true, minFilter: THREE.LinearMipmapLinearFilter });
    this.captured = false;
    // The soft spray's half-size target, and frame pixels per pixel of it.
    this.sprayTarget = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false });
    this.sprayResolution = { value: new THREE.Vector2(1, 1) };
    this.fragScale = { value: new THREE.Vector2(2, 2) };
    this.clearColor = new THREE.Color();
    const uniforms = {
      time: { value: 0 }, sources: { value: spray.sources }, levels: { value: this.levels }, lows: { value: this.lows }, visible: { value: this.visible },
      wind: { value: new THREE.Vector3(0.45, 0, 0.2) }, atlas: { value: this.atlas },
      foamMap: { value: foam }, hasFoam: { value: foam ? 1 : 0 },
      sceneDepth: particleDepth.map, depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange, hasDepth: { value: 0 },
      sceneColor: { value: this.behind.texture }, sceneResolution: { value: new THREE.Vector2(1, 1) }, hasScene: { value: 0 },
      fragScale: { value: new THREE.Vector2(1, 1) },
      sunDirection: lighting.sunDirection, sunColor: lighting.sunColor, ambient: lighting.ambient,
      ambientTop: lighting.ambientTop, ambientBottom: lighting.ambientBottom, fade: lighting.fade,
    };
    const material = (vertexShader, fragmentShader, side, extra = {}) => new THREE.ShaderMaterial({
      uniforms: { ...uniforms, side: { value: side }, ...extra }, vertexShader, fragmentShader,
      transparent: true, depthWrite: false, premultipliedAlpha: true, blending: THREE.NormalBlending,
      side: THREE.DoubleSide, toneMapped: false, forceSinglePass: true,
    });
    let state = 0x51ed27;
    const random = () => ((state = Math.imul(state, 1664525) + 1013904223 >>> 0) / 4294967296);
    // One fountain's particles (kind, phase, salt); a splash's eight parts
    // share its phase and event number. Each fountain in view draws the same
    // block, varied by its own salt.
    const seeds = [];
    for (let kind = 0; kind < COUNTS.length; kind++) {
      if (kind === SPLASH) for (let e = 0; e < COUNTS[kind] / 8; e++) {
        const phase = random();
        for (let part = 0; part < 8; part++) seeds.push([kind, phase, e + part / 8]);
      } else for (let j = 0; j < COUNTS[kind]; j++) seeds.push([kind, (j + random()) / COUNTS[kind], random()]);
    }
    const corner = new THREE.BufferAttribute(new Float32Array([-1, -1, 1, -1, 1, 1, -1, 1]), 2);
    // Room for every fountain's block; each frame draws as many as are in view.
    const blocks = block => new Float32Array(Array.from({ length: FOUNTAINS }, () => block).flat(2));
    // On the pools (rings, foam); soft (mist, froth, skirt, spray); the rest
    // (splashes, drops, clumps, core). The same particles in both halves,
    // kinds in opposite orders.
    const group = kind => kind >= RING ? 'pool' : kind <= SPRAY ? 'soft' : 'sharp';
    const particles = (which, order) => {
      const chosen = seeds.filter(seed => group(seed[0]) === which);
      const sorted = order === 'before' ? [...chosen].sort((a, b) => b[0] - a[0]) : chosen;
      const g = new THREE.InstancedBufferGeometry();
      g.setAttribute('corner', corner);
      g.setIndex([0, 1, 2, 0, 2, 3]);
      g.setAttribute('seed', new THREE.InstancedBufferAttribute(blocks(sorted), 3));
      g.userData.perFountain = sorted.length;
      return g;
    };
    // Ribbons: every stream of a fountain, the outer arcs outermost.
    const { strip, index } = ribbonGeometry(), streams = tier ? ROPES + STRANDS + ARCS : ROPES + STRANDS;
    const ribbons = order => {
      const ropes = Array.from({ length: streams }, (_, k) => order === 'before' ? k : (k + ROPES + STRANDS) % streams);
      const g = new THREE.InstancedBufferGeometry();
      g.setAttribute('strip', new THREE.BufferAttribute(strip, 2));
      g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(strip.length / 2 * 3), 3));
      g.setIndex(index);
      g.setAttribute('rope', new THREE.InstancedBufferAttribute(blocks(ropes), 1));
      g.userData.perFountain = streams;
      return g;
    };
    const instanced = (attribute, size, data, index) => {
      const g = new THREE.InstancedBufferGeometry();
      g.setAttribute(attribute, new THREE.BufferAttribute(data, size));
      g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(data.length / size * 3), 3));
      g.setIndex(index);
      g.userData.perFountain = 1;
      return g;
    };
    const { sweep, index: columnIndex } = columnGeometry(), { film, index: filmIndex } = filmGeometry();
    const draw = (name, geometry, [vertex, fragment, side, extra = {}]) => Object.assign(new THREE.Mesh(geometry,
      material(vertex, fragment, side, { perFountain: { value: geometry.userData.perFountain }, ...extra })), { name });
    this.core = new THREE.Mesh(instanced('sweep', 3, sweep, columnIndex), new THREE.ShaderMaterial({
      uniforms, vertexShader: columnVertex, fragmentShader: columnFragment,
      transparent: true, depthWrite: false, premultipliedAlpha: true, blending: THREE.NormalBlending, toneMapped: false,
    }));
    this.core.name = 'fountain-jets-core';
    // The soft spray draws in scenes of its own, into the half-size target.
    const soft = order => {
      const mesh = draw(`fountain-spray-${order}`, particles('soft', order), [vertexShader, fragmentShader, order === 'before' ? 1 : 0, { fragScale: this.fragScale }]);
      mesh.frustumCulled = false;
      mesh.onBeforeRender = () => { mesh.material.uniforms.hasDepth.value = 1; mesh.material.uniformsNeedUpdate = true; };
      const sprayScene = new THREE.Scene();
      sprayScene.add(mesh);
      const composite = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), new THREE.ShaderMaterial({
        vertexShader: 'void main(){gl_Position=vec4(position.xy,0.,1.);}', fragmentShader: compositeFragment,
        uniforms: { sprayMap: { value: this.sprayTarget.texture }, sprayResolution: this.sprayResolution, enabled: { value: 0 },
          sceneDepth: particleDepth.map, depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange },
        transparent: true, depthWrite: false, depthTest: false, toneMapped: false,
        blending: THREE.CustomBlending, blendEquation: THREE.AddEquation,
        blendSrc: THREE.OneMinusDstColorFactor, blendDst: THREE.OneMinusSrcAlphaFactor,
        blendSrcAlpha: THREE.ZeroFactor, blendDstAlpha: THREE.OneFactor,
      }));
      composite.name = `fountain-spray-${order}`;
      composite.userData.spray = mesh;
      composite.userData.scene = sprayScene;
      return composite;
    };
    // Far to near.
    this.meshes = [
      draw('fountain-pool', particles('pool'), [vertexShader, fragmentShader, -1]),
      ...(tier ? [draw('fountain-overflow', instanced('film', 3, film, filmIndex), [filmVertex, filmFragment, 0])] : []),
      soft('beyond'),
      draw('fountain-ropes', ribbons('beyond'), [ribbonVertex, ribbonFragment, 0]),
      draw('fountain-jets', particles('sharp', 'beyond'), [vertexShader, fragmentShader, 0]),
      this.core,
      draw('fountain-jets-near', particles('sharp', 'before'), [vertexShader, fragmentShader, 1]),
      ...(tier ? [draw('fountain-overflow-near', instanced('film', 3, film, filmIndex), [filmVertex, filmFragment, 1])] : []),
      draw('fountain-ropes-near', ribbons('before'), [ribbonVertex, ribbonFragment, 1]),
      soft('before'),
    ];
    for (const [k, mesh] of this.meshes.entries()) {
      mesh.renderOrder = 9 + k * 0.05;
      mesh.frustumCulled = false;
      mesh.visible = false;
      if (mesh.userData.spray) mesh.onBeforeRender = (r, s, c) => this.drawSpray(r, c, mesh);
      else mesh.onBeforeRender = (r, s, c) => {
        const u = mesh.material.uniforms;
        u.hasDepth.value = particleDepth.capture(r, s, c, true) ? 1 : 0;
        u.hasScene.value = this.capture(r) ? 1 : 0;
        mesh.material.uniformsNeedUpdate = true;
      };
      scene.add(mesh);
    }
  }
  // Draws one half's soft spray into the half-size target, just before its
  // composite lays it over the frame.
  drawSpray(renderer, camera, composite) {
    const source = renderer.getRenderTarget(), enabled = composite.material.uniforms.enabled;
    enabled.value = source === this.graphics.sceneTarget && particleDepth.capture(renderer, null, camera, true) ? 1 : 0;
    composite.material.uniformsNeedUpdate = true;
    if (!enabled.value) return;
    const width = Math.max(1, Math.ceil(source.width / 2)), height = Math.max(1, Math.ceil(source.height / 2));
    this.sprayTarget.setSize(width, height);
    this.sprayResolution.value.set(width, height);
    this.fragScale.value.set(source.width / width, source.height / height);
    renderer.getClearColor(this.clearColor);
    const alpha = renderer.getClearAlpha(), autoClear = renderer.autoClear;
    try {
      renderer.autoClear = true;
      renderer.setClearColor(0, 0);
      renderer.setRenderTarget(this.sprayTarget);
      renderer.render(composite.userData.scene, camera);
    } finally {
      renderer.setRenderTarget(source);
      renderer.setClearColor(this.clearColor, alpha);
      renderer.autoClear = autoClear;
    }
  }
  capture(renderer) {
    const source = renderer.getRenderTarget();
    if (source !== this.graphics.sceneTarget) return false;
    if (this.captured) return true;
    this.captured = true;
    const { width, height } = source, gl = renderer.getContext();
    this.behind.setSize(width, height);
    renderer.setRenderTarget(this.behind);
    renderer.state.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
    renderer.state.bindFramebuffer(gl.READ_FRAMEBUFFER, drawFramebuffer(renderer, source));
    gl.blitFramebuffer(0, 0, width, height, 0, 0, width, height, gl.COLOR_BUFFER_BIT, gl.NEAREST);
    // A blit leaves the smaller mipmaps stale.
    renderer.state.bindTexture(gl.TEXTURE_2D, renderer.properties.get(this.behind.texture).__webglTexture);
    gl.generateMipmap(gl.TEXTURE_2D);
    renderer.state.unbindTexture();
    renderer.setRenderTarget(source);
    this.core.material.uniforms.sceneResolution.value.set(width, height);
    return true;
  }
  // `fountains` are the visible ones, each with its slot and the levels of
  // its pools (the tier's, and the outer one's if it has one).
  update(time, fountains) {
    this.captured = false;
    let shown = 0;
    for (const f of fountains) if (f.slot < FOUNTAINS) {
      this.visible[shown++] = f.slot;
      this.levels[f.slot] = f.level;
      this.lows[f.slot] = f.low ?? f.level;
    }
    for (const mesh of this.meshes) {
      const drawn = mesh.userData.spray || mesh;
      drawn.material.uniforms.time.value = time;
      drawn.geometry.instanceCount = drawn.geometry.userData.perFountain * shown;
      mesh.visible = shown > 0;
    }
  }
  dispose() {
    for (const mesh of this.meshes) {
      const spray = mesh.userData.spray;
      if (spray) { spray.removeFromParent(); spray.geometry.dispose(); spray.material.dispose(); }
      mesh.removeFromParent(); mesh.geometry.dispose(); mesh.material.dispose();
    }
    this.atlas.dispose();
    this.behind.dispose();
    this.sprayTarget.dispose();
  }
}
