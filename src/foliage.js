import * as THREE from "three";
import { lighting } from "./materials.js";
import { foliageAtmosphere } from "./foliage-material.js";
import { FoliageGpuBatch } from "./foliage-gpu.js";

// Placed foliage, drawn the way the 2004 renderer draws it: grasspatch (a bed
// of cards, Theed's flowers) and leafpatch (leaves around a point or along a
// vine, Theed's creepers). The live GPU path faces/sways static instanced
// cards in the shader; the CPU path below is the visual reference. They are
// fullbright (texture x grey x tint, no sun or ambient), cut
// out at alpha 0.25, double-sided, and cast no shadows.
//
// Grass: NumParticles cards scattered over BoxSize, MinSize..MaxSize (0.9 x
// that tall, 1.1 x wide), each a grey from DarknessMin..DarknessMax and one
// of the four plants of a 2x2 atlas in turn. The cards lean their tops toward
// the camera (up 70% toward it), turn their widths a little each (ten fixed
// turns), and sway their tops by 8% of their width at their own rates. Placed
// beds are drawn to MaxDistance, sinking into the ground over the last 20 m.
//
// Leaves: laid out once per class by the class's Seed through the engine's
// own random generator, so every vine is the same shape: along a vine walk
// (Vine, VineLength, VineSpread), each leaf MinSize..MaxSize, smaller toward
// the tips, greyer on the side away from the sun. Each faces the camera, a
// little toward it, its width turned by one of ten fixed amounts, wiggling by
// WiggleAmount of its width; odd leaves are mirrored. Placed leaf patches keep
// their up axis and position (yaw is discarded, so upright vines keep
// their class's x-y or z-y growth plane) and are drawn to the near scene's end, fading over the last
// 40%.

const CUTOFF = 0.25;
const GRASS_FADE = 20; // m over which a bed sinks into the ground
const LEAF_FADE = 0.4; // share of the distance over which leaves fade
const GRASS_TURNS = [0, -0.05, 0.1, -0.15, 0.2, 0, 0.05, -0.1, 0.15, -0.2];
const LEAF_TURNS = [0, -1, 2, -3, 4, 0, 1, -2, 3, -4];
const UP = new THREE.Vector3(0, 1, 0);

// The engine's seeded random numbers (its leaf layout generator).
class Seeded {
  constructor(seed) {
    this.s = (seed ^ 0xc71c2830) >>> 0;
    this.next();
  }

  next() {
    const s1 = (Math.imul(this.s, 1664525) + 1013904223) >>> 0;
    const s2 = (Math.imul(s1, 1664525) + 1013904223) >>> 0;
    this.s = s2;
    const u = ((s2 & 0xffff0000) | (s1 >>> 16)) >>> 0;
    return (u >>> 9) / 8388608;
  }
}

// A grasspatch class's cards (the original scatters them with its global
// random numbers, so only their spread matches).
function buildGrass(def) {
  const count = Math.max(Math.floor(def.NumParticles ?? 50), 0);
  const [bx, bz] = Array.isArray(def.BoxSize) ? def.BoxSize : [-1, -1];
  const rMin = def.RadiusFadeMin ?? -1;
  const rMax = def.RadiusFadeMax ?? -1;
  const min = def.MinSize ?? 10;
  const max = def.MaxSize ?? 15;
  const dMin = def.DarknessMin ?? 1;
  const dMax = def.DarknessMax ?? 1;
  const cards = [];
  for (let i = 0; i < count; i++) {
    let x = 0;
    let z = 0;
    let c = 1;
    if (bx > 0 || bz > 0) {
      z = (Math.random() - 0.5) * bz;
      x = (Math.random() - 0.5) * bx;
    } else if (rMax > 0) {
      for (let tries = 0; tries < 50; tries++) {
        z = (2 * Math.random() - 1) * rMax;
        x = (2 * Math.random() - 1) * rMax;
        const d = Math.hypot(x, z);
        if (d >= rMax) continue;
        c = d < rMin ? 1 : 1 - (d - rMin) / (rMax - rMin);
        if (c > 0) break;
      }
    }
    const size = min + Math.random() * (max - min);
    const grey = dMin + Math.random() * (dMax - dMin);
    cards.push({
      x,
      y: (def.YOffset ?? 0) - 0.5 * min * (1 - c),
      z,
      height: 0.9 * size,
      width: 1.1 * size,
      grey,
      phase: 10 * Math.random(),
      part: i & 3,
      rate: (1 + 0.2 * (i % 5)) * 0.8,
    });
  }
  return cards;
}

