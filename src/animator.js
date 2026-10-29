import * as THREE from "three";
import { Pose } from "./anim.js";

// Drives the trooper skeleton from the game's own rifle clips, following the
// 2004 engine's soldier animator.
//
// The legs carry an angle relative to the soldier's facing. Standing still
// they stay planted while the facing turns, until that angle passes a limit
// for the posture; then a turn clip steps them round, overshooting a little.
// Moving, the angle eases toward the travel direction, held within 72 degrees
// of the facing: within that the forward cycle plays, beyond it the backward
// one with the legs reversed. Relaxed, the torso and rifle simply follow the
// legs and only the neck and head turn toward the aim; after firing, the alert
// clips play and the spine twists the torso onto the aim. Cycles play at their
// authored speed scaled by the soldier's ground speed.
//
// Every body plays in two layers. The legs, hips, and the weapon hardpoint
// always play the rifle clips, which set all the timing. The 13 upper-body
// joints (spine, ribcage, neck, head, and both arms from the clavicle to the
// hand) play the held weapon's bank (pistol, grenade, the droids' sbdroid and
// bdroid...) for the same action at the same phase, borrowing the clips a
// bank lacks as the original game's tables do (tools/zero_anim.clip_sets); turns
// show the bank's idle there. Alert clips exist only standing.
//
// Weapon actions (shoot, reload, a grenade throw) replace that upper clip, or
// the whole body when standing idle: shoot clips blend in over 0.02 s, the
// rest over 0.5 s.
//
// Of all the joints only the root and the weapon hardpoint take a clip's
// translation; the rest keep their own body's bind lengths, which is how one
// set of clips fits the clone, the battle droid, and the super battle droid.

const CLIPS = {
  stand: {
    idle: "human_rifle_stand_idle_emote",
    walk: "human_rifle_stand_walkforward",
    run: "human_rifle_stand_runforward",
    walkBack: "human_rifle_stand_walkbackward",
    runBack: "human_rifle_stand_runbackward",
    turnLeft: "human_rifle_stand_turnleft",
    turnRight: "human_rifle_stand_turnright",
  },
  crouch: {
    idle: "human_rifle_crouch_idle_emote",
    walk: "human_rifle_crouch_walkforward",
    walkBack: "human_rifle_crouch_walkbackward",
    turnLeft: "human_rifle_crouch_turnleft",
    turnRight: "human_rifle_crouch_turnright",
  },
  prone: {
    idle: "human_rifle_prone_idle_emote",
    walk: "human_rifle_prone_walkforward",
    walkBack: "human_rifle_prone_walkbackward",
    walkLeft: "human_rifle_prone_walkleft",
    walkRight: "human_rifle_prone_walkright",
    turnLeft: "human_rifle_prone_turnleft",
    turnRight: "human_rifle_prone_turnright",
  },
};
const POSTURES = ["stand", "crouch", "prone"];
// The upper layer's joints.
const UPPER = [
  "bone_a_spine", "bone_b_spine", "bone_ribcage", "bone_neck", "bone_head",
  "bone_l_clavicle", "bone_l_upperarm", "bone_l_forearm", "bone_l_hand",
  "bone_r_clavicle", "bone_r_upperarm", "bone_r_forearm", "bone_r_hand",
];
// Joints that keep their bind translation.
const BIND_LENGTH = [
  "bone_pelvis", "bone_a_spine", "bone_b_spine", "bone_ribcage", "bone_neck", "bone_head",
  ...["l", "r"].flatMap((side) => ["clavicle", "upperarm", "forearm", "hand", "thigh", "calf", "foot", "toe"].map((part) => `bone_${side}_${part}`)),
];
// Standing alert, the legs' rifle clips are the alert ones for these.
const ALERT_BASE = new Set(["idle", "run", "runBack"]);
const TRANSITIONS = {
  "stand>prone": "human_rifle_stand_getdown_prone",
  "crouch>prone": "human_rifle_crouch_getdown_prone",
  "prone>stand": "human_rifle_prone_getup_stand",
  "prone>crouch": "human_rifle_prone_getup_crouch",
};

