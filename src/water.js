import { WaterState } from "./runtime/water-state.js";
import { drawFramebuffer } from "./render-targets.js";
import * as THREE from "three";
import { lighting } from "./materials.js";
import { particleDepth } from "./particle-depth.js";
import { WATER_QUALITIES } from "./settings.js";
import { OceanSurface } from "./ocean-surface.js";

// Terrain water, as the 2004 game draws it on PC: a flat surface over the
// map's water cells (32 m squares on Theed) at the layer's height, bobbing
// 0.06 m at 0.3 Hz.
//
// Two passes over the same surface, after the opaque scene:
//   1. what lies under the water is multiplied by RefractionColor;
//   2. a reflection, at ReflectionColor's alpha, plus white sun glints
//      (N.H)^8, blended by a Fresnel term from FresnelMin looking straight
//      down to FresnelMax at grazing angles.
// The surface normal comes from the NormalMapTextures frames, cross-faded at
// their frame rate and tiled Tile times per cell; the reflection is jittered
// by the BumpMapTextures frames (0.03 of the screen, turned with the view).
//
// Low uses MainTexture and scrolling specular masks. High reflects nearby
// geometry in screen space; Ultra uses a full mirrored scene. Enhanced water resolves opaque color/depth once for
// depth-aware refraction/absorption, with animated world-space normal detail.
//
// Rings spread where things meet the water (radius growing 1.5 m/s, fading
// in over the first 15% of their life and out over the rest), and under the
// surface the screen takes UnderwaterColor.

const BOB = 0.06; // m
const BOB_RATE = 0.3; // Hz
const DISTORT = 0.03; // share of the screen the bump frames jitter the reflection
const RIPPLE_MAX = 100;
const RIPPLE_SEGMENTS = 40;
const RIPPLE_BAND = 0.35; // m either side of the ring's radius
const RIPPLE_LIFT = 0.01; // m above the surface
const RIPPLE_GROWTH = 1.5; // m/s
const RIPPLE_RANGE = 120; // m from the camera past which rings are dropped
const REFLECTION_SIZE = [512, 256];
// The layer the mirror draws the sky's models on, alone (reflect()).
const MIRROR_SKY_LAYER = 5;
// A soldier in the water: wakes come every 1 / (0.3 v^2 + 4) s, a fall
// faster than 5 m/s splashes (at most once a second), and with his head under
// for 3 s he drowns at 30 health a second.
const WAKE_LIFE = 2;
const WAKE_RADIUS = 0.25;
const SPLASH_SPEED = 5; // m/s down
const SPLASH_GAP = 1; // s
const DROWN_AFTER = 3; // s
const DROWN_DAMAGE = 30; // per second

const rgba = (c, fallback = [255, 255, 255, 255]) => {
  const [r, g, b, a = 255] = c || fallback;
  return [r / 255, g / 255, b / 255, a / 255];
};

const VERTEX = /* glsl */ `
  varying float vSurfaceDepth;
  varying vec3 vWorld;
  varying vec2 vUv;
  void main() {
    vUv = uv;
    vec4 world = modelMatrix * vec4(position, 1.0);
    vWorld = world.xyz;
    vec4 view = viewMatrix * world;
    vSurfaceDepth = -view.z;
    gl_Position = projectionMatrix * view;
    // Preserve plane coverage across the near plane, including triangles
    // straddling behind the eye. Fragments restore the true surface depth.
    gl_Position.z = 0.0;
  }
`;

// The far scene's copy of a pass (FAR_WATER, Water.farPass) draws from where
// the near scene ends (surfaceRange.x) out to the water's reach
// (surfaceRange.y), in the far scene's fog.
const SURFACE_DEPTH = /* glsl */ `
    #ifdef FAR_WATER
      if (vSurfaceDepth < surfaceRange.x || vSurfaceDepth > surfaceRange.y) discard;
    #else
      if (vSurfaceDepth > cameraRange.y) discard;
    #endif
    gl_FragDepth = clamp((cameraRange.y - cameraRange.x * cameraRange.y / max(vSurfaceDepth, 0.0001))
      / (cameraRange.y - cameraRange.x), 0.0, 1.0);
`;

const FAR_WATER_PARS = /* glsl */ `
  #ifdef FAR_WATER
    uniform vec2 surfaceRange;
    uniform vec3 fogColor;
    uniform vec2 fogRange;
    // The far scene's linear fog (materials.js FAR), 0 where there is none.
    float farFog(float depth) {
      return clamp((depth - fogRange.x) / (fogRange.y - fogRange.x), 0.0, 1.0);
    }
  #endif
`;

// Native low quality uses the authored scrolling colour texture and animated
// specular mask. It needs no mirrored scene or opaque framebuffer copy.
const TEXTURED_WATER = /* glsl */ `
  ${FAR_WATER_PARS}
  uniform sampler2D mainMap, maskA, maskB;
  uniform float maskMix, time;
  uniform vec4 tint;
  uniform vec2 tile, velocity, maskTile, maskVelocity, cameraRange;
  uniform vec3 toSun;
  varying float vSurfaceDepth;
  varying vec3 vWorld;
  varying vec2 vUv;
  void main() {
    ${SURFACE_DEPTH}
    vec4 color = texture2D(mainMap, (vUv + fract(velocity * time)) * tile) * tint;
    vec2 uv = (vUv + fract(maskVelocity * time)) * maskTile;
    vec3 mask = mix(texture2D(maskA, uv).rgb, texture2D(maskB, uv).rgb, maskMix);
    vec3 halfVector = normalize(normalize(cameraPosition - vWorld) + toSun);
    color.rgb += mask * pow(max(halfVector.y, 0.0), 32.0);
    #ifdef FAR_WATER
      color.rgb = mix(color.rgb, fogColor, farFog(vSurfaceDepth));
    #endif
    gl_FragColor = color;
  }
`;

// Wading soldiers (xz, depth, activity) for contact foam and ripples.
const WADERS = 8;
const WADER_RIPPLES = /* glsl */ `
  uniform vec4 waders[${WADERS}];
  // Rings spreading from the legs of each wading soldier, as a slope.
  vec2 waderSlope(vec2 at, float time, out float near) {
    vec2 slope = vec2(0.0);
    near = 0.0;
    for (int i = 0; i < ${WADERS}; i++) {
      if (waders[i].w <= 0.0) continue;
      vec2 delta = at - waders[i].xy;
      float d = length(delta);
      near = max(near, (1.0 - smoothstep(0.35, 0.8, d)) * waders[i].w);
      float wave = cos(d * 26.0 - time * 7.5) * exp(-max(d - 0.25, 0.0) * 2.2) * smoothstep(0.15, 0.3, d);
      slope += delta / max(d, 0.01) * wave * 0.09 * waders[i].w;
    }
    return slope;
  }
`;

