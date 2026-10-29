import * as THREE from "three";
import { lighting } from "./materials.js";
import { particleDepth } from "./particle-depth.js";
import { sheathColor, vivid } from "./blaster-bolts.js";

// The Enhanced look of a sniper shot (Video > Sniper Shots). Presentation
// only: the round is the world's own bolt (a class with a FadeOutTime: the
// sniper rifles' 2000 m/s rounds), flown and cut by ordnance.js, a worker's
// or a server's records, or the page's predicted cosmetic. Where it goes,
// what it hits and when it hits are untouched. Classic keeps the 2004 streak
// (Ordnance.drawBolt).
//
// A shot is drawn as a tracer, in the round's own LaserGlowColor (kept
// saturated: the Rebels' pale yellow stays yellow):
//
//   - a puff of vapour at the muzzle: the bolt flashes the water in the air
//     there, a small cloud that blooms out, throws wisps and drifts off.
//   - a short, hot bolt, drawn flying at TRACER.speed from the gun as drawn
//     to where the round ends (2000 m/s crosses 50 m inside two frames, too
//     fast to see). The world's impact effect and sound, and a head shot's
//     flourish, wait for it to arrive (defer, headshot).
//   - a thin tracer line along its path that cools from the gun outward, so
//     nothing is left by a gun that kicks and moves on, and a wisp of smoke
//     that stays where it was made, rises and tears apart.
//   - at the impact a flash, sparks thrown back off the surface, embers and
//     a light; a head shot adds a gold star and more sparks.
//
// In the shooter's own eye view the bolt leaves his first-person gun as it
// is drawn each frame, fades in over its first metre, and his smoke starts
// a little way out.
//
// Bolts, tracers, vapour and sparks are one instanced draw of capsules,
// shaded per pixel from the closest distance between the view ray and the
// segment (as blaster-bolts.js draws plasma); flares are a second instanced
// draw of quads.

export const sniperLook = { enhanced: false };

const MAX_RECORDS = 48;
const NEAR_EYE = 2.5; // m: a muzzle this near the camera is the shooter's own eye view
const OWN_CLEAR = 0.6; // m of his own shot left out right in front of his eye
const OWN_FADE_IN = 1.2; // m his own bolt and tracer fade in over from the gun
const FADE_IN = 0.5; // m anyone else's do
const OWN_SMOKE_CLEAR = 2.5; // m of his own smoke left out in front of his gun
const HEAD_SPARKS = 26;
const SPARK_GRAVITY = 9.8;
const UP = new THREE.Vector3(0, 1, 0);
const SIDE = new THREE.Vector3(1, 0, 0);
const MERGE_SAME = 0.6; // m: a new source this close to a fresh record's ends is the same shot
const WHITE = [1, 1, 1];
const GOLD = [1, 0.82, 0.45];
const EMBER = [1, 0.5, 0.15];

// The look: a short, hot bolt drawn flying at
// `speed` (the round's 2000 m/s crosses 50 m inside two frames, too fast to
// be seen), `length` m long with radii `core` and `glow`; its tail runs in to
// the impact over `collapse` s. It leaves a thin tracer line along its path
// for `after` s, cooling from the gun outward at `retreat` m/s, and a wisp of smoke that rises and tears apart over
// smoke.life. Its muzzle puffs: the bolt flashes the water in the air there
// to vapour, a small cloud that blooms out and drifts off over PUFF_LIFE.
const TRACER = { retreat: 140, speed: 650, length: 6, core: 0.045, glow: 0.17, bright: 2.0, collapse: 0.05, after: 0.6, afterGlow: 0.03, afterBright: 0.6,
  smoke: { life: 2.2, radius: 0.22, opacity: 0.5, rise: 0.12, glow: 0.4 }, sparks: 12, embers: 6, flash: 0.8, light: 4 };
const BURN = 0.03; // s the tracer burns whole before it cools from the gun
const PUFF_LIFE = 0.9; // s
const PUFF_WISPS = 7;
const VAPOUR = [0.85, 0.9, 1];

const QUAD = new Float32Array([-1, -1, 1, -1, 1, 1, -1, 1]);
const QUAD_INDEX = [0, 1, 2, 0, 2, 3];
// Each batch is one interleaved instance buffer (one upload a frame): name,
// size, offset in floats.
const STREAK_ATTRIBUTES = [["iHead", 3, 0], ["iTail", 3, 3], ["iColor", 4, 6], ["iSize", 4, 10], ["iMisc", 4, 14]];
const STREAK_STRIDE = 18;
const FLARE_ATTRIBUTES = [["iCenter", 3, 0], ["iAxis", 3, 3], ["iColor", 4, 6], ["iParam", 4, 10]];
const FLARE_STRIDE = 14;

const NOISE = /* glsl */ `
  float hash12(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
  }
  float vnoise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash12(i), hash12(i + vec2(1.0, 0.0)), u.x),
               mix(hash12(i + vec2(0.0, 1.0)), hash12(i + vec2(1.0, 1.0)), u.x), u.y);
  }
  float fbm(vec2 p) {
    return vnoise(p) * 0.55 + vnoise(p * 2.13 + 7.1) * 0.3 + vnoise(p * 4.37 + 3.7) * 0.15;
  }
`;

