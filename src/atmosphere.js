import * as THREE from 'three';
import { makeGround } from './collision.js';
import { lighting } from './materials.js';
import { particleDepth } from './particle-depth.js';

// The world's haze (map.json `atmosphere`, tools/zero_atmosphere.py), drawn
// as the 2004 engine draws it:
//
//   FogCloud  a band of big soft squares around the camera, 16 m or so over
//             the terrain, drifting with the wind and slowly turning
//             (Geonosis's blowing haze; Endor, the Citadel, both Yavins)
//   dust      dusteffect objects: clouds of squares blowing through a box,
//             flattened into low sheets (Geonosis, Hoth, Kamino, the
//             Citadel, the Dune Sea, both Yavins)
//
// Both are page-side presentation: the same code draws them in the main
// thread, under a world worker and online. The simulations are small (a
// fixed pattern, a few dozen particles a class) and run here; each texture
// is one instanced draw of at most CAPACITY squares, sorted back to front.

const CAPACITY = 4096;
const SQRT_HALF = Math.SQRT1_2;

// The engine's random numbers: two steps of a 32-bit LCG make one draw, the
// seed first xored with 0xC71C2830 (its foliage and FogCloud use them).
export class EngineRandom {
  constructor(seed = 1) { this.s = (seed ^ 0xc71c2830) >>> 0; }
  int() {
    const s1 = (Math.imul(this.s, 1664525) + 1013904223) >>> 0;
    const s2 = (Math.imul(s1, 1664525) + 1013904223) >>> 0;
    this.s = s2;
    return ((s2 & 0xffff0000) | (s1 >>> 16)) >>> 0;
  }
  float() { return (this.int() >>> 9) / 8388608; }
}

const wrap = (value, period) => { const m = value % period; return m < 0 ? m + period : m; };

// The FogCloud squares in view at `time` seconds, for a view (eye; unit
// right, up and forward; the tangents of half the field of view across and
// up): emit(x, y, z, angle, alpha) for each. Every 256 m cell holds the same
// `density` points, drawn once from a fixed seed, scrolled by the wind and
// stood `height` over the ground. A point draws inside the box around the
// view's slab from `range` near to far, its alpha 1 - t^2 of the colour's, t
// running -1..1 as the squared distance runs between the range's squares.
export function fogCloudParticles(def, time, view, heightAt, emit) {
  const [near, far] = def.range, cell = def.cell || 256, size = def.size;
  if (!(far > near) || !(def.density > 0)) return 0;
  const { eye, right, up, forward, tanX, tanY } = view;
  const norm = 1 / Math.sqrt(1 + tanX * tanX + tanY * tanY);
  let minX = Infinity, minY = Infinity, minZ = Infinity, maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
  const take = (dx, dy, dz, distance) => {
    const x = eye.x + dx * distance, y = eye.y + dy * distance, z = eye.z + dz * distance;
    if (x < minX) minX = x; if (x > maxX) maxX = x;
    if (y < minY) minY = y; if (y > maxY) maxY = y;
    if (z < minZ) minZ = z; if (z > maxZ) maxZ = z;
  };
  take(forward.x, forward.y, forward.z, near);
  take(forward.x, forward.y, forward.z, far);
  for (const sx of [-1, 1]) for (const sy of [-1, 1]) {
    const dx = (forward.x + right.x * sx * tanX + up.x * sy * tanY) * norm;
    const dy = (forward.y + right.y * sx * tanX + up.y * sy * tanY) * norm;
    const dz = (forward.z + right.z * sx * tanX + up.z * sy * tanY) * norm;
    take(dx, dy, dz, near);
    take(dx, dy, dz, far);
  }
  const pad = size * SQRT_HALF;
  minX -= pad; minY -= pad; minZ -= pad; maxX += pad; maxY += pad; maxZ += pad;
  const x0 = Math.floor(minX / cell), x1 = Math.floor(maxX / cell);
  const z0 = Math.floor(minZ / cell), z1 = Math.floor(maxZ / cell);
  const offsetX = wrap(def.velocity[0] * time, cell), offsetZ = wrap(def.velocity[1] * time, cell);
  const spin = wrap(def.rotation * time + 1, 2) - 1;
  const mid = (near * near + far * far) * 0.5, scale = 2 / (far * far - near * near);
  const alpha = (def.color[3] ?? 255) / 255;
  const random = new EngineRandom(1);
  let count = 0;
  for (let i = 0; i < def.density; i++) {
    let x = random.float() * cell + offsetX;
    let z = random.float() * cell + offsetZ;
    const phase = random.float() * 2;
    const turn = random.int() & 0x10 ? -1 : 1;
    if (x > cell) x -= cell;
    if (z > cell) z -= cell;
    let angle = turn * spin + phase;
    if (angle > 1) angle -= 2;
    for (let cx = x0; cx <= x1; cx++) {
      const px = cx * cell + x;
      if (px < minX || px >= maxX) continue;
      for (let cz = z0; cz <= z1; cz++) {
        const pz = cz * cell + z;
        if (pz < minZ || pz >= maxZ) continue;
        const ground = heightAt(px, pz);
        if (ground === null) continue;
        const py = ground + def.height;
        if (py < minY || py >= maxY) continue;
        const dx = px - eye.x, dy = py - eye.y, dz = pz - eye.z;
        const t = (dx * dx + dy * dy + dz * dz - mid) * scale;
        if (t * t >= 1) continue;
        emit(px, py, pz, angle * Math.PI, (1 - t * t) * alpha);
        count++;
      }
    }
  }
  return count;
}

