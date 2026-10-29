import * as THREE from "three";
import { NavControl } from "./control.js";
import { GraphCursor } from "./cursor.js";
import { detour } from "./barriers.js";
import { FLT_MAX, TINY, d2, f32, flatDir, h2, same, vec } from "./math.js";
import { KIND, hasJetpack, isSoldier, isSoldierOrDroid, nominalSpeed, traversalMask, traversalRadius } from "./units.js";

// The ground movement helper agents drive (GroundGotoHelper).
//
// States, as the 2004 game numbers them:
//   1 idle
//   2 waiting for a local path; the unit stands still until it arrives
//   3 direct: a straight path, cut short 1 m before the first barrier it
//     crosses; when it runs out, retry (toward a random sidestep if it was
//     cut short), at most 5 times
//   4 local detour: walk around barriers by their corners, a bounded
//     number of steps at a time (non-infantry direct moves)
//   5 follow a unit, replanning when it moves (Soldier and Droid only)
//   6 following a local path while the next one is planned
//   7 stopped
//   8 a raw two-point path, no barrier checks
//
// goto() (the default "route" mode) corrects the destination out of
// barriers, then for a trip of 50 m or more (horizontally) through the
// authored graph starts a graph route and plans the local leg to its first
// waypoint; shorter trips plan one local path straight to the destination.
// Each leg's path is followed while the next leg is planned, so the graph
// waypoints chain without stopping. Status 1/2 from the executor are only
// reported (helper.status); agents and interrupts decide what to do.
//
// In states 3, 4, 5, 6 and 8 a jet trooper may launch its jetpack: when the
// fuel is above JetFuelMinBorder, the randomized deadline has passed (it then
// moves on 5 s), a coin flip passes, the destination is over 20 m away and 40
// m straight up is clear above both the unit and its destination. The draws
// come from the game's second generator, not the AI one.

const UP = new THREE.Vector3(0, 1, 0);
const scratch = { a: new THREE.Vector3(), b: new THREE.Vector3(), hit: null };

export class GroundGotoHelper {
  constructor(controller, world, options = {}) {
    this.controller = controller;
    this.world = world;
    this.nav = world.nav;
    this.control = controller.navControl ?? (controller.navControl = new NavControl(this.nav, controller));
    this.options = options;
    this.state = 1;
    this.pending = 0;
    this.destination = new THREE.Vector3();
    this.failed = false; // the planner answered "no path"
    this.followDistance = 0;
    this.cursor = null;
    this.adjusted = false; // the direct path was cut short
    this.normal = new THREE.Vector3(); // the edge normal it was cut at
    this.retries = 0;
    this.lastTarget = new THREE.Vector3(-FLT_MAX, 0, -FLT_MAX);
    this.lastRefresh = -100;
    this.followRemembered = false;
    this.target = null; // { unit, generation }
    this.detourSide = 0;
    this.detourPoint = new THREE.Vector3();
    this.detourFinal = new THREE.Vector3();
    this.jetDeadline = 0;
  }

  get unit() {
    const unit = this.controller.unit;
    if (unit) this.control.attach(unit);
    return unit;
  }

  get mask() {
    return traversalMask(this.unit);
  }

  get radius() {
    return traversalRadius(this.unit);
  }

  get random() {
    return this.world.random;
  }

  // ---- interface (docs/ai.md) ----

  // mode "route" (default), "direct" (state 3 for Soldier/Droid, else the
  // state-4 detour) or "straight".
  goto(point, options = {}) {
    if (!this.unit) return;
    this.control.strand(false);
    const mode = options.mode ?? "route";
    if (mode === "direct") {
      if (isSoldierOrDroid(this.unit)) this.gotoDirect(point);
      else this.gotoDetour(point);
    } else if (mode === "straight") {
      this.gotoStraight(point);
    } else {
      this.dropRoute();
      this.route(false, point);
      this.transition(2);
    }
    this.flush();
  }

  stop() {
    this.dropRoute();
    this.control.strand(false);
    this.transition(7);
    this.flush();
  }

