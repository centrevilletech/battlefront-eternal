// The world surface's numeric boundary. Water, shore and foliage keep their
// renderer resources; this binding publishes facts and borrows native arrays.
import { Surface as LIMITS, SurfaceConfig as CONFIG, SurfaceFrame as FRAME,
  SurfaceCommand as COMMAND, SurfaceCommandKind as KIND, SurfaceResult as RESULT,
  ShoreTriangle as TRIANGLE } from './layout.js';

export class SurfaceHost {
  constructor(owner) {
    this.owner = owner;
    this.bindings = []; this.buffer = null; this.refresh();owner.hosts.push(this);
    // Contacts acquire an id only at first observation. The WeakMap retains
    // no object after its page owner dies; native rows expire independently.
    // This map carries identity, never wave or animation state.
    this.contacts = new WeakMap();
    this.nextContact = 0;
    this.commandCount = 0;
    this.firstCommand = 0; this.droppedCommands = 0;
  }
  // Linear-memory growth detaches every old view, even when each native
  // address stays fixed. Refresh only at publication and command boundaries.
  refresh() {
    const x = this.owner.x, buffer = x.memory.buffer;
    if (this.buffer === buffer) return false;
    this.buffer = buffer;
    this.config = new Float64Array(buffer, x.surface_config_ptr(), CONFIG.STRIDE);
    this.frame = new Float64Array(buffer, x.surface_frame_ptr(), FRAME.STRIDE);
    this.commands = new Float64Array(buffer, x.surface_commands_ptr(), LIMITS.COMMANDS * COMMAND.STRIDE);
    this.result = new Float64Array(buffer, x.surface_result_ptr(), RESULT.STRIDE);
    // Draw buffers are borrowed directly. Only their active prefixes upload;
    // the native addresses stay fixed while growth can replace their buffer.
    // Shore collision scratch is separate from its draw and cache storage.
    const vertices = LIMITS.RIPPLES * LIMITS.SEGMENTS * 4;
    this.positions = new Float32Array(buffer, x.surface_ring_ptr(0), vertices * 3);
    this.uvs = new Float32Array(buffer, x.surface_ring_ptr(1), vertices * 2);
    this.alpha = new Float32Array(buffer, x.surface_ring_ptr(2), vertices);
    this.shore = new Float32Array(buffer, x.surface_shore_ptr(0), LIMITS.SHORE_PATCHES * 4);
    this.phases = new Float32Array(buffer, x.surface_shore_ptr(1), LIMITS.SHORE_PATCHES);
    this.triangles = new Float64Array(buffer, x.surface_triangles_ptr(), LIMITS.SHORE_TRIANGLES * TRIANGLE.STRIDE);
    this.stormPositions = new Float32Array(buffer, x.surface_storm_ptr(0), LIMITS.STORM_VERTICES * 3);
    this.stormTangents = new Float32Array(buffer, x.surface_storm_ptr(1), LIMITS.STORM_VERTICES * 3);
    this.stormEdges = new Float32Array(buffer, x.surface_storm_ptr(2), LIMITS.STORM_VERTICES);
    this.stormWidths = new Float32Array(buffer, x.surface_storm_ptr(3), LIMITS.STORM_VERTICES);
    this.stormEnergies = new Float32Array(buffer, x.surface_storm_ptr(4), LIMITS.STORM_VERTICES);
    for (let i = 0; i < this.bindings.length; i++) {
      const binding=this.bindings[i];binding.target[binding.key]=binding.get?binding.get(this):this[binding.name];
    }
    return true;
  }
  // Water.ringMesh, WaterShore and StormSky borrow native publication arrays.
  // Rebind those retained packets after growth, without BufferAttribute shells.
  bindArray(target,key,name) {
    this.refresh();const get=typeof name==='function'?name:null;target[key]=get?get(this):this[name];this.bindings.push({target,key,name,get});
  }
  // Births are bounded while loading or paused. Water.ripple formerly shifted
  // at 100 births; native application preserves that newest-ring policy.
  command(kind) {
    this.refresh();
    if (this.commandCount === LIMITS.COMMANDS) {
      this.firstCommand = (this.firstCommand + 1) % LIMITS.COMMANDS;
      this.commandCount--; this.droppedCommands++;
    }
    const at = ((this.firstCommand + this.commandCount++) % LIMITS.COMMANDS) * COMMAND.STRIDE;
    this.commands[at + COMMAND.KIND] = kind;
    return at;
  }
  // Water.ripple's arguments become one ordered record, without retaining a
  // Vector3 clone or creating a temporary array for the transfer.
  ripple(position, life, radius, velocity) {
    const at = this.command(KIND.RIPPLE), c = this.commands;
    c[at + COMMAND.POSITION] = position.x; c[at + COMMAND.POSITION + 1] = position.y; c[at + COMMAND.POSITION + 2] = position.z;
    c[at + COMMAND.LIFE] = life; c[at + COMMAND.RADIUS] = radius;
    c[at + COMMAND.HAS_VELOCITY] = velocity ? 1 : 0;
    if (velocity) { c[at + COMMAND.VELOCITY] = velocity.x; c[at + COMMAND.VELOCITY + 1] = velocity.y; c[at + COMMAND.VELOCITY + 2] = velocity.z; }
  }
  // soldierStep publishes contact facts after its gameplay water query.
  // The phase computes speed, lifetime, distance order and the eight uniforms.
  wader(contact, feet, velocity, depth) {
    let id = this.contacts.get(contact);
    if (!id) { id = ++this.nextContact; this.contacts.set(contact, id); }
    const at = this.command(KIND.WADER), c = this.commands;
    c[at + COMMAND.ID] = id;
    c[at + COMMAND.POSITION] = feet.x; c[at + COMMAND.POSITION + 1] = feet.y; c[at + COMMAND.POSITION + 2] = feet.z;
    c[at + COMMAND.VELOCITY] = velocity.x; c[at + COMMAND.VELOCITY + 2] = velocity.z;
    c[at + COMMAND.DEPTH] = depth;
  }
  // Camera matrices are already native outputs on the battle page. Reading
  // their forward column avoids getWorldDirection's second matrix update.
  prepare(dt, camera) {
    this.refresh();
    const f = this.frame, m = camera.matrixWorld.elements;
    f[FRAME.DT] = dt;
    f[FRAME.EYE] = camera.position.x; f[FRAME.EYE + 1] = camera.position.y; f[FRAME.EYE + 2] = camera.position.z;
    f[FRAME.FORWARD] = -m[8]; f[FRAME.FORWARD + 1] = -m[9]; f[FRAME.FORWARD + 2] = -m[10];
    f[FRAME.COMMANDS] = this.commandCount;
    f[FRAME.COMMAND_START] = this.firstCommand; f[FRAME.COMMAND_DROPPED] = this.droppedCommands;
    this.commandCount = 0; this.firstCommand = 0; this.droppedCommands = 0;
  }
}
