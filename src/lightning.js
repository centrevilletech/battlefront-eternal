import * as THREE from "three";
import { ArcLightning } from "./arc-lightning.js";

// LightningBolt effects (the arc caster's), drawn the way the 2004 renderer
// draws them.
//
// A bolt lives FadeTime seconds. Its shape is two random shapes blended,
// sliding toward the second at InterpolationSpeed per second and drawing a
// fresh one each time it gets there. It is split every BreakDistance into at
// least 3 segments (more than 100 and it isn't drawn); each point inside is
// pushed off the straight line by up to SpreadFactor / 2 on each axis (less
// any push along the bolt), and the half-width falls evenly from Width at the
// start to almost nothing at the end; both ends are clear. At each inside
// point, with BranchFactor odds and up to MaxBranches, a branch BranchLength
// long leaves along the point's push plus the bolt's direction x
// BranchSpreadFactor. Around the main bolt, max(NumChildren x charge, 1)
// children with their own Child settings redraw from fresh shapes every
// frame. The texture repeats every TextureSize metres along it.
//
// The charge scales it: the width by 0.3 + 0.7 x charge, and the alpha by
// (0.5 + 0.5 x charge), times a fade of min(2 x life / FadeTime, 1). Each
// segment is a ribbon facing the camera, drawn additively.
//
// Its texture is a narrow bright band across the ribbon with clear edges,
// stored with no smaller levels (common.lvl: arccaster_lightning has one),
// so the original samples the band at full size even where a ribbon turns
// edge-on. The port's KTX2 twin carries a full chain; its small levels
// smear the band over the whole ribbon, as wide pale sheets where a bolt
// is seen end-on (from the shooter's own eye). Only its top level is
// sampled.
//
// That is the Classic look (Video > Bolt Caster). Enhanced, the default,
// draws the same bolts as lit, animated lightning that shocks the soldiers
// it strikes (arc-lightning.js); either way the bolts' lives and holds are
// kept here.

const MAX_SEGMENTS = 100;
const CAPACITY = 16384; // vertices

