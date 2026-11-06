import * as THREE from "three";
import { SkyHost, loaded } from "./environment/module.js";
import { SkyClass as SKY_CLASS, SkyConfig as SKY_CONFIG, SkyFrame as SKY_FRAME, SkyResult as SKY_RESULT } from "./environment/layout.js";
import { lighting } from "./materials.js";
import { modelPack } from "./models.js";
import { motionUniforms, motionVectors } from "./motion-vectors.js";
import { SkyProbe } from "./sky-probe.js";

// The sky's flyers: the sky config's SkyObjects (maps/<map>/sky/flyers.json,
// tools/zero_sky.py), flown as the 2004 game flies them (SkyObjectClass;
// docs/formats.md, "Sky flyers"):
//
//   - A class keeps NumObjects ships in the sky all the time. A ship starts
//     Distance from the world's origin in a random direction, Height above
//     it, fading in over 4 s. It heads across that direction, either way,
//     turned inward by a random InDirectionFactor / 2 to InDirectionFactor
//     times the inward direction, level, nose along its heading.
//   - It flies at a velocity drawn once in its own axes (VelocityX right,
//     VelocityY up, VelocityZ forward, each between its min and max), which
//     Acceleration (in its axes too) adds to every frame. It never turns or
//     banks.
//   - Once it is more than Distance + 10 m from the origin across the
//     ground, or older than LifeTime (its age starts at 80% to 100% of it,
//     so it lives up to a fifth of LifeTime), its fade is set to -1: drawn
//     as its absolute value, it jumps to full and fades out over 4 s, still
//     flying, and the ship starts again somewhere else.
//   - It is drawn in the far scene, after the dome, depth tested against
//     the far terrain, without fog; lit by the map's ambient and sun, never
//     by lamps; blended while it fades, still writing depth.
//
// That is Classic: the battle's own classes (those its script loads), cut
// off by the far scene's far plane. Graphics > Ocean and sky's Enhanced
// (the default) keeps the flight and adds:
//
//   - every class of the battle's era (flyers.json `enhanced`), so Bespin:
//     Platforms has capital ships and Cloud City's Galactic Civil War battle
//     Star Destroyers;
//   - ships past the far plane drawn anyway, scaled toward the eye (their
//     image unchanged) with their depth just inside the far plane, so the
//     far scene's terrain still hides them; far off, low over the horizon,
//     they take on the sky behind them (sky-probe.js);
//   - smooth fades: a ship starts its fade-in 4 s back along its course, so
//     it is solid where the original starts it, and fades out from where it
//     is, without the jump;
//   - fading ships write no depth (drawn after the solid ones), so they never
//     hide a ship behind them.
//
// In both, ships draw their own motion for the temporal upscaler
// (motion-vectors.js), so a crossing fighter doesn't smear.
//
// Presentation only, page-side: no simulation reads it, it isn't in the
// content fingerprint, and each page draws its own random ships. A class is
// one instanced draw per model part, sorted far to near (Enhanced: another
// for its fading ships).

// The setting whose Enhanced choice turns the Enhanced sky on.
export const SKY_SETTING = "oceanEffects";

const vertexShader = /* glsl */ `
  uniform vec3 sunDirection;
  uniform vec3 sunColor;
  uniform vec3 ambient;
  uniform float reach;
  // fade, haze, past the far plane (1) or not, the scale it is drawn at
  attribute vec4 look;
  varying vec2 vUv;
  varying vec3 vLight;
  varying float vFade;
  varying float vHaze;
  varying vec3 vDirection;
  void main() {
    mat4 placed = modelMatrix;
    #ifdef USE_INSTANCING
      placed = placed * instanceMatrix;
    #endif
    #ifdef LIT
      // A prop's light: the ambient and the sun, per vertex.
      vec3 n = normalize(mat3(placed) * normal);
      vLight = clamp(ambient + max(dot(n, -sunDirection), 0.0) * sunColor, 0.0, 1.0);
    #else
      vLight = vec3(1.0);
    #endif
    vUv = uv;
    vFade = look.x;
    vHaze = look.y;
    vec4 world = placed * vec4(position, 1.0);
    vDirection = world.xyz - cameraPosition;
    gl_Position = projectionMatrix * viewMatrix * world;
    if (look.z > 0.5) {
      // Drawn scaled toward the eye (Enhanced): its depth goes just inside
      // the far plane, in the order of its true distance, behind everything
      // else the far scene draws.
      float far = length(vDirection) / max(look.w, 1e-6);
      float near = clamp(reach / far, 0.0, 1.0);
      gl_Position.z = gl_Position.w * (0.999999 - 9e-6 * near);
    }
  }
`;

