import * as THREE from "three";
import { makeHit } from "./collision.js";
import { DROIDEKA_COLLISION, collisionRule, contactDamage } from "./collision-damage.js";
import { Pose } from "./anim.js";
import { barrelTips } from "./muzzles.js";

// The droideka (cis_inf_droideka, ODF class walkerdroid), as the 2004
// game runs it. It stands in for a soldier: the same
// fields the rest of the game reads, and its own animator.
//
// Modes, each a clip from its own bank played at AnimationSpeed (2x): idle,
// turning, dying, the four walking steps, folding, the ball, and unfolding.
// Clips don't loop; when one ends the next mode comes from the original game's
// table for what the droideka wants (stop, forward, backward, turn, die,
// roll up), the leftover time running on into it. Walking backward plays the
// steps in reverse.
//
// Deployed it walks only by its clips: each frame it moves by the change in
// DummyRoot's forward travel, along the aim heading (about 2.3 m/s;
// MaxSpeed is never used). Turning (MaxTurnSpeed, the strafe keys turning
// too) swings the body while the legs stay put, up to TurnThreshold behind,
// when a turn clip brings them round 30 degrees. The legs' lag shows as
// DummyRoot turned by it and the spine twisted back a third of it each.
//
// The use key folds it into a ball (if there is room it unfolds again).
// Rolling keeps the original top speed, with direct steering at any throttle,
// acceleration only from movement input, and braking when input is released.
// Each movement step ends in collision resolution; there is no unchecked
// second integration after the collision pass.
//
// Its shield (the secondary weapon) starts full and down; each press of the
// secondary trigger raises or lowers it. Up, it drains at AddShield a second
// and takes all damage first (by the attack's ShieldScale), the rest going
// to health; down it refills at AddShieldOff. It drops at 0 and while rolled.
// Its twin blasters fire from hp_gun_1 and hp_gun_2 in turn, at the point
// under the crosshair, from an eye at EyePointOffset; nothing fires while it
// folds, rolls, or unfolds. Dying, it plays death01 with a blast at one of
// its nodes every 0.4 s, then goes up in its ExplosionName and its pieces;
// rolled, it goes up at once. Its five pieces all hang from dummyroot and are
// modelled in the standing pose: the classic pieces (debris.js) rebuild a
// standing droideka there, while Enhanced (destruction.js) carries each on
// the bone whose part of the body it covers.

const GRAVITY = 9.8;
const UP = new THREE.Vector3(0, 1, 0);
const DOWN = new THREE.Vector3(0, -1, 0);
const CLIPS = [
  "idle", "turnl", "turnr", "death01", null, "idle_to_leftfoot", null,
  "walk_leftfoot_rightfoot", "walk_rightfoot_leftfoot", "leftfoot_to_idle", "rightfoot_to_idle", "fold", "roll", "unfold",
];
// Next mode by [mode][action]; actions: 0 stop, 1 stop (in reverse), 2
// forward, 3 backward, 4 turn left, 5 turn right, 6 die, 7 roll up.
const NEXT = [
  [0, 0, 5, 9, 1, 2, 3, 11],
  [0, 0, 5, 9, 0, 0, 3, 11],
  [0, 0, 5, 10, 0, 0, 3, 11],
  [4, 4, 4, 4, 4, 4, 4, 11],
  [4, 4, 4, 4, 4, 4, 4, 11],
  [9, 0, 7, 10, 9, 9, 3, 11],
  [10, 0, 8, 9, 10, 10, 3, 11],
  [10, 5, 8, 8, 10, 10, 3, 11],
  [9, 7, 7, 7, 9, 9, 3, 11],
  [0, 5, 5, 8, 1, 2, 3, 11],
  [0, 7, 5, 7, 1, 2, 3, 11],
  [12, 12, 12, 12, 12, 12, 4, 12],
  [13, 13, 12, 12, 12, 12, 4, 12],
  [0, 0, 0, 0, 0, 0, 4, 0],
];
const FOLD = 11;
const BALL = 12;
const UNFOLD = 13;
const DEATH = 3;
const DEAD = 4;
const TURN_STEP = Math.PI / 6; // a turn clip brings the legs round 30 degrees
const LEG_EASE = 3.2; // 1/s, the legs' lag dying away outside idle and turns
const BALL_CENTER = 0.65; // m, the ball's centre above the origin (BallRadius)
const BODY_RADIUS = 1.0; // m, p_base's radius, deployed
const GROUND = 0.7; // normal.y above which a deployed contact is ground
const BALL_GROUND = 0.6;
const TRACTION = 0.65; // preserve the original 13 m/s cap with one physics step
const HEADROOM = 1.5; // m of clearance the unfold test wants on an axis

