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
export class AtmosphereHost {
  constructor(module, classes, instances, fog, ground, owner = null) {
    const x = owner ? owner.attach({ host_ground: ground }) : new WebAssembly.Instance(module, imports({ host_ground: ground })).exports;
    this.x = x;
    x.atm_new(classes, instances, fog ? 1 : 0);
    const buffer = x.memory.buffer;
    this.config = new Float64Array(buffer, x.atm_config_ptr(), x.atm_config_len());
    this.instances = new Float64Array(buffer, x.atm_instances_ptr(), x.atm_instances_len());
    this.view = new Float64Array(buffer, x.atm_view_ptr(), View.STRIDE);
    this.result = new Float64Array(buffer, x.atm_result_ptr(), x.atm_result_len());
    this.buffer = buffer;
  }

  // After the configuration is written: builds the haze, then the squares'
  // records can be viewed.
  // Building the haze allocates (and so may grow the module's memory, which
  // detaches the views taken before); the views are taken again after it.
  start(batches) {
    this.x.atm_start();
    const x = this.x, buffer = x.memory.buffer;
    this.buffer = buffer;
    this.view = new Float64Array(buffer, x.atm_view_ptr(), View.STRIDE);
    this.result = new Float64Array(buffer, x.atm_result_ptr(), x.atm_result_len());
    this.batches = Array.from({ length: batches }, (_, b) => ({
      center: new Float32Array(buffer, x.atm_batch_ptr(b, 0), Squares.CAPACITY * 4),
      spin: new Float32Array(buffer, x.atm_batch_ptr(b, 1), Squares.CAPACITY * 2),
    }));
  }

  update() {
    this.x.atm_update();
  }
}

// An instance for the snow (rust/bfe-environment snow.rs): the flakes, the
// wind and the boxes in view, with the cover cache's heights. `cast(x, y, z)`
// answers a cover cell (the height of the first roof or floor below the
// point, or of open water over it).
export class SnowHost {
  constructor(module, cast, owner = null) {
    const x = owner ? owner.attach({ host_cover: cast }) : new WebAssembly.Instance(module, imports({ host_cover: cast })).exports;
    this.x = x;
    x.snow_new();
    this.config = new Float64Array(x.memory.buffer, x.snow_config_ptr(), SnowConfig.STRIDE);
  }

  // After the configuration is written. Building the snow allocates, so the
  // views are taken after it.
  start() {
    const x = this.x;
    x.snow_start();
    const buffer = x.memory.buffer;
    this.view = new Float64Array(buffer, x.snow_view_ptr(), View.STRIDE);
    this.result = new Float64Array(buffer, x.snow_result_ptr(), SnowResult.STRIDE);
    this.flakes = new Float32Array(buffer, x.snow_data_ptr(0), x.snow_data_len(0));
    this.boxes = new Float32Array(buffer, x.snow_data_ptr(1), x.snow_data_len(1));
    this.heights = new Float32Array(buffer, x.snow_data_ptr(2), x.snow_data_len(2));
    this.cell = x.snow_cover_cell();
  }

  update() {
    this.x.snow_update();
  }
}

// An instance for the sky's flyers (rust/bfe-environment sky.rs): the
// configuration is written, then `start` builds the ships.
export class SkyHost {
  constructor(module, classes, owner = null) {
    const x = owner ? owner.x : new WebAssembly.Instance(module, imports()).exports;
    this.x = x;
    x.sky_new(classes);
    this.classes = classes;
    this.config = new Float64Array(x.memory.buffer, x.sky_config_ptr(), x.sky_config_len());
  }

  // Building the ships allocates, so the views are taken after it.
  start() {
    const x = this.x, buffer = x.memory.buffer;
    x.sky_start();
    this.buffer = x.memory.buffer;
    this.frame = new Float64Array(this.buffer, x.sky_frame_ptr(), SkyFrame.STRIDE);
    this.result = new Float64Array(this.buffer, x.sky_result_ptr(), x.sky_result_len());
  }

  // A class's seven records (solid matrices and looks, fading ones, motion's
  // matrices, previous matrices and shown flags) as typed arrays.
  arrays(class_) {
    const x = this.x;
    return Array.from({ length: Sky.ARRAYS }, (_, w) => new Float32Array(this.buffer, x.sky_array_ptr(class_, w), x.sky_array_len(class_, w)));
  }
}