const fragmentShader = /* glsl */ `
  uniform sampler2D map;
  uniform samplerCube skyProbe;
  varying vec2 vUv;
  varying vec3 vLight;
  varying float vFade;
  varying float vHaze;
  varying vec3 vDirection;
  void main() {
    vec4 texel = texture2D(map, vUv);
    #ifdef ALPHA_TEST
      if (texel.a * 255.0 <= 127.0) discard;
    #endif
    float alpha = vFade;
    #ifdef BLENDED
      alpha *= texel.a;
    #endif
    if (alpha <= 0.0) discard;
    vec3 color = texel.rgb * vLight;
    // Enhanced, far off and low: the sky behind it, softened.
    if (vHaze > 0.0) color = mix(color, textureCube(skyProbe, normalize(vDirection), 2.0).rgb, vHaze);
    #ifdef ADDITIVE
      gl_FragColor = vec4(color * alpha, 1.0);
    #else
      gl_FragColor = vec4(color, alpha);
    #endif
  }
`;

// A part's material by its MTRL flags: lit (0x1) or not, cut out (0x2),
// laid over by its alpha (0x4) or added (0x80), two-sided (0x86). Every
// part blends by the ship's fade, as an object fading out does, from the
// far scene's opaque queue, after the terrain (renderOrder). Its
// opaque parts write depth even then, as the 2004 game's do; blended
// parts (0x84) never do. `fading`: Enhanced's draw of the fading ships,
// which writes no depth at all.
export function flyerMaterial(part, map, { fading = false } = {}) {
  const flags = part.flags ?? 1;
  const defines = {};
  if (flags & 0x1) defines.LIT = "";
  if (flags & 0x2) defines.ALPHA_TEST = "";
  if (flags & 0x80) defines.ADDITIVE = "";
  else if (flags & 0x4) defines.BLENDED = "";
  const material = new THREE.ShaderMaterial({
    name: fading ? "sky-flyer-fading" : "sky-flyer",
    defines,
    uniforms: {
      map: { value: map || null },
      sunDirection: lighting.sunDirection,
      sunColor: lighting.sunColor,
      ambient: lighting.ambient,
      skyProbe: { value: null },
      reach: { value: 1 },
    },
    vertexShader,
    fragmentShader,
    side: flags & 0x86 ? THREE.DoubleSide : THREE.FrontSide,
  });
  material.transparent = false;
  material.blending = THREE.CustomBlending;
  material.blendSrc = flags & 0x80 ? THREE.OneFactor : THREE.SrcAlphaFactor;
  material.blendDst = flags & 0x80 ? THREE.OneFactor : THREE.OneMinusSrcAlphaFactor;
  material.depthWrite = !fading && !(flags & 0x84);
  return material;
}

// The motion a ship's pixels moved since the last frame, for the temporal
// upscaler (motion-vectors.js). It covers the pixels the far scene shows
// (where the near scene drew nothing), from its true placement now and
// then; past the near scene's far plane it is kept by taking no depth.
const motionVertexShader = /* glsl */ `
  uniform mat4 currentViewProjection;
  uniform mat4 previousViewProjection;
  uniform float still;
  attribute mat4 previousMatrix;
  attribute float shown;
  varying vec4 vCurrent;
  varying vec4 vPrevious;
  varying float vShown;
  void main() {
    vec4 world = modelMatrix * instanceMatrix * vec4(position, 1.0);
    vCurrent = currentViewProjection * world;
    // A frame without flight (paused): where it was is where it is.
    vec4 before = still > 0.5 ? world : modelMatrix * previousMatrix * vec4(position, 1.0);
    vPrevious = previousViewProjection * before;
    vShown = shown;
    gl_Position = projectionMatrix * viewMatrix * world;
    gl_Position.z = 0.0;
  }
`;