// Whether a droideka (or its record, or its presented state) has gone up:
// dead, its death clip played out (or dead rolled, at once). Its final blast
// and pieces went off then (onExploded), and its body is gone for that life.
export function goneUp(state) {
  return Boolean(state?.dead && state.mode === DEAD);
}

export class Droideka {
  constructor(world, def, clips, actor) {
    this.world = world;
    this.def = def;
    this.clips = clips;
    this.actor = actor;
    this.maxTurn = def.MaxTurnSpeed ?? 2;
    this.pitchLimits = (def.PitchLimits || [-28.6, 28.6]).map((d) => (d * Math.PI) / 180);
    this.speed = def.AnimationSpeed ?? 1;
    this.turnThreshold = def.TurnThreshold ?? 0.5;
    this.ballRadius = def.BallRadius ?? BALL_CENTER;
    this.maxSpin = (def.BallMoveSpeed ?? 10) * (def.BallSpinScale ?? 2);
    this.maxBallSlope = Math.cos(((def.MaxBallAngle ?? 35) * Math.PI) / 180);
    // What the rest of the game reads off a soldier.
    this.jet = { recharge: 0, minBorder: 0 };
    this.fuel = 0;
    this.maxFlyHeight = 200;
    this.getupTime = 0;
    this.diveTime = 0;
    this.position = new THREE.Vector3();
    this.previous = new THREE.Vector3();
    this.settled = new THREE.Vector3();
    this.velocity = new THREE.Vector3();
    this.aimYaw = 0;
    this.aimPitch = 0;
    this.air = null;
    this.dive = 0;
    this.lock = 0;
    this.stateTime = 0;
    this.grounded = true;
    this.mode = 0;
    this.time = 0; // s into the mode's clip, on the clip's own clock
    this.reverse = false;
    this.legs = 0; // how far the legs trail the body, rad
    this.spin = 0; // the ball's spin, rad/s
    this.roll = 0; // the ball's turn about its axle, for drawing
    this.lean = 0;
    this.dead = false;
    this.onDeathBlast = () => {}; // a blast during the death clip
    this.onExploded = () => {}; // the end of the death clip
    this.onShieldHit = () => {}; // actual absorbed damage, with optional world source
    this.shield = null;
    this.guns = [];
    this.gun = 0;
    this.eyeHeight = (def.cameras?.STAND?.EyePointOffset || [0, 1.8, 0])[1];
    this.blastClock = 0;
    this.lookX = 0;
    this.lookY = 0;
    this.hit = makeHit();
    // Near-horizontal building and prop contacts since a bot's path executor
    // last read them (as Soldier.avoidance).
    this.avoidance = new THREE.Vector3();
    // Landing too hard hurts (src/collision-damage.js), as a soldier's does.
    this.collisionRule = collisionRule(def, DROIDEKA_COLLISION);
    this.collisionDamage = 0;
    this.v = [new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3()];
    this.closest = new THREE.Vector3();
    this.rootIndex = actor.bones.findIndex((bone) => bone.name.toLowerCase() === "dummyroot");
    this.animator = new DroidekaAnimator(this, actor, clips);
  }

  // "stand" deployed, "crouch" (its rolled camera section) folding, rolled,
  // or unfolding.
  get posture() {
    return this.mode >= FOLD ? "crouch" : "stand";
  }

  get rolled() {
    return this.mode >= FOLD;
  }

  // Its weapons: the blaster's aimers (hardpoint names) and the shield.
  arm(weapons) {
    const blaster = weapons.find((w) => w.aimers?.length);
    const byName = (name) => this.actor.bones.find((bone) => bone.name.toLowerCase() === name.toLowerCase());
    this.guns = (blaster?.aimers || []).map(byName).filter(Boolean);
    this.gunBarrels = this.guns.map(gun => {
      const body = this.actor.body, bone = gun.parent;
      const index = this.actor.bones.indexOf(bone);
      if (!body?.isSkinnedMesh || index < 0) return [gun];
      const position = body.geometry.attributes.position, skin = body.geometry.attributes.skinIndex;
      const toBone = new THREE.Matrix4().multiplyMatrices(body.skeleton.boneInverses[index], body.bindMatrix);
      const points = [];
      for (let i = 0; i < position.count; i++) if (skin.getX(i) === index) points.push(new THREE.Vector3().fromBufferAttribute(position, i).applyMatrix4(toBone));
      const tips = barrelTips(points, "x", 1, "z");
      return tips.length ? tips.map((point, i) => {
        const muzzle = new THREE.Object3D(); muzzle.name = `${gun.name}_barrel_${i}`;
        muzzle.position.copy(point); bone.add(muzzle); return muzzle;
      }) : [gun];
    });
    // Collision muzzles use a small, independent skeleton. Render LOD and
    // interpolation may pose the visible body as often as the display needs.
    const bones = this.actor.bones.map(bone => bone.clone(false));
    const root = new THREE.Group();
    this.actor.bones.forEach((bone, i) => {
      const parent = this.actor.bones.indexOf(bone.parent);
      (parent < 0 ? root : bones[parent]).add(bones[i]);
    });
    const actor = { root, bones };
    this.muzzleAnimator = new DroidekaAnimator(this, actor, this.animator.clips);
    this.muzzleAnimator.rollCenter.copy(this.animator.rollCenter);
    this.muzzleActor = actor;
    this.physicalBarrels = this.gunBarrels.map(barrels => barrels.map(barrel => {
      const muzzle = barrel.clone(false);
      const parent = this.actor.bones.indexOf(barrel.parent);
      (parent < 0 ? root : bones[parent]).add(muzzle);
      return muzzle;
    }));
    this.sampleMuzzles(0);
    const shield = weapons.find((w) => w.base === "shield");
    if (shield) {
      this.shield = {
        def: shield,
        max: shield.MaxShield || 0,
        value: shield.MaxShield || 0,
        on: false,
        drain: shield.AddShield ?? 0,
        refill: shield.AddShieldOff ?? 0,
      };
    }
  }

