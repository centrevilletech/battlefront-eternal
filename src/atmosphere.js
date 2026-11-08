import {sceneProperty} from "./rust-render/scene-changes.js";
import {EnvironmentRecipe as RECIPE} from "./environment/layout.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { makeGround } from './collision.js';
import { loaded, AtmosphereHost } from './environment/module.js';
import { AtmosphereConfig as ATMOSPHERE, AtmosphereStats as STATS, DustConfig as DUST, SquareBatch as SQUARE, View as VIEW } from './environment/layout.js';
import { particleDepth } from './particle-depth.js';

// Cold recipes share the required environment module. Only authored JSON and
// numeric results cross; asset handles and scene objects stay in this host.
let content;
export function environmentRecipe(kind, data, owner = null) {
  const x = owner?.x || (content ??= new WebAssembly.Instance(loaded(), {
    surface_math: {surface_sin:Math.sin,surface_cos:Math.cos,surface_tan:Math.tan,surface_atan2:Math.atan2,surface_hypot:Math.hypot},
    env: {host_debris_cast:()=>0,host_debris_hit:()=>0,host_debris_low:()=>0,host_debris_event:()=>{},host_shore_tile:()=>0,host_shore_exposed:()=>0,host_cast:()=>0,host_cast_out:()=>0,host_ground:()=>NaN,host_cover:()=>-100000,host_impact:()=>-Infinity,host_impact_normal:()=>0},
  }).exports);
  const bytes = new TextEncoder().encode(JSON.stringify(data,(_,v)=>Object.is(v,-0)?'~negative zero~':v).replaceAll('"~negative zero~"','-0.0'));
  const at = x.content_input(bytes.length);
  new Uint8Array(x.memory.buffer,at,bytes.length).set(bytes);
  if (x.content_recipe(kind)) throw Error('Invalid environment content');
  owner?.refresh();
  return JSON.parse(new TextDecoder().decode(new Uint8Array(x.memory.buffer,x.content_output(),x.content_length())));
}

// The world's haze (map.json `atmosphere`, tools/zero_atmosphere.py), drawn
// as the 2004 engine draws it:
//
//   FogCloud  a band of big soft squares around the camera, 16 m or so over
//             the terrain, drifting with the wind and slowly turning
//             (Geonosis's blowing haze; Endor, the Citadel, both Yavins)
//   dust      dusteffect objects: clouds of squares blowing through a box,
//             flattened into low sheets (Geonosis, Hoth, Kamino, the
//             Citadel, the Dune Sea, both Yavins)
//
// Both are page-side presentation. Their state and the squares in view are a
// Rust phase's (rust/bfe-environment, atmosphere.rs): the page hands it the
// camera and the time, and each texture is one instanced draw of at most 4096
// squares, sorted back to front, whose records are views onto its memory.
// What is left here is the shell around them: the meshes, materials and
// uniforms.




// The square batches drawing into each scene (keyed by the scene at the
// root of theirs), for a renderer that draws them itself
// (src/rust-render/effects.js).
export const squareBatches = new WeakMap();

// The page's shell around one texture's squares: one instanced draw whose
// records are views onto what the Rust phase gathered and sorted this frame.
class SquareBatch {
  constructor(scene, texture, name, records, tint = [1, 1, 1, 1]) {
    // SquareBatch holds the phase's borrowed output and live camera facts.
    // Rust owns the quad, sampler binding, normal blend and material layout.
    this.nativeKind = EFFECT.SQUARE.ID; this.variant = 0;
    this.texture = texture; this.values = new Float64Array(EFFECT.SQUARE.LENGTH);
    this.values.set(tint, EFFECT.SQUARE.TINT);
    this.values[EFFECT.SQUARE.AXIS_U] = 1; this.values[EFFECT.SQUARE.AXIS_V + 1] = 1;
    this.arrays = { center: records.center, spin: records.spin };
    this.capacity = records.center.length / 4; this.count = 0;
    this.mesh = new THREE.Object3D();
    this.mesh.name = name;
    sceneProperty(this.mesh,"frustumCulled",false);
    sceneProperty(this.mesh,"visible",false);
    sceneProperty(this.mesh,"renderOrder",10);
    this.mesh.onBeforeRender = (renderer, s, camera) => particleDepth.capture(renderer, s, camera);
    scene.add(this.mesh);
    let root = scene;
    while (root.parent) root = root.parent;
    if (!squareBatches.has(root)) squareBatches.set(root, []);
    squareBatches.get(root).push(this);
  }
  // After the phase's frame: n squares, far to near, and the camera's axes
  // as this texture's squares are laid out.
  synced(n, result, at) {
    this.count = n;
    for (let i = 0; i < 3; i++) {
      this.values[EFFECT.SQUARE.AXIS_U + i] = result[at + SQUARE.AXIS_U + i];
      this.values[EFFECT.SQUARE.AXIS_V + i] = result[at + SQUARE.AXIS_V + i];
    }
    sceneProperty(this.mesh,"visible",n > 0);
  }
  dispose() { this.mesh.removeFromParent();  }
}

