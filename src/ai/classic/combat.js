import * as THREE from "three";
import { ANGLE, aimPointFor, dot32, leadPoint, lengthSq32, lengthSqXZ32, lookupCos, lookupSin, normalizeD3DX, normalizeXZ } from "./aim.js";
import { Agent } from "./agents.js";
import { DONE, Fsm } from "./controller.js";
import { emitVo } from "./squads.js";
import { Perception, TargetTracker, notifyDamage, notifyDanger, notifyTargetedShot, sightRange, validTarget } from "./perception.js";
import {
  consumeFire, effectiveRange, forwardSpeed, lowerBand, rebuildProtectedHumans, registerAIData, selectWeapon,
  selectedWeapon, sensorRange, switchWeapon, upperBand, useClassicRandom, weaponAI,
} from "./weapons-ai.js";

// Classic combat: the agent a bot runs while fighting (UnitCombatAgent,
// controller state 2), the attack patterns that move it while it shoots
// (AttackPatterns), the cover helper, and the dispatcher that decides
// whether and how a bot engages a target, following the 2004 game
// (docs/ai-combat.md).
//
// The combat agent starts in one of four states from its source goal:
// investigate a point (2: a shot, damage or sound from there), fight its
// target (4), get away from a danger point (5), or go prone (6).
//   2  stand and turn to the point (pulled out to 15 m when nearer than
//      10 m); once facing it within acos(0.939), fight at once if a target
//      has turned up, else look around 0.75 s later (3);
//   3  look around (soldiers and droidekas only): turn 30-75 degrees to a
//      random side, then twice back past centre, a tenth of a second each
//      once aligned, then finish; fight if a target turns up;
//   4  fight: pick a pattern for the target and run it; pick again when the
//      pattern or the target changes; finish when the target's last seen
//      point is more than sensor range + 10 away; look around when there is
//      nothing left to fight. A droideka raises its shield when it is down
//      and its energy is above half;
//   5  run 20 m from the danger point, or (half the time, when the point is
//      well off to a side) dive the other way; 2 s, then look around;
//   6  go prone for 4 s, then look around.
// Every update it tracks its target, keeps the alert pose, runs its
// interrupt checks, and (unless the pattern fires for it) sets its fire
// requests from the tracker.
//
// Patterns (chosen by dispatch()): 2 stand and shoot (one bot in three
// crouches); 3 pause 1.9 s, then move to a random point near optimal range
// from the target (any update of the move may end it, 1 in 6); 4 move 30 m
// across and 30 m toward or away from the target to keep in the range band,
// re-deciding every 3 s; 5 move to the target's side at optimal range every
// 4 s; 6 run at the target (melee); 9 take cover (from 2..5 and 8, at random
// intervals); 10 and 11 jetpack moves; 12 go look where the target was last
// seen. Vehicle patterns (7, 13-16) belong to the vehicle code: their
// selectors give weapon eligibility only here, and world.vehicles hooks
// receive their states.

const f = Math.fround;
const FLT_MAX = 3.4028234663852886e38;
const UP = new THREE.Vector3(0, 1, 0);
const COS_ALIGN = f(0.939);

export { DONE };
// The generic driver controller.js defines, shared by every
// agent and helper.
export const HelperFsm = Fsm;

function resolve(ref) {
  const u = ref?.unit;
  return u && u.alive && u.generation === ref.generation ? u : null;
}

function now(world) {
  return f(world?.time ?? 0);
}

// The two shared generators: A for almost every draw, B for the cube offsets
// (the jet offsets here).
function rngA(world) {
  return world.randomA ?? world.random;
}

function rngB(world) {
  return world.randomB ?? rngA(world);
}

// ---------------------------------------------------------------------------
// Controller requests (the Controllable's fields, through the movement
// helper's request slots in the 2004 game).

function lookAt(c, point) {
  if (c.lookAt) c.lookAt(point);
  else {
    c.aimState = 1;
    (c.aimPoint ||= new THREE.Vector3()).copy(point);
  }
}

function lookAlongPath(c) {
  if (c.lookAlongPath) c.lookAlongPath();
  else c.aimState = 2;
}

function setStance(c, stance) {
  if (c.setStance) c.setStance(stance);
  else c.stance = stance;
}

function fastTurn(c) {
  if (c.requestFacing) c.requestFacing();
  else if (c.unit?.kind === 0 || c.unit?.kind === 3) c.facing = true;
}

// A droideka's wanted mode: 1 deploy, 2 roll.
function wantMode(c, mode) {
  c.wantedMode = mode;
}

// A dive aside, -1 (left) or +1 (right), for the AI
// input step to turn into the soldier's dive (see docs/ai-combat.md).
function dodge(c, side) {
  c.dodge = side;
}

function helperGoto(helper, point, options) {
  helper?.goto?.(point, options);
}

// ---------------------------------------------------------------------------
// Engagement dispatch: whether `unit` engages `target`, with which
// attack pattern and cover probability, and which weapon. Returns out =
// { pattern, probability, aux (a weapon could hit it), action, inBand,
// weapon }; engaging means pattern != 0. switchAllowed: select the chosen
// weapon.

const ROLES = { scout: 0, "anti-armor": 1, assault: 2, support: 3, repair: 3, special: 3, pilot: 4, trooper: 5 };

export function roleOf(classDef) {
  if (Number.isInteger(classDef?.ai?.role)) return classDef.ai.role;
  if (Number.isInteger(classDef?.role)) return classDef.role;
  return ROLES[String(classDef?.UnitType ?? "").trim().toLowerCase()] ?? 6;
}

const choice = { ok: false, index: -1, weapon: null, action: 6, inBand: false };

export function dispatch(controller, unit, target, switchAllowed, world, out = {}) {
  out.pattern = 0;
  out.probability = 0;
  out.aux = false;
  out.action = 6;
  out.inBand = false;
  out.weapon = null;
  if (!unit || !target) return out;
  const s = unit.kind;
  const t = target.kind;
  if (s === 0 || s === 1) {
    const role = s === 1 ? 5 : roleOf(unit.classDef);
    if (t === 0 || t === 1 || t === 3) footPolicy(controller, unit, target, role, switchAllowed, world, out);
    else if (t === 2 || t === 4 || t === 6 || t === 8 || t === 9) vehiclePolicy(controller, unit, target, switchAllowed, world, out);
    else if (t === 5) selectOnly(controller, unit, target, switchAllowed, world, out);
  } else if (s === 3 || s === 7) {
    // A droideka (and a mounted gun) engages anything but aircraft with
    // pattern 2 while in band.
    selectOnly(controller, unit, target, switchAllowed, world, out);
    if (t !== 5 && out.aux && out.inBand) out.pattern = 2;
  } else {
    // Walkers, flyers, hovers: vehicle code (weapon eligibility only here).
    const hook = world?.vehicles?.dispatch;
    if (hook) return hook.call(world.vehicles, controller, unit, target, switchAllowed, world, out);
    selectOnly(controller, unit, target, switchAllowed, world, out);
  }
  return out;
}

