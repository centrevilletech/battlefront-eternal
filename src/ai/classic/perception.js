import * as THREE from "three";
import { ANGLE, Aimer, dot32, dropCorrect, lengthSq32, lengthSqXZ32, lookupCos, normalizeOrX } from "./aim.js";
import {
  FireControl, clusterState, expandedRange, facingAndRange, forwardSpeed, maxExpandedRange, selectedWeapon, sensorRange, setTarget,
} from "./weapons-ai.js";

// What a Classic bot notices, following the 2004 game (docs/ai-combat.md).
//
// Sight is a pipeline spread over updates. When the scheduler services a
// controller, scan() gathers living, hostile, targetable units within
// clamp(largest expanded weapon range, 75, 350) of its eye, keeps those
// inside a 155-degree cone of its aim and within (30 + 20 r) of it (r: the
// target's broadphase radius; double for a flyer looking), that its attack
// patterns would engage (dispatch), and ranks them in a 112-entry heap by
// 20000 - trunc(distance) x (21 - matchup priority), with 10000 more cost for
// targets only a weapon (no pattern) could hit. In phase 1 its agent thins
// that heap into per-unit ranks (random skips when more than four), and the
// global deferred-LOS service tests the ranks in order across all bots, a
// few tests per update (6, or 12 on Hard; doubled when the path planner is
// idle). The first clear one becomes the controller's candidate, which its
// agent's target tracker takes up at the next service. No line of sight
// means no candidate: there is no other way to acquire a target by sight.
//
// A tracker keeps its target for 3 s (or longer in some patterns) after the
// candidate stops matching it, switching early only to a cheaper one. While
// the candidate matches, the target is fresh and the controller requests
// validated fire (B); a remembered target draws direct fire (A) only when
// its remembered point is still within 3 units of it, in the aim cone, in
// range and in clear view.
//
// Hearing is separate and needs no sight: once per scheduler cycle every
// unit within min(30, its noise) of an enemy bot (noise: half its weapon
// sound's MuteDistance while firing, else 3 x its speed up to 50) is queued
// on that bot (20 at most), unless nearly straight above or below it beyond
// 3.5 m. Grenades and sticky charges warn everyone within 7.5 of where they
// lie (after a delay drawn by the enemy skill tier, every second at most),
// and shots aimed at a bot and damage taken alert it to the shooter. What a
// bot does with these is its agent's call (acceptance policies) and the
// interrupt helper's (the response functions below).

const f = Math.fround;
const LOCAL_CAPACITY = 112;

// The shared generators: A (world.randomA, else world.random) for nearly
// every draw; B (world.randomB) for the cube offsets.
export function rngA(world) {
  return world.randomA ?? world.random;
}

export function rngB(world) {
  return world.randomB ?? rngA(world);
}
const LOCAL_SENTINEL = 20000;
const GLOBAL_CAPACITY = 200;
const GLOBAL_SENTINEL = 200;
const SCAN_CAPACITY = 175;

// ---------------------------------------------------------------------------
// Heaps. One-based max-heaps over integer priorities with a sentinel at 0.
// Insertion bubbles up only past strictly lower parents; removal moves the
// last entry to the root and sinks it, taking the right child only when it
// is strictly higher, stopping at a child no higher than it. A full heap
// replaces its minimum (scanned from floor(n/2), earlier index on ties) only
// for a strictly higher newcomer, which then bubbles up from that slot.

export class PriorityHeap {
  constructor(capacity, sentinel) {
    this.capacity = capacity;
    this.priority = new Int32Array(capacity + 1);
    this.items = new Array(capacity + 1).fill(null);
    this.priority[0] = sentinel;
    this.count = 0;
    this.dropped = 0; // equal or lower newcomers turned away when full
  }

  get size() {
    return this.count;
  }

  clear() {
    for (let i = 1; i <= this.count; i++) this.items[i] = null;
    this.count = 0;
  }

  // Place at slot k and bubble up.
  place(k, priority, item) {
    let parent = k >> 1;
    while (this.priority[parent] < priority) {
      this.priority[k] = this.priority[parent];
      this.items[k] = this.items[parent];
      k = parent;
      parent = k >> 1;
    }
    this.priority[k] = priority;
    this.items[k] = item;
  }

  // Zero-based index of the minimum, scanning slots
  // floor(n/2) .. n and keeping the earlier on ties.
  minIndex() {
    let best = this.count >> 1;
    for (let i = best + 1; i <= this.count; i++) if (this.priority[i] < this.priority[best]) best = i;
    return best - 1;
  }

