import * as THREE from "three";
import { texturePixels } from "./texture-pixels.js";

export const LOCAL_LIGHT_COUNT = 4;
export const localLighting = {
  positions: { value: Array.from({ length: LOCAL_LIGHT_COUNT }, () => new THREE.Vector4()) },
  colors: { value: Array.from({ length: LOCAL_LIGHT_COUNT }, () => new THREE.Vector3()) },
  // How many of the slots are lit: the shader stops there.
  count: { value: 0 },
  select: () => {},
};

// The 2004 maps place no lights for characters: lamps, ceiling panels, light
// strips and consoles were baked into the architecture's vertex colours and
// drawn as self-lit (unlit) texture regions. Reconstruct lights from those
// regions so a soldier brightens under a corridor panel and beside a lamp.
// Baked architecture keeps its own lighting.
//
// A self-lit material is examined once its texture loads: triangles whose
// texture is bright there are emitters, gathered into 2.5 m cells, one light
// per cell, placed a little in front of the glowing surface (so occlusion
// rays don't stop on the fixture itself), with a reach that grows with the
// glowing area so high ceilings still light the floor. Materials named as
// lamps or lights accept dimmer, coloured glass.
const CELL = 2.5;
const GRID = 8; // m per spatial-hash cell for selection
const SAMPLES = [[1 / 3, 1 / 3], [0.5, 0.5], [0, 0.5], [0.5, 0]];

export class LocalLights {
  constructor() {
    this.sources = [];
    this.grid = new Map();
    this.reach = 0;
    this.pending = [];
    this.cache = new WeakMap();
    this.eye = new THREE.Vector3();
    this.direction = new THREE.Vector3();
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    this.enabled = true;
    this.rays = 0;
    localLighting.select = (object, viewModel) => this.select(object, viewModel);
  }

  add(position, color, radius = 5, intensity = 0.65) {
    const source = { position: new THREE.Vector3().fromArray(position), color: new THREE.Vector3().fromArray(color).multiplyScalar(intensity), radius, strength: intensity };
    this.sources.push(source);
    const key = this.key(Math.floor(source.position.x / GRID), Math.floor(source.position.y / GRID), Math.floor(source.position.z / GRID));
    if (!this.grid.has(key)) this.grid.set(key, []);
    this.grid.get(key).push(source);
    this.reach = Math.max(this.reach, radius);
    return source;
  }

  // A number per cell (±4096 cells on each axis), so lookups allocate nothing.
  key(x, y, z) {
    return ((x + 4096) * 8192 + (y + 4096)) * 8192 + (z + 4096);
  }

  // `lamp`: a material named as a lamp or light, whose coloured glass counts
  // as glowing; otherwise only bright self-lit texels do.
  register(mesh, lamp = true) {
    if (!mesh.geometry.index || !mesh.geometry.attributes.uv) return;
    // Keep the triangles as built: shadow and view culling later replace a
    // world mesh's index with a compacted, per-view buffer.
    this.pending.push({ mesh, lamp, triangles: mesh.geometry.index.array.slice() });
  }

  begin(enabled) {
    this.enabled = enabled;
    this.rays = 0;
    this.stamp = (this.stamp || 0) + 1;
    this.now = performance.now() / 1000;
    // Read pixels only once after each source texture finishes loading.
    if (!this.pending.length) return;
    this.pending = this.pending.filter((item) => {
      const pixels = texturePixels(item.mesh.material.uniforms?.map?.value);
      if (!pixels) return true;
      this.emitters(item, pixels.data, pixels.width, pixels.height);
      return false;
    });
  }