function selectOnly(controller, unit, target, switchAllowed, world, out) {
  selectWeapon(controller, unit, target, switchAllowed, world, choice);
  out.aux = choice.ok;
  out.action = choice.action;
  out.inBand = choice.inBand;
  out.weapon = choice.weapon;
  return out;
}

// A soldier against a soldier, droid or droideka. A hero never
// engages a hero, and a hero target gets no pattern (weapon eligibility
// only). Scout with a rifle-class weapon: pattern 2 in band, else none;
// assault with melee: pattern 6; otherwise the range policy.
function footPolicy(controller, unit, target, role, switchAllowed, world, out) {
  if (unit.hero && target.hero) return out;
  selectOnly(controller, unit, target, switchAllowed, world, out);
  if (target.hero || !out.aux) return out;
  if (role === 0 && out.action === 1) {
    out.pattern = out.inBand ? 2 : 0;
    return out;
  }
  if (role === 2 && out.action === 4) {
    out.probability = 0;
    out.pattern = 6;
    return out;
  }
  return rangePolicy(controller, world, out);
}

// Grenades stand and throw in band; otherwise cover probability
// 0.5 and a pattern by the GLOBAL enemy skill, the Hard tier and the
// controller's roll mod 6.
export function rangePolicy(controller, world, out) {
  const B = out.inBand;
  out.pattern = 0;
  if (out.action === 2) {
    if (B) out.pattern = 2;
    return out;
  }
  out.probability = f(0.5);
  const difficulty = world?.difficulty;
  const skill = difficulty?.enemySkill ?? 14;
  const hard = difficulty?.hard ?? difficulty?.tier === 3;
  const I = (((controller?.roll ?? 0) % 6) + 6) % 6;
  if (skill < 17) {
    if (skill < 14) out.pattern = I <= 2 ? (B ? 2 : 3) : I === 3 ? 3 : I === 4 ? 4 : 5;
    else out.pattern = I <= 1 ? (B ? 2 : 3) : I <= 3 ? 3 : I === 4 ? 4 : 5;
  } else if (hard) out.pattern = I <= 4 ? 4 : 5;
  else out.pattern = I === 0 ? (B ? 2 : 3) : I === 1 ? 3 : I <= 4 ? 4 : 5;
  return out;
}

// A soldier against a vehicle or building: probability 0.75;
// pistols, rifles, grenades and launchers take pattern 5 on even rolls,
// pattern 2 on odd rolls in band; melee and tools none.
function vehiclePolicy(controller, unit, target, switchAllowed, world, out) {
  out.probability = f(0.75);
  selectOnly(controller, unit, target, switchAllowed, world, out);
  if (!out.aux) {
    out.pattern = 0;
    return out;
  }
  if (out.action <= 3) {
    const odd = Math.abs((controller?.roll ?? 0) % 2) === 1;
    if (!odd) out.pattern = 5;
    else if (out.inBand) out.pattern = 2;
  } else if (out.action === 4 || out.action === 6) out.pattern = 0;
  return out;
}

// ---------------------------------------------------------------------------
// Cover hints and their reservations. Hints come from Nav:
// world.nav.hints.query(mask, center, halfExtent, cap) -> [{ position,
// forward, primaryMask, secondaryMask, sideMask, siblings }] (or the
// adapter's queryHints). A reservation lasts 3 s unless renewed (equality
// expires).

const reservations = new WeakMap();

function hintAvailable(hint, owner, t) {
  const r = reservations.get(hint);
  return !r || r.owner === owner || !(f(t - r.at) < 3);
}

function reserveHint(hint, owner, t) {
  if (!hint) return;
  for (const h of hintRing(hint)) reservations.set(h, { owner, at: t });
}

function releaseHint(hint) {
  if (!hint) return;
  for (const h of hintRing(hint)) {
    const r = reservations.get(h);
    if (r) r.at = f(-FLT_MAX);
  }
}

function hintRing(hint) {
  const ring = [hint];
  for (const s of hint?.siblings || []) if (s !== hint) ring.push(s);
  return ring;
}

// A stance 0..2 enabled in the mask, by rejection sampling.
function drawStance(random, mask) {
  if (!(mask & 7)) return 0;
  let k;
  do k = random.int(0, 2);
  while (!(mask & (1 << k)));
  return k;
}

// A side 0/1 enabled in the hint's side mask.
function drawSide(random, mask) {
  if (!(mask & 3)) return 0;
  let k;
  do k = random.int(0, 1);
  while (!(mask & (1 << k)));
  return k;
}

let cos60 = null;
const coverN = { x: 0, y: 0, z: 0 };

// A cover hint's score (lower is better), FLT_MAX when unsuitable.
// Suitable: the hint lies beyond the target's point from the bot
// (dot((T - P)xz, H - T) < 0), faces the target within 60 degrees, and its
// horizontal distance from the target is between the lower band and the
// cover range. Score: 2.25 x |H - P|^2 + |H - T|^2, horizontally.
export function coverScore(agent, hint) {
  const unit = agent.controller.unit;
  const T = agent.tracker.point;
  const P = unit.position;
  const H = hint.position;
  cos60 ??= lookupCos(ANGLE.FACE_WIDE);
  const dx = f(T.x - P.x), dz = f(T.z - P.z);
  const ex = f(H.x - T.x), ey = f(H.y - T.y), ez = f(H.z - T.z);
  if (!(dot32(dx, 0, dz, ex, ey, ez) < 0)) return FLT_MAX;
  normalizeXZ(f(T.x - H.x), f(T.z - H.z), coverN);
  const F = hint.forward;
  if (!(cos60 < dot32(F.x, F.y, F.z, coverN.x, coverN.y, coverN.z))) return FLT_MAX;
  const R = coverRange(agent);
  const h2 = lengthSqXZ32(ex, ez);
  if (!(h2 < f(R * R))) return FLT_MAX;
  const L = lowerBand(unit, agent.controller);
  if (!(f(L * L) < h2)) return FLT_MAX;
  return f(f(lengthSqXZ32(f(H.x - P.x), f(H.z - P.z)) * 2.25) + h2);
}

// min(40, the selected weapon's effective range); 40 without one.
function coverRange(agent) {
  const c = agent.controller;
  const weapon = selectedWeapon(c.unit, c);
  if (!weapon) return 40;
  const r = effectiveRange(c.unit, c, weapon.def);
  return r <= 40 ? r : 40;
}

function queryCover(agent, center, half) {
  const world = agent.world;
  const hints = world.nav?.hints;
  if (hints?.query) return hints.query(4, center, half, 30) || [];
  const q = agent.ai?.nav?.queryHints;
  if (q) return q({ mask: 4, center, halfExtent: half, cap: 30 }) || [];
  return [];
}

