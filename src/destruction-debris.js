import * as THREE from "three";
import { localLighting } from "./local-lights.js";
import { motionVectors } from "./motion-vectors.js";
import { selectLights } from "./materials.js";
import { Dst, DstPiece as PIECE } from "./environment/layout.js";

// The pieces of a droid blown apart, for the Enhanced destruction effects
// (src/destruction.js). Classic pieces stay with src/debris.js.
//
// Each piece is a rigid body: its centre of mass (its model's box centre),
// its orientation, and its linear and angular velocity, with the inertia of
// a box its size. The bodies and their lives are the Rust destruction
// phase's (rust/bfe-environment, solver.rs and destruction.rs): it finds the
// ground under each piece, bounces, rolls and settles it, lets pieces push
// each other apart, tips one that comes to rest on end, and lets one go when
// it has lain a while (crumbling away, noise-dithered, its shadow going with
// it). They are only for show: they don't collide with soldiers or rounds,
// and nothing in the simulation reads them.
//
// What stays here is what needs the page's models: a piece's shape worked
// out from its model's vertices, the models themselves (made to glow: hot
// where they tore away and in ragged patches around it, cooling from white
// through orange to dull red, and charred; lit by the map like a soldier and
// casting shadows) and the materials that do it.

// Directions to the extreme points: the faces, edges and corners of a cube.
const DIRECTIONS = [];
for (let x = -1; x <= 1; x++) for (let y = -1; y <= 1; y++) for (let z = -1; z <= 1; z++) {
  if (x || y || z) DIRECTIONS.push(new THREE.Vector3(x, y, z).normalize());
}

// A piece's shape from its model (meshes in the object's own frame): the
// centre of its box, the extreme vertices (from that centre), the box's half
// size, its reach, and the inverse inertia of a 1 kg box that size.
export function pieceShape(object) {
  const vertices = [];
  const v = new THREE.Vector3();
  object.updateMatrixWorld(true);
  const toObject = new THREE.Matrix4().copy(object.matrixWorld).invert();
  const local = new THREE.Matrix4();
  object.traverse((mesh) => {
    const position = mesh.isMesh ? mesh.geometry.attributes.position : null;
    if (!position) return;
    local.multiplyMatrices(toObject, mesh.matrixWorld);
    for (let i = 0; i < position.count; i++) vertices.push(v.fromBufferAttribute(position, i).applyMatrix4(local).clone());
  });
  const box = new THREE.Box3().setFromPoints(vertices);
  if (box.isEmpty()) box.set(new THREE.Vector3(-0.1, -0.1, -0.1), new THREE.Vector3(0.1, 0.1, 0.1));
  const center = box.getCenter(new THREE.Vector3());
  const half = box.getSize(new THREE.Vector3()).multiplyScalar(0.5).max(new THREE.Vector3(0.02, 0.02, 0.02));
  const points = [];
  for (const direction of DIRECTIONS) {
    let best = null, most = -Infinity;
    for (const p of vertices) {
      const d = (p.x - center.x) * direction.x + (p.y - center.y) * direction.y + (p.z - center.z) * direction.z;
      if (d > most) { most = d; best = p; }
    }
    if (!best) continue;
    const point = best.clone().sub(center);
    if (!points.some((q) => q.distanceToSquared(point) < 0.0004)) points.push(point);
  }
  if (!points.length) for (const direction of DIRECTIONS) points.push(direction.clone().multiply(half));
  const radius = Math.max(...points.map((p) => p.length()), 0.05);
  const [x2, y2, z2] = [half.x * half.x, half.y * half.y, half.z * half.z];
  const inertia = new THREE.Vector3(Math.max((y2 + z2) / 3, 0.002), Math.max((x2 + z2) / 3, 0.002), Math.max((x2 + y2) / 3, 0.002));
  return { center, half, points, radius, inverseInertia: new THREE.Vector3(1 / inertia.x, 1 / inertia.y, 1 / inertia.z) };
}

