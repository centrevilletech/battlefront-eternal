// Enhanced aim (docs/ai-enhanced.md).
//
// Classic filters each bot's gun direction through the side's skill curves
// (an acceleration limit, damping, extra spread per shot and extra sniper
// error), with the player's team on the weakest curves. Enhanced replaces
// that filter with one model for every bot:
//
//   - reaction: a new target is not fired at until a reaction time drawn
//     around the tier's median (never under its floor) has passed;
//   - error: the gun points off the body's aim by a slowly wandering angle
//     (correlated over 0.25 s) whose size grows with range and with the
//     bot's own speed, plus a bias drawn at acquisition that fades over the
//     tier's convergence time;
//   - slew: the gun turns no faster than the tier allows;
//   - weapons: the player's spread rules, with no bot-only extra spread
//     (curve B), no extra sniper error (curve S), and the player's spread
//     floor, so a bot's first shots are as tight as a player's.
//
// Each bot also gets a stable +-10% skill variation for the match. The
// numbers start from the design's initial values and are tuned by measurement
// (docs/ai-enhanced.md, "Tuning").

import * as THREE from "three";
import { ClassicRandom } from "../random.js";
import { consumeFire, selectedWeapon, sniperWeapon } from "../classic/weapons-ai.js";

const DEG = Math.PI / 180;

// By tier (1 Easy, 2 Normal, 3 Hard). Times in seconds, angles in degrees.
// Reaction, convergence and slew are the design's initial values. The error
// at 25 m (sigma25) is measured, not the initial 1.2, 0.8, 0.5: with those,
// bots hit with 30% of their rifle rounds, against 18% for Classic Medium's
// enemies, and killed half again as often. These put Normal's rifles at
// Classic Medium's enemy rate and Hard's a step past Classic Hard's
// (docs/ai-enhanced.md, "Tuning").
export const AIM_PROFILES = Object.freeze({
  1: Object.freeze({ name: "easy", reaction: 0.45, reactionFloor: 0.3, convergence: 0.65, sigma25: 3.0, slew: 120 }),
  2: Object.freeze({ name: "normal", reaction: 0.33, reactionFloor: 0.22, convergence: 0.48, sigma25: 1.6, slew: 160 }),
  3: Object.freeze({ name: "hard", reaction: 0.25, reactionFloor: 0.17, convergence: 0.35, sigma25: 1.0, slew: 200 }),
});

export const AIM_MODEL = Object.freeze({
  tau: 0.25, // s: how long the wandering error stays correlated
  bias: 2.5, // the acquisition bias, in error sigmas
  reactionSpread: 0.25, // log-normal spread of the reaction draw
  movement: 0.5, // extra error at full own speed
  variation: 0.1, // +- per-bot skill variation
  retain: 1.5, // s: a target lost for less than this is not a new acquisition
  scope: 0.5, // error through a zoomed sniper scope, as a player's shrinks
});

const resolve = (ref) => {
  const unit = ref?.unit;
  return unit && unit.alive && unit.generation === ref.generation ? unit : null;
};

const _right = new THREE.Vector3();
const _up = new THREE.Vector3();
const _aimed = new THREE.Vector3();
const _origin = new THREE.Vector3();
const _point = new THREE.Vector3();
const UP = new THREE.Vector3(0, 1, 0);

export class EnhancedAimer {
  constructor(random, profile, skill = 1) {
    this.random = random;
    this.profile = profile;
    this.skill = skill; // > 1 worse, < 1 better
    this.direction = new THREE.Vector3(0, 0, 1);
    this.started = false;
    this.error = [0, 0]; // yaw, pitch, in sigmas
    this.bias = [0, 0];
    this.key = null; // the target being aimed at
    this.lastKey = null;
    this.lostAt = -Infinity;
    this.acquiredAt = -Infinity;
    this.readyAt = -Infinity;
  }

