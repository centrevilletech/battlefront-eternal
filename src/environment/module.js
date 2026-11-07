// The destruction and weather phase's WebAssembly module (rust/bfe-environment),
// required: compiled once while the battle loads and instantiated for each
// owner. A failure is reported to the player; nothing stands in for it.
import { ENVIRONMENT_WASM } from "../environment-wasm.js";
import { Dst, DstBurst, DstChunk, DstChunkAt, DstCommand, DstFrame, DstMatrixAt, DstSlotState, DstSlotStatic, DstStation, DstStationState, EnvironmentFrame, SkyFrame, Sky, WeatherFrame, WeatherResult, SnowConfig, SnowResult, Squares, View, WakeFrame, WakeActor, WakeResult, Wakes } from "./layout.js";

let compiling = null;
let compiled = null;

export function loadEnvironment() {
  compiling ||= (async () => {
    const text = atob(ENVIRONMENT_WASM);
    const bytes = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
    try { return (compiled = await WebAssembly.compile(bytes)); }
    catch (error) { throw new Error(`environment: the required Rust module could not load (${error.message}); reload the game`); }
  })();
  return compiling;
}

// The compiled module, for an owner built where loading cannot wait.
export function loaded() {
  if (!compiled) throw new Error("environment: the required Rust module is not loaded (await loadEnvironment() first)");
  return compiled;
}

// The host functions the module imports (what it asks of the page's
// collision world); an owner that does not use one is given the empty answer.
const imports = (host = {}) => ({ surface_math: { surface_sin: Math.sin, surface_cos: Math.cos, surface_atan2: Math.atan2, surface_hypot: Math.hypot }, env: { host_shore_tile: () => 0, host_shore_exposed: () => 0, host_cast: () => 0, host_cast_out: () => 0, host_ground: () => NaN, host_cover: () => -100000, host_impact: () => -Infinity, host_impact_normal: () => 0, ...host } });

// An instance for the destruction effects (rust/bfe-environment destruction.rs
// and fx.rs): the pools of hot and smoke particles, the arcs, the pieces of
// droids blown apart and the wrecks, with typed-array views onto its fixed
// blocks. `cast(ox, oy, oz, dx, dy, dz, far)` is the page's collision world:
// it returns 1 for a hit and leaves the point and the normal in `out`.
// Memory never grows after construction, so the views stay valid.
export class DestructionHost {
  constructor(module, hot, smoke, arcs, seed, cast) {
    this.out = new Float64Array(6);
    const { exports: x } = new WebAssembly.Instance(module, imports({
      host_cast: (ox, oy, oz, dx, dy, dz, far) => cast(ox, oy, oz, dx, dy, dz, far, this.out),
      host_cast_out: (i) => this.out[i],
    }));
    this.x = x;
    x.env_new(hot, smoke, arcs, seed >>> 0);
    this.hotCapacity = hot;
    this.smokeCapacity = smoke;
    this.arcCapacity = arcs;
    const buffer = x.memory.buffer;
    const f64 = (ptr, n) => new Float64Array(buffer, ptr, n);
    this.frame = f64(x.dst_frame_ptr(), DstFrame.STRIDE);
    this.commands = f64(x.dst_commands_ptr(), Dst.COMMANDS * DstCommand.STRIDE);
    this.chunks = f64(x.dst_chunks_ptr(), DstChunkAt.LEN * DstChunk.STRIDE);
    this.matrices = f64(x.dst_matrices_ptr(), DstMatrixAt.LEN * 16);
    this.stationConfig = f64(x.dst_station_config_ptr(), Dst.STATIONS * DstStation.STRIDE);
    this.stationState = f64(x.dst_station_state_ptr(), Dst.STATIONS * DstStationState.STRIDE);
    this.slotStatic = f64(x.dst_slot_static_ptr(), Dst.SLOTS * DstSlotStatic.STRIDE);
    this.slotState = f64(x.dst_slot_state_ptr(), Dst.SLOTS * DstSlotState.STRIDE);
    this.slotMatrices = f64(x.dst_slot_matrices_ptr(), Dst.SLOTS * Dst.BONES * 16);
    this.burst = f64(x.dst_burst_ptr(), Dst.SLOTS * DstBurst.STRIDE);
    this.points = f64(x.dst_points_ptr(), Dst.SHAPE_POINTS * 3);
    this.result = f64(x.dst_result_ptr(), x.dst_result_len());
    this.fxResult = f64(x.env_fx_result_ptr(), x.env_fx_result_len());
    this.hot = Array.from({ length: 4 }, (_, a) => new Float32Array(buffer, x.env_pool_ptr(0, a), hot * 4));
    this.smoke = Array.from({ length: 5 }, (_, a) => new Float32Array(buffer, x.env_pool_ptr(1, a), smoke * 4));
    const vertices = x.env_arc_vertices();
    this.positions = new Float32Array(buffer, x.env_arc_ptr(0), vertices * 3);
    this.tangents = new Float32Array(buffer, x.env_arc_ptr(1), vertices * 3);
    this.meta = new Float32Array(buffer, x.env_arc_ptr(2), vertices * 4);
    this.indices = new Uint16Array(buffer, x.env_arc_indices_ptr(), x.env_arc_indices_len());
  }

