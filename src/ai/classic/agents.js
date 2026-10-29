import * as THREE from "three";
import { COMBAT_MODE, DONE, Fsm, GOAL, SourceGoal, refOf, resolveRef } from "./controller.js";
import { InterruptHelper } from "./interrupts.js";
import { PatrolHelper, arrivedAtObjective, cpCenter, cpOffset } from "./patrol.js";
import { COMMAND, SquadCommand, countFollowers, currentControllable, sendCommand } from "./squads.js";

// Classic agents: the behavior each controller state runs
// (docs/ai-controller.md).
//
// Every agent is a state machine of its own, driven by the controller once
// per scheduler phase. In phase 0 the agent's machine only ticks: its
// behavior runs (interrupt checks first, then the per-state work, then the
// movement helper's tick) and any state it requests waits. In phase 1 the
// machine drains those requests, running exits and entries, and then ticks
// again, which for every agent means the combat target-ranking pass rather
// than behavior. tick() returns whether the agent has reached its terminal
// state; the controller then starts its next queued goal.
//
// Alert and request policies (which interrupts an agent lets through, whether
// an interrupted goal is saved) are per agent and often per agent state;
// each is spelled out below as the 2004 game has it.

const _v = new THREE.Vector3();
const _w = new THREE.Vector3();
const _p = new THREE.Vector3();

// The 2004 game's table sine/cosine: 2048 float samples per turn, linearly
// interpolated, the index truncated toward zero (negative angles extrapolate
// from the wrapped sample, as the original does).
const SIN = new Float32Array(2049);
const COS = new Float32Array(2049);
{
  const step = Math.fround((2 * Math.PI) / 2048);
  for (let i = 0; i < 2048; i++) {
    const a = Math.fround(i * step);
    SIN[i] = Math.fround(Math.sin(a));
    COS[i] = Math.fround(Math.cos(a));
  }
}
const TABLE_SCALE = Math.fround(2048 / (2 * Math.PI));
export function tableSinCos(angle, out = { sin: 0, cos: 0 }) {
  const f32 = Math.fround;
  const t = f32(angle * TABLE_SCALE);
  const whole = Math.trunc(t);
  const frac = f32(t - whole);
  const i = whole & 2047;
  const j = (whole + 1) & 2047;
  const keep = f32(1 - frac);
  out.sin = f32(f32(SIN[i] * keep) + f32(SIN[j] * frac));
  out.cos = f32(f32(COS[i] * keep) + f32(COS[j] * frac));
  return out;
}

// Rotate a direction about +Y by `angle` with the table sine/cosine.
const _sc = { sin: 0, cos: 0 };
export function rotateY(out, v, angle) {
  const { sin, cos } = tableSinCos(angle, _sc);
  const x = v.x * cos - v.z * sin;
  const z = v.z * cos + v.x * sin;
  return out.set(x, v.y, z);
}

// A vector with independent components in [-1, 1): three draws, the first
// becoming z, the second y, the third x.
export function randomCube(random, out) {
  const z = Math.fround(random.uniform() - 0.5);
  const y = Math.fround(random.uniform() - 0.5);
  const x = Math.fround(random.uniform() - 0.5);
  return out.set(x * 2, y * 2, z * 2);
}

function horizontalDistanceSq(a, b) {
  const dx = a.x - b.x;
  const dz = a.z - b.z;
  return dx * dx + dz * dz;
}

// Unit classifier helpers (unit.kind is the classifier ID).
export const isSoldier = (unit) => unit?.kind === 0;
export const isDroid = (unit) => unit?.kind === 1;
export const isDroideka = (unit) => unit?.kind === 3;
export const isFlyer = (unit) => unit?.kind === 5;
export const isWalker = (unit) => unit?.kind === 2;

// Nominal class forward speed (what the 2004 game's speed getter returns:
// class data, not current velocity).
export function nominalSpeed(unit) {
  if (!unit) return 0;
  const s = unit.soldier;
  return s?.maxSpeed ?? unit.classDef?.MaxSpeed ?? unit.classDef?.ForwardSpeed ?? 0;
}

// ---------------------------------------------------------------------------
// The degraded movement helper, used only when the navigation code's
// GroundGotoHelper is not bound: straight-line steering to the destination,
// no graph, no barriers. Not Classic behavior.

export class DirectHelper {
  constructor(agent) {
    this.agent = agent;
    this.controller = agent.controller;
    this.target = new THREE.Vector3();
    this.active = false;
    this.arrived = false;
    this.failed = false;
    this.status = 0;
    this.hasPath = false;
    this.radius = 1;
  }

  // mode "route" (default), "direct" or "straight": all straight lines here.
  goto(point) {
    this.target.copy(point);
    this.active = true;
    this.arrived = false;
    this.hasPath = true;
  }

  stop() {
    this.active = false;
    this.hasPath = false;
    this.controller.request.valid = false;
  }

  terminate() {
    this.stop();
  }

  destinationPoint(out) {
    return out.copy(this.target);
  }

  installPath(points) {
    if (points.length) this.goto(points[points.length - 1]);
  }

  tick() {
    const unit = this.controller.unit;
    const request = this.controller.request;
    if (!this.active || !unit) {
      request.valid = false;
      return;
    }
    if (horizontalDistanceSq(unit.position, this.target) < this.radius * this.radius) {
      this.arrived = true;
      this.active = false;
      this.hasPath = false;
      request.valid = false;
      return;
    }
    request.valid = true;
    request.steeringTarget.copy(this.target);
    request.waypoint.copy(this.target);
    request.segmentDirection.subVectors(this.target, unit.position).setY(0).normalize();
  }
}