  // Returns the displaced item, the newcomer if it was
  // dropped, or null.
  insert(priority, item) {
    if (this.count < this.capacity) {
      this.count += 1;
      this.place(this.count, priority, item);
      return null;
    }
    const slot = this.minIndex() + 1;
    if (this.priority[slot] < priority) {
      const old = this.items[slot];
      this.place(slot, priority, item);
      return old;
    }
    this.dropped += 1;
    return item;
  }

  peek() {
    return this.count ? this.items[1] : null;
  }

  peekPriority() {
    return this.count ? this.priority[1] : 0;
  }

  pop() {
    if (!this.count) return null;
    const top = this.items[1];
    const n = this.count;
    const vp = this.priority[n];
    const vi = this.items[n];
    this.items[n] = null;
    this.count = n - 1;
    let i = 1;
    if (this.count >= 2) {
      while (true) {
        let c = i * 2;
        if (c < this.count && this.priority[c] < this.priority[c + 1]) c += 1;
        if (vp >= this.priority[c]) break;
        this.priority[i] = this.priority[c];
        this.items[i] = this.items[c];
        i = c;
        if (!(c <= this.count >> 1)) break;
      }
    }
    if (this.count >= 1) {
      this.priority[i] = vp;
      this.items[i] = vi;
    }
    return top;
  }

  // Items in slot order, for tests and debugging.
  slots() {
    return this.items.slice(1, this.count + 1);
  }
}

// ---------------------------------------------------------------------------
// Matchup priority and the target cost. IDs are the
// classifier's (unit.kind): 0 soldier, 1 droid, 2 walker, 3 droideka,
// 5 flyer, 6 hover, 7 turret/passenger slot, 8 armed building, 9 building.

const PRIORITY_INFANTRY = [10, 5, 20, 10, 5, 3, 15, 0, 20, 1, 0];
const PRIORITY_GROUND = [5, 1, 20, 5, 1, 10, 10, 0, 10, 1, 0];
const PRIORITY_FLYER = [5, 1, 20, 5, 10, 20, 10, 0, 20, 10, 0];

export function matchupPriority(source, target) {
  let row = null;
  if (source === 0 || source === 1 || source === 3) row = PRIORITY_INFANTRY;
  else if (source === 2 || source === 4 || source === 6 || source === 8) row = PRIORITY_GROUND;
  else if (source === 5) row = PRIORITY_FLYER;
  return row ? row[target] ?? 0 : 0;
}

const eye = new THREE.Vector3();
const look = new THREE.Vector3();
const aimAt = new THREE.Vector3();

// The port's objective focus (docs/objectives.md, "The AI"): a destructible
// post's building ranks as a bot's top match-up (an armed building's 20, not
// a building's 1) when that post is the bot's objective, so the bots sent to
// destroy the shield generator shoot it rather than every soldier on the
// way. Not in the 2004 game, where they rarely did.
export const OBJECTIVE_PRIORITY = 20;
// The port's tow focus (docs/ai-vehicles.md "Pattern 15: the tow-cable
// run"): a pilot whose craft's cable holds a walker ranks that walker above
// anything (cost 0, so nothing is ever cheaper), and flies the tow circle
// round it until it falls or the cable lets go. Ranked as the 2004 game
// ranks (the nearest first, every walker, turret and craft alike), a pilot
// towing an AT-AT on Hoth was drawn off by a nearer AT-ST, turret or the
// other AT-AT within a dozen seconds, before three circuits, and the cable
// stretched past its length behind him. Not in the 2004 game.
export const TOWED_PRIORITY = 21;
export function targetPriority(unit, target, controller = null) {
  const post = target?.building?.post;
  if (post && controller?.objective === post) return OBJECTIVE_PRIORITY;
  const hull = unit?.pilot ? unit.vehicle : null;
  if (hull && hull.towTarget >= 0 && target?.vehicle && hull.system?.vehicles?.[hull.towTarget] === target.vehicle) return TOWED_PRIORITY;
  return matchupPriority(unit.kind, target.kind);
}

// trunc(distance to the target's aim point) x (21 - priority).
export function rawCost(unit, target, controller = null) {
  unit.origin(eye);
  target.aimPoint(aimAt);
  const d = f(Math.sqrt(lengthSq32(f(aimAt.x - eye.x), f(aimAt.y - eye.y), f(aimAt.z - eye.z))));
  return Math.trunc(d) * (21 - targetPriority(unit, target, controller));
}

// ---------------------------------------------------------------------------
// Sight geometry.

let coneCos = null;