// A copy of one of the game's materials (materials.js zeroMaterial) that
// glows where hot and chars: the same lighting, shadows and fog, plus heat.
// Per-piece values ride on the drawn object (object.userData.heated).
const HEAT_PARS = /* glsl */ `
  #ifdef HEATED
    uniform float heat;
    uniform vec4 heatTear; // where it tore away (its own frame), and how far the heat spreads
    uniform float heatBase; // heat everywhere, besides the tear's
    uniform float heatScale; // the hot patches' size: 1 for pieces, less for bigger patches
    uniform float soot;
    uniform float dissolve;
    varying vec3 vHeatLocal;
    float hHash(vec3 p) {
      p = fract(p * 0.3183099 + 0.1);
      p *= 17.0;
      return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
    }
    float hNoise(vec3 x) {
      vec3 i = floor(x), f = fract(x);
      f = f * f * (3.0 - 2.0 * f);
      return mix(mix(mix(hHash(i), hHash(i + vec3(1, 0, 0)), f.x), mix(hHash(i + vec3(0, 1, 0)), hHash(i + vec3(1, 1, 0)), f.x), f.y),
                 mix(mix(hHash(i + vec3(0, 0, 1)), hHash(i + vec3(1, 0, 1)), f.x), mix(hHash(i + vec3(0, 1, 1)), hHash(i + vec3(1, 1, 1)), f.x), f.y), f.z);
    }
    vec3 heated(vec3 color) {
      float n = hNoise(vHeatLocal * 13.0 * heatScale) * 0.62 + hNoise(vHeatLocal * 34.0 * heatScale + 5.0) * 0.38;
      if (n < dissolve) discard;
      float tear = max(exp(-distance(vHeatLocal, heatTear.xyz) / max(heatTear.w, 0.01)), heatBase);
      float scorch = clamp(tear * 1.25 + (n - 0.45) * 0.9, 0.0, 1.0);
      color *= 1.0 - soot * (0.18 + 0.55 * scorch);
      // Ragged hot patches, densest where it tore; white at the hottest.
      float glow = smoothstep(0.52, 0.92, tear * 0.8 + n * 0.55) * heat;
      vec3 hot = mix(vec3(0.7, 0.07, 0.01), vec3(1.9, 0.7, 0.16), smoothstep(0.2, 0.7, heat));
      hot = mix(hot, vec3(2.4, 1.85, 1.15), smoothstep(0.75, 1.0, heat) * smoothstep(0.55, 0.9, glow));
      return color + hot * glow;
    }
  #endif
`;

// The stock fragment shader with heat: declared after its common chunk, and
// applied to the lit colour before the near scene dissolves into the far
// one, so a hot or sooted piece fades out with everything else at range.
// Without that block (a shader of another shape) it goes last instead.
const FAR_FADE = /#ifdef FAR(\s*)\/\/ Linear fog by depth/;
const APPLY_HEAT = "#ifdef HEATED\n        color.rgb = heated(color.rgb);\n      #endif\n      ";
function heatedFragment(source) {
  const shader = source.replace("#include <common>", `#include <common>\n${HEAT_PARS}`);
  if (FAR_FADE.test(shader)) return shader.replace(FAR_FADE, (fade) => APPLY_HEAT + fade);
  return shader.replace("gl_FragColor = color;", `${APPLY_HEAT}gl_FragColor = color;`);
}

const heatMaterials = new WeakMap();
const NO_HEAT = { heat: 0, tear: new THREE.Vector4(0, 0, 0, 0.3), soot: 0, dissolve: 0, sun: 1, brightness: 1, darken: 0, lighten: 0 };

export function heatMaterial(source) {
  let material = heatMaterials.get(source);
  if (material) return material;
  if (!source?.isShaderMaterial || !source.fragmentShader.includes("gl_FragColor = color;")) return source;
  const uniforms = {
    ...source.uniforms,
    sunScale: { value: 1 }, brightness: { value: 1 }, ambientPull: { value: new THREE.Vector2() },
    heat: { value: 0 }, heatTear: { value: new THREE.Vector4(0, 0, 0, 0.3) }, heatBase: { value: 0 }, heatScale: { value: 1 }, soot: { value: 0 }, dissolve: { value: 0 },
  };
  material = new THREE.ShaderMaterial({
    name: `${source.name}:heated`,
    defines: { ...source.defines, HEATED: "" },
    uniforms,
    vertexShader: source.vertexShader
      .replace("#include <common>", "#include <common>\n#ifdef HEATED\n  varying vec3 vHeatLocal;\n#endif")
      .replace("#include <begin_vertex>", "#include <begin_vertex>\n    #ifdef HEATED\n      vHeatLocal = transformed;\n    #endif"),
    fragmentShader: heatedFragment(source.fragmentShader),
    lights: source.lights,
    transparent: source.transparent,
    depthWrite: source.depthWrite,
    depthTest: source.depthTest,
    blending: source.blending,
    side: source.side,
    alphaToCoverage: source.alphaToCoverage,
  });
  material.map = source.map;
  material.alphaTest = source.alphaTest;
  material.shadowSide = source.shadowSide;
  const dynamic = source.defines?.DYNAMIC !== undefined;
  material.onBeforeRender = (renderer, scene, camera, geometry, object) => {
    // The point lights this draw reads, as its source's draws choose them.
    if (source.userData.pointLit) selectLights(material, object, geometry, scene, camera);
    const look = object.userData.heated || NO_HEAT;
    uniforms.heat.value = look.heat;
    uniforms.heatTear.value.copy(look.tear);
    uniforms.heatBase.value = look.base || 0;
    uniforms.heatScale.value = look.scale || 1;
    uniforms.soot.value = look.soot;
    uniforms.dissolve.value = look.dissolve;
    uniforms.sunScale.value = look.sun;
    uniforms.brightness.value = look.brightness;
    uniforms.ambientPull.value.set(look.darken, look.lighten);
    if (dynamic) localLighting.select(object, scene.userData.isViewScene);
    material.uniformsNeedUpdate = true;
  };
  heatMaterials.set(source, material);
  return material;
}