const motionFragmentShader = /* glsl */ `
  uniform highp sampler2D sceneDepth;
  varying vec4 vCurrent;
  varying vec4 vPrevious;
  varying float vShown;
  void main() {
    if (vShown < 0.5) discard;
    if (texelFetch(sceneDepth, ivec2(gl_FragCoord.xy), 0).r < 1.0) discard;
    vec2 now = vCurrent.xy / vCurrent.w, then = vPrevious.xy / vPrevious.w;
    gl_FragColor = vec4((now - then) * 0.5, 0.0, 0.5);
  }
`;

let motionMaterial = null;
function flyerMotion() {
  motionMaterial ??= new THREE.ShaderMaterial({
    name: "sky-flyer-motion",
    vertexShader: motionVertexShader,
    fragmentShader: motionFragmentShader,
    uniforms: {
      currentViewProjection: motionUniforms.currentViewProjection,
      previousViewProjection: motionUniforms.previousViewProjection,
      sceneDepth: motionUniforms.sceneDepth,
      still: { value: 0 },
    },
    depthTest: false, depthWrite: false, side: THREE.DoubleSide,
  });
  return motionMaterial;
}

// The flyers a battle draws: its mission's classes from flyers.json, as the
// original draws them or (`enhanced`) as the Enhanced sky does. A map
// prepared before the Enhanced lists has only the original's.
export function battleFlyers(data, mission, enhanced = false) {
  const table = enhanced && data?.enhanced ? data.enhanced : data?.missions;
  const indices = table?.[mission] || [];
  return indices.map((i) => data.flyers[i]).filter((def) => def && def.count > 0);
}

// Whether the Enhanced sky is on.
export const enhancedSky = (settings) => settings?.get?.(SKY_SETTING) === "enhanced";

// The flight (launch, fly, fade, placement, the far plane's reach, the haze),
// the order of the draws and their instance records are a Rust phase's
// (rust/bfe-environment, sky.rs). The page keeps the meshes, materials and
// the sky probe the haze reads.
export class SkyFlyers {
  // data: flyers.json; buffer, indexBuffer: flyers.bin and flyers_index.bin;
  // texture(name): the sky folder's textures; scene: the far scene;
  // graphics: its far camera (Enhanced's reach), renderer (the sky probe)
  // and settings; seed: the ships' random draws (the engine's generator,
  // src/atmosphere.js); motion: the temporal upscaler's motion pass.
  constructor({ data, mission, buffer, indexBuffer, texture = () => null, scene = null, seed = 1,
    graphics = null, settings = graphics?.settings ?? null, motion = motionVectors }) {
    this.graphics = graphics;
    this.settings = settings;
    this.motion = motion;
    this.group = new THREE.Group();
    this.group.name = "sky-flyers";
    this.group.matrixAutoUpdate = false;
    this.classes = [];
    this.time = 0;
    this.stepped = false;
    this.probe = null;
    this.enhanced = enhancedSky(settings);
    const classic = new Set(battleFlyers(data, mission, false));
    const enhanced = new Set(battleFlyers(data, mission, true));
    const models = modelPack(data?.models, buffer, indexBuffer);
    const kept = [];
    for (const def of data?.flyers || []) {
      if (!classic.has(def) && !enhanced.has(def)) continue;
      const parts = models.get(def.model);
      if (!parts?.length) continue;
      kept.push({ def, parts, modes: { classic: classic.has(def), enhanced: enhanced.has(def) } });
    }
    // Rust keeps the ships; the page's draws are built over its records.
    this.host = new SkyHost(loaded(), kept.length);
    const config = this.host.config;
    config[SKY_CONFIG.SEED] = seed;
    config[SKY_CONFIG.CLASSES] = kept.length;
    config[SKY_CONFIG.ENHANCED] = this.enhanced ? 1 : 0;
    const radii = kept.map(({ parts }) => parts.reduce((r, part) => {
      const sphere = part.geometry.boundingSphere;
      return Math.max(r, sphere.center.length() + sphere.radius);
    }, 0));
    kept.forEach(({ def, modes }, i) => {
      const at = SKY_CONFIG.STRIDE + i * SKY_CLASS.STRIDE;
      config[at + SKY_CLASS.COUNT] = def.count;
      config[at + SKY_CLASS.DISTANCE] = def.distance;
      config.set(def.height, at + SKY_CLASS.HEIGHT);
      config[at + SKY_CLASS.IN_DIRECTION] = def.inDirection;
      def.velocity.forEach((range, k) => config.set(range, at + SKY_CLASS.VELOCITY + k * 2));
      config.set(def.acceleration, at + SKY_CLASS.ACCELERATION);
      config[at + SKY_CLASS.LIFE_TIME] = def.lifeTime;
      config[at + SKY_CLASS.RADIUS] = radii[i];
      config[at + SKY_CLASS.CLASSIC] = modes.classic ? 1 : 0;
      config[at + SKY_CLASS.ENHANCED] = modes.enhanced ? 1 : 0;
    });
    this.host.start();
    kept.forEach(({ def, parts, modes }, i) => this.classes.push(this.build(def, parts, texture, modes, radii[i], i)));
    this.motionChecked = -1;
    this.unsubscribe = settings?.onChange?.((key) => { if (key === SKY_SETTING) this.setEnhanced(enhancedSky(settings)); }) || null;
    this.place(null);
    scene?.add(this.group);
  }

