import * as THREE from "three";
import { COMBAT_MODE, DONE, GOAL, SourceGoal, refOf, resolveRef } from "./controller.js";
import { COMMAND, countFollowers, currentControllable, emitVo } from "./squads.js";

// The interrupt helper every agent runs at the start of its behavior
// (docs/ai-controller.md).
//
// Eight checks in strict order; the first that succeeds builds a source goal
// and the rest are skipped:
//
//   1 grenade/sticky danger at a point (soldiers only) -> combat, evade mode
//   2 targeted shot or damage from a Character        -> combat at the source's
//     current position when in sensor range and engageable, else (soldiers) an
//     evade point within a unit of itself
//   3 the scanner's acquired target                   -> combat on that target
//   4 acoustic contacts (moving/firing enemies nearby) -> combat at the contact
//   5 a squad command addressed to this bot           -> hold/follow/board/unboard
//   6 a vehicle boarding candidate (vehicles)         -> board
//   7 the time-limited crew-wait window (vehicles)    -> wait for crew
//   8 a stuck movement executor (soldier/droideka)    -> random-movement recovery
//
// A successful check then either prepends the new goal alone, or, when the
// running agent saves on interrupt, prepends the new goal followed by a copy
// of the current one so the interrupted goal resumes afterwards. Both
// insertions are refused while another insertion is pending.

export const PRIORITY = Object.freeze({
  DANGER: 1,
  SHOT: 2,
  TARGET: 3,
  ACOUSTIC: 4,
  COMMAND: 5,
  BOARD: 6,
  WAIT_SECONDARY: 7,
  RECOVERY: 8,
});

const _self = new THREE.Vector3();
const _point = new THREE.Vector3();
const _cube = new THREE.Vector3();

const kindOf = (unit) => unit?.kind;
const isFollowOrHold = (kind) => kind === GOAL.FOLLOW || kind === GOAL.HOLD;

function horizontalDistanceSq(a, b) {
  const dx = a.x - b.x;
  const dz = a.z - b.z;
  return dx * dx + dz * dz;
}

function cube(random, out) {
  const z = Math.fround(random.uniform() - 0.5);
  const y = Math.fround(random.uniform() - 0.5);
  const x = Math.fround(random.uniform() - 0.5);
  return out.set(x * 2, y * 2, z * 2);
}

function nominalSpeed(unit) {
  return unit?.soldier?.maxSpeed ?? unit?.classDef?.MaxSpeed ?? 0;
}

export class InterruptHelper {
  constructor(agent) {
    this.agent = agent;
    this.checkedAt = 0; // last time the charge test ran
    this.charge = false; // its cached result
    this.last = 0; // priority of the last success (debug)
  }

  get controller() {
    return this.agent.controller;
  }

  get ai() {
    return this.agent.ai;
  }

  now() {
    return this.agent.now();
  }

  // Run the checks; true when one fired (its goal is already queued).
  tick() {
    const out = { goal: null };
    let priority = 0;
    if (this.danger(out)) priority = PRIORITY.DANGER;
    else if (this.shot(out)) priority = PRIORITY.SHOT;
    else if (this.target(out)) priority = PRIORITY.TARGET;
    else if (this.acoustic(out)) priority = PRIORITY.ACOUSTIC;
    else if (this.command(out)) priority = PRIORITY.COMMAND;
    else if (this.boarding(out)) priority = PRIORITY.BOARD;
    else if (this.secondaryWait(out)) priority = PRIORITY.WAIT_SECONDARY;
    else if (this.recovery(out)) priority = PRIORITY.RECOVERY;
    if (!priority) return false;
    this.last = priority;
    this.push(out.goal);
    this.controller.trace("interrupt", { priority, kind: out.goal.kind, mode: out.goal.mode });
    return true;
  }