// ---------------------------------------------------------------------------
// Class helper policy bases. classes.js supplies their behavior machines;
// these remain the degraded fallbacks when that optional module is absent.

export class SnipeHelper extends Fsm {
  constructor(agent, scout = false) {
    super();
    this.agent = agent;
    this.scout = scout;
    this.state = 1;
    this.pending = 1;
    this.hint = null;
    this.stance = 0;
    this.look = false;
    this.lookPoint = new THREE.Vector3();
  }

  // Choose a snipe hint; true sends the agent to its sniping state.
  select() {
    return this.agent.ai.classes?.selectSnipe?.(this.agent, this) ?? false;
  }

  start(hint) {
    this.hint = hint;
    this.requestState(2);
    this.flush();
  }

  tick(dt) {
    this.update(dt, false);
  }

  exitState() {
    this.onExit();
  }

  get busy() {
    return false;
  }

  acceptsHazard() {
    return true;
  }

  acceptsAlert(known, point) {
    return !(this.state === 3 && known && this.inForwardCone(point));
  }

  acceptsTarget(target, agent) {
    if (this.state !== 3) return agent.canEngage(target);
    if (this.inForwardCone(target.position)) return false;
    const unit = agent.unit;
    if (!unit || unit.position.distanceToSquared(target.position) >= 1600) return false;
    return agent.canEngage(target);
  }

  acceptsAcoustic(source, agent) {
    if (this.state !== 3) return agent.canEngage(source);
    return !this.inForwardCone(source.position) && agent.canEngage(source);
  }

  acceptsBoard(candidate, agent) {
    return this.state === 3 ? false : agent.boardPolicy(candidate);
  }

  acceptsSecondaryWait() {
    return this.state !== 3;
  }

  // Occupying a hint, the forward test (strict 3D dot against the hint's
  // forward above cos(1.3526301384 rad)). Without a hint: never.
  inForwardCone(_point) {
    return false;
  }
}

export class MineHelper extends Fsm {
  constructor(agent) {
    super();
    this.agent = agent;
    this.state = 1;
    this.pending = 1;
  }

  // Whether the unit carries a usable mine dispenser.
  eligible() {
    return this.agent.ai.classes?.canMine?.(this.agent) ?? false;
  }

  // Choose a minefield hint and placement; true sends the agent to mining.
  select() {
    return this.agent.ai.classes?.selectMine?.(this.agent, this) ?? false;
  }

  start(hint, placement) {
    this.hint = hint;
    this.placement = placement;
    this.requestState(2);
    this.flush();
  }

  tick(dt) {
    this.update(dt, false);
  }

  exitState() {
    this.onExit();
  }

  acceptsHazard() {
    return true;
  }

  acceptsAlert() {
    return true;
  }

  acceptsTarget(target, agent) {
    return agent.canEngage(target);
  }

  acceptsAcoustic(source, agent) {
    return agent.canEngage(source);
  }

  acceptsBoard() {
    return true;
  }

  acceptsSecondaryWait() {
    return false;
  }
}

// ---------------------------------------------------------------------------
// The agent base (UnitAgent).

export class Agent extends Fsm {
  constructor(controller, goal) {
    super();
    this.controller = controller;
    this.ai = controller.ai;
    this.world = controller.world;
    this.unit = controller.unit;
    this.goal = goal ?? null;
    this.interrupts = new InterruptHelper(this);
    this.helper = this.ai.createHelper(this);
    this.state = 1;
    this.ai.combat?.initAgent?.(this);
  }

  now() {
    return this.world.time ?? 0;
  }

  // Controller-facing tick: phase 0 only ticks the current state (behavior);
  // phase 1 drains requested states and ranks targets. Returns "finished".
  tick(dt, phase) {
    this.update(dt, phase === 0);
    return this.state === DONE;
  }

  // The machine's tick: behavior in phase 0, the target-ranking pass in
  // phase 1.
  onTick(dt) {
    if (this.controller.phase !== 0) {
      this.ai.combat?.rankTargets?.(this);
      return;
    }
    this.behave(dt);
  }

  // Per-state behavior (phase 0). Subclasses.
  behave(_dt) {}

  // Controller state exit: end the movement helper, then this machine.
  exit() {
    if (this.helper) (this.helper.terminate ?? this.helper.stop)?.call(this.helper);
    this.terminate();
  }

  dispose() {
    this.helper?.dispose?.();
  }

  // -- shared steps ---------------------------------------------------------------

  // The full combat target update most behaviors run first (Combat).
  updateCombatTarget() {
    this.ai.combat?.updateTarget?.(this);
  }

  // Aim selection with a posture request: the current combat target, else
  // the remembered point, else `look` when given, else along the path.
  aim(stance = 0, look = null) {
    const controller = this.controller;
    controller.setStance(stance);
    const combat = this.ai.combat;
    const target = combat?.currentTarget?.(this) ?? null;
    if (target) {
      controller.aimAt(refOf(target));
      return;
    }
    const memory = combat?.memoryPoint?.(this) ?? null;
    if (memory) controller.lookAt(memory);
    else if (look) controller.lookAt(look);
    else controller.lookAlongPath();
  }

  tickHelper(dt) {
    this.helper?.tick?.(dt);
  }

  // Weapon/tactical eligibility against an entity (Combat): the base target
  // and acoustic policies.
  canEngage(target) {
    const engage = this.ai.combat?.canEngage;
    return engage ? Boolean(engage(this.controller, target)) : true;
  }