// An instance for the rain (rust/bfe-environment weather.rs): the drops'
// clock and camera motion, the cover cache, the splashes and which draws
// show. `cover(x, y, z)` answers a cover cell (the height of the first roof
// or floor below the point, or of open water over it); `impact(x, y, z)` a
// splash's cast, returning the height it ends at and leaving its normal and
// whether it is water in `normal` (x, y, z, water).
export class WeatherHost {
  constructor(module, capacity, enhanced, range, size, cover, impact, normal, owner = null) {
    const callbacks = { host_cover: cover, host_impact: impact, host_impact_normal: (i) => normal[i] };
    const x = owner ? owner.attach(callbacks) : new WebAssembly.Instance(module, imports(callbacks)).exports;
    this.x = x;
    x.weather_new(capacity, enhanced ? 1 : 0, range, size);
    const buffer = x.memory.buffer;
    this.frame = new Float64Array(buffer, x.weather_frame_ptr(), WeatherFrame.STRIDE);
    this.result = new Float64Array(buffer, x.weather_result_ptr(), WeatherResult.STRIDE);
    const f32 = (kind) => new Float32Array(buffer, x.weather_data_ptr(kind), x.weather_data_len(kind));
    this.cover = f32(0);
    this.locations = f32(1);
    this.normals = f32(2);
    this.seeds = f32(3);
    this.mist = f32(4);
    this.streaks = new Uint8Array(buffer, x.weather_streaks_ptr(), x.weather_streaks_len());
    this.cell = x.weather_cover_cell();
  }

  // Classic or Enhanced rain: a fresh cover cache of the range and size,
  // in the same memory (nothing is allocated, so no view moves).
  mode(enhanced, range, size) {
    this.x.weather_mode(enhanced ? 1 : 0, range, size);
    this.cell = this.x.weather_cover_cell();
    this.cover = new Float32Array(this.x.memory.buffer, this.x.weather_data_ptr(0), this.x.weather_data_len(0));
  }

  update() {
    this.x.weather_update();
  }
}

// An instance for the placed effects' visibility (rust/bfe-environment
// environment.rs): each instance's sphere, position and kind are written,
// then `start`; each frame the camera, the reach and the mode go in and the
// instances in view come out.
export class VisibilityHost {
  constructor(module, count, owner = null) {
    const x = owner ? owner.x : new WebAssembly.Instance(module, imports()).exports;
    this.x = x;
    x.vis_new(count);
    this.count = count;
    this.config = new Float64Array(x.memory.buffer, x.vis_config_ptr(), x.vis_config_len());
  }

  start() {
    const x = this.x;
    x.vis_start();
    const buffer = x.memory.buffer;
    this.view = new Float64Array(buffer, x.vis_view_ptr(), View.STRIDE);
    this.frame = new Float64Array(buffer, x.vis_frame_ptr(), EnvironmentFrame.STRIDE);
    this.result = new Float64Array(buffer, x.vis_result_ptr(), x.vis_result_len());
    this.handles = new Float64Array(buffer, x.vis_handles_ptr(), this.count);
    this.wakeFrame = new Float64Array(buffer, x.vis_wake_frame_ptr(), WakeFrame.STRIDE);
    this.wakeActors = new Float64Array(buffer, x.vis_wake_actors_ptr(), Wakes.ACTORS * WakeActor.STRIDE);
    this.wakeResult = new Float64Array(buffer, x.vis_wake_result_ptr(), WakeResult.STRIDE);
    this.wakePoints = new Float64Array(buffer, x.vis_wake_points_ptr(), Wakes.COUNT * 4);
    this.wakeMotion = new Float64Array(buffer, x.vis_wake_motion_ptr(), Wakes.COUNT * 4);
  }

  update() {
    this.x.vis_update();
  }
}

// All surface owners on a battle share this module and its fixed heap. Their
// constructors install collision queries before any frame can run. A shared
// owner advances every installed native system with one surface_frame call.
export class SurfaceOwner {
  constructor(module = loaded()) {
    this.callbacks = Object.create(null);
    const env = {
      host_ground: (x, z) => this.callbacks.host_ground(x, z),
      host_cover: (x, y, z) => this.callbacks.host_cover(x, y, z),
      host_impact: (x, y, z) => this.callbacks.host_impact(x, y, z),
      host_impact_normal: i => this.callbacks.host_impact_normal(i),
      host_shore_tile: (x, z, height) => this.callbacks.host_shore_tile(x, z, height),
      host_shore_exposed: (x, z, height) => this.callbacks.host_shore_exposed(x, z, height),
    };
    this.x = new WebAssembly.Instance(module, imports(env)).exports;
    this.x.surface_reserve();
    this.x.surface_new();
  }
  attach(callbacks) { Object.assign(this.callbacks, callbacks); return this.x; }
  update() { this.x.surface_frame(); }
}