export const ANIM = {
  idleSpeed: 0.3, // m/s to leave idle and start a moving gait
  idleStopSpeed: 0.18, // keep a moving gait through small contact-speed noise
  legLimit: 1.2566371, // rad, furthest the legs turn off the facing
  forwardHold: 1.8849558, // rad of travel still run forward once already going forward
  alertLegLimit: [-0.7853982, 0.5497787], // the alert stance keeps the legs closer
  alertForward: [-1.8064158, 1.1780972],
  runAbove: 5.5, // m/s, walk to run
  walkBelow: 5.0, // m/s, run back to walk
  rateBias: 0.05, // playback = speed / authored speed, pulled this far toward 1
  legEase: 0.25, // share of the way the legs move toward their target each 1/30 s
  legTurnSpeed: 8, // rad/s, cap a forward/backward gait's large visual pivot
  // Idle turn limits per posture: legs this far left or right of the facing.
  turnLimits: { stand: [-0.7068, 0.3927], crouch: [-1.1781, 0.3927], prone: [-0.4712, 0.4712] },
  turnOvershoot: 0.2,
  jumpPose: 0.3, // s on the jump pose before the fall pose
  hardLanding: 2, // s in the air before a landing is a hard one
  softCut: 0.15, // s of landsoft before moving cuts it short
  thrownLanding: 0.25, // s a thrown body shows its landing pose before resting
  thrownBounce: 0.5, // s a thrown soldier who lives holds the bounce pose
  aimYawLimit: 0.6283185,
  aimPitchLimit: 1.0995574,
  aimEase: 0.15, // per 1/30 s
  alertOn: 2.5, // s the alert timer starts at when the soldier fires
  alertHold: 2,
  blend: 0.15, // s crossfade between clips
  shootBlend: 0.02, // s a shoot clip takes to blend in over the upper body
  actionBlend: 0.5, // s other weapon actions take
};

const UP = new THREE.Vector3(0, 1, 0);

// Whether a clip is an idle (its time follows the animator's clock).
const isIdle = (clip) => (clip.idle ??= /_idle_/.test(clip.name));
const scaledRate = (clip, s) => THREE.MathUtils.clamp(((1 - ANIM.rateBias) * s) / Math.max(Math.abs(clip?.speed ?? 0), 0.1) + ANIM.rateBias, 0.1, 2.5);
const gaitTo = (out, gait, legs, rate) => { out.gait = gait; out.legs = legs; out.rate = rate; return out; };

function wrap(a) {
  return Math.atan2(Math.sin(a), Math.cos(a));
}

// Per-update shares were tuned for a 30 Hz update; keep them rate independent.
function ease(share, dt) {
  return 1 - Math.pow(1 - share, dt * 30);
}

export class SoldierAnimator {
  // banks: the clip sets per weapon bank (anims.json "sets"), if extracted.
  constructor(actor, clips, banks = null) {
    this.actor = actor;
    this.bones = actor.bones;
    this.byName = actor.byName;
    this.pose = new Pose(actor.bones.length, actor.byName.DummyRoot.userData.index);
    const pick = (set) => Object.fromEntries(Object.entries(set).map(([key, name]) => [key, clips.get(name)]));
    this.sets = Object.fromEntries(POSTURES.map((p) => [p, pick(CLIPS[p])]));
    this.alertSet = Object.fromEntries(
      Object.entries(CLIPS.stand).map(([key, name]) => [key, (ALERT_BASE.has(key) && clips.get(`${name}_alert`)) || clips.get(name)]),
    );
    this.banks = banks?.banks || null;
    this.bank = "rifle";
    this.upperJoints = UPPER.map((name) => actor.byName[name]?.userData.index).filter((i) => i !== undefined);
    this.upperPose = new Pose(actor.bones.length, actor.byName.DummyRoot.userData.index);
    this.upperClip = null;
    this.upperBlend = 1;
    this.upperBank = "rifle";
    this.upperLast = this.upperJoints.map(i => this.bones[i].quaternion.clone());
    this.upperFrom = this.upperLast.map(q => q.clone());
    this.bindLengths = BIND_LENGTH.map((name) => actor.byName[name]).filter(Boolean).map((bone) => [bone, bone.position.clone()]);
    this.rootBind = actor.byName.DummyRoot.position.clone();
    this.transitions = Object.fromEntries(Object.entries(TRANSITIONS).map(([k, name]) => [k, clips.get(name)]));
    this.jumpClip = clips.get("human_rifle_jump");
    this.fallClip = clips.get("human_rifle_fall");
    this.landSoft = clips.get("human_rifle_landsoft");
    this.landHard = clips.get("human_rifle_landhard");
    this.diveClip = clips.get("human_rifle_diveforward");
    // Every weapon hovers with the rifle's jetpack clip.
    this.jetClip = clips.get("human_rifle_jetpack_hover");
    // Thrown by a blast (the original game's thrown_* set): the flying pose by
    // whether he flies forward or backward, then the bounce a live soldier
    // gets up from, or the landing and the rest pose a dead body settles
    // into.
    this.thrownClips = Object.fromEntries(["front", "back"].map((way) => [way, {
      flying: clips.get(`human_rifle_thrown_flying${way}`), bounce: clips.get(`human_rifle_thrown_bounce${way}soft`),
      land: clips.get(`human_rifle_thrown_land${way}soft`), rest: clips.get(`human_rifle_thrown_rest${way}soft`),
    }]));
    this.thrownWay = "back";
    this.clips = clips;
    this.spine = ["bone_a_spine", "bone_b_spine", "bone_ribcage"].map((n) => actor.byName[n]);
    // Weapon actions play over the upper layer's joints.
    this.upper = this.upperJoints;
    this.weaponPose = new Pose(actor.bones.length, actor.byName.DummyRoot.userData.index);
    this.weaponLayer = null;
    this.blendQ = new THREE.Quaternion();
    this.upperClips = new Map();
    this.picked = { gait: "idle", legs: 0, rate: 1 };
    this.blendP = new THREE.Vector3();
    this.head = ["bone_neck", "bone_head"].map((n) => actor.byName[n]);
    this.layers = [];
    this.twistQ = new THREE.Quaternion();
    this.parentQ = new THREE.Quaternion();
    this.deltaQ = new THREE.Quaternion();
    this.right = new THREE.Vector3();
    this.reset(0);
  }

