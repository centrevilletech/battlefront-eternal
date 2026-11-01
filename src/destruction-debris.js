import * as THREE from "three";
import { localLighting } from "./local-lights.js";
import { motionVectors } from "./motion-vectors.js";
import { selectLights } from "./materials.js";

// The pieces of a droid blown apart, for the Enhanced destruction effects
// (src/destruction.js). Classic pieces stay with src/debris.js.
//
// Each piece is a rigid body: its centre of mass (its model's box centre),
// its orientation, and its linear and angular velocity, with the inertia of
// a box its size. It touches the world with a few dozen of its own extreme
// vertices. Each frame a ray straight down from its centre finds the ground
// under it, and a ray along its motion finds what it is about to hit; both
// count as planes. Contacts push back with an impulse at the touching point
// (so a piece landing on a corner turns over), with Coulomb friction and a
// bounce that only fast impacts keep. Pieces tumble, roll over, and come to
// rest lying on a face; one at rest for a moment sleeps until it is gone. A
// long piece that comes to rest standing on end is tipped over, the way
// wreckage falls, rather than left standing. Pieces also push each other
// apart (each a box a little smaller than its own), so a pile doesn't sink
// into itself. A piece that would start inside a wall or the ground starts
// on the blast's side of it.
//
// They are only for show: they don't collide with soldiers or rounds, and
// nothing in the simulation reads them.
//
// A piece is drawn with its own model and material, made to glow: hot where
// it tore away and in ragged patches around it, cooling from white through
// orange to dull red, and charred. Lit by the map like a soldier (the light
// region and light map where it lies) and casting shadows. At the end of its
// time it crumbles away (noise-dithered), its shadow going with it.

const GRAVITY = 11; // m/s², a touch over 1 g: pieces this size read weightier
const AIR_DRAG = 0.08; // 1/s
const SUBSTEP = 1 / 120; // s
const MAX_SUBSTEPS = 6;
const ITERATIONS = 4;
const RESTITUTION = 0.3;
const BOUNCE_MIN = 1.4; // m/s into a surface before anything bounces
const FRICTION = 0.62;
const SLOP = 0.004; // m of overlap left alone, so resting contact doesn't jitter
const CONTACT_DAMP = 1.6; // 1/s, rolling resistance while touching
const SETTLE_DAMP = 5; // 1/s once nearly still: the last rocking dies away
const SLEEP_SPEED = 0.12; // m/s
const SLEEP_SPIN = 0.4; // rad/s
const SLEEP_TIME = 0.3; // s
const DROWSY_SPEED = 0.6; // m/s: a piece only buzzing against others this long
const DROWSY_SPIN = 1.4; // rad/s
const DROWSY_TIME = 1.2; // s ...sleeps too
const TALL = 1.8; // centre this many smallest half sizes up: standing on end
const NUDGES = 2; // tips given to a piece balanced on end before it may rest
const NUDGE_SPIN = 2.6; // rad/s
const GROUND_STEP = 0.05; // m a touching piece moves before the ground under it is looked for again
const GROUND_REST = 0.25; // s ... or this long, touching
const GROUND_FLYING = 0.1; // s between looks in the air (its motion ray sees what it nears)
const PAIR_DELAY = 0.12; // s of flight before pieces of one body push each other
const PAIR_SHRINK = 0.85; // a piece's box, scaled, for other pieces to touch
const UP = new THREE.Vector3(0, 1, 0);
const DOWN = new THREE.Vector3(0, -1, 0);

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

// One rigid body: where its centre of mass is, how it is turned, and how it
// moves. Planes are what it may touch this frame: { normal, point }.
export class RigidPiece {
  constructor() {
    this.shape = null;
    this.position = new THREE.Vector3();
    this.quaternion = new THREE.Quaternion();
    this.velocity = new THREE.Vector3();
    this.spin = new THREE.Vector3();
    this.rotation = new THREE.Matrix3();
    this.inverseInertia = new THREE.Matrix3();
    this.planes = [0, 1].map(() => ({ normal: new THREE.Vector3(), point: new THREE.Vector3() }));
    this.planeCount = 0;
    // The ground last found under it: where it was looked for, and when.
    this.ground = { found: false, normal: new THREE.Vector3(), point: new THREE.Vector3(), at: new THREE.Vector3(), age: Infinity };
    this.sleeping = false;
    this.still = 0;
    this.drowsy = 0;
    this.nudges = 0;
    this.touching = false;
    this.flight = 0;
    this.impact = 0; // the hardest landing since it was last read, m/s
    this.impactPoint = new THREE.Vector3();
  }