// Terrain heights on a grid of corners, each asked of the collision world
// once and blended between: FogCloud stands hundreds of squares a frame on
// the ground, and its points move a few centimetres a frame.
export class GroundHeights {
  constructor(collision, cell = 8, limit = 65536) {
    this.collision = collision; this.cell = cell; this.limit = limit;
    this.corners = new Map(); this.ground = makeGround(); this.queries = 0;
  }
  corner(ix, iz) {
    const key = ix * 1048576 + iz;
    let height = this.corners.get(key);
    if (height === undefined) {
      if (this.corners.size >= this.limit) this.corners.clear();
      const hit = this.collision?.ground(ix * this.cell, iz * this.cell, 100000, this.ground);
      height = hit ? hit.height : NaN;
      this.corners.set(key, height);
      this.queries++;
    }
    return height;
  }
  at(x, z) {
    const fx = x / this.cell, fz = z / this.cell, ix = Math.floor(fx), iz = Math.floor(fz), u = fx - ix, v = fz - iz;
    const a = this.corner(ix, iz), b = this.corner(ix + 1, iz), c = this.corner(ix, iz + 1), d = this.corner(ix + 1, iz + 1);
    if (a === a && b === b && c === c && d === d) return (a * (1 - u) + b * u) * (1 - v) + (c * (1 - u) + d * u) * v;
    // At the terrain's edge or a cut, the corners that have ground.
    let sum = 0, weight = 0;
    for (const [h, w] of [[a, (1 - u) * (1 - v)], [b, u * (1 - v)], [c, (1 - u) * v], [d, u * v]]) if (h === h) { sum += h * w; weight += w; }
    return weight > 0 ? sum / weight : null;
  }
}