const LIGHT_ZERO = { sun: 1, darken: 0, lighten: 0, brightness: 1 };

// The pieces on the field: their models and their looks, as the phase says
// they are. model(name) gives a function making that model (as debris.js
// uses). lightAt(position, out) gives { sun, darken, lighten, brightness }
// where a soldier there would be lit (game.js), or null.
export class DebrisPieces {
  constructor({ scene, model, lightAt = null, host }) {
    this.scene = scene;
    this.model = model;
    this.lightAt = lightAt;
    this.host = host;
    this.pieces = [];
    this.shapes = new Map(); // model name -> shape (null: no such model)
    this.handles = new Map(); // model name -> the phase's handle for its shape
    this.names = []; // handle -> model name
    this.free = new Map(); // model name -> idle models
    this.light = { ...LIGHT_ZERO };
    this.position = new THREE.Vector3();
  }

  get count() {
    return this.pieces.length;
  }

  // A model's shape, worked out once from its vertices and given to the phase.
  shape(name) {
    if (this.shapes.has(name)) return this.shapes.get(name);
    const object = this.build(name);
    // The phase has room for 64 models' shapes (the most any map has is 23); a
    // model past them throws no piece, as one with no model would not.
    const room = this.names.length < Dst.SHAPES;
    const shape = object && room ? pieceShape(object) : null;
    if (object) this.release(name, object);
    this.shapes.set(name, shape);
    if (shape) {
      const handle = this.names.length;
      this.names.push(name);
      this.handles.set(name, handle);
      this.host.shape(handle, shape);
    }
    return shape;
  }

  // The phase's handle for a model's shape, or -1 for a model with none.
  handle(name) {
    return this.shape(name) ? this.handles.get(name) : -1;
  }

  // An idle copy of the model, or a fresh one.
  acquire(name) {
    const idle = this.free.get(name);
    return idle?.length ? idle.pop() : this.build(name);
  }

  // A copy of the model with its materials heated.
  build(name) {
    const make = this.model(name);
    if (!make) return null;
    const object = make();
    object.traverse((mesh) => {
      if (!mesh.isMesh) return;
      mesh.material = Array.isArray(mesh.material) ? mesh.material.map(heatMaterial) : heatMaterial(mesh.material);
      mesh.receiveShadow = true;
      motionVectors.track(mesh);
    });
    object.userData.heated = { heat: 0, tear: new THREE.Vector4(0, 0, 0, 0.3), soot: 0, dissolve: 0, sun: 1, brightness: 1, darken: 0, lighten: 0 };
    object.traverse((mesh) => { if (mesh.isMesh) mesh.userData.heated = object.userData.heated; });
    this.scene.add(object);
    return object;
  }

  release(name, object) {
    object.visible = false;
    if (!object.parent) this.scene.add(object);
    if (!this.free.has(name)) this.free.set(name, []);
    this.free.get(name).push(object);
  }