const REFRACTION = /* glsl */ `
  ${FAR_WATER_PARS}
  ${WADER_RIPPLES}
  uniform vec3 tint;
  uniform float quality;
  uniform sampler2D sceneColor;
  uniform sampler2D sceneDepth;
  uniform vec2 resolution;
  uniform vec2 cameraRange;
  uniform float underwater;
  uniform float time;
  varying float vSurfaceDepth;
  varying vec3 vWorld;
  float viewDepth(float depth) {
    return cameraRange.x * cameraRange.y / (cameraRange.y - depth * (cameraRange.y - cameraRange.x));
  }
  void main() {
    ${SURFACE_DEPTH}
    #ifdef FAR_WATER
    {
      // Past the near scene: what lies under it multiplied by the tint, or
      // (Enhanced) the colour the near pass gives water that deep, in the
      // fog (toward white where it multiplies).
      float fog = farFog(vSurfaceDepth);
      gl_FragColor = vec4(quality < 0.5 ? mix(tint, vec3(1.0), fog) : mix(tint * vec3(0.16, 0.36, 0.34), fogColor, fog), 1.0);
      return;
    }
    #endif
    if (quality < 0.5) { gl_FragColor = vec4(tint, 1.0); return; }
    vec2 screen = gl_FragCoord.xy / resolution;
    if (underwater > 0.5) {
      // The underside transmits the view above it; it must never reuse the
      // last above-water planar reflection when the eye crosses the surface.
      gl_FragColor = vec4(texture2D(sceneColor, screen).rgb * mix(vec3(1.0), tint, 0.25), 1.0);
      return;
    }
    float surface = max(vSurfaceDepth, 0.0001);
    float thickness = max(0.0, viewDepth(texture2D(sceneDepth, screen).r) - surface);
    vec2 bend = vec2(sin(vWorld.z * 1.2 + time), cos(vWorld.x * 0.9 - time * 0.8));
    vec2 refracted = clamp(screen + bend * 0.003 * min(thickness, 1.0), vec2(0.001), vec2(0.999));
    // Never pull an above-water object into the refracted silhouette.
    if (viewDepth(texture2D(sceneDepth, refracted).r) < surface) refracted = screen;
    vec3 transmission = exp(-thickness * vec3(0.22, 0.085, 0.13));
    vec3 deep = tint * vec3(0.16, 0.36, 0.34);
    vec3 color = mix(deep, texture2D(sceneColor, refracted).rgb, transmission);
    float caustic = pow(max(0.0, sin(vWorld.x * 2.5 + time) * sin(vWorld.z * 2.8 - time * 0.8)), 8.0);
    color += vec3(0.045, 0.07, 0.055) * caustic * exp(-thickness * 0.65);
    float edge = (1.0 - smoothstep(0.02, 0.55, thickness)) * smoothstep(0.0, 0.04, thickness);
    float foam = edge * (0.3 + 0.15 * sin(vWorld.x * 13.0 + vWorld.z * 11.0 + time * 1.7));
    color = mix(color, vec3(0.55, 0.64, 0.59), foam);
    // White water hugging whatever breaks the surface near a wading soldier
    // (his legs), broken up by moving noise, and the crests of his ripples.
    float near;
    vec2 ripple = waderSlope(vWorld.xz, time, near);
    float churn = 0.55 + 0.45 * sin(vWorld.x * 37.0 + time * 4.1) * sin(vWorld.z * 31.0 - time * 3.3);
    float hug = near * (1.0 - smoothstep(0.015, 0.14, thickness)) * smoothstep(0.0, 0.01, thickness);
    float crest = clamp(length(ripple) * 4.0, 0.0, 1.0) * near;
    color = mix(color, vec3(0.86, 0.92, 0.9), clamp(hug * churn * 1.1 + crest * 0.18, 0.0, 0.9));
    gl_FragColor = vec4(color, 1.0);
  }
`;

const REFLECTION = /* glsl */ `
  ${FAR_WATER_PARS}
  ${WADER_RIPPLES}
  uniform mat4 projectionMatrix;
  uniform sampler2D sceneDepth;
  uniform sampler2D sceneColor;
  uniform float screenSpace;
  uniform vec2 resolution;
  uniform sampler2D reflection;
  uniform mat4 reflectionMatrix;
  uniform sampler2D normalA;
  uniform sampler2D normalB;
  uniform float normalMix;
  uniform sampler2D bumpA;
  uniform sampler2D bumpB;
  uniform float bumpMix;
  uniform float distort;
  uniform float viewYaw;
  uniform vec2 tile;
  uniform vec3 toSun;
  uniform float reflectAmount;
  uniform vec2 fresnel;
  uniform float quality;
  uniform float time;
  uniform vec3 sunColor;
  uniform vec2 cameraRange;
  varying float vSurfaceDepth;
  varying vec3 vWorld;
  varying vec2 vUv;
  void main() {
    ${SURFACE_DEPTH}
    vec2 uv = vUv * tile;
    vec3 n = mix(texture2D(normalA, uv).rgb, texture2D(normalB, uv).rgb, normalMix) * 2.0 - 1.0;
    // The maps hold x in red, z in green, and up in blue.
    vec3 N = normalize(vec3(n.r, n.b, n.g));
    if (quality > 0.5) {
      // Cross-flowing world-space waves remove cell seams and visibly repeat
      // less than the original low-resolution flipbook on its own.
      vec2 flow = vWorld.xz * 0.22;
      vec3 detail = texture2D(normalA, flow + vec2(time * 0.018, -time * 0.011)).rgb * 2.0 - 1.0;
      vec3 crossFlow = texture2D(normalB, flow * 2.17 + vec2(-time * 0.013, time * 0.019)).rgb * 2.0 - 1.0;
      vec2 slope = vec2(
        cos(dot(vWorld.xz, vec2(0.72, 0.38)) + time * 0.95),
        cos(dot(vWorld.xz, vec2(-0.31, 1.13)) - time * 1.3)) * 0.075;
      float near;
      slope += waderSlope(vWorld.xz, time, near);
      N = normalize(vec3(N.xz * 0.35 + detail.rg * 0.22 + crossFlow.rg * 0.09 + slope, 1.0)).xzy;
    }
    vec3 V = normalize(cameraPosition - vWorld);
    vec3 H = normalize(V + toSun);
    float spec = pow(clamp(dot(N, H), 0.0, 1.0), 8.0);
    float grazing = 1.0 - clamp(dot(N, V), 0.0, 1.0);
    float F = fresnel.x + (fresnel.y - fresnel.x) * grazing * grazing;
    vec4 clip = reflectionMatrix * vec4(vWorld, 1.0);
    vec2 at = clip.xy / clip.w;
    if (distort > 0.0) {
      vec2 d = (mix(texture2D(bumpA, uv).rg, texture2D(bumpB, uv).rg, bumpMix) * 255.0 - 128.0) / 127.0;
      float c = cos(viewYaw);
      float s = sin(viewYaw);
      at += distort * vec2(c * d.x - s * d.y, s * d.x + c * d.y);
    }
    if (quality > 0.5) {
      // Fine normal distortion and energy-bounded highlights, with derivative
      // roughness to suppress shimmering subpixel sun glints in motion.
      at += N.xz * 0.012;
      float variance = dot(dFdx(N), dFdx(N)) + dot(dFdy(N), dFdy(N));
      float exponent = mix(90.0, 14.0, clamp(variance * 50.0, 0.0, 1.0));
      spec = pow(max(dot(N, H), 0.0), exponent) * 0.7;
      F = mix(max(0.04, fresnel.x * 0.4), fresnel.y, pow(grazing, 5.0));
    }
    vec3 mirrored = texture2D(reflection, clamp(at, vec2(0.002), vec2(0.998))).rgb;
    #ifdef FAR_WATER
    {
      // Past the near scene there is nothing near to reflect or meet.
      vec3 glint = quality > 0.5 ? sunColor * spec : vec3(spec);
      gl_FragColor = vec4(mix(mirrored * reflectAmount + glint, fogColor, farFog(vSurfaceDepth)), F);
      return;
    }
    #endif
    if (screenSpace > 0.5) {
      // High: the near scene comes from the opaque colour/depth copy the
      // water already takes, by a short march along the reflected ray in
      // view space, instead of a second (mirrored) render of the whole near
      // scene. Off-screen rays keep the far scene's planar mirror.
      vec3 viewPos = (viewMatrix * vec4(vWorld, 1.0)).xyz;
      vec3 ray = reflect(normalize(viewPos), normalize(mat3(viewMatrix) * N));
      vec3 p = viewPos;
      float stride = 0.2, found = 0.0;
      vec2 hit = vec2(0.0);
      for (int i = 0; i < 24; i++) {
        p += ray * stride;
        stride *= 1.28;
        vec4 clip = projectionMatrix * vec4(p, 1.0);
        if (clip.w <= 0.0) break;
        vec2 uv = clip.xy / clip.w * 0.5 + 0.5;
        if (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0)))) break;
        float stored = texture2D(sceneDepth, uv).r;
        float scene = cameraRange.x * cameraRange.y / (cameraRange.y - stored * (cameraRange.y - cameraRange.x));
        float depth = -p.z;
        if (depth > scene && depth - scene < stride * 1.6 + 0.25) {
          vec2 edge = min(uv, 1.0 - uv);
          found = smoothstep(0.0, 0.06, min(edge.x, edge.y)) * (1.0 - smoothstep(14.0, 24.0, float(i)));
          hit = uv;
          break;
        }
      }
      mirrored = mix(mirrored, texture2D(sceneColor, hit).rgb, found);
    }
    vec3 highlight = quality > 0.5 ? sunColor * spec : vec3(spec);
    if (quality > 0.5) {
      // Where a leg, wall or step meets the surface the mirror fades, so the
      // contact shows its foam and what's under it instead of a hard edge.
      float stored = texture2D(sceneDepth, gl_FragCoord.xy / resolution).r;
      float behind = cameraRange.x * cameraRange.y / (cameraRange.y - stored * (cameraRange.y - cameraRange.x));
      float meet = smoothstep(0.0, 0.18, behind - vSurfaceDepth);
      F *= meet;
      highlight *= meet;
    }
    gl_FragColor = vec4(mirrored * reflectAmount + highlight, F);
  }
`;

