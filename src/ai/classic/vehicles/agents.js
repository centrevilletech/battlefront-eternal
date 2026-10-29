import * as THREE from "three";
import { Agent } from "../agents.js";
import { DONE, refOf, resolveRef } from "../controller.js";
import { serviceOf } from "./service.js";

// The vehicle agents: boarding (controller state 5), leaving (state 6) and
// the flyer pilot's objective agent (state 7), as the 2004 game has them
// (docs/ai-vehicles.md).
//
// They run on the controller's Agent base (src/ai/classic
// agents.js) and reach vehicles through this.ai.vehicles (the VehicleAI
// service in index.js).

const _p = new THREE.Vector3();
const _q = new THREE.Vector3();
const _d = new THREE.Vector3();
const DOWN_UP = new THREE.Vector3(0, 1, 0);

// ---------------------------------------------------------------------------
// State 5: walk to the vehicle and get in.
//
// Entry (state 2): with the vehicle resolving, an existing seat reservation
// of this unit's, or (failing that) free capacity for its team, which it
// then reserves, sends it walking to the vehicle; otherwise it gives up.
// Each tick, after its interrupts and the ordinary target update (it keeps
// fighting as it goes): it stays only while it may enter the vehicle, its
// reservation still holds (renewed every tick) and the vehicle is all but
// still (speed below 2 m/s, squared below 4). Approaching (2), once the two
// bounding spheres are within half a metre it boards (3); boarding asks the
// vehicle to take it, which seats it at once when a seat is still free.
// Danger, alerts, scanner targets and sounds are refused while the vehicle
// resolves with no eligible enemy within 8 m of it; it never takes a
// boarding interrupt or the crew wait, and never saves its goal. Commands and
// recovery go through the base policies.
export class UnitBoardAgent extends Agent {
  constructor(controller, goal) {
    super(controller, goal);
    this.vehicle = goal?.refA ?? null;
    this.requestState(2);
  }

  get candidate() {
    return resolveRef(this.vehicle);
  }

  onEnter() {
    const vehicles = serviceOf(this.ai) ?? this.ai.vehicles;
    if (this.state === 2) {
      const vehicle = this.candidate;
      if (!vehicle || !vehicles?.reserveSeat?.(this.controller, vehicle)) {
        this.requestState(DONE);
        return;
      }
      this.helper?.goto?.(vehicle.position);
    } else if (this.state === 3) {
      this.helper?.stop?.();
    }
  }

  behave(dt) {
    if (this.state === DONE) return;
    if (this.interrupts.tick()) return;
    this.updateCombatTarget();
    const vehicles = serviceOf(this.ai) ?? this.ai.vehicles;
    const vehicle = this.candidate;
    const unit = this.controller.unit;
    if (!vehicle || !unit || !vehicles?.canEnter?.(this.controller, vehicle) || !vehicles.holdReservation?.(this.controller, vehicle)) {
      this.requestState(DONE);
      return;
    }
    const v = vehicle.velocity;
    if (!(v.x * v.x + v.y * v.y + v.z * v.z < 4)) {
      this.requestState(DONE);
      return;
    }
    if (this.state === 2) {
      const a = unit.sphere;
      const b = vehicle.sphere;
      const reach = a.radius + b.radius + 0.5;
      if (a.center.distanceToSquared(b.center) < reach * reach) this.requestState(3);
      else if (this.helper?.arrived && !this.helper.failed) {
        // The walk is over and the seat is still out of reach (a turret on
        // the deck above the point the route ends under): give it up, and
        // the next invitation for a while. (Port: the reservation renews
        // every tick, and the bot stood there for good.)
        this.controller.reboardAt = this.now() + 15;
        this.requestState(DONE);
        return;
      }
      this.aim(0);
    } else if (this.state === 3) {
      vehicles.boardNow?.(this.controller, vehicle);
    }
    this.tickHelper(dt);
  }

  // Alerts, hazards and commands: refused while the vehicle is uncontested.
  contested() {
    const vehicle = this.candidate;
    if (!vehicle) return true;
    return !((serviceOf(this.ai) ?? this.ai.vehicles)?.boardPathClear?.(this.controller, vehicle) ?? true);
  }

  acceptsHazard() {
    return this.contested();
  }

  acceptsAlert() {
    return this.contested();
  }

  acceptsTarget(target) {
    return this.contested() && this.canEngage(target);
  }

  acceptsAcoustic(source) {
    return this.contested() && this.canEngage(source);
  }

  acceptsBoard() {
    return false;
  }

  acceptsSecondaryWait() {
    return false;
  }

  savesOnInterrupt() {
    return false;
  }
}