  toggleShield() {
    const shield = this.shield;
    if (!shield || this.rolled || this.dead) return;
    shield.on = !shield.on && shield.value > 0;
  }

  // Damage on the shield first: returns what is left for health. amount is
  // the health damage, onShield what the same attack does to a shield.
  absorb(amount, onShield = amount, source = null, kind = null) {
    const shield = this.shield;
    if (!shield?.on || !(onShield > 0)) return amount;
    const taken = Math.min(shield.value, onShield);
    shield.value -= taken;
    if (taken > 0) this.onShieldHit(taken, source, kind);
    if (shield.value <= 0) shield.on = false;
    return amount * (1 - taken / onShield);
  }

  // The next gun's muzzle, in turn.
  nextGun(out) {
    if (!this.guns.length) return null;
    const index = this.gun % this.guns.length;
    const barrel = Math.floor(this.gun / this.guns.length) % (this.gunBarrels?.[index]?.length || 1);
    this.lastGun = { index, barrel };
    this.gun = (this.gun + 1) % (this.guns.length * 2);
    return this.gunAt(out, this.lastGun);
  }

  gunAt(out, selected = this.lastGun) {
    const bone = this.physicalBarrels?.[selected?.index]?.[selected?.barrel]
      || this.gunBarrels?.[selected?.index]?.[selected?.barrel] || this.guns[selected?.index];
    if (!bone) return null;
    bone.updateWorldMatrix(true, false);
    return out.setFromMatrixPosition(bone.matrixWorld);
  }

  sampleMuzzles(dt) {
    if (!this.muzzleActor) return;
    this.muzzleActor.root.position.copy(this.position);
    this.muzzleAnimator.update(dt, this);
    this.muzzleActor.root.updateMatrixWorld(true);
  }

  // The next gun's visible muzzle, in turn: the page's cosmetic copy of a
  // round leaves from the barrel as drawn (nextGun is the worker's, off
  // the physical rig). Advancing the same counter keeps the alternation
  // the authority fires with.
  nextVisualGun(out) {
    if (!this.guns?.length) return null;
    const index = this.gun % this.guns.length;
    const barrel = Math.floor(this.gun / this.guns.length) % (this.gunBarrels?.[index]?.length || 1);
    this.lastGun = { index, barrel };
    this.gun = (this.gun + 1) % (this.guns.length * 2);
    return this.visualGunAt(out, this.lastGun);
  }

  visualGunAt(out, selected = this.lastGun) {
    const bone = this.gunBarrels?.[selected?.index]?.[selected?.barrel] || this.guns[selected?.index];
    if (!bone) return null;
    bone.updateWorldMatrix(true, false);
    return out.setFromMatrixPosition(bone.matrixWorld);
  }

  // Where it sees from: EyePointOffset over its origin (in first person
  // too, with no pull-back).
  eye(feet, out) {
    return out.set(feet.x, feet.y + (this.rolled ? 1.0 : this.eyeHeight), feet.z);
  }

