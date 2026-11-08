import {sceneProperty} from "./rust-render/scene-changes.js";
import {admitEnvironment,drawValues} from './environment/module.js';
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from "three";
import { makeHit } from "./collision.js";

import { Surface as LIMITS, SurfaceConfig as LIMIT_CONFIG, SurfaceResult as RESULT, ShoreTriangle as TRIANGLE, ShoreQuery as QUERY,EnvironmentDraw as DRAW,EnvironmentAdmission as ADMISSION } from './environment/layout.js';

const MAX_PATCHES = LIMITS.SHORE_PATCHES;
const DOWN = new THREE.Vector3(0, -1, 0);

export class WaterShore {
  constructor({ water, collision, graphics }) {
    this.water = water;
    this.collision = collision;
    this.graphics = graphics;
    this.host = water.host;this.host.config[LIMIT_CONFIG.HEIGHT]=water.height;
    this.count = 0;
    this.time = 0;
    this.origin = new THREE.Vector3();
    this.hit = makeHit();
    this.box = new THREE.Box3();
    this.queryBuffer=this.host.owner.x.memory.buffer;
    this.query=new Float64Array(this.queryBuffer,this.host.owner.x.surface_shore_query_ptr(),QUERY.STRIDE);
    this.terrainQuery = {
      intersectsBounds: box => box.intersectsBox(this.box),
      intersectsTriangle: triangle => { this.copyTriangle(triangle); return false; },
    };
    this.host.owner.attach({
      host_shore_tile: (x, z, height) => this.collect(x, z, height),
      host_water_over:(x,z)=>water.over(x,z)?1:0,
      host_water_level:()=>water.level,
      host_shore_exposed:(x,y,z,far)=>this.collision.cast(this.origin.set(x,y,z),DOWN,far,this.hit)?.point.y??-Infinity,
    });
    this.group = new THREE.Group();
    this.group.name = "water-shore-lapping";
    water.group.add(this.group);
    // WaterShore's quad, sampler-free material and instance layout live in
    // effect_resources.rs. Only native shore points and live matrices cross.
    this.batches=[0,1].map(splash=>{
      const batch={nativeKind:EFFECT.WATER_SHORE.ID,family:'waterShore',variant:0,texture:null,count:0,capacity:MAX_PATCHES,
        visibility:'anchor',arrays:{shore:this.host.shore,phase:this.host.phases},serial:0};
      this.host.bindArray(batch.arrays,'shore','shore');this.host.bindArray(batch.arrays,'phase','phases');
      drawValues(this.host,batch,DRAW.SHORE,EFFECT.WATER_SHORE,splash);
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
    water.shore = this;this.prepareFrame();
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
    const x=this.host.owner.x,buffer=x.memory.buffer;
    if(this.queryBuffer!==buffer){this.queryBuffer=buffer;this.query=new Float64Array(buffer,x.surface_shore_query_ptr(),QUERY.STRIDE);}
    const q=this.query;this.origin.fromArray(q,QUERY.CENTER);
    const polygons = this.collision.gather(this.origin, q[QUERY.RADIUS]);
    for (let p = 0; p < polygons.length; p++) {
      const triangles = polygons[p].triangles;
      for (let t = 0; t < triangles.length; t++) this.copyTriangle(triangles[t]);
    }
    this.box.min.fromArray(q,QUERY.BOUNDS);this.box.max.fromArray(q,QUERY.BOUNDS+3);
    this.collision.terrain?.shapecast(this.terrainQuery);
    return this.triangleCount;
  }
  prepareFrame() {
    const quality = this.graphics.settings.get('waterQuality');
    if(quality!==this.quality){this.quality=quality;const mode=admitEnvironment(ADMISSION.SHORE,{quality},this.host.owner);sceneProperty(this.group,'visible',mode.visible);}
  }
  publishFrame() {
    this.host.refresh();
    if (!this.group.visible) return;
    const result = this.host.result;
    this.time = result[RESULT.SHORE_TIME];
    if (!result[RESULT.SHORE_DIRTY]) return;
    this.count = result[RESULT.SHORE_COUNT];
    for(const batch of this.batches){batch.count=this.count;batch.serial++;}
  }

  dispose() {
    if (this.water.shore === this) this.water.shore = null;
    this.group.removeFromParent();
  }
}
