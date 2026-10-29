import * as THREE from "three";
import { EXEC, PathExecutor } from "./executor.js";
import { f32 } from "./math.js";
import { KIND } from "./units.js";

// The movement request a path executor (or a stop) writes for the unit, and
// the unit's input step turns into analogs (docs/ai.md
// "Movement helper and request").
export class MovementRequest {
  constructor() {
    this.valid = false;
    this.steeringTarget = new THREE.Vector3();
    this.waypoint = new THREE.Vector3();
    this.segmentDirection = new THREE.Vector3();
    this.mode = 0;
    this.lower = -1;
    this.upper = 1;
  }

  write(target, waypoint, direction, mode, lower, upper) {
    this.valid = true;
    this.steeringTarget.set(target.x, target.y, target.z);
    this.waypoint.set(waypoint.x, waypoint.y, waypoint.z);
    this.segmentDirection.set(direction.x, direction.y, direction.z);
    this.mode = mode;
    this.lower = lower;
    this.upper = upper;
    return this;
  }

  clear() {
    this.valid = false;
    return this;
  }
}

// One AI unit's navigation state: what the 2004 game keeps on the unit's
// Controllable and UnitController for paths.
//
// - request: the MovementRequest.
// - executor: the PathExecutor following the installed path, or null.
// - published/ready: the planner's answer slot. ready with a null path is a
//   completed failure, not an arrival.
// - segments: the unsigned byte of outstanding flagged path segments; it
//   raises queued request priorities and gates the planner's
//   straight-segment shortcut.
// - jet: a pending jet request and its direction; jetting is the two-step
//   jump-then-jet sequence in progress.
// - stance: the requested posture, 0 stand, 1 crouch, 2 prone.
// - avoidSide: the remembered contact-sidestep side.
// - stranded: the movement helper gave up short of where it was sent (the
//   planner found no way, or the graph route ran out). Standing there with
//   no path, the unit reports the executor's "no improvement" status once
//   that has lasted as long as an executor waits to report it, so the
//   stagnation recovery that frees a unit stuck on a path frees this one
//   too. Stranded again within 2 m of where it was (new orders between, a
//   look round in a fight), the time already spent there counts on.
//   (Port: the 2004 game reports 0 without an executor, and a bot whose
//   route fails stands where it is until something else moves it.)
//
// The control survives a controller's lives: attach() at each spawn starts
// fresh (a new serial, so the planner drops the old life's requests).
export class NavControl {
  constructor(nav, controller = null) {
    this.nav = nav;
    this.planner = nav.planner;
    this.controller = controller;
    this.unit = null;
    this.generation = -1;
    this.serial = 0;
    this.disposed = false;
    this.request = controller?.request instanceof MovementRequest ? controller.request : new MovementRequest();
    if (controller && controller.request !== this.request) controller.request = this.request;
    this.executor = null;
    this.published = null;
    this.ready = false;
    this.segments = 0;
    this.installFlags = [false, false];
    this.landing = false;
    this.stance = 0;
    this.jet = { pending: false, vector: new THREE.Vector3() };
    this.jetting = false;
    this.avoidSide = 0;
    this.stranded = false;
    this.strandedFor = 0;
    this.strandedAt = new THREE.Vector3(Infinity, 0, 0);
    this.leader = null;
    this.trail = null;
    this.trailDelay = 0;
  }

  // Bind to the unit now driven; a new unit or a new life resets the state.
  attach(unit) {
    if (unit === this.unit && unit?.generation === this.generation) return this;
    this.reset();
    this.unit = unit;
    this.generation = unit?.generation ?? -1;
    return this;
  }

  reset() {
    this.serial++;
    this.planner.cancel(this);
    this.dropExecutor();
    this.request.clear();
    this.segments = 0;
    this.jet.pending = false;
    this.jet.vector.set(0, 0, 0);
    this.jetting = false;
    this.stance = 0;
    this.avoidSide = 0;
    this.stranded = false;
    this.strandedFor = 0;
    this.strandedAt.set(Infinity, 0, 0);
    this.landing = false;
    this.leader = null;
    this.trail?.release();
    this.trail = null;
    this.trailDelay = 0;
  }

  // The helper gave up short of its destination (true), or has new orders.
  strand(on = true) {
    if (!on) { this.stranded = false; return; }
    if (this.stranded) return;
    this.stranded = true;
    const p = this.unit?.position;
    if (p && p.distanceToSquared(this.strandedAt) <= 4) return;
    this.strandedFor = 0;
    if (p) this.strandedAt.copy(p);
  }

  dispose() {
    this.reset();
    this.disposed = true;
  }

  // The planner's answer. An unconsumed path it replaces goes back
  // to the pools.
  publish(path, ready) {
    if (this.published && this.published !== path) this.published.release();
    this.published = path;
    this.ready = ready;
  }