const UNDERWATER_FRAGMENT = /* glsl */ `
  uniform sampler2D sceneColor;
  uniform sampler2D sceneDepth;
  uniform vec2 cameraRange;
  uniform vec3 tint;
  uniform float time;
  varying vec2 vUv;
  void main() {
    vec2 drift = vec2(sin(vUv.y * 25.0 + time * 0.7), cos(vUv.x * 21.0 + time * 0.55)) * 0.0007;
    vec2 at = clamp(vUv + drift, vec2(0.001), vec2(0.999));
    float z = texture2D(sceneDepth, at).r;
    float distance = cameraRange.x * cameraRange.y / (cameraRange.y - z * (cameraRange.y - cameraRange.x));
    vec3 source = texture2D(sceneColor, at).rgb;
    // Nearby surfaces remain legible; distant objects disappear into colored
    // scattering instead of applying the same dark tint at every distance.
    vec3 transmission = exp(-distance * vec3(0.105, 0.045, 0.065));
    vec3 scattering = tint * vec3(0.3, 0.55, 0.52);
    gl_FragColor = vec4(mix(scattering, source * vec3(0.86, 0.98, 0.93), transmission), 1.0);
  }
`;

const RIPPLE_FRAGMENT = /* glsl */ `
  uniform sampler2D map;
  varying vec2 vUv;
  varying float vAlpha;
  void main() {
    gl_FragColor = vec4(1.0, 1.0, 1.0, texture2D(map, vUv).a * vAlpha);
  }
`;
const RIPPLE_VERTEX = /* glsl */ `
  attribute float alpha;
  varying vec2 vUv;
  varying float vAlpha;
  void main() {
    vUv = uv;
    vAlpha = alpha;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
`;

// Whether the water is a sea rather than a pool or canal: its cells reach
// all four edges of the water grid (the map ends in water on every side).
export function isSea(data) {
  const n = data.grid;
  if (!n || !data.cells?.length) return false;
  let west = false, east = false, north = false, south = false;
  for (const [i, j] of data.cells) {
    if (i === 0) west = true;
    if (i === n - 1) east = true;
    if (j === 0) north = true;
    if (j === n - 1) south = true;
  }
  return west && east && north && south;
}