  reset(shape, position, quaternion, velocity, spin) {
    this.shape = shape;
    this.position.copy(position);
    this.quaternion.copy(quaternion).normalize();
    this.velocity.copy(velocity);
    this.spin.copy(spin);
    this.sleeping = false;
    this.still = 0;
    this.drowsy = 0;
    this.nudges = 0;
    this.touching = false;
    this.flight = 0;
    this.impact = 0;
    this.planeCount = 0;
    this.ground.found = false;
    this.ground.age = Infinity;
    this.orient();
    return this;
  }

  // Its rotation matrix and world-space inverse inertia, from its quaternion.
  orient() {
    const r = this.rotation.setFromMatrix4(m4.makeRotationFromQuaternion(this.quaternion));
    const e = r.elements, k = this.shape.inverseInertia;
    // R diag(k) R^T, written out.
    const i = this.inverseInertia.elements;
    for (let row = 0; row < 3; row++) {
      for (let col = 0; col < 3; col++) {
        i[col * 3 + row] = e[row] * k.x * e[col] + e[3 + row] * k.y * e[3 + col] + e[6 + row] * k.z * e[6 + col];
      }
    }
  }
}

const m4 = new THREE.Matrix4();
const s = {
  r: new THREE.Vector3(), p: new THREE.Vector3(), vel: new THREE.Vector3(),
  t: new THREE.Vector3(), a: new THREE.Vector3(), b: new THREE.Vector3(), dq: new THREE.Quaternion(),
  local: new THREE.Vector3(), normal: new THREE.Vector3(), rb: new THREE.Vector3(), velB: new THREE.Vector3(),
  origin: new THREE.Vector3(), dir: new THREE.Vector3(), mt: new THREE.Matrix3(),
};

// How easily one body gives at offset r along unit direction n (its
// inverse effective mass): 1/m + n . ((I^-1 (r x n)) x r), with m = 1. An
// impulse that changes the speed there by dv is dv over this.
function reach(piece, r, n) {
  s.a.crossVectors(r, n).applyMatrix3(piece.inverseInertia);
  return 1 + n.dot(s.b.crossVectors(s.a, r));
}

function push(piece, r, direction, amount) {
  piece.velocity.addScaledVector(direction, amount);
  s.a.crossVectors(r, direction).applyMatrix3(piece.inverseInertia);
  piece.spin.addScaledVector(s.a, amount);
}

// Steps every awake piece against the world (collision: anything with
// cast(origin, direction, far, hit)).
export class PieceSolver {
  constructor(collision) {
    this.collision = collision;
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
    this.rays = 0;
  }

  // What a piece may touch this frame: the ground under its centre, and
  // whatever lies along its motion. The ground is looked for again only once
  // a piece on it has moved off the spot (or now and then in the air).
  probe(piece, dt, touched) {
    const r = piece.shape.radius;
    const ground = piece.ground;
    ground.age += dt;
    const stale = touched
      ? ground.age >= GROUND_REST || ground.at.distanceToSquared(piece.position) > GROUND_STEP * GROUND_STEP
      : ground.age >= GROUND_FLYING;
    if (stale) {
      ground.age = 0;
      ground.at.copy(piece.position);
      s.origin.copy(piece.position).addScaledVector(UP, 0.05);
      this.rays++;
      ground.found = Boolean(this.collision.cast(s.origin, DOWN, r + 0.6 + Math.max(-piece.velocity.y, 0) * GROUND_FLYING, this.hit));
      if (ground.found) {
        ground.normal.copy(this.hit.normal).normalize();
        ground.point.copy(this.hit.point);
      }
    }
    piece.planeCount = 0;
    if (ground.found) this.plane(piece, ground.normal, ground.point);
    const speed = piece.velocity.length();
    if (speed > 0.25) {
      s.dir.copy(piece.velocity).divideScalar(speed);
      this.rays++;
      const hit = this.collision.cast(piece.position, s.dir, r + speed * dt + 0.05, this.hit);
      if (hit && hit.normal.dot(s.dir) < -0.05) this.plane(piece, hit.normal, hit.point);
    }
  }