export class Atmosphere {
  constructor({ data, scene, collision, texture, surfaceOwner }) {
    this.batches = [];
    const fog = data?.fogCloud;
    const fogMap = fog && texture(fog.texture);
    const classes = [];
    const dust = data?.dust;
    const recipe = environmentRecipe(RECIPE.ATMOSPHERE, {fogCloud:fog,dust:{classes:Object.entries(dust?.classes||{}),instances:dust?.instances||[]}}, surfaceOwner);
    for (const source of recipe.classes) {
      const {name,instances,seed} = source, def = dust.classes[name], map = texture(def.texture);
      if (map) classes.push({def,map,name,instances,seed});
    }
    // The terrain's height under a grid corner, asked of the collision world
    // by the phase the first time it needs it.
    const ground = makeGround();
    this.host = new AtmosphereHost(surfaceOwner, classes.length, classes.reduce((n, c) => n + c.instances.length, 0), Boolean(fogMap),
      (x, z) => { const hit = collision?.ground(x, z, 100000, ground); return hit ? hit.height : NaN; });
    const config = this.host.config;
    if (fogMap) {
      config[ATMOSPHERE.HAS_FOG] = 1;
      config[ATMOSPHERE.NEAR] = fog.range[0]; config[ATMOSPHERE.FAR] = fog.range[1];
      config[ATMOSPHERE.CELL] = fog.cell || 256;
      config[ATMOSPHERE.SIZE] = fog.size; config[ATMOSPHERE.DENSITY] = fog.density;
      config[ATMOSPHERE.VELOCITY] = fog.velocity[0]; config[ATMOSPHERE.VELOCITY + 1] = fog.velocity[1];
      config[ATMOSPHERE.ROTATION] = fog.rotation;
      config[ATMOSPHERE.ALPHA] = recipe.alpha;
      config[ATMOSPHERE.HEIGHT] = fog.height;
    }
    config[ATMOSPHERE.CLASSES] = classes.length;
    let first = 0;
    classes.forEach(({ def, instances, seed }, k) => {
      const at = ATMOSPHERE.STRIDE + k * DUST.STRIDE;
      config[at + DUST.COUNT] = def.count;
      config.set(def.minPos, at + DUST.MIN_POS); config.set(def.maxPos, at + DUST.MAX_POS);
      config.set(def.minVel, at + DUST.MIN_VEL); config.set(def.maxVel, at + DUST.MAX_VEL);
      config[at + DUST.MIN_SIZE] = def.minSize; config[at + DUST.MAX_SIZE] = def.maxSize;
      config[at + DUST.MIN_LIFETIME] = def.minLifetime; config[at + DUST.MAX_LIFETIME] = def.maxLifetime;
      config[at + DUST.ALPHA] = def.alpha;
      config[at + DUST.FADE_MIN] = def.radiusFade[0]; config[at + DUST.FADE_MAX] = def.radiusFade[1];
      config[at + DUST.RADIUS] = def.radius;
      config[at + DUST.MIN_DISTANCE] = def.minDistance; config[at + DUST.MAX_DISTANCE] = def.maxDistance;
      config[at + DUST.HEIGHT_SCALE] = def.heightScale;
      config[at + DUST.SEED] = seed;
      config[at + DUST.INSTANCES] = instances.length;
      instances.forEach((position, i) => this.host.instances.set(position, (first + i) * 3));
      first += instances.length;
    });
    this.host.start(classes.length + Number(Boolean(fogMap)));
    const records = this.host.batches;
    if (fogMap) {
      this.batches.push(new SquareBatch(scene, fogMap, 'map-fog-cloud', records[0], recipe.tint));
    }
    classes.forEach(({ map, name }, k) => this.batches.push(new SquareBatch(scene, map, `map-dust-${name}`, records[k + Number(Boolean(fogMap))])));
    for(let k=0;k<this.batches.length;k++){this.host.bindArray(this.batches[k].arrays,"center",h=>h.batches[k].center);this.host.bindArray(this.batches[k].arrays,"spin",h=>h.batches[k].spin);}
    this.stats = { fog: 0, dust: 0, objects: 0 };
  }
  // The frame: the camera and the time go to the phase, which steps the
  // clouds and leaves each texture's squares in view, far to near.
  prepareFrame(dt, camera) {
    this.host.refresh();
    const view = this.host.view;
    view[VIEW.DT] = dt;
    view.set(camera.matrixWorld.elements, VIEW.CAMERA);
    view.set(camera.matrixWorldInverse.elements, VIEW.INVERSE);
    view.set(camera.projectionMatrix.elements, VIEW.PROJECTION);
    view[VIEW.FOV] = camera.fov ?? NaN;
    view[VIEW.ASPECT] = camera.aspect ?? NaN;
  }
  publishFrame() {
    this.host.refresh();
    const r = this.host.result;
    this.stats.fog = r[STATS.FOG]; this.stats.dust = r[STATS.DUST]; this.stats.objects = r[STATS.OBJECTS];
    for (let k = 0; k < this.batches.length; k++) {
      const at = STATS.STRIDE + k * SQUARE.STRIDE;
      this.batches[k].synced(r[at + SQUARE.COUNT], r, at);
    }
  }

  dispose() { for (const batch of this.batches) batch.dispose(); this.batches.length = 0; }
}
