import * as THREE from "three";
import { EngineRandom } from "./atmosphere.js";
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

export const FADE_RATE = 0.25; // of full alpha a second: 4 s in, 4 s out
export const KILL_MARGIN = 10; // m past Distance, across the ground from the origin
// The setting whose Enhanced choice turns the Enhanced sky on.
export const SKY_SETTING = "oceanEffects";
// Enhanced: how far back along its course a ship starts, its whole fade-in.
export const PRE_ROLL = 1 / FADE_RATE;
// Enhanced: the share of the far plane a farther ship is drawn inside.
export const REACH = 0.98;
// Enhanced: how much of the sky behind it a far, low ship takes on at most.
export const HAZE = 0.7;

const span = ([low, high], random) => low + random.float() * (high - low);
const smooth = (a, b, x) => { const t = Math.min(Math.max((x - a) / (b - a), 0), 1); return t * t * (3 - 2 * t); };

// A ship starts again: the draws in the 2004 game's order.
export function launch(def, random, ship = {}) {
  ship.fade = 0;
  let dx = 0, dz = 0;
  while (dx === 0 && dz === 0) {
    dz = random.float() - 0.5;
    dx = random.float() - 0.5;
  }
  const length = Math.hypot(dx, dz);
  dx /= length;
  dz /= length;
  const height = span(def.height, random);
  ship.position ??= new THREE.Vector3();
  ship.position.set(dx * def.distance, height, dz * def.distance);
  // Across the direction it starts in (up x direction), either way, then
  // turned inward.
  let tx = dz, tz = -dx;
  if (random.float() > 0.5) { tx = -tx; tz = -tz; }
  if (def.inDirection > 0) {
    const inward = random.float() * def.inDirection * 0.5 + def.inDirection * 0.5;
    tx -= dx * inward;
    tz -= dz * inward;
  }
  const heading = Math.hypot(tx, tz);
  ship.forward ??= new THREE.Vector3();
  ship.forward.set(tx / heading, 0, tz / heading);
  // Its velocity in its own axes, drawn forward first.
  const vz = span(def.velocity[2], random);
  const vy = span(def.velocity[1], random);
  const vx = span(def.velocity[0], random);
  ship.velocity = [vx, vy, vz];
  ship.age = def.lifeTime * (random.float() * 0.2 + 0.8);
  ship.entered = true;
  ship.launched = true;
  return ship;
}

// Enhanced: the same start, the same draws, then PRE_ROLL seconds back
// along its course (its acceleration undone too): it fades in on the way
// and is solid where the original starts it. Its age counts from there,
// and it may leave its ring only once it has been inside.
export function launchEnhanced(def, random, ship = {}) {
  launch(def, random, ship);
  const t = PRE_ROLL, v = ship.velocity, a = def.acceleration, f = ship.forward;
  const back = [0, 1, 2].map((k) => v[k] * t - 0.5 * a[k] * t * t);
  ship.position.x -= f.z * back[0] + f.x * back[2];
  ship.position.y -= back[1];
  ship.position.z -= -f.x * back[0] + f.z * back[2];
  for (let k = 0; k < 3; k++) v[k] -= a[k] * t;
  ship.age -= t;
  ship.entered = false;
  return ship;
}

// Its flight over dt in its own axes: Acceleration first, then the move.
function move(def, ship, dt) {
  const v = ship.velocity, a = def.acceleration;
  v[0] += a[0] * dt;
  v[1] += a[1] * dt;
  v[2] += a[2] * dt;
  // Right is up x forward, (fz, 0, -fx); up is the world's.
  const f = ship.forward;
  ship.position.x += (f.z * v[0] + f.x * v[2]) * dt;
  ship.position.y += v[1] * dt;
  ship.position.z += (-f.x * v[0] + f.z * v[2]) * dt;
}

// One frame of a ship. False when it has just faded out: it starts again
// instead of moving.
export function fly(def, ship, dt) {
  if (ship.fade >= 0) {
    const reach = def.distance + KILL_MARGIN;
    const away = ship.position.x * ship.position.x + ship.position.z * ship.position.z;
    if (away > reach * reach || ship.age > def.lifeTime) ship.fade = -1;
  }
  ship.age += dt;
  const before = ship.fade;
  ship.fade = Math.min(before + dt * FADE_RATE, 1);
  if (ship.fade >= 0 && before < 0) return false;
  move(def, ship, dt);
  return true;
}