  setLeader(ref) { this.control.setLeader(ref); }
  followTrail(on) { this.control.recordTrail(on); }
  holdPath(on) { if (this.control.executor) this.control.executor.hold = Boolean(on); }
  installPath(points) {
    if (!this.unit || !points.length) return;
    this.dropRoute();
    const path = this.nav.planner.path();
    for (const point of points) path.append(point);
    this.destination.copy(points.at(-1));
    this.control.install(path);
    this.state = 8;
    this.pending = 0;
  }

  // Keep `distance` from a unit ({unit, generation}); with
  // `remembered` the point is the controller's remembered position of its
  // current target instead (options.remembered(controller, unit) supplies
  // it). Only Soldier and Droid enter the follow state.
  follow(ref, distance = 0, remembered = false) {
    if (!this.unit) return;
    this.followDistance = distance;
    this.followRemembered = remembered;
    if (this.state === 5 && resolve(this.world, this.target) === resolve(this.world, ref)) {
      this.flush();
      return;
    }
    this.dropRoute();
    this.control.strand(false);
    this.target = ref;
    if (isSoldierOrDroid(this.unit)) this.transition(5);
    this.flush();
  }

  // The helper's FSM update: pending transitions, then the state's
  // work, repeated while the work requests another transition.
  tick(dt) {
    if (!this.unit) return;
    for (let i = 0; i < 8; i++) {
      this.flush();
      this.work(dt);
      if (!this.pending) break;
    }
    this.flush();
  }

  get arrived() {
    switch (this.state) {
      case 2:
      case 5:
        return false;
      case 3:
      case 4:
        return !this.control.executor && same(this.control.request.waypoint, this.destination);
      case 7:
        return true;
      default:
        if (!this.cursor) return !this.control.executor;
        if (this.control.executor) return false;
        return same(this.cursor.waypoint, this.destination);
    }
  }

  get stopped() {
    return this.state === 7;
  }

  get status() {
    return this.control.status;
  }

  // Where the helper is going; the unit's own position when
  // `current` or when a follow target is gone.
  destinationPoint(out = new THREE.Vector3(), current = this.state === 7) {
    if (!current) {
      if (this.state !== 5) return out.copy(this.destination);
      if (this.followPoint(out)) return out;
    }
    return out.copy(this.unit.position);
  }

  // Requested posture, 0 stand, 1 crouch, 2 prone.
  stance(value) {
    this.control.stance = value;
    this.flush();
  }

  // A jet request with a launch direction; a zero vector selects the
  // soldier's ordinary launch direction. Not normalized.
  jet(vector) {
    this.control.jet.pending = true;
    this.control.jet.vector.set(vector.x, vector.y, vector.z);
    this.flush();
  }

  // The controller's aim state (docs/ai.md controller.aimState):
  // 1 a stored point, 0 a unit (aimTarget), 2 along the path. Delegates to
  // the controller's own lookAt/aimAt/lookAlongPath when it has them.
  aimAt(point) {
    const c = this.controller;
    if (typeof c.lookAt === "function") c.lookAt(point);
    else {
      c.aimState = 1;
      (c.aimPoint ??= new THREE.Vector3()).copy(point);
    }
    this.flush();
  }

  aimAtTarget(ref) {
    const c = this.controller;
    if (typeof c.aimAt === "function") c.aimAt(ref);
    else {
      c.aimState = 0;
      c.aimTarget = ref;
    }
    this.flush();
  }

  aimAlongPath() {
    const c = this.controller;
    if (typeof c.lookAlongPath === "function") c.lookAlongPath();
    else c.aimState = 2;
    this.flush();
  }

  // ---- state machine ----

  transition(state) {
    this.pending = state;
  }

  flush() {
    for (let i = 0; this.pending && i < 16; i++) {
      const next = this.pending;
      this.pending = 0;
      this.state = next;
      this.enter(next);
    }
  }

  enter(state) {
    switch (state) {
      case 2:
      case 7:
        this.control.stop();
        break;
      case 3:
        this.retries = 0;
        this.adjusted = false;
        this.directPath(this.destination);
        break;
      case 5:
        this.retries = 0;
        this.adjusted = false;
        this.lastRefresh = -100;
        this.lastTarget.set(-FLT_MAX, 0, -FLT_MAX);
        if (!this.refreshFollow()) this.transition(7);
        break;
      case 4:
        this.detourPoint.copy(this.destination);
        this.detourSide = 0;
        this.detourFinal.set(0, 0, 0);
        this.localDetour();
        break;
      default:
        break;
    }
  }

