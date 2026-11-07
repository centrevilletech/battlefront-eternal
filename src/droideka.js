import {sceneTransform} from "./rust-render/scene-changes.js";
import {actorPoint,actorPointCount,actorJoint} from './actor-resources.js';
import * as THREE from "three";
import {registerBody,spawnDroideka} from "./presentation/body.js";
import { DROIDEKA_COLLISION, collisionRule } from "./collision-damage.js";
import { Pose } from "./anim.js";
import { barrelTips } from "./muzzles.js";

// Droideka construction and page rig presentation. The native unit owner
// advances deployed root motion, rolling, collisions, shields and weapons.
// Clip/mode helpers remain for the spawn preview and drawn death poses.

const UP = new THREE.Vector3(0, 1, 0);
const CLIPS = [
  "idle", "turnl", "turnr", "death01", null, "idle_to_leftfoot", null,
  "walk_leftfoot_rightfoot", "walk_rightfoot_leftfoot", "leftfoot_to_idle", "rightfoot_to_idle", "fold", "roll", "unfold",
];
const FOLD = 11;
const BALL = 12;
const UNFOLD = 13;
const DEATH = 3;
const DEAD = 4;
const BALL_CENTER = 0.65; // m, the ball's centre above the origin (BallRadius)

// Whether a droideka (or its record, or its presented state) has gone up:
// dead, its death clip played out (or dead rolled, at once). Its final blast
// and pieces went off then (onExploded), and its body is gone for that life.
export function goneUp(state) {
  return Boolean(state?.dead && state.mode === DEAD);
}