  emitters({ mesh, lamp, triangles }, data, width, height) {
    const { position, uv, normal } = mesh.geometry.attributes;
    mesh.updateMatrixWorld();
    const matrix = mesh.matrixWorld, cells = new Map();
    const a = new THREE.Vector3(), b = new THREE.Vector3(), c = new THREE.Vector3(), n = new THREE.Vector3(), e1 = new THREE.Vector3(), e2 = new THREE.Vector3();
    const texel = (u, v) => {
      const x = Math.min(width - 1, Math.floor(THREE.MathUtils.euclideanModulo(u, 1) * width));
      const y = Math.min(height - 1, Math.floor(THREE.MathUtils.euclideanModulo(v, 1) * height));
      return (y * width + x) * 4;
    };
    for (let t = 0; t + 2 < triangles.length; t += 3) {
      const i0 = triangles[t], i1 = triangles[t + 1], i2 = triangles[t + 2];
      let bright = 0, r = 0, g = 0, bl = 0;
      for (const [s0, s1] of SAMPLES) {
        const s2 = 1 - s0 - s1;
        const at = texel(uv.getX(i0) * s2 + uv.getX(i1) * s0 + uv.getX(i2) * s1, uv.getY(i0) * s2 + uv.getY(i1) * s0 + uv.getY(i2) * s1);
        const peak = Math.max(data[at], data[at + 1], data[at + 2]);
        const luma = 0.3 * data[at] + 0.59 * data[at + 1] + 0.11 * data[at + 2];
        if (data[at + 3] < (lamp ? 32 : 128) || (lamp ? peak < 64 : luma < 185)) continue;
        bright++; r += data[at]; g += data[at + 1]; bl += data[at + 2];
      }
      if (bright < 2) continue;
      a.fromBufferAttribute(position, i0).applyMatrix4(matrix);
      b.fromBufferAttribute(position, i1).applyMatrix4(matrix);
      c.fromBufferAttribute(position, i2).applyMatrix4(matrix);
      n.crossVectors(e1.subVectors(b, a), e2.subVectors(c, a));
      const area = n.length() / 2;
      if (area < 1e-5) continue;
      // Face the way the surface's own normals do; winding isn't reliable.
      if (normal) {
        e1.fromBufferAttribute(normal, i0).add(e2.fromBufferAttribute(normal, i1)).add(e2.fromBufferAttribute(normal, i2)).transformDirection(matrix);
        if (n.dot(e1) < 0) n.negate();
      }
      n.normalize();
      const cx = (a.x + b.x + c.x) / 3, cy = (a.y + b.y + c.y) / 3, cz = (a.z + b.z + c.z) / 3;
      const key = this.key(Math.floor(cx / CELL), Math.floor(cy / CELL), Math.floor(cz / CELL));
      let cell = cells.get(key);
      if (!cell) cells.set(key, (cell = { area: 0, at: new THREE.Vector3(), facing: new THREE.Vector3(), rgb: [0, 0, 0] }));
      cell.area += area;
      cell.at.x += cx * area; cell.at.y += cy * area; cell.at.z += cz * area;
      cell.facing.addScaledVector(n, area);
      const w = area / bright / 255;
      cell.rgb[0] += r * w; cell.rgb[1] += g * w; cell.rgb[2] += bl * w;
    }
    for (const cell of cells.values()) {
      if (cell.area < (lamp ? 0.005 : 0.04)) continue;
      const center = cell.at.divideScalar(cell.area);
      const facing = cell.facing.lengthSq() > 1e-8 ? cell.facing.normalize() : cell.facing.set(0, 0, 0);
      center.addScaledVector(facing, 0.3);
      // Several disconnected glass panes can belong to the same fixture.
      if (this.sources.some((source) => source.position.distanceToSquared(center) < 1)) continue;
      const size = Math.sqrt(cell.area);
      const radius = lamp ? THREE.MathUtils.clamp(5 + 3 * size, 6, 9) : THREE.MathUtils.clamp(5 + 3 * size, 6, 12);
      // Ceiling panels light the room below; a lit window on a facade
      // throws much less onto the platform outside it.
      const aim = lamp || facing.y < -0.5 ? 1 : facing.y > 0.5 ? 0.8 : 0.55;
      const intensity = (lamp ? 0.75 : THREE.MathUtils.clamp(0.45 + 0.2 * size, 0.5, 0.8)) * aim;
      this.add(center.toArray(), cell.rgb.map((v) => v / cell.area), radius, intensity).from = mesh.name;
    }
  }

  select(object, viewModel = false) {
    const colors = localLighting.colors.value, positions = localLighting.positions.value;
    for (let i = 0; i < colors.length; i++) colors[i].set(0, 0, 0);
    localLighting.count.value = 0;
    if (!this.enabled || !this.sources.length) return;
    this.eye.setFromMatrixPosition(object.matrixWorld);
    // Body origins sit at the feet; view models are already at the eye.
    if (object.isSkinnedMesh && !viewModel) this.eye.y += 0.9;
    let cache = this.cache.get(object);
    if (!cache) { cache = { at: -Infinity, stamp: -1, eye: new THREE.Vector3(), lights: [], candidates: [], ranks: [], distances: [] }; this.cache.set(object, cache); }
    // Every material group of a body, in every pass, asks again: the
    // first call in a frame decides.
    if (cache.stamp !== this.stamp && (this.now - cache.at >= 0.1 || cache.eye.distanceToSquared(this.eye) > .25)) {
      cache.at = this.now; cache.eye.copy(this.eye);
      // Insertion-select the four strongest (strength x falloff) without
      // allocating a record for every lamp on every character refresh.
      const { candidates, ranks, distances } = cache;
      candidates.length = ranks.length = distances.length = 0;
      const span = Math.ceil(this.reach / GRID);
      const gx = Math.floor(this.eye.x / GRID), gy = Math.floor(this.eye.y / GRID), gz = Math.floor(this.eye.z / GRID);
      for (let x = gx - span; x <= gx + span; x++) for (let y = gy - span; y <= gy + span; y++) for (let z = gz - span; z <= gz + span; z++) {
        const list = this.grid.get(this.key(x, y, z));
        if (!list) continue;
        for (const source of list) {
          const distance = source.position.distanceTo(this.eye);
          if (distance >= source.radius) continue;
          const falloff = 1 - (distance / source.radius) ** 2;
          const rank = -source.strength * falloff * falloff;
          let at = candidates.length;
          while (at > 0 && rank < ranks[at - 1]) at--;
          if (at >= LOCAL_LIGHT_COUNT) continue;
          const end = Math.min(candidates.length, LOCAL_LIGHT_COUNT - 1);
          for (let i = end; i > at; i--) {
            candidates[i] = candidates[i - 1];
            ranks[i] = ranks[i - 1];
            distances[i] = distances[i - 1];
          }
          candidates[at] = source; ranks[at] = rank; distances[at] = distance;
        }
      }
      cache.lights.length = 0;
      for (let i = 0; i < candidates.length; i++) {
        const source = candidates[i], distance = distances[i];
        if (this.collision && distance >= .4) {
          this.direction.subVectors(source.position, this.eye).normalize();
          this.rays++;
          if (this.collision.cast(this.eye, this.direction, Math.max(0, distance - .35), this.hit)) continue;
        }
        cache.lights.push(source);
      }
    }
    cache.stamp = this.stamp;
    localLighting.count.value = cache.lights.length;
    for (let i = 0; i < cache.lights.length; i++) {
      const source = cache.lights[i];
      positions[i].set(source.position.x, source.position.y, source.position.z, source.radius);
      colors[i].copy(source.color);
    }
  }
}