  // The aim for PlayerCombat: from the eye along the aim, converging on the
  // point under the crosshair (from the camera brought level with the eye in
  // third person), up to 512 m.
  aim(combat) {
    const yaw = this.aimYaw;
    const p = this.aimPitch;
    const forward = combat.aimDir.set(Math.sin(yaw) * Math.cos(p), Math.sin(p), Math.cos(yaw) * Math.cos(p));
    combat.left.set(Math.cos(yaw), 0, -Math.sin(yaw));
    combat.up.crossVectors(forward, combat.left);
    this.eye(this.position, combat.origin);
    combat.pivot.copy(combat.origin);
    // A bot (no camera) fires along its aimer's direction.
    if (!combat.camera) {
      combat.shotDir.copy(combat.aimOverride || forward).normalize();
      combat.target.copy(combat.origin).addScaledVector(combat.shotDir, 512);
      return;
    }
    const from = combat.from.copy(combat.origin);
    if (!combat.firstPerson) {
      const camera = combat.camera.position;
      const behind = from.subVectors(camera, combat.origin).dot(forward);
      from.copy(camera).addScaledVector(forward, -behind);
    }
    const hit = this.world.cast(from, forward, 512, this.hit);
    combat.target.copy(from).addScaledVector(forward, hit ? hit.distance : 512);
    combat.shotDir.subVectors(combat.target, combat.origin).normalize();
  }

  pitchRange() {
    return this.pitchLimits;
  }

  spawn(x, y, z, yaw) {
    this.position.set(x, y, z);
    const from = this.v[0].set(x, y + 1.5, z);
    const hit = this.world.cast(from, DOWN, 50, this.hit);
    if (hit) this.position.y = hit.point.y;
    this.velocity.set(0, 0, 0);
    this.aimYaw = yaw;
    this.aimPitch = 0;
    this.legs = 0;
    this.spin = 0;
    this.dead = false;
    // Each life's shield starts full and down, whatever the body's last life
    // (a pooled bot's, the player's kept body) left it.
    if (this.shield) {
      this.shield.on = false;
      this.shield.value = this.shield.max;
    }
    this.enter(0);
    this.previous.copy(this.position);
    this.settled.copy(this.position);
    this.animator.reset(yaw);
    this.muzzleAnimator?.reset(yaw);
  }

  // Mouse look: turning and pitching at MaxTurnSpeed per 2048 counts.
  look(dx, dy, sensitivity = 1, invert = false) {
    this.lookX += dx * sensitivity;
    this.lookY += (invert ? dy : -dy) * sensitivity;
  }

  clip(mode = this.mode) {
    return CLIPS[mode] ? this.clips.get(CLIPS[mode]) : null;
  }

  // A mode's length on the game's clock: its frames at 30 fps, sped up.
  length(mode = this.mode) {
    const clip = this.clip(mode);
    return clip ? clip.frames / 30 / this.speed : 0;
  }

  enter(mode, carry = 0) {
    const was = this.mode;
    this.mode = mode;
    // Only the walking steps play in reverse.
    if (mode < 5 || mode > 10) this.reverse = false;
    this.time = this.reverse ? this.length(mode) - carry : carry;
    if (was === BALL && mode !== BALL) {
      this.velocity.set(0, 0, 0);
      this.legs = 0;
    }
    if (mode === BALL) this.spin = 0;
    if (mode === DEATH) this.blastClock = 0;
    if (mode === DEAD && was !== DEAD) this.onExploded();
  }

  // The action the droideka wants now.
  want(move, dead) {
    if (dead) return 6;
    if (Math.abs(move) < 1e-3) return this.reverse ? 1 : 0;
    return move > 0 ? 2 : 3;
  }

  // input: forward, right, turn in [-1, 1]; use is a press (fold or unfold).
  tick(input, dt) {
    this.previous.copy(this.position);
    this.stateTime += dt;
    this.collisionDamage = 0;
    const dead = this.dead;
    const move = dead ? 0 : input.forward || 0;
    const strafe = dead ? 0 : input.right || 0;
    // The strafe keys turn it, and the mouse.
    let turn = dead ? 0 : THREE.MathUtils.clamp((input.turn || 0) + strafe, -1, 1);
    const mouseTurn = dead ? 0 : this.lookX * (this.maxTurn / 2048);
    // A controller's pitch stick turns at MaxPitchSpeed (a bot's; the
    // player pitches with the mouse).
    const stickPitch = dead ? 0 : (input.pitch || 0) * (this.def.MaxPitchSpeed ?? 0.5) * dt;
    const mousePitch = dead ? 0 : this.lookY * (this.maxTurn / 2048) + stickPitch;
    this.lookX = 0;
    this.lookY = 0;
    if (this.mode === FOLD) turn = 0;

    // Fold, or unfold where there is room.
    if (input.use && !dead) {
      if (this.mode === BALL) {
        if (this.roomToStand()) this.enter(NEXT[BALL][0]);
      } else if (this.mode < FOLD && this.mode !== DEATH && this.mode !== DEAD) this.enter(NEXT[this.mode][7]);
    }
    if (dead && this.mode !== DEATH && this.mode !== DEAD) this.enter(NEXT[this.mode][6]);

    this.aimPitch = THREE.MathUtils.clamp(this.aimPitch + mousePitch, this.pitchLimits[0], this.pitchLimits[1]);
    // The shield drains while up and refills while down.
    const shield = this.shield;
    if (shield) {
      shield.value = THREE.MathUtils.clamp(shield.value + (shield.on ? shield.drain : shield.refill) * dt, 0, shield.max);
      if (shield.value <= 0 || this.rolled) shield.on = false;
    }
    if (this.mode === BALL) this.ballMove(dt, move, turn, mouseTurn);
    else this.walkMove(dt, move, turn, mouseTurn);

    // Resolve every displacement, including gravity, before exposing position.
    this.integrate(dt);
    this.collide(dt);
    this.settled.copy(this.position);

    // Dying: blasts at its nodes every 0.4 s, then it explodes.
    if (this.mode === DEATH) {
      this.blastClock -= dt;
      if (this.blastClock <= 0) {
        this.blastClock += 0.4;
        this.onDeathBlast();
      }
    }
  }