const streakVertex = /* glsl */ `
  attribute vec2 corner;
  attribute vec3 iHead;
  attribute vec3 iTail;
  attribute vec4 iColor; // rgb (max channel 1), brightness
  attribute vec4 iSize;  // core radius, glow radius, kind (0 tracer line, 1 vapour, 2 bolt or spark), seed
  attribute vec4 iMisc;  // age (s), vapour opacity (a bolt's: metres it fades in over), metres from the muzzle at the tail, 1 if it may sit before the eye
  uniform float pixelAngle;
  varying vec2 vPlane;
  varying vec3 vHead;
  varying vec3 vTail;
  varying vec4 vColor;
  varying vec4 vSize;
  varying vec4 vMisc;
  varying float vEnergy;
  varying float vCoreShare;
  void main() {
    vec3 h = (modelViewMatrix * vec4(iHead, 1.0)).xyz;
    vec3 t = (modelViewMatrix * vec4(iTail, 1.0)).xyz;
    const float NEAR = 0.06;
    if (h.z > -NEAR && t.z > -NEAR) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
    vec3 h0 = h, t0 = t;
    if (h.z > -NEAR) h = mix(t0, h0, (t0.z + NEAR) / (t0.z - h0.z));
    if (t.z > -NEAR) t = mix(h0, t0, (h0.z + NEAR) / (h0.z - t0.z));
    float nearest = min(-h.z, -t.z);
    bool vapor = iSize.z > 0.5 && iSize.z < 1.5;
    float glow = vapor ? iSize.y : max(iSize.y, nearest * pixelAngle * 2.5);
    float core = max(iSize.x, nearest * pixelAngle * 0.75);
    vEnergy = vapor ? 1.0 : clamp(iSize.y / glow, 0.08, 1.0);
    vCoreShare = clamp(iSize.x / core, 0.0, 1.0);
    vec2 hp = h.xy / -h.z, tp = t.xy / -t.z;
    // The quad reaches only as far from the segment as the fragment shader
    // can light a pixel: beyond it every pixel is let go (its discards),
    // so the quad can stop there. Each kind's bound is its brightest term
    // solved for the 0.002 cut, with the shader's own peak factors folded
    // in (vapour: body 1.3 x keep x (0.9 a + 0.6 opacity); tracer line:
    // the halo 0.32 e^-2x, x 1.7 redGlow x 1.35 pulse, the sheath folded in
    // as 1.5 % past x = 2; bolt and spark: the sheath 2 e^-2.4x^2 with its
    // red glow 1.25 wider and 1.7 brighter).
    float energy = iColor.a * vEnergy;
    float reach;
    if (vapor) {
      float radius = glow * 0.5;
      reach = radius * sqrt(max(-log(0.0019 / (1.3 * (0.9 * iColor.a + 0.6 * iMisc.y))), 0.0) / 1.6);
    } else if (iSize.z < 0.5) {
      reach = glow * max(2.0, 0.5 * log(392.0 * energy));
    } else {
      reach = glow * sqrt(max(log(1790.0 * energy), 0.0) * 0.651);
    }
    // A pixel's distance from the segment falls short of its offset in the
    // plane (times the depth) by its ray's slant, 1 / |(p, 1)|: each end's
    // half width is widened to what covers the slant at the quad's own far
    // edge (r = k sqrt(1 + (|p| + r)^2), k the reach over the depth).
    float kh = min(reach / -h.z, 0.9), kt = min(reach / -t.z, 0.9);
    float lh = length(hp), lt = length(tp);
    float rh = (kh * kh * lh + kh * sqrt(max(1.0 + lh * lh - kh * kh, 0.0))) / (1.0 - kh * kh);
    float rt = (kt * kt * lt + kt * sqrt(max(1.0 + lt * lt - kt * kt, 0.0))) / (1.0 - kt * kt);
    float r = max(rh, rt);
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
    // The tail's distance from the muzzle, carried to where the clipped
    // tail now starts, so the noise stays fixed along the path.
    float cut = length(t - t0);
    vMisc = vec4(iMisc.x, iMisc.y, iMisc.z + cut, iMisc.w);
    float depth = nearest;
    gl_Position = projectionMatrix * vec4(p * depth, -depth, 1.0);
  }
`;