  // Queue the interrupt's goal, saving the current goal when the agent wants
  // to resume it.
  push(goal) {
    const commands = this.controller.commands;
    if (this.agent.savesOnInterrupt(goal)) commands.prependTwo(commands.currentCopy(), goal);
    else commands.prependOne(goal);
  }

  // -- shared gates ------------------------------------------------------------------

  // Priorities 1-4 first pass a gate: a class flag that only stock vehicle
  // classes set can mute them, and a flyer inside an authored danger box
  // (expanded by its traversal radius, at least 15) takes none.
  alertsOpen() {
    const ai = this.ai;
    const controller = this.controller;
    if (ai.vehicles?.alertsBlocked?.(controller)) return false;
    const unit = controller.unit;
    if (kindOf(unit) === 5) {
      const nav = ai.nav;
      const padding = Math.max(15, nav?.traversalRadius?.(unit) ?? 0);
      if (nav?.inDangerRegion?.(unit.position, padding)) return false;
    }
    return true;
  }

  // The common weapon gate: no selected weapon, no reaction.
  armed() {
    const has = this.ai.combat?.hasWeapon;
    return has ? Boolean(has(this.controller)) : true;
  }

  // A turret that cannot swing onto the point asks to get out instead.
  turretCannotReach(point) {
    const ai = this.ai;
    const unit = this.controller.unit;
    if (!unit || nominalSpeed(unit) !== 0 || ai.stayInTurrets?.()) return false;
    const reach = ai.vehicles?.mountReach;
    return reach ? !reach(unit, point) : false;
  }

  // -- 1: grenade/sticky danger --------------------------------------------------------

  // Combat publishes the perception side of priorities 1, 2 and 4 (range,
  // weapon gate, engageability) as response functions returning
  // { mode, point, exit, alert } or null; when bound they decide, and the
  // goal and its exit/VO side effects are built here.
  adopt(result, out) {
    if (!result) return false;
    out.goal = SourceGoal.combat(result.mode ?? COMBAT_MODE.LOCATION, { point: result.point ?? null, target: result.target ?? null });
    const unit = this.controller.unit;
    if (result.alert && kindOf(unit) === 0) emitVo(this.ai, unit, { event: "heardEnemy", property: "HeardEnemySound" });
    if (result.exit) {
      this.controller.requestExit();
      return false;
    }
    return true;
  }

  danger(out) {
    if (!this.alertsOpen()) return false;
    const respond = this.ai.combat?.hazardResponse;
    if (respond) return this.adopt(respond(this.controller, this.agent), out);
    const controller = this.controller;
    if (!controller.danger.pending) return false;
    if (!this.armed() || !this.agent.acceptsHazard()) return false;
    if (kindOf(controller.unit) !== 0) return false;
    out.goal = SourceGoal.combat(COMBAT_MODE.EVADE, { point: controller.danger.point });
    return true;
  }

  // -- 2: targeted shot / damage ---------------------------------------------------------

  shot(out) {
    if (!this.alertsOpen()) return false;
    const respond = this.ai.combat?.alertResponse;
    if (respond) return this.adopt(respond(this.controller, this.agent), out);
    const exit = { request: false };
    const fired = this.shotResponse(out, exit);
    if (exit.request) {
      this.controller.requestExit();
      return false;
    }
    return fired;
  }

  shotResponse(out, exit) {
    const ai = this.ai;
    const controller = this.controller;
    const unit = controller.unit;
    const alert = controller.shot;
    if (!alert.pending || !alert.source || !unit) return false;
    // The response point is where the source's current control is now (its
    // vehicle or remote when it has one), not the remembered alert point.
    const source = currentControllable(ai, alert.source);
    if (!source) return false;
    _self.copy(unit.position);
    _point.copy(source.position);
    if (!this.armed() || !this.agent.acceptsAlert(alert.known, _point)) return false;
    const sensor = ai.combat?.sensorRange;
    const range = sensor ? sensor(controller, source) : ai.fallbackSensorRange;
    if (_point.distanceToSquared(_self) < range * range) {
      const engage = ai.combat?.canEngage;
      if (!engage || engage(controller, source)) {
        out.goal = SourceGoal.combat(COMBAT_MODE.LOCATION, { point: _point });
        if (this.turretCannotReach(_point)) exit.request = true;
        return true;
      }
    }
    if (kindOf(unit) === 0) {
      cube(ai.randomB, _cube).add(_self);
      out.goal = SourceGoal.combat(COMBAT_MODE.EVADE, { point: _cube });
      return true;
    }
    return false;
  }