// The cover hints within the cover range of the bot,
// visited in random order; the available one with the strictly lowest score.
export function findCover(agent) {
  const c = agent.controller;
  const world = agent.world;
  const list = queryCover(agent, c.unit.position, Math.trunc(coverRange(agent))).slice(0, 30);
  const random = rngA(world);
  const t = now(world);
  let best = null;
  let bestScore = FLT_MAX;
  for (let i = list.length - 1; i >= 0; i--) {
    const k = random.int(0, i);
    const hint = list[k];
    if (hintAvailable(hint, c.unit, t)) {
      const s = coverScore(agent, hint);
      if (s < bestScore) {
        best = hint;
        bestScore = s;
      }
    }
    list[k] = list[i];
  }
  return best;
}

// The cover helper: 2 go
// to the hint; 3 hold a primary stance 2..4 s; then a secondary stance
// 2..6 s (4) or, when the hint has sides, step 3 m along it for 4..7 s (5)
// and back (2). The target is held while approaching and in the primary
// stance; the sidestep remembers it 4 s. Every 5 s the hint is rescored (an
// unsuitable one is let go: another, or finish); every 16..25 s a better one
// is sought.
export class CoverHelper extends Fsm {
  constructor(agent) {
    super();
    this.agent = agent;
    this.hint = null;
    this.posture = 0;
    this.deadline = 0;
    this.checkAt = 0;
    this.reselectAt = 0;
  }

  get world() {
    return this.agent.world;
  }

  get random() {
    return rngA(this.world);
  }

  init(hint) {
    const t = now(this.world);
    this.hint = hint;
    reserveHint(hint, this.agent.controller.unit, t);
    this.posture = 0;
    this.checkAt = f(t + 5);
    this.reselectAt = f(t + this.random.int(16, 25));
    this.requestState(2);
    this.flush();
  }

  renew() {
    reserveHint(this.hint, this.agent.controller.unit, now(this.world));
  }

  onEnter() {
    const a = this.agent;
    const t = now(this.world);
    switch (this.state) {
      case 2:
        this.renew();
        helperGoto(a.helper, this.hint.position);
        this.posture = 0;
        a.tracker.freeze(true, t);
        return;
      case 3:
        this.deadline = f(t + this.random.int(2, 4));
        this.posture = drawStance(this.random, this.hint.primaryMask ?? 1);
        a.tracker.freeze(true, t);
        return;
      case 4:
        this.deadline = f(t + this.random.int(2, 6));
        this.posture = drawStance(this.random, this.hint.secondaryMask ?? 1);
        return;
      case 5: {
        this.deadline = f(t + this.random.int(4, 7));
        const side = drawSide(this.random, this.hint.sideMask ?? 0);
        const across = new THREE.Vector3().crossVectors(this.hint.forward, UP).multiplyScalar(side ? 3 : -3);
        helperGoto(a.helper, across.add(this.hint.position));
        this.posture = drawStance(this.random, this.hint.secondaryMask ?? 1);
        a.tracker.setDelay(4, t);
      }
    }
  }

  onExit() {
    const a = this.agent;
    const t = now(this.world);
    this.posture = 0;
    if (this.state > 1 && this.state < 4) a.tracker.freeze(false, t);
    else if (this.state === 5) a.tracker.defaultDelay(t);
  }

  onTick() {
    if (this.state === DONE) return;
    const a = this.agent;
    const t = now(this.world);
    if (this.checkAt < t) {
      this.checkAt = f(t + 5);
      if (coverScore(a, this.hint) === FLT_MAX) {
        releaseHint(this.hint);
        const next = findCover(a);
        if (!next) {
          this.hint = null;
          this.requestState(DONE);
          return;
        }
        this.hint = next;
        this.renew();
        this.requestState(2);
        return;
      }
    }
    if (this.reselectAt < t) {
      this.reselectAt = f(t + this.random.int(16, 25));
      const next = findCover(a);
      if (next) {
        this.hint = next;
        this.renew();
        this.requestState(2);
        return;
      }
    }
    switch (this.state) {
      case 2:
        if (!a.helper?.arrived) this.renew();
        else this.requestState(3);
        return;
      case 3:
        this.renew();
        if (this.deadline < t) this.requestState((this.hint.sideMask ?? 0) > 0 ? 5 : 4);
        return;
      case 4:
        this.renew();
        if (this.deadline < t) this.requestState(3);
        return;
      case 5:
        this.renew();
        if (this.deadline < t) this.requestState(2);
    }
  }

  cleanup() {
    releaseHint(this.hint);
    this.hint = null;
  }
}

// ---------------------------------------------------------------------------
// AttackPatterns.

const pa = new THREE.Vector3();
const pb = new THREE.Vector3();
const pc = new THREE.Vector3();
const pn = { x: 0, y: 0, z: 0 };
let cosTight = null;
let cosWide = null;

export class AttackPatterns extends Fsm {
  constructor(agent) {
    super();
    this.agent = agent;
    this.cover = new CoverHelper(agent);
    this.probability = 0; // cover chance
    this.coverRetry = -1;
    this.prior = DONE; // the pattern to return to
    this.pathActive = false; // melee
    this.crouch = 0; // pattern 2 posture
    this.paused = false; // pattern 3
    this.pauseUntil = 0;
    this.toward = 1; // pattern 4: 1 toward, 3 away
    this.side = 0; // pattern 4: 0 or 2
    this.bandCheckAt = 0;
    this.stepAt = 0;
    this.facing = false; // pattern 4: facing the target
    this.stepped = false;
    this.lateralAt = 0; // pattern 5
    this.lateralFacing = false;
    this.lastPath = -100; // melee path retry
    this.retreatAt = -1; // pattern 8
    this.coverHint = null;
    this.jetting = false;
    this.jetRetry = 0;
    this.jetStart = 0;
    this.jetHint = null;
    this.hintJetRetry = 0;
    this.searched = false; // pattern 12
    this.skipFire = false;
  }

  get world() {
    return this.agent.world;
  }

  get controller() {
    return this.agent.controller;
  }

  get random() {
    return rngA(this.world);
  }

  // Start a pattern with a cover probability.
  init(pattern, probability) {
    this.probability = f(probability);
    this.prior = DONE;
    this.requestState(pattern);
    this.flush();
  }

  // Back to the pattern that was interrupted.
  restore() {
    const next = this.prior;
    this.prior = DONE;
    this.requestState(next);
  }

  onEnter() {
    const random = this.random;
    const t = now(this.world);
    this.coverRetry = -1;
    this.jetRetry = f(t + random.int(3, 8));
    this.hintJetRetry = f(t + random.int(3, 8));
    const a = this.agent;
    switch (this.state) {
      case 2:
        a.helper?.stop?.();
        this.crouch = (this.controller.roll ?? 0) % 3 === 1 ? 1 : 0;
        return;
      case 3:
        this.reposition();
        this.paused = false;
        return;
      case 4:
        this.toward = 1;
        this.side = random.int(0, 1) ? 2 : 0;
        this.bandCheckAt = 0;
        this.stepAt = 0;
        this.flank();
        return;
      case 5:
        this.lateralAt = 0;
        a.helper?.stop?.();
        return;
      case 6:
        this.pathActive = false;
        this.lastPath = -100;
        return;
      case 8:
        a.helper?.stop?.();
        this.retreatAt = -1;
        return;
      case 9:
        this.cover.init(this.coverHint);
        return;
      case 10:
        this.directJet();
        return;
      case 11:
        this.world.nav?.jetHints?.enter?.(this.controller, this.jetHint);
        return;
      case 12:
        a.tracker.setDelay(16, t);
        helperGoto(a.helper, a.tracker.point);
        this.searched = false;
        return;
      default:
        if (this.state >= 7) this.world.vehicles?.enterPattern?.(this, this.state);
    }
  }