// The squared distance to the target's aim point when inside the
// 77.5-degree half-angle cone of the bot's aim (or within 0.01 of its eye),
// else -2 (outside).
export function coneDistanceSq(unit, target) {
  coneCos ??= lookupCos(ANGLE.CONE);
  unit.origin(eye);
  unit.forward(look);
  target.aimPoint(aimAt);
  const rx = f(aimAt.x - eye.x), ry = f(aimAt.y - eye.y), rz = f(aimAt.z - eye.z);
  const L2 = lengthSq32(rx, ry, rz);
  if (f(0.0001) < L2) {
    const z = dot32(look.x, look.y, look.z, rx, ry, rz);
    if (z <= 0 || f(z * z) <= f(f(L2 * coneCos) * coneCos)) return -2;
  }
  return L2;
}

// The collision broadphase radius the sight range reads. Soldiers' come from
// their posture (the collision constructor's square roots: standing
// sqrt(2 w^2 + (h + 0.2)^2 / 4) with w 0.72, h 1.9; crouching with w 0.45,
// h 1.6; prone sqrt((0.8 x 0.3333)^2 + 2.56)); other units use their sphere.
const POSTURE_RADIUS = (() => {
  const w = [0.7200000286, 0.4499999881, 0.8000000119].map(f);
  const h = [1.8999999762, 1.6000000238, 0.5500000119].map(f);
  const upright = (i) => f(Math.sqrt(f(f(f(2 * f(w[i] * w[i]))) + f(0.25 * f(f(h[i] + f(0.2)) * f(h[i] + f(0.2)))))));
  const p = f(w[2] * f(0.3332999945));
  return { stand: upright(0), crouch: upright(1), prone: f(Math.sqrt(f(f(p * p) + f(2.5600001812)))) };
})();

export function boundRadius(unit) {
  if (unit?.kind === 0 && unit.soldier) {
    const posture = unit.soldier.air ? "stand" : unit.soldier.posture;
    return POSTURE_RADIUS[posture] ?? POSTURE_RADIUS.stand;
  }
  return f(unit?.sphere?.radius ?? 0);
}

// (30 + 20 r) x (2 for a flyer looking, else 1).
export function sightRange(owner, target) {
  const m = owner?.kind === 5 ? 2 : 1;
  return f(f(f(f(boundRadius(target) * 20) + 30)) * m);
}

// Living, finite MaxHealth, a targetable class, hostile.
export function validTarget(world, unit, target) {
  if (!target?.alive || target === unit) return false;
  if (target.targetable === false) return false;
  const max = target.combat?.maxHealth ?? target.classDef?.MaxHealth ?? 0;
  if (!(max < 3.4028234663852886e38)) return false;
  return world.units.relation(unit.team, target.team) < 0;
}

// The point a sight test looks at: for a unit with a controllable
// (soldiers, droidekas: anything with an eye), midway between its eye and
// its aim point; otherwise its aim point.
const losEye = new THREE.Vector3();

export function losPoint(target, out) {
  target.aimPoint(out);
  if (typeof target.origin === "function") {
    target.origin(losEye);
    out.set(f(f(losEye.x + out.x) * 0.5), f(f(losEye.y + out.y) * 0.5), f(f(losEye.z + out.z) * 0.5));
  }
  return out;
}

// The eye-to-point segment is clear (terrain, statics, props,
// vehicles, soldiers), the shooter and the target excepted.
export function clearShot(world, unit, point, target) {
  unit.origin(eye);
  return world.los(eye, point, unit, target);
}

// ---------------------------------------------------------------------------
// Per-controller perception state (the UnitController's, plus the aimer and
// the fire control the AIThread uses).

export class Perception {
  constructor(controller) {
    this.controller = controller;
    this.heap = new PriorityHeap(LOCAL_CAPACITY, LOCAL_SENTINEL);
    this.candidate = null; // ref: the deferred LOS service's pick
    this.candidatePoint = new THREE.Vector3(); // its sampled point
    this.aimer = new Aimer();
    this.fire = new FireControl();
    this.lastScanCount = 0;
  }

  reset() {
    this.heap.clear();
    this.candidate = null;
    this.aimer.started = false;
    this.fire.reset();
  }
}

function resolve(ref) {
  const u = ref?.unit;
  return u && u.alive && u.generation === ref.generation ? u : null;
}

function refOf(unit) {
  return unit ? { unit, generation: unit.generation } : null;
}

// ---------------------------------------------------------------------------
// The scan. dispatch(controller, unit, target) -> { pattern,
// aux }: the attack-pattern/weapon eligibility (combat.js's dispatch()).

const found = [];
const scanTargets = [];