  plane(piece, normal, point) {
    for (let i = 0; i < piece.planeCount; i++) {
      const known = piece.planes[i];
      if (known.normal.dot(normal) > 0.98 && Math.abs(known.normal.dot(s.a.subVectors(point, known.point))) < 0.02) return;
    }
    if (piece.planeCount >= piece.planes.length) return;
    const plane = piece.planes[piece.planeCount++];
    plane.normal.copy(normal).normalize();
    plane.point.copy(point);
  }

  step(pieces, dt) {
    if (dt <= 0) return;
    for (const piece of pieces) if (!piece.sleeping) {
      const touched = piece.touching;
      piece.touching = false;
      this.probe(piece, dt, touched);
    }
    const n = Math.min(Math.max(Math.ceil(dt / SUBSTEP), 1), MAX_SUBSTEPS);
    const h = dt / n;
    for (let k = 0; k < n; k++) {
      for (const piece of pieces) {
        if (piece.sleeping) continue;
        this.integrate(piece, h);
        this.contacts(piece, h);
      }
      this.pairs(pieces);
    }
    for (const piece of pieces) {
      if (piece.sleeping) continue;
      piece.flight += dt;
      const speed = piece.velocity.lengthSq(), spin = piece.spin.lengthSq();
      const slow = speed < SLEEP_SPEED * SLEEP_SPEED && spin < SLEEP_SPIN * SLEEP_SPIN;
      piece.still = piece.touching && slow ? piece.still + dt : 0;
      piece.drowsy = piece.touching && speed < DROWSY_SPEED * DROWSY_SPEED && spin < DROWSY_SPIN * DROWSY_SPIN ? piece.drowsy + dt : 0;
      if (piece.still >= SLEEP_TIME || piece.drowsy >= DROWSY_TIME) {
        if (this.balanced(piece)) continue;
        piece.sleeping = true;
        piece.velocity.set(0, 0, 0);
        piece.spin.set(0, 0, 0);
      }
    }
  }

  // A long piece come to rest standing on end gets a tip, so pieces lie
  // down the way wreckage does instead of standing like a statue. Returns
  // whether it was tipped.
  balanced(piece) {
    if (piece.nudges >= NUDGES || !piece.planeCount) return false;
    const { normal, point } = piece.planes[0];
    const height = normal.dot(s.p.subVectors(piece.position, point));
    const half = piece.shape.half;
    if (height <= TALL * Math.min(half.x, half.y, half.z) + 0.02) return false;
    piece.nudges++;
    piece.still = piece.drowsy = 0;
    // About a level axis, across the piece's longest way up.
    s.t.set(1, 0, 0);
    if (half.y >= half.x && half.y >= half.z) s.t.set(0, 1, 0);
    else if (half.z >= half.x) s.t.set(0, 0, 1);
    s.t.applyMatrix3(piece.rotation).addScaledVector(normal, -s.t.dot(normal));
    if (s.t.lengthSq() < 1e-4) s.t.set(normal.y, -normal.x, 0).add(s.a.set(0.3, 0, 0.7));
    s.a.crossVectors(normal, s.t.normalize()).normalize();
    piece.spin.addScaledVector(s.a, NUDGE_SPIN * (piece.nudges % 2 ? 1 : -1));
    piece.velocity.addScaledVector(s.t, 0.15);
    return true;
  }