const streakFragment = /* glsl */ `
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
  varying vec4 vMisc;
  varying float vEnergy;
  varying float vCoreShare;
  ${NOISE}
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
    float len = sqrt(vv);
    // Metres from the muzzle (the tail end) at this pixel.
    float m = vMisc.z + (1.0 - s) * len;
    float age = vMisc.x;
    float kind = vSize.z;
    float seed = vSize.w;
    vec3 rgb = vColor.rgb;
    // A red-dominant bolt (the CIS's) is drawn as the films draw droid
    // bolts: a thin white-hot core inside a wide, bright red glow. Spread
    // across the whole core, the white's green and blue turned it pink.
    float redness = clamp((rgb.r - max(rgb.g, rgb.b)) * 1.2, 0.0, 1.0);
    float whiten = mix(0.45, 0.85, redness);
    float thinCore = mix(1.0, 0.45, redness);
    float redGlow = mix(1.0, 1.7, redness);
    vec3 point = ray * along;
    float depth = -point.z;
    float near = clamp((fade.y - depth) / max(fade.y - fade.x, 1.0), 0.0, 1.0);
    // Nothing flares right in front of the eye (the shooter's own view down
    // the shot), but the puff at his own muzzle (vMisc.w).
    float eye = vMisc.w > 0.5 ? smoothstep(0.08, 0.3, depth) : smoothstep(0.3, 2.5, depth);
    float keep = near * near * eye;
    vec3 energy = vec3(0.0);
    float cover = 0.0;
    if (kind < 0.5) {
      // A tracer line: an even glow along it.
      float core = vSize.x * thinCore, glow = vSize.y;
      float axial = vv > 1e-8 ? abs(rv) / len : 1.0;
      float hot = (1.0 - smoothstep(core * 0.5, core * 1.15, d)) * vCoreShare;
      float sheath = exp(-d2 / (glow * glow) * 2.6) * (1.0 + 0.4 * axial);
      float halo = exp(-d / glow * 2.0) * 0.32;
      float pulse = 1.0 + 0.35 * exp(-age * 2.5) * sin(m * 0.8 - age * 70.0 + seed * 6.28);
      float shimmer = 0.9 + 0.1 * sin(time * 83.0 + seed * 40.0 + m * 0.3);
      energy = (mix(rgb, vec3(1.0), whiten) * hot * 2.2 + rgb * (sheath * 2.0 + halo) * redGlow) * pulse * shimmer;
      energy *= vColor.a * vEnergy;
    } else if (kind < 1.5) {
      // Vapour: a soft tube that tears into wisps as it ages.
      float radius = vSize.y * 0.5;
      float body = exp(-d2 / (radius * radius) * 1.6);
      // The noise below is the costliest part of the look: where even its
      // densest (1.3 x body) would end under the discard below, the pixel
      // is let go first.
      if (body * 1.3 * keep * (vColor.a * 0.9 + vMisc.y * 0.6) < 0.002) discard;
      float tear = fbm(vec2(m * 0.42 + seed * 31.0, age * 0.55));
      float wisp = fbm(vec2(m * 1.6 - seed * 7.0, d / radius * 1.7 - age * 1.2));
      float density = body * smoothstep(0.18, 0.72, tear + 0.42 - age * 0.22) * (0.55 + 0.75 * wisp);
      cover = clamp(density * vMisc.y, 0.0, 1.0);
      // Fresh vapour still glows with the shot's colour.
      energy = rgb * density * vColor.a * 0.9;
    } else {
      // A spark: a short tapering streak.
      float core = vSize.x * thinCore * mix(1.0, 0.25, s), glow = vSize.y * mix(1.0, 0.5, s) * mix(1.0, 1.25, redness);
      float hot = (1.0 - smoothstep(core * 0.5, core * 1.2, d)) * vCoreShare;
      float sheath = exp(-d2 / (glow * glow) * 2.4);
      energy = (mix(rgb, vec3(1.0), whiten) * hot * 2.0 + rgb * sheath * 2.0 * redGlow) * vColor.a * mix(1.0, 0.25, s) * vEnergy;
    }
    // A bolt or tracer comes out of the gun: it fades in over its first
    // metres (vMisc.y), so its world-fixed start never shows as an edge
    // beside a gun that has since kicked and moved on.
    if (kind < 0.5 || kind > 1.5) energy *= smoothstep(0.0, max(vMisc.y, 1e-3), m);
    vec3 light = (1.0 - exp(-energy)) * keep;
    cover *= keep;
    // Vapour covers what is behind it in a pale haze; light adds.
    vec3 haze = mix(vec3(0.62, 0.66, 0.72), rgb, 0.18) * cover * 0.55;
    vec3 color = light + haze;
    // It also covers a little of the ground behind its glow, so the red
    // holds over bright sand without going dark.
    float shade = kind > 0.5 && kind < 1.5 ? 0.5 : mix(0.5, 0.62, redness);
    float alpha = max(max(light.r, max(light.g, light.b)) * shade, cover * 0.6);
    // Faint already, it is let go before the scene's depth is read (the
    // soft edge below only dims it further).
    if (max(color.r, max(color.g, color.b)) < 0.002 && alpha < 0.002) discard;
    if (softParticles > 0.5) {
      float stored = texture2D(sceneDepth, gl_FragCoord.xy / depthResolution).x;
      float surface = cameraRange.x * cameraRange.y / (cameraRange.y - stored * (cameraRange.y - cameraRange.x));
      float soft = smoothstep(0.0, max(vSize.y * 0.8, 0.05), surface - depth);
      color *= soft;
      alpha *= soft;
      if (max(color.r, max(color.g, color.b)) < 0.002 && alpha < 0.002) discard;
    }
    vec4 clip = projectionMatrix * vec4(point, 1.0);
    gl_FragDepth = clamp(clip.z / clip.w * 0.5 + 0.5, 0.0, 1.0);
    gl_FragColor = vec4(color, alpha);
  }
`;

const flareVertex = /* glsl */ `
  attribute vec2 corner;
  attribute vec3 iCenter;
  attribute vec3 iAxis;  // a ring's facing; zero for one that faces the camera
  attribute vec4 iColor; // rgb, brightness
  attribute vec4 iParam; // radius, ring width (of the radius), kind (0 flare, 1 ring), turn
  varying vec2 vCorner;
  varying vec4 vColor;
  varying vec4 vParam;
  void main() {
    vCorner = corner;
    vColor = iColor;
    vParam = iParam;
    float radius = iParam.x;
    vec4 view;
    if (dot(iAxis, iAxis) > 0.25) {
      vec3 a = normalize(iAxis);
      vec3 u = normalize(cross(a, abs(a.y) < 0.9 ? vec3(0.0, 1.0, 0.0) : vec3(1.0, 0.0, 0.0)));
      vec3 v = cross(a, u);
      view = modelViewMatrix * vec4(iCenter + (u * corner.x + v * corner.y) * radius, 1.0);
    } else {
      view = modelViewMatrix * vec4(iCenter, 1.0);
      // Pulled toward the eye, so a flash on a wall isn't cut in half by it.
      view.xyz += normalize(-view.xyz) * min(radius * 0.6, max(-view.z - 0.1, 0.0));
      float c = cos(iParam.w), s = sin(iParam.w);
      view.xy += mat2(c, s, -s, c) * corner * radius;
    }
    gl_Position = projectionMatrix * view;
  }
`;