export function scan(controller, world, dispatch) {
  const unit = controller.unit;
  const p = controller.perception;
  if (!unit?.alive || !p) return 0;
  p.heap.clear();
  scanTargets.length = 0;
  let special = false;
  // A human-driven walker or hover shares its driver's target with an AI
  // mount, past the cone (see docs/ai-combat.md).
  const shared = world.vehicles?.sharedTarget?.(controller) ?? null;
  if (shared) {
    scanTargets.push(shared);
    special = true;
  } else {
    let radius = maxExpandedRange(unit, controller);
    radius = radius <= 75 ? 75 : radius;
    radius = 350 <= radius ? 350 : radius;
    unit.origin(eye);
    world.units.query(eye, radius, found, (u) => u !== unit && world.units.relation(unit.team, u.team) < 0);
    for (let i = 0; i < found.length && i < SCAN_CAPACITY; i++) scanTargets.push(found[i]);
  }
  let queued = 0;
  for (const target of scanTargets) {
    if (!validTarget(world, unit, target)) continue;
    if (target.collisionCategory === 16) continue;
    const L2 = special ? -1 : coneDistanceSq(unit, target);
    if (L2 === -2) continue;
    const R = sightRange(unit, target);
    if (!(L2 < f(R * R))) continue;
    const e = dispatch(controller, unit, target, false, world);
    if (!e.pattern && !e.aux) continue;
    let cost = Math.max(1, rawCost(unit, target, controller));
    if (!e.pattern && e.aux) cost += 10000;
    p.heap.insert(LOCAL_SENTINEL - cost, refOf(target));
    queued += 1;
  }
  p.lastScanCount = queued;
  return queued;
}

// ---------------------------------------------------------------------------
// The global deferred-LOS queue: two heaps, with submit, drain and service.

export class DeferredLos {
  constructor() {
    this.active = new PriorityHeap(GLOBAL_CAPACITY, GLOBAL_SENTINEL);
    this.scratch = new PriorityHeap(GLOBAL_CAPACITY, GLOBAL_SENTINEL);
    this.overflow = 0;
    this.tested = 0; // tests run, total (debugging)
  }

  // A full queue keeps the request only if it outranks the
  // minimum; turned-away requests are counted.
  submit(controller, target, priority) {
    const req = { controller, target, generation: controller.generation };
    if (this.active.insert(priority, req) === req) this.overflow += 1;
  }

  // Before a controller's new scan: drop its old requests, dead ones, and
  // those of controllers already holding a candidate.
  drain(controller) {
    const from = this.active;
    const to = this.scratch;
    to.clear();
    while (from.size) {
      const priority = from.peekPriority();
      const req = from.pop();
      if (req.controller === controller || !requestLive(req) || resolve(req.controller.perception?.candidate)) continue;
      to.insert(priority, req);
    }
    this.active = to;
    this.scratch = from;
  }

  // Up to `budget` sight tests, in rank order. Returns whether any ran.
  service(world, budget) {
    let tests = 0;
    let any = false;
    while (tests < budget && this.active.size) {
      const req = this.active.pop();
      if (!requestLive(req)) continue;
      const controller = req.controller;
      if (resolve(controller.perception?.candidate)) continue;
      const target = resolve(req.target);
      tests += 1;
      this.tested += 1;
      any = true;
      const unit = controller.unit;
      losPoint(target, losAt);
      if (clearShot(world, unit, losAt, target)) recordTarget(controller, target, losAt, world);
    }
    return any;
  }
}

const losAt = new THREE.Vector3();

function requestLive(req) {
  const c = req?.controller;
  return Boolean(c?.unit?.alive && c.generation === req.generation && c.perception && resolve(req.target));
}

// An agent's phase-1 pass: pop the local heap into the global
// queue as per-unit ranks 112, 111, ... (the retained target no lower than
// 110). With more than four candidates each pop may skip on a 1-in-2 draw,
// unless it is the retained target (the draw happens either way while
// another candidate remains).
export function thin(controller, world, retainedRef, queue) {
  const p = controller.perception;
  if (!p) return 0;
  const heap = p.heap;
  const total = heap.size;
  const retained = resolve(retainedRef);
  let rank = LOCAL_CAPACITY;
  let submitted = 0;
  while (heap.size) {
    let c = resolve(heap.pop());
    if (total > 4) {
      let skip = true;
      while (heap.size && skip) {
        skip = rngA(world).int(1, 2) !== 1;
        if (!retained || c !== retained) {
          if (skip) c = resolve(heap.pop());
        } else skip = false;
      }
    }
    // (A candidate gone stale compares equal to no retained target, as the
    // 2004 game's pointers do.)
    let priority = rank;
    if (c === retained) priority = Math.max(rank, LOCAL_CAPACITY - 2);
    queue.submit(controller, refOf(c), priority);
    submitted += 1;
    rank -= 1;
  }
  return submitted;
}

// The LOS service accepted `target`: it becomes the controller's
// candidate with the sampled point. For a soldier target, at most every 5 s
// per target, look for a cluster: three or more soldiers within 9 of it
// with none allied to the attacker flags it (grenade preference) for 5 s.
const clusterFound = [];