  reset(yaw) {
    this.time = 0;
    this.facing = yaw;
    this.legs = 0;
    this.action = "loco";
    this.actionTime = 0;
    this.gait = "idle";
    this.turn = null;
    this.turnTime = 0;
    this.turnSpan = 0;
    this.posture = "stand";
    this.airborne = false;
    this.died = false;
    this.alert = -1;
    this.aimYaw = 0;
    this.aimPitch = 0;
    this.phase = 0;
    this.layers = [];
    this.weaponLayer = null;
    this.upperBank = this.bank;
    this.upperBlend = 1;
  }

  // Manning a turret: the 9-pose clip, sampled by the gun's pitch (-1 up to 1
  // down) and how fast it turns (-1..1): frame 4 + turn level, frame 1 or 7
  // + turn at full pitch, blended by the pitch. Null stops it. seat: the
  // seat node's turn in the world (a Quaternion, src/props.js
  // Turret.seatTurn), which the body takes whole, lean included, in place
  // of its facing; null keeps him upright at his facing.
  gunner(clip, pitch = 0, turn = 0, time = null, seat = null) {
    if (!clip) { this.gunning = null; return; }
    const seatTurn = seat ? (this.gunning?.seat || new THREE.Quaternion()).copy(seat) : null;
    this.gunning = { clip, pitch: THREE.MathUtils.clamp(pitch, -1, 1), turn: THREE.MathUtils.clamp(turn, -1, 1), time, seat: seatTurn };
  }

  // Killed: a death clip, held on its last frame until the soldier respawns.
  die(direction = "backward") {
    const clip = this.clips.get(`human_rifle_stand_death_${direction}`) || this.clips.get("human_rifle_die");
    if (clip) this.start("dead", clip);
    this.died = true;
    this.weaponLayer = null;
  }

  // The held weapon's animation bank (an unknown one is the rifle's).
  // Changing it drops the weapon action under way.
  setBank(bank) {
    const next = this.banks && this.banks[bank] ? bank : "rifle";
    if (next === this.bank) return;
    this.bank = next;
    this.weaponLayer = null;
  }

  // A weapon action over the upper body: "shoot", "shoot2", "shoot3",
  // "charge", or "reload", from the weapon's animation bank.
  weaponAction(action, bank = "rifle", posture = this.posture) {
    const pick = (b, p) => this.clips.get(`human_${b}_${p}_${action}`);
    const named = this.banks?.[bank]?.weapon?.[posture]?.[action] ?? this.banks?.rifle?.weapon?.[posture]?.[action];
    const clip = (named && this.clips.get(named)) || pick(bank, posture) || (bank === "grenade" && pick(bank, "stand")) || pick("rifle", posture);
    if (!clip) return;
    const blend = action.startsWith("shoot") ? ANIM.shootBlend : ANIM.actionBlend;
    const keep = this.weaponLayer?.weight ?? 0;
    this.weaponLayer = { clip, action, time: 0, weight: keep, blend, stopping: false };
  }

