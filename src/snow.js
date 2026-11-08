import {sceneProperty} from "./rust-render/scene-changes.js";
import {EnvironmentDraw as DRAW} from "./environment/layout.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { makeHit } from './collision.js';
import { particleDepth } from './particle-depth.js';
import { SnowHost,drawValues } from './environment/module.js';
import { SnowResult as RESULT, View as VIEW } from './environment/layout.js';


// Native admission and the snow phase own density, cover and repeated boxes.
// This shell retains the actual texture, scene anchor and GPU image handles.
export class Snow {
  constructor({ data, scene, collision, water, texture, surfaceOwner }) {
    const p = data.precipitation;
    // A roof or floor over a cell: what a cast straight down first hits, or
    // open water over it if higher (the cover cache's query).
    const down = new THREE.Vector3(0, -1, 0), from = new THREE.Vector3(), hit = makeHit();
    surfaceOwner.attach({host_water_over:(x,z)=>water?.over(x,z)?1:0,host_water_level:()=>water?.level??0});
    const cover=(x,y,z)=>collision.cast(from.set(x,y,z),down,512,hit)?.point.y??-100000;
    this.host = new SnowHost(surfaceOwner, cover, data);
    const recipe=this.host.recipe,count=recipe.count,range=recipe.range,boxLimit=recipe.boxes;
    this.flakeCount = count;
    this.range = range;
    this.boxLimit = boxLimit;
    // The cache of cover heights the shader clips snow against.
    const size = recipe.coverSize;
    // Snow's persistent phase arrays supply texels directly to the native
    // image owner. Count and camera axes are typed facts, not uniforms.
    this.cover = { heights:this.host.heights,cell:this.host.cell,size,queries:0 };
    this.host.bindArray(this.cover,"heights",h=>h.heights);
    this.nativeKind=EFFECT.SNOW.ID;this.family='snow';this.variant=0;
    this.texture=texture(p.Texture?.[0]);this.count=0;this.capacity=0;this.arrays={};
    this.visibility='anchor';this.resources=null;this.samples=new Uint32Array(3);this.samples.fill(-1);
    drawValues(this.host,this,DRAW.SNOW,EFFECT.SNOW);
    this.mesh=new THREE.Object3D();this.mesh.userData.effectBatch=this;
    this.mesh.name = 'map-snow';
    sceneProperty(this.mesh,"frustumCulled",false);
    sceneProperty(this.mesh,"visible",false);
    sceneProperty(this.mesh,"renderOrder",20);
    this.mesh.onBeforeRender = (renderer, s, camera) => particleDepth.capture(renderer, s, camera);
    scene.add(this.mesh);
    this.stats = { boxes: 0, flakes: 0 };
  }
  // The frame: the camera and the time go to the phase, which moves the
  // flakes and the wind, refreshes the cover cache and picks the boxes in
  // view; what it changed is marked for upload.
  prepareFrame(dt, camera) {
    this.host.refresh();
    const view = this.host.view;
    view[VIEW.DT] = dt;
    view.set(camera.matrixWorld.elements, VIEW.CAMERA);
    view.set(camera.matrixWorldInverse.elements, VIEW.INVERSE);
    view.set(camera.projectionMatrix.elements, VIEW.PROJECTION);
    view[VIEW.FOV] = camera.fov ?? NaN;
    view[VIEW.ASPECT] = camera.aspect ?? NaN;
    view[VIEW.POSITION] = camera.position.x; view[VIEW.POSITION + 1] = camera.position.y; view[VIEW.POSITION + 2] = camera.position.z;
  }
  publishFrame() {
    this.host.refresh();
    const r=this.host.result;
    if(this.resources){
      if(this.imageBuffer!==this.host.buffer){
        const gpu=this.resources.gpu;
        gpu.effect_pixels(this.samples[0],this.host.flakes);gpu.effect_pixels(this.samples[1],this.host.boxes);gpu.effect_pixels(this.samples[2],this.host.heights);
        this.imageBuffer=this.host.buffer;
      }
      this.resources.gpu.snow_pixels(this.samples[0],this.samples[1],this.samples[2],r[RESULT.CHANGED]!==0);
    }
    this.cover.queries=r[RESULT.QUERIES];
    const boxes=r[RESULT.BOXES];this.count=r[RESULT.INSTANCES];
    sceneProperty(this.mesh,"visible",Boolean(r[RESULT.VISIBLE]));
    this.stats.boxes = boxes; this.stats.flakes = r[RESULT.INSTANCES];
  }
  // The native descriptor fixes format/type/filter and derives dimensions
  // from flake and box counts; no Texture or Source object is constructed.
  prepareResources(resources) {
    this.host.refresh();this.resources=resources;this.imageBuffer=this.host.buffer;const gpu=resources.gpu;
    this.samples[0]=gpu.effect_data(1,this.flakeCount,this.host.flakes);
    this.samples[1]=gpu.effect_data(2,this.boxLimit,this.host.boxes);
    this.samples[2]=gpu.effect_data(3,this.cover.size,this.host.heights);
    return this.samples;
  }
  dispose() {
    this.mesh.removeFromParent();
    if(this.resources)for(const sample of this.samples)this.resources.gpu.dispose_sample(sample);
  }
}