export class Water extends WaterState {
  // data: the map's water (tools/zero_water.py); texture(name) gives a map
  // texture; graphics draws the scene; weather is the map's (an Enhanced
  // ocean storms only where it rains, ocean-surface.js).
  constructor({ data, texture, graphics, weather = null }) {
    super(data);
    this.data = data;
    this.graphics = graphics;
    const look = data.look;
    // The original draws an ocean where the look asks (OceanEnable). A sea
    // the look leaves flat (Kashyyyk's: water out to every edge of its
    // grid) takes the Enhanced ocean too, while Enhanced ocean effects are
    // on; Classic keeps the flat layer the 2004 game drew there.
    const oceanLook = (look.OceanEnable?.[0] || 0) > 0;
    const sea = !oceanLook && isSea(data);
    const ocean = oceanLook || sea;
    this.height = data.height;
    this.cell = data.cell;
    this.offset = data.offset;
    this.grid = data.grid;
    this.cells = new Set(data.cells.map(([i, j]) => j * data.grid + i));
    this.bobbing = (look.OscillationEnable?.[0] ?? 1) > 0;
    this.time = 0;
    this.wave = 0;
    const scale = data.scale || 1;
    const tile = look.Tile || [1, 1];
    this.frames = (key) => {
      const set = look[key];
      if (!set || set.length < 3) return null;
      const textures = Array.from({ length: Math.round(set[1]) }, (_, k) => texture(`${set[0]}${k}`)).filter(Boolean);
      return textures.length ? { textures, fps: set[2] } : null;
    };
    this.normals = this.frames("NormalMapTextures");
    this.bumps = this.frames("BumpMapTextures");
    this.masks = this.frames("SpecularMaskTextures");

    // The surface: a grid of PatchDivisions x scale quads per cell, and the
    // patched cells' own triangles, in world space at the layer's height.
    const divisions = look.PatchDivisions || [16, 16];
    const geometry = this.surface(ocean ? 1 : Math.max(1, Math.round(divisions[0] * scale)),
      ocean ? 1 : Math.max(1, Math.round((divisions[1] ?? divisions[0]) * scale)));
    const c0 = rgba(look.RefractionColor);
    const c1 = rgba(look.ReflectionColor);
    const fresnel = look.FresnelMinMax || [0.2, 0.9];

    this.target = new THREE.WebGLRenderTarget(REFLECTION_SIZE[0], REFLECTION_SIZE[1], { type: THREE.UnsignedByteType });
    this.mirror = new THREE.PerspectiveCamera();
    this.plainProjection = new THREE.Matrix4();
    this.obliqueProjection = new THREE.Matrix4();
    this.reflectionMatrix = new THREE.Matrix4();
    this.tintPass = new THREE.Mesh(
      geometry,
      new THREE.ShaderMaterial({
        uniforms: {
          tint: { value: new THREE.Vector3(c0[0], c0[1], c0[2]) },
          quality: { value: 0 }, time: { value: 0 }, underwater: { value: 0 },
          sceneColor: { value: null }, sceneDepth: { value: null },
          resolution: { value: new THREE.Vector2(1, 1) },
          cameraRange: { value: new THREE.Vector2(0.5, 200) },
          waders: { value: Array.from({ length: WADERS }, () => new THREE.Vector4()) },
        },
        vertexShader: VERTEX,
        fragmentShader: REFRACTION,
        transparent: true,
        depthWrite: true,
        side: THREE.DoubleSide,
        forceSinglePass: true,
        blending: THREE.CustomBlending,
        blendEquation: THREE.AddEquation,
        blendSrc: THREE.DstColorFactor,
        blendDst: THREE.OneMinusSrcAlphaFactor,
      }),
    );
    const blank = new THREE.DataTexture(new Uint8Array([128, 128, 255, 255]), 1, 1);
    blank.needsUpdate = true;
    this.mirrorPass = new THREE.Mesh(
      geometry,
      new THREE.ShaderMaterial({
        uniforms: {
          reflection: { value: this.target.texture },
          quality: { value: 0 },
          time: { value: 0 },
          sunColor: lighting.sunColor,
          cameraRange: this.tintPass.material.uniforms.cameraRange,
          // Shared with the tint pass: its opaque depth copy and the waders.
          sceneDepth: this.tintPass.material.uniforms.sceneDepth,
          sceneColor: this.tintPass.material.uniforms.sceneColor,
          screenSpace: { value: 0 },
          resolution: this.tintPass.material.uniforms.resolution,
          waders: this.tintPass.material.uniforms.waders,
          reflectionMatrix: { value: this.reflectionMatrix },
          normalA: { value: this.normals?.textures[0] || blank },
          normalB: { value: this.normals?.textures[0] || blank },
          normalMix: { value: 0 },
          bumpA: { value: this.bumps?.textures[0] || blank },
          bumpB: { value: this.bumps?.textures[0] || blank },
          bumpMix: { value: 0 },
          distort: { value: this.bumps ? DISTORT : 0 },
          viewYaw: { value: 0 },
          tile: { value: new THREE.Vector2(tile[0] * scale, (tile[1] ?? tile[0]) * scale) },
          toSun: { value: new THREE.Vector3(0, 1, 0) },
          reflectAmount: { value: c1[3] },
          fresnel: { value: new THREE.Vector2(fresnel[0], fresnel[1]) },
        },
        vertexShader: VERTEX,
        fragmentShader: REFLECTION,
        transparent: true,
        depthWrite: false,
        side: THREE.DoubleSide,
        forceSinglePass: true,
      }),
    );
    // After the opaque scene, ahead of everything else that blends.
    this.tintPass.renderOrder = -20;
    this.mirrorPass.renderOrder = -19;
    this.group = new THREE.Group();
    this.group.add(this.tintPass, this.mirrorPass);
    const mainMap = look.MainTexture?.[0] ? texture(look.MainTexture[0]) : null;
    this.lowPass = null;
    if (mainMap) {
      const black = new THREE.DataTexture(new Uint8Array([0, 0, 0, 255]), 1, 1);
      black.needsUpdate = true;
      const pair = (values, multiplier = 1) => new THREE.Vector2(
        (values?.[0] ?? 0) * multiplier, (values?.[1] ?? values?.[0] ?? 0) * multiplier);
      this.lowPass = new THREE.Mesh(geometry, new THREE.ShaderMaterial({
        vertexShader: VERTEX, fragmentShader: TEXTURED_WATER,
        uniforms: {
          mainMap: { value: mainMap }, tint: { value: new THREE.Vector4(...c0) },
          tile: { value: pair(tile, scale) }, velocity: { value: pair(look.Velocity, 1 / scale) },
          maskTile: { value: pair(look.SpecularMaskTile || [1, 1], scale) },
          maskVelocity: { value: pair(look.SpecularMaskScrollSpeed, 1 / scale) },
          maskA: { value: this.masks?.textures[0] || black },
          maskB: { value: this.masks?.textures[0] || black }, maskMix: { value: 0 },
          time: this.mirrorPass.material.uniforms.time,
          toSun: this.mirrorPass.material.uniforms.toSun,
          cameraRange: this.tintPass.material.uniforms.cameraRange,
        },
        transparent: true, depthWrite: true, side: THREE.DoubleSide, forceSinglePass: true,
      }));
      this.lowPass.frustumCulled = false;
      this.lowPass.renderOrder = -20;
      this.group.add(this.lowPass);
    }
    for (const mesh of [this.tintPass, this.mirrorPass]) mesh.frustumCulled = false;
    graphics.scene.add(this.group);

    // The far scene's water. The original draws the surface again in the far
    // scene, after the low-res terrain and in its fog (the sky's draw hands
    // the water's draw the far flag), out to the look's own FarSceneRange,
    // or else the sky's, when that reaches past the near
    // scene. Without it the far terrain's sea bed showed bare past the near
    // scene (both Kashyyyks). The same passes on a coarse grid, drawn only
    // from the eye's far camera (graphics.eyeLayer): mirrors leave it out.
    this.farReach = look.FarSceneRange?.[0] || 0;
    const farGeometry = this.surface(1, 1);
    this.farUniforms = {
      cameraRange: { value: new THREE.Vector2(2, 1500) },
      surfaceRange: { value: new THREE.Vector2(1e9, 0) },
      fogColor: lighting.fogColor,
      fogRange: lighting.fogRange,
    };
    this.farView = new THREE.PerspectiveCamera();
    this.farFrustum = new THREE.Frustum();
    this.farTint = this.farPass(this.tintPass, farGeometry);
    this.farMirror = this.farPass(this.mirrorPass, farGeometry);
    this.farLow = this.lowPass ? this.farPass(this.lowPass, farGeometry) : null;
    this.farGroup = new THREE.Group();
    this.farGroup.name = "far-water";
    this.farGroup.add(...[this.farTint, this.farMirror, this.farLow].filter(Boolean));
    this.farGroup.visible = false;
    graphics.farScene?.add(this.farGroup);
    this.bounds = new THREE.Box3().setFromBufferAttribute(geometry.getAttribute("position"));
    this.bounds.min.y -= 1;
    this.bounds.max.y += 1;
    // Each water cell's own box: one box around a whole canal is "in view"
    // from most of the city, and with it the scene's second (mirrored)
    // render ran nearly every frame.
    this.cellBounds = data.cells.map(([i, j]) => new THREE.Box3(
      new THREE.Vector3(i * data.cell - data.offset[0], data.height - 1, j * data.cell - data.offset[1]),
      new THREE.Vector3((i + 1) * data.cell - data.offset[0], data.height + 1, (j + 1) * data.cell - data.offset[1])));

    // Rings.
    this.ripples = [];
    this.rings = this.ringMesh(texture("FXrippl1"));
    graphics.scene.add(this.rings);

    // The tint under the surface.
    const u = rgba(look.UnderwaterColor, [255, 255, 255, 0]);
    this.underwater = new THREE.Mesh(
      new THREE.PlaneGeometry(2, 2),
      new THREE.ShaderMaterial({
        uniforms: { color: { value: new THREE.Vector4(...u) } },
        vertexShader: "void main() { gl_Position = vec4(position.xy, 0.0, 1.0); }",
        fragmentShader: "uniform vec4 color; void main() { gl_FragColor = color; }",
        transparent: true,
        depthTest: false,
        depthWrite: false,
      }),
    );
    this.underwater.frustumCulled = false;
    this.underwaterScene = new THREE.Scene();
    this.underwaterScene.add(this.underwater);
    this.passCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);