  onExit() {
    const a = this.agent;
    const t = now(this.world);
    setStance(this.controller, 0);
    switch (this.state) {
      case 7:
        a.tracker.defaultDelay(t);
        this.world.vehicles?.exitPattern?.(this, this.state);
        return;
      case 12:
        a.tracker.defaultDelay(t);
        return;
      case 9:
        // The cover helper's own exit first: in states 2 and 3 it holds the
        // target (tracker freeze), and only its exit lets go. (Port: the
        // state was zeroed without it, so a bot that left cover for another
        // pattern held a target it no longer saw for the rest of the fight,
        // standing in pattern 2 facing where it was last seen.)
        this.cover.onExit();
        this.cover.cleanup();
        this.cover.state = 0;
        this.cover.pending = 0;
        return;
      case 11:
        this.world.nav?.jetHints?.exit?.(this.controller, this.jetHint);
        return;
      default:
        if (this.state >= 13) this.world.vehicles?.exitPattern?.(this, this.state);
    }
  }

  onTick(dt) {
    this.skipFire = false;
    switch (this.state) {
      case 2:
        return this.standing();
      case 3:
        return this.pauseAndMove();
      case 4:
        return this.bandFlank();
      case 5:
        return this.lateral();
      case 6:
        return this.melee();
      case 8:
        return this.retreat();
      case 9:
        return this.inCover(dt);
      case 10:
        return this.jetTick();
      case 11:
        return this.hintJetTick(dt);
      case 12:
        return this.lostTarget();
      default:
        this.world.vehicles?.tickPattern?.(this, this.state, dt);
    }
  }

  cleanup() {
    this.cover.cleanup();
  }

  // The posture and facing call each pattern ends with.
  face(posture = 0) {
    this.agent.aim(posture);
  }

  // -- transitions out of a pattern -----------------------------------------

  // Every 16..25 s, with the cover probability, take cover.
  tryCover(from) {
    const random = this.random;
    const t = now(this.world);
    if (!(this.coverRetry < t)) return false;
    this.coverRetry = f(t + random.int(16, 25));
    if (!(random.range(0, 1) < this.probability)) return false;
    const hint = findCover(this.agent);
    this.coverHint = hint;
    if (!hint) return false;
    reserveHint(hint, this.controller.unit, t);
    this.prior = from;
    this.requestState(9);
    return true;
  }

  // A jetpack (class JetJump > 0) with more fuel than JetFuelMinBorder.
  jetReady() {
    const unit = this.controller.unit;
    if (unit?.kind !== 0) return false;
    const jet = unit.soldier?.jet;
    const cls = unit.classDef || {};
    if (!((jet?.jump ?? cls.JetJump ?? 0) > 0)) return false;
    return f(jet?.minBorder ?? cls.JetFuelMinBorder ?? 0) < (unit.soldier?.fuel ?? 0);
  }

  // A jet hint, retried every 3..8 s.
  tryHintJet(from) {
    if (!this.jetReady()) return false;
    const jetHints = this.world.nav?.jetHints;
    if (jetHints?.excluded?.(this.controller)) return false;
    const t = now(this.world);
    if (!(this.hintJetRetry < t)) return false;
    this.hintJetRetry = f(t + this.random.int(3, 8));
    const hint = jetHints?.find?.(this.controller) ?? null;
    this.jetHint = hint;
    if (!hint) return false;
    this.prior = from;
    this.requestState(11);
    return true;
  }

  // A direct jet move, every 6..12 s.
  tryDirectJet(from) {
    if (!this.jetReady()) return false;
    const t = now(this.world);
    if (!(this.jetRetry < t)) return false;
    const n = this.random.int(6, 12);
    this.prior = from;
    this.jetRetry = f(t + n);
    this.requestState(10);
    return true;
  }

  // The target unseen for over 2 s: go look (pattern 12).
  tryLostTarget(from) {
    const t = now(this.world);
    if (!(f(t - this.agent.tracker.sampleTime) > 2)) return false;
    this.prior = from;
    this.requestState(12);
    return true;
  }

  // -- pattern 2: stand and shoot --------------------------------------------

  standing() {
    if (this.tryCover(2) || this.tryHintJet(2) || this.tryDirectJet(2)) return;
    const unit = this.controller.unit;
    // A slow walker or hover sidesteps now and then.
    if ((unit.kind === 6 || unit.kind === 2) && forwardSpeed(unit) < 13) this.world.vehicles?.sidestep?.(this);
    this.agent.stationary = true;
    this.face(this.crouch);
  }

  // -- pattern 3: pause, then move near optimal range ---------------------------

  pauseAndMove() {
    if (this.tryCover(3) || this.tryHintJet(3) || this.tryDirectJet(3) || this.tryLostTarget(3)) return;
    const t = now(this.world);
    const helper = this.agent.helper;
    if (!this.paused) {
      if (helper?.arrived || this.random.int(1, 6) === 2) {
        this.paused = true;
        this.pauseUntil = f(t + f(1.9));
      }
      this.face(0);
      return;
    }
    if (t <= this.pauseUntil) {
      helper?.stop?.();
      this.face(0);
      return;
    }
    this.paused = false;
    this.reposition();
    this.face(0);
  }

  // A random horizontal direction (cube draw z, y, x; y dropped;
  // x nudged by 0.0001) at a distance drawn from [Optimal - 0.2 (Optimal -
  // Min), Optimal + 0.2 (effective - Optimal)] from the target's point.
  reposition() {
    const r = this.random;
    const z = f(f(r.uniform() - 0.5) * 2);
    r.uniform(); // y, dropped
    const x = f(f(r.uniform() - 0.5) * 2);
    normalizeD3DX(f(x + 0.0001), 0, z, pn);
    const c = this.controller;
    const unit = c.unit;
    const weapon = selectedWeapon(unit, c);
    const a = weaponAI(weapon?.def);
    let R = 0;
    if (a) {
      const opt = a.optimalRange;
      const eff = effectiveRange(unit, c, weapon.def);
      R = drawRange(r, f(opt - f(f(opt - a.minRange) * 0.2)), f(f(f(eff - opt) * 0.2) + opt));
    }
    const T = this.agent.tracker.point;
    pa.set(f(T.x + f(pn.x * R)), f(T.y + f(pn.y * R)), f(T.z + f(pn.z * R)));
    helperGoto(this.agent.helper, pa);
  }

  // -- pattern 4: keep in the band, moving across ------------------------------