export function recordTarget(controller, target, point, world) {
  const p = controller.perception;
  p.candidate = refOf(target);
  p.candidatePoint.copy(point);
  if (target.kind !== 0) return;
  const state = clusterState(target, true);
  const now = f(world.time ?? 0);
  if (!state || !(f(state.scanAt + 5) < now)) return;
  state.scanAt = now;
  const attacker = controller.unit;
  const units = world.units;
  units.query(target.position, 9, clusterFound, (u) => u !== target && units.relation(target.team, u.team) !== 0);
  let count = 1;
  for (let i = 0; i < clusterFound.length && i < 10; i++) {
    const u = clusterFound[i];
    if (units.relation(attacker.team, u.team) > 0) return;
    if (u.kind === 0) count += 1;
  }
  if (count > 2) state.flaggedAt = now;
}

// The scheduler clears the candidate after phase 1.
export function clearCandidate(controller) {
  if (controller.perception) controller.perception.candidate = null;
}

// ---------------------------------------------------------------------------
// The target tracker. Each agent has one.

const DEFAULT_DELAY = 3;
const FLT_MAX = 3.4028234663852886e38;

export class TargetTracker {
  constructor(controller) {
    this.controller = controller;
    this.ref = null;
    this.fresh = false; // the candidate matches the target
    this.point = new THREE.Vector3(); // last accepted sampled point
    this.sampleTime = -1000;
    this.cost = 10000;
    this.delay = DEFAULT_DELAY;
    this.frozen = false; // deadline at FLT_MAX
    this.counting = false;
    this.deadline = 0;
  }

  get target() {
    return resolve(this.ref);
  }

  // The target while it is fresh.
  get freshTarget() {
    return this.fresh ? this.target : null;
  }

  // Take a new target (or none), resetting the memory.
  reset(target) {
    this.ref = refOf(target);
    this.fresh = false;
    this.sampleTime = -1000;
    this.cost = 10000;
    this.counting = false;
  }

  // The combat goal's own target.
  assign(target) {
    this.reset(target);
  }

  // A fresh sample of the target.
  refresh(target, point, now) {
    this.fresh = true;
    this.point.copy(point);
    this.sampleTime = f(now);
    const unit = this.controller.unit;
    this.cost = unit && target ? rawCost(unit, target, this.controller) : 10000;
    this.counting = false;
  }

  // Start the retention countdown.
  startCountdown(now) {
    this.counting = true;
    this.deadline = this.frozen ? FLT_MAX : f(f(now) + f(this.delay));
  }

  // The retention delay (4 s in cover sidestep, 12 hover passes, 16
  // lost-target search).
  setDelay(seconds, now) {
    this.delay = f(seconds);
    if (this.counting) this.startCountdown(now);
  }

  defaultDelay(now) {
    this.setDelay(DEFAULT_DELAY, now);
  }

  // Hold the target indefinitely (cover) or release.
  freeze(on, now) {
    this.frozen = Boolean(on);
    if (this.counting) this.startCountdown(now);
  }

  // Follow the controller's candidate. Returns whether the target
  // changed.
  select(world) {
    const controller = this.controller;
    const unit = controller.unit;
    const now = world.time ?? 0;
    const p = controller.perception;
    const candidate = resolve(p?.candidate);
    const before = this.target;
    let current = before;
    if (current && unit && world.units.relation(unit.team, current.team) >= 0) {
      this.reset(candidate);
      current = this.target;
    }
    if (!current) {
      this.reset(candidate);
      current = this.target;
    } else if (current !== candidate) {
      const cost = candidate ? rawCost(unit, candidate, controller) : 10000;
      if (cost < this.cost) {
        this.reset(candidate);
        current = this.target;
      } else if (!this.counting) {
        this.startCountdown(now);
      } else if (f(this.deadline + 0) < f(now)) {
        this.reset(candidate);
        current = this.target;
      }
    }
    if (current && current === candidate) this.refresh(current, p.candidatePoint, now);
    else this.fresh = false;
    return before !== this.target;
  }

  // The weapon's target, or (remembered, not fresh) none and the
  // remembered point to aim at.
  assignWeaponTarget(world) {
    const controller = this.controller;
    const target = this.target;
    if (!this.fresh && target) {
      setTarget(controller, controller.unit, null, world);
      return this.point;
    }
    setTarget(controller, controller.unit, target, world);
    return null;
  }

  // The remembered point is still within 3 units of the target's
  // sight point.
  pointNearTarget() {
    const target = this.target;
    if (!target) return false;
    losPoint(target, losAt);
    return lengthSq32(f(this.point.x - losAt.x), f(this.point.y - losAt.y), f(this.point.z - losAt.z)) < 9;
  }

