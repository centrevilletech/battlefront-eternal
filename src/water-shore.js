import * as THREE from "three";
import { makeHit } from "./collision.js";

const TILE = 8;
const RANGE = 40;
const SPACING = 1.6;
const MAX_PATCHES = 384;
const DOWN = new THREE.Vector3(0, -1, 0);

// Intersect real collision triangles with the water plane. Grid-cell borders
// never contribute a shoreline; coplanar water/floor triangles contribute none.
export function waterlineSegment(triangle, height) {
  const vertices = [triangle.a, triangle.b, triangle.c];
  const points = [];
  for (let i = 0; i < 3; i++) {
    const a = vertices[i], b = vertices[(i + 1) % 3];
    const da = a.y - height, db = b.y - height;
    if (Math.abs(da) < 1e-6 && Math.abs(db) < 1e-6) continue;
    if (da * db > 0 || Math.abs(da - db) < 1e-8) continue;
    const t = da / (da - db);
    const point = new THREE.Vector3().lerpVectors(a, b, t);
    if (!points.some(p => p.distanceToSquared(point) < 1e-8)) points.push(point);
  }
  return points.length === 2 && points[0].distanceToSquared(points[1]) > 0.01 ? points : null;
}

function clipToTile(a, b, x, z) {
  let lo = 0, hi = 1;
  for (const [axis, min] of [["x", x], ["z", z]]) {
    const delta = b[axis] - a[axis];
    if (Math.abs(delta) < 1e-8) {
      if (a[axis] < min || a[axis] > min + TILE) return null;
      continue;
    }
    const t0 = (min - a[axis]) / delta, t1 = (min + TILE - a[axis]) / delta;
    lo = Math.max(lo, Math.min(t0, t1));
    hi = Math.min(hi, Math.max(t0, t1));
  }
  return hi > lo ? [a.clone().lerp(b, lo), a.clone().lerp(b, hi)] : null;
}

const VERTEX = /* glsl */ `
  attribute vec4 shore;
  attribute float phase;
  uniform float time;
  uniform float height;
  uniform float splash;
  varying vec2 vUv;
  varying float vPhase;
  varying float vPulse;
  varying vec3 vWorld;
  void main() {
    vUv = uv;
    vPhase = phase;
    vec2 normal = shore.zw;
    vec2 tangent = vec2(-normal.y, normal.x);
    float lap = 0.5 + 0.5 * sin(time * 1.35 + phase * 6.283);
    float event = fract(time * 0.115 + phase);
    vPulse = sin(clamp(event / 0.16, 0.0, 1.0) * 3.14159);
    float width = mix(0.28, 0.64, lap);
    vec2 at = shore.xy + tangent * (uv.x - 0.5) * 1.85;
    at += normal * (splash > 0.5 ? 0.06 + uv.y * 0.09 : 0.03 + uv.y * width);
    float y = height + 0.024 + (splash > 0.5 ? uv.y * (0.08 + 0.27 * vPulse) : 0.0);
    vec4 world = modelMatrix * vec4(at.x, y, at.y, 1.0);
    vWorld = world.xyz;
    gl_Position = projectionMatrix * viewMatrix * world;
  }
`;
const FRAGMENT = /* glsl */ `
  uniform float time;
  uniform float splash;
  varying vec2 vUv;
  varying float vPhase;
  varying float vPulse;
  varying vec3 vWorld;
  float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1,311.7))) * 43758.5453); }
  void main() {
    float endFade = smoothstep(0.0, 0.16, vUv.x) * (1.0 - smoothstep(0.84, 1.0, vUv.x));
    float ripple = sin(vUv.y * 18.0 - time * 1.7 + vPhase * 6.283 + sin(vUv.x * 15.0) * 0.5);
    float flecks = hash(floor(vUv * vec2(55.0, 18.0) + vec2(time * 0.35, 0.0)));
    float band = pow(max(ripple, 0.0), 3.0) * (0.3 + 0.7 * flecks);
    float edgeFade = smoothstep(0.0, 0.08, vUv.y) * (1.0 - smoothstep(0.45, 1.0, vUv.y));
    float alpha = band * edgeFade * endFade * 0.38;
    if (splash > 0.5) {
      float column = floor(vUv.x * 13.0);
      float center = 0.2 + hash(vec2(column, vPhase)) * 0.5;
      vec2 drop = vec2((fract(vUv.x * 13.0) - 0.5) * 3.2, (vUv.y - center) * 8.0);
      float droplets = 1.0 - smoothstep(0.35, 1.0, length(drop));
      alpha = droplets * endFade * vPulse * 0.28;
    }
    alpha *= 1.0 - smoothstep(28.0, 40.0, distance(cameraPosition.xz, vWorld.xz));
    if (alpha < 0.006) discard;
    gl_FragColor = vec4(0.66, 0.76, 0.73, alpha);
  }
`;

