import {sceneProperty} from "./rust-render/scene-changes.js";
import { EFFECT } from './rust-render/effect-layout.js';
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
    // WaterShore's quad, sampler-free material and instance layout live in
    // effect_resources.rs. Only native shore points and live matrices cross.
    this.batches=[0,1].map(splash=>{
      const batch={nativeKind:EFFECT.WATER_SHORE.ID,family:'waterShore',variant:0,texture:null,count:0,capacity:MAX_PATCHES,
        visibility:'anchor',arrays:{shore:this.host.shore,phase:this.host.phases},values:new Float64Array(EFFECT.WATER_SHORE.LENGTH),serial:0};
      this.host.bindArray(batch.arrays,'shore','shore');this.host.bindArray(batch.arrays,'phase','phases');
      batch.values[EFFECT.WATER_SHORE.HEIGHT]=water.height;batch.values[EFFECT.WATER_SHORE.SPLASH]=splash;
      const mesh=batch.mesh=new THREE.Object3D();sceneProperty(mesh,"renderOrder",-17);sceneProperty(mesh,"frustumCulled",false);mesh.userData.effectBatch=batch;
      mesh.onBeforeRender=(renderer,scene,camera)=>{
        const v=batch.values;v.set(mesh.matrixWorld.elements,EFFECT.WATER_SHORE.MODEL_MATRIX);
        v.set(camera.projectionMatrix.elements,EFFECT.WATER_SHORE.PROJECTION_MATRIX);v.set(camera.matrixWorldInverse.elements,EFFECT.WATER_SHORE.VIEW_MATRIX);
        const e=camera.matrixWorld.elements;v[EFFECT.WATER_SHORE.CAMERA_POSITION]=e[12];v[EFFECT.WATER_SHORE.CAMERA_POSITION+1]=e[13];v[EFFECT.WATER_SHORE.CAMERA_POSITION+2]=e[14];
      };
      this.group.add(mesh);return batch;
    });
    this.meshes=this.batches.map(batch=>batch.mesh);
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
    sceneProperty(this.group,"visible",quality === 'high' || quality === 'ultra');
    this.host.frame[FRAME.SHORE_QUALITY] = quality === 'ultra' ? 2 : quality === 'high' ? 1 : 0;
  }
  publishFrame() {
    this.host.refresh();
    if (!this.group.visible) return;
    const result = this.host.result;
    this.time = result[RESULT.SHORE_TIME];
    for(const batch of this.batches)batch.values[EFFECT.WATER_SHORE.TIME]=this.time;
    if (!result[RESULT.SHORE_DIRTY]) return;
    this.count = Math.min(result[RESULT.SHORE_COUNT],MAX_PATCHES);
    for(const batch of this.batches){batch.count=this.count;batch.serial++;}
  }

  dispose() {
    if (this.water.shore === this) this.water.shore = null;
    this.group.removeFromParent();
  }
}