  integrate(dt) {
    this.velocity.y -= GRAVITY * dt;
    if (this.mode === BALL) this.position.addScaledVector(this.velocity, dt);
    else this.position.y += this.velocity.y * dt;
  }

  // Deployed: the clips walk it, and the body turns over its legs.
  walkMove(dt, move, turn, mouseTurn) {
    const rate = this.maxTurn;
    const a = turn * rate * dt + mouseTurn;
    this.aimYaw -= a;
    this.legs += a;
    const moving = 5 * Math.abs(move) > Math.abs(turn);
    if (this.mode === 0 && moving) this.enter(NEXT[0][move > 0 ? 2 : 3]);
    else if (this.mode === 0 && (turn || mouseTurn) && Math.abs(Math.sin(this.legs / 6)) > Math.sin(this.turnThreshold / 6)) {
      // The legs left behind on the left: the right turn brings them round.
      this.enter(NEXT[0][this.legs > 0 ? 5 : 4]);
    }
    if (this.mode > 2 && this.mode !== DEATH && this.mode !== DEAD) this.legs *= Math.exp(-LEG_EASE * dt);
    // Walking backward plays the steps in reverse; a change of sign turns
    // the clip round where it is.
    if (this.mode >= 5 && this.mode <= 10 && moving) this.reverse = move < 0;
    this.advance(dt, move, moving);
  }

  // Runs the mode's clip, moving by its root travel, and on to the next
  // mode at its end.
  advance(dt, move, moving) {
    let left = dt;
    let walked = 0;
    // Its walking speed, for those who read a velocity (the camera's bob,
    // the arms).
    const heading = this.aimYaw;
    const report = () => {
      const speed = dt > 0 ? walked / dt : 0;
      this.velocity.x = Math.sin(heading) * speed;
      this.velocity.z = Math.cos(heading) * speed;
    };
    for (let guard = 0; guard < 4 && left > 0; guard++) {
      const mode = this.mode;
      const clip = this.clip(mode);
      const length = this.length(mode);
      if (!clip || mode === BALL || mode === DEAD) break;
      const step = left * (this.reverse && mode >= 5 && mode <= 10 ? -1 : 1);
      const from = this.time;
      let to = from + step;
      let over = 0;
      if (to > length) {
        over = to - length;
        to = length;
      } else if (to < 0) {
        over = -to;
        to = 0;
      }
      // Root travel follows the aim immediately; leg lag remains visual.
      if (mode >= 5 && mode <= 10) {
        const travel = this.travel(clip, to) - this.travel(clip, from);
        const heading = this.aimYaw;
        this.position.x += Math.sin(heading) * travel;
        this.position.z += Math.cos(heading) * travel;
        walked += travel;
      }
      if (mode === 1 || mode === 2) {
        // The turn clip brings the legs round toward the body.
        const share = (to - from) / Math.max(length, 1e-3);
        this.legs -= Math.sign(this.legs) * Math.min(Math.abs(this.legs), TURN_STEP * Math.abs(share));
      }
      this.time = to;
      left = over;
      if (over <= 0 && to > 0 && to < length) {
        report();
        return;
      }
      if (mode === DEATH && to >= length) {
        this.enter(DEAD);
        return;
      }
      this.enter(NEXT[mode][this.want(moving ? move : 0, this.dead)], 0);
    }
    report();
  }