// One dusteffect class's particles, shared by every object of the class.
// Each is [x, y, z, vx, vy, vz, size, life left, age, alpha] in the object's
// frame (its axes the world's, before the class's height scale).
export class DustCloud {
  constructor(def, random = new EngineRandom(1)) {
    this.def = def; this.random = random;
    this.particles = Array.from({ length: def.count }, () => new Float32Array(10));
    for (const p of this.particles) {
      this.spawn(p);
      // A cloud starts full: lives part spent, every particle at its alpha.
      p[7] *= this.random.float();
      p[9] = def.alpha;
    }
  }
  between(a, b) { return a + this.random.float() * (b - a); }
  spawn(p) {
    const d = this.def;
    p[2] = this.between(d.minPos[2], d.maxPos[2]);
    p[1] = this.between(d.minPos[1], d.maxPos[1]);
    p[0] = this.between(d.minPos[0], d.maxPos[0]);
    p[9] = 0; p[8] = 0;
    p[5] = this.between(d.minVel[2], d.maxVel[2]);
    p[4] = this.between(d.minVel[1], d.maxVel[1]);
    p[3] = this.between(d.minVel[0], d.maxVel[0]);
    p[6] = this.between(d.minSize, d.maxSize);
    p[7] = this.between(d.minLifetime, d.maxLifetime);
  }
  // Ages and moves each particle; a spent one, or one past the radius
  // fade, starts again.
  update(dt) {
    const d = this.def, [fadeMin, fadeMax] = d.radiusFade;
    for (const p of this.particles) {
      p[8] += dt; p[7] -= dt;
      if (p[7] < 0) { this.spawn(p); continue; }
      p[0] += p[3] * dt; p[1] += p[4] * dt; p[2] += p[5] * dt;
      // In at 1 a second to the class's alpha; its last second, Alpha x
      // life left x that (the engine's own rule, which steps down there).
      const rise = Math.min(p[8], d.alpha);
      p[9] = p[7] < 1 ? d.alpha * p[7] * rise : rise;
      if (fadeMin > 0) {
        const r2 = p[0] * p[0] + p[2] * p[2];
        if (r2 > fadeMin * fadeMin) {
          if (r2 > fadeMax * fadeMax) { this.spawn(p); continue; }
          p[9] *= 1 - (Math.sqrt(r2) - fadeMin) / (fadeMax - fadeMin);
        }
      }
    }
  }
}

// How strongly an object of the class draws, its position's view depth
// given: the class's radius on top, out past MaxDistance, fading in over the
// 200 m before it (and over 100 m beyond a MinDistance). 0 when not drawn.
export function dustFade(def, depth) {
  const reach = depth + def.radius;
  if (reach < def.minDistance || reach > def.maxDistance) return 0;
  if (def.minDistance > 0 && reach < def.minDistance + 100) return Math.max(0, (reach - def.minDistance) * 0.01);
  return Math.min((def.maxDistance - reach) * 0.005, 1);
}

const VERTEX = /* glsl */ `
  attribute vec2 corner;
  attribute vec4 center; // position, size across
  attribute vec2 spin; // angle, alpha
  uniform vec3 axisU, axisV;
  varying vec2 vUv;
  varying float vAlpha, vDepth, vSoft;
  void main() {
    float c = cos(spin.x), s = sin(spin.x);
    vec2 turned = vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y);
    vec3 world = center.xyz + (axisU * turned.x + axisV * turned.y) * (center.w * 0.5);
    vec4 view = viewMatrix * vec4(world, 1.0);
    vUv = corner * 0.5 + 0.5;
    vAlpha = spin.y;
    vDepth = -view.z;
    vSoft = clamp(center.w * 0.1, 0.5, 3.0);
    gl_Position = projectionMatrix * view;
  }
`;
const FRAGMENT = /* glsl */ `
  uniform sampler2D map;
  uniform vec4 tint;
  uniform vec2 fade, depthResolution, cameraRange;
  uniform sampler2D sceneDepth;
  uniform float softParticles;
  varying vec2 vUv;
  varying float vAlpha, vDepth, vSoft;
  void main() {
    vec4 color = texture2D(map, vUv) * vec4(tint.rgb, tint.a * vAlpha);
    // The near scene's end, as every particle fades there.
    float near = clamp((fade.y - vDepth) / max(fade.y - fade.x, 1.0), 0.0, 1.0);
    color.a *= near * near;
    if (softParticles > 0.5) {
      float depth = texture2D(sceneDepth, gl_FragCoord.xy / depthResolution).x;
      float surface = cameraRange.x * cameraRange.y / (cameraRange.y - depth * (cameraRange.y - cameraRange.x));
      color.a *= smoothstep(0.0, vSoft, surface - vDepth);
    }
    if (color.a < 0.002) discard;
    gl_FragColor = color;
  }
`;