  work(dt) {
    switch (this.state) {
      case 2:
        this.processPath(false);
        break;
      case 3:
        this.jetAttempt();
        if (!this.control.executor) {
          if (this.arrived || ++this.retries > 5) {
            this.transition(7);
            break;
          }
          this.directPath(this.adjusted ? this.sidestep(new THREE.Vector3()) : this.destination);
        }
        break;
      case 4:
        this.jetAttempt();
        if (!this.control.executor) {
          if (this.arrived) {
            this.transition(7);
            break;
          }
          this.detourPoint.copy(this.destination);
        }
        this.localDetour();
        break;
      case 5:
        this.jetAttempt();
        if (!this.refreshFollow()) this.transition(7);
        break;
      case 6:
        this.jetAttempt();
        if (this.control.executor) this.processPath(true);
        else this.transition(this.cursor ? 2 : 7);
        break;
      case 7:
        this.control.stop();
        break;
      case 8:
        this.jetAttempt();
        if (!this.control.executor) this.transition(7);
        break;
      default:
        break;
    }
  }

  // ---- routing ----

  // Drop the graph route and any pending local path work.
  dropRoute() {
    this.cursor = null;
    this.nav.planner.cancel(this.control);
  }

  // Plan toward point, then stand still until a path arrives.
  route(local, point) {
    const unit = this.unit;
    const start = vec(unit.position.x, unit.position.y, unit.position.z);
    const dest = vec(point.x, point.y, point.z);
    this.nav.barriers.correct(dest, this.radius, this.mask);
    this.destination.set(dest.x, dest.y, dest.z);
    this.failed = false;
    this.control.segments = 0;
    const far = h2(unit.position.x, unit.position.z, this.destination.x, this.destination.z) >= 2500;
    if (!local && far && this.nav.graph && this.nav.graph.table(this.mask) >= 0) this.graphRoute(start);
    else this.localLeg(start, this.destination, 0);
    this.control.stop();
  }

  // A graph route and its first local leg.
  graphRoute(start) {
    const nav = this.nav;
    const cursor = (this.cursor = new GraphCursor(nav.graph, nav.barriers));
    cursor.init(start, this.destination, this.mask, this.unit.kind === KIND.droideka ? 4 : 0, this.random);
    if (!nav.barriers.pointClear(cursor.waypoint, this.radius, this.mask)) cursor.retry(this.radius, this.random);
    this.localLeg(start, cursor.waypoint, 1);
  }

  // Ask the planner for start → end with start pushed out of barriers
  // for the leg.
  localLeg(start, end, priority) {
    const from = vec(start.x, start.y, start.z);
    this.nav.barriers.correct(from, this.radius, this.mask);
    this.request(from, vec(end.x, end.y, end.z), priority);
  }

  // A full planner queue refuses the request: answer it as a failure, so the
  // helper does not wait for a path that will never come.
  request(start, end, priority) {
    if (!this.nav.planner.request(this.control, start, end, this.radius, this.mask, priority)) this.control.publish(null, true);
  }

  // Take the planner's answer and line up the next graph leg. A route given
  // up short of the destination leaves the control stranded (control.js).
  processPath(chained) {
    const answer = this.control.consume(chained ? 1 : 0);
    if (!answer.ready) {
      if (!chained) this.control.stop();
      return;
    }
    if (!answer.success) {
      this.failed = true;
      this.cursor = null;
      this.control.strand();
    } else if (this.cursor) {
      const cursor = this.cursor;
      if (same(cursor.waypoint, this.destination)) {
        this.cursor = null;
      } else {
        const start = vec(cursor.waypoint.x, cursor.waypoint.y, cursor.waypoint.z);
        if (!cursor.advance(this.random)) {
          this.control.strand();
          this.transition(7);
          return;
        }
        let end = vec(cursor.waypoint.x, cursor.waypoint.y, cursor.waypoint.z);
        let lost = false;
        if (!this.nav.barriers.pointClear(end, this.radius, this.mask)) {
          if (cursor.retry(this.radius, this.random)) end = vec(cursor.waypoint.x, cursor.waypoint.y, cursor.waypoint.z);
          else lost = true;
        }
        if (lost) {
          this.cursor = null;
          this.control.strand();
        } else this.request(start, end, 1);
      }
    }
    if (!chained) this.transition(6);
  }