// A leafpatch class's leaves, laid out from its Seed.
function buildLeaves(def) {
  const rand = new Seeded(def.Seed >>> 0);
  const r = () => rand.next();
  const min = def.MinSize ?? 10;
  const max = def.MaxSize ?? 15;
  const leaves = [];
  const emit = (p, size) => leaves.push({ p: p.clone(), size });
  if (Array.isArray(def.Vine)) {
    const [vx, vz] = def.Vine;
    const [lMin, lMax] = Array.isArray(def.VineLength) ? def.VineLength : [2, 4];
    const spread = def.VineSpread ?? 0.6;
    const vine = (start, direction, total) => {
      const p = start.clone();
      const d = direction.clone();
      let n = total - 1;
      if (n < 0) return;
      let s = (min + r() * (max - min)) * Math.min((n + 1) / 4, 1);
      emit(p, s);
      const m = Math.min(Math.floor(lMin + r() * (lMax - lMin)), n);
      for (let k = 0; k < m; k++) {
        const a = (r() - r()) / 2;
        const b = (r() - r()) / 2;
        d.add(new THREE.Vector3(b * vx, 0.2, a * vz)).normalize();
        p.addScaledVector(d, s * spread);
        s = (min + r() * (max - min)) * Math.min((n + 1) / 4, 1);
        n -= 1;
        emit(p, s);
      }
      if (n > 0) {
        let a = r() - r();
        let b = r() - r();
        const d1 = d.clone().lerp(new THREE.Vector3(b * vx, 0, a * vz), 0.8).normalize();
        const n1 = Math.floor((0.5 * r() + 0.25) * n);
        vine(p.clone().addScaledVector(d1, s * spread), d1, n1);
        a = r() - r();
        b = r() - r();
        const d2 = d.clone().lerp(new THREE.Vector3(b * vx, 0, a * vz), 0.8).normalize();
        vine(p.clone().addScaledVector(d2, s * spread), d2, n - n1);
      }
    };
    vine(new THREE.Vector3(), UP, Math.floor(def.NumParticles ?? 50));
  } else if (Array.isArray(def.BoxSize) && def.BoxSize.length === 3) {
    const [bx, by, bz] = def.BoxSize;
    for (let i = 0; i < Math.floor(def.NumParticles ?? 50); i++) {
      const z = (r() - 0.5) * bz, y = (r() - 0.5) * by, x = (r() - 0.5) * bx;
      emit(new THREE.Vector3(x, y, z), min + r() * (max - min));
    }
  } else {
    // A rounded heap Radius across and Height tall, coned in at the top.
    const radius = def.Radius ?? 5;
    const height = def.Height ?? 5;
    const scale = def.HeightScale ?? 1;
    const cone = def.ConeHeight ?? 0;
    let q = 0;
    for (let i = 0; i < Math.floor(def.NumParticles ?? 50); i++) {
      const p = new THREE.Vector3();
      for (let tries = 0; tries < 100; tries++) {
        p.set(r() * 1.5 * radius, r() * radius, r() * 1.5 * radius);
        p.set(p.z, p.y, p.x);
        if (q < 2) p.x = -p.x;
        if (q === 1 || q === 2) p.z = -p.z;
        q = (q + 1) & 3;
        if (p.length() <= radius) break;
      }
      p.y *= height / (radius * scale);
      if (cone > 0 && p.y > height - cone) {
        const f = (height - p.y) / cone;
        p.x *= f;
        p.z *= f;
      }
      emit(p, min + r() * (max - min));
    }
  }
  // Grey: darker on the side away from the sun; a phase for the wiggle.
  const dMin = def.DarknessMin ?? 1;
  const span = (def.DarknessMax ?? 1) - dMin;
  const sun = lighting.sunDirection.value;
  const [ox, oy, oz] = Array.isArray(def.Offset) ? def.Offset : [0, 0, 0];
  const toward = new THREE.Vector3();
  return leaves.map((leaf, i) => {
    const base = dMin + (r() * span) / 2;
    toward.copy(leaf.p);
    if (toward.lengthSq() > 1e-8) toward.normalize();
    const grey = base - toward.dot(sun) * (span / 2);
    leaf.p.add(new THREE.Vector3(ox, oy / (def.HeightScale ?? 1), oz));
    return { ...leaf, grey, phase: 100 * r(), part: i & 3 };
  });
}