    // Enhanced submerged views are composited inside the scene, after other
    // transparency and before the final AA. Capture here even when the surface
    // is out of view (e.g. looking at the riverbed), so depth never goes stale.
    const surfaceUniforms = this.tintPass.material.uniforms;
    this.underwaterFog = new THREE.Mesh(
      new THREE.PlaneGeometry(2, 2),
      new THREE.ShaderMaterial({
        uniforms: {
          sceneColor: surfaceUniforms.sceneColor, sceneDepth: surfaceUniforms.sceneDepth,
          cameraRange: surfaceUniforms.cameraRange, tint: surfaceUniforms.tint, time: surfaceUniforms.time,
        },
        vertexShader: "varying vec2 vUv; void main() { vUv = uv; gl_Position = vec4(position.xy, 0.0, 1.0); }",
        fragmentShader: UNDERWATER_FRAGMENT,
        transparent: true, blending: THREE.NoBlending, depthTest: false, depthWrite: false,
      }),
    );
    this.underwaterFog.frustumCulled = false;
    this.underwaterFog.renderOrder = 1e8;
    this.underwaterFog.visible = false;
    this.underwaterFog.onBeforeRender = (renderer, scene, camera) => this.captureSurface(renderer, camera);
    graphics.scene.add(this.underwaterFog);