  // A class's draws: per model part one instanced mesh for its ships (all of
  // them in Classic, the solid ones in Enhanced), one for Enhanced's fading
  // ships, and its motion. Their instance records are views onto the phase's.
  build(def, parts, texture, modes, radius, index) {
    const count = def.count;
    const records = this.host.arrays(index);
    const solid = { matrices: new THREE.InstancedBufferAttribute(records[0], 16), looks: new THREE.InstancedBufferAttribute(records[1], 4) };
    const fading = { matrices: new THREE.InstancedBufferAttribute(records[2], 16), looks: new THREE.InstancedBufferAttribute(records[3], 4) };
    const motion = {
      matrices: new THREE.InstancedBufferAttribute(records[4], 16),
      previous: new THREE.InstancedBufferAttribute(records[5], 16),
      shown: new THREE.InstancedBufferAttribute(records[6], 1),
    };
    for (const attribute of Object.values({ ...solid, ...fading }).concat(Object.values(motion))) attribute.setUsage(THREE.DynamicDrawUsage);
    // The part's buffers, with these instance attributes beside them.
    const geometryOf = (part, extra) => {
      const geometry = new THREE.BufferGeometry();
      for (const [name, attribute] of Object.entries(part.geometry.attributes)) geometry.setAttribute(name, attribute);
      geometry.setIndex(part.geometry.index);
      geometry.boundingSphere = part.geometry.boundingSphere;
      for (const [name, attribute] of Object.entries(extra)) geometry.setAttribute(name, attribute);
      return geometry;
    };
    const mesh = (geometry, material, matrices, order) => {
      const m = new THREE.InstancedMesh(geometry, material, count);
      m.instanceMatrix = matrices;
      m.name = def.model;
      m.frustumCulled = false;
      m.matrixAutoUpdate = false;
      m.renderOrder = order;
      return m;
    };
    const draws = [], fades = [], motions = [];
    for (const part of parts) {
      const map = texture(part.texture);
      // After the far scene's terrain and models; the fading ships after
      // every solid one.
      draws.push(mesh(geometryOf(part, { look: solid.looks }), flyerMaterial(part, map), solid.matrices, 1));
      fades.push(mesh(geometryOf(part, { look: fading.looks }), flyerMaterial(part, map, { fading: true }), fading.matrices, 2));
      const moving = mesh(geometryOf(part, { previousMatrix: motion.previous, shown: motion.shown }), flyerMotion(), motion.matrices, 0);
      moving.onBeforeRender = () => this.motionFrame();
      motions.push(moving);
    }
    for (const m of [...draws, ...fades]) this.group.add(m);
    for (const m of motions) this.motion?.addExtra?.(m);
    const attributes = [solid.matrices, solid.looks, fading.matrices, fading.looks, motion.matrices, motion.previous, motion.shown];
    return { def, ...modes, index, radius, solid, fading, motion, draws, fades, motions, meshes: [...draws, ...fades, ...motions], attributes };
  }

