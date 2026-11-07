import {sceneProperty} from "./rust-render/scene-changes.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from "three";
import { particleDepth } from "./particle-depth.js";
import { Result as RESULT } from "./environment/layout.js";

// The particles and electric arcs of the Enhanced destruction effects
// (src/destruction.js): droids shorting out and blowing apart, and wrecked
// power-up droids smoking.
//
// Two fixed pools of instanced quads, each one draw:
//
//   hot    additive light: sparks (streaks along their flight that skip off
//          the floor), fire, flashes, the glow a blast throws on the floor,
//          and embers
//   smoke  alpha-blended: smoke lit by the map's sun and ambient and drifting
//          with the wind, flakes of dark plating, and scorch marks
//
// A particle is written once at its birth (where, when, how fast, how big)
// and the vertex shader works out where it is from its age: drag, gravity or
// buoyancy, expansion, the wind. The CPU only retires dead ones, so a frame
// uploads what was born or moved in the pool, nothing per live particle.
//
// Smoke and fire are drawn without textures: a ball eaten into by four
// octaves of noise, the noise's slope bending the ball's normal so the sun
// picks out billows. Fire is coloured by how dense and how young it is, from
// white-yellow through orange to a dull red.
//
// Arcs are jagged ribbons between two points, each end fixed in the world or
// carried by an object (a bone, a flying piece). Their shape is redrawn 30
// times a second with a new random jag, which is what makes them crackle.
// One dynamic buffer, one draw.

export const SPARK = 0;
export const FIRE = 1;
export const FLASH = 2;
export const GLOW = 3;
export const EMBER = 4;
export const SMOKE = 0;
export const CHIP = 1;
export const SCORCH = 2;

// The pools by their meshes, for a renderer that draws them itself
// (src/rust-render/effects.js): while a pool is `external`, three is shown
// it never and uploads it nothing.
export const destructionPools = new WeakMap();

// The page's shell around one of the Rust phase's pools: a mesh whose
// instanced attributes are views onto the records the phase keeps
// (rust/bfe-environment), so three can draw one until the renderer takes
// over. Births are appended, deaths swap in the last record, and the clock is
// the effects' own (fxTime, not the frame's time); all of that is Rust's.
class Pool {
  constructor(scene, arrays, { name, capacity, attributes, renderOrder }) {
    this.capacity = capacity;
    this.count = 0;
    this.external = false;
    this.emitted = 0;
    this.dropped = 0;
    // The first record changed since the renderer last read the pool.
    this.unread = Infinity;
    // Pool borrows destruction.rs's persistent planar streams. Native
    // descriptors own its plane and the hot/smoke material policy.
    const layout=name==='destruction-hot'?EFFECT.HOT:EFFECT.SMOKE;
    this.nativeKind=layout.ID;this.family='pool';this.variant=0;this.texture=null;
    this.values=new Float64Array(layout.LENGTH);this.arrays={};
    if(layout===EFFECT.SMOKE)this.values.set([.55,0,.32],EFFECT.SMOKE.WIND);
    for(let i=0;i<attributes;i++)this.arrays['a'+i]=arrays[i];
    this.mesh=new THREE.Object3D();this.mesh.userData.effectBatch=this;
    this.mesh.name = name;
    sceneProperty(this.mesh,"frustumCulled",false);
    sceneProperty(this.mesh,"renderOrder",renderOrder);
    sceneProperty(this.mesh,"visible",false);
    this.mesh.onBeforeRender = (renderer, scene, camera) => particleDepth.capture(renderer, scene, camera);
    destructionPools.set(this.mesh, this);
    scene.add(this.mesh);
  }

  // After the phase's frame: its counts, and what three must upload of the
  // records changed since the last frame (`dirty`: the first of them, -1 for
  // none).
  synced(count, dirty, emitted, dropped) {
    this.count = count;
    this.emitted = emitted;
    this.dropped = dropped;
    if (dirty >= 0 && dirty < this.unread) this.unread = dirty;
    sceneProperty(this.mesh,"visible",count > 0);
  }

  dispose() {
    this.mesh.removeFromParent();
  }
}

// The arcs by their mesh, for a renderer that draws them itself
// (src/rust-render/effects.js): while `external`, three is shown them never
// and uploads them nothing.
export const destructionArcs = new WeakMap();

// The page's shell around the phase's arcs: one dynamic mesh whose vertices
// and indices are views onto what the phase wrote.
class Arcs {
  constructor(scene, host) {
    this.capacity = host.arcCapacity;
    this.external = false;
    this.live = 0;
    this.spawned = 0;
    this.vertex = 0;
    this.index = 0;
    this.positions = host.positions;
    this.tangents = host.tangents;
    this.meta = host.meta;
    this.indices = host.indices;
    // Arcs publishes the native phase's live prefixes directly. Raster,
    // attribute and index descriptors are owned by effect_mesh.
    this.nativeKind=EFFECT.DESTRUCTION_ARCS.ID;this.values=new Float64Array(0);
    this.arrays={position:this.positions,tangent:this.tangents,meta:this.meta};
    this.mesh=new THREE.Object3D();this.mesh.userData.effectMesh=this;
    this.mesh.name = "destruction-arcs";
    sceneProperty(this.mesh,"frustumCulled",false);
    sceneProperty(this.mesh,"renderOrder",12);
    sceneProperty(this.mesh,"visible",false);
    destructionArcs.set(this.mesh, this);
    scene.add(this.mesh);
  }

  // After the phase's frame: how much of the mesh is drawn, and what three
  // must upload of it.
  synced(live, spawned, vertex, index) {
    this.live = live;
    this.spawned = spawned;
    this.vertex = vertex;
    this.index = index;
    sceneProperty(this.mesh,"visible",index>0);
  }

  dispose() {
    this.mesh.removeFromParent();
  }
}

// The pools and arcs of the destruction effects as meshes. Their births and
// arcs are made by the destruction phase (rust/bfe-environment); `sync` reads
// what it left after each of its frames.
export class DestructionFx {
  constructor(scene, host) {
    this.host = host;
    this.hot=new Pool(scene,host.hot,{name:'destruction-hot',capacity:host.hotCapacity,attributes:4,renderOrder:11});
    this.smoke=new Pool(scene,host.smoke,{name:'destruction-smoke',capacity:host.smokeCapacity,attributes:5,renderOrder:9});
    this.arcs = new Arcs(scene, host);
  }

  get time() {
    return this.host.fxResult[RESULT.TIME];
  }

  get draws() {
    return Number(this.hot.mesh.visible) + Number(this.smoke.mesh.visible) + Number(this.arcs.mesh.visible);
  }

  // After the phase's frame.
  sync() {
    const r = this.host.fxResult;
    this.hot.values[EFFECT.HOT.FX_TIME]=r[RESULT.TIME];this.smoke.values[EFFECT.SMOKE.FX_TIME]=r[RESULT.TIME];
    this.hot.synced(r[RESULT.HOT_COUNT], r[RESULT.HOT_DIRTY], r[RESULT.HOT_EMITTED], r[RESULT.HOT_DROPPED]);
    this.smoke.synced(r[RESULT.SMOKE_COUNT], r[RESULT.SMOKE_DIRTY], r[RESULT.SMOKE_EMITTED], r[RESULT.SMOKE_DROPPED]);
    this.arcs.synced(r[RESULT.ARCS_LIVE], r[RESULT.ARCS_SPAWNED], r[RESULT.ARC_VERTICES], r[RESULT.ARC_INDICES]);
  }

  dispose() {
    this.hot.dispose();
    this.smoke.dispose();
    this.arcs.dispose();
  }
}