  // -- 3: the scanner's acquired target ------------------------------------------------

  target(out) {
    if (!this.alertsOpen()) return false;
    const ai = this.ai;
    const controller = this.controller;
    const unit = controller.unit;
    const target = ai.combat?.acquiredTarget?.(this.agent) ?? null;
    if (!target || !this.agent.acceptsTarget(target)) return false;
    if (kindOf(unit) === 5) {
      const vehicles = ai.vehicles;
      if (vehicles?.targetReserved?.(target, unit.team)) return false;
      vehicles?.reserveTarget?.(target, unit);
    }
    // A droideka takes scanner targets only within 10 m.
    if (kindOf(unit) === 3 && target.position.distanceToSquared(unit.position) > 100) return false;
    ai.combat?.setTarget?.(this.agent, target);
    out.goal = SourceGoal.combat(COMBAT_MODE.TARGET, { target: refOf(target) });
    return true;
  }

  // -- 4: acoustic contacts ---------------------------------------------------------------

  acoustic(out) {
    if (!this.alertsOpen()) return false;
    const respond = this.ai.combat?.acousticResponse;
    if (respond) return this.adopt(respond(this.controller, this.agent), out);
    const ai = this.ai;
    const controller = this.controller;
    const unit = controller.unit;
    if (!unit) return false;
    _self.copy(unit.position);
    const list = controller.acoustic;
    for (let i = 0; i < list.length; i++) {
      const source = resolveRef(list[i]);
      if (!source) continue;
      if (!this.armed() || !this.agent.acceptsAcoustic(source)) continue;
      _point.copy(source.position);
      const hearing = ai.combat?.acousticRange;
      let range = hearing ? hearing(controller) : ai.fallbackSensorRange;
      if (kindOf(unit) === 3) range *= 0.5;
      if (horizontalDistanceSq(_self, _point) < range * range) {
        out.goal = SourceGoal.combat(COMBAT_MODE.LOCATION, { point: _point });
        if (kindOf(unit) === 0) emitVo(ai, unit, { event: "heardEnemy", property: "HeardEnemySound" });
        if (this.turretCannotReach(_point)) {
          controller.requestExit();
          return false;
        }
        return true;
      }
    }
    return false;
  }

  // -- 5: squad commands --------------------------------------------------------------------

  command(out) {
    const ai = this.ai;
    const controller = this.controller;
    const agent = this.agent;
    if (!controller.command.pending) return false;
    const command = controller.command.record;
    if (!command || !agent.acceptsCommand(command)) return false;
    controller.commands.removeKind(GOAL.FOLLOW);
    controller.commands.removeKind(GOAL.HOLD);
    // The yes comes before any command-specific check; there is no "no".
    if (controller.unit) emitVo(ai, controller.unit, { event: "tac_yes", property: "AISCResponseYessirSound", command: command.kind });
    let accepted = false;
    switch (command.kind) {
      case COMMAND.MOVE_OUT:
      case COMMAND.AT_EASE:
        agent.onMoveOut();
        if (isFollowOrHold(controller.commands.current.kind) && controller.lastCommand?.source === command.source) {
          agent.requestState(DONE);
        }
        break;
      case COMMAND.HOLD:
        if (ai.vehicles?.refusesHold?.(controller)) break;
        out.goal = SourceGoal.of(GOAL.HOLD);
        accepted = true;
        break;
      case COMMAND.FOLLOW: {
        if (!command.target) break;
        const leader = currentControllable(ai, command.source);
        if (!leader) break;
        const mine = countFollowers(ai, controller.unit);
        const theirs = countFollowers(ai, resolveRef(command.target));
        if (theirs < 3 && mine < 2) {
          out.goal = SourceGoal.follow(command.target, refOf(leader));
          accepted = true;
        }
        break;
      }
      case COMMAND.BOARD:
        out.goal = SourceGoal.board(command.target);
        accepted = true;
        break;
      case COMMAND.UNBOARD:
        out.goal = SourceGoal.of(GOAL.UNBOARD);
        controller.reboardAt = this.now() + 20;
        accepted = true;
        break;
      default:
    }
    controller.lastCommand = { ...command };
    return accepted;
  }