  integrate(piece, h) {
    piece.velocity.y -= GRAVITY * h;
    piece.velocity.multiplyScalar(1 - AIR_DRAG * h);
    piece.position.addScaledVector(piece.velocity, h);
    const w = piece.spin, q = piece.quaternion;
    const dq = s.dq.set(w.x * h * 0.5, w.y * h * 0.5, w.z * h * 0.5, 0).multiply(q);
    q.set(q.x + dq.x, q.y + dq.y, q.z + dq.z, q.w + dq.w).normalize();
    piece.orient();
  }

  // Impulses where its points are in a plane, then out of it.
  contacts(piece, h) {
    const points = piece.shape.points;
    for (let p = 0; p < piece.planeCount; p++) {
      const { normal, point } = piece.planes[p];
      let deepest = 0;
      for (let iteration = 0; iteration < ITERATIONS; iteration++) {
        for (const local of points) {
          const r = s.r.copy(local).applyMatrix3(piece.rotation);
          const depth = -normal.dot(s.p.addVectors(piece.position, r).sub(point));
          if (depth < 0) continue;
          piece.touching = true;
          if (depth > deepest) deepest = depth;
          const vel = s.vel.crossVectors(piece.spin, r).add(piece.velocity);
          const into = vel.dot(normal);
          if (into >= 0) continue;
          if (iteration === 0 && -into > piece.impact) {
            piece.impact = -into;
            piece.impactPoint.copy(s.p);
          }
          const target = iteration === 0 && into < -BOUNCE_MIN ? -RESTITUTION * into : 0;
          const j = (target - into) / reach(piece, r, normal);
          push(piece, r, normal, j);
          // Friction against the sliding, at most mu times the push.
          vel.crossVectors(piece.spin, r).add(piece.velocity);
          const t = s.t.copy(vel).addScaledVector(normal, -vel.dot(normal));
          const slide = t.length();
          if (slide < 1e-5) continue;
          t.divideScalar(slide);
          push(piece, r, t, -Math.min(slide / reach(piece, r, t), FRICTION * j));
        }
      }
      if (deepest > SLOP) piece.position.addScaledVector(normal, deepest - SLOP);
    }
    if (piece.touching) {
      const slow = piece.velocity.lengthSq() < 0.36 && piece.spin.lengthSq() < 2.5;
      const damp = Math.exp(-(slow ? SETTLE_DAMP : CONTACT_DAMP) * h);
      piece.spin.multiplyScalar(damp);
      if (slow) piece.velocity.multiplyScalar(damp);
    }
  }

  // Pieces against each other: a point of one inside another's box is
  // pushed out through the nearest face. A sleeping piece stays put.
  pairs(pieces) {
    for (let i = 0; i < pieces.length; i++) {
      const a = pieces[i];
      for (let k = i + 1; k < pieces.length; k++) {
        const b = pieces[k];
        if (a.sleeping && b.sleeping) continue;
        if (a.flight < PAIR_DELAY || b.flight < PAIR_DELAY) continue;
        const span = a.shape.radius + b.shape.radius;
        if (!(a.position.distanceToSquared(b.position) <= span * span)) continue;
        if (!a.sleeping) this.inside(a, b);
        if (!b.sleeping) this.inside(b, a);
      }
    }
  }