  // Boarding acceptance base policy: a mobile candidate always; a stationary
  // one (zero nominal speed) needs retained target memory and the candidate
  // able to fire at it, or StayInTurrets for non-repair roles.
  boardPolicy(candidate) {
    const vehicles = this.ai.vehicles;
    if (vehicles?.boardPolicy) return Boolean(vehicles.boardPolicy(this, candidate));
    return nominalSpeed(candidate) > 0;
  }

  // -- policies (agent virtuals) ----------------------------------------------------

  // The agent virtuals the interrupt helper consults, one per alert source:
  //   acceptsHazard()            grenade/sticky danger
  //   acceptsAlert(known, point) targeted shot / damage, at the source's current point
  //   acceptsTarget(unit)        the scanner's acquired target
  //   acceptsAcoustic(unit)      an acoustic contact
  //   acceptsBoard(unit)         a vehicle boarding candidate
  //   acceptsSecondaryWait()     the crew-wait window
  //   acceptsCommand(command)    a squad command (before the yes)
  //   acceptsRecovery()          stagnation recovery
  //   savesOnInterrupt(goal)     keep the interrupted goal to resume
  acceptsHazard() {
    return true;
  }

  acceptsAlert() {
    return true;
  }

  acceptsTarget(target) {
    return this.canEngage(target);
  }

  acceptsAcoustic(source) {
    return this.canEngage(source);
  }

  acceptsBoard(candidate) {
    return this.boardPolicy(candidate);
  }

  // Whether the time-limited crew-wait interrupt may run.
  acceptsSecondaryWait() {
    return false;
  }

  // Whether a squad command is taken at all (before the yes acknowledgement).
  acceptsCommand(_command) {
    return true;
  }

  // Whether priority-8 stagnation recovery may run.
  acceptsRecovery() {
    return true;
  }

  // Whether an interrupt saves the current goal to resume later.
  savesOnInterrupt(_goal) {
    return true;
  }

  // The move-out handling for the crew-wait and flyer agents.
  onMoveOut() {}
}

// ---------------------------------------------------------------------------
// State 1: Wait.

export class UnitWaitAgent extends Agent {
  constructor(controller, goal) {
    super(controller, goal);
    this.since = 0;
    this.lookPoint = new THREE.Vector3();
    this.requestState(2);
  }

  onEnter() {
    if (this.state !== 2) return;
    this.since = this.now();
    this.pickLook();
  }

  // A look point 1000 units out along the controller's home direction,
  // turned by up to +-75 degrees (+-30 for stationary controllers).
  pickLook() {
    const controller = this.controller;
    const range = controller.homeFacing ? 0.5235988 : 1.3089969;
    const angle = this.ai.randomA.range(-range, range);
    rotateY(_v, controller.homeDirection, angle);
    const unit = this.unit;
    if (unit) this.lookPoint.copy(unit.position).addScaledVector(_v, 1000);
  }

  behave(dt) {
    if (this.interrupts.tick()) return;
    this.updateCombatTarget();
    if (this.state === 2) {
      if (this.ai.randomA.int(1, 9) === 2) this.pickLook();
      const unit = this.unit;
      const combat = this.ai.combat;
      if (unit && nominalSpeed(unit) === 0) {
        if (!combat?.memoryPoint?.(this)) {
          if (this.now() > this.since + (this.ai.stayDelay?.() ?? 10)) {
            this.controller.requestExit();
            return;
          }
        } else {
          this.since = this.now();
        }
      }
      const target = combat?.currentTarget?.(this) ?? null;
      if (target) this.controller.aimAt(refOf(target));
      else this.controller.lookAt(combat?.memoryPoint?.(this) ?? this.lookPoint);
    }
    this.tickHelper(dt);
  }

  acceptsSecondaryWait() {
    return true;
  }
}

// ---------------------------------------------------------------------------
// State 4: wait for crew (a vehicle driver). The agent's
// bookkeeping is here so the interrupt and command paths are exact.

export class UnitWaitSecondaryAgent extends Agent {
  constructor(controller, goal) {
    super(controller, goal);
    this.requestState(2);
  }

  onEnter() {
    if (this.state !== 2) return;
    this.helper?.stop?.();
    const vehicle = this.ai.vehicles?.boardedVehicle?.(this.controller);
    if (vehicle && this.controller.character) {
      const command = new SquadCommand(this.controller.character, COMMAND.BOARD, this.now(), refOf(vehicle));
      sendCommand(this.ai, command, 0.3);
    }
  }

  behave(dt) {
    if (this.interrupts.tick()) return;
    this.updateCombatTarget();
    if (this.state === 2) {
      const vehicles = this.ai.vehicles;
      const vehicle = vehicles?.boardedVehicle?.(this.controller);
      if (vehicle) {
        const controller = this.controller;
        if (vehicles.seatsFull?.(vehicle)) {
          controller.secondaryWaitUntil = this.now();
          this.requestState(DONE);
        } else if (this.now() > controller.secondaryWaitUntil && !vehicles.pendingReservations?.(vehicle, controller.unit?.team)) {
          this.requestState(DONE);
        }
      }
    }
    this.tickHelper(dt);
  }

  // A flyer driver ends the wait window on any alert.
  flyerCloses() {
    if (!isFlyer(this.unit)) return false;
    this.controller.secondaryWaitUntil = 0;
    return true;
  }

  acceptsHazard() {
    return !this.flyerCloses();
  }

  acceptsAlert() {
    return !this.flyerCloses();
  }

  acceptsTarget(target) {
    return isFlyer(this.unit) ? false : this.canEngage(target);
  }