// One batch of camera-facing cards in one texture.
class Cards {
  constructor(scene, map, capacity) {
    this.capacity = capacity;
    this.positions = new Float32Array(capacity * 4 * 3);
    this.colors = new Float32Array(capacity * 4 * 4);
    this.uvs = new Float32Array(capacity * 4 * 2);
    const index = new Uint32Array(capacity * 6);
    for (let i = 0; i < capacity; i++) index.set([i * 4, i * 4 + 2, i * 4 + 1, i * 4 + 1, i * 4 + 2, i * 4 + 3], i * 6);
    this.geometry = new THREE.BufferGeometry();
    this.geometry.setAttribute("position", new THREE.BufferAttribute(this.positions, 3).setUsage(THREE.DynamicDrawUsage));
    this.geometry.setAttribute("color", new THREE.BufferAttribute(this.colors, 4).setUsage(THREE.DynamicDrawUsage));
    this.geometry.setAttribute("uv", new THREE.BufferAttribute(this.uvs, 2).setUsage(THREE.DynamicDrawUsage));
    this.geometry.setIndex(new THREE.BufferAttribute(index, 1));
    this.material = foliageAtmosphere(new THREE.MeshBasicMaterial({ map, vertexColors: true, alphaTest: CUTOFF, side: THREE.DoubleSide }));
    this.mesh = new THREE.Mesh(this.geometry, this.material);
    this.mesh.frustumCulled = false;
    this.mesh.castShadow = false;
    this.mesh.receiveShadow = false;
    scene.add(this.mesh);
    this.count = 0;
  }

  begin() {
    this.count = 0;
  }

  // Corners bottom-left, bottom-right, top-left, top-right; uv as
  // [u0, v0, u1, v1] with v0 at the top.
  push(bl, br, tl, tr, r, g, b, alpha, uv, mirror) {
    if (this.count >= this.capacity) return;
    const k = this.count++;
    const p = this.positions;
    const offset = k * 12;
    p[offset] = bl.x; p[offset + 1] = bl.y; p[offset + 2] = bl.z;
    p[offset + 3] = br.x; p[offset + 4] = br.y; p[offset + 5] = br.z;
    p[offset + 6] = tl.x; p[offset + 7] = tl.y; p[offset + 8] = tl.z;
    p[offset + 9] = tr.x; p[offset + 10] = tr.y; p[offset + 11] = tr.z;
    for (let j = k * 16; j < (k + 1) * 16; j += 4) {
      this.colors[j] = r; this.colors[j + 1] = g; this.colors[j + 2] = b; this.colors[j + 3] = alpha;
    }
    const [u0, v0, u1, v1] = uv;
    const left = mirror ? u1 : u0, right = mirror ? u0 : u1;
    const u = this.uvs, at = k * 8;
    u[at] = left; u[at + 1] = v1; u[at + 2] = right; u[at + 3] = v1;
    u[at + 4] = left; u[at + 5] = v0; u[at + 6] = right; u[at + 7] = v0;
  }

  end() {
    this.geometry.setDrawRange(0, this.count * 6);
    // Only the visible prefix was rebuilt. Avoid uploading culled card
    // capacity, and avoid per-card temporary arrays in push above.
    for (const attribute of Object.values(this.geometry.attributes)) {
      attribute.clearUpdateRanges();
      if (this.count > 0) {
        attribute.addUpdateRange(0, this.count * 4 * attribute.itemSize);
        attribute.needsUpdate = true;
      }
    }
    this.mesh.visible = this.count > 0;
  }
}

// The four plants of a 2x2 atlas, by card part.
const ATLAS = [
  [0, 0.5, 0.5, 1],
  [0.5, 0.5, 1, 1],
  [0, 0, 0.5, 0.5],
  [0.5, 0, 1, 0.5],
];
const WHOLE = [0, 0, 1, 1];