  inside(a, b) {
    const half = b.shape.half;
    for (const local of a.shape.points) {
      const ra = s.r.copy(local).applyMatrix3(a.rotation);
      s.p.addVectors(a.position, ra);
      // Into b's frame: R^T (p - centre).
      s.local.subVectors(s.p, b.position).applyMatrix3(s.mt.copy(b.rotation).transpose());
      const ex = half.x * PAIR_SHRINK - Math.abs(s.local.x);
      const ey = half.y * PAIR_SHRINK - Math.abs(s.local.y);
      const ez = half.z * PAIR_SHRINK - Math.abs(s.local.z);
      if (!(ex > 0 && ey > 0 && ez > 0)) continue;
      const depth = Math.min(ex, ey, ez);
      if (depth === ex) s.normal.set(Math.sign(s.local.x) || 1, 0, 0);
      else if (depth === ey) s.normal.set(0, Math.sign(s.local.y) || 1, 0);
      else s.normal.set(0, 0, Math.sign(s.local.z) || 1);
      const normal = s.normal.applyMatrix3(b.rotation).normalize();
      const rb = s.rb.subVectors(s.p, b.position);
      const vel = s.vel.crossVectors(a.spin, ra).add(a.velocity);
      if (!b.sleeping) vel.sub(s.velB.crossVectors(b.spin, rb).add(b.velocity));
      const into = vel.dot(normal);
      a.touching = true;
      if (into < 0) {
        const response = reach(a, ra, normal) + (b.sleeping ? 0 : reach(b, rb, normal));
        const j = -into * 1.1 / response;
        push(a, ra, normal, j);
        if (!b.sleeping) push(b, rb, normal, -j);
      }
      const share = b.sleeping ? 1 : 0.5;
      a.position.addScaledVector(normal, depth * share);
      if (!b.sleeping) b.position.addScaledVector(normal, -depth * share);
    }
  }
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

const LINGER = 22; // s at rest before a piece crumbles away
const CRUMBLE = 1.1; // s to crumble
const MAX_FLIGHT = 14; // s; a piece still moving then (lost off the map) crumbles anyway
const COOL = 2.6; // s for the heat to fall by e
const LIGHT_EVERY = 0.25; // s between light samples while moving
const LOST = 30; // m below where it started: fallen out of the world
const SHADOW_REACH = 45; // m from the eye past which a piece casts no shadow

// The pieces on the field: their bodies, their models, and their looks.
// model(name) gives a function making that model (as debris.js uses).
// lightAt(position, out) gives { sun, darken, lighten, brightness } where
// a soldier there would be lit (game.js), or null.
export class DebrisPieces {
  constructor({ scene, collision, model, lightAt = null, capacity = 40 }) {
    this.scene = scene;
    this.model = model;
    this.lightAt = lightAt;
    this.capacity = capacity;
    this.solver = new PieceSolver(collision);
    this.pieces = [];
    this.bodies = [];
    this.shapes = new Map(); // model name -> shape
    this.free = new Map(); // model name -> idle models
    this.spawned = 0;
    this.matrix = new THREE.Matrix4();
    this.position = new THREE.Vector3();
    this.quaternion = new THREE.Quaternion();
    this.scale = new THREE.Vector3();
    this.light = { sun: 1, darken: 0, lighten: 0, brightness: 1 };
    this.offset = new THREE.Vector3();
    this.from = new THREE.Vector3();
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
  }

  get count() {
    return this.pieces.length;
  }