  // Whether a class flies in the current mode.
  active(kind) {
    return this.enhanced ? kind.enhanced : kind.classic;
  }

  // Classic or Enhanced, as the setting changes: classes coming in start
  // afresh; ships flying on keep their course under the new rules.
  setEnhanced(enhanced) {
    if (enhanced === this.enhanced) return;
    this.enhanced = enhanced;
    this.host.x.sky_set_enhanced(enhanced ? 1 : 0);
    this.place(null);
  }

  get count() {
    return this.classes.reduce((sum, kind) => sum + (this.active(kind) ? kind.def.count : 0), 0);
  }

  // A frame of flight, then the draws: far to near from the eye.
  update(dt, camera = null) {
    if (dt > 0) {
      this.time += dt;
      this.stepped = true;
    }
    if (this.enhanced && camera) this.sky(camera);
    this.place(camera?.position || null, dt);
  }

  // Enhanced: the sky behind the ships (sky-probe.js), for their haze.
  sky(camera) {
    const renderer = this.graphics?.renderer;
    if (!renderer || !this.classes.length) return;
    if (!this.probe) {
      this.probe = new SkyProbe(this.graphics, 64);
      this.probe.enabled = true;
      for (const kind of this.classes) for (const m of [...kind.draws, ...kind.fades]) m.material.uniforms.skyProbe.value = this.probe.texture;
    }
    this.probe.update(this.time, camera);
  }

  // The phase's frame (flight over dt, then the layout of the draws from
  // the eye, none without one), and the meshes set from what it left.
  place(eye, dt = 0) {
    const frame = this.host.frame;
    frame[SKY_FRAME.DT] = dt;
    frame[SKY_FRAME.HAS_EYE] = eye ? 1 : 0;
    if (eye) { frame[SKY_FRAME.EYE] = eye.x; frame[SKY_FRAME.EYE + 1] = eye.y; frame[SKY_FRAME.EYE + 2] = eye.z; }
    const far = this.graphics?.farCamera?.far || 0;
    frame[SKY_FRAME.FAR] = far;
    frame[SKY_FRAME.VISIBLE] = this.group.visible ? 1 : 0;
    frame[SKY_FRAME.MISTY] = this.probe ? 1 : 0;
    this.host.x.sky_update();
    const r = this.host.result;
    for (const kind of this.classes) {
      const at = kind.index * SKY_RESULT.STRIDE;
      if (!r[at + SKY_RESULT.ON]) {
        for (const m of kind.meshes) m.visible = false;
        continue;
      }
      const solids = r[at + SKY_RESULT.SOLIDS], fadings = r[at + SKY_RESULT.FADINGS];
      for (const m of kind.draws) {
        m.count = solids;
        m.visible = solids > 0;
        m.material.uniforms.reach.value = far;
      }
      for (const m of kind.fades) {
        m.count = fadings;
        m.visible = fadings > 0;
        m.material.uniforms.reach.value = far;
      }
      for (const m of kind.motions) m.visible = true;
      for (const attribute of kind.attributes) attribute.needsUpdate = true;
    }
  }

  // Before the motion pass draws a class (once a frame, motion-vectors.js):
  // a frame with no flight since the last one (paused) shows no motion.
  motionFrame() {
    const frame = this.motion?.frame ?? 0;
    if (frame === this.motionChecked) return;
    this.motionChecked = frame;
    flyerMotion().uniforms.still.value = this.stepped ? 0 : 1;
    this.stepped = false;
  }

  dispose() {
    this.unsubscribe?.();
    this.group.removeFromParent();
    for (const kind of this.classes) {
      for (const m of [...kind.draws, ...kind.fades]) {
        m.geometry.dispose();
        m.material.dispose();
        m.dispose();
      }
      for (const m of kind.motions) {
        this.motion?.removeExtra?.(m);
        m.geometry.dispose();
        m.dispose();
      }
    }
    this.probe?.dispose();
    this.probe = null;
    this.classes = [];
  }
}