// Enhanced: the same frame, but a ship leaves its ring only once it has
// been inside it, and fades out from the alpha it has.
export function flyEnhanced(def, ship, dt) {
  const reach = def.distance + KILL_MARGIN;
  const away = ship.position.x * ship.position.x + ship.position.z * ship.position.z;
  if (away <= reach * reach) ship.entered = true;
  if (ship.fade >= 0 && ((ship.entered && away > reach * reach) || ship.age > def.lifeTime)) {
    ship.fade = -Math.max(ship.fade, 1e-6);
  }
  ship.age += dt;
  const before = ship.fade;
  ship.fade = Math.min(before + dt * FADE_RATE, 1);
  if (ship.fade >= 0 && before < 0) return false;
  move(def, ship, dt);
  return true;
}

// Its placement: the model's x along right, y up, z along its heading.
export function placement(ship, out = new THREE.Matrix4()) {
  const f = ship.forward;
  return out.set(
    f.z, 0, f.x, ship.position.x,
    0, 1, 0, ship.position.y,
    -f.x, 0, f.z, ship.position.z,
    0, 0, 0, 1,
  );
}

// Enhanced: where a ship is drawn so that the far plane `far` doesn't cut
// it. A ship (of bounding radius `radius` about its origin) reaching past
// REACH x far is scaled about the eye by the share that brings its far side
// there: its image is the same, it lies inside the far plane, and its
// depth is put just inside it (the shader). Returns that scale (1: drawn
// where it is) and writes the placement drawn into `out`.
export function withinReach(matrix, eye, radius, far, out) {
  const x = matrix.elements[12] - eye.x, y = matrix.elements[13] - eye.y, z = matrix.elements[14] - eye.z;
  const distance = Math.hypot(x, y, z);
  const reach = far * REACH;
  out.copy(matrix);
  if (!(distance + radius > reach) || !(reach > 0)) return 1;
  const scale = reach / (distance + radius);
  const e = out.elements;
  for (let i = 0; i < 12; i++) if (i % 4 !== 3) e[i] *= scale;
  e[12] = eye.x + x * scale;
  e[13] = eye.y + y * scale;
  e[14] = eye.z + z * scale;
  return scale;
}

// Enhanced: how much of the sky behind it a ship at `distance`, `rise`
// metres above the eye, takes on: none overhead or near, up to HAZE low on
// the horizon three times the far plane away.
export function haze(distance, rise, far) {
  if (!(distance > 0) || !(far > 0)) return 0;
  return HAZE * (1 - smooth(0.04, 0.35, rise / distance)) * smooth(0.6, 3, distance / far);
}

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

const drawn = new THREE.Matrix4();
const order = [];
const SOLID = 0.999;

export class SkyFlyers {
  // data: flyers.json; buffer, indexBuffer: flyers.bin and flyers_index.bin;
  // texture(name): the sky folder's textures; scene: the far scene;
  // graphics: its far camera (Enhanced's reach), renderer (the sky probe)
  // and settings; seed: the ships' random draws (the engine's generator,
  // src/atmosphere.js); motion: the temporal upscaler's motion pass.
  constructor({ data, mission, buffer, indexBuffer, texture = () => null, scene = null, seed = 1,
    graphics = null, settings = graphics?.settings ?? null, motion = motionVectors }) {
    this.random = new EngineRandom(seed);
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
    for (const def of data?.flyers || []) {
      if (!classic.has(def) && !enhanced.has(def)) continue;
      const parts = models.get(def.model);
      if (!parts?.length) continue;
      this.classes.push(this.build(def, parts, texture, { classic: classic.has(def), enhanced: enhanced.has(def) }));
    }
    // Only the classes flying now: one coming in later (setEnhanced) starts
    // then, so Classic's random draws are the original's alone.
    for (const kind of this.classes) if (this.active(kind)) this.relaunch(kind);
    this.motionChecked = -1;
    this.unsubscribe = settings?.onChange?.((key) => { if (key === SKY_SETTING) this.setEnhanced(enhancedSky(settings)); }) || null;
    this.place(null);
    scene?.add(this.group);
  }