export class Foliage {
  // data: map.json's foliage; texture(name) a texture; nearEnd() the near
  // scene's far end (leaves are drawn that far).
  constructor(scene, data, { texture, nearEnd, gpu = false }) {
    this.gpu = gpu;
    this.nearEnd = nearEnd;
    this.kinds = new Map();
    this.time = 0;
    const counts = new Map();
    for (const inst of data.instances || []) counts.set(inst.class, (counts.get(inst.class) || 0) + 1);
    for (const [name, def] of Object.entries(data.classes || {})) {
      const grass = def.base === "grasspatch";
      const cards = grass ? buildGrass(def) : buildLeaves(def);
      const batch = gpu ? new FoliageGpuBatch(scene, texture(def.Texture), cards, def, grass, counts.get(name) || 0)
        : new Cards(scene, texture(def.Texture), Math.max(cards.length * (counts.get(name) || 0), 1));
      const bounds = new THREE.Box3();
      if (grass) for (const card of cards) {
        const center = new THREE.Vector3(card.x, card.y, card.z);
        const reach = card.height + card.width;
        bounds.expandByPoint(center.clone().addScalar(reach));
        bounds.expandByPoint(center.clone().addScalar(-reach));
      }
      if (!grass) for (const leaf of cards) {
        const center = leaf.p.clone();
        center.y *= def.HeightScale ?? 1;
        const size = leaf.size * Math.max(1, def.HeightScale ?? 1);
        bounds.expandByPoint(center.clone().addScalar(size));
        bounds.expandByPoint(center.clone().addScalar(-size));
      }
      // Every placement of a class uses the same cards and camera-facing
      // orientation. Build that local frame once per card, then transform it
      // by each patch's up axis and position.
      const frames = gpu ? null : cards.map(() => ({ width: new THREE.Vector3(), sway: 0,
        corners: grass ? null : Array.from({ length: 4 }, () => new THREE.Vector3()), center: new THREE.Vector3() }));
      this.kinds.set(name, { def, grass, cards, frames, batch, instances: [], bounds });
    }
    for (const inst of data.instances || []) {
      const kind = this.kinds.get(inst.class);
      if (!kind) continue;
      const m = new THREE.Matrix4().fromArray(inst.matrix);
      // Leaves get a tint per instance.
      const shade = Math.random();
      const position = new THREE.Vector3().setFromMatrixPosition(m);
      // Native patches retain their up axis and discard authored yaw.
      const axisY = new THREE.Vector3().setFromMatrixColumn(m, 1).normalize();
      const axisX = new THREE.Vector3(axisY.y, -axisY.x, 0).normalize();
      const axisZ = new THREE.Vector3().crossVectors(axisX, axisY).normalize();
      m.makeBasis(axisX, axisY, axisZ).setPosition(position);
      const sphere = kind.grass && !inst.terrain ? null : kind.bounds.getBoundingSphere(new THREE.Sphere());
      if (sphere) sphere.applyMatrix4(m);
      kind.instances.push({
        position,
        matrix: m,
        cullDistance: inst.terrain ? 1.45 * inst.distance + (sphere?.radius || 0) : Infinity,
        maxDistance: inst.terrain ? (kind.grass ? 0 : inst.distance) : (kind.grass ? kind.def.MaxDistance ?? 100 : 0),
        sphere,
        tint: inst.tint || (kind.grass ? [1, 1, 1] : [0, 1, 2].map(() => 0.7 + 0.2 * shade + 0.1 * Math.random())),
      });
    }
    this.v = Array.from({ length: 12 }, () => new THREE.Vector3());
    this.frustum = new THREE.Frustum();
    this.viewProjection = new THREE.Matrix4();
  }