  // The weapon left the state its action plays for: blend the action out.
  endWeaponAction(action) {
    const layer = this.weaponLayer;
    if (layer && (!action || layer.action === action)) layer.stopping = true;
  }

  // The soldier fired: hold the alert stance.
  fire() {
    this.alert = this.alert <= 0 ? ANIM.alertOn : ANIM.alertHold;
  }

  // A gun held at a charge: the alert stance kept up every frame (the
  // original game refreshes its alert timer while a weapon charges), coming in
  // as after a shot, without restarting its blend.
  holdAlert() {
    this.alert = this.alert <= 0 ? ANIM.alertOn : Math.max(this.alert, ANIM.alertHold);
  }

  update(dt, soldier) {
    this.time += dt;
    this.actionTime += dt;
    this.alert -= dt;
    const airborne = Boolean(soldier.air);

    // The legs stay put in the world while the facing turns.
    const turned = wrap(soldier.aimYaw - this.facing);
    this.facing = soldier.aimYaw;
    this.legs = wrap(this.legs - turned);

    this.updateAction(dt, soldier, airborne);

    // Velocity on the facing's axes: left and forward.
    const sin = Math.sin(this.facing);
    const cos = Math.cos(this.facing);
    const vx = soldier.velocity.x;
    const vz = soldier.velocity.z;
    const left = vx * cos - vz * sin;
    const forward = vx * sin + vz * cos;

    if (this.action === "dive") {
      // A side dive plays the forward dive with the legs turned toward it.
      const entry = THREE.MathUtils.smoothstep(this.actionTime, 0, 0.1);
      this.legs = this.diveLegs + wrap(-soldier.dive * (Math.PI / 2) - this.diveLegs) * entry;
      // Physics uses a shortened dive window; fit the recovery into it too.
      const phase = Math.min(this.actionTime / Math.max(soldier.diveTime, 1e-3), 1);
      this.play(this.actionClip, dt, phase * this.actionClip.duration);
    } else if (this.action === "jet" || this.action === "thrown") {
      // Hovering or flying, the legs come back under the facing; the hover
      // clip cycles, the flying pose holds.
      this.legs += wrap(-this.legs) * ease(ANIM.legEase, dt);
      this.play(this.actionClip, dt, this.action === "thrown" ? this.actionTime : null);
    } else if (this.action === "loco") {
      const pick = this.pick(left, forward, soldier.posture);
      if (pick.gait === "idle") {
        this.turnInPlace(dt, soldier.posture);
      } else {
        this.turn = null;
        const step = wrap(pick.legs - this.legs) * ease(ANIM.legEase, dt);
        this.legs += THREE.MathUtils.clamp(step, -ANIM.legTurnSpeed * dt, ANIM.legTurnSpeed * dt);
      }
      this.gait = pick.gait;
      if (this.turn) this.play(this.clipFor(soldier.posture, this.turn), dt, this.turnTime);
      else this.play(this.clipFor(soldier.posture, pick.gait), dt, null, pick.rate ?? 1);
    } else {
      this.play(this.actionClip, dt, this.actionTime);
    }

    const pose = this.pose;
    pose.clear();
    if (this.gunning) {
      const { clip, pitch, turn, time } = this.gunning;
      const fps = clip.fps;
      if (time !== null && time !== undefined) pose.add(clip, time, 1);
      else {
        pose.add(clip, (4 + turn) / fps, 1 - Math.abs(pitch), false);
        pose.add(clip, ((pitch > 0 ? 7 : 1) + turn) / fps, Math.abs(pitch), false);
      }
      this.legs = 0;
    } else {
      for (const layer of this.layers) pose.add(layer.clip, layer.time, layer.weight, layer.loop);
    }
    pose.apply(this.bones);
    // A pilot pose's own root offset (the 9-pose's 0.87 m back, the riding
    // clip's 2 m up) isn't the body's: its root sits on the seat.
    if (this.gunning) this.byName.DummyRoot.position.copy(this.rootBind);
    if (this.gunning?.seat) this.actor.root.quaternion.copy(this.gunning.seat);
    // Off a seat that leaned him, he stands upright again.
    else if (this.seated) this.actor.root.rotation.set(0, this.facing + this.legs, 0);
    else this.actor.root.rotation.y = this.facing + this.legs;
    this.seated = Boolean(this.gunning?.seat);
    if (!this.gunning) {
      this.overlayUpper(dt, soldier);
      if (this.action !== "dive") this.overlayWeapon(dt);
    }
    // The body keeps its own bone lengths.
    for (let i = 0; i < this.bindLengths.length; i++) this.bindLengths[i][0].position.copy(this.bindLengths[i][1]);
    if (this.gunning || this.action === "dive") {
      this.upperJoints.forEach((i, joint) => this.upperLast[joint].copy(this.bones[i].quaternion));
      return;
    }
    this.aimUpperBody(dt, soldier);
  }