  bandFlank() {
    if (this.tryCover(4) || this.tryHintJet(4) || this.tryDirectJet(4) || this.tryLostTarget(4)) return;
    const t = now(this.world);
    const was = this.facing;
    cosTight ??= lookupCos(ANGLE.FACE_TIGHT);
    this.facing = this.agent.facingTarget(cosTight);
    if (!this.facing) {
      if (was) this.stepAt = 0;
      if (this.stepAt < t) {
        const inBand = this.stepToBand();
        if (!inBand) this.stepAt = f(t + 3);
        this.stepped = inBand;
      }
      this.face(0);
      return;
    }
    if (!was) this.bandCheckAt = 0;
    const unit = this.controller.unit;
    const T = this.agent.tracker.point;
    const d2 = lengthSqXZ32(f(T.x - unit.position.x), f(T.z - unit.position.z));
    if (this.bandCheckAt < t) {
      const lo = lowerBand(unit, this.controller);
      if (d2 < f(lo * lo)) return this.redirect(3, t);
      const hi = this.upperBand();
      if (f(hi * hi) < d2) return this.redirect(1, t);
    }
    if (!was || this.random.int(1, 2) === 1) this.flank();
    this.face(0);
  }

  redirect(toward, t) {
    this.toward = toward;
    this.bandCheckAt = f(t + 3);
    this.flank();
    this.face(0);
  }

  upperBand() {
    const unit = this.controller.unit;
    const target = this.agent.tracker.target;
    return upperBand(unit, this.controller, target ? sightRange(unit, target) : 0);
  }

  // 30 m across (the side alternating) and 30 m toward (1) or
  // away (3) from the target's point; one time in five also a dive aside.
  flank() {
    const unit = this.controller.unit;
    const T = this.agent.tracker.point;
    const P = unit.position;
    normalizeXZ(f(T.x - P.x), f(T.z - P.z), pn);
    this.side = this.side !== 0 ? 0 : 2;
    pc.crossVectors(pb.set(pn.x, pn.y, pn.z), UP);
    const across = this.side === 2 ? 30 : -30;
    const along = this.toward === 1 ? 30 : -30;
    pa.set(f(f(pc.x * across) + f(pn.x * along)), f(f(pc.y * across) + f(pn.y * along)), f(f(pc.z * across) + f(pn.z * along)));
    pa.set(f(P.x + pa.x), f(P.y + pa.y), f(P.z + pa.z));
    helperGoto(this.agent.helper, pa);
    if (this.random.int(1, 5) === 2) dodge(this.controller, this.side === 0 ? -1 : 1);
  }

  // A 15 m step from the target's point toward the band; true
  // (after stopping) when already in it.
  stepToBand() {
    const unit = this.controller.unit;
    const T = this.agent.tracker.point;
    const P = unit.position;
    const dx = f(P.x - T.x), dz = f(P.z - T.z);
    normalizeD3DX(dx, 0, dz, pn);
    const d2 = lengthSqXZ32(dx, dz);
    const lo = lowerBand(unit, this.controller);
    if (d2 < f(lo * lo)) {
      helperGoto(this.agent.helper, pa.set(f(P.x + f(pn.x * 15)), f(P.y + f(pn.y * 15)), f(P.z + f(pn.z * 15))));
      return false;
    }
    const hi = this.upperBand();
    if (f(hi * hi) < d2) {
      helperGoto(this.agent.helper, pa.set(f(P.x - f(pn.x * 15)), f(P.y - f(pn.y * 15)), f(P.z - f(pn.z * 15))));
      return false;
    }
    this.agent.helper?.stop?.();
    return true;
  }

  // -- pattern 5: across the target at optimal range --------------------------------

  lateral() {
    if (this.tryCover(5) || this.tryHintJet(5) || this.tryDirectJet(5) || this.tryLostTarget(5)) return;
    const t = now(this.world);
    if (this.lateralAt < t) {
      this.lateralFacing = false;
      const c = this.controller;
      const unit = c.unit;
      const T = this.agent.tracker.point;
      const P = unit.position;
      normalizeD3DX(f(P.x - T.x), 0, f(P.z - T.z), pn);
      cosWide ??= lookupCos(ANGLE.FACE_WIDE);
      if (!this.agent.facingTarget(cosWide)) {
        // Not facing: step toward the band; already in it, keep the timer.
        if (this.stepToBand()) {
          this.face(0);
          return;
        }
      } else {
        this.lateralFacing = true;
        const side = pc.crossVectors(pb.set(pn.x, 0, pn.z), UP).clone(); // dir x up
        unit.forward(pb);
        pb.y = 0;
        const turn = f(f(pb.z * pn.x) - f(pb.x * pn.z)); // (forward x dir).y
        const k = f((weaponAI(selectedWeapon(unit, c)?.def)?.optimalRange ?? 0) * (turn > 0 ? -1 : 1));
        helperGoto(this.agent.helper, pa.set(f(T.x + f(side.x * k)), f(T.y + f(side.y * k)), f(T.z + f(side.z * k))));
      }
      this.lateralAt = f(t + 4);
    }
    this.face(0);
  }

  // -- pattern 6: melee pursuit ----------------------------------------------------

  // Follow the target while the way to it is clear; otherwise keep
  // an unfinished path, or ask for a new one every 2 s.
  melee() {
    if (this.tryLostTarget(6) || this.tryDirectJet(6) || this.tryHintJet(6)) return;
    const t = now(this.world);
    const unit = this.controller.unit;
    const target = this.agent.tracker.target;
    const helper = this.agent.helper;
    if (target && helper) {
      if (this.world.nav?.directClear?.(unit, target.position)) {
        if (helper.follow) helper.follow(target, { mode: 0, follow: 1 });
        else helperGoto(helper, target.position, { mode: "direct" });
        this.pathActive = false;
      } else {
        let wait = false;
        if (this.pathActive) {
          if (helper.arrived) this.pathActive = false;
          else wait = true;
        }
        if (!wait && f(this.lastPath + 2) < t) {
          this.pathActive = true;
          this.lastPath = t;
          helperGoto(helper, target.position);
        }
      }
    }
    this.face(0);
  }

  // -- pattern 8 (dormant: no stock dispatch selects it): retreat ---------------------

  retreat() {
    if (this.tryCover(8) || this.tryHintJet(8) || this.tryDirectJet(8)) return;
    const t = now(this.world);
    if (this.retreatAt < t) {
      this.retreatAt = f(t + drawRange(this.random, 3, 6));
      const unit = this.controller.unit;
      const T = this.agent.tracker.point;
      const P = unit.position;
      normalizeXZ(f(P.x - T.x), f(P.z - T.z), pn);
      const angle = drawRange(this.random, f(-0.7853981852531433), f(0.7853981852531433));
      const c = Math.cos(angle), s = Math.sin(angle);
      helperGoto(this.agent.helper, pa.set(P.x + 100 * (pn.x * c + pn.z * s), P.y, P.z + 100 * (-pn.x * s + pn.z * c)));
    }
    this.face(0);
  }

  // -- pattern 9: cover ---------------------------------------------------------------

  inCover(dt) {
    this.cover.update(dt, false);
    if (!this.cover.done) {
      this.face(this.cover.posture);
      return;
    }
    this.restore();
  }