  // DummyRoot's forward travel at a time on the mode's clock.
  travel(clip, t) {
    if (this.rootIndex < 0) return 0;
    const k = clip.tracks.indexOf(this.rootIndex);
    if (k < 0) return 0;
    const frames = clip.frames;
    const f = Math.min(Math.max(t * this.speed * clip.fps, 0), frames - 1);
    const i0 = Math.floor(f);
    const i1 = Math.min(i0 + 1, frames - 1);
    const d = clip.data;
    const z0 = d[(k * frames + i0) * 7 + 6];
    const z1 = d[(k * frames + i1) * 7 + 6];
    return z0 + (z1 - z0) * (f - i0);
  }

  // The ball: spin, turn, and the lean into a turn.
  ballMove(dt, move, turn, mouseTurn) {
    const d = this.def;
    const rate = d.BallTurnSpeed ?? 2;
    this.aimYaw -= turn * rate * dt + mouseTurn;
    const targetLean = THREE.MathUtils.degToRad(d.BallMaxLean ?? 10) * turn * Math.abs(this.spin / this.maxSpin);
    this.lean += (targetLean - this.lean) * (1 - Math.exp(-12 * dt));
    if (Math.abs(move) > 1e-3) {
      this.spin = THREE.MathUtils.clamp(this.spin + (d.BallAcceleration ?? 100) * move * dt, -this.maxSpin, this.maxSpin);
    } else if (this.grounded) {
      this.spin -= Math.sign(this.spin) * Math.min(Math.abs(this.spin), (d.BallRollingFriction ?? 50) * dt);
    }
    if (this.grounded) {
      this.velocity.x = Math.sin(this.aimYaw) * TRACTION * this.spin;
      this.velocity.z = Math.cos(this.aimYaw) * TRACTION * this.spin;
    }
    this.roll += this.spin * dt;
  }

  // Against the buildings and terrain: deployed, its base stands on what
  // faces up and slides off what doesn't; rolled, its ball slides along walls and
  // takes traction where the ground is flat enough.
  collide(dt) {
    const ball = this.mode === BALL;
    // Nearby-triangle caches for the base and the ball (collision.js).
    if (this.world.gatherCache) this.gatherCaches ??= [this.world.gatherCache(), this.world.gatherCache()];
    const r = ball ? this.ballRadius : BODY_RADIUS;
    const [c, n, forward, right] = this.v;
    let grounded = false;
    for (let pass = 0; pass < 4; pass++) {
      c.set(this.position.x, this.position.y + r, this.position.z);
      let best = Infinity;
      let moving = false;
      for (const polygon of this.world.gather(c, r, false, this.gatherCaches?.[ball ? 1 : 0])) {
        for (const tri of polygon.triangles) {
          tri.closestPointToPoint(c, this.closest);
          const distance = this.closest.distanceTo(c);
          if (distance < r && distance < best && distance > 1e-5) {
            best = distance;
            moving = Boolean(polygon.moving);
            n.subVectors(c, this.closest).divideScalar(distance);
          }
        }
      }
      let depth = best < Infinity ? r - best : 0;
      let wall = depth > 0;
      // The terrain is a height field under the centre.
      const ground = this.world.cast(c, DOWN, r + 0.05, this.hit);
      if (ground && ground.terrain && r - ground.distance > depth) {
        depth = r - ground.distance;
        n.copy(ground.normal);
        wall = false;
        moving = false;
      }
      if (depth <= 0) break;
      if (wall && Math.abs(n.y) < 0.3) this.avoidance.add(n);
      if (n.y > (ball ? this.maxBallSlope : GROUND)) {
        grounded = true;
        // Coming down on it too fast hurts (src/collision-damage.js). Only
        // what it stands on: the port's rolling ball is driven into a wall
        // again every tick, where the original game's bounces off it once.
        if (!moving) {
          const facing = n.x * Math.sin(this.aimYaw) + n.z * Math.cos(this.aimYaw);
          this.collisionDamage += contactDamage(this.collisionRule, this.velocity.dot(n), n.y, facing);
        }
      }
      if (ball) {
        if (n.y <= BALL_GROUND) this.position.addScaledVector(n, depth);
        else this.position.y += depth / n.y;
        const into = this.velocity.dot(n);
        if (into < 0) this.velocity.addScaledVector(n, -into);
        if (n.y > this.maxBallSlope) {
          right.set(Math.cos(this.aimYaw), 0, -Math.sin(this.aimYaw));
          forward.crossVectors(right, n).normalize();
          this.velocity.copy(forward).multiplyScalar(TRACTION * this.spin);
        }
      } else if (n.y > GROUND) {
        this.position.y += depth / n.y;
        if (this.velocity.y < 0) this.velocity.y = 0;
      } else {
        const flat = Math.hypot(n.x, n.z);
        if (flat > 1e-4) {
          this.position.x += (n.x / flat) * depth;
          this.position.z += (n.z / flat) * depth;
        }
      }
    }
    this.grounded = grounded;
  }