  // ---- direct moves (state 3) ----

  // The direct move for Soldier/Droid.
  gotoDirect(point) {
    if (this.state === 5 && same(this.destination, point)) return;
    this.dropRoute();
    this.destination.copy(point);
    this.followDistance = 0;
    this.transition(3);
  }

  // The direct move for other units: only when the point changes.
  gotoDetour(point) {
    if (same(this.destination, point)) return;
    this.dropRoute();
    this.destination.copy(point);
    this.followDistance = 0;
    this.transition(4);
  }

  // A raw two-point path, no barrier test.
  gotoStraight(point) {
    this.dropRoute();
    const p = this.unit.position;
    this.control.install(this.nav.planner.segment(p, point), true, false);
    this.transition(8);
  }

  // A straight path toward target, cut 1 m short of the first barrier
  // crossed.
  directPath(target) {
    const p = this.unit.position;
    const start = vec(p.x, p.y, p.z);
    const end = vec(target.x, target.y, target.z);
    const barriers = this.nav.barriers;
    barriers.correct(start, this.radius, this.mask);
    this.adjusted = barriers.obstruct(start, end, this.radius, this.mask, this.normal);
    if (this.adjusted) pullBack(start, end);
    this.dropRoute();
    this.control.install(this.nav.planner.segment(start, end), true, false);
  }

  // A point 10-20 m off along the cut edge's normal turned 45-90 degrees to
  // a random side. The draws: distance, angle, side.
  sidestep(out) {
    const r = this.random;
    const distance = r.int(10, 20);
    const lo = f32(Math.PI / 4);
    const hi = f32(Math.PI / 2);
    const angle = f32(r.uniform() * (hi - lo) + lo);
    const side = r.int(0, 1) !== 0 ? -1 : 1;
    const a = side * angle;
    const s = Math.sin(a);
    const c = Math.cos(a);
    const n = this.normal;
    const x = n.x * c - n.z * s;
    const z = n.z * c + n.x * s;
    const p = this.unit.position;
    return out.set(p.x + x * distance, p.y + n.y * distance, p.z + z * distance);
  }

  // ---- local detour (state 4) ----

  // Up to five corner steps around barriers toward the detour point, using
  // twice the clearance; a new two-point path when the point changed
  //.
  localDetour() {
    const unit = this.unit;
    const barriers = this.nav.barriers;
    const mask = this.mask;
    const r = f32(2 * this.radius);
    const start = vec(unit.position.x, unit.position.y, unit.position.z);
    barriers.correct(start, r, mask);
    const hit = { barrier: null, distanceSq: 0 };
    const a = vec();
    const b = vec();
    for (let i = 0; i < 5; i++) {
      const reach = Math.max(1, Math.min(30, 1.5 * nominalSpeed(unit)));
      const point = this.detourPoint;
      const dx = point.x - start.x;
      const dz = point.z - start.z;
      const probe = vec(point.x, point.y, point.z);
      if (reach * reach < dx * dx + dz * dz) {
        const dir = flatDir(vec(), dx, dz);
        probe.x = start.x + reach * dir.x;
        probe.y = start.y;
        probe.z = start.z + reach * dir.z;
      }
      if (!barriers.nearest(start, probe, r, mask, hit)) break;
      const sides = detour(hit.barrier, start, probe, r, a, b);
      let chosen;
      if (this.detourSide === 0) {
        const da = sides & 1 ? h2(a.x, a.z, start.x, start.z) : Infinity;
        const db = sides & 2 ? h2(b.x, b.z, start.x, start.z) : Infinity;
        if (da === Infinity && db === Infinity) break;
        if (db <= da) {
          this.detourSide = 2;
          chosen = b;
        } else {
          this.detourSide = 1;
          chosen = a;
        }
      } else {
        chosen = this.detourSide === 1 ? a : b;
        if (!(sides & this.detourSide)) break;
      }
      point.set(chosen.x, chosen.y, chosen.z);
    }
    if (same(this.detourPoint, this.detourFinal)) return;
    this.dropRoute();
    this.control.install(this.nav.planner.segment(start, this.detourPoint), true, false);
    this.detourFinal.copy(this.detourPoint);
  }