// One texture's squares: one instanced draw, refilled every frame.
class SquareBatch {
  constructor(scene, texture, name, tint = [1, 1, 1, 1]) {
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.setAttribute('corner', new THREE.Float32BufferAttribute([-1, -1, 1, -1, 1, 1, -1, 1], 2));
    this.geometry.setIndex([0, 1, 2, 0, 2, 3]);
    this.center = new THREE.InstancedBufferAttribute(new Float32Array(CAPACITY * 4), 4).setUsage(THREE.DynamicDrawUsage);
    this.spin = new THREE.InstancedBufferAttribute(new Float32Array(CAPACITY * 2), 2).setUsage(THREE.DynamicDrawUsage);
    this.geometry.setAttribute('center', this.center);
    this.geometry.setAttribute('spin', this.spin);
    this.geometry.instanceCount = 0;
    this.material = new THREE.ShaderMaterial({
      vertexShader: VERTEX, fragmentShader: FRAGMENT,
      uniforms: {
        map: { value: texture }, tint: { value: new THREE.Vector4(...tint) },
        axisU: { value: new THREE.Vector3(1, 0, 0) }, axisV: { value: new THREE.Vector3(0, 1, 0) },
        fade: lighting.fade, sceneDepth: particleDepth.map, softParticles: particleDepth.enabled,
        depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
      },
      transparent: true, depthWrite: false, side: THREE.DoubleSide, forceSinglePass: true, blending: THREE.NormalBlending,
    });
    this.mesh = new THREE.Mesh(this.geometry, this.material);
    this.mesh.name = name;
    this.mesh.frustumCulled = false;
    this.mesh.visible = false;
    this.mesh.renderOrder = 10;
    this.mesh.onBeforeRender = (renderer, s, camera) => particleDepth.capture(renderer, s, camera);
    scene.add(this.mesh);
    // Squares gathered this frame: [depth, x, y, z, size, angle, alpha].
    this.items = []; this.used = 0;
  }
  begin() { this.used = 0; }
  push(depth, x, y, z, size, angle, alpha) {
    if (this.used === CAPACITY || !(alpha > 0.001)) return;
    const item = this.items[this.used] ||= [0, 0, 0, 0, 0, 0, 0];
    item[0] = depth; item[1] = x; item[2] = y; item[3] = z; item[4] = size; item[5] = angle; item[6] = alpha;
    this.used++;
  }
  end() {
    const items = this.items, n = this.used;
    // Far to near, so the soft squares blend over one another in order.
    if (items.length > n) items.length = n;
    items.sort((a, b) => b[0] - a[0]);
    const center = this.center.array, spin = this.spin.array;
    for (let i = 0; i < n; i++) {
      const item = items[i];
      center[i * 4] = item[1]; center[i * 4 + 1] = item[2]; center[i * 4 + 2] = item[3]; center[i * 4 + 3] = item[4];
      spin[i * 2] = item[5]; spin[i * 2 + 1] = item[6];
    }
    this.geometry.instanceCount = n;
    this.mesh.visible = n > 0;
    if (!n) return;
    for (const attribute of [this.center, this.spin]) {
      attribute.clearUpdateRanges();
      attribute.addUpdateRange(0, n * attribute.itemSize);
      attribute.needsUpdate = true;
    }
  }
  dispose() { this.mesh.removeFromParent(); this.geometry.dispose(); this.material.dispose(); }
}

const RIGHT = new THREE.Vector3(), UP = new THREE.Vector3(), FORWARD = new THREE.Vector3(), EYE = new THREE.Vector3();
const FRUSTUM = new THREE.Frustum(), VIEW_PROJECTION = new THREE.Matrix4(), SPHERE = new THREE.Sphere();