// ---------------------------------------------------------------------------
// State 6: get out. Its entry (state 1) asks for the exit; the seat lets
// the unit out at the next input batch, and the unit's own controller takes
// over from there. Meanwhile it keeps its target updated. It refuses
// danger, alerts, sounds, boarding and the crew wait, and saves nothing; a
// scanner target, a command or a recovery goes through the base policies.
// (If the seat is already gone, it finishes.)
export class UnitUnBoardAgent extends Agent {
  constructor(controller, goal) {
    super(controller, goal);
    this.state = 0;
    this.requestState(1);
  }

  onEnter() {
    if (this.state === 1) this.controller.requestExit();
  }

  behave(dt) {
    if (this.state === DONE) return;
    if (this.interrupts.tick()) return;
    this.updateCombatTarget();
    if (!this.ai.vehicles?.boarded?.(this.controller)) this.requestState(DONE);
    this.tickHelper(dt);
  }

  acceptsHazard() {
    return false;
  }

  acceptsAlert() {
    return false;
  }

  acceptsAcoustic() {
    return false;
  }

  acceptsBoard() {
    return false;
  }

  acceptsSecondaryWait() {
    return false;
  }

  savesOnInterrupt() {
    return false;
  }
}

// ---------------------------------------------------------------------------
// State 7, the flyer pilot's objective (UnitFlyAgent): fly to a command
// post, find a landing hint near it, land, and (a transport) let its AI
// passengers out, (any other flyer) get out itself.
//
//   1 start: flying on to 4; on the ground, wait (tick: on to 3)
//   2 climb out: a straight 200 m path ahead (1 m up) until 100 m straight
//     up is clear, then 4
//   3 wait: a transport whose flag is set waits 10 s; anything else until
//     the absolute time 4 (the 2004 game writes the literal); then 4 if
//     the sky is clear above, else 2
//   4 travel: up to ten random posts until one passes the ownership test or
//     lies 100 m away, then 20 m above it; meanwhile every 3 s look for a
//     landing hint within an 80 m cube (one candidate a look); a found one
//     goes to 5; an empty arrival picks again
//   5 land: hold the hint, fly the approach (L - 2 MaxSpeed d) to the
//     landing point L and set down; on the ground, 6
//   6 landed: a set flag lets the AI passengers out (players stay); a
//     non-transport closes the hint for 20 s and its pilot leaves, carrying
//     the post, not to board again for 15 s; a transport (or a clear flag)
//     flips the flag and waits again (3)
//
// A stuck flyer (executor status 2) is destroyed, as the 2004 game does
// to its stranded aircraft. One still on its pad, its takeoff failing, is
// left by its crew instead (port: VehicleAI.abandonPad).
export class UnitFlyAgent extends Agent {
  constructor(controller, goal) {
    super(controller, goal);
    this.hint = null;
    this.searchAt = -Infinity;
    this.flag = false;
    this.deadline = 0;
    this.post = null;
    this.state = 0;
    this.requestState(1);
  }

  get flyer() {
    return this.unit?.vehicle ?? null;
  }

  // VehicleType Transport.
  get transport() {
    return String(this.unit?.classDef?.VehicleType ?? "").toLowerCase() === "transport";
  }

  get airborne() {
    const v = this.flyer;
    return Boolean(v && !v.grounded);
  }

  // On its pad: landed, or lifting off it and not yet going over to flight
  // (a lift a roof holds keeps the craft over its pad).
  get onPad() {
    const v = this.flyer;
    return Boolean(v && (v.grounded || v.flightState === "TakingOff" && v.gear >= 1));
  }

  onEnter() {
    const helper = this.helper;
    const unit = this.unit;
    const now = this.now();
    switch (this.state) {
      case 1:
        this.flag = true;
        if (this.airborne) this.requestState(4);
        else helper?.stop?.();
        return;
      case 2: {
        const yaw = unit.bodyYaw ?? 0;
        const P = unit.position;
        helper?.followPath?.([[P.x, P.y, P.z], [P.x + 200 * Math.sin(yaw), P.y + 1, P.z + 200 * Math.cos(yaw)]], true, true);
        return;
      }
      case 3:
        this.deadline = this.transport && this.flag ? now + 10 : 4;
        helper?.stop?.();
        return;
      case 4:
        this.pickPost();
        return;
      case 5:
        this.approach();
        return;
      case 6:
        this.landed();
        return;
      default:
    }
  }

  behave(dt) {
    if (this.state === DONE) return;
    if (this.controller.executorStatus === 2) {
      const vehicles = serviceOf(this.ai);
      // Never off the pad: the crew get out and go on foot, and the craft
      // stays for whoever can lift it out.
      if (this.onPad && vehicles?.abandonPad) {
        vehicles.abandonPad(this.controller);
        this.requestState(DONE);
        return;
      }
      vehicles?.destroyStuck?.(this.controller);
      return;
    }
    if (this.interrupts.tick()) return;
    this.updateCombatTarget();
    const now = this.now();
    switch (this.state) {
      case 1:
        this.requestState(3);
        break;
      case 2:
        if (this.clearAbove()) this.requestState(4);
        break;
      case 3:
        if (now > this.deadline) this.requestState(this.clearAbove() ? 4 : 2);
        break;
      case 4:
        if (this.findHint()) this.requestState(5);
        else if (this.helper?.arrived) {
          // Arrived with no hint: pick again.
          this.onExit();
          this.onEnter();
        }
        break;
      case 5:
        if (this.hint) serviceOf(this.ai)?.holdHint?.(this.hint, this.controller);
        if (!this.airborne) this.requestState(6);
        break;
      default:
    }
    this.aim(0);
    this.tickHelper(dt);
  }