  // ---- follow (state 5) ----

  // Where following puts the unit.
  followPoint(out) {
    const target = resolve(this.world, this.target);
    if (!target) return false;
    if (!this.followRemembered) {
      const tp = target.position;
      const p = this.unit.position;
      const dir = flatDir(vec(), p.x - tp.x, p.z - tp.z);
      out.set(tp.x + dir.x * this.followDistance, tp.y, tp.z + dir.z * this.followDistance);
      return true;
    }
    const remembered = this.options.remembered?.(this.controller, target);
    if (!remembered) return false;
    out.copy(remembered);
    return true;
  }

  // Replan toward the moving follow point when it has moved over 1 m, at
  // most every 1.5 s (3 s beyond 5 m), leading it by one second of the
  // target's velocity.
  refreshFollow() {
    const point = new THREE.Vector3();
    if (!this.followPoint(point)) return false;
    const target = resolve(this.world, this.target);
    const unit = this.unit;
    if (target === unit) {
      this.control.stop();
      return true;
    }
    if (h2(point.x, point.z, this.lastTarget.x, this.lastTarget.z) > 1) {
      const throttle = h2(unit.position.x, unit.position.z, point.x, point.z) > 25 ? 3 : 1.5;
      const now = this.world.time;
      if (this.lastRefresh + throttle < now) {
        this.lastRefresh = now;
        if (target?.velocity) point.add(target.velocity);
        this.directPath(point);
        this.lastTarget.copy(point);
      }
    }
    // The direct path is used up short of the point (cut at a barrier, or
    // pushed aside past its end), and a target that keeps still never asks
    // for another.
    if (!this.control.executor && h2(unit.position.x, unit.position.z, point.x, point.z) > 4) this.control.strand();
    return true;
  }

  // ---- opportunistic jet ----

  jetAttempt() {
    const unit = this.unit;
    if (!isSoldier(unit) || !hasJetpack(unit)) return;
    const soldier = unit.soldier;
    const border = unit.classDef?.JetFuelMinBorder ?? soldier.jet?.minBorder ?? 0;
    const rng = gameRandom(this.world);
    const now = this.world.time;
    if (!(soldier.fuel > border)) {
      this.jetDeadline = f32(now + 5 + f32(rng.uniform() * 10));
      return;
    }
    if (this.control.jetting) return;
    if (!(now > this.jetDeadline)) return;
    this.jetDeadline = f32(this.jetDeadline + 5);
    if ((rng.next() & 1) === 0) return;
    const q = this.destinationPoint(scratch.b, false);
    if (!(d2(unit.position, q) > 400)) return;
    if (!clearAbove(this.world, unit.position, unit)) return;
    this.destinationPoint(q, false);
    if (d2(q, this.destination) < TINY) q.copy(this.control.request.steeringTarget);
    if (!clearAbove(this.world, q, unit)) return;
    this.jet(scratch.a.set(0, 0, 0));
  }
}

// The Flyer movement helper (FlyGotoHelper). Interface-complete, behavior
// minimal (Provisional): goto plans a local path with the Flyer mask and no
// graph route (forceLocal); "direct" is a straight two-point path;
// the flight-specific altitude, throttle and landing logic belongs to the
// vehicle code and is not here.
export class FlyGotoHelper {
  constructor(controller, world, options = {}) {
    this.controller = controller;
    this.world = world;
    this.nav = world.nav;
    this.control = controller.navControl ?? (controller.navControl = new NavControl(this.nav, controller));
    this.options = options;
    this.state = 1;
    this.pending = 0;
    this.destination = new THREE.Vector3();
    this.failed = false;
  }