  // Room to unfold: headroom straight up, and on each of four axes at 1.5 m
  // up at least one side clear for 1.5 m.
  roomToStand() {
    const from = this.v[0].set(this.position.x, this.position.y + 1.5, this.position.z);
    const dir = this.v[1];
    if (this.world.cast(this.v[2].set(this.position.x, this.position.y + 0.7, this.position.z), UP, 1.1 * 2.2, this.hit)) return false;
    for (const [x, z] of [[1, 0], [0, 1], [0.707, 0.707], [0.707, -0.707]]) {
      const plus = !this.world.cast(from, dir.set(x, 0, z), HEADROOM, this.hit);
      const minus = !this.world.cast(from, dir.set(-x, 0, -z), HEADROOM, this.hit);
      if (!plus && !minus) return false;
    }
    return true;
  }
}

// The droideka's pose: its mode's clip, the legs' lag shown at DummyRoot and
// twisted back up the spine, the ball spinning on center_box.
export class DroidekaAnimator {
  constructor(droideka, actor, clips) {
    this.d = droideka;
    this.actor = actor;
    this.clips = clips;
    const index = (name) => actor.bones.findIndex((bone) => bone.name.toLowerCase() === name);
    this.root = index("dummyroot");
    this.ball = index("center_box");
    this.spine = ["bone_spine1", "bone_spine2", "bone_spine3"].map(index).filter((i) => i >= 0);
    this.pose = new Pose(actor.bones.length, this.root);
    this.bind = actor.bones.map((bone) => ({ q: bone.quaternion.clone(), p: bone.position.clone() }));
    this.q = new THREE.Quaternion();
    this.axisX = new THREE.Vector3(1, 0, 0);
    this.facing = 0;
    this.legs = 0;
    this.action = "loco";
    this.gait = "idle";
    this.mode = -1;
    this.wentUpRolled = false;
    this.rollOrigin = 0;
    this.transition = 0;
    this.from = this.bind.map(({ q, p }) => ({ q: q.clone(), p: p.clone() }));
    this.pivot = this.ball >= 0 ? this.bind[this.ball].p.clone() : new THREE.Vector3(0, BALL_CENTER, 0);
    this.rollCenter = new THREE.Vector3(0, droideka.ballRadius, 0);
    this.offset = new THREE.Vector3();
    this.spinRotation = new THREE.Quaternion();
    this.axisZ = new THREE.Vector3(0, 0, 1);
    // center_box is an animation control, not the folded shell's axle. Find
    // that axle once from the real skinned roll pose, in its parent's space.
    const roll = clips.get("roll");
    if (this.ball >= 0 && actor.body?.isSkinnedMesh && roll) {
      this.pose.add(roll, 0, 1, false);
      this.pose.apply(actor.bones);
      actor.root.updateMatrixWorld(true);
      const parentInverse = actor.bones[this.ball].parent.matrixWorld.clone().invert();
      const toParent = new THREE.Matrix4().multiplyMatrices(parentInverse, actor.body.matrixWorld);
      const bounds = new THREE.Box3();
      const vertex = new THREE.Vector3();
      const count = actor.body.geometry.attributes.position.count;
      const points = [];
      for (let i = 0; i < count; i++) {
        actor.body.getVertexPosition(i, vertex).applyMatrix4(toParent);
        bounds.expandByPoint(vertex);
        points.push(vertex.clone());
      }
      if (count) {
        bounds.getCenter(this.pivot);
        // Average the silhouette midpoint across axle angles. A single AABB
        // favors whichever protruding foot happens to be vertical at frame 0.
        this.pivot.y = this.pivot.z = 0;
        for (let k = 0; k < 32; k++) {
          const y = Math.cos(k * Math.PI * 2 / 32), z = Math.sin(k * Math.PI * 2 / 32);
          let min = Infinity, max = -Infinity;
          for (const p of points) { const dot = p.y * y + p.z * z; min = Math.min(min, dot); max = Math.max(max, dot); }
          this.pivot.y += (min + max) * y / 32;
          this.pivot.z += (min + max) * z / 32;
        }
        let radius = 0;
        for (const p of points) radius = Math.max(radius, Math.hypot(p.y - this.pivot.y, p.z - this.pivot.z));
        this.rollCenter.y = Math.max(droideka.ballRadius, radius);
      }
      actor.bones.forEach((bone, i) => { bone.quaternion.copy(this.bind[i].q); bone.position.copy(this.bind[i].p); });
      this.pose.clear();
      actor.root.updateMatrixWorld(true);
    }
  }

