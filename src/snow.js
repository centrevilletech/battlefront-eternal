import {sceneProperty} from "./rust-render/scene-changes.js";
import {EnvironmentRecipe as RECIPE} from "./environment/layout.js";
import { environmentRecipe } from './atmosphere.js';
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { makeHit } from './collision.js';
import { particleDepth } from './particle-depth.js';
import { SnowHost } from './environment/module.js';
import { SnowResult as RESULT, View as VIEW } from './environment/layout.js';


// Snow: a world's Precipitation of Type("Quads") (Hoth), as the 2004 engine
// draws it. `ParticleDensity` flakes fill a 5 m box, and the box repeats over
// the world's 5 m grid out to `Range` in view: the flakes keep their place in
// the world as the camera moves through them. Each falls at `Velocity` with
// the wind, wandering up to `VelocityRange` along each axis; every half
// second its wander, its alpha (one end of AlphaMinMax or the other) and its
// spin (Rotation ± RotationRange / 2 turns a second) ease a fifth of the way
// toward new random ones. A flake is a camera-facing square reaching
// `ParticleSize` from its centre to each side, the texture tinted `Color`;
// boxes fade out over the last third of the range. Roofs stop it, as they
// stop the rain (weather.js's cover cache, standing in for the engine's
// rainshadow regions).
//
// The flakes (a few hundred), the wind, the cover cache and the choice of
// boxes in view are a Rust phase's (rust/bfe-environment, snow.rs); one
// instanced draw repeats the flakes over the boxes, nearest last. What is
// here is the shell around them: the mesh, the material and the textures
// the shader reads the phase's records from.







export class Snow {
  constructor({ data, scene, collision, water, texture, surfaceOwner }) {
    const p = data.precipitation;
    // The flakes of one box: how many, and how they fall and ease.
    const recipe = environmentRecipe(RECIPE.SNOW, data, surfaceOwner);
    const count = recipe.count, range = recipe.range, boxLimit = recipe.boxes;
    // A roof or floor over a cell: what a cast straight down first hits, or
    // open water over it if higher (the cover cache's query).
    const down = new THREE.Vector3(0, -1, 0), from = new THREE.Vector3(), hit = makeHit();
    const cover = (x, y, z) => {
      const found = collision.cast(from.set(x, y, z), down, 512, hit);
      let height = found ? found.point.y : -100000;
      if (water?.over(x, z)) height = Math.max(height, water.level);
      return height;
    };
    this.host = new SnowHost(surfaceOwner, cover);
    this.host.config.set(recipe.config);
    this.host.start();
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
    this.values=new Float64Array(EFFECT.SNOW.LENGTH);
    const v=this.values;
    v[EFFECT.SNOW.FLAKE_COUNT]=count;v[EFFECT.SNOW.SIZE]=recipe.size;
    v[EFFECT.SNOW.CAMERA_RIGHT]=1;v[EFFECT.SNOW.CAMERA_UP+1]=1;
    v.set(recipe.tint,EFFECT.SNOW.TINT);
    v[EFFECT.SNOW.COVER_CELL]=this.cover.cell;v[EFFECT.SNOW.COVER_SIZE]=size;
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
    const r=this.host.result,v=this.values;
    for(let i=0;i<3;i++){
      v[EFFECT.SNOW.CAMERA_RIGHT+i]=r[RESULT.CAMERA_RIGHT+i];
      v[EFFECT.SNOW.CAMERA_UP+i]=r[RESULT.CAMERA_UP+i];
    }
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
    sceneProperty(this.mesh,"visible",r[RESULT.COVERED] === 1 && boxes > 0);
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