  acceptsAcoustic(source) {
    return this.flyerCloses() ? false : this.canEngage(source);
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

  // Move out / at ease from anyone ends the wait two seconds ago.
  onMoveOut() {
    this.controller.secondaryWaitUntil = this.now() - 2;
  }
}

// ---------------------------------------------------------------------------
// State 9: stagnation recovery. Random ordinary-input movement until the unit
// is more than 10 m (2 m for a droideka) from where it started and clear of
// planning barriers. No teleport.

export class UnitRandomAgent extends Agent {
  constructor(controller, goal) {
    super(controller, goal);
    this.start = new THREE.Vector3();
    if (this.unit) this.start.copy(this.unit.position);
    this.lastRetry = 0;
    this.retriedAt = new THREE.Vector3();
    if (this.unit) this.retriedAt.copy(this.unit.position);
    this.requestState(2);
  }

  onEnter() {
    if (this.state === 2) this.retry();
  }

  onExit() {
    this.controller.wantedMode = 2;
  }

  // A fresh point: the current position plus a cube offset up to 10 on each
  // axis, as a direct two-point path. A soldier still within a metre of
  // where the last timed retry left it jumps as it sets off. (Port: Classic
  // recovery never jumps, and a soldier in a hollow of steep slopes slid
  // back from every direction it tried, for good.)
  retry(timed = false) {
    const unit = this.unit;
    if (!unit) return;
    if (timed) {
      if (isSoldier(unit) && unit.position.distanceToSquared(this.retriedAt) < 1) this.controller.requestJet(_w.set(0, 0, 0));
      this.retriedAt.copy(unit.position);
    }
    randomCube(this.ai.randomB, _v).multiplyScalar(10);
    _p.copy(unit.position).add(_v);
    this.helper?.goto?.(_p, { mode: "straight" });
    this.lastRetry = this.now();
  }

  clearOfBarriers() {
    const unit = this.unit;
    const nav = this.ai.nav;
    if (!unit) return false;
    const clear = nav?.pointClear;
    if (!clear) {
      this.ai.warnOnce("pointClear", "Nav pointClear is not bound: recovery treats every point as clear of barriers");
      return true;
    }
    return Boolean(clear.call(nav, unit.position, nav.traversalRadius?.(unit) ?? 0.5, nav.traversalMask?.(unit) ?? 1, unit));
  }

  behave(dt) {
    if (this.interrupts.tick()) return;
    this.updateCombatTarget();
    if (this.state === 2) {
      const unit = this.unit;
      this.controller.wantedMode = 1;
      const threshold = isDroideka(unit) ? 2 : 10;
      const moved = unit ? unit.position.distanceToSquared(this.start) : 0;
      if (moved > threshold * threshold && this.clearOfBarriers()) {
        this.requestState(DONE);
      } else {
        const side = this.controller.avoidSide;
        const noPath = !this.controller.hasExecutor;
        const timed = this.now() > this.lastRetry + 3;
        if (side !== 0 || timed || noPath) this.retry(timed);
      }
    }
    this.aim(0);
    this.tickHelper(dt);
  }

  acceptsSecondaryWait() {
    return true;
  }

  acceptsRecovery() {
    return false;
  }

  savesOnInterrupt() {
    return false;
  }
}

// ---------------------------------------------------------------------------
// State 10: Follow a squad leader.

// Follow formation radii: near 0 and far 3 unless a mission sets them.
export const FOLLOW_RADII = { near: 0, far: 3 };

export class UnitFollowAgent extends Agent {
  constructor(controller, goal) {
    super(controller, goal);
    this.leader = goal?.refA ?? null;
    this.leaderPoint = new THREE.Vector3();
    this.countdown = 1;
    this.requestState(2);
  }

  leaderUnit() {
    return resolveRef(this.leader);
  }

  onEnter() {
    const controller = this.controller;
    const leader = this.leaderUnit();
    switch (this.state) {
      case 2:
        if (!leader) return;
        this.leaderPoint.copy(leader.position);
        this.helper?.goto?.(leader.position);
        this.setLeader(this.leader);
        this.helper?.followTrail?.(true);
        return;
      case 4: {
        if (!leader) return;
        // Stand off the leader by a random flat offset up to the far radius.
        randomCube(this.ai.randomB, _v).multiplyScalar(this.ai.followRadii.far);
        _v.y = 0;
        if (_v.lengthSq() < 1) _v.normalize();
        _p.copy(leader.position).add(_v);
        const nav = this.ai.nav;
        if (nav?.clipSegment && this.unit) nav.clipSegment(leader.position, _p, nav.traversalRadius?.(this.unit) ?? 0.5, nav.traversalMask?.(this.unit) ?? 1);
        this.installHold(_p);
        this.helper?.holdPath?.(true);
        return;
      }
      case DONE:
        this.setLeader(null);
        return;
      default:
    }
  }

  // The control's follow leader (what the follower count reads). Setting it
  // also drops the control's current path (Nav), and clearing it drops the
  // recorded trail.
  setLeader(ref) {
    this.controller.leader = ref;
    this.helper?.setLeader?.(ref);
  }

  onExit() {
    switch (this.state) {
      case 1:
        if (!this.leaderUnit()) this.requestState(DONE);
        return;
      case 2:
        this.helper?.followTrail?.(false);
        return;
      case 4:
        this.helper?.holdPath?.(false);
        return;
      default:
    }
  }

  // A path of two identical points: stand here.
  installHold(point) {
    const helper = this.helper;
    if (helper?.installPath) helper.installPath([point.clone(), point.clone()]);
    else helper?.goto?.(point);
  }