    this.frustum = new THREE.Frustum();
    this.matrix = new THREE.Matrix4();
    this.forward = new THREE.Vector3();
    this.up = new THREE.Vector3();
    this.at = new THREE.Vector3();
    this.plane = new THREE.Plane();
    this.clip = new THREE.Vector4();
    this.q = new THREE.Vector4();
    this.reflectionFar = null;
    this.surfaceBuffer = null;
    // Where the far reflection was last drawn from (held every other frame).
    this.reflectedEye = new THREE.Vector3();
    this.reflectedForward = new THREE.Vector3();
    this.reflectedSize = new THREE.Vector2();
    this.reflectedLevel = NaN;
    this.reflectFrames = 0;
    this.tintPass.onBeforeRender = (renderer, scene, camera) => this.captureSurface(renderer, camera);
    this.waders = new Map();
    this.waderList = [];
    this.oceanSurface = ocean ? new OceanSurface({ data, texture, graphics, weather }) : null;
    this.oceanAlways = oceanLook;
    this.applyQuality();
    graphics.settings.onChange((key) => { if (key === "waterQuality" || key === "oceanEffects") this.applyQuality(); });
  }

  // A near pass's copy for the far scene: its shader as FAR_WATER, its
  // uniforms shared but for the far camera's range, the span it draws in and
  // the far scene's fog.
  farPass(pass, geometry) {
    const source = pass.material;
    const material = new THREE.ShaderMaterial({
      vertexShader: source.vertexShader,
      fragmentShader: source.fragmentShader,
      uniforms: { ...source.uniforms, ...this.farUniforms },
      defines: { FAR_WATER: "" },
      transparent: source.transparent, depthWrite: source.depthWrite, side: source.side, forceSinglePass: true,
      blending: source.blending, blendEquation: source.blendEquation, blendSrc: source.blendSrc, blendDst: source.blendDst,
    });
    const mesh = new THREE.Mesh(geometry, material);
    mesh.name = "far-water";
    mesh.renderOrder = pass.renderOrder;
    mesh.frustumCulled = false;
    mesh.layers.set(this.graphics.eyeLayer ?? 4);
    mesh.onBeforeRender = (renderer, scene, camera) => this.placeFar(camera);
    return mesh;
  }

  // Where the far water draws, in view depth: from the near scene's end out
  // to the look's FarSceneRange (scaled with the view distance, as the far
  // scene's own reach is) or else the far scene's reach. None when the far
  // scene doesn't reach past the near scene's fade start, as the sky's draw
  // decides for the low-res terrain and the water together (Yavin 4:
  // Temple, both Rhen Vars, Jabba's Palace).
  farSpan(farCamera = this.graphics.farCamera) {
    const end = farCamera?.far || 0;
    const [fadeStart, fadeEnd] = lighting.fade.value.toArray();
    if (!(end > fadeStart)) return [fadeEnd, 0];
    const reach = this.graphics.farReach?.[1];
    const scale = reach > 0 ? end / reach : 1;
    return [fadeEnd, this.farReach > 0 ? this.farReach * scale : end];
  }

  placeFar(camera) {
    const [start, end] = this.farSpan(camera);
    this.farUniforms.cameraRange.value.set(camera.near, camera.far);
    this.farUniforms.surfaceRange.value.set(start, end);
  }

  // Whether any water cell lies in the eye's view between the far span's ends.
  farInView(camera) {
    const [start, end] = this.farSpan();
    if (!(end > start)) return false;
    const view = this.farView;
    view.fov = camera.fov;
    view.aspect = camera.aspect;
    view.near = Math.max(camera.near, 0.1);
    view.far = end;
    view.updateProjectionMatrix();
    this.farFrustum.setFromProjectionMatrix(this.matrix.multiplyMatrices(view.projectionMatrix, camera.matrixWorldInverse));
    if (!this.farFrustum.intersectsBox(this.bounds)) return false;
    return this.cellBounds.some((box) => this.farFrustum.intersectsBox(box));
  }

  // The ocean drawn in place of the flat surface: always where the look
  // asks for one, and on an Enhanced sea while ocean effects are enhanced.
  get ocean() {
    const surface = this.oceanSurface;
    if (!surface) return null;
    return this.oceanAlways || this.graphics.settings.get("oceanEffects") === "enhanced" ? surface : null;
  }

  applyQuality() {
    const name = this.graphics.settings.get("waterQuality");
    this.quality = WATER_QUALITIES[name] || WATER_QUALITIES.high;
    if (this.oceanSurface) {
      const active = Boolean(this.ocean);
      this.oceanSurface.near.visible = this.oceanSurface.far.visible = active;
      if (active) return; // textured ocean does not need planar reflection targets
    }
    const enhanced = this.quality.scale > 0;
    if (this.lowPass) this.lowPass.visible = !enhanced;
    this.tintPass.visible = enhanced || !this.lowPass;
    this.mirrorPass.visible = enhanced || !this.lowPass;
    if (!enhanced) this.underwaterFog.visible = false;
    this.mirrorPass.material.uniforms.quality.value = enhanced ? 1 : 0;
    this.mirrorPass.material.uniforms.screenSpace.value = enhanced && !this.quality.planarNear ? 1 : 0;
    this.tintPass.material.uniforms.quality.value = enhanced ? 1 : 0;
    this.tintPass.material.blending = enhanced ? THREE.NoBlending : THREE.CustomBlending;
    // The far scene's passes follow (their quality uniforms are these).
    if (this.farLow) this.farLow.visible = this.lowPass.visible;
    this.farTint.visible = this.tintPass.visible;
    this.farMirror.visible = this.mirrorPass.visible;
    this.farTint.material.blending = this.tintPass.material.blending;
    if (enhanced && !this.surfaceBuffer) {
      this.surfaceBuffer = new THREE.WebGLRenderTarget(1, 1, {
        depthTexture: new THREE.DepthTexture(1, 1, THREE.UnsignedIntType),
      });
      this.tintPass.material.uniforms.sceneColor.value = this.surfaceBuffer.texture;
      this.tintPass.material.uniforms.sceneDepth.value = this.surfaceBuffer.depthTexture;
    }
    if (!enhanced && this.surfaceBuffer) {
      this.surfaceBuffer.dispose();
      this.surfaceBuffer = null;
      this.tintPass.material.uniforms.sceneColor.value = null;
      this.tintPass.material.uniforms.sceneDepth.value = null;
    }
    this.mirrorPass.material.uniforms.distort.value = this.bumps ? (enhanced ? DISTORT * 0.18 : DISTORT) : 0;
    const planar = enhanced && Boolean(this.quality.planarNear);
    if (planar && !this.reflectionFar) this.reflectionFar = new THREE.WebGLRenderTarget(1, 1);
    if (!planar && this.reflectionFar) {
      this.reflectionFar.dispose();
      this.reflectionFar = null;
    }
    this.resizeReflection();
  }

  captureSurface(renderer, camera) {
    if (!this.surfaceBuffer) return;
    const source = renderer.getRenderTarget();
    if (source !== this.graphics.sceneTarget) return;
    const target = this.surfaceBuffer;
    target.setSize(source.width, source.height);
    const gl = renderer.getContext();
    const framebuffer = drawFramebuffer(renderer, source);
    renderer.setRenderTarget(target);
    // r170 tracks READ separately but FRAMEBUFFER binds change it too.
    // Invalidate that cache entry before selecting the source for the blit.
    renderer.state.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
    renderer.state.bindFramebuffer(gl.READ_FRAMEBUFFER, framebuffer);
    // Resolve opaque color/depth before the water's first transparent draw;
    // no second geometry pass, including when the scene uses multisampling.
    gl.blitFramebuffer(0, 0, source.width, source.height, 0, 0, target.width, target.height,
      gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT, gl.NEAREST);
    renderer.setRenderTarget(source);
    const u = this.tintPass.material.uniforms;
    u.resolution.value.set(source.width, source.height);
    u.cameraRange.value.set(camera.near, camera.far);
  }

  resizeReflection() {
    const { renderer } = this.graphics;
    // The 3D frame's resolution (smaller than the canvas under an upscaler).
    const size = this.graphics.renderSize?.clone() ?? renderer.getDrawingBufferSize(new THREE.Vector2());
    const q = this.quality;
    const scale = Math.min(q.scale, q.maxSize / Math.max(size.x, size.y));
    const width = q.scale ? Math.max(1, Math.round(size.x * scale)) : REFLECTION_SIZE[0];
    const height = q.scale ? Math.max(1, Math.round(size.y * scale)) : REFLECTION_SIZE[1];
    this.target.setSize(width, height);
    this.reflectionFar?.setSize(width, height);
  }

  surface(divisions, rows = divisions) {
    const { cell, offset, height } = this;
    const positions = [];
    const uvs = [];
    const indices = [];
    for (const [i, j, patched] of this.data.cells) {
      if (patched) continue;
      const x0 = i * cell - offset[0];
      const z0 = j * cell - offset[1];
      const base = positions.length / 3;
      for (let b = 0; b <= rows; b++) {
        for (let a = 0; a <= divisions; a++) {
          positions.push(x0 + (a * cell) / divisions, height, z0 + (b * cell) / rows);
          uvs.push(a / divisions, b / rows);
        }
      }
      const row = divisions + 1;
      for (let b = 0; b < rows; b++) {
        for (let a = 0; a < divisions; a++) {
          const k = base + b * row + a;
          indices.push(k, k + row, k + 1, k + 1, k + row, k + row + 1);
        }
      }
    }
    for (const patch of this.data.patches || []) {
      const base = positions.length / 3;
      for (const p of patch.positions) positions.push(p[0], p[1], p[2]);
      for (const uv of patch.uvs) uvs.push(uv[0], uv[1]);
      for (const k of patch.indices) indices.push(base + k);
    }
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
    geometry.setAttribute("uv", new THREE.Float32BufferAttribute(uvs, 2));
    geometry.setIndex(indices);
    return geometry;
  }

  ringMesh(map) {
    const quads = RIPPLE_MAX * RIPPLE_SEGMENTS;
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(quads * 4 * 3), 3).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute("uv", new THREE.BufferAttribute(new Float32Array(quads * 4 * 2), 2).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute("alpha", new THREE.BufferAttribute(new Float32Array(quads * 4), 1).setUsage(THREE.DynamicDrawUsage));
    const index = new Uint32Array(quads * 6);
    for (let q = 0; q < quads; q++) index.set([q * 4, q * 4 + 1, q * 4 + 2, q * 4 + 2, q * 4 + 1, q * 4 + 3], q * 6);
    geometry.setIndex(new THREE.BufferAttribute(index, 1));
    geometry.setDrawRange(0, 0);
    const mesh = new THREE.Mesh(
      geometry,
      new THREE.ShaderMaterial({
        uniforms: { map: { value: map } },
        vertexShader: RIPPLE_VERTEX,
        fragmentShader: RIPPLE_FRAGMENT,
        transparent: true,
        depthWrite: false,
        side: THREE.DoubleSide,
        forceSinglePass: true,
      }),
    );
    mesh.renderOrder = -18;
    mesh.frustumCulled = false;
    return mesh;
  }

  // The gameplay step reports each soldier in the water every tick; note the
  // wet ones for the surface's contact foam and ripples (High/Ultra).
  soldierStep(dt, contact, feet, head, gun, velocity, splash) {
    const damage = super.soldierStep(dt, contact, feet, head, gun, velocity, splash);
    if (this.level > feet.y && this.over(feet.x, feet.z)) {
      let wader = this.waders.get(contact);
      if (!wader) this.waders.set(contact, (wader = { position: new THREE.Vector3(), speed: 0, seen: 0, depth: 0 }));
      wader.position.copy(feet);
      wader.speed = Math.hypot(velocity.x, velocity.z);
      wader.depth = this.level - feet.y;
      wader.seen = this.time;
    }
    return damage;
  }

  // The nearest wading soldiers, for the shaders.
  updateWaders(camera) {
    const list = this.waderList;
    list.length = 0;
    for (const [contact, wader] of this.waders) {
      if (this.time - wader.seen > 0.25) this.waders.delete(contact);
      else list.push(wader);
    }
    if (list.length > WADERS) list.sort((a, b) => a.position.distanceToSquared(camera.position) - b.position.distanceToSquared(camera.position));
    const uniform = this.tintPass.material.uniforms.waders.value;
    for (let i = 0; i < WADERS; i++) {
      const wader = list[i];
      if (wader) uniform[i].set(wader.position.x, wader.position.z, wader.depth, Math.min(1, 0.55 + wader.speed * 0.12));
      else uniform[i].w = 0;
    }
  }

  // A ring spreading from `position`, starting at `radius`, for `life` s.
  // velocity (a wading soldier's) shapes it into a wake.
  ripple(position, life, radius, velocity = null) {
    if (!this.over(position.x, position.z)) return;
    if (this.ripples.length >= RIPPLE_MAX) this.ripples.shift();
    this.ripples.push({ x: position.x, z: position.z, age: 0, life, radius, velocity: velocity ? velocity.clone() : null });
  }

  update(dt, camera) {
    this.ocean?.update(dt, camera);
    this.time += dt;
    this.wave = this.bobbing ? BOB * Math.sin(2 * Math.PI * BOB_RATE * this.time) : 0;
    this.group.position.y = this.farGroup.position.y = this.wave;
    const pick = (frames, uniformA, uniformB, uniformMix, pass = this.mirrorPass) => {
      if (!frames) return;
      const n = frames.textures.length;
      const f = frames.fps * this.time;
      const k = Math.floor(f);
      const u = pass.material.uniforms;
      u[uniformA].value = frames.textures[k % n];
      u[uniformB].value = frames.textures[(k + 1) % n];
      u[uniformMix].value = f - k;
    };
    pick(this.normals, "normalA", "normalB", "normalMix");
    pick(this.bumps, "bumpA", "bumpB", "bumpMix");
    if (this.lowPass) pick(this.masks, "maskA", "maskB", "maskMix", this.lowPass);
    const u = this.mirrorPass.material.uniforms;
    u.time.value = this.time;
    this.tintPass.material.uniforms.time.value = this.time;
    u.toSun.value.copy(lighting.sunDirection.value).negate().normalize();
    this.updateWaders(camera);
    camera.getWorldDirection(this.forward);
    u.viewYaw.value = Math.atan2(this.forward.x, this.forward.z) - Math.PI / 2;

    // Rings: grown and faded, dropped when spent or far from the camera.
    this.ripples = this.ripples.filter((r) => {
      r.age += dt;
      return r.age < r.life && Math.hypot(r.x - camera.position.x, r.z - camera.position.z) < RIPPLE_RANGE;
    });
    this.updateRings();
  }

  updateRings() {
    const geometry = this.rings.geometry;
    const pos = geometry.getAttribute("position");
    const uv = geometry.getAttribute("uv");
    const alpha = geometry.getAttribute("alpha");
    const y = this.level + RIPPLE_LIFT;
    let q = 0;
    for (const r of this.ripples) {
      const f = r.age / r.life;
      const k = f < 0.15 ? f / 0.15 : (1 - f) / 0.85;
      const a = Math.min(0.75, 0.25 * r.life * k);
      const radius = r.radius + RIPPLE_GROWTH * r.age;
      // A wake is a V: brighter ahead, shaped by how fast the wader moves.
      let shape = null;
      if (r.velocity) {
        const v = r.velocity;
        const speed = v.length();
        let base = a * (speed < 1.5 ? speed / 1.5 : 1);
        const m = Math.hypot(v.x, v.z) / 1.5;
        const g = 4 * (m - 1) ** 2 + 0.4;
        base *= g / (g + 1);
        const heading = Math.atan2(v.z, v.x);
        shape = { base, m, heading };
      }
      for (let s = 0; s < RIPPLE_SEGMENTS; s++) {
        const a0 = (s / RIPPLE_SEGMENTS) * 2 * Math.PI;
        const a1 = ((s + 1) / RIPPLE_SEGMENTS) * 2 * Math.PI;
        let segAlpha = a;
        if (shape) {
          const c = Math.cos((a0 + a1) / 2 - shape.heading);
          const w = shape.m <= 1 ? (1 - ((1 - c) * shape.m) ** 2) * (2 - shape.m) : 1 - (1 - c * shape.m) ** 2 / shape.m;
          segAlpha = shape.base * Math.max(w, 0);
        }
        if (segAlpha <= 1 / 255) continue;
        const o = q * 4;
        const outer = radius + RIPPLE_BAND;
        const inner = Math.max(radius - RIPPLE_BAND, 0);
        pos.setXYZ(o, r.x + Math.cos(a0) * outer, y, r.z + Math.sin(a0) * outer);
        pos.setXYZ(o + 1, r.x + Math.cos(a0) * inner, y, r.z + Math.sin(a0) * inner);
        pos.setXYZ(o + 2, r.x + Math.cos(a1) * outer, y, r.z + Math.sin(a1) * outer);
        pos.setXYZ(o + 3, r.x + Math.cos(a1) * inner, y, r.z + Math.sin(a1) * inner);
        uv.setXY(o, s * 0.25, 0);
        uv.setXY(o + 1, s * 0.25, 1);
        uv.setXY(o + 2, (s + 1) * 0.25, 0);
        uv.setXY(o + 3, (s + 1) * 0.25, 1);
        for (let v = 0; v < 4; v++) alpha.setX(o + v, segAlpha);
        q++;
      }
    }
    for (const attribute of [pos, uv, alpha]) attribute.needsUpdate = true;
    geometry.setDrawRange(0, q * 6);
  }

  // Render the mirror image into the reflection target: the far scene from
  // the camera mirrored in the surface, cut at the surface. Skipped when no
  // water is in view or the camera is under it.
  reflect(camera) {
    this.farGroup.visible = false;
    if (this.ocean) { this.group.visible = false; this.underwaterFog.visible = false; return; }
    this.graphics.updateScene?.();
    camera.updateMatrixWorld();
    this.matrix.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.frustum.setFromProjectionMatrix(this.matrix);
    const level = this.level;
    this.tintPass.material.uniforms.cameraRange.value.set(camera.near, camera.far);
    let visible = false;
    if (this.frustum.intersectsBox(this.bounds)) for (const box of this.cellBounds) if (this.frustum.intersectsBox(box)) { visible = true; break; }
    const below = camera.position.y < level;
    this.underwaterFog.visible = Boolean(this.quality.scale && below && this.over(camera.position.x, camera.position.z));
    this.tintPass.material.uniforms.underwater.value = below ? 1 : 0;
    this.group.visible = visible;
    // The far scene's water, seen from above (it takes the same mirror).
    const farVisible = !below && this.farInView(camera);
    this.farGroup.visible = farVisible;
    // A mirror camera on/below the plane has no valid above-water reflection.
    // In particular, do not draw yesterday's texture/projective matrix here.
    this.mirrorPass.visible = this.farMirror.visible = false;
    if (!(visible || farVisible) || this.lowPass?.visible || camera.position.y - level < 0.03) return;
    const graphics = this.graphics;
    const renderer = graphics.renderer;
    const far = graphics.farCamera;
    const mirror = this.mirror;
    // The far scene's mirror image (High, or the textureless fallback) is held every other frame
    // while the eye stays within 0.3 m and 2 degrees of where it was drawn
    // from: the sky and the far buildings are 100 m and more away, so a
    // frame's parallax is a fraction of a texel, and at rest the image is
    // the same one. Ultra mirrors the near scene (soldiers) every frame.
    this.resizeReflection();
    camera.getWorldDirection(this.forward);
    const planarNear = Boolean(this.reflectionFar && this.quality.planarNear);
    const held = !planarNear && this.reflectFrames++ % 2 === 1 && this.reflectedLevel === level
      && this.reflectedSize.x === this.target.width && this.reflectedSize.y === this.target.height
      && this.reflectedEye.distanceToSquared(camera.position) < 0.09 && this.reflectedForward.dot(this.forward) > 0.9994;
    if (held) { this.mirrorPass.visible = this.farMirror.visible = true; return; }
    mirror.fov = camera.fov;
    mirror.aspect = camera.aspect;
    mirror.near = this.reflectionFar ? camera.near : far.near || 2;
    mirror.far = far.far || camera.far;
    mirror.updateProjectionMatrix();
    mirror.position.copy(camera.position);
    mirror.position.y = 2 * level - camera.position.y;
    camera.getWorldDirection(this.forward);
    this.forward.y = -this.forward.y;
    this.up.set(0, 1, 0).applyQuaternion(camera.quaternion);
    this.up.y = -this.up.y;
    mirror.up.copy(this.up);
    mirror.lookAt(this.at.copy(mirror.position).add(this.forward));
    mirror.updateMatrixWorld();
    // The sky is drawn through this one (below).
    this.plainProjection.copy(mirror.projectionMatrix);

    // Clip at the surface by tilting the near plane onto it.
    this.plane.set(new THREE.Vector3(0, 1, 0), -level).applyMatrix4(mirror.matrixWorldInverse);
    this.clip.set(this.plane.normal.x, this.plane.normal.y, this.plane.normal.z, this.plane.constant);
    const p = mirror.projectionMatrix;
    const e = p.elements;
    this.q.set((Math.sign(this.clip.x) + e[8]) / e[0], (Math.sign(this.clip.y) + e[9]) / e[5], -1, (1 + e[10]) / e[14]);
    const clipDenominator = this.clip.dot(this.q);
    if (Math.abs(clipDenominator) < 1e-6 || !Number.isFinite(clipDenominator)) return;
    this.clip.multiplyScalar(2 / clipDenominator);
    e[2] = this.clip.x;
    e[6] = this.clip.y;
    e[10] = this.clip.z + 1;
    e[14] = this.clip.w;

    // World point -> the target's 0..1 square.
    this.reflectionMatrix.set(0.5, 0, 0, 0.5, 0, 0.5, 0, 0.5, 0, 0, 0.5, 0.5, 0, 0, 0, 1);
    this.reflectionMatrix.multiply(p).multiply(mirror.matrixWorldInverse);

    this.reflectedEye.copy(camera.position);
    camera.getWorldDirection(this.reflectedForward);
    this.reflectedSize.set(this.target.width, this.target.height);
    this.reflectedLevel = level;
    const previous = renderer.getRenderTarget();
    const sky = graphics.sky;
    const skyAt = sky ? sky.position.clone() : null;
    // The sky's other models (graphics.skyLayers) are mirrored with it.
    const layersAt = (graphics.skyLayers || []).map(({ mesh }) => mesh.position.clone());
    const clearColor = renderer.getClearColor(new THREE.Color());
    const clearAlpha = renderer.getClearAlpha();
    const autoClear = renderer.autoClear;
    const autoShadows = renderer.shadowMap.autoUpdate;
    const backgroundMap = graphics.background.material.uniforms.map.value;
    const farMap = lighting.farMap.value;
    const resolution = lighting.resolution.value.clone();
    const softParticles = particleDepth.enabled.value;
    const ringsVisible = this.rings.visible;
    try {
      if (sky) sky.position.copy(mirror.position);
      for (const { mesh } of graphics.skyLayers || []) if (mesh !== sky) mesh.position.copy(mirror.position);
      renderer.autoClear = true;
      renderer.shadowMap.autoUpdate = false;
      const near = Boolean(this.reflectionFar && this.quality.planarNear);
      renderer.setRenderTarget(near ? this.reflectionFar : this.target);
      renderer.setClearColor(0x000000, 0);
      // The sky first, through the plain mirror: its models stand around the
      // mirrored eye, under the surface, where the oblique near plane cut
      // every one of them away and left the mirror black above the horizon.
      // Then the rest of the far scene over it (the sky writes no depth).
      const skyMeshes = (graphics.skyLayers || []).map(({ mesh }) => mesh).filter((mesh) => mesh.visible);
      const oblique = this.obliqueProjection.copy(mirror.projectionMatrix);
      try {
        if (skyMeshes.length) {
          mirror.projectionMatrix.copy(this.plainProjection);
          mirror.layers.set(MIRROR_SKY_LAYER);
          for (const mesh of skyMeshes) mesh.layers.enable(MIRROR_SKY_LAYER);
          renderer.render(graphics.farScene, mirror);
          for (const mesh of skyMeshes) mesh.visible = false;
          mirror.layers.set(0);
          mirror.projectionMatrix.copy(oblique);
          renderer.autoClear = false;
        }
        renderer.render(graphics.farScene, mirror);
      } finally {
        for (const mesh of skyMeshes) { mesh.layers.disable(MIRROR_SKY_LAYER); mesh.visible = true; }
        mirror.layers.set(0);
        mirror.projectionMatrix.copy(oblique);
        renderer.autoClear = true;
      }
      if (near) {
        // Full detail architecture and characters share the same mirrored
        // camera. Hide the water to avoid sampling its own render target.
        this.group.visible = false;
        this.rings.visible = false;
        particleDepth.enabled.value = 0;
        lighting.resolution.value.set(this.target.width, this.target.height);
        lighting.farMap.value = this.reflectionFar.texture;
        graphics.background.material.uniforms.map.value = this.reflectionFar.texture;
        renderer.setRenderTarget(this.target);
        renderer.render(graphics.scene, mirror);
      }
    } finally {
      this.group.visible = visible;
      this.mirrorPass.visible = this.farMirror.visible = true;
      this.rings.visible = ringsVisible;
      lighting.farMap.value = farMap;
      lighting.resolution.value.copy(resolution);
      particleDepth.enabled.value = softParticles;
      graphics.background.material.uniforms.map.value = backgroundMap;
      renderer.shadowMap.autoUpdate = autoShadows;
      renderer.autoClear = autoClear;
      renderer.setClearColor(clearColor, clearAlpha);
      renderer.setRenderTarget(previous);
      if (sky) sky.position.copy(skyAt);
      (graphics.skyLayers || []).forEach(({ mesh }, i) => mesh.position.copy(layersAt[i]));
    }
  }

  // The screen's tint with the eye under the surface over water.
  drawUnderwater(renderer, camera) {
    if (this.quality.scale) return;
    if (camera.position.y >= this.level || !this.over(camera.position.x, camera.position.z)) return;
    const autoClear = renderer.autoClear;
    renderer.autoClear = false;
    renderer.setRenderTarget(null);
    renderer.render(this.underwaterScene, this.passCamera);
    renderer.autoClear = autoClear;
  }
}