  shape(name) {
    if (this.shapes.has(name)) return this.shapes.get(name);
    const object = this.build(name);
    const shape = object ? pieceShape(object) : null;
    if (object) this.release(name, object);
    this.shapes.set(name, shape);
    return shape;
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

  // A piece of model `name`, its model frame at `matrix` (world), moving at
  // `velocity` and turning at `spin` (world rad/s). tear: the point (model
  // frame) it tore away at; heat 0..1; from: a point in the open (the blast)
// it must start on the same side of every surface as. Returns the piece, or
// null.
  spawn(name, matrix, { velocity, spin, tear = null, heat = 1, soot = 1, from = null } = {}) {
    const shape = this.shape(name);
    if (!shape) return null;
    // A frame or motion that isn't finite would only spread to its neighbours.
    if (!matrix.elements.every(Number.isFinite) || !finite(velocity) || !finite(spin)) return null;
    if (this.pieces.length >= this.capacity) this.retire(this.pieces[0]);
    const object = this.acquire(name);
    if (!object) return null;
    // A model just let go of may come straight back somewhere else: its
    // motion for the upscaler starts afresh rather than from where it was.
    object.traverse((mesh) => { if (mesh.userData.motion) mesh.userData.motion.userData.seen = -1; });
    matrix.decompose(this.position, this.quaternion, this.scale);
    const centre = this.offset.copy(shape.center).applyQuaternion(this.quaternion).add(this.position);
    // It starts on the blast's side of every surface: a centre through the
    // ground (a foot in a slope) or a wall is brought back out of it.
    if (from) {
      const ray = this.from.subVectors(centre, from);
      const length = ray.length();
      if (length > 0.01 && this.solver.collision.cast(from, ray.divideScalar(length), length, this.hit)) {
        centre.copy(this.hit.point).addScaledVector(ray, -0.03);
      }
    }
    const body = new RigidPiece().reset(shape, centre, this.quaternion, velocity, spin);
    body.lowest = centre.y - LOST;
    const look = object.userData.heated;
    look.heat = heat;
    look.soot = soot;
    look.dissolve = 0;
    if (tear) look.tear.set(tear.x, tear.y, tear.z, Math.max(shape.radius * 0.45, 0.08));
    else look.tear.set(shape.center.x, shape.center.y, shape.center.z, shape.radius * 0.5);
    const piece = { name, object, body, shape, age: 0, rest: 0, heat, crumble: 0, lightClock: 0, gone: false, shadows: true };
    object.visible = true;
    this.shadows(piece, true);
    this.place(piece);
    this.sample(piece);
    this.pieces.push(piece);
    this.bodies.push(body);
    this.spawned++;
    return piece;
  }

  // Gone at once (over budget): back to the idle models.
  retire(piece) {
    piece.gone = true;
    this.release(piece.name, piece.object);
    const i = this.pieces.indexOf(piece);
    if (i >= 0) { this.pieces.splice(i, 1); this.bodies.splice(i, 1); }
  }

  place(piece) {
    const { object, body, shape } = piece;
    object.quaternion.copy(body.quaternion);
    object.position.copy(shape.center).applyQuaternion(body.quaternion).negate().add(body.position);
    object.updateMatrixWorld(true);
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

  // The map's light where it lies.
  sample(piece) {
    if (!this.lightAt) return;
    const light = this.lightAt(piece.body.position, this.light);
    const look = piece.object.userData.heated;
    look.sun = light.sun;
    look.darken = light.darken;
    look.lighten = light.lighten;
    look.brightness = light.brightness;
  }

  // eye: the camera's position (pieces far from it cast no shadow).
  update(dt, eye = null) {
    if (dt <= 0 || !this.pieces.length) return;
    this.solver.step(this.bodies, dt);
    for (let i = this.pieces.length - 1; i >= 0; i--) {
      const piece = this.pieces[i];
      const body = piece.body;
      piece.age += dt;
      piece.heat *= Math.exp(-dt / COOL);
      const look = piece.object.userData.heated;
      look.heat = piece.heat;
      if (!body.sleeping) {
        this.place(piece);
        piece.lightClock -= dt;
        if (piece.lightClock <= 0) {
          piece.lightClock = LIGHT_EVERY;
          this.sample(piece);
        }
      } else if (piece.rest === 0) this.sample(piece);
      if (body.sleeping) piece.rest += dt;
      if (piece.rest >= LINGER || piece.age >= MAX_FLIGHT + LINGER || (!body.sleeping && body.flight >= MAX_FLIGHT)) piece.crumble += dt / CRUMBLE;
      if (piece.crumble > 0) look.dissolve = Math.min(piece.crumble, 1) * 1.02;
      // No shadow once it starts to crumble, or far off.
      const shadows = piece.crumble <= 0 && (!eye || eye.distanceToSquared(body.position) < SHADOW_REACH * SHADOW_REACH);
      if (shadows !== piece.shadows) this.shadows(piece, shadows);
      if (piece.crumble >= 1 || !(body.position.y >= body.lowest) || !finite(body.position)) this.retire(piece);
    }
  }

  clear() {
    for (const piece of [...this.pieces]) this.retire(piece);
  }

  // Out of the scene for good (the models' geometry and materials are the
  // model cache's, shared with everything else that draws them).
  dispose() {
    this.clear();
    for (const idle of this.free.values()) for (const object of idle) object.removeFromParent();
    this.free.clear();
  }
}

function finite(v) {
  return !v || (Number.isFinite(v.x) && Number.isFinite(v.y) && Number.isFinite(v.z));
}