  // A model's shape from the geometry (pieceShape): centre, half size, reach,
  // inverse inertia and extreme points.
  shape(handle, { center, half, points, radius, inverseInertia }) {
    points.forEach((p, i) => { this.points[i * 3] = p.x; this.points[i * 3 + 1] = p.y; this.points[i * 3 + 2] = p.z; });
    this.x.dst_shape(handle, center.x, center.y, center.z, half.x, half.y, half.z, radius, inverseInertia.x, inverseInertia.y, inverseInertia.z, points.length);
  }

  watch(count) { this.x.dst_watch(count); }
  prepare() { this.x.dst_prepare(); }
  // Runs the commands written so far, without a frame.
  apply() { this.x.dst_apply(); }
  update() { this.x.dst_update(); }
}

// An instance for the map's haze (rust/bfe-environment atmosphere): blocks
// for its configuration and objects, the camera it is given each frame and
// the batches of squares it leaves. `ground(x, z)` answers the terrain's
// height under a point (NaN for none).
// Shared hosts borrow memory only until the next allocation. Producers bind
// their retained draw arrays here so a growth refresh also repairs aliases.
// A cold mode can replace one vector while every other borrowed row stays put.
function borrow(old,Type,buffer,offset,length) { return old&&old.buffer===buffer&&old.byteOffset===offset&&old.length===length?old:new Type(buffer,offset,length); }
class SharedHost {
  constructor(owner) { this.owner=owner;this.x=owner.x;this.buffer=null;this.bindings=[];owner.hosts.push(this); }
  changed(force) { const b=this.x.memory.buffer;if(!force&&b===this.buffer)return false;this.buffer=b;return true; }
  bindArray(target,key,get) { this.refresh();target[key]=get(this);this.bindings.push({target,key,get}); }
  publishBindings() { for(let i=0;i<this.bindings.length;i++){const b=this.bindings[i];b.target[b.key]=b.get(this);} }
}

export class AtmosphereHost extends SharedHost {
  constructor(owner,classes,instances,fog,ground) {
    super(owner);owner.attach({host_ground:ground});this.x.atm_new(classes,instances,+!!fog);this.batches=[];this.batchCount=0;this.refresh();
  }
  refresh(force=false) {
    if(!this.changed(force))return false;const x=this.x,b=this.buffer;
    this.config=borrow(this.config,Float64Array,b,x.atm_config_ptr(),x.atm_config_len());
    this.instances=borrow(this.instances,Float64Array,b,x.atm_instances_ptr(),x.atm_instances_len());
    this.view=borrow(this.view,Float64Array,b,x.atm_view_ptr(),View.STRIDE);this.result=borrow(this.result,Float64Array,b,x.atm_result_ptr(),x.atm_result_len());
    for(let i=0;i<this.batchCount;i++){const row=this.batches[i]??={};row.center=borrow(row.center,Float32Array,b,x.atm_batch_ptr(i,0),Squares.CAPACITY*4);row.spin=borrow(row.spin,Float32Array,b,x.atm_batch_ptr(i,1),Squares.CAPACITY*2);}
    this.publishBindings();return true;
  }
  // Building haze can move its vectors without growing the memory buffer.
  start(batches) { this.x.atm_start();this.batchCount=batches;this.refresh(true); }
}