export class Atmosphere {
  constructor({ data, scene, collision, texture }) {
    this.time = 0; this.batches = [];
    this.view = { eye: EYE, right: RIGHT, up: UP, forward: FORWARD, tanX: 1, tanY: 1 };
    this.heights = new GroundHeights(collision);
    const fog = data?.fogCloud;
    const map = texture(fog?.texture);
    if (fog && map) {
      const c = fog.color;
      this.fog = { def: fog, batch: this.add(new SquareBatch(scene, map, 'map-fog-cloud', [c[0] / 255, c[1] / 255, c[2] / 255, 1])) };
    }
    this.dust = [];
    const dust = data?.dust;
    for (const [name, def] of Object.entries(dust?.classes || {})) {
      const map = texture(def.texture);
      const instances = dust.instances.filter(i => i.class === name).map(i => new THREE.Vector3(...i.position));
      if (!map || !instances.length || !def.count) continue;
      let seed = 0;
      for (const ch of name) seed = (Math.imul(seed, 31) + ch.charCodeAt(0)) >>> 0;
      this.dust.push({ def, instances, cloud: new DustCloud(def, new EngineRandom(seed)), batch: this.add(new SquareBatch(scene, map, `map-dust-${name}`)) });
    }
    this.stats = { fog: 0, dust: 0, objects: 0 };
  }
  add(batch) { this.batches.push(batch); return batch; }
  update(dt, camera) {
    dt = Math.min(Math.max(dt, 0), 0.25);
    this.time += dt;
    camera.updateMatrixWorld();
    const e = camera.matrixWorld.elements;
    EYE.set(e[12], e[13], e[14]);
    RIGHT.set(e[0], e[1], e[2]).normalize();
    UP.set(e[4], e[5], e[6]).normalize();
    FORWARD.set(-e[8], -e[9], -e[10]).normalize();
    const tanY = Math.tan(THREE.MathUtils.degToRad(camera.fov ?? 60) * 0.5);
    this.view.tanY = tanY; this.view.tanX = tanY * (camera.aspect ?? 1);
    // Copies are culled as the engine culls any object: by a sphere of the
    // class's radius around them, against the view.
    FRUSTUM.setFromProjectionMatrix(VIEW_PROJECTION.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
    for (const batch of this.batches) batch.begin();
    this.stats.fog = this.stats.dust = this.stats.objects = 0;
    if (this.fog) {
      const { def, batch } = this.fog;
      batch.material.uniforms.axisU.value.copy(RIGHT);
      batch.material.uniforms.axisV.value.copy(UP);
      this.stats.fog = fogCloudParticles(def, this.time, this.view, (x, z) => this.heights.at(x, z), (x, y, z, angle, alpha) =>
        batch.push((x - EYE.x) * FORWARD.x + (y - EYE.y) * FORWARD.y + (z - EYE.z) * FORWARD.z, x, y, z, def.size, angle, alpha));
    }
    for (const { def, instances, cloud, batch } of this.dust) {
      cloud.update(dt);
      const scale = def.heightScale;
      // The camera's axes in the objects' frame: the world's, the height
      // scaled. u runs down the square, v across, as the engine maps them.
      batch.material.uniforms.axisU.value.set(-UP.x, -UP.y * scale, -UP.z);
      batch.material.uniforms.axisV.value.set(RIGHT.x, RIGHT.y * scale, RIGHT.z);
      for (const at of instances) {
        const fade = dustFade(def, (at.x - EYE.x) * FORWARD.x + (at.y - EYE.y) * FORWARD.y + (at.z - EYE.z) * FORWARD.z);
        if (fade <= 0 || !FRUSTUM.intersectsSphere(SPHERE.set(at, def.radius))) continue;
        this.stats.objects++;
        for (const p of cloud.particles) {
          if (!(p[9] > 0)) continue;
          const x = at.x + p[0], y = at.y + p[1] * scale, z = at.z + p[2];
          batch.push((x - EYE.x) * FORWARD.x + (y - EYE.y) * FORWARD.y + (z - EYE.z) * FORWARD.z, x, y, z, p[6], 0, p[9] * fade);
          this.stats.dust++;
        }
      }
    }
    for (const batch of this.batches) batch.end();
  }
  dispose() { for (const batch of this.batches) batch.dispose(); this.batches.length = 0; }
}