  // -- patterns 10 and 11: jetpack ----------------------------------------------------

  // Ten tries at a point 20..40 m away (a generator-B cube x 40,
  // made horizontal), clear of barriers, with 40 m of open sky above it, and
  // a clear barrier segment to it; then jet there.
  directJet() {
    const world = this.world;
    const unit = this.controller.unit;
    const nav = world.nav;
    const r = rngB(world);
    for (let tries = 10; tries > 0; tries--) {
      const z = f(f(f(r.uniform() - 0.5) * 2) * 40);
      r.uniform();
      const x = f(f(f(r.uniform() - 0.5) * 2) * 40);
      if (lengthSq32(x, 0, z) < 400) continue;
      pa.set(f(unit.position.x + x), unit.position.y, f(unit.position.z + z));
      if (!(nav?.pointClear?.(pa, unit) ?? false)) continue;
      pb.copy(pa).addScaledVector(UP, 40);
      if (!world.los(pa, pb, unit, null)) continue;
      if (!world.mission?.noJetObstacleTest && nav?.segmentBlocked?.(unit.position, pa, unit)) continue;
      helperGoto(this.agent.helper, pa, { mode: "direct" });
      this.controller.requestJet?.(pc.set(0, 0, 0));
      this.jetStart = now(world);
      this.jetting = true;
      return;
    }
    this.jetStart = -10;
    this.restore();
  }

  jetTick() {
    if (f(this.jetStart + 1) < now(this.world) && !this.agent.helper?.flying) {
      this.jetting = false;
      this.restore();
    }
  }

  hintJetTick(dt) {
    const done = this.world.nav?.jetHints?.tick?.(this.controller, this.jetHint, dt) ?? true;
    if (done) this.restore();
  }

  // -- pattern 12: lost target ------------------------------------------------------------

  lostTarget() {
    const a = this.agent;
    if (a.tracker.freshTarget) {
      this.restore();
      return;
    }
    if (a.helper?.arrived && !this.searched) {
      this.searched = true;
      a.tracker.defaultDelay(now(this.world));
      a.nextLook();
      return;
    }
    this.face(0);
  }
}

// The 2004 game's float draw: lo itself, without a draw, when
// the ends are equal.
function drawRange(random, lo, hi) {
  lo = f(lo);
  hi = f(hi);
  return lo === hi ? lo : random.range(lo, hi);
}

// ---------------------------------------------------------------------------
// The combat agent (UnitCombatAgent), on the controller's Agent
// base (interrupt helper, movement helper, target tracker, phase handling).

const ca = new THREE.Vector3();
const cb = new THREE.Vector3();
const cn = { x: 0, y: 0, z: 0 };
const cm = { x: 0, y: 0, z: 0 };
const pick = {};

export class UnitCombatAgent extends Agent {
  // goal: the combat source goal (SourceGoal: mode 0 location, 1 target, 2
  // evade, 3 prone; point; refA the target).
  constructor(controller, goal) {
    super(controller, goal);
    this.tracker ??= new TargetTracker(controller);
    this.patterns = new AttackPatterns(this);
    this.point = new THREE.Vector3();
    this.hazard = new THREE.Vector3();
    this.alignAt = 0;
    this.looks = 0;
    this.lookAngle = 0;
    this.aligned = false;
    this.hazardUntil = 0;
    this.proneUntil = 0;
    this.pattern = 0;
    this.stationary = false;
    this.state = 1;
    switch (goal?.mode) {
      case 0:
        this.requestState(2);
        if (goal.point) this.point.copy(goal.point);
        break;
      case 1:
        this.requestState(4);
        this.tracker.assign(resolve(goal.refA));
        break;
      case 2:
        this.requestState(5);
        if (goal.point) this.hazard.copy(goal.point);
        break;
      case 3:
        this.requestState(6);
        break;
    }
  }

  get random() {
    return this.ai?.randomA ?? rngA(this.world);
  }

  // Controller state exit: the patterns let go of cover,
  // then the base ends the helper and this machine.
  exit() {
    this.patterns.cleanup();
    super.exit();
  }

  // -- policies ------------------------------------------------------------------------

  // A shot or damage alert from `point`. Walkers,
  // hovers and flyers never; a zero-speed mount that cannot bring a weapon
  // to bear always; unknown positions always; known ones only
  // while looking around (3), or while fighting (4) when the point is outside
  // the 77.5-degree flat forward cone.
  acceptsAlert(known, point) {
    const unit = this.controller.unit;
    const k = unit?.kind;
    if (k === 2 || k === 6 || k === 5) return false;
    if (unit && forwardSpeed(unit) === 0 && this.mountUnreachable(point)) return true;
    if (!known) return true;
    if (this.state === 3) return true;
    if (this.state === 4) return !inForwardCone(unit, point);
    return false;
  }

  // Grenade danger, unless a flyer or already evading.
  acceptsHazard() {
    return this.controller.unit?.kind !== 5 && this.state !== 5;
  }

  // A sound source, for a ground unit not investigating,
  // when the sound is outside the forward cone and nearer than 0.66 of the
  // remembered target (squared 0.4356), and the base policy (weapon in slot
  // 0, the dispatcher would engage) agrees.
  acceptsAcoustic(source) {
    const unit = this.controller.unit;
    const k = unit?.kind;
    if (!source || k === 2 || k === 6 || k === 5) return false;
    const point = source.position;
    if (forwardSpeed(unit) === 0 && this.mountUnreachable(point)) return true;
    if (this.state === 2 || inForwardCone(unit, point) || !this.tracker.target) return false;
    const P = unit.position;
    const T = this.tracker.point;
    const a = lengthSqXZ32(f(point.x - P.x), f(point.z - P.z));
    const b = lengthSqXZ32(f(T.x - P.x), f(T.z - P.z));
    if (!(a < f(b * f(0.43560004)))) return false;
    return this.canEngage(source);
  }

  // Never. The scanner's targets reach a fighting bot
  // through its tracker, not as a new combat goal (accepting would restart
  // the agent on every perception cycle).
  acceptsTarget() {
    return false;
  }

  // A boarding candidate only while no hostile target
  // is near it, then the base policy.
  acceptsBoard(candidate) {
    return boardingClear(this.world, this.controller.unit, candidate) && super.acceptsBoard(candidate);
  }

  // No crew wait (as the base); commands are taken (as the base);
  // stagnation recovery may run; an interrupt does not save the combat goal.
  acceptsRecovery() {
    return true;
  }

  savesOnInterrupt() {
    return false;
  }

  mountUnreachable(point) {
    const reach = this.ai?.vehicles?.mountReach;
    return reach ? !reach(this.controller.unit, point) : false;
  }

  // -- shared steps ---------------------------------------------------------------------

  // The target is fresh and the bot's flat aim
  // (unnormalized) dotted with the flat direction to it exceeds cosLimit.
  facingTarget(cosLimit) {
    const target = this.tracker.freshTarget;
    if (!target) return false;
    const unit = this.controller.unit;
    target.aimPoint(ca);
    unit.origin(cb);
    normalizeD3DX(f(ca.x - cb.x), 0, f(ca.z - cb.z), cn);
    unit.forward(cb);
    return cosLimit < dot32(cb.x, 0, cb.z, cn.x, cn.y, cn.z);
  }