export class SnowHost extends SharedHost {
  constructor(owner,cast) { super(owner);owner.attach({host_cover:cast});this.x.snow_new();this.started=false;this.refresh(); }
  refresh(force=false) {
    if(!this.changed(force))return false;const x=this.x,b=this.buffer;
    this.config=borrow(this.config,Float64Array,b,x.snow_config_ptr(),SnowConfig.STRIDE);
    if(this.started){
      this.view=borrow(this.view,Float64Array,b,x.snow_view_ptr(),View.STRIDE);this.result=borrow(this.result,Float64Array,b,x.snow_result_ptr(),SnowResult.STRIDE);
      this.flakes=borrow(this.flakes,Float32Array,b,x.snow_data_ptr(0),x.snow_data_len(0));this.boxes=borrow(this.boxes,Float32Array,b,x.snow_data_ptr(1),x.snow_data_len(1));this.heights=borrow(this.heights,Float32Array,b,x.snow_data_ptr(2),x.snow_data_len(2));this.cell=x.snow_cover_cell();
    }
    this.publishBindings();return true;
  }
  start() { this.x.snow_start();this.started=true;this.refresh(true); }
}

export class SkyHost extends SharedHost {
  constructor(owner,classes) { super(owner);this.classes=classes;this.classArrays=[];this.started=false;this.x.sky_new(classes);this.refresh(); }
  refresh(force=false) {
    if(!this.changed(force))return false;const x=this.x,b=this.buffer;
    this.config=borrow(this.config,Float64Array,b,x.sky_config_ptr(),x.sky_config_len());
    if(this.started){
      this.frame=borrow(this.frame,Float64Array,b,x.sky_frame_ptr(),SkyFrame.STRIDE);this.result=borrow(this.result,Float64Array,b,x.sky_result_ptr(),x.sky_result_len());
      for(let i=0;i<this.classArrays.length;i++){const row=this.classArrays[i];if(row)for(let w=0;w<Sky.ARRAYS;w++)row[w]=borrow(row[w],Float32Array,b,x.sky_array_ptr(i,w),x.sky_array_len(i,w));}
    }
    this.publishBindings();return true;
  }
  start() { this.x.sky_start();this.started=true;this.refresh(true); }
  arrays(class_) {
    this.refresh();return this.classArrays[class_]??=Array.from({length:Sky.ARRAYS},(_,w)=>new Float32Array(this.buffer,this.x.sky_array_ptr(class_,w),this.x.sky_array_len(class_,w)));
  }
}