  // The held weapon's bank on the upper-body joints: the same action as the
  // legs, at the legs' phase.
  overlayUpper(dt, soldier) {
    if (this.upperBank !== this.bank) {
      this.upperBank = this.bank;
      this.upperBlend = 0;
      this.upperFrom.forEach((q, i) => q.copy(this.upperLast[i]));
    }
    this.upperBlend = Math.min(this.upperBlend + dt / ANIM.blend, 1);
    const pose = this.upperPose;
    pose.clear();
    // Retarget every crossfading leg layer to its weapon bank. Resetting an
    // independent upper fade on each gait change briefly exposed the rifle
    // pose underneath (especially visible on the battle droid's shoulders).
    for (let l = 0; l < this.layers.length; l++) {
      const layer = this.layers[l];
      const clip = this.upperFor(layer.clip);
      const phase = layer.clip.duration > 0 ? layer.time / layer.clip.duration : 0;
      // A turn's bank overlay is idle, not an entire multi-second idle clip
      // squeezed into the short foot-turn animation.
      const idle = isIdle(clip);
      pose.add(clip, idle ? this.time : phase * clip.duration, layer.weight, idle || layer.loop);
      this.upperClip = clip;
    }
    for (let joint = 0; joint < this.upperJoints.length; joint++) {
      const i = this.upperJoints[joint];
      if (pose.w[i] <= 1e-6) continue;
      this.blendQ.set(pose.q[i * 4], pose.q[i * 4 + 1], pose.q[i * 4 + 2], pose.q[i * 4 + 3]).normalize();
      this.bones[i].quaternion.copy(this.upperFrom[joint]).slerp(this.blendQ, this.upperBlend);
      this.upperLast[joint].copy(this.bones[i].quaternion);
    }
  }

  // Memoized: the lookup builds strings and runs a regex.
  upperFor(base) {
    let byBank = this.upperClips.get(this.bank);
    if (!byBank) this.upperClips.set(this.bank, (byBank = new Map()));
    let clip = byBank.get(base);
    if (!clip) byBank.set(base, (clip = this.findUpper(base)));
    return clip;
  }

  findUpper(base) {
    const set = this.banks?.[this.bank];
    if (!set || this.bank === "rifle" || !base.name.startsWith("human_rifle_")) return base;
    const suffix = base.name.slice("human_rifle_".length);
    const match = /^(stand|crouch|prone)_(.+?)(_alert)?$/.exec(suffix);
    let name = set.postureless?.[suffix];
    if (match && !name) {
      const [, posture, action, alert] = match;
      const gait = action.startsWith("turn") ? "idle_emote" : action;
      name = (alert && set.alert?.[gait]) || set.loco?.[posture]?.[gait];
    }
    return (name && this.clips.get(name)) || base;
  }

  // The weapon action, blended over the upper body's bones: in over its
  // blend time, and back out over the same once the clip has run.
  overlayWeapon(dt) {
    const layer = this.weaponLayer;
    if (!layer) return;
    layer.time += dt;
    const ending = layer.stopping || layer.time >= layer.clip.duration;
    layer.weight += (ending ? -dt : dt) / Math.max(ending ? ANIM.actionBlend : layer.blend, 1e-3);
    layer.weight = THREE.MathUtils.clamp(layer.weight, 0, 1);
    if (ending && layer.weight <= 0) {
      this.weaponLayer = null;
      return;
    }
    const pose = this.weaponPose;
    pose.clear();
    pose.add(layer.clip, Math.min(layer.time, layer.clip.duration), 1, false);
    // Standing idle, the action takes the whole body.
    const whole = this.action === "loco" && this.gait === "idle" && !this.turn && this.posture === "stand";
    for (const i of whole ? this.wholeBody() : this.upper) {
      const w = pose.w[i];
      if (w <= 1e-6) continue;
      const bone = this.bones[i];
      this.blendQ.set(pose.q[i * 4], pose.q[i * 4 + 1], pose.q[i * 4 + 2], pose.q[i * 4 + 3]).normalize();
      bone.quaternion.slerp(this.blendQ, layer.weight);
      this.blendP.set(pose.t[i * 3] / w, pose.t[i * 3 + 1] / w, pose.t[i * 3 + 2] / w);
      bone.position.lerp(this.blendP, layer.weight);
    }
  }