  // The controller's two fire requests.
  fireControls(world) {
    const controller = this.controller;
    const unit = controller.unit;
    let a = false;
    let b = false;
    const target = this.target;
    if (target && unit) {
      if (!this.fresh) {
        const weapon = selectedWeapon(unit, controller);
        if (weapon) dropCorrect(this.point, unit, weapon, leadAt);
        else leadAt.copy(this.point);
        if (weapon && facingAndRange(controller, unit, leadAt, this.point) && this.pointNearTarget()) {
          a = clearShot(world, unit, this.point, target);
        }
      } else b = true;
    }
    controller.fireA = a;
    controller.fireB = b;
  }

  // For agents that track and shoot (combat, trooper, scout,
  // repair...): selection, weapon choice (switching allowed), the weapon's
  // target, the fire requests.
  update(world, dispatch) {
    this.select(world);
    const controller = this.controller;
    const target = this.target;
    if (target && controller.unit?.combat?.arsenal?.weapons?.length) dispatch?.(controller, controller.unit, target, true, world);
    this.assignWeaponTarget(world);
    this.fireControls(world);
  }
}

const leadAt = new THREE.Vector3();

// ---------------------------------------------------------------------------
// Acoustic awareness (the broadcast, soldier and droid noise; consumption is
// acousticResponse() below).

// The MuteDistance of a weapon's active sound. The manifests give the stock
// infantry fire sounds 400 (rep rifle and sniper, cis blaster); without a
// sound table that is the stand-in for every weapon (provisional).
const DEFAULT_MUTE = 400;

function weaponNoise(weapon, sounds) {
  if (!weapon) return 0;
  // States 1..5 (firing, charging...) read the fire sound; idle and
  // reloading states read a second sound only for some weapons (which ones is
  // not known).
  const active = weapon.charging || weapon.busy || weapon.wait > 1e-6;
  if (!active) return 0;
  const mute = sounds?.(weapon) ?? weapon.def?.MuteDistance ?? DEFAULT_MUTE;
  return f(mute);
}

// A source's radius before the broadcast's 30 cap. Soldiers: half the
// active weapon sound's MuteDistance, or 3 x speed up to 50, whichever is
// larger; droids 2 x speed up to 50; mounted turrets and passenger slots
// nothing; flyers the class constant 1 (with their engine sound running in
// the 2004 game). Walkers, hovers and droidekas take their engine and event
// sounds' MuteDistances (x5 for walkers and droidekas), which needs the
// sound table: until it is wired they stand in with 2 x speed up to 50
// (provisional).
export function noiseRadius(unit, sounds = null) {
  const v = unit.velocity;
  const speed = v ? f(Math.sqrt(lengthSq32(v.x, v.y, v.z))) : 0;
  const k = unit.kind;
  if (k === 7) return 0;
  if (k === 5) return 1;
  if (k !== 0) return clamp50(f(speed * 2));
  const weapon = unit.combat?.arsenal ? selectedWeapon(unit, unit.controller) ?? unit.combat.arsenal.current(0) : null;
  const w = f(weaponNoise(weapon, sounds) * 0.5);
  const m = clamp50(f(speed * 3));
  return w <= m ? m : w;
}

function clamp50(x) {
  return x < 0 ? 0 : 50 < x ? 50 : x;
}

const heard = [];
const up = new THREE.Vector3(0, 1, 0);
const n3 = { x: 0, y: 0, z: 0 };

// `source` is a unit (bot or human). Returns how many bots were notified.
export function broadcastAcoustic(source, world, sounds = null) {
  if (!source?.alive) return 0;
  const raw = noiseRadius(source, sounds);
  const R = raw <= 30 ? raw : 30;
  if (!(R > 0)) return 0;
  const units = world.units;
  const at = source.position;
  units.query(at, R, heard, (u) => u !== source && units.relation(source.team, u.team) < 0);
  let notified = 0;
  for (let i = 0; i < heard.length && i < SCAN_CAPACITY; i++) {
    const recipient = heard[i];
    const controller = recipient.controller;
    if (!controller?.addAcoustic) continue;
    const dx = f(at.x - recipient.position.x), dy = f(at.y - recipient.position.y), dz = f(at.z - recipient.position.z);
    normalizeOrX(dx, dy, dz, n3);
    const vertical = Math.abs(dot32(up.x, up.y, up.z, n3.x, n3.y, n3.z));
    if (f(0.87) <= vertical && f(12.25) <= lengthSq32(dx, dy, dz)) continue;
    controller.addAcoustic(refOf(source));
    notified += 1;
  }
  return notified;
}