  // The next look: first 30..75 degrees to a random side, then -2x
  // the last; look 100 m that way (the direction's Y set to 0.0001).
  nextLook() {
    const random = this.random;
    if (this.looks === 0) {
      this.lookAngle = f(drawRange(random, 30, 75) * f(0.017453292));
      if (random.int(0, 1) === 0) this.lookAngle = f(this.lookAngle * -1);
    } else this.lookAngle = f(this.lookAngle * -2);
    const unit = this.controller.unit;
    unit.forward(ca);
    const c = Math.cos(this.lookAngle), s = Math.sin(this.lookAngle);
    const x = ca.x * c + ca.z * s;
    const z = -ca.x * s + ca.z * c;
    this.point.set(f(unit.position.x + f(x * 100)), f(unit.position.y + f(f(0.0001) * 100)), f(unit.position.z + f(z * 100)));
    lookAt(this.controller, this.point);
    this.looks += 1;
  }

  // The bot's flat aim within acos(0.939) of the flat direction to `point`.
  alignedTo(point) {
    const unit = this.controller.unit;
    unit.origin(ca);
    normalizeD3DX(f(point.x - ca.x), 0, f(point.z - ca.z), cn);
    unit.forward(cb);
    normalizeD3DX(cb.x, 0, cb.z, cm);
    return COS_ALIGN < dot32(cn.x, cn.y, cn.z, cm.x, cm.y, cm.z);
  }

  // -- the machine -----------------------------------------------------------------------

  onEnter() {
    const c = this.controller;
    const unit = c.unit;
    const t = now(this.world);
    const k = unit?.kind;
    if (k === 5 && (this.state === 2 || this.state === 3 || this.state === 5 || this.state === 6)) {
      this.requestState(DONE);
      return;
    }
    if (this.state === 3 && k !== 0 && k !== 3) {
      this.requestState(DONE);
      return;
    }
    wantMode(c, 1);
    switch (this.state) {
      case 1:
        this.helper?.stop?.();
        return;
      case 2: {
        this.helper?.stop?.();
        const P = unit.position;
        const dx = f(this.point.x - P.x), dz = f(this.point.z - P.z);
        if (lengthSqXZ32(dx, dz) < 100) {
          normalizeXZ(dx, dz, cn);
          this.point.set(f(P.x + f(cn.x * 15)), f(P.y + f(cn.y * 15)), f(P.z + f(cn.z * 15)));
        }
        lookAt(c, this.point);
        setStance(c, 0);
        this.aligned = false;
        return;
      }
      case 3:
        this.helper?.stop?.();
        this.looks = 0;
        this.nextLook();
        this.aligned = false;
        return;
      case 4: {
        this.helper?.stop?.();
        const target = this.tracker.target;
        if (!target) {
          this.requestState(3);
          return;
        }
        const e = dispatch(c, unit, target, true, this.world, pick);
        this.pattern = e.pattern;
        if (e.pattern) this.patterns.init(e.pattern, e.probability);
        else this.requestState(DONE);
        return;
      }
      case 5:
        this.evade(t);
        return;
      case 6:
        this.helper?.stop?.();
        setStance(c, 2);
        lookAlongPath(c);
        this.proneUntil = f(t + 4);
    }
  }

  // State 5 entry: 20 m straight away from the danger point; or, on a 0
  // draw with the point more than 45 degrees off the heading, a dive to the
  // side (and stop).
  evade(t) {
    const c = this.controller;
    const unit = c.unit;
    const P = unit.position;
    normalizeXZ(f(P.x - this.hazard.x), f(P.z - this.hazard.z), cn);
    ca.set(f(P.x + f(cn.x * 20)), f(P.y + f(cn.y * 20)), f(P.z + f(cn.z * 20)));
    let dived = false;
    if (this.random.int(0, 1) === 0) {
      normalizeXZ(f(ca.x - P.x), f(ca.z - P.z), cn);
      unit.forward(cb);
      normalizeXZ(cb.x, cb.z, cm);
      const crossY = f(f(cn.z * cm.x) - f(cn.x * cm.z));
      const s45 = lookupSin(ANGLE.MELEE);
      if (s45 < crossY) {
        dodge(c, 1);
        this.helper?.stop?.();
        dived = true;
      } else if (crossY < -s45) {
        dodge(c, -1);
        this.helper?.stop?.();
        dived = true;
      }
    }
    if (!dived) helperGoto(this.helper, ca);
    setStance(c, 0);
    lookAlongPath(c);
    this.hazardUntil = f(t + 2);
  }

  onExit() {
    wantMode(this.controller, 2);
    if (this.state === 4) this.patterns.onExit();
  }

  // Phase 0: one service's work.
  behave(dt) {
    const c = this.controller;
    const unit = c.unit;
    const world = this.world;
    if (!unit?.alive) return;
    const t = now(world);
    wantMode(c, 1);
    let skipFire = false;
    this.stationary = false;
    const changed = this.tracker.select(world);
    this.tracker.assignWeaponTarget(world);
    c.alertAnim = true;
    const jetting = this.patterns.state === 11 && this.helper?.flying;
    if (!jetting && this.interrupts?.tick?.()) return;
    switch (this.state) {
      case 2: {
        if (this.alignedTo(this.point)) {
          if (this.tracker.target) {
            this.requestState(4);
            break;
          }
          if (!this.aligned) {
            this.aligned = true;
            this.alignAt = f(t + f(0.75));
          } else if (this.alignAt < t) {
            this.requestState(3);
            break;
          }
        }
        lookAt(c, this.point);
        break;
      }
      case 3:
        if (this.tracker.target) {
          this.requestState(4);
          break;
        }
        if (this.alignedTo(this.point)) {
          if (!this.aligned) {
            this.aligned = true;
            this.alignAt = f(t + f(0.1));
          } else if (this.alignAt < t) {
            if (this.looks < 2) {
              this.nextLook();
              this.aligned = false;
            } else this.requestState(DONE);
            break;
          }
        }
        fastTurn(c);
        break;
      case 4: {
        if (jetting) {
          this.patterns.update(dt, false);
          break;
        }
        const target = this.tracker.target;
        if (target) {
          const P = unit.position;
          const T = this.tracker.point;
          const limit = f(sensorRange(unit, c) + 10);
          if (f(limit * limit) < lengthSqXZ32(f(T.x - P.x), f(T.z - P.z))) this.requestState(DONE);
          const e = dispatch(c, unit, target, true, world, pick);
          if (e.pattern) {
            if (!changed && e.pattern === this.pattern) {
              this.patterns.update(dt, false);
              skipFire = this.patterns.skipFire;
              if (unit.kind === 3 && this.raiseShield(unit)) skipFire = true;
              break;
            }
            this.requestState(4);
            break;
          }
        }
        this.requestState(3);
        break;
      }
      case 5:
        if (this.hazardUntil < t) this.requestState(3);
        break;
      case 6:
        if (this.proneUntil < t) this.requestState(3);
        break;
    }
    this.helper?.tick?.(dt);
    if (!skipFire) this.tracker.fireControls(world);
  }