  get unit() {
    const unit = this.controller.unit;
    if (unit) this.control.attach(unit);
    return unit;
  }

  goto(point, options = {}) {
    const unit = this.unit;
    if (!unit) return;
    if (options.mode === "direct" || options.mode === "straight") {
      this.nav.planner.cancel(this.control);
      this.destination.copy(point);
      this.pending = 3;
    } else {
      const mask = traversalMask(unit);
      const radius = traversalRadius(unit);
      const dest = vec(point.x, point.y, point.z);
      this.nav.barriers.correct(dest, radius, mask);
      this.destination.set(dest.x, dest.y, dest.z);
      this.failed = false;
      this.control.segments = 0;
      const from = vec(unit.position.x, unit.position.y, unit.position.z);
      this.nav.barriers.correct(from, radius, mask);
      this.nav.planner.request(this.control, from, dest, radius, mask, 0);
      this.control.stop();
      this.pending = 2;
    }
    this.flush();
  }

  stop() {
    this.pending = 5;
    this.flush();
  }

  flush() {
    for (let i = 0; this.pending && i < 16; i++) {
      const next = this.pending;
      this.pending = 0;
      this.state = next;
      if (next === 2 || next === 5) this.control.stop();
      else if (next === 3) {
        const p = this.unit.position;
        this.control.install(this.nav.planner.segment(p, this.destination), false, false);
      }
    }
  }

  tick() {
    if (!this.unit) return;
    this.flush();
    if (this.state === 2) {
      const answer = this.control.consume(0);
      if (answer.ready) {
        this.failed = !answer.success;
        this.pending = answer.success ? 4 : 5;
      } else {
        this.control.stop();
      }
    } else if (this.state === 5) {
      this.control.stop();
    } else if ((this.state === 3 || this.state === 4) && this.arrived) {
      this.pending = 5;
    }
    this.flush();
  }

  get arrived() {
    if (this.state === 2) return false;
    if (this.state === 3) return !this.control.executor && same(this.destination, this.control.request.waypoint);
    if (this.state === 5) return true;
    return !this.control.executor;
  }

  get stopped() {
    return this.state === 5;
  }

  get status() {
    return this.control.status;
  }

  destinationPoint(out = new THREE.Vector3(), current = false) {
    return current ? out.copy(this.unit.position) : out.copy(this.destination);
  }
}

// Shorten the end 1 m toward the start, or to the start when within 1 m
//.
function pullBack(start, end) {
  const dx = end.x - start.x, dy = end.y - start.y, dz = end.z - start.z;
  const len2 = dx * dx + dy * dy + dz * dz;
  if (len2 > 0.1 && len2 > 1) {
    const l = Math.sqrt(len2);
    end.x -= dx / l;
    end.y -= dy / l;
    end.z -= dz / l;
    return;
  }
  end.x = start.x;
  end.y = start.y;
  end.z = start.z;
}

function resolve(world, ref) {
  if (!ref) return null;
  if (world.units?.resolve) return world.units.resolve(ref);
  const unit = ref.unit ?? ref;
  return unit && unit.alive !== false && (ref.generation === undefined || unit.generation === ref.generation) ? unit : null;
}

// 40 m straight up is clear for sight (mask 0x4e), ignoring the unit.
function clearAbove(world, p, unit) {
  const from = scratch.a.set(p.x, p.y, p.z);
  if (typeof world.los === "function") {
    const to = new THREE.Vector3(p.x, p.y + 40, p.z);
    return world.los(from, to, unit, null);
  }
  if (world.collision?.cast) {
    scratch.hit ??= { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
    return !world.collision.cast(from, UP, 40, scratch.hit);
  }
  return true;
}

// The game's second generator (the one the jet attempt draws from), apart
// from the AI generator. Integrators provide world.gameRandom; until then a
// private stream keeps these draws from moving world.random (Provisional).
function gameRandom(world) {
  if (world.gameRandom) return world.gameRandom;
  if (!world.__navGameRandom) {
    const Ctor = world.random?.constructor;
    world.__navGameRandom = Ctor ? new Ctor(0x9e3779b9) : { uniform: () => 0.5, next: () => 0 };
  }
  return world.__navGameRandom;
}