const flareFragment = /* glsl */ `
  varying vec2 vCorner;
  varying vec4 vColor;
  varying vec4 vParam;
  void main() {
    float r = length(vCorner);
    if (r > 1.0) discard;
    vec3 rgb = vColor.rgb;
    vec3 energy;
    if (vParam.z < 0.5) {
      // A flare: a hot point, a soft bloom and a four-pointed star.
      float core = exp(-r * r * 60.0) * 2.4;
      float bloom = exp(-r * 5.0) * 0.8;
      float a = atan(vCorner.y, vCorner.x);
      float rays = pow(abs(cos(a * 2.0)), 48.0) * exp(-r * 3.2) * 1.3;
      energy = vec3(1.0) * core + rgb * (bloom + rays);
    } else {
      float width = max(vParam.y, 0.02);
      float band = exp(-pow((r - (1.0 - width)) / width, 2.0));
      energy = mix(rgb, vec3(1.0), 0.35) * band * 1.6;
    }
    energy *= vColor.a * (1.0 - smoothstep(0.85, 1.0, r));
    vec3 color = 1.0 - exp(-energy);
    if (max(color.r, max(color.g, color.b)) < 0.002) discard;
    gl_FragColor = vec4(color, 0.0);
  }
`;

function instanced(attributes, stride, material, capacity, name, order) {
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setAttribute("corner", new THREE.BufferAttribute(QUAD, 2));
  geometry.setIndex(QUAD_INDEX);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.name = name;
  mesh.frustumCulled = false;
  mesh.renderOrder = order;
  mesh.visible = false;
  const batch = { geometry, mesh, data: null, buffer: null, range: { start: 0, count: 0 }, capacity: 0, count: 0, attributes, stride };
  grow(batch, capacity);
  return batch;
}

function grow(batch, capacity) {
  const data = new Float32Array(capacity * batch.stride);
  if (batch.data) data.set(batch.data);
  batch.data = data;
  batch.buffer = new THREE.InstancedInterleavedBuffer(data, batch.stride).setUsage(THREE.DynamicDrawUsage);
  for (const [name, size, offset] of batch.attributes) batch.geometry.setAttribute(name, new THREE.InterleavedBufferAttribute(batch.buffer, size, offset));
  // three keeps the instance limit of the buffers it first bound until the
  // geometry is disposed: grown, the new instances would not be drawn.
  delete batch.geometry._maxInstanceCount;
  batch.capacity = capacity;
}

// warm: an empty batch draws one dark instance (all zero: no brightness,
// which its fragment shader lets go) so the program's pipeline is built.
function upload(batch, warm = false) {
  let drawn = batch.count;
  if (!drawn && warm) {
    batch.data.fill(0, 0, batch.stride);
    drawn = 1;
  }
  batch.geometry.instanceCount = drawn;
  batch.mesh.visible = drawn > 0;
  if (!drawn) return;
  // What was drawn, through one range kept for it (three empties the list
  // once it has uploaded).
  const ranges = batch.buffer.updateRanges;
  ranges.length = 0;
  batch.range.count = drawn * batch.stride;
  ranges.push(batch.range);
  batch.buffer.needsUpdate = true;
}

const premultiplied = (uniforms, vertexShader, fragmentShader) => new THREE.ShaderMaterial({
  uniforms, vertexShader, fragmentShader,
  transparent: true, depthWrite: false,
  blending: THREE.CustomBlending, blendEquation: THREE.AddEquation,
  blendSrc: THREE.OneFactor, blendDst: THREE.OneMinusSrcAlphaFactor,
  side: THREE.DoubleSide, forceSinglePass: true,
});

const ease = (x) => 1 - (1 - x) * (1 - x);

// A sheath colour (largest channel 1) at least MIN_SATURATION saturated, its
// hue kept: the Rebels' pale yellow (224, 224, 160) would otherwise draw
// nearly white beside the Republic's blue, the CIS's red and the Empire's
// green.
const MIN_SATURATION = 0.65;
export function saturated(rgb) {
  const least = Math.min(...rgb), saturation = 1 - least;
  if (saturation >= MIN_SATURATION || saturation <= 1e-3) return rgb;
  const k = MIN_SATURATION / saturation;
  return rgb.map((c) => 1 - (1 - c) * k);
}