  // A droideka with its shield down and more than half its energy selects
  // the shield (inventory index 1) and presses it directly.
  raiseShield(unit) {
    const shield = unit.soldier?.shield;
    if (!shield || shield.on) return false;
    if (!(f(f(shield.max ?? 0) * 0.5) < (shield.value ?? 0))) return false;
    switchWeapon(this.controller, unit, 1);
    this.controller.fireA = true;
    this.controller.fireB = false;
    return true;
  }
}

// No valid hostile target (living, finite health, hostile to the
// bot) within 8 of a boarding candidate, looking at no more than 30 nearby
// units.
const boardFound = [];

export function boardingClear(world, unit, candidate) {
  if (!world?.units || !candidate?.position) return true;
  world.units.query(candidate.position, 8, boardFound, (u) => u !== candidate && u !== unit);
  for (let i = 0; i < boardFound.length && i < 30; i++) if (validTarget(world, unit, boardFound[i])) return false;
  return true;
}

let cosCone = null;

// The point inside the 77.5-degree cone around the bot's flat
// heading (both directions flattened and normalized).
export function inForwardCone(unit, point) {
  cosCone ??= lookupCos(ANGLE.CONE);
  const P = unit.position;
  normalizeXZ(f(point.x - P.x), f(point.z - P.z), cn);
  unit.forward(ca);
  normalizeXZ(ca.x, ca.z, cm);
  return cosCone < dot32(cm.x, cm.y, cm.z, cn.x, cn.y, cn.z);
}

// ---------------------------------------------------------------------------
// The services the controller's combat adapter (index.js) takes
// from this module beyond its defaults: classicBinding(ai) entries win.

const aimDesired = new THREE.Vector3();

export function classicBinding(ai) {
  const world = ai.world;
  // The sides' ai.json (world.classData: { side: ai.json }): the weapons' AI
  // ranges, masks and bursts.
  for (const data of Object.values(world.classData ?? {})) registerAIData(data);
  const binding = {
    // Each bind (spawn): fresh perception, and the shared generator for the
    // bot's weapon spread.
    createPerception(controller) {
      const p = new Perception(controller);
      p.reset();
      return p;
    },
    attachController(controller) {
      const arsenal = controller.unit?.combat?.arsenal;
      if (arsenal) useClassicRandom(arsenal, rngA(world));
    },
    detachController(controller) {
      if (controller.perception) controller.perception.candidate = null;
    },
    // At a scheduler cycle wrap.
    rebuildProtected() {
      rebuildProtectedHumans(world);
    },
    // Aim state 0: the target led by the AI's accumulated dt (false without
    // a weapon in hand: the scheduler then looks 30 m ahead).
    leadPoint(controller, target, dt, out) {
      const unit = controller.unit;
      const weapon = selectedWeapon(unit, controller);
      if (!unit || !weapon) return false;
      leadPoint(dt, unit, target, weapon, out);
      return true;
    },
    aimPoint(controller, unit, dt, out) {
      return aimPointFor(controller, unit, dt, out);
    },
    // AI input execution: the fire buttons, and (until the integrator calls
    // aim() after the body moves) the Aimer's direction for intent.aim.
    consumeFire(controller, unit, step, intent) {
      consumeFire(controller, unit, world);
      if (!binding.aimHooked && intent) intent.aim = binding.updateAim(controller, step);
    },
    // The faithful order: every step after soldier.tick, before the weapons
    // update (src/units.js Unit.tick), as the soldier update runs the filter
    // in the 2004 game. Writes intent.aim (what becomes aimOverride) and
    // returns it.
    aim(controller, dt) {
      binding.aimHooked = true;
      const direction = binding.updateAim(controller, dt);
      const intent = controller.unit?.intent;
      if (intent) intent.aim = direction;
      return direction;
    },
    updateAim(controller, dt) {
      const unit = controller.unit;
      const p = controller.perception;
      if (!unit?.alive || !p) return null;
      unit.forward(aimDesired);
      const curves = curvesOf(controller, unit, ai);
      return p.aimer.update(aimDesired, dt, curves, selectedWeapon(unit, controller), unit.hero);
    },
    aimHooked: false,
    markSkip() {},
    // The event journal: targeted shots and damage become controller alerts.
    // Call once per step before ai.update.
    reader: world.events?.reader?.() ?? null,
    ingest() {
      binding.reader?.read((e) => ingestEvent(world, e));
    },
    // Each simulation step, for each live grenade or sticky round: the
    // danger broadcast. firstContact: the time it first touched
    // something (Infinity before); the notifier's state lives on the round.
    // The first soldier warned shouts its class FleeSound (a "vo" record).
    grenadeUpdate(round, position, owner, firstContact = Infinity) {
      const state = (round.aiDanger ||= { firstContact: Infinity, lastNotify: -1 });
      if (firstContact < state.firstContact) state.firstContact = firstContact;
      const skill = world.difficulty?.enemySkill ?? 14;
      return notifyDanger(world, state, position, owner, skill, (u) => emitVo(ai, u, { event: "flee", property: "FleeSound" }));
    },
  };
  return binding;
}

// The side's curves: the controller's when Population bound them, else the
// world difficulty's for the unit's team.
function curvesOf(controller, unit, ai) {
  const d = controller.difficulty;
  if (d && Number.isFinite(d.A)) return d;
  return ai?.curvesFor?.(unit.team) ?? d?.curves?.(unit.team) ?? null;
}

// A shot alerts the shooter's target: the event's `target` when the
// producer names one, else a bot shooter's channel target (the 2004 game's
// per-channel fallback: the controllable's SetTarget writes whatever the
// weapon's masks say, not the weapon's own masked
// target, so a rifle shot at a turret still alerts its gunner). The alert
// carries the shooter's position.
function ingestEvent(world, e) {
  if (e.kind === "shot") {
    const shooter = eventUnit(e.source);
    const controller = shooter?.controller;
    const target = eventUnit(e.target) ?? (controller?.perception?.fire ? resolve(controller.target) : null);
    if (shooter && target) notifyTargetedShot(world, shooter, target, shooter.position);
  } else if (e.kind === "damage") {
    const victim = eventUnit(e.target);
    const attacker = eventUnit(e.attacker) ?? e.attackerCharacter ?? null;
    if (victim && attacker) {
      notifyDamage(world, victim, attacker, { amount: e.amount, generic: e.generic ?? 1, typed: e.typed ?? 1 },
        Boolean(e.known), e.point ? vec(e.point) : null);
    }
  }
}

function eventUnit(ref) {
  if (!ref) return null;
  if (ref.unit) return resolve(ref);
  return ref.alive !== undefined ? (ref.alive ? ref : null) : null;
}

function vec(a) {
  return Array.isArray(a) ? new THREE.Vector3(a[0], a[1], a[2]) : a ?? null;
}