export class WaterShore {
  constructor({ water, collision, graphics }) {
    this.water = water;
    this.collision = collision;
    this.graphics = graphics;
    this.cache = new Map();
    this.queue = [];
    this.tile = "";
    this.count = 0;
    this.time = 0;
    this.origin = new THREE.Vector3();
    this.hit = makeHit();
    this.box = new THREE.Box3();
    this.group = new THREE.Group();
    this.group.name = "water-shore-lapping";
    water.group.add(this.group);
    const plane = new THREE.PlaneGeometry(1, 1);
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.index = plane.index;
    this.geometry.attributes.position = plane.attributes.position;
    this.geometry.attributes.uv = plane.attributes.uv;
    this.shore = new THREE.InstancedBufferAttribute(new Float32Array(MAX_PATCHES * 4), 4).setUsage(THREE.DynamicDrawUsage);
    this.phase = new THREE.InstancedBufferAttribute(new Float32Array(MAX_PATCHES), 1).setUsage(THREE.DynamicDrawUsage);
    this.geometry.setAttribute("shore", this.shore);
    this.geometry.setAttribute("phase", this.phase);
    this.geometry.instanceCount = 0;
    this.materials = [0, 1].map(splash => new THREE.ShaderMaterial({
      uniforms: { time: { value: 0 }, height: { value: water.height }, splash: { value: splash } },
      vertexShader: VERTEX, fragmentShader: FRAGMENT,
      transparent: true, depthTest: true, depthWrite: false, side: THREE.DoubleSide, forceSinglePass: true,
      toneMapped: false,
    }));
    this.materials.forEach(material => {
      const mesh = new THREE.Mesh(this.geometry, material);
      mesh.frustumCulled = false;
      mesh.renderOrder = -17;
      this.group.add(mesh);
    });
  }

  // A downward probe distinguishes the exposed water side of a wall from
  // streets/roofs covering the plane. No foam is placed across dry ground.
  exposed(x, z) {
    if (!this.water.over(x, z)) return false;
    this.origin.set(x, this.water.height + 12, z);
    const hit = this.collision.cast(this.origin, DOWN, 30, this.hit);
    return !hit || hit.point.y < this.water.height - 0.08;
  }

  sampleTile(tx, tz) {
    const x = tx * TILE, z = tz * TILE, height = this.water.height;
    const points = new Map();
    const take = triangle => {
      const raw = waterlineSegment(triangle, height);
      if (!raw) return;
      const segment = clipToTile(...raw, x, z);
      if (!segment) return;
      const [a, b] = segment;
      const length = a.distanceTo(b);
      const nx = -(b.z - a.z) / length, nz = (b.x - a.x) / length;
      const steps = Math.max(1, Math.ceil(length / SPACING));
      for (let i = 0; i < steps; i++) {
        const t = (i + 0.5) / steps;
        const px = THREE.MathUtils.lerp(a.x, b.x, t), pz = THREE.MathUtils.lerp(a.z, b.z, t);
        const plus = this.exposed(px + nx * 0.45, pz + nz * 0.45);
        const minus = this.exposed(px - nx * 0.45, pz - nz * 0.45);
        if (plus === minus) continue;
        const sign = plus ? 1 : -1;
        points.set(`${Math.round(px * 2)},${Math.round(pz * 2)}`, [px, pz, nx * sign, nz * sign]);
      }
    };
    const center = new THREE.Vector3(x + TILE / 2, height, z + TILE / 2);
    for (const polygon of this.collision.gather(center, TILE / 2)) for (const triangle of polygon.triangles) take(triangle);
    // Terrain is separate from the architecture contact list.
    this.box.min.set(x, height - 0.1, z);
    this.box.max.set(x + TILE, height + 0.1, z + TILE);
    this.collision.terrain?.shapecast({
      intersectsBounds: box => box.intersectsBox(this.box),
      intersectsTriangle: triangle => { take(triangle); return false; },
    });
    return [...points.values()];
  }

  update(dt, camera) {
    const quality = this.graphics.settings.get("waterQuality");
    this.group.visible = quality === "high" || quality === "ultra";
    if (!this.group.visible) return;
    this.time += dt;
    for (const material of this.materials) material.uniforms.time.value = this.time;
    const tx = Math.floor(camera.position.x / TILE), tz = Math.floor(camera.position.z / TILE);
    const key = `${tx},${tz}`;
    if (key !== this.tile) {
      this.tile = key;
      this.queue = [];
      for (const [cached, entry] of this.cache) if (Math.abs(entry.x - tx) > 6 || Math.abs(entry.z - tz) > 6) this.cache.delete(cached);
      for (let dz = -5; dz <= 5; dz++) for (let dx = -5; dx <= 5; dx++) {
        if (dx * dx + dz * dz > 26) continue;
        const x = tx + dx, z = tz + dz, k = `${x},${z}`;
        if (!this.cache.has(k)) this.queue.push({ x, z, key: k, distance: dx * dx + dz * dz });
      }
      this.queue.sort((a, b) => a.distance - b.distance);
    }
    // Two small BVH tiles per update; nearby results are reused while moving.
    let dirty = key !== this.drawTile;
    for (let i = 0; i < 2 && this.queue.length; i++) {
      const tile = this.queue.shift();
      this.cache.set(tile.key, { ...tile, points: this.sampleTile(tile.x, tile.z) });
      dirty = true;
    }
    if (!dirty && this.drawQuality === quality) return;
    this.drawTile = key;
    this.drawQuality = quality;
    const points = [...this.cache.values()].flatMap(entry => entry.points)
      .map(point => ({ point, distance: (point[0] - camera.position.x) ** 2 + (point[1] - camera.position.z) ** 2 }))
      .filter(p => p.distance < RANGE * RANGE).sort((a, b) => a.distance - b.distance);
    const limit = quality === "ultra" ? MAX_PATCHES : MAX_PATCHES / 2;
    this.count = Math.min(points.length, limit);
    for (let i = 0; i < this.count; i++) {
      const p = points[i].point;
      this.shore.setXYZW(i, ...p);
      this.phase.setX(i, ((Math.sin(p[0] * 12.9898 + p[1] * 78.233) * 43758.5453) % 1 + 1) % 1);
    }
    this.geometry.instanceCount = this.count;
    this.shore.needsUpdate = this.phase.needsUpdate = true;
  }

  dispose() {
    this.group.removeFromParent();
    this.geometry.dispose();
    for (const material of this.materials) material.dispose();
    this.cache.clear();
  }
}