  // Every joint but the root.
  wholeBody() {
    if (!this.allJoints) {
      const root = this.byName.DummyRoot.userData.index;
      this.allJoints = this.bones.map((_, i) => i).filter((i) => i !== root);
    }
    return this.allJoints;
  }

  // Locomotion choice from the velocity on the body's axes.
  // The locomotion gait, leg heading and playback rate, written into one
  // reused record (read immediately by update()).
  pick(left, forward, posture) {
    const out = this.picked;
    const speed = Math.hypot(left, forward);
    const idleAt = this.gait === "idle" ? ANIM.idleSpeed : ANIM.idleStopSpeed;
    if (speed < idleAt) { out.gait = "idle"; out.legs = 0; out.rate = undefined; return out; }
    const set = this.sets[posture];
    const angle = Math.atan2(left, forward);
    const alert = this.alert > 0;
    if (set.walkLeft && set.walkRight) {
      // Prone has side crawls.
      if (Math.abs(left) > Math.abs(forward)) {
        return left > 0
          ? gaitTo(out, "walkLeft", wrap(angle - Math.PI / 2), scaledRate(set.walkLeft, left))
          : gaitTo(out, "walkRight", wrap(angle + Math.PI / 2), scaledRate(set.walkRight, -left));
      }
      if (forward > 0 || Math.abs(forward) < Math.abs(left) * 0.1) return gaitTo(out, "walk", angle, scaledRate(set.walk, speed));
      return gaitTo(out, "walkBack", wrap(angle + Math.PI), scaledRate(set.walkBack, speed));
    }
    // Brief wall/step contacts may report idle during roll recovery. Resume
    // the gait closest to the planted leg heading instead of reversing the
    // body just because the previous frame's speed crossed the idle cutoff.
    const forwardTurn = Math.abs(wrap(angle - this.legs));
    const backwardTurn = Math.abs(wrap(angle + Math.PI - this.legs));
    const going = this.gait === "walk" || this.gait === "run"
      || (this.gait === "idle" && forwardTurn <= backwardTurn + 1e-5);
    const low = going ? -ANIM.forwardHold : alert ? ANIM.alertForward[0] : -ANIM.legLimit;
    const high = going ? ANIM.forwardHold : alert ? ANIM.alertForward[1] : ANIM.legLimit;
    const legLow = alert ? ANIM.alertLegLimit[0] : -ANIM.legLimit;
    const legHigh = alert ? ANIM.alertLegLimit[1] : ANIM.legLimit;
    if (angle > low && angle < high) {
      const legs = THREE.MathUtils.clamp(angle, legLow, legHigh);
      const runAt = this.gait === "run" ? ANIM.walkBelow : ANIM.runAbove;
      if (set.run && speed > runAt) return gaitTo(out, "run", legs, scaledRate(set.run, speed));
      return gaitTo(out, "walk", legs, scaledRate(set.walk, speed));
    }
    const legs = THREE.MathUtils.clamp(wrap(angle + Math.PI), legLow, legHigh);
    if (set.runBack) return gaitTo(out, "runBack", legs, scaledRate(set.runBack, speed));
    return gaitTo(out, "walkBack", legs, scaledRate(set.walkBack, speed));
  }

  turnInPlace(dt, posture) {
    if (!this.turn) {
      const [low, high] = ANIM.turnLimits[posture];
      if (this.legs > high) {
        this.turn = "turnRight";
        this.turnSpan = -this.legs - ANIM.turnOvershoot;
      } else if (this.legs < low) {
        this.turn = "turnLeft";
        this.turnSpan = ANIM.turnOvershoot - this.legs;
      } else {
        return;
      }
      this.turnTime = 0;
    }
    const clip = this.clipFor(posture, this.turn);
    const step = Math.min(dt, clip.duration - this.turnTime);
    this.legs += (this.turnSpan / clip.duration) * Math.max(step, 0);
    this.turnTime += dt;
    if (this.turnTime >= clip.duration) this.turn = null;
  }

  clipFor(posture, gait) {
    const set = this.alert > 0 && posture === "stand" ? this.alertSet : this.sets[posture];
    return set[gait] || set.idle;
  }