// A ParticleRenderer batch of its own kind (ParticleRenderer.custom): begin()
// and end() bracket each frame's bolts, and Ordnance.drawBolt hands it each
// sniper round it draws (touch). A shot outlives its bolt here: its vapour
// and sparks run on after the world has let the round go.
export class SniperTrace {
  // lights: the frame's LightPool; view: the render camera, when known (the
  // shooter's own eye view is told apart by how near it is to the muzzle).
  // muzzle: his first-person gun's muzzle as drawn now, or null.
  constructor(lights = null, view = null, muzzle = null) {
    this.lights = lights;
    this.view = view;
    this.muzzle = muzzle;
    this.waiting = [];
    this.puffs = [];
    this.records = [];
    this.bySource = new WeakMap();
    this.sparks = [];
    this.sparkPool = []; // spent sparks, reused
    this.bursts = [];
    this.time = null;
    // Frames left over which an empty batch still draws one dark instance
    // (upload): the driver builds a program's pipeline at its first real
    // draw, which would otherwise be the first shot.
    this.warming = 90;
    this.camera = new THREE.Vector3(0, 1e6, 0);
    this.pixelAngle = { value: 0.002 };
    const uniforms = {
      fade: lighting.fade, time: lighting.time, pixelAngle: this.pixelAngle,
      sceneDepth: particleDepth.map, softParticles: particleDepth.enabled,
      depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
    };
    // Sized for a busy fight (a dozen rifles' smoke, vapour and sparks at
    // once): growing mid-battle makes new buffers.
    this.streaks = instanced(STREAK_ATTRIBUTES, STREAK_STRIDE, premultiplied(uniforms, streakVertex, streakFragment), 512, "sniper-trace", 11);
    this.flares = instanced(FLARE_ATTRIBUTES, FLARE_STRIDE, premultiplied({}, flareVertex, flareFragment), 64, "sniper-flares", 12);
    this.flares.mesh.material.depthTest = true;
    this.mesh = new THREE.Group();
    this.mesh.name = "sniper-shots";
    this.mesh.add(this.streaks.mesh, this.flares.mesh);
    const size = new THREE.Vector2();
    this.streaks.mesh.onBeforeRender = (renderer, scene, camera, ...rest) => {
      particleDepth.capture(renderer, scene, camera, ...rest);
      if (!camera.isPerspectiveCamera) return;
      this.camera.setFromMatrixPosition(camera.matrixWorld);
      const target = renderer.getRenderTarget();
      const height = target ? target.height : renderer.getDrawingBufferSize(size).y;
      this.pixelAngle.value = (2 * Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2)) / Math.max(height, 1) / (camera.zoom || 1);
    };
    this.tmp = new THREE.Vector3();
    this.tmp2 = new THREE.Vector3();
    this.endPoint = new THREE.Vector3();
  }

  now() {
    return lighting.time.value;
  }

  eye() {
    const camera = this.view?.();
    return camera ? camera.getWorldPosition(this.tmp) : this.camera;
  }

  begin() {
    for (const record of this.records) record.touched = false;
  }

  // A sniper round as Ordnance.drawBolt draws it this frame: from `start`
  // (the gun as drawn) to `head`, `ended` once its flight is over, `fade`
  // (1 to 0) what is left of it after.
  touch(source, def, start, head, direction, ended, fade, origin = start) {
    const t = this.now();
    let record = this.bySource.get(source);
    if (!record) {
      // One shot can be drawn from a new object (a replicated round rebuilt
      // from its terminal, or the authority's copy of a cosmetic): keep the
      // shot that is already there.
      record = this.records.find((r) => !r.touched && t - r.born < 0.5 && r.direction.dot(direction) > 0.999
        && (r.start.distanceTo(start) < MERGE_SAME || r.head.distanceTo(head) < MERGE_SAME));
      if (!record) record = this.add(def, start, direction, t, origin);
      this.bySource.set(source, record);
    }
    record.touched = true;
    record.source = source;
    record.target.copy(head);
    record.fade = fade;
    // The impact plays when the drawn bolt gets there (end).
    if (ended) record.ended = true;
  }

  add(def, start, direction, t, origin = start) {
    if (this.records.length >= MAX_RECORDS) this.records.shift();
    const color = saturated(vivid(sheathColor(def.LaserGlowColor || def.LightColor || [120, 160, 255, 255]), 1.4));
    const style = TRACER;
    const record = {
      def, color, style, born: t, landed: null, ended: false, touched: true, fade: 1, gone: null,
      start: start.clone(), head: start.clone(), target: start.clone(), direction: direction.clone().normalize(),
      origin: origin.clone(), drawn: direction.clone().normalize(), fired: start.clone(),
      reach: (def.Velocity || 0) * (def.LifeSpan || 0),
      own: start.distanceTo(this.eye()) < NEAR_EYE,
      seed: Math.random(), headshot: false,
    };
    // His own view: the gun is drawn with the camera, so the shot's start
    // rides with the camera too, staying on the barrel as he moves and the
    // view kicks; anywhere else it stays where it was fired.
    const camera = record.own ? this.view?.() : null;
    if (camera) { camera.updateMatrixWorld(); record.cameraLocal = camera.worldToLocal(start.clone()); }
    this.records.push(record);
    if (!record.own) this.burst({ kind: 0, at: record.start, color, radius: 0.45, life: 0.07, brightness: 1.2 });
    this.puff(record.start, record.direction, color);
    return record;
  }

  // Vapour flashed out of the air at the muzzle: a cloud along the shot's
  // first half metre, and wisps thrown out round it.
  puff(at, direction, color) {
    if (this.puffs.length > 24) this.puffs.shift();
    const across = new THREE.Vector3().crossVectors(direction, Math.abs(direction.y) < 0.9 ? UP : SIDE).normalize();
    const wisps = [];
    for (let i = 0; i < PUFF_WISPS; i++) {
      const angle = (i + Math.random() * 0.6) / PUFF_WISPS * Math.PI * 2;
      wisps.push(across.clone().applyAxisAngle(direction, angle).addScaledVector(direction, 0.3 + Math.random() * 0.4).normalize().multiplyScalar(1.2 + Math.random() * 1.2));
    }
    this.puffs.push({ at: at.clone(), direction: direction.clone(), color, born: this.now(), seed: Math.random(), wisps });
  }

  // The flight is over: an impact, unless it simply ran out of life in the air.
  land(record, t) {
    record.ended = true;
    record.landed = t;
    record.head.copy(record.target ?? record.head);
    for (const play of record.onLand?.splice(0) || []) play();
    const flown = record.head.distanceTo(record.start);
    if (record.reach && flown > record.reach - 1 && !record.headAt) return;
    record.impact = true;
    const { color, style } = record, at = record.head;
    this.burst({ kind: 0, at, color: WHITE, radius: 0.6 * style.flash, life: 0.12, brightness: 1.3, turn: record.seed * 3 });
    this.burst({ kind: 0, at, color, radius: 0.9 * style.flash, life: 0.45, brightness: 0.55, turn: record.seed * 3 });
    this.spray(at, record.direction, color, style.sparks, 1);
    this.embers(at, record.direction, style.embers);
    this.lights?.flash?.(at.clone().addScaledVector(record.direction, -0.4), color.map((c) => c * 1.5), 6 + 2 * style.flash, 0.4);
    if (record.headAt) { this.flourish(record.headAt, record.direction); record.headAt = null; }
  }

  // The world's "headshot" effect at `point`: the shot that made it, if
  // this page drew one there, gets the head shot's flourish.
  headshot(point) {
    const t = this.now();
    // Once a hit: the page's own round shows it as it lands, and the
    // world's effect for the same hit arrives later.
    this.heads = (this.heads || []).filter((h) => t - h.t < 1.2);
    if (this.heads.some((h) => h.point.distanceTo(point) < 1.5)) return;
    this.heads.push({ t, point: point.clone() });
    // The shot whose line passes it (its drawn head, or the target it has
    // been told of so far, may still be short of it).
    let record = null, best = 1.5;
    for (const r of this.records) {
      if (r.headshot || t - r.born > 1) continue;
      const d = this.offLine(r, point);
      if (d < best) { best = d; record = r; }
    }
    if (record) record.headshot = true;
    // Its drawn bolt still on the way: the flourish waits for it (land).
    if (record && record.landed === null) { record.headAt = point.clone(); return; }
    // No shot drawn there yet (the page's own round reports its hit before
    // the frame draws it): wait a moment for one.
    if (!record) { this.waiting.push({ point: point.clone(), until: t + 0.15 }); return; }
    this.flourish(point, record.direction);
  }

  // Head shots waiting for their shot: on the first record whose line
  // passes them, or on their own once the wait is over.
  claimWaiting(t) {
    this.waiting = this.waiting.filter((w) => {
      const record = this.records.find((r) => !r.headshot && r.landed === null && this.offLine(r, w.point) < 1.5);
      if (record) { record.headshot = true; record.headAt = w.point; return false; }
      if (t < w.until) return true;
      this.flourish(w.point, this.tmp.set(0, 0, 1));
      return false;
    });
  }

  flourish(point, direction) {
    this.burst({ kind: 0, at: point, color: GOLD, radius: 1.4, life: 0.16, brightness: 1.9, turn: 0.785 });
    this.spray(point, direction, GOLD, HEAD_SPARKS, 1.3);
    this.embers(point, direction, 10);
    this.lights?.flash?.(point.clone(), [1.6, 1.4, 1.1], 8, 0.45);
  }

  // The world's impact for a shot whose drawn bolt is still on its way:
  // `play` runs when it lands. False when no such shot is drawn here.
  defer(point, play) {
    const t = this.now();
    for (const r of this.records) {
      if (r.landed !== null || t - r.born > 1 || this.offLine(r, point) > 1.5) continue;
      (r.onLand ||= []).push(play);
      return true;
    }
    return false;
  }

  // How far `point` lies from a shot's line, ahead of its gun.
  offLine(r, point) {
    const v = this.tmp2.subVectors(point, r.origin), along = v.dot(r.direction);
    return along < 0 ? Infinity : v.addScaledVector(r.direction, -along).length();
  }

  burst(spec) {
    if (this.bursts.length > 64) this.bursts.shift();
    this.bursts.push({ ...spec, at: spec.at.clone(), axis: spec.axis?.clone() ?? null, born: this.now() + (spec.delay || 0) });
  }

  // Sparks thrown back off the surface, against the way the round came.
  spray(at, direction, color, count, speed) {
    for (let i = 0; i < count; i++) {
      const s = this.spark(at, false);
      const v = s.v.randomDirection();
      if (v.dot(direction) > 0) v.addScaledVector(direction, -2 * v.dot(direction));
      v.addScaledVector(direction, -0.6).normalize().multiplyScalar((3 + Math.random() * 11) * speed);
      s.life = 0.2 + Math.random() * 0.45;
      s.color = Math.random() < 0.5 ? WHITE : color;
      this.addSpark(s);
    }
  }

  // Slow, hot embers that fall and cool from white to orange.
  embers(at, direction, count) {
    for (let i = 0; i < count; i++) {
      const s = this.spark(at, true);
      const v = s.v.randomDirection().addScaledVector(direction, -0.8).normalize().multiplyScalar(1 + Math.random() * 3);
      v.y += 1.5;
      s.life = 0.6 + Math.random() * 0.7;
      s.color = EMBER;
      this.addSpark(s);
    }
  }

  // A spark at `at`, from the spent ones when there are any.
  spark(at, ember) {
    const s = this.sparkPool.pop() || { at: new THREE.Vector3(), from: new THREE.Vector3(), v: new THREE.Vector3(), life: 0, age: 0, color: null, ember: false };
    s.at.copy(at);
    s.from.copy(at);
    s.age = 0;
    s.ember = ember;
    return s;
  }

  addSpark(s) {
    if (this.sparks.length > 160) this.sparkPool.push(this.sparks.shift());
    this.sparks.push(s);
  }

  streak(head, tail, color, brightness, core, glow, kind, seed, age = 0, opacity = 0, offset = 0, near = 0) {
    const b = this.streaks;
    if (b.count === b.capacity) grow(b, b.capacity * 2);
    const d = b.data, o = b.count++ * STREAK_STRIDE;
    d[o] = head.x; d[o + 1] = head.y; d[o + 2] = head.z;
    d[o + 3] = tail.x; d[o + 4] = tail.y; d[o + 5] = tail.z;
    d[o + 6] = color[0]; d[o + 7] = color[1]; d[o + 8] = color[2]; d[o + 9] = brightness;
    d[o + 10] = core; d[o + 11] = glow; d[o + 12] = kind; d[o + 13] = seed;
    d[o + 14] = age; d[o + 15] = opacity; d[o + 16] = offset; d[o + 17] = near;
  }

  flare(center, axis, color, brightness, radius, width, kind, turn = 0) {
    const b = this.flares;
    if (b.count === b.capacity) grow(b, b.capacity * 2);
    const d = b.data, o = b.count++ * FLARE_STRIDE;
    d[o] = center.x; d[o + 1] = center.y; d[o + 2] = center.z;
    d[o + 3] = axis?.x || 0; d[o + 4] = axis?.y || 0; d[o + 5] = axis?.z || 0;
    d[o + 6] = color[0]; d[o + 7] = color[1]; d[o + 8] = color[2]; d[o + 9] = brightness;
    d[o + 10] = radius; d[o + 11] = width; d[o + 12] = kind; d[o + 13] = turn;
  }

  end() {
    const t = this.now();
    const dt = this.time === null ? 0 : Math.min(Math.max(t - this.time, 0), 0.1);
    this.time = t;
    this.streaks.count = 0;
    this.flares.count = 0;
    if (this.waiting.length) this.claimWaiting(t);
    // Each list is kept in place, the spent left out (no new array a
    // frame).
    const records = this.records;
    let kept = 0;
    for (let i = 0; i < records.length; i++) if (this.drawRecord(records[i], t)) records[kept++] = records[i];
    records.length = kept;
    const puffs = this.puffs;
    kept = 0;
    for (let i = 0; i < puffs.length; i++) if (this.drawPuff(puffs[i], t)) puffs[kept++] = puffs[i];
    puffs.length = kept;
    const bursts = this.bursts;
    kept = 0;
    for (let i = 0; i < bursts.length; i++) {
      const b = bursts[i];
      const age = t - b.born;
      if (age >= 0) {
        const k = Math.min(age / b.life, 1);
        if (b.kind === 0) this.flare(b.at, null, b.color, b.brightness * (1 - k) * (1 - k), b.radius * (0.7 + 0.5 * ease(k)), 0, 0, b.turn || 0);
        else this.flare(b.at, b.axis, b.color, b.brightness * (1 - k), b.radius[0] + (b.radius[1] - b.radius[0]) * ease(k), b.width * (1 - 0.5 * k), 1);
      }
      if (age < b.life) bursts[kept++] = b;
    }
    bursts.length = kept;
    const sparks = this.sparks, tail = this.tmp2;
    kept = 0;
    for (let i = 0; i < sparks.length; i++) {
      const s = sparks[i];
      s.age += dt;
      s.from.copy(s.at);
      s.v.y -= SPARK_GRAVITY * dt;
      s.v.multiplyScalar(Math.exp(-dt * 2.5));
      s.at.addScaledVector(s.v, dt);
      const k = 1 - s.age / s.life;
      if (k > 0) {
        tail.copy(s.at).addScaledVector(s.v, s.ember ? -0.012 : -0.03);
        if (s.ember) this.streak(s.at, tail, k > 0.6 ? WHITE : s.color, 1.1 * k, 0.01, 0.035, 2, 0);
        else this.streak(s.at, tail, s.color, 1.4 * k, 0.012, 0.05, 2, 0);
      }
      if (s.age < s.life) sparks[kept++] = s;
      else this.sparkPool.push(s);
    }
    sparks.length = kept;
    const warm = this.warming > 0;
    if (warm) this.warming--;
    upload(this.streaks, warm);
    upload(this.flares, warm);
  }

  // A shot's frame; false once nothing of it is left.
  drawRecord(r, t) {
    const lift = this.tmp, tail = this.tmp2;
    const age = t - r.born, style = r.style;
    // Its bolt gone (or never touched again): it has landed.
    if (!r.touched) { r.gone ??= t; r.ended = true; }
    // The bolt is drawn flying at the style's speed, slower than the
    // round (2000 m/s crosses 30 m inside one frame); the hit itself is
    // the world's, already decided. It lands when the drawn bolt arrives.
    // Drawn from the gun as drawn to where the round ends (or, still in
    // flight, the far end of its line): the round itself leaves from the
    // gun origin beside the eye, so running the drawn bolt along its line
    // from the muzzle put it beside the real path, at an angle to the gun.
    // His own shot leaves his first-person gun as it is drawn this frame
    // (it kicks and settles after the shot), else rides with the camera.
    const muzzle = r.own ? this.muzzle?.() : null;
    const camera = r.cameraLocal ? this.view?.() : null;
    if (muzzle && muzzle.distanceTo(r.start) < 1.5) r.start.copy(muzzle);
    else if (camera) r.start.copy(r.cameraLocal).applyMatrix4(camera.matrixWorld);
    if (r.ended) this.endPoint.copy(r.target);
    else this.endPoint.copy(r.origin).addScaledVector(r.direction, r.reach || 300);
    r.drawn.subVectors(this.endPoint, r.start);
    if (r.drawn.lengthSq() > 1e-8) r.drawn.normalize(); else r.drawn.copy(r.direction);
    const reach = r.ended ? r.target.distanceTo(r.start) : Infinity;
    const flown = Math.min(reach, style.speed * age);
    if (r.landed === null) {
      r.head.copy(r.start).addScaledVector(r.drawn, flown);
      if (r.ended && flown >= reach) this.land(r, t);
    }
    const after = r.landed === null ? 0 : t - r.landed;
    const smoke = style.smoke;
    if (r.landed !== null && after > style.collapse && after > style.after && (!smoke || age > smoke.life)) return false;
    const length = r.head.distanceTo(r.start);
    // Not from inside the shooter's own eye, and faded in from the gun.
    const clear = r.own ? Math.min(OWN_CLEAR, length) : 0;
    const fadeIn = r.own ? OWN_FADE_IN : FADE_IN;
    // The bolt: a streak `length` long racing to the target, its tail
    // running in to the impact once it has struck.
    const back = Math.max(0, length - style.length);
    const k = r.landed === null ? 0 : Math.min(1, after / style.collapse);
    const tailAt = Math.max(clear, back + (length - back) * k);
    if (k < 1 && length - tailAt > 0.05) {
      tail.copy(r.start).addScaledVector(r.drawn, tailAt);
      const thin = r.own ? 0.6 : 1;
      this.streak(r.head, tail, r.color, style.bright * (1 - 0.5 * k), style.core * thin, style.glow * thin, 2, r.seed, age, fadeIn, tailAt);
      if (!r.own) this.lights?.add?.(r.head, r.color, style.light);
    }
    // What it leaves for a moment along its whole path.
    const glowLeft = r.landed === null ? 1 : 1 - after / style.after;
    // It cools from the gun outward: its near end runs off down the path,
    // so nothing is left by the gun to swing with its kick.
    const cooled = Math.max(clear, (age - BURN) * style.retreat);
    if (glowLeft > 0 && length > cooled + 0.5) {
      tail.copy(r.start).addScaledVector(r.drawn, cooled);
      this.streak(r.head, tail, r.color, style.afterBright * glowLeft * glowLeft, style.afterGlow * 0.3, style.afterGlow * (0.6 + 0.4 * glowLeft), 0, r.seed, age, fadeIn, cooled);
    }
    // Smoke, for the styles that leave it; it rises as it ages.
    if (smoke) {
      const left = Math.max(0, 1 - age / smoke.life);
      if (left > 0 && length > clear + OWN_SMOKE_CLEAR * Number(r.own)) {
        const rise = smoke.rise * age;
        const radius = 0.03 + smoke.radius * ease(Math.min(age / (smoke.life * 0.6), 1));
        const opacity = smoke.opacity * Math.pow(left, 1.3) * Math.min(1, age / 0.04 + 0.2);
        // Smoke stays where it was made: from where the gun was when it
        // fired, not on the gun as it moves on.
        const from = r.fired, reachSmoke = r.head.distanceTo(from), skip = r.own ? Math.min(OWN_SMOKE_CLEAR, reachSmoke) : 0;
        lift.copy(r.head).y += rise;
        tail.subVectors(r.head, from).multiplyScalar(skip / Math.max(reachSmoke, 1e-6)).add(from).y += rise;
        this.streak(lift, tail, r.color, (smoke.glow || 0.5) * Math.exp(-age * 4), 0, radius * 2, 1, r.seed, age, opacity * (r.own ? 0.75 : 1), skip);
      }
    }
    return true;
  }

  // Muzzle puffs: they bloom out fast, then slow, swell and rise as they
  // thin. Drawn right in front of the shooter's own eye too. False once it
  // is gone.
  drawPuff(f, t) {
    const lift = this.tmp, tail = this.tmp2;
    const age = t - f.born;
    if (age > PUFF_LIFE) return false;
    const k = age / PUFF_LIFE, out = 1 - Math.exp(-age * 6), opacity = 0.42 * Math.pow(1 - k, 1.6);
    const glow = 0.35 * Math.exp(-age * 16);
    lift.copy(f.at).addScaledVector(f.direction, 0.15 + 0.5 * out).y += 0.12 * age;
    tail.copy(f.at).addScaledVector(f.direction, 0.05 * out).y += 0.12 * age;
    this.streak(lift, tail, VAPOUR, glow, 0, (0.05 + 0.24 * ease(Math.min(age / 0.5, 1))) * 2, 1, f.seed, age * 0.5, opacity, 0, 1);
    for (let i = 0; i < f.wisps.length; i++) {
      const w = f.wisps[i];
      lift.copy(f.at).addScaledVector(w, 0.3 * out).y += 0.15 * age;
      tail.copy(lift).addScaledVector(w, -0.08);
      this.streak(lift, tail, VAPOUR, glow * 0.3, 0, (0.03 + 0.1 * ease(Math.min(age / 0.4, 1))) * 2, 1, f.seed + i, age * 0.5, opacity * 0.7, 0, 1);
    }
    return true;
  }
}