  // A piece the phase made: its model, hot where it tore away (tear: the
  // point in the model's frame and how far the heat spreads), and shown.
  spawned(record, r) {
    const name = this.names[r[record + PIECE.SHAPE]];
    const object = this.acquire(name);
    // A model just let go of may come straight back somewhere else: its
    // motion for the upscaler starts afresh rather than from where it was.
    object.traverse((mesh) => { if (mesh.userData.motion) mesh.userData.motion.userData.seen = -1; });
    const look = object.userData.heated;
    look.heat = r[record + PIECE.HEAT];
    look.soot = r[record + PIECE.SOOT];
    look.dissolve = 0;
    look.tear.set(r[record + PIECE.TEAR], r[record + PIECE.TEAR + 1], r[record + PIECE.TEAR + 2], r[record + PIECE.TEAR + 3]);
    object.visible = true;
    const piece = { uid: r[record + PIECE.UID], name, object, shadows: true };
    this.shadows(piece, true);
    this.pieces.push(piece);
    return piece;
  }

  // A piece the phase let go of, by its place in the list: back to the idle
  // models.
  retire(i) {
    const [piece] = this.pieces.splice(i, 1);
    this.release(piece.name, piece.object);
  }

  // Shadows on or off. Off, each mesh keeps the setting graphics gave it.
  shadows(piece, on) {
    piece.shadows = on;
    piece.object.traverse((mesh) => {
      if (!mesh.isMesh) return;
      if (on && mesh.userData.castShadowSaved !== undefined) {
        mesh.castShadow = mesh.userData.castShadowSaved;
        delete mesh.userData.castShadowSaved;
      } else if (!on && mesh.userData.castShadowSaved === undefined) {
        mesh.userData.castShadowSaved = mesh.castShadow;
        mesh.castShadow = false;
      }
    });
  }

  // The pieces as the phase's records say they are: those it no longer lists
  // are let go, those it lists that the page does not have yet are made, and
  // each is set from its record. The pairing is by the phase's id for a
  // piece, never by place, and the records are the whole truth each time, so
  // nothing the page missed can leave the two out of step.
  sync(r, first, n) {
    const stride = PIECE.STRIDE;
    for (let i = this.pieces.length - 1; i >= 0; i--) {
      let listed = false;
      for (let k = 0; k < n && !listed; k++) listed = r[first + k * stride + PIECE.UID] === this.pieces[i].uid;
      if (!listed) this.retire(i);
    }
    for (let k = 0; k < n; k++) {
      const at = first + k * stride, uid = r[at + PIECE.UID];
      let piece = null;
      for (let i = 0; i < this.pieces.length && !piece; i++) if (this.pieces[i].uid === uid) piece = this.pieces[i];
      // A piece of a model the page has no shape for is not shown.
      if (!piece && this.names[r[at + PIECE.SHAPE]] === undefined) continue;
      piece ||= this.spawned(at, r);
      this.place(piece, r, at);
    }
  }

  // The map's light where a piece lies, asked for by the phase at a place.
  sample(piece, x, y, z) {
    if (!this.lightAt) return;
    const light = this.lightAt(this.position.set(x, y, z), this.light);
    const look = piece.object.userData.heated;
    look.sun = light.sun;
    look.darken = light.darken;
    look.lighten = light.lighten;
    look.brightness = light.brightness;
  }

  // The phase's record of a piece (its model's place, its heat and how far it
  // has crumbled, whether it casts a shadow, whether it wants the map's light
  // sampled). The matrices of its model follow from its position and turn when
  // the scene next updates them.
  place(piece, r, at) {
    const object = piece.object, look = object.userData.heated;
    if (r[at + PIECE.MOVED]) {
      object.quaternion.set(r[at + PIECE.QUATERNION], r[at + PIECE.QUATERNION + 1], r[at + PIECE.QUATERNION + 2], r[at + PIECE.QUATERNION + 3]);
      object.position.set(r[at + PIECE.POSITION], r[at + PIECE.POSITION + 1], r[at + PIECE.POSITION + 2]);
    }
    look.heat = r[at + PIECE.HEAT];
    if (r[at + PIECE.DISSOLVE] > 0) look.dissolve = r[at + PIECE.DISSOLVE];
    const shadows = r[at + PIECE.SHADOWS] === 1;
    if (shadows !== piece.shadows) this.shadows(piece, shadows);
    if (r[at + PIECE.SAMPLE]) this.sample(piece, r[at + PIECE.SAMPLE_POSITION], r[at + PIECE.SAMPLE_POSITION + 1], r[at + PIECE.SAMPLE_POSITION + 2]);
  }

  clear() {
    while (this.pieces.length) this.retire(this.pieces.length - 1);
  }

  // Out of the scene for good (the models' geometry and materials are the
  // model cache's, shared with everything else that draws them).
  dispose() {
    this.clear();
    for (const idle of this.free.values()) for (const object of idle) object.removeFromParent();
    this.free.clear();
  }
}