  // Jump, fall, landings, dives, and prone transitions take over from locomotion.
  updateAction(dt, soldier, airborne) {
    // Thrown by a blast: the flying pose until he lands, over a death clip
    // too (a soldier the blast killed flies whole). Landed, a dead body
    // settles into the thrown rest pose; a live one bounces up and goes on.
    if (soldier.air === "thrown") {
      if (this.action !== "thrown") {
        const sin = Math.sin(soldier.aimYaw), cos = Math.cos(soldier.aimYaw);
        this.thrownWay = soldier.velocity.x * sin + soldier.velocity.z * cos < 0 ? "back" : "front";
        this.start("thrown", this.thrownClips[this.thrownWay].flying || this.fallClip);
        this.turn = null;
        this.weaponLayer = null;
        this.jetted = false;
      }
      this.airborne = true;
      this.posture = soldier.posture;
      return;
    }
    if (this.action === "thrown") {
      const clips = this.thrownClips[this.thrownWay];
      if (this.died) this.start("thrownRest", clips.land || clips.rest || this.landSoft);
      else this.start("thrownLand", clips.bounce || this.landSoft);
      this.airborne = false;
    }
    if (this.action === "thrownRest") {
      const rest = this.thrownClips[this.thrownWay].rest;
      if (rest && this.actionClip !== rest && this.actionTime >= ANIM.thrownLanding) this.setAction("thrownRest", rest);
      return;
    }
    if (this.action === "dead") return;
    const posture = soldier.posture;
    if (soldier.dive && this.action !== "dive" && this.diveClip) {
      this.diveLegs = this.legs;
      this.start("dive", this.diveClip);
      this.turn = null;
      this.weaponLayer = null;
    } else if (!soldier.dive && this.action === "dive") {
      this.action = "loco";
      // A sideways roll already points the legs into the travel direction.
      // Preserve the forward gait's directional hysteresis on recovery.
      this.gait = "walk";
      this.turn = null;
    }
    if (this.action === "dive") {
      this.posture = posture;
      this.airborne = airborne;
      return;
    }
    if (airborne && !this.airborne) {
      this.start(soldier.air === "jump" ? "jump" : "fall", soldier.air === "jump" ? this.jumpClip : this.fallClip);
      this.airTime = 0;
      this.jetted = false;
    }
    if (airborne) {
      this.airTime += dt;
      if (soldier.air === "jet" && this.jetClip) {
        if (this.action !== "jet") this.start("jet", this.jetClip);
        this.jetted = true;
      } else if (this.action === "jet") {
        this.start("fall", this.fallClip);
      }
      if (this.action === "jump" && this.actionTime > ANIM.jumpPose) this.setAction("fall", this.fallClip);
      if (soldier.air !== "jump" && this.action === "jump") this.setAction("fall", this.fallClip);
    } else if (this.airborne) {
      // A landing from a jetpack flight is always the soft one.
      if (this.airTime > ANIM.hardLanding && !this.jetted) this.start("landhard", this.landHard);
      else this.start("landsoft", this.landSoft);
    }
    this.airborne = airborne;

    if (!airborne && posture !== this.posture) {
      const clip = this.transitions[`${this.posture}>${posture}`];
      if (clip) this.start("transition", clip);
      this.posture = posture;
    }

    if (this.action === "landsoft" || this.action === "landhard" || this.action === "transition" || this.action === "thrownLand") {
      const moving = Math.hypot(soldier.velocity.x, soldier.velocity.z) > 1;
      const bounce = this.action === "thrownLand";
      const done = this.actionTime >= (bounce ? ANIM.thrownBounce : this.actionClip.duration);
      if (done || ((this.action === "landsoft" || bounce) && this.actionTime > ANIM.softCut && moving)) this.action = "loco";
    }
  }

  start(action, clip) {
    this.action = action;
    this.actionClip = clip;
    this.actionTime = 0;
  }

  setAction(action, clip) {
    this.action = action;
    this.actionClip = clip;
  }