  gaussian() {
    const u = Math.max(this.random.uniform(), 1e-12);
    const v = this.random.uniform();
    return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v);
  }

  reset(direction) {
    this.direction.copy(direction);
    this.started = true;
  }

  // Follow the controller's firing target: a new one starts a reaction time
  // and an acquisition bias.
  observe(target, now) {
    const key = target ? `${target.id}:${target.generation}` : null;
    if (key === this.key) return;
    if (!key) {
      this.lastKey = this.key;
      this.lostAt = now;
      this.key = null;
      return;
    }
    this.key = key;
    if (key === this.lastKey && now - this.lostAt < AIM_MODEL.retain) return;
    const p = this.profile;
    this.acquiredAt = now;
    const reaction = p.reaction * this.skill * Math.exp(AIM_MODEL.reactionSpread * this.gaussian());
    this.readyAt = now + Math.max(p.reactionFloor * this.skill, reaction);
    this.bias[0] = AIM_MODEL.bias * this.gaussian();
    this.bias[1] = AIM_MODEL.bias * this.gaussian();
  }

  // Still reacting to the current target: hold fire.
  reacting(now) {
    return this.key != null && now < this.readyAt;
  }

  // The error's size (radians, one sigma) at distance d for a unit moving
  // at `speedFraction` of its top speed, through a scope or not.
  sigma(d, speedFraction, scoped = false) {
    const range = 0.7 + 0.3 * Math.min(Math.max(d / 25, 0), 4);
    const scope = scoped ? AIM_MODEL.scope : 1;
    return this.profile.sigma25 * DEG * this.skill * range * (1 + AIM_MODEL.movement * speedFraction) * scope;
  }

  // desired: the body's aim (unit vector); returns the gun direction.
  update(desired, dt, { distance = 25, speedFraction = 0, scoped = false, now = 0, hero = false } = {}) {
    if (!this.started) this.reset(desired);
    if (hero || !(dt > 0)) {
      if (hero) this.direction.copy(desired);
      return this.direction;
    }
    const a = Math.exp(-dt / AIM_MODEL.tau);
    const b = Math.sqrt(1 - a * a);
    this.error[0] = a * this.error[0] + b * this.gaussian();
    this.error[1] = a * this.error[1] + b * this.gaussian();
    const fade = Math.exp((-3 * Math.max(0, now - this.acquiredAt)) / this.profile.convergence);
    const s = this.sigma(distance, speedFraction, scoped);
    const yaw = s * (this.error[0] + this.bias[0] * fade);
    const pitch = s * (this.error[1] + this.bias[1] * fade);
    // Offset the body's aim by the error, in its own frame.
    _right.crossVectors(desired, UP);
    if (_right.lengthSq() < 1e-8) _right.set(1, 0, 0);
    _right.normalize();
    _up.crossVectors(_right, desired).normalize();
    _aimed.copy(desired).addScaledVector(_right, Math.tan(yaw)).addScaledVector(_up, Math.tan(pitch)).normalize();
    // Turn toward it no faster than the tier's slew rate.
    const angle = this.direction.angleTo(_aimed);
    const step = this.profile.slew * DEG * dt;
    if (angle <= step || !Number.isFinite(angle)) this.direction.copy(_aimed);
    else this.direction.lerp(_aimed, step / angle).normalize();
    if (!Number.isFinite(this.direction.x)) this.direction.copy(desired);
    return this.direction;
  }
}

// The player's spread rules for a bot's weapon: no extra spread or sniper
// error, and the player's floor (src/weapons.js: a player's kick starts at,
// and recovers to, minus SpreadThreshold).
export function playerSpread(weapon) {
  if (!weapon) return;
  weapon.spreadScale = 1;
  weapon.sniperScale = 0;
  if (!weapon.enhancedFloor) {
    weapon.enhancedFloor = true;
    const floor = -(weapon.def?.SpreadThreshold || 0);
    if (floor < weapon.floor) {
      weapon.floor = floor;
      weapon.kick = Math.min(weapon.kick, floor);
    }
  }
}

// The combat services Enhanced replaces (createClassicAI options.combat):
// the fire buttons (held while reacting) and the gun direction.
export function enhancedCombat(world, { tier = 2, seed = 0 } = {}) {
  const profile = AIM_PROFILES[tier] ?? AIM_PROFILES[2];
  const random = new ClassicRandom((seed ^ 0x51ed270b) >>> 0);
  const desired = new THREE.Vector3();
  const skills = new WeakMap(); // Character -> stable skill factor
  let hooked = false;
  const now = () => world.time ?? 0;

  function aimerOf(controller) {
    const p = controller.perception;
    if (!p) return null;
    if (!p.enhancedAimer) {
      const character = controller.character;
      let skill = character ? skills.get(character) : undefined;
      if (skill === undefined) {
        skill = 1 + AIM_MODEL.variation * (2 * random.uniform() - 1);
        if (character) skills.set(character, skill);
      }
      p.enhancedAimer = new EnhancedAimer(random, profile, skill);
    }
    return p.enhancedAimer;
  }

  function updateAim(controller, dt) {
    const unit = controller.unit;
    const p = controller.perception;
    if (!unit?.alive || !p) return null;
    const aimer = aimerOf(controller);
    const target = resolve(controller.target);
    aimer.observe(target, now());
    unit.forward(desired);
    const weapon = selectedWeapon(unit, controller);
    playerSpread(weapon);
    let distance = 25;
    if (target) {
      if (unit.origin) unit.origin(_origin);
      else _origin.copy(unit.position);
      distance = _origin.distanceTo(target.aimPoint ? target.aimPoint(_point) : target.position);
    }
    const top = unit.soldier?.maxSpeed || unit.classDef?.MaxSpeed || 0;
    const speed = unit.velocity ? Math.hypot(unit.velocity.x, unit.velocity.z) : 0;
    const direction = aimer.update(desired, dt, {
      distance,
      speedFraction: top > 0 ? Math.min(speed / top, 1) : 0,
      scoped: Boolean(weapon) && unit.kind === 0 && sniperWeapon(weapon),
      now: now(),
      hero: unit.hero,
    });
    // The fire cone (weapons-ai.js firingCone) reads the Classic Aimer's
    // direction: keep it the gun's.
    if (p.aimer) {
      p.aimer.direction.copy(direction);
      p.aimer.velocity?.set(0, 0, 0);
      p.aimer.started = true;
    }
    return direction;
  }

  return {
    profile,
    aimerOf,
    updateAim,
    consumeFire(controller, unit, step, intent) {
      const aimer = aimerOf(controller);
      if (aimer) {
        aimer.observe(resolve(controller.target), now());
        if (aimer.reacting(now())) {
          controller.fireA = false;
          controller.fireB = false;
        }
      }
      consumeFire(controller, unit, world);
      if (!hooked && intent) intent.aim = updateAim(controller, step);
    },
    aim(controller, dt) {
      hooked = true;
      const direction = updateAim(controller, dt);
      const intent = controller.unit?.intent;
      if (intent) intent.aim = direction;
      return direction;
    },
  };
}