  // The soldier animator's hooks; the droideka has no weapon clips.
  reset(yaw) {
    this.facing = yaw;
    this.mode = -1;
    this.wentUpRolled = false;
    this.transition = 0;
    this.rollOrigin = this.d.roll;
  }
  setBank() {}
  weaponAction() {}
  endWeaponAction() {}
  fire() {}
  gunner() {}

  die() {
    this.d.dead = true;
  }

  update(dt, droideka) {
    const d = droideka;
    if (d.mode !== this.mode) {
      if (d.mode === BALL) this.rollOrigin = d.roll;
      if (d.mode === DEAD) this.wentUpRolled = this.mode >= FOLD;
      // Unfold starts from a fixed authored orientation; preserve the last
      // displayed pose instead of snapping out of an arbitrary live spin.
      this.transition = d.mode === UNFOLD && this.mode === BALL ? 0.16 : 0;
      if (this.transition) this.actor.bones.forEach((bone, i) => {
        this.from[i].q.copy(bone.quaternion); this.from[i].p.copy(bone.position);
      });
      this.mode = d.mode;
    }
    // Gone up (dead, which has no clip): the pose it went up in, never the
    // bind pose. On its feet that is death01's last frame; rolled, the ball.
    const mode = d.mode === DEAD ? (this.wentUpRolled ? BALL : DEATH) : d.mode;
    const clip = d.clip(mode);
    const pose = this.pose;
    pose.clear();
    if (clip) {
      const t = d.mode === DEAD ? d.length(DEATH) * d.speed : Math.min(Math.max(d.time, 0), d.length()) * d.speed;
      pose.add(clip, mode === BALL ? 0 : t, 1, false);
    }
    for (let i = 0; i < this.actor.bones.length; i++) {
      if (pose.w[i] > 1e-6) continue;
      const bone = this.actor.bones[i];
      bone.quaternion.copy(this.bind[i].q);
      bone.position.copy(this.bind[i].p);
    }
    pose.apply(this.actor.bones);
    // The legs trail the body: DummyRoot turned by the lag, the spine
    // twisted back a third of it at each of its three bones.
    const folded = mode === BALL ? 1 : mode === FOLD
      ? THREE.MathUtils.smoothstep(d.time / Math.max(d.length(), 1e-3), 0.35, 1)
      : mode === UNFOLD ? 1 - THREE.MathUtils.smoothstep(d.time / Math.max(d.length(), 1e-3), 0, 0.3) : 0;
    const lag = d.legs * (1 - folded);
    if (this.root >= 0) this.actor.bones[this.root].quaternion.premultiply(this.q.setFromAxisAngle(UP, lag));
    for (const i of this.spine) this.actor.bones[i].quaternion.multiply(this.q.setFromAxisAngle(this.axisX, -lag / 3));
    // The blasters and head follow vertical aim together; feet keep the
    // walking pose. Fade the correction at the folding clip boundaries.
    const aiming = d.dead || mode === BALL ? 0 : mode === FOLD
      ? 1 - THREE.MathUtils.smoothstep(d.time / Math.max(d.length(), 1e-3), 0, 0.25)
      : mode === UNFOLD ? THREE.MathUtils.smoothstep(d.time / Math.max(d.length(), 1e-3), 0.7, 1) : 1;
    if (this.spine.length && aiming) this.actor.bones[this.spine[0]].quaternion.premultiply(this.q.setFromAxisAngle(this.axisX, -d.aimPitch * aiming));
    if (this.ball >= 0 && mode >= FOLD) {
      const ball = this.actor.bones[this.ball];
      // Bring the folded shell above the contact point gradually, so the
      // fold endpoint and the first rolling frame agree exactly.
      this.offset.subVectors(this.rollCenter, this.pivot).multiplyScalar(folded);
      if (mode === BALL) {
        this.spinRotation.setFromAxisAngle(this.axisZ, d.lean)
          .multiply(this.q.setFromAxisAngle(this.axisX, d.roll - this.rollOrigin));
        ball.position.sub(this.pivot).applyQuaternion(this.spinRotation).add(this.pivot);
        ball.quaternion.premultiply(this.spinRotation);
      }
      ball.position.add(this.offset);
    }
    if (this.transition > 0) {
      this.transition = Math.max(0, this.transition - dt);
      const blend = THREE.MathUtils.smoothstep(1 - this.transition / 0.16, 0, 1);
      this.actor.bones.forEach((bone, i) => {
        bone.quaternion.slerpQuaternions(this.from[i].q, bone.quaternion, blend);
        bone.position.lerpVectors(this.from[i].p, bone.position, blend);
      });
    }
    this.facing = d.aimYaw;
    this.legs = d.legs;
    const root = this.actor.root;
    root.rotation.set(0, d.aimYaw, 0);
  }
}

export { ShieldBubble } from "./droideka-shield.js";