  // Crossfade onto the clip; other layers fade out over the blend. A clip
  // played once gets its time from the caller (at); cycles run on their own
  // clock at the given rate.
  play(clip, dt, at = null, rate = 1) {
    if (!clip) return;
    const once = at !== null;
    const k = Math.min(dt / ANIM.blend, 1);
    const layers = this.layers;
    const from = layers[layers.length - 1];
    let current = null;
    for (let i = 0; i < layers.length; i++) if (layers[i].clip === clip && layers[i].loop === !once) { current = layers[i]; break; }
    if (!current) {
      current = { clip, time: 0, weight: layers.length ? 0 : 1, loop: !once, rate };
      layers.push(current);
    }
    // Synchronize even when reusing a fading cycle during a quick reversal.
    if (current !== from && !once && from?.loop && from.clip.rootMotion && clip.rootMotion && from.clip.duration > 0) {
      current.time = (from.time / from.clip.duration % 1) * clip.duration;
    }
    current.rate = rate;
    current.time = once ? at : isIdle(clip) ? this.time : current.time + dt * rate;
    const nextWeight = Math.min(current.weight + k, 1);
    const remaining = current.weight < 1 ? (1 - nextWeight) / (1 - current.weight) : 0;
    for (let i = 0; i < layers.length; i++) {
      const layer = layers[i];
      layer.weight = layer === current ? nextWeight : layer.weight * remaining;
      if (layer !== current && layer.loop) layer.time += dt * (layer.rate ?? 1);
    }
    // Drop faded layers in place, keeping their order, and normalize. The
    // bank overlay reads the active layer at the end, including when a
    // still-fading walk cycle is reused immediately after a side roll.
    let kept = 0, weight = 0;
    for (let i = 0; i < layers.length; i++) {
      const layer = layers[i];
      if (layer === current || layer.weight <= 0.001) continue;
      layers[kept++] = layer;
      weight += layer.weight;
    }
    layers[kept++] = current;
    layers.length = kept;
    weight += current.weight;
    for (let i = 0; i < kept; i++) layers[i].weight /= weight;
  }

  // Relaxed: the neck and head turn part of the way toward the aim. Alert:
  // the spine does, building up to the whole twist at the ribcage.
  aimUpperBody(dt, soldier) {
    const k = ease(ANIM.aimEase, dt);
    this.aimYaw += (THREE.MathUtils.clamp(0, -ANIM.aimYawLimit, ANIM.aimYawLimit) - this.aimYaw) * k;
    let pitch = THREE.MathUtils.clamp(soldier.aimPitch, -ANIM.aimPitchLimit, ANIM.aimPitchLimit);
    if (soldier.posture === "prone" && pitch < 0) pitch = 0;
    this.aimPitch += (pitch - this.aimPitch) * k;
    const yaw = (this.aimYaw - this.legs) / 8;
    const tilt = this.aimPitch / 8;

    // Blend between the relaxed head look (0) and the alert spine aim (1).
    // Hovering on a jetpack the aim stays relaxed, firing or not.
    let mix = 0;
    const t = this.alert;
    if (t > -0.5) mix = t > 2 ? 1 - 2 * (t - 2) : t > 0 ? 1 : 1 + 2 * t;
    const prone = soldier.posture === "prone";
    if (prone) mix = 1;
    if (soldier.air === "jet") mix = 0;

    this.right.set(-Math.cos(this.facing), 0, Math.sin(this.facing));
    // rotateWorld() reads each bone's parent through getWorldQuaternion,
    // which refreshes that chain itself. Forcing the whole body's matrices
    // here (and every rotated bone's subtree below) redid ~300 bones per
    // soldier per frame that the frame's one scene update recomputes anyway.
    if (!prone && mix <= 0.5) {
      const w = 1 - 2 * mix;
      this.toward(this.head, [2, 5], yaw * w, tilt * w);
    } else {
      const w = prone ? 1 : 2 * mix - 1;
      this.toward(this.spine, [1, 4, 8], yaw * w, tilt * w);
    }
  }

  // Each bone ends up turned by its share of eighths in total; children follow.
  toward(bones, eighths, yaw, tilt) {
    let done = 0;
    for (let i = 0; i < bones.length; i++) {
      const step = eighths[i] - done;
      done = eighths[i];
      this.rotateWorld(bones[i], UP, yaw * step);
      this.rotateWorld(bones[i], this.right, tilt * step);
    }
  }

  rotateWorld(bone, axis, angle) {
    if (!bone || Math.abs(angle) < 1e-5) return;
    bone.parent.getWorldQuaternion(this.parentQ);
    this.twistQ.setFromAxisAngle(axis, angle);
    // local' = parent^-1 * R * parent * local
    this.deltaQ.copy(this.parentQ).invert().multiply(this.twistQ).multiply(this.parentQ);
    bone.quaternion.premultiply(this.deltaQ);
  }
}