  update(dt, camera) {
    this.time += dt;
    camera.updateMatrixWorld();
    this.frustum.setFromProjectionMatrix(this.viewProjection.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
    const e = camera.matrixWorld.elements;
    const right = this.v[0].set(e[0], e[1], e[2]);
    const up = this.v[1].set(e[4], e[5], e[6]);
    const back = this.v[2].set(e[8], e[9], e[10]); // toward the camera
    for (const kind of this.kinds.values()) {
      if (this.gpu) {
        kind.batch.update(kind.instances, this.time, camera, right, up, back, this.frustum, this.nearEnd());
        continue;
      }
      kind.batch.begin();
      if (kind.grass) this.drawGrass(kind, dt, camera, right, back);
      else this.drawLeaves(kind, dt, camera, right, up, back);
      kind.batch.end();
    }
  }

  drawGrass(kind, dt, camera, right, back) {
    const def = kind.def;
    const atlas = (def.NumParts ?? 4) !== 1;
    const alpha = def.Alpha ?? 0.5;
    for (let i = 0; i < kind.cards.length; i++) {
      const card = kind.cards[i], frame = kind.frames[i];
      card.phase += card.rate * dt;
      const k = GRASS_TURNS[i % 10];
      frame.width.copy(right).multiplyScalar(1.1 * (1 - Math.abs(k))).addScaledVector(back, 1.1 * k).multiplyScalar(card.width);
      frame.sway = 0.08 * Math.sin(card.phase) - 0.5;
    }
    // Up leans toward the camera; flat toward it too.
    const lean = this.v[3].set(0, 1, 0).addScaledVector(back, -0.7 * back.y).normalize();
    const bl = this.v[5], br = this.v[6], tl = this.v[7], tr = this.v[8], at = this.v[10];
    const camPos = camera.position;
    const view = this.v[2];
    for (const inst of kind.instances) {
      const maxDist = inst.maxDistance || this.nearEnd();
      if (Math.hypot(inst.position.x - camPos.x, inst.position.z - camPos.z) > inst.cullDistance) continue;
      if (inst.sphere && !this.frustum.intersectsSphere(inst.sphere)) continue;
      // Depth along the view.
      const depth = -at.subVectors(inst.position, camPos).dot(view);
      if (depth > maxDist || depth < -5) continue;
      const a = depth <= maxDist - GRASS_FADE ? 1 : (maxDist - depth) / GRASS_FADE;
      const sink = 1 - (1 - a) ** 2;
      for (let i = 0; i < kind.cards.length; i++) {
        const card = kind.cards[i], frame = kind.frames[i], wd = frame.width;
        const base = at.set(card.x, card.y * sink, card.z);
        bl.copy(base).addScaledVector(wd, -0.5);
        br.copy(bl).add(wd);
        tl.copy(base).addScaledVector(lean, card.height * sink).addScaledVector(wd, frame.sway);
        tr.copy(tl).add(wd);
        // Radial edge cards are clipped at the patch's base plane, including
        // their UVs. Transform the whole card through its authored up axis.
        const uv = [...(atlas ? ATLAS[card.part] : WHOLE)];
        if (card.y < 0) {
          const clip = Math.min(1, -card.y / card.width);
          bl.lerp(tl, clip); br.lerp(tr, clip);
          uv[3] += (uv[1] - uv[3]) * clip;
        }
        for (const corner of [bl, br, tl, tr]) corner.applyMatrix4(inst.matrix);
        const g = card.grey, tint = inst.tint;
        kind.batch.push(bl, br, tl, tr, g * tint[0], g * tint[1], g * tint[2], alpha,
          uv, !atlas && (card.part & 1) === 1);
      }
    }
  }

  drawLeaves(kind, dt, camera, right, up, back) {
    const def = kind.def;
    const alpha = def.Alpha ?? 0.5;
    const speed = def.WiggleSpeed ?? 1.2;
    const amount = def.WiggleAmount ?? 0.11;
    const bl = this.v[5], br = this.v[6], tl = this.v[7], tr = this.v[8], center = this.v[10], us = this.v[11];
    const camPos = camera.position;
    const scale = def.HeightScale ?? 1;
    const atlas = (def.NumParts ?? 4) === 4;
    for (let i = 0; i < kind.cards.length; i++) {
      const leaf = kind.cards[i], frame = kind.frames[i], ws = frame.width;
      leaf.phase += speed * dt;
      frame.center.set(leaf.p.x, leaf.p.y * scale, leaf.p.z).addScaledVector(back, 0.1);
      ws.copy(right).addScaledVector(back, 0.3 * LEAF_TURNS[i % 10]).normalize().multiplyScalar(leaf.size);
      us.copy(up).multiplyScalar(leaf.size * scale);
      const [lb, rb, lt, rt] = frame.corners;
      lt.copy(frame.center).addScaledVector(us, 0.5).addScaledVector(ws, amount * Math.sin(leaf.phase) - 0.5);
      rt.copy(lt).add(ws);
      lb.copy(frame.center).addScaledVector(us, -0.5).addScaledVector(ws, -0.5);
      rb.copy(lb).add(ws);
    }
    for (const inst of kind.instances) {
      const maxDist = inst.maxDistance || this.nearEnd();
      if (Math.hypot(inst.position.x - camPos.x, inst.position.z - camPos.z) > inst.cullDistance) continue;
      if (inst.sphere && !this.frustum.intersectsSphere(inst.sphere)) continue;
      const depth = -center.subVectors(inst.position, camPos).dot(back);
      if (depth > maxDist + 20 || depth < -25) continue;
      for (let i = 0; i < kind.cards.length; i++) {
        const leaf = kind.cards[i], frame = kind.frames[i];
        center.copy(frame.center).applyMatrix4(inst.matrix);
        const d = -this.v[3].subVectors(center, camPos).dot(back);
        if (d > maxDist) continue;
        const a = d <= (1 - LEAF_FADE) * maxDist ? 1 : (maxDist - d) / (LEAF_FADE * maxDist);
        const [lb, rb, lt, rt] = frame.corners;
        bl.copy(lb).applyMatrix4(inst.matrix); br.copy(rb).applyMatrix4(inst.matrix);
        tl.copy(lt).applyMatrix4(inst.matrix); tr.copy(rt).applyMatrix4(inst.matrix);
        const g = leaf.grey;
        kind.batch.push(bl, br, tl, tr, g * inst.tint[0], g * inst.tint[1], g * inst.tint[2], alpha * a, atlas ? ATLAS[leaf.part] : WHOLE, !atlas && (leaf.part & 1) === 1);
      }
    }
  }
}