export class Droideka {
  constructor(world, def, clips, actor, native=world.rays.x) {
    this.world = world;
    registerBody(this,native);
    // Set later (collision, water, firing), undefined till then as before:
    // here so every droideka keeps one shape (Unit's constructor).
    this.waterClock = undefined;
    this.lastGun = undefined;
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
    this.onExploded = () => {}; // the end of the death clip
    this.onShieldHit = () => {}; // actual absorbed damage, with optional world source
    this.shield = null;
    this.guns = [];
    this.gun = 0;
    this.eyeHeight = (def.cameras?.STAND?.EyePointOffset || [0, 1.8, 0])[1];
    this.blastClock = 0;
    this.lookX = 0;
    this.lookY = 0;
    // Near-horizontal building and prop contacts since a bot's path executor
    // last read them (as Soldier.avoidance).
    this.avoidance = new THREE.Vector3();
    // Landing too hard hurts (src/collision-damage.js), as a soldier's does.
    this.collisionRule = collisionRule(def, DROIDEKA_COLLISION);
    this.collisionDamage = 0;
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
      const asset=body.userData.actorAsset;
      const toBone = new THREE.Matrix4().multiplyMatrices(body.skeleton.boneInverses[index], body.bindMatrix);
      const points = [];
      for(let i=0;i<actorPointCount(asset);i++)if(actorJoint(asset,i)===index)points.push(sceneTransform(actorPoint(asset,i,new THREE.Vector3()).applyMatrix4(toBone)));
      const tips = barrelTips(points, "x", 1, "z");
      return tips.length ? tips.map((point, i) => {
        const muzzle = new THREE.Object3D(); muzzle.name = `${gun.name}_barrel_${i}`;
        sceneTransform(muzzle.position.copy(point)); bone.add(muzzle); return muzzle;
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

  sampleMuzzles(dt) {
    if (!this.muzzleActor) return;
    sceneTransform(this.muzzleActor.root.position.copy(this.position));
    this.muzzleAnimator.update(dt, this);
    this.muzzleActor.root.updateMatrixWorld(true);
  }

  // The session selects a barrel; presentation reads its visible muzzle.
  // The native world owns the physical rig and advances the shared cursor.
  visualGunAt(out, selected = this.lastGun) {
    const bone = this.gunBarrels?.[selected?.index]?.[selected?.barrel] || this.guns[selected?.index];
    if (!bone) return null;
    bone.updateWorldMatrix(true, false);
    return out.setFromMatrixPosition(bone.matrixWorld);
  }

  pitchRange() {
    return this.pitchLimits;
  }

  // Cold placement shares the native unit reset, including its shield.
  spawn(x,y,z,yaw){spawnDroideka(this,x,y,z,yaw);}

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
      const count=actorPointCount(actor.body.userData.actorAsset);
      const points = [];
      for (let i = 0; i < count; i++) {
        sceneTransform(actor.body.getVertexPosition(i, vertex).applyMatrix4(toParent));
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
      actor.bones.forEach((bone, i) => { sceneTransform(bone.quaternion.copy(this.bind[i].q)); sceneTransform(bone.position.copy(this.bind[i].p)); });
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
      sceneTransform(bone.quaternion.copy(this.bind[i].q));
      sceneTransform(bone.position.copy(this.bind[i].p));
    }
    pose.apply(this.actor.bones);
    // The legs trail the body: DummyRoot turned by the lag, the spine
    // twisted back a third of it at each of its three bones.
    const folded = mode === BALL ? 1 : mode === FOLD
      ? THREE.MathUtils.smoothstep(d.time / Math.max(d.length(), 1e-3), 0.35, 1)
      : mode === UNFOLD ? 1 - THREE.MathUtils.smoothstep(d.time / Math.max(d.length(), 1e-3), 0, 0.3) : 0;
    const lag = d.legs * (1 - folded);
    if (this.root >= 0) sceneTransform(this.actor.bones[this.root].quaternion.premultiply(this.q.setFromAxisAngle(UP, lag)));
    for (const i of this.spine) sceneTransform(this.actor.bones[i].quaternion.multiply(this.q.setFromAxisAngle(this.axisX, -lag / 3)));
    // The blasters and head follow vertical aim together; feet keep the
    // walking pose. Fade the correction at the folding clip boundaries.
    const aiming = d.dead || mode === BALL ? 0 : mode === FOLD
      ? 1 - THREE.MathUtils.smoothstep(d.time / Math.max(d.length(), 1e-3), 0, 0.25)
      : mode === UNFOLD ? THREE.MathUtils.smoothstep(d.time / Math.max(d.length(), 1e-3), 0.7, 1) : 1;
    if (this.spine.length && aiming) sceneTransform(this.actor.bones[this.spine[0]].quaternion.premultiply(this.q.setFromAxisAngle(this.axisX, -d.aimPitch * aiming)));
    if (this.ball >= 0 && mode >= FOLD) {
      const ball = this.actor.bones[this.ball];
      // Bring the folded shell above the contact point gradually, so the
      // fold endpoint and the first rolling frame agree exactly.
      this.offset.subVectors(this.rollCenter, this.pivot).multiplyScalar(folded);
      if (mode === BALL) {
        this.spinRotation.setFromAxisAngle(this.axisZ, d.lean)
          .multiply(this.q.setFromAxisAngle(this.axisX, d.roll - this.rollOrigin));
        sceneTransform(sceneTransform(ball.position.sub(this.pivot)).applyQuaternion(this.spinRotation)).add(this.pivot);
        sceneTransform(ball.quaternion.premultiply(this.spinRotation));
      }
      sceneTransform(ball.position.add(this.offset));
    }
    if (this.transition > 0) {
      this.transition = Math.max(0, this.transition - dt);
      const blend = THREE.MathUtils.smoothstep(1 - this.transition / 0.16, 0, 1);
      this.actor.bones.forEach((bone, i) => {
        sceneTransform(bone.quaternion.slerpQuaternions(this.from[i].q, bone.quaternion, blend));
        sceneTransform(bone.position.lerpVectors(this.from[i].p, bone.position, blend));
      });
    }
    this.facing = d.aimYaw;
    this.legs = d.legs;
    const root = this.actor.root;
    sceneTransform(root.rotation.set(0, d.aimYaw, 0));
  }
}

export { ShieldBubble } from "./droideka-shield.js";