// A small seeded random number source.
function random(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const newSeed = () => (Math.random() * 0xffffffff) >>> 0;

export class Lightning {
  // options: settings (arcCasterEffects picks the look), and what the
  // Enhanced look reads: lights, units, collision, effects (ArcLightning).
  constructor(scene, texture, options = {}) {
    this.texture = texture;
    this.scene = scene;
    this.settings = options.settings || null;
    this.options = options;
    this.enhanced = null;
    // Built now when it is on, so its programs are built while the battle
    // loads.
    if (this.isEnhanced()) this.arcs();
    this.bolts = [];
    this.geometry = new THREE.BufferGeometry();
    this.positions = new Float32Array(CAPACITY * 3);
    this.uvs = new Float32Array(CAPACITY * 2);
    this.colors = new Float32Array(CAPACITY * 4);
    this.indices = new Uint32Array(CAPACITY * 3);
    this.geometry.setAttribute("position", new THREE.BufferAttribute(this.positions, 3).setUsage(THREE.DynamicDrawUsage));
    this.geometry.setAttribute("uv", new THREE.BufferAttribute(this.uvs, 2).setUsage(THREE.DynamicDrawUsage));
    this.geometry.setAttribute("color", new THREE.BufferAttribute(this.colors, 4).setUsage(THREE.DynamicDrawUsage));
    this.geometry.setIndex(new THREE.BufferAttribute(this.indices, 1).setUsage(THREE.DynamicDrawUsage));
    this.materials = new Map();
    this.mesh = new THREE.Mesh(this.geometry, null);
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = 5;
    this.mesh.visible = false;
    scene.add(this.mesh);
    this.vertex = 0;
    this.index = 0;
    this.a = new THREE.Vector3();
    this.b = new THREE.Vector3();
    this.dir = new THREE.Vector3();
    this.side = new THREE.Vector3();
    this.toEye = new THREE.Vector3();
    this.points = [];
    this.widths = [];
    this.offsets = [];
    for (let i = 0; i <= MAX_SEGMENTS + 1; i++) {
      this.points.push(new THREE.Vector3());
      this.offsets.push(new THREE.Vector3());
    }
    this.offsetB = new THREE.Vector3();
  }

  material(name) {
    if (!this.materials.has(name)) {
      this.materials.set(
        name,
        new THREE.MeshBasicMaterial({
          map: this.texture(name),
          vertexColors: true,
          transparent: true,
          blending: THREE.AdditiveBlending,
          depthWrite: false,
          side: THREE.DoubleSide,
        }),
      );
    }
    return this.materials.get(name);
  }

  // A bolt from `start` to `end`. follow(out), when given, moves the start
  // each frame (a bolt held to the shooter's gun).
  spawn(def, start, end, charge, follow = null) {
    this.material(def.Texture);
    const bolt = { def, start: start.clone(), end: end.clone(), charge, follow, life: def.FadeTime, phase: 0, seedA: newSeed(), seedB: newSeed() };
    this.bolts.push(bolt);
    if (this.isEnhanced()) this.arcs().add(bolt);
  }

  // The bolts whose time is up left out, in place.
  prune() {
    const bolts = this.bolts;
    let kept = 0;
    for (let i = 0; i < bolts.length; i++) if (bolts[i].life > 0) bolts[kept++] = bolts[i];
    bolts.length = kept;
  }

  // Whether the Enhanced look is on (Video > Bolt Caster; on by default).
  isEnhanced() {
    return (this.settings?.get?.("arcCasterEffects") ?? "enhanced") !== "classic";
  }

  arcs() {
    return (this.enhanced ||= new ArcLightning(this.scene, this.options));
  }

  update(dt, camera) {
    for (const bolt of this.bolts) {
      const def = bolt.def;
      bolt.life -= dt;
      if (bolt.life <= 0) continue;
      bolt.phase += def.InterpolationSpeed * dt;
      while (bolt.phase >= 1) {
        bolt.phase -= 1;
        bolt.seedA = bolt.seedB;
        bolt.seedB = newSeed();
      }
      if (bolt.follow) bolt.follow(bolt.start);
    }
    if (this.isEnhanced()) {
      this.mesh.visible = false;
      this.arcs().update(dt, camera);
      this.prune();
      return;
    }
    this.enhanced?.clear();
    this.vertex = 0;
    this.index = 0;
    let texture = null;
    for (const bolt of this.bolts) {
      const def = bolt.def;
      if (bolt.life <= 0) continue;
      texture ||= def.Texture;
      const fade = Math.min((2 * bolt.life) / def.FadeTime, 1);
      const width = 0.3 + 0.7 * bolt.charge;
      const alpha = fade * (0.5 + 0.5 * bolt.charge);
      const [r, g, b, a] = def.Color.map((c) => c / 255);
      this.strand(camera, bolt.start, bolt.end, {
        halfWidth: def.Width * width,
        spread: def.SpreadFactor,
        breakDistance: def.BreakDistance,
        textureSize: def.TextureSize,
        color: [r, g, b, a * alpha],
        seedA: bolt.seedA,
        seedB: bolt.seedB,
        phase: bolt.phase,
        branches: { odds: def.BranchFactor, most: def.MaxBranches, length: def.BranchLength, spread: def.BranchSpreadFactor },
      });
      const children = Math.max(Math.floor(def.NumChildren * bolt.charge), 1);
      const [cr, cg, cb, ca] = def.ChildColor.map((c) => c / 255);
      for (let i = 0; i < children; i++) {
        const seed = newSeed();
        this.strand(camera, bolt.start, bolt.end, {
          halfWidth: def.ChildWidth * width,
          spread: def.ChildSpreadFactor,
          breakDistance: def.ChildBreakDistance,
          textureSize: def.ChildTextureSize,
          color: [cr, cg, cb, ca * alpha],
          seedA: seed,
          seedB: seed,
          phase: 0,
          branches: null,
        });
      }
    }
    this.prune();
    this.mesh.visible = this.index > 0;
    if (!this.index) return;
    this.mesh.material = this.material(texture);
    // The top level only, once the data has arrived (a KTX2 twin's sets its
    // own filters when it does).
    const map = this.mesh.material.map;
    if (map && map.minFilter !== THREE.LinearFilter && (map.image || map.mipmaps?.length)) {
      map.minFilter = THREE.LinearFilter;
      map.generateMipmaps = false;
      map.needsUpdate = true;
    }
    this.geometry.setDrawRange(0, this.index);
    for (const name of ["position", "uv", "color"]) this.geometry.attributes[name].needsUpdate = true;
    this.geometry.index.needsUpdate = true;
  }

  // One strand from `from` to `to`, and its branches.
  strand(camera, from, to, o) {
    const length = from.distanceTo(to);
    const n = Math.max(Math.floor(length / Math.max(o.breakDistance, 1e-3)), 3);
    if (n > MAX_SEGMENTS) return;
    const dir = this.dir.subVectors(to, from).divideScalar(Math.max(length, 1e-6));
    const randA = random(o.seedA);
    const randB = random(o.seedB);
    const points = this.points;
    const widths = this.widths;
    for (let i = 0; i <= n; i++) {
      const p = points[i].lerpVectors(from, to, i / n);
      const offset = this.offsets[i].set(0, 0, 0);
      if (i > 0 && i < n) {
        offset.set(randA() - 0.5, randA() - 0.5, randA() - 0.5).multiplyScalar(o.spread);
        this.offsetB.set(randB() - 0.5, randB() - 0.5, randB() - 0.5).multiplyScalar(o.spread);
        offset.lerp(this.offsetB, o.phase);
        offset.addScaledVector(dir, -offset.dot(dir));
        p.add(offset);
      }
      widths[i] = i === 0 ? o.halfWidth : Math.max(o.halfWidth * (1 - (i - 1) / (n + 1)), (2 * o.halfWidth) / (n + 1));
    }
    this.ribbon(camera, n, o);
    if (!o.branches) return;
    // Branches leave from inside points (they don't branch again).
    const branchRand = random(o.seedA ^ 0x9e3779b9);
    let made = 0;
    const spots = [];
    for (let i = 1; i < n && made < o.branches.most; i++) {
      if (branchRand() >= o.branches.odds) continue;
      const way = this.offsets[i].clone().addScaledVector(dir, o.branches.spread).normalize();
      spots.push([points[i].clone(), way, widths[i]]);
      made++;
    }
    spots.forEach(([start, way, halfWidth], k) => {
      this.strand(camera, start, start.clone().addScaledVector(way, o.branches.length), {
        ...o,
        halfWidth,
        seedA: (o.seedA + k + 1) >>> 0,
        seedB: (o.seedB + k + 1) >>> 0,
        branches: null,
      });
    });
  }

  // The points as a ribbon across the line to the camera.
  ribbon(camera, n, o) {
    if (this.vertex + (n + 1) * 2 > CAPACITY || this.index + n * 6 > CAPACITY * 3) return;
    const points = this.points;
    const base = this.vertex;
    let u = 0;
    for (let i = 0; i <= n; i++) {
      const p = points[i];
      const prev = points[Math.max(i - 1, 0)];
      const next = points[Math.min(i + 1, n)];
      this.toEye.subVectors(camera.position, p);
      this.side.subVectors(next, prev).cross(this.toEye).normalize().multiplyScalar(this.widths[i]);
      if (i > 0) u += p.distanceTo(prev) / Math.max(o.textureSize, 1e-3);
      const alpha = i === 0 || i === n ? 0 : o.color[3];
      for (const [sign, v] of [[1, 0], [-1, 1]]) {
        const k = this.vertex++;
        this.positions[k * 3] = p.x + this.side.x * sign;
        this.positions[k * 3 + 1] = p.y + this.side.y * sign;
        this.positions[k * 3 + 2] = p.z + this.side.z * sign;
        this.uvs[k * 2] = u;
        this.uvs[k * 2 + 1] = v;
        this.colors[k * 4] = o.color[0];
        this.colors[k * 4 + 1] = o.color[1];
        this.colors[k * 4 + 2] = o.color[2];
        this.colors[k * 4 + 3] = alpha;
      }
    }
    for (let i = 0; i < n; i++) {
      const a = base + i * 2;
      this.indices[this.index++] = a;
      this.indices[this.index++] = a + 1;
      this.indices[this.index++] = a + 2;
      this.indices[this.index++] = a + 1;
      this.indices[this.index++] = a + 3;
      this.indices[this.index++] = a + 2;
    }
  }
}