// ---------------------------------------------------------------------------
// Grenade and sticky danger, called by the ordnance for each live
// grenade or sticky charge every update. state: { firstContact (Infinity
// until it first touches something), lastNotify (-1) } kept on the round.
// One RNG draw every call, by the GLOBAL enemy skill tier (every recipient,
// allies included): < 14 [1.1, 2.1) s, 14..16 [0.6, 1.3), 17+ [0, 0.8).

const dangerFound = [];

export function dangerDelayRange(enemySkill) {
  if (enemySkill < 17) return enemySkill < 14 ? [f(1.1), f(2.1)] : [f(0.6), f(1.3)];
  return [0, f(0.8)];
}

export function notifyDanger(world, state, position, owner, enemySkill, onWarn = null) {
  const [lo, hi] = world.difficulty?.dangerRow?.() ?? dangerDelayRange(enemySkill);
  const delay = rngA(world).range(lo, hi);
  const now = f(world.time ?? 0);
  if (!(f(state.firstContact + delay) < now) || !(f(state.lastNotify + 1) < now)) return 0;
  world.units.query(position, 7.5, dangerFound, (u) => u !== owner);
  let warned = false;
  let notified = 0;
  for (let i = 0; i < dangerFound.length && i < SCAN_CAPACITY; i++) {
    const u = dangerFound[i];
    const controller = u.controller;
    if (!controller && !u.human) continue;
    if (!warned && u.kind === 0) {
      warned = true;
      onWarn?.(u);
    }
    if (controller?.notifyDanger) {
      controller.notifyDanger(position);
      notified += 1;
    }
  }
  state.lastNotify = now;
  return notified;
}

// ---------------------------------------------------------------------------
// Targeted-fire and damage alerts (on the victim's controller:
// controller.notifyShot(source, known, point)).

// A shot aimed at `target` by `shooter` (units): every AI controller on the
// target hears it, with the shooter's position, when they are enemies.
export function notifyTargetedShot(world, shooter, target, shooterPosition = shooter?.position) {
  if (!shooter || !target?.alive) return false;
  const source = shooter.character ?? null;
  if (!source) return false;
  if (!(world.units.relation(shooter.team, target.team) < 0)) return false;
  if (target.human) return false;
  let notified = false;
  for (const controller of alertControllers(target, alerted)) {
    controller.notifyShot(source, true, shooterPosition);
    notified = true;
  }
  return notified;
}

// The UnitControllers an alert on `target` reaches (the target entity's
// controllable list, each resolved to its active control): a unit's own
// controller, the controller of the seat a seated soldier holds (his boarded
// controllable), and for a vehicle or a turret (an armed building, whose one
// control is its mounted turret) every AI crew controller aboard. Before this
// a shot at, or damage to, a turret or a hull reached no one: their registry
// entries have no controller of their own, and gunners sat on while they were
// shot to pieces.
const alerted = [];
export function alertControllers(target, out = []) {
  out.length = 0;
  const add = (c) => {
    if (c?.notifyShot && !out.includes(c)) out.push(c);
  };
  const lease = target.seat;
  if (lease && !lease.human && lease.controllable?.controller) add(lease.controllable.controller);
  else add(target.controller);
  const body = target.turret ?? target.vehicle ?? null;
  if (body && !target.lease) {
    const leases = Array.isArray(body.seats) ? body.seats.map((s) => s.lease) : [body.lease];
    for (const l of leases) {
      if (!l || l.human || l.released || l.live === false) continue;
      add(l.controllable?.controller ?? l.unit?.controller);
    }
  }
  return out;
}

// Damage to `victim` from an attacker (a unit, or a Character). packet:
// { amount, generic (factor), scales: { person, ... } (per health type) }.
// The alert needs an attributed hostile source and more than 1 of damage by
// either factor; `known`/`point` as the damage producer supplies them (bolt
// hits back-project 30 units along the round).
export function notifyDamage(world, victim, attacker, packet, known = false, point = null) {
  if (!victim?.alive || !attacker) return false;
  const source = attacker.character ?? attacker;
  const team = attacker.team ?? source.team;
  if (!(world.units.relation(victim.team, team) < 0)) return false;
  const amount = f(packet?.amount ?? 0);
  const generic = f(packet?.generic ?? 1);
  const typed = f(packet?.typed ?? 1);
  if (!(f(amount * generic) > 1 || f(amount * typed) > 1)) return false;
  let notified = false;
  for (const controller of alertControllers(victim, alerted)) {
    controller.notifyShot(source, known, point);
    notified = true;
  }
  return notified;
}

// ---------------------------------------------------------------------------
// The interrupt helper's perception responses. Each returns a goal for the
// controller to push ({ kind: 2 (combat), mode, point }) plus flags, or
// null. agent: the active agent (its acceptance policies).