export class WeatherHost extends SharedHost {
  constructor(owner,capacity,enhanced,range,size,cover,impact,normal) {
    super(owner);owner.attach({host_cover:cover,host_impact:impact,host_impact_normal:i=>normal[i]});this.x.weather_new(capacity,+!!enhanced,range,size);this.refresh();
  }
  refresh(force=false) {
    if(!this.changed(force))return false;const x=this.x,b=this.buffer;
    this.frame=borrow(this.frame,Float64Array,b,x.weather_frame_ptr(),WeatherFrame.STRIDE);this.result=borrow(this.result,Float64Array,b,x.weather_result_ptr(),WeatherResult.STRIDE);
    this.cover=borrow(this.cover,Float32Array,b,x.weather_data_ptr(0),x.weather_data_len(0));this.locations=borrow(this.locations,Float32Array,b,x.weather_data_ptr(1),x.weather_data_len(1));this.normals=borrow(this.normals,Float32Array,b,x.weather_data_ptr(2),x.weather_data_len(2));this.seeds=borrow(this.seeds,Float32Array,b,x.weather_data_ptr(3),x.weather_data_len(3));this.mist=borrow(this.mist,Float32Array,b,x.weather_data_ptr(4),x.weather_data_len(4));
    this.streaks=borrow(this.streaks,Uint8Array,b,x.weather_streaks_ptr(),x.weather_streaks_len());this.cell=x.weather_cover_cell();this.publishBindings();return true;
  }
  // A mode change may replace cover storage inside an existing memory buffer.
  mode(enhanced,range,size) { this.x.weather_mode(+!!enhanced,range,size);this.refresh(true); }
}

export class VisibilityHost extends SharedHost {
  constructor(owner,count) { super(owner);this.count=count;this.started=false;this.x.vis_new(count);this.refresh(); }
  refresh(force=false) {
    if(!this.changed(force))return false;const x=this.x,b=this.buffer;
    this.config=borrow(this.config,Float64Array,b,x.vis_config_ptr(),x.vis_config_len());
    if(this.started){
      this.view=borrow(this.view,Float64Array,b,x.vis_view_ptr(),View.STRIDE);this.frame=borrow(this.frame,Float64Array,b,x.vis_frame_ptr(),EnvironmentFrame.STRIDE);this.result=borrow(this.result,Float64Array,b,x.vis_result_ptr(),x.vis_result_len());this.handles=borrow(this.handles,Float64Array,b,x.vis_handles_ptr(),this.count);
      this.wakeFrame=borrow(this.wakeFrame,Float64Array,b,x.vis_wake_frame_ptr(),WakeFrame.STRIDE);this.wakeActors=borrow(this.wakeActors,Float64Array,b,x.vis_wake_actors_ptr(),Wakes.ACTORS*WakeActor.STRIDE);this.wakeResult=borrow(this.wakeResult,Float64Array,b,x.vis_wake_result_ptr(),WakeResult.STRIDE);this.wakePoints=borrow(this.wakePoints,Float64Array,b,x.vis_wake_points_ptr(),Wakes.COUNT*4);this.wakeMotion=borrow(this.wakeMotion,Float64Array,b,x.vis_wake_motion_ptr(),Wakes.COUNT*4);
    }
    this.publishBindings();return true;
  }
  start() { this.x.vis_start();this.started=true;this.refresh(true); }
}

// All surface owners on a battle share this module and its retained state. Their
// constructors install collision queries before any frame can run. A shared
// owner advances every installed native system with one surface_frame call.
export class SurfaceOwner {
  constructor(module = loaded(), water = false) {
    this.callbacks = Object.create(null);this.hosts=[];
    const env = {
      host_ground: (x, z) => this.callbacks.host_ground(x, z),
      host_cover: (x, y, z) => this.callbacks.host_cover(x, y, z),
      host_impact: (x, y, z) => this.callbacks.host_impact(x, y, z),
      host_impact_normal: i => this.callbacks.host_impact_normal(i),
      host_shore_tile: (x, z, height) => this.callbacks.host_shore_tile(x, z, height),
      host_shore_exposed: (x, z, height) => this.callbacks.host_shore_exposed(x, z, height),
    };
    this.x = new WebAssembly.Instance(module, imports(env)).exports;
    this.x.surface_new(water ? 1 : 0);
  }
  attach(callbacks) { Object.assign(this.callbacks, callbacks); return this.x; }
  refresh() { const buffer=this.x.memory.buffer;if(this.buffer===buffer)return;this.buffer=buffer;for(let i=0;i<this.hosts.length;i++)this.hosts[i].refresh(); }
  update() { this.refresh();this.x.surface_frame();this.refresh(); }
}