  behave(dt) {
    if (this.interrupts.tick()) return;
    this.updateCombatTarget();
    const unit = this.unit;
    if (this.state !== DONE) {
      const leader = this.leaderUnit();
      if (!leader) {
        this.requestState(DONE);
        return;
      }
      if (countFollowers(this.ai, leader) > 3) this.requestState(DONE);
      const vehicle = this.ai.vehicles?.boardedVehicleOf?.(leader) ?? null;
      if (vehicle) {
        if (!isDroideka(unit)) this.controller.commands.prependOne(SourceGoal.board(refOf(vehicle)));
        this.requestState(DONE);
        return;
      }
      if (unit && leader.position.distanceToSquared(unit.position) > 400) {
        this.countdown = Math.fround(this.countdown - 0.05);
        if (this.countdown <= 0) this.requestState(DONE);
      }
    }
    const radii = this.ai.followRadii;
    const leader = this.leaderUnit();
    switch (this.state) {
      case 2:
        if (unit && horizontalDistanceSq(unit.position, this.leaderPoint) < 0.2) {
          this.requestState(3);
          this.aim(0);
          return;
        }
        this.tickHelper(dt);
        break;
      case 3:
        if (this.controller.executorStatus !== 0) {
          this.requestState(2);
          this.aim(0);
          return;
        }
        if (leader && unit) {
          const near = radii.near + 5;
          if (leader.position.distanceToSquared(unit.position) < near * near) {
            this.requestState(4);
            this.aim(0);
            return;
          }
        }
        break;
      case 4:
        if (this.controller.executorStatus !== 0) {
          if (unit) this.installHold(unit.position);
          this.aim(0);
          return;
        }
        if (leader && unit) {
          const far = radii.far + 6;
          if (far * far < leader.position.distanceToSquared(unit.position)) {
            this.requestState(3);
            this.aim(0);
            return;
          }
        }
        break;
      default:
    }
    this.aim(0);
  }

  acceptsHazard() {
    return false;
  }

  acceptsAlert() {
    return false;
  }

  acceptsTarget() {
    return false;
  }

  acceptsAcoustic() {
    return false;
  }

  acceptsBoard() {
    return false;
  }

  acceptsCommand(command) {
    return command.kind !== COMMAND.FOLLOW;
  }

  // Interrupted by anything but another follow/hold: resume afterwards.
  savesOnInterrupt(goal) {
    return !(goal.kind === GOAL.FOLLOW || goal.kind === GOAL.HOLD);
  }
}

// ---------------------------------------------------------------------------
// State 11: Hold position. Holding still fights.

export class UnitHoldAgent extends Agent {
  constructor(controller, goal) {
    super(controller, goal);
    this.countdown = 1;
    this.lookPoint = new THREE.Vector3();
    this.requestState(2);
  }

  onEnter() {
    if (this.state === 2) this.pickLook();
  }

  // Ten units out: along the home direction turned up to +-30 degrees for a
  // stationary controller, otherwise a random flat direction.
  pickLook() {
    const unit = this.unit;
    if (!unit) return;
    const controller = this.controller;
    const random = this.ai.randomA;
    if (controller.homeFacing) {
      rotateY(_v, controller.homeDirection, random.range(-0.5235988, 0.5235988));
      this.lookPoint.copy(unit.position).addScaledVector(_v, 10);
      return;
    }
    randomCube(random, _v).multiplyScalar(10);
    _v.y = 0;
    this.lookPoint.copy(unit.position).add(_v);
  }

  behave(dt) {
    if (this.state === DONE) return;
    if (this.interrupts.tick()) return;
    this.updateCombatTarget();
    // The issuer of the last handled command (the hold order), at its
    // current control: its vehicle when it sits in one.
    const source = this.controller.lastCommand?.source ?? null;
    const sourceUnit = source ? currentControllable(this.ai, source) : null;
    const unit = this.unit;
    if (!sourceUnit || !unit) {
      this.requestState(DONE);
      return;
    }
    if (sourceUnit.position.distanceToSquared(unit.position) > 400) {
      this.countdown = Math.fround(this.countdown - 0.05);
      if (this.countdown <= 0) {
        this.requestState(DONE);
        return;
      }
    }
    if (this.state === 2) {
      if (this.ai.randomA.int(1, 9) === 2) this.pickLook();
      const combat = this.ai.combat;
      const target = combat?.currentTarget?.(this) ?? null;
      if (target) this.controller.aimAt(refOf(target));
      else this.controller.lookAt(combat?.memoryPoint?.(this) ?? this.lookPoint);
    }
    this.tickHelper(dt);
  }

  acceptsBoard() {
    return false;
  }

  savesOnInterrupt(goal) {
    return !(goal.kind === GOAL.FOLLOW || goal.kind === GOAL.HOLD);
  }
}

// ---------------------------------------------------------------------------
// States 5/6: boarding and leaving vehicles. These stubs keep
// the policies exact and finish at once when no vehicle service is bound.

export class UnitBoardAgent extends Agent {
  constructor(controller, goal) {
    super(controller, goal);
    this.vehicle = goal?.refA ?? null;
    this.requestState(2);
  }

  onEnter() {
    if (this.state !== 2) return;
    const vehicles = this.ai.vehicles;
    const vehicle = resolveRef(this.vehicle);
    if (!vehicles?.reserveSeat || !vehicle || !vehicles.reserveSeat(this.controller, vehicle)) {
      this.requestState(DONE);
      return;
    }
    this.helper?.goto?.(vehicle.position);
  }