  // -- 6: vehicle boarding candidates ---------------------------------------

  boarding(out) {
    const ai = this.ai;
    const controller = this.controller;
    const unit = controller.unit;
    if (kindOf(unit) === 5) {
      const nav = ai.nav;
      if (nav?.inDangerRegion?.(unit.position, Math.max(15, nav.traversalRadius?.(unit) ?? 0))) return false;
    }
    if (kindOf(unit) !== 0) return false;
    const notes = controller.vehicleNotes;
    if (!notes.length) return false;
    const pick = ai.randomA.int(0, notes.length - 1);
    const candidate = resolveRef(notes[pick]);
    const vehicles = ai.vehicles;
    if (!candidate || !vehicles) return false;
    if (vehicles.canEnter && !vehicles.canEnter(controller, candidate)) return false;
    if (!((vehicles.freeSeats?.(candidate, unit.team, 1) ?? 0) > 0)) return false;
    if (!this.agent.acceptsBoard(candidate)) return false;
    if (!(controller.reboardAt < this.now())) return false;
    out.goal = SourceGoal.board(refOf(candidate));
    vehicles.reserve?.(candidate, unit, 1);
    return true;
  }

  // -- 7: the crew-wait window --------------------------------------------------------------

  secondaryWait(out) {
    const controller = this.controller;
    if (!(this.now() < controller.secondaryWaitUntil)) return false;
    if (!this.agent.acceptsSecondaryWait()) return false;
    out.goal = SourceGoal.of(GOAL.WAIT_SECONDARY);
    return true;
  }

  // -- 8: stagnation recovery ------------------------------------------------------------------

  recovery(out) {
    if (!this.agent.acceptsRecovery()) return false;
    const kind = kindOf(this.controller.unit);
    if (kind !== 0 && kind !== 3) return false;
    if (this.controller.executorStatus === 0) return false;
    out.goal = SourceGoal.of(GOAL.RECOVERY);
    return true;
  }

  // -- the charge test -----------------------------------------------------------------------

  // An objective-bound soldier with three allied AI soldiers fighting within
  // 7 m keeps charging: it shouts its ChargeSound and its approach state
  // refuses danger, shot, target and acoustic interrupts. Evaluated at most
  // once per distinct time; the result is cached in between.
  charging() {
    const now = this.now();
    if (now <= this.checkedAt) return this.charge;
    this.charge = false;
    this.checkedAt = now;
    const ai = this.ai;
    const unit = this.controller.unit;
    if (kindOf(unit) !== 0) return this.charge;
    const near = ai.queryAllies(unit, 7, 30);
    if (!near.length) return this.charge;
    let fighting = 0;
    for (const other of near) {
      if (fighting > 2) break;
      const controller = other.controller;
      if (kindOf(other) === 0 && controller && controller !== this.controller && controller.state === 2 && controller.isUnitController) {
        fighting += 1;
      }
    }
    if (fighting === 3) {
      emitVo(ai, unit, { event: "charge", property: "ChargeSound" });
      this.charge = true;
    }
    return this.charge;
  }
}