  onExit() {
    if (this.state === 5) this.helper?.land?.(false);
  }

  // 100 m straight up is clear.
  clearAbove() {
    const unit = this.unit;
    const collision = this.world.collision;
    if (!collision) return true;
    _p.copy(unit.position);
    return !collision.cast(_p, DOWN_UP, 100, hitScratch, this.flyer?.body ?? null);
  }

  // State 4's destination.
  pickPost() {
    const posts = this.world.posts?.posts ?? [];
    const unit = this.unit;
    if (!posts.length) return;
    const random = this.ai.randomA;
    const transport = this.transport;
    let post = null;
    for (let attempt = 1; ; attempt++) {
      post = posts[random.int(0, posts.length - 1)];
      const ok = !transport || (this.flag ? !this.friendly(post) : this.friendly(post));
      const near = post.position.distanceToSquared(unit.position) < 10000;
      if (ok || !near || attempt >= 10) break;
    }
    this.post = post;
    _q.copy(post.position);
    _q.y += 20;
    this.helper?.goto?.(_q);
  }

  // Provisionally: held by the unit's side or an ally.
  friendly(post) {
    return this.ai.relation(this.unit.team, post.team) > 0;
  }

  // One landing hint near, every 3 s.
  findHint() {
    const now = this.now();
    if (!(now > this.searchAt)) return false;
    this.hint = null;
    this.searchAt = now + 3;
    const query = this.ai.nav?.queryHints;
    if (!query) return false;
    const unit = this.unit;
    const found = query({ mask: 32, center: unit.position, halfExtent: 80, cap: 1, requester: this.controller, now }) ?? [];
    if (!found.length) return false;
    const hint = found[this.ai.randomA.int(0, found.length - 1)];
    if (!hint.commandPost) return false;
    if (this.transport && (this.flag ? this.friendly(hint.commandPost) : !this.friendly(hint.commandPost))) return false;
    if (hint.position.y > unit.position.y) return false;
    this.hint = hint;
    return true;
  }

  // State 5's entry: the landing point L in the hint's frame, and the
  // approach to it.
  approach() {
    const hint = this.hint;
    const unit = this.unit;
    if (!hint) {
      this.requestState(4);
      return;
    }
    serviceOf(this.ai)?.holdHint?.(hint, this.controller);
    const c = unit.classDef ?? {};
    const time = c.LandingTime ?? 5;
    const up = 0.5 * time * (c.LandingSpeed ?? 3) + (c.LandedHeight ?? 0);
    const back = -0.5 * time * (c.MinSpeed ?? 10);
    const L = _p.copy(hint.position).addScaledVector(hint.up ?? DOWN_UP, up).addScaledVector(hint.forward, back);
    _d.set(hint.forward.x, -0.2, hint.forward.z);
    const A = _q.copy(L).addScaledVector(_d, -2 * (c.MaxSpeed ?? 30));
    const P = unit.position;
    this.helper?.followPath?.([[P.x, P.y, P.z], [A.x, A.y, A.z], [L.x, L.y, L.z]], true, true);
    this.helper?.land?.(true);
  }

  landed() {
    const vehicles = serviceOf(this.ai) ?? this.ai.vehicles;
    const now = this.now();
    const post = this.hint?.commandPost ?? this.post ?? null;
    if (this.flag) vehicles?.disembarkPassengers?.(this.controller, post);
    if (!this.transport) {
      if (this.hint) vehicles?.closeHint?.(this.hint, now + 20);
      vehicles?.requestExit?.(this.controller, post);
      this.controller.reboardAt = now + 15;
    }
    if (this.transport || !this.flag) {
      this.flag = !this.flag;
      this.requestState(3);
    }
  }

  // The pilot's policies: no danger, alerts, sounds or boarding; a scanner
  // target only outside the climb-out and landing states and never for a
  // transport; the crew wait yes; commands and recovery as the base; the
  // interrupted goal is saved.
  acceptsHazard() {
    return false;
  }

  acceptsAlert() {
    return false;
  }

  acceptsTarget(target) {
    if (this.state === 2 || this.state === 5 || this.transport) return false;
    return this.canEngage(target);
  }

  acceptsAcoustic() {
    return false;
  }

  acceptsBoard() {
    return false;
  }

  acceptsSecondaryWait() {
    return true;
  }

  savesOnInterrupt() {
    return true;
  }
}

const hitScratch = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };

export { refOf };