  // Take a finished answer: { ready, success }. A path is
  // installed fresh (mode 0) or appended to the current one (mode 1) and the
  // slot is emptied; a failure leaves the slot as it is.
  consume(mode) {
    if (!this.ready) return { ready: false, success: false };
    const path = this.published;
    if (!path) return { ready: true, success: false };
    this.published = null;
    if (mode === 0 || !this.executor) this.install(path, true, false);
    else this.append(path);
    this.ready = false;
    return { ready: true, success: true };
  }

  // A fresh executor on path; the segment count restarts at one.
  install(path, a = true, b = false) {
    this.dropExecutor();
    this.strand(false);
    this.segments = 1;
    this.installFlags[0] = a;
    this.installFlags[1] = b;
    this.landing = false;
    this.executor = new PathExecutor(path, this, this.unit);
  }

  // Continue the current executor with path.
  append(path) {
    this.segments = (this.segments + 1) & 0xff;
    this.executor.append(path, this.unit);
  }

  // Drop the path and ask the unit to stand still where it is, facing as it
  // faces.
  stop() {
    this.dropExecutor();
    const u = this.unit;
    if (!u) return;
    const p = u.position;
    const yaw = u.soldier?.aimYaw ?? 0;
    this.request.write(p, p, { x: Math.sin(yaw), y: 0, z: Math.cos(yaw) }, 0, -1, 1);
  }

  dropExecutor() {
    if (!this.executor) return;
    this.executor.release();
    this.executor = null;
  }

  // Follow's leader and temporary path recorder.
  setLeader(ref) {
    this.dropExecutor();
    this.leader = ref;
    if (!ref) { this.trail?.release(); this.trail = null; }
    else this.trailDelay = 0;
  }

  recordTrail(on) {
    if (on) {
      this.trail?.release();
      this.trail = this.planner.path();
      return;
    }
    const path = this.trail;
    this.trail = null;
    if (!path) return;
    if (!path.length) { path.release(); return; }
    if (!this.executor) this.install(path);
    else {
      this.executor.append(path, this.unit, false);
      this.executor.complete = false;
    }
  }

  // Quarter-second sampling only after a strict 0.25m
  // displacement. Three generator-B cube draws precede the distance test;
  // the offset is local to the leader and uses the mission's far radius.
  sampleTrail(dt, leader) {
    if (this.executor?.hold) return;
    this.trailDelay = Math.fround(this.trailDelay - dt);
    if (!(this.trailDelay < 0)) return;
    const world = this.controller.world;
    const rng = world.randomB ?? world.gameRandom ?? world.random;
    const far = this.controller.ai?.followRadii?.far ?? world.missionControls?.formation?.far ?? 3;
    const z = 2 * Math.fround(rng.uniform() - 0.5) * far;
    rng.uniform(); // the discarded local Y still consumes a draw
    const x = 2 * Math.fround(rng.uniform() - 0.5) * far;
    const forward = leader.forward?.(new THREE.Vector3()) ?? new THREE.Vector3(0, 0, 1);
    forward.y = 0; forward.normalize();
    const point = leader.position.clone().addScaledVector(forward, z)
      .add(new THREE.Vector3(forward.z * x, 0, -forward.x * x));
    const tail = this.trail?.nodes.at(-1) ?? (!this.trail ? this.executor?.final : null);
    if (tail && point.distanceToSquared(tail) <= 0.0625) return;
    this.trailDelay = 0.25;
    if (this.trail) {
      if (this.trail.length > 29) this.trail.dropFront(1);
      this.trail.append(point);
    } else if (this.executor) this.executor.appendPoint(point, this.unit, 30);
  }

  // A flagged node was consumed.
  consumeSegment() {
    this.segments = (this.segments - 1) & 0xff;
    this.planner.promote(this);
  }

  writeRequest(target, waypoint, direction, mode, lower, upper) {
    this.request.write(target, waypoint, direction, mode, lower, upper);
  }

  // The per-tick path step of the unit's input (without a follow target):
  // run the executor; a completed path is dropped after its last steering.
  update(dt) {
    const ref = this.leader;
    const leader = ref?.unit?.alive && ref.unit.generation === ref.generation ? ref.unit : null;
    if (leader) this.sampleTrail(dt, leader);
    const e = this.executor;
    if (!e && this.stranded) this.strandedFor = f32(this.strandedFor + dt);
    else if (this.strandedFor && this.unit?.position.distanceToSquared(this.strandedAt) > 4) this.strandedFor = 0;
    if (!e || !this.unit) return;
    e.tick(dt, this.unit);
    if (e.complete && this.executor === e && !leader) this.dropExecutor();
  }

  // Executor progress status: 0, 1 no improvement, 2 stationary; stranded
  // without a path, 1 once the executor's no-progress time has passed (for
  // the soldiers and droidekas stagnation recovery serves).
  get status() {
    if (this.executor) return this.executor.status;
    if (!this.stranded) return 0;
    const kind = this.unit?.kind;
    if (kind !== KIND.soldier && kind !== KIND.droideka) return 0;
    const limit = kind === KIND.droideka ? EXEC.droidekaNoProgress : EXEC.noProgress;
    return limit < this.strandedFor ? 1 : 0;
  }

  get following() {
    return this.executor !== null;
  }
}
