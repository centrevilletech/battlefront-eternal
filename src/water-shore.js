import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from "three";
import { makeHit } from "./collision.js";

import { Surface as LIMITS, SurfaceFrame as FRAME, SurfaceResult as RESULT, ShoreTriangle as TRIANGLE } from './environment/layout.js';

const TILE = 8;
const MAX_PATCHES = LIMITS.SHORE_PATCHES;
const DOWN = new THREE.Vector3(0, -1, 0);

export class WaterShore {
  constructor({ water, collision, graphics }) {
    this.water = water;
    this.collision = collision;
    this.graphics = graphics;
    this.host = water.host;
    this.count = 0;
    this.time = 0;
    this.origin = new THREE.Vector3();
    this.hit = makeHit();
    this.box = new THREE.Box3();
    this.terrainQuery = {
      intersectsBounds: box => box.intersectsBox(this.box),
      intersectsTriangle: triangle => { this.copyTriangle(triangle); return false; },
    };
    this.host.owner.attach({
      host_shore_tile: (x, z, height) => this.collect(x, z, height),
      host_shore_exposed: (x, z, height) => {
        if (!this.water.over(x, z)) return 0;
        this.origin.set(x, height + 12, z);
        const hit = this.collision.cast(this.origin, DOWN, 30, this.hit);
        return !hit || hit.point.y < height - 0.08 ? 1 : 0;
      },
    });
    this.group = new THREE.Group();
    this.group.name = "water-shore-lapping";
    water.group.add(this.group);
    const plane = new THREE.PlaneGeometry(1, 1);
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.index = plane.index;
    this.geometry.attributes.position = plane.attributes.position;
    this.geometry.attributes.uv = plane.attributes.uv;
    this.shore = this.host.bind(new THREE.InstancedBufferAttribute(this.host.shore, 4).setUsage(THREE.DynamicDrawUsage), "shore");
    this.phase = this.host.bind(new THREE.InstancedBufferAttribute(this.host.phases, 1).setUsage(THREE.DynamicDrawUsage), "phases");
    this.geometry.setAttribute("shore", this.shore);
    this.geometry.setAttribute("phase", this.phase);
    this.geometry.instanceCount = 0;
    this.materials = [0, 1].map(splash => ownedEffectMaterial('effects/water-shore', {
      uniforms: { time: { value: 0 }, height: { value: water.height }, splash: { value: splash } },
      transparent: true, depthTest: true, depthWrite: false, side: THREE.DoubleSide, forceSinglePass: true,
      toneMapped: false,
    }));
    this.meshes = this.materials.map(material => {
      const mesh = new THREE.Mesh(this.geometry, material);
      mesh.frustumCulled = false;
      mesh.renderOrder = -17;
      this.group.add(mesh);
      return mesh;
    });
    // The Rust renderer's water pass draws its meshes (src/rust-render/water.js).
    water.shore = this;
  }

  // CollisionWorld still owns the collision resource. This bridge copies the
  // native gather's triangles into one fixed tile block in their original
  // order, then the terrain shapecast's triangles. It samples no shore points.
  copyTriangle(triangle) {
    const at = this.triangleCount++ * TRIANGLE.STRIDE;
    if (at + TRIANGLE.STRIDE > this.host.triangles.length) return;
    const out = this.host.triangles;
    out[at + TRIANGLE.A] = triangle.a.x; out[at + TRIANGLE.A + 1] = triangle.a.y; out[at + TRIANGLE.A + 2] = triangle.a.z;
    out[at + TRIANGLE.B] = triangle.b.x; out[at + TRIANGLE.B + 1] = triangle.b.y; out[at + TRIANGLE.B + 2] = triangle.b.z;
    out[at + TRIANGLE.C] = triangle.c.x; out[at + TRIANGLE.C + 1] = triangle.c.y; out[at + TRIANGLE.C + 2] = triangle.c.z;
  }
  collect(tx, tz, height) {
    this.triangleCount = 0;
    const x = tx * TILE, z = tz * TILE;
    this.origin.set(x + TILE / 2, height, z + TILE / 2);
    const polygons = this.collision.gather(this.origin, TILE / 2);
    for (let p = 0; p < polygons.length; p++) {
      const triangles = polygons[p].triangles;
      for (let t = 0; t < triangles.length; t++) this.copyTriangle(triangles[t]);
    }
    this.box.min.set(x, height - 0.1, z); this.box.max.set(x + TILE, height + 0.1, z + TILE);
    this.collision.terrain?.shapecast(this.terrainQuery);
    return this.triangleCount;
  }
  prepareFrame() {
    const quality = this.graphics.settings.get('waterQuality');
    this.group.visible = quality === 'high' || quality === 'ultra';
    this.host.frame[FRAME.SHORE_QUALITY] = quality === 'ultra' ? 2 : quality === 'high' ? 1 : 0;
  }
  publishFrame() {
    this.host.refresh();
    if (!this.group.visible) return;
    const result = this.host.result;
    this.time = result[RESULT.SHORE_TIME];
    for (let i = 0; i < this.materials.length; i++) this.materials[i].uniforms.time.value = this.time;
    if (!result[RESULT.SHORE_DIRTY]) return;
    this.count = result[RESULT.SHORE_COUNT]; this.geometry.instanceCount = this.count;
    this.shore.needsUpdate = this.phase.needsUpdate = true;
  }

  dispose() {
    if (this.water.shore === this) this.water.shore = null;
    this.group.removeFromParent();
    this.geometry.dispose();
    for (const material of this.materials) material.dispose();
  }
}