  behave(dt) {
    if (this.interrupts.tick()) return;
    this.updateCombatTarget();
    this.ai.vehicles?.boardTick?.(this, dt);
    this.aim(0);
    this.tickHelper(dt);
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

export class UnitUnBoardAgent extends Agent {
  constructor(controller, goal) {
    super(controller, goal);
    this.requestState(2);
  }

  onEnter() {
    if (this.state === 1) this.controller.requestExit();
  }

  behave(dt) {
    if (this.interrupts.tick()) return;
    this.updateCombatTarget();
    const vehicles = this.ai.vehicles;
    if (!vehicles?.boarded?.(this.controller)) this.requestState(DONE);
    this.tickHelper(dt);
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
// State 3: the objective agents. The trooper drives three embedded behavior
// machines (patrol in state 2, sniping in 4, mine laying in 5); state 3 walks
// to the command post and decides between them.

export class UnitTrooperAgent extends Agent {
  constructor(controller, goal) {
    super(controller, goal);
    this.post = goal?.post ?? controller.objective ?? null;
    this.patrol = new PatrolHelper(this);
    // index.js supplies classes.js's behavior helpers when loaded.
    const helpers = this.ai.helperTypes ?? {};
    this.snipe = new (helpers.snipe ?? SnipeHelper)(this, false);
    this.mine = new (helpers.mine ?? MineHelper)(this);
    this.startPosition = new THREE.Vector3();
    this.center = new THREE.Vector3();
    this.offset = new THREE.Vector3();
    this.entryTime = 0;
    this.state = 1;
    this.requestState(1);
  }

  arrived(offset = null) {
    return arrivedAtObjective(this.ai, this.unit, this.post, offset);
  }

  // Leave for the post with a computed goal point (center plus the post's
  // random offset).
  approach() {
    const unit = this.unit;
    if (unit) this.startPosition.copy(unit.position);
    cpCenter(this.ai, this.post, this.center);
    // A pilot whose hull cannot stand on the post goes where it can fight
    // for it (port; vehicles/support.js): the offset then reaches that
    // point.
    this.support = this.ai.vehicles?.supportPoint?.(unit, this.post, this.helper, _p) ? true : false;
    if (this.support) this.offset.subVectors(_p, this.center);
    else cpOffset(this.ai, this.post, this.offset);
    this.entryTime = this.now();
    _p.copy(this.center).add(this.offset);
    this.helper?.goto?.(_p);
  }

  // A moving objective (a command post on a vehicle) drifted more than 20
  // units since the approach began, 15 seconds have passed, and the unit is
  // moving away from it or has arrived without a stuck executor.
  objectiveMoved() {
    const unit = this.unit;
    if (!unit || !this.post) return false;
    cpCenter(this.ai, this.post, _w);
    if (_w.distanceToSquared(this.center) <= 400) return false;
    if (!(this.entryTime + 15 < this.now())) return false;
    _v.subVectors(_w, unit.position);
    _p.subVectors(unit.position, this.startPosition);
    if (_v.dot(_p) < 0) return true;
    return Boolean(this.helper?.arrived) && this.controller.executorStatus === 0;
  }

  // Soldiers that are not heroes may take a snipe hint.
  canSnipe() {
    const unit = this.unit;
    return isSoldier(unit) && !unit.hero && this.snipe.select();
  }

  canMine() {
    return this.mine.eligible() && this.mine.select();
  }

  onEnter() {
    switch (this.state) {
      case 1:
        if (!this.arrived()) {
          this.requestState(3);
          // A one-in-three-ish "follow me": VO chatter from AI (not delivered).
          const character = this.controller.character;
          if (character) sendCommand(this.ai, new SquadCommand(character, COMMAND.FOLLOW, this.now(), refOf(this.unit)), 0.3);
        } else {
          this.requestState(2);
        }
        return;
      case 2:
        this.patrol.start(this.post);
        return;
      case 3:
        this.mineTries = 0;
        this.approach();
        return;
      case 4:
        this.snipe.start(this.snipe.hint);
        return;
      case 5:
        this.mine.start(this.mine.hint, this.mine.placement);
        return;
      default:
    }
  }

  onExit() {
    switch (this.state) {
      case 2:
        this.patrol.exitState();
        return;
      case 4:
        this.snipe.exitState();
        return;
      case 5:
        this.mine.exitState();
        return;
      default:
    }
  }

  exit() {
    this.patrol.terminate();
    this.snipe.terminate();
    this.mine.terminate();
    super.exit();
  }

  behave(dt) {
    if (this.interrupts.tick()) return;
    const state = this.state;
    if ((state > 0 && state < 5) || state === DONE) this.updateCombatTarget();
    switch (state) {
      case 2: {
        const unit = this.unit;
        if (isWalker(unit) && this.ai.vehicles?.isAtAt?.(unit)) {
          this.helper?.stop?.();
          break;
        }
        if (this.canSnipe()) {
          this.requestState(4);
          break;
        }
        this.patrol.tick(dt);
        if (this.patrol.look) this.aim(this.patrol.stance, this.patrol.lookPoint);
        else this.aim(this.patrol.stance);
        break;
      }
      case 3:
        if (!this.arrived(this.offset) && !this.helper?.arrived) {
          if (this.objectiveMoved()) {
            this.requestState(3);
            break;
          }
          if (this.canSnipe()) {
            this.requestState(4);
            break;
          }
          if (this.canMine()) {
            this.requestState(5);
            break;
          }
          this.aim(0);
          break;
        }
        this.requestState(2);
        if (this.controller.character) {
          sendCommand(this.ai, new SquadCommand(this.controller.character, COMMAND.MOVE_OUT, this.now()), 1);
        }
        break;
      case 4:
        this.snipe.tick(dt);
        if (this.snipe.done) {
          this.requestState(3);
          break;
        }
        if (this.snipe.busy) break;
        if (this.snipe.look) this.aim(this.snipe.stance, this.snipe.lookPoint);
        else this.aim(this.snipe.stance);
        break;
      case 5:
        this.mine.tick(dt);
        if (!this.mine.done) {
          if (this.mine.state === 3 && this.mine.aimPoint) this.controller.lookAt(this.mine.aimPoint);
          else this.controller.lookAlongPath();
          this.controller.setStance(0);
          break;
        }
        this.requestState(3);
        break;
      default:
    }
    this.tickHelper(dt);
  }

  // -- policies by state --------------------------------------------------------

  // The embedded machine that answers for this state, if any.
  activeHelper() {
    switch (this.state) {
      case 2:
        return this.patrol;
      case 4:
        return this.snipe;
      case 5:
        return this.mine;
      default:
        return null;
    }
  }

  // The walking-to-the-objective state, where a charge suppresses alerts.
  get approachState() {
    return 3;
  }

  charging() {
    return this.state === this.approachState && this.interrupts.charging();
  }

  // The active helper's answer for one policy (its arguments, then this
  // agent). A helper without that method (an injected class helper from a
  // later wave) gets the agent's base policy instead, with a warning, rather
  // than failing mid-match.
  helperPolicy(helper, name, fallback, ...args) {
    const fn = helper[name];
    if (typeof fn === "function") return Boolean(fn.call(helper, ...args, this));
    const type = helper.constructor?.name ?? "helper";
    this.ai.warnOnce(`policy:${type}.${name}`, `${type} has no ${name}(): using the agent's base policy`);
    return Boolean(fallback());
  }

  acceptsHazard() {
    const helper = this.activeHelper();
    if (helper) return this.helperPolicy(helper, "acceptsHazard", () => true);
    return !this.charging();
  }

  acceptsAlert(known, point) {
    const helper = this.activeHelper();
    if (helper) return this.helperPolicy(helper, "acceptsAlert", () => true, known, point);
    return !this.charging();
  }

  acceptsTarget(target) {
    const helper = this.activeHelper();
    if (helper) return this.helperPolicy(helper, "acceptsTarget", () => this.canEngage(target), target);
    if (this.charging()) return false;
    return this.canEngage(target);
  }

  acceptsAcoustic(source) {
    const helper = this.activeHelper();
    if (helper) return this.helperPolicy(helper, "acceptsAcoustic", () => this.canEngage(source), source);
    if (this.charging()) return false;
    return this.canEngage(source);
  }

  acceptsBoard(candidate) {
    const helper = this.activeHelper();
    if (helper) return this.helperPolicy(helper, "acceptsBoard", () => this.boardPolicy(candidate), candidate);
    return this.boardPolicy(candidate);
  }

  acceptsSecondaryWait() {
    const helper = this.activeHelper();
    return helper ? this.helperPolicy(helper, "acceptsSecondaryWait", () => false) : true;
  }
}

// The scout (role 0): the trooper's machine with a scout-configured snipe
// helper and a 60..100 s sniping deadline; it neither announces "follow me"
// nor "move out", checks sniping before arrival in state 3, and does not
// consult the movement helper's arrival there.
export class UnitScoutAgent extends UnitTrooperAgent {
  constructor(controller, goal) {
    super(controller, goal);
    this.snipe = new (this.ai.helperTypes?.snipe ?? SnipeHelper)(this, true);
    this.deadline = 0;
  }

  onEnter() {
    switch (this.state) {
      case 1:
        this.requestState(this.arrived() ? 2 : 3);
        return;
      case 4:
        this.snipe.start(this.snipe.hint);
        this.deadline = this.now() + this.ai.randomA.range(60, 100);
        return;
      default:
        super.onEnter();
    }
  }

  behave(dt) {
    if (this.interrupts.tick()) return;
    const state = this.state;
    if (state === 1 || state === 2 || state === 3 || state === DONE) this.updateCombatTarget();
    else if (state === 4) this.ai.combat?.updateTargetSelection?.(this);
    switch (state) {
      case 2:
        this.patrol.tick(dt);
        if (this.patrol.look) this.aim(this.patrol.stance, this.patrol.lookPoint);
        else this.aim(this.patrol.stance);
        break;
      case 3:
        if (this.canSnipe()) {
          this.requestState(4);
          break;
        }
        if (this.arrived(this.offset)) {
          this.requestState(2);
          break;
        }
        if (this.objectiveMoved()) {
          this.requestState(3);
          break;
        }
        if (this.canMine()) {
          this.requestState(5);
          break;
        }
        this.aim(0);
        break;
      case 4:
        this.snipe.tick(dt);
        if (!this.snipe.done && this.now() <= this.deadline) {
          if (!this.snipe.busy) {
            if (this.snipe.look) this.aim(this.snipe.stance, this.snipe.lookPoint);
            else this.aim(this.snipe.stance);
          }
          this.ai.combat?.gateFire?.(this);
          break;
        }
        this.requestState(3);
        break;
      case 5:
        this.mine.tick(dt);
        if (!this.mine.done) {
          if (this.mine.state === 3 && this.mine.aimPoint) this.controller.lookAt(this.mine.aimPoint);
          else this.controller.lookAlongPath();
          this.controller.setStance(0);
          break;
        }
        this.requestState(3);
        break;
      default:
    }
    this.tickHelper(dt);
  }

  canSnipe() {
    return this.snipe.select();
  }

  acceptsSecondaryWait() {
    return true;
  }
}

// The repair/support/pilot agent (roles 3 and 4). States: 2 walk to the
// objective, 3 service a repair/resupply job, 4 patrol, 5 snipe, 6 mines.
// Every 5 seconds it asks classes.js for a repair/resupply job. Without that
// module its fallback patrols in state 4.
export class UnitRepairAgent extends UnitTrooperAgent {
  constructor(controller, goal) {
    super(controller, goal);
    this.jobAt = -1;
    this.job = null;
  }

  // Every five seconds (strictly after the stored time) search for a job.
  jobCheck() {
    if (!(this.jobAt <= this.now() && this.now() !== this.jobAt)) return false;
    this.jobAt = this.now() + 5;
    const find = this.ai.classes?.findRepairJob;
    this.job = find ? find(this) : null;
    return Boolean(this.job);
  }

  activeHelper() {
    switch (this.state) {
      case 4:
        return this.patrol;
      case 5:
        return this.snipe;
      case 6:
        return this.mine;
      default:
        return null;
    }
  }

  get approachState() {
    return 2;
  }

  onEnter() {
    switch (this.state) {
      case 1:
        this.requestState(this.arrived() ? 4 : 2);
        return;
      case 2:
        if (this.jobCheck()) {
          this.requestState(3);
          return;
        }
        this.mineTries = 0;
        this.approach();
        return;
      case 3:
        this.ai.classes?.startRepairService?.(this);
        return;
      case 4:
        if (this.jobCheck()) {
          this.requestState(3);
          return;
        }
        this.patrol.start(this.post);
        return;
      case 5:
        this.snipe.start(this.snipe.hint);
        return;
      case 6:
        this.mine.start(this.mine.hint, this.mine.placement);
        return;
      default:
    }
  }

  onExit() {
    switch (this.state) {
      case 4:
        this.patrol.exitState();
        return;
      case 5:
        this.snipe.exitState();
        return;
      case 6:
        this.mine.exitState();
        return;
      default:
    }
  }

  behave(dt) {
    if (this.interrupts.tick()) return;
    const state = this.state;
    if (state === 1 || state === 2 || state === 4 || state === 5 || state === DONE) this.updateCombatTarget();
    switch (state) {
      case 2:
        if (this.jobCheck()) {
          this.requestState(3);
          break;
        }
        if (this.arrived(this.offset)) {
          this.requestState(4);
          break;
        }
        if (this.objectiveMoved()) {
          this.requestState(2);
          break;
        }
        if (isSoldier(this.unit) && this.snipe.select()) {
          this.requestState(5);
          break;
        }
        if (this.canMine()) {
          this.requestState(6);
          break;
        }
        this.aim(0);
        break;
      case 3: {
        const service = this.ai.classes?.repairService;
        const result = service ? service(this, dt) : null;
        if (!result) {
          this.requestState(2);
          break;
        }
        if (result.near && result.target) {
          if (result.target.alive) this.controller.aimAt(refOf(result.target));
          else this.controller.lookAt(result.target.aimPoint?.(_p) ?? result.target.position);
          this.controller.setStance(0);
        } else {
          this.aim(0);
        }
        break;
      }
      case 4:
        if (isSoldier(this.unit) && this.snipe.select()) {
          this.requestState(5);
          break;
        }
        this.patrol.tick(dt);
        if (this.jobCheck()) {
          this.requestState(3);
          break;
        }
        if (this.patrol.look) this.aim(this.patrol.stance, this.patrol.lookPoint);
        else this.aim(this.patrol.stance);
        break;
      case 5:
        this.snipe.tick(dt);
        if (this.snipe.done) {
          this.requestState(2);
          break;
        }
        if (!this.snipe.busy) {
          if (this.snipe.look) this.aim(this.snipe.stance, this.snipe.lookPoint);
          else this.aim(this.snipe.stance);
        }
        break;
      case 6:
        this.mine.tick(dt);
        if (this.jobCheck()) {
          this.requestState(3);
          break;
        }
        if (this.mine.done) {
          this.requestState(2);
          break;
        }
        if (this.mine.state === 3 && this.mine.aimPoint) this.controller.lookAt(this.mine.aimPoint);
        else this.controller.lookAlongPath();
        this.controller.setStance(0);
        break;
      default:
    }
    this.tickHelper(dt);
  }

  // Outside its helpers a repair agent answers scanner targets only inside
  // 25 m (horizontal), and not while charging in state 2.
  acceptsTarget(target) {
    const helper = this.activeHelper();
    if (helper) return this.helperPolicy(helper, "acceptsTarget", () => this.canEngage(target), target);
    const unit = this.unit;
    if (!unit || !target || horizontalDistanceSq(unit.position, target.position) >= 625) return false;
    if (this.charging()) return false;
    return this.canEngage(target);
  }

  acceptsSecondaryWait() {
    return true;
  }
}

// The combat agent lives in the combat module. Until it is bound this
// stand-in finishes at once, so the controller resumes the saved goal.
export class MissingCombatAgent extends Agent {
  constructor(controller, goal) {
    super(controller, goal);
    this.requestState(DONE);
  }

  savesOnInterrupt() {
    return false;
  }
}

// The default state-to-agent table (index.js lets siblings replace entries).
export const DEFAULT_AGENTS = {
  wait: UnitWaitAgent,
  combat: MissingCombatAgent,
  trooper: UnitTrooperAgent,
  scout: UnitScoutAgent,
  repair: UnitRepairAgent,
  waitSecondary: UnitWaitSecondaryAgent,
  board: UnitBoardAgent,
  unboard: UnitUnBoardAgent,
  random: UnitRandomAgent,
  follow: UnitFollowAgent,
  hold: UnitHoldAgent,
};

export { COMBAT_MODE };