  // A class's draws: per model part one instanced mesh for its ships (all of
  // them in Classic, the solid ones in Enhanced), one for Enhanced's fading
  // ships, and its motion.
  build(def, parts, texture, modes) {
    const count = def.count;
    const solid = { matrices: new THREE.InstancedBufferAttribute(new Float32Array(count * 16), 16), looks: new THREE.InstancedBufferAttribute(new Float32Array(count * 4), 4) };
    const fading = { matrices: new THREE.InstancedBufferAttribute(new Float32Array(count * 16), 16), looks: new THREE.InstancedBufferAttribute(new Float32Array(count * 4), 4) };
    const motion = {
      matrices: new THREE.InstancedBufferAttribute(new Float32Array(count * 16), 16),
      previous: new THREE.InstancedBufferAttribute(new Float32Array(count * 16), 16),
      shown: new THREE.InstancedBufferAttribute(new Float32Array(count), 1),
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
    let radius = 0;
    const draws = [], fades = [], motions = [];
    for (const part of parts) {
      const sphere = part.geometry.boundingSphere;
      radius = Math.max(radius, sphere.center.length() + sphere.radius);
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
    const ships = Array.from({ length: def.count }, () => ({ matrix: new THREE.Matrix4(), previous: new THREE.Matrix4() }));
    const attributes = [solid.matrices, solid.looks, fading.matrices, fading.looks, motion.matrices, motion.previous, motion.shown];
    return { def, ...modes, radius, ships, solid, fading, motion, draws, fades, motions, meshes: [...draws, ...fades, ...motions], attributes };
  }

  // Whether a class flies in the current mode.
  active(kind) {
    return this.enhanced ? kind.enhanced : kind.classic;
  }

  relaunch(kind) {
    for (const ship of kind.ships) this.launch(kind, ship);
  }

  launch(kind, ship) {
    if (this.enhanced) launchEnhanced(kind.def, this.random, ship);
    else launch(kind.def, this.random, ship);
    placement(ship, ship.matrix);
    ship.previous.copy(ship.matrix);
  }

  // Classic or Enhanced, as the setting changes: classes coming in start
  // afresh; ships flying on keep their course under the new rules.
  setEnhanced(enhanced) {
    if (enhanced === this.enhanced) return;
    const was = this.classes.map((kind) => this.active(kind));
    this.enhanced = enhanced;
    this.classes.forEach((kind, i) => {
      if (!this.active(kind)) return;
      if (!was[i]) this.relaunch(kind);
      else if (enhanced) {
        for (const ship of kind.ships) {
          const reach = kind.def.distance + KILL_MARGIN;
          ship.entered = ship.position.x ** 2 + ship.position.z ** 2 <= reach * reach;
        }
      }
    });
    this.place(null);
  }

  get count() {
    return this.classes.reduce((sum, kind) => sum + (this.active(kind) ? kind.ships.length : 0), 0);
  }

  // A frame of flight, then the draws: far to near from the eye.
  update(dt, camera = null) {
    if (dt > 0) {
      this.time += dt;
      for (const kind of this.classes) {
        if (!this.active(kind)) continue;
        for (const ship of kind.ships) {
          ship.previous.copy(ship.matrix);
          const flying = this.enhanced ? flyEnhanced(kind.def, ship, dt) : fly(kind.def, ship, dt);
          if (flying) placement(ship, ship.matrix);
          else this.launch(kind, ship);
        }
      }
      this.stepped = true;
    }
    if (this.enhanced && camera) this.sky(camera);
    this.place(camera?.position || null);
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

  place(eye) {
    const far = this.graphics?.farCamera?.far || 0;
    const enhanced = this.enhanced && eye && far > 0;
    // The haze needs the sky behind (sky()); without it, none.
    const misty = enhanced && this.probe;
    for (const kind of this.classes) {
      const { ships, solid, fading, motion } = kind;
      const on = this.active(kind) && this.group.visible;
      if (!on) {
        for (const m of kind.meshes) m.visible = false;
        continue;
      }
      order.length = 0;
      for (let i = 0; i < ships.length; i++) {
        const ship = ships[i];
        ship.distance = eye ? ship.position.distanceToSquared(eye) : 0;
        order.push(ship);
        // Its motion, at its true placement now and then.
        ship.matrix.toArray(motion.matrices.array, i * 16);
        ship.previous.toArray(motion.previous.array, i * 16);
        motion.shown.array[i] = Math.abs(ship.fade) >= 0.5 ? 1 : 0;
      }
      if (eye) order.sort((a, b) => b.distance - a.distance);
      let solids = 0, fadings = 0;
      for (const ship of order) {
        const alpha = Math.abs(ship.fade);
        let scale = 1, mist = 0;
        drawn.copy(ship.matrix);
        if (enhanced) {
          scale = withinReach(ship.matrix, eye, kind.radius, far, drawn);
          if (misty) mist = haze(Math.sqrt(ship.distance), ship.position.y - eye.y, far);
        }
        const into = this.enhanced && alpha < SOLID ? fading : solid;
        const at = into === solid ? solids++ : fadings++;
        drawn.toArray(into.matrices.array, at * 16);
        const look = into.looks.array;
        look[at * 4] = alpha;
        look[at * 4 + 1] = mist;
        look[at * 4 + 2] = scale < 1 ? 1 : 0;
        look[at * 4 + 3] = scale;
      }
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