export const COMBAT_GOAL = 2;

// The common gate: a selected weapon.
function weaponGate(controller) {
  return Boolean(selectedWeapon(controller.unit, controller));
}

// The response point for an alert source: the source Character's current
// controllable (its body, vehicle or remote droid; the vehicle code's
// currentControllable() when bound, as the interrupt helper resolves it).
function sourceUnit(controller, source) {
  if (!source) return null;
  if (source.alive !== undefined && source.position) return source.alive ? source : null;
  const u = controller.ai?.vehicles?.currentControllable?.(source) ?? source.remote ?? source.unit ?? null;
  return u?.alive ? u : null;
}

const delta = new THREE.Vector3();

// A targeted shot or damage. In range of the source's sight and
// worth engaging: combat goal mode 0 at the source's current position. A
// soldier that heard it but cannot engage instead dodges to a random point
// within 1 of itself (mode 2).
export function alertResponse(controller, world, agent, dispatch) {
  const shot = controller.shot;
  if (!shot?.pending) return null;
  const unit = controller.unit;
  const src = sourceUnit(controller, shot.source);
  if (!unit || !src) return null;
  const point = src.position.clone();
  if (!weaponGate(controller) || !agent?.acceptsAlert?.(shot.known, point)) return null;
  const R = sightRange(unit, src);
  delta.subVectors(point, unit.position);
  if (lengthSq32(f(delta.x), f(delta.y), f(delta.z)) < f(R * R) && engageable(controller, unit, src, world, dispatch)) {
    return { kind: COMBAT_GOAL, mode: 0, point, exit: mountedExit(controller, world, point) };
  }
  if (unit.kind !== 0) return null;
  const r = rngB(world);
  const z = f(2 * f(r.uniform() - 0.5));
  const y = f(2 * f(r.uniform() - 0.5));
  const x = f(2 * f(r.uniform() - 0.5));
  return { kind: COMBAT_GOAL, mode: 2, point: new THREE.Vector3(f(unit.position.x + x), f(unit.position.y + y), f(unit.position.z + z)), exit: false };
}

// Weapon slot 0 present and the dispatcher would engage.
function engageable(controller, unit, target, world, dispatch) {
  if (!unit.combat?.arsenal?.weapons?.length) return false;
  const e = dispatch?.(controller, unit, target, false, world);
  return Boolean(e?.pattern);
}

// A zero-speed mount whose guns cannot be brought to bear on the point is
// left (an exit request), unless the mission keeps bots in turrets. The
// angular reach is the vehicle code's (ai.vehicles.mountReach); without it
// nobody leaves.
function mountedExit(controller, world, point) {
  const unit = controller.unit;
  if (!unit || forwardSpeed(unit) !== 0) return false;
  const ai = controller.ai;
  if (ai?.stayInTurrets?.() ?? world.mission?.stayInTurrets) return false;
  const reach = ai?.vehicles?.mountReach ?? world.vehicles?.mountReach;
  return reach ? !reach(unit, point) : false;
}

// A grenade or sticky charge nearby: a soldier whose agent accepts
// gets combat goal mode 2 at the danger point.
export function hazardResponse(controller, world, agent) {
  const danger = controller.danger;
  if (!danger?.pending) return null;
  const unit = controller.unit;
  if (!unit || !weaponGate(controller) || !agent?.acceptsHazard?.()) return null;
  if (unit.kind === 5 && world.nav?.inDangerRegion?.(unit)) return null;
  if (unit.kind !== 0) return null;
  return { kind: COMBAT_GOAL, mode: 2, point: danger.point.clone(), exit: false };
}

// Queued acoustic sources in order; the first the agent accepts
// within the bot's sensor range (horizontal; half for a droideka) gives
// combat goal mode 0 at its current position.
export function acousticResponse(controller, world, agent) {
  const unit = controller.unit;
  if (!unit || !controller.acoustic?.length) return null;
  if (unit.kind === 5 && world.nav?.inDangerRegion?.(unit)) return null;
  for (const ref of controller.acoustic) {
    const src = resolve(ref);
    if (!src || !weaponGate(controller) || !agent?.acceptsAcoustic?.(src)) continue;
    let R = sensorRange(unit, controller);
    if (unit.kind === 3) R = f(R * 0.5);
    const point = src.position.clone();
    if (lengthSqXZ32(f(point.x - unit.position.x), f(point.z - unit.position.z)) < f(R * R)) {
      const exit = mountedExit(controller, world, point);
      return { kind: COMBAT_GOAL, mode: 0, point, exit, alert: true };
    }
  }
  return null;
}

export { expandedRange };
