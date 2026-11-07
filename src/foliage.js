import {sceneTransform} from "./rust-render/scene-changes.js";
import * as THREE from "three";
import { lighting } from "./materials.js";
import { SurfaceHost } from "./environment/surfaces.js";
import { SurfaceResult as RESULT } from "./environment/layout.js";

// Placed foliage, drawn the way the 2004 renderer draws it: grasspatch (a bed
// of cards, Theed's flowers) and leafpatch (leaves around a point or along a
// vine, Theed's creepers). Rust selects the patches and draws static
// instanced cards with source-owned shaders. They are
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
export class Foliage {
  // data: map.json's foliage; texture(name) a texture; nearEnd() the near
  // scene's far end (leaves are drawn that far).
  constructor(scene, data, { texture, nearEnd, drawer, surfaceOwner }) {
    this.nearEnd = nearEnd;
    this.surfaceHost = new SurfaceHost(surfaceOwner);
    this.kinds = new Map();
    this.time = 0;
    for (const [name, def] of Object.entries(data.classes || {})) {
      const grass = def.base === "grasspatch";
      const cards = grass ? buildGrass(def) : buildLeaves(def);
      const map = texture(def.Texture);
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
      this.kinds.set(name, { def, grass, cards, map, instances: [], bounds });
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
      if (sphere) sceneTransform(sphere.applyMatrix4(m));
      kind.instances.push({
        position,
        matrix: m,
        cullDistance: inst.terrain ? 1.45 * inst.distance + (sphere?.radius || 0) : Infinity,
        maxDistance: inst.terrain ? (kind.grass ? 0 : inst.distance) : (kind.grass ? kind.def.MaxDistance ?? 100 : 0),
        sphere,
        tint: inst.tint || (kind.grass ? [1, 1, 1] : [0, 1, 2].map(() => 0.7 + 0.2 * shade + 0.1 * Math.random())),
      });
    }
    // The Rust near phase installs the drawer during loading preparation.
    this.drawer = drawer;
    scene.userData.foliage = this;
  }

  // Foliage.update's wind clock is native. Placement selection was already
  // Rust-owned in the retained renderer; its resource input still reads time.
  publishFrame(result, camera) {
    this.time = result[RESULT.FOLIAGE_TIME];
    this.drawer.select(this, camera);
  }


}
