import * as THREE from "three";
import { JetpackNozzles } from "./jetpack-nozzles.js";
import { buildActor } from "./actor.js";
import { loadClips } from "./anim.js";
import { SoldierAnimator } from "./animator.js";
import { PlayerCombat } from "./combat.js";
import { Droideka, ShieldBubble, goneUp } from "./droideka.js";
import { heatMaterial } from "./destruction-debris.js";
import { givePowerup } from "./items.js";
import { ENGINE, Soldier } from "./soldier.js";
import { takeImpacts } from "./collision-damage.js";
import { SoldierPresentation, HeadlessSoldierPresentation } from "./soldier-presentation.js";
import { RIDER_SHAPES } from "./rider-pose.js";

// The battle's units: every soldier on the field, the player's included.
//
// A unit is one body of one class: its physics (Soldier, or Droideka), its
// weapons (the same combat code the player's soldier runs), its skinned
// body and animation, and the smoothing that draws it between physics
// steps. Bots and the player share all of it; only what feeds them differs.
// A bot's controller writes an ActorIntent (docs/ai.md) each tick, and the
// unit plays it through the same movement and weapon code a player's keys
// go through. Bots never move by setting their position.
//
// Units are pooled per class: a Character that dies and respawns as the
// same class takes a free body of that class back, its generation bumped so
// that old references to it go stale.

// A body lies 4 s where it fell, then is gone (the soldier's dead state).
const DEATH_LINGER = 4;
// Animation detail by distance (presentation only; the simulation never
// reads a bot's pose, except a droideka's gun bones, so droidekas always
// animate). Bodies out of view pose every 8th frame, near ones every frame.
// Posing every 2nd or 4th frame at mid range made visible soldiers' limbs
// step while their bodies glided: everything in view out to POSE_FAR poses
// every frame; farther (a few pixels tall) every 2nd; out of view, every 6th.
const POSE_FAR = 120; // m
const heldScale = new THREE.Vector3();
const SHADOW_NEAR = 70; // m: soldiers (and their guns) cast shadows within this
const JET_EFFECT_BONE = "bone_ribcage";
// A body that has gone up (a droid blown apart, a droideka at the end of its
// death clip) is drawn no more in that life, whatever a later record,
// correction or summary says. A droid's pieces take its place at once; a
// droideka's body dissolves over GONE_FADE under its final blast.
const GONE_FADE = 0.5; // s

// What rounds strike on a soldier, per posture, in its frame (x its left, y
// up, z forward), as the 2004 game builds them: an upright cylinder for the
// body standing or crouched (a box lying prone, or dead) and a sphere for
// the head, whose hits do three times the damage. s is the class's
// CollisionRootScale and S = max(s, 1).
//
// A droideka is struck through its class's OrdnanceCollision primitives
// (cis_walk_droideka): p_rollsphere, a sphere of 0.65 m resting on the
// ground, and p_ord, an upright cylinder of radius 0.8 and height 2 standing
// 0.506 m ahead of the root, which takes in its head and guns. Rolled it is
// the ball alone. The 2004 game's hit location table (HitLocation) is empty
// for every class but the soldier's (which adds his head at 3), so nothing
// on a droideka takes more than one round's damage.
const DROIDEKA_BALL = 0.65;
const DROIDEKA_ORD = { at: [0, 1.0, 0.506], radius: 0.8, half: 1.0 };
function soldierShapes(posture, s) {
  const S = Math.max(s, 1);
  const lift = 0.17 * (S - 1);
  if (posture === "prone") {
    return [
      { type: "box", part: "body", at: [0, 0.2, -0.25 * s], half: [0.25, 0.2, 0.87 * s] },
      { type: "sphere", part: "head", at: [-0.07, 0.28 * s, 0.75 * s], radius: 0.17 * S },
    ];
  }
  if (posture === "crouch") {
    return [
      { type: "cylinder", part: "body", at: [0, 0.6 * s + lift, 0], radius: 0.35, half: 0.45 * s },
      { type: "sphere", part: "head", at: [-0.12, 1.2 * s, 0.25], radius: 0.17 * S },
    ];
  }
  return [
    { type: "cylinder", part: "body", at: [0, 0.8 * s + lift, 0], radius: 0.35, half: 0.775 * s },
    { type: "sphere", part: "head", at: [-0.09, 1.65 * s, 0.16], radius: 0.17 * S },
  ];
}
// The broadphase sphere (bounds { center, radius }) around a body's first
// `count` hit shapes: about the middle of their centres, reaching past the
// farthest shape.
function boundShapes(shapes, count, bounds) {
  const c = bounds.center.set(0, 0, 0);
  for (let i = 0; i < count; i++) c.add(shapes[i].center);
  c.divideScalar(count);
  let r = 0;
  for (let i = 0; i < count; i++) {
    const shape = shapes[i];
    const reach = shape.type === "box" ? shape.halfSize.length() : shape.type === "cylinder" ? Math.hypot(shape.radius, shape.half)
      : shape.type === "capsule" ? shape.halfSize.length() + shape.radius : shape.radius;
    r = Math.max(r, c.distanceTo(shape.center) + reach);
  }
  bounds.radius = r;
}

// Mines feel a soldier's bounding sphere: its centre above the feet and its
// size (as main.js has them for the player).
const BODY_MIDDLE = 1.048;
const BODY_RADIUS = 1.278;

export function blankIntent() {
  return {
    forward: 0, right: 0, turn: 0, pitch: 0,
    jump: false, crouch: false, prone: false, use: false,
    buttons: [0, 0],
    reload: false, nextPrimary: false, previousPrimary: false, nextSecondary: false, zoom: false,
    aim: null,
    jetDirection: null, // a jump jet's launch direction, as the path asks (Vector3)
    exit: false, // leave the vehicle or turret (edge)
  };
}

// Turns an intent's weapon part into the take/held calls PlayerCombat makes
// of the keyboard. A channel's button counts above zero are held; the first
// held count after a release is a press. (The Classic fire-request consumer
// supplies the counts; see docs/ai-combat.md.)
export class IntentControls {
  constructor(unit) {
    this.unit = unit;
    this.was = [0, 0];
    this.pressed = [false, false];
  }

  // Called once per tick before the combatant reads it.
  latch() {
    const b = this.unit.intent.buttons;
    for (let c = 0; c < 2; c++) {
      this.pressed[c] = b[c] > 0 && !(this.was[c] > 0);
      this.was[c] = b[c];
    }
  }

  take(name) {
    const intent = this.unit.intent;
    switch (name) {
      case "firePrimary": {
        const p = this.pressed[0];
        this.pressed[0] = false;
        return p;
      }
      case "fireSecondary": {
        const p = this.pressed[1];
        this.pressed[1] = false;
        return p;
      }
      case "use":
      case "reload":
      case "nextPrimary":
      case "previousPrimary":
      case "nextSecondary":
      case "zoom": {
        const v = intent[name];
        intent[name] = false;
        return v;
      }
      default:
        return false;
    }
  }

  held(name) {
    const b = this.unit.intent.buttons;
    if (name === "firePrimary") return b[0] > 0;
    if (name === "fireSecondary") return b[1] > 0;
    return false;
  }
}

export class Unit {
  constructor({ side, className, def, actor, soldier, animator, combat, presentation, walker = false, human = false }) {
    this.id = 0;
    this.generation = 0;
    this.side = side;
    this.className = className;
    this.classDef = def;
    this.actor = actor;
    this.soldier = soldier;
    this.animator = animator;
    this.combat = combat;
    this.presentation = presentation;
    this.walker = walker;
    this.human = human;
    // EntityDroideka classifies as 3, every other soldier body as 0.
    this.kind = walker ? 3 : 0;
    this.hero = Boolean(def.hero || def.IsHero);
    this.team = 0;
    this.character = null;
    this.controller = null;
    this.seat = null;
    this.alive = false;
    this.dead = false;
    this.deadTime = 0;
    this.removed = true;
    this.intent = blankIntent();
    this.controls = new IntentControls(this);
    this.input = { forward: 0, right: 0, turn: 0, pitch: 0, jump: false, crouch: false, prone: false, use: false, jetDirection: null };
    this.bounds = { center: new THREE.Vector3(), radius: 1 };
    this.rootScale = def.CollisionRootScale ?? 1;
    this.shapes = [0, 1].map(() => ({ type: "sphere", part: "body", center: new THREE.Vector3(), radius: 0, half: 0, yaw: 0, halfSize: new THREE.Vector3() }));
    // The set hitShapes() answers with; a rider's (riderHits, RIDER_SHAPES) are made
    // for his first exposed seat (no plain field: a worker's replica of the
    // unit carries neither).
    this.hitSet = this.shapes;
    this.shapeCount = 0;
    this.spheresTick = -1;
    this.tickCount = 0;
    this.jetFlame = null;
    this.bubble = null;
    this.poseClock = 0; // s of animation not yet applied
    // The page's: the life whose body has gone up, and how long ago
    // ({ generation, t }); the body's own materials while it dissolves.
    this.gone = null;
    this.ownMaterials = null;
    this.onDeath = () => {};
    this.onRemoved = () => {};
    this.onMoved = () => {};
  }

  get position() {
    return this.soldier.position;
  }

  get velocity() {
    return this.soldier.velocity;
  }

  get health() {
    return this.combat.health;
  }

  // The broadphase sphere around the hit spheres, for this tick's pose.
  get sphere() {
    if (this.spheresTick !== this.tickCount) this.updateSpheres();
    return this.bounds;
  }

  // The point others aim at: the middle of the body.
  aimPoint(out) {
    return out.copy(this.sphere.center);
  }

  // Where the unit looks from: the aim pivot (the gun origin's base).
  origin(out) {
    if (this.seat) return this.seat.origin(out);
    if (this.combat.pivot.lengthSq() > 0) return out.copy(this.combat.pivot);
    const p = this.soldier.position;
    return out.set(p.x, p.y + (this.soldier.posture === "prone" ? 0.6 : this.soldier.posture === "crouch" ? 1.2 : 1.7), p.z);
  }

  // The aim direction, unit length.
  forward(out) {
    if (this.seat) return this.seat.forward(out);
    const s = this.soldier;
    return out.set(Math.sin(s.aimYaw) * Math.cos(s.aimPitch), Math.sin(s.aimPitch), Math.cos(s.aimYaw) * Math.cos(s.aimPitch));
  }

  // Hit shapes in the world, for this tick's pose (src/registry.js
  // segmentShape): on foot by posture, or shown on an exposed seat, his
  // body as the seat poses it.
  hitShapes() {
    if (this.spheresTick !== this.tickCount) this.updateSpheres();
    const shapes = this.hitSet;
    return this.shapeCount === shapes.length ? shapes : shapes.slice(0, this.shapeCount);
  }

  // The posture whose shapes apply: standing in the air, crouched in a
  // dive, lying down prone or dead.
  get hitPosture() {
    const s = this.soldier;
    if (this.dead) return "prone";
    if (s.dive) return "crouch";
    if (s.air) return "stand";
    return s.posture;
  }

  updateSpheres() {
    const s = this.soldier;
    const p = s.position;
    // A rider shown in the open (SeatLease exposed): rounds meet his posed
    // body on the seat (src/rider-pose.js), as a client draws it.
    this.hitSet = this.shapes;
    // Held by a creature (src/props.js), his body as it hangs from its node.
    if (this.seat?.exposed || this.held?.frame) {
      this.riderHits ||= Array.from({ length: RIDER_SHAPES }, () => ({ type: "sphere", part: "body", center: new THREE.Vector3(), radius: 0, half: 0, yaw: 0, halfSize: new THREE.Vector3() }));
      const count = this.seat?.exposed ? this.seat.riderShapes(this, this.riderHits) : this.held.prop.heldShapes(this, this.riderHits);
      if (count) {
        this.hitSet = this.riderHits;
        this.shapeCount = count;
        boundShapes(this.riderHits, count, this.bounds);
        this.spheresTick = this.tickCount;
        return;
      }
    }
    if (this.walker) {
      // The ball: p_rollsphere alone. Deployed, p_ord too, the upright
      // cylinder that takes in the head and the guns.
      const ball = this.shapes[0];
      const r = s.rolled ? s.ballRadius : DROIDEKA_BALL;
      ball.type = "sphere";
      ball.part = "body";
      ball.center.set(p.x, p.y + r, p.z);
      ball.radius = r;
      ball.yaw = s.aimYaw;
      this.shapeCount = 1;
      if (!s.rolled) {
        const ord = this.shapes[1];
        const [, y, z] = DROIDEKA_ORD.at;
        ord.type = "cylinder";
        ord.part = "body";
        ord.center.set(p.x + Math.sin(s.aimYaw) * z, p.y + y, p.z + Math.cos(s.aimYaw) * z);
        ord.radius = DROIDEKA_ORD.radius;
        ord.half = DROIDEKA_ORD.half;
        ord.yaw = s.aimYaw;
        this.shapeCount = 2;
      }
      boundShapes(this.shapes, this.shapeCount, this.bounds);
    } else {
      const sin = Math.sin(s.aimYaw);
      const cos = Math.cos(s.aimYaw);
      const parts = soldierShapes(this.hitPosture, this.rootScale);
      parts.forEach((part, i) => {
        const shape = this.shapes[i];
        const [x, y, z] = part.at;
        // The soldier's left is (cos yaw, 0, -sin yaw), forward (sin yaw, 0, cos yaw).
        shape.center.set(p.x + cos * x + sin * z, p.y + y, p.z - sin * x + cos * z);
        shape.type = part.type;
        shape.part = part.part;
        shape.radius = part.radius || 0;
        shape.half = part.type === "box" ? shape.halfSize.set(...part.half) : part.half || 0;
        shape.yaw = s.aimYaw;
      });
      this.shapeCount = parts.length;
      boundShapes(this.shapes, parts.length, this.bounds);
    }
    this.spheresTick = this.tickCount;
  }

  // Into the world at (x, y, z) facing yaw, fresh.
  spawn(x, y, z, yaw) {
    this.seat = null;
    this.letGo();
    this.targetable = true;
    this.combat.mounted = false;
    this.generation += 1;
    this.soldier.dead = false;
    this.soldier.spawn(x, y, z, yaw);
    this.combat.reset();
    this.presentation.reset();
    this.animator.reset(yaw);
    this.actor.root.visible = true;
    this.actor.root.position.copy(this.presentation.position);
    this.alive = true;
    this.dead = false;
    this.deadTime = 0;
    this.removed = false;
    this.intent = blankIntent();
    this.controls.was = [0, 0];
    this.tickCount += 1;
    this.updateSpheres();
  }

  // Another body for the same unit (the player's, between lives): its class,
  // physics, weapons and model, and everything that follows from the class.
  // The unit keeps its id, generation, team and Character.
  setBody({ side, className, def, actor, soldier, animator, combat, presentation, walker = false }) {
    this.restoreLook();
    this.gone = null;
    this.side = side;
    this.className = className;
    this.classDef = def;
    this.actor = actor;
    this.soldier = soldier;
    this.animator = animator;
    this.combat = combat;
    this.presentation = presentation;
    this.walker = walker;
    this.kind = walker ? 3 : 0;
    this.hero = Boolean(def.hero || def.IsHero);
    this.rootScale = def.CollisionRootScale ?? 1;
    this.spheresTick = -1;
    this.poseClock = 0;
    this.shadowNear = undefined;
    this.jetFlame?.stop();
    this.jetFlame = null;
  }

  // Off the field (after a death, or between lives). Its Character lets go
  // of it: the body goes back to the pool for anyone's next spawn.
  remove() {
    this.alive = false;
    this.removed = true;
    this.letGo();
    if (this.character?.unit === this) this.character.unit = null;
    this.character = null;
    this.undraw();
    this.onRemoved(this);
  }

  // Out of a creature's hold (src/props.js release): upright again, in his
  // own clips, his weapons his own.
  letGo() {
    if (!this.held) return;
    this.held = null;
    if (this.combat) this.combat.mounted = false;
    this.animator?.gunner?.(null);
    this.actor?.root?.rotation.set(0, this.actor.root.rotation.y, 0);
  }

  // Nothing of it drawn: its body, a droideka's shield, a jet flame, a
  // weapon's charge glow.
  undraw() {
    this.actor.root.visible = false;
    this.restoreLook();
    this.bubble?.hide();
    this.jetFlame?.stop();
    this.jetFlame = null;
    this.combat?.clearCharge?.();
  }

  // Its body went up (the page's): nothing more of this life is drawn, but
  // for a droideka's body dissolving away first.
  goUp() {
    if (this.gone?.generation !== this.generation) this.gone = { generation: this.generation, t: 0 };
  }

  // A gone-up body this frame: 0 if it hasn't gone up (drawn as usual), the
  // share of a droideka's dissolve so far, or -1 once gone (undrawn).
  presentGone(dt) {
    const gone = this.gone;
    if (!gone) return 0;
    if (gone.generation !== this.generation) {
      this.gone = null;
      this.restoreLook();
      return 0;
    }
    gone.t += Math.max(dt, 0);
    if (!this.walker || gone.t >= GONE_FADE) {
      this.undraw();
      return -1;
    }
    this.actor.root.visible = true;
    return Math.max(gone.t / GONE_FADE, 1e-3);
  }

  // The body dissolving by `share` (0 to 1) with heatMaterial's noise, lit
  // as its own materials are this frame: charred and glowing as it burns
  // away in Enhanced (enhanced), plain in Classic.
  dissolve(share, enhanced) {
    const body = this.actor.body;
    if (!this.ownMaterials) {
      this.ownMaterials = body.material;
      body.material = Array.isArray(body.material) ? body.material.map((material) => heatMaterial(material)) : heatMaterial(body.material);
      body.userData.heated = { heat: 0, tear: new THREE.Vector4(0, 0.9, 0, 0.6), base: 0, scale: 0.6, soot: 0, dissolve: 0, sun: 1, brightness: 1, darken: 0, lighten: 0 };
    }
    const look = body.userData.heated;
    const lit = [this.ownMaterials].flat()[0]?.uniforms;
    if (lit?.sunScale) {
      look.sun = lit.sunScale.value;
      look.brightness = lit.brightness.value;
      look.darken = lit.ambientPull.value.x;
      look.lighten = lit.ambientPull.value.y;
    }
    look.dissolve = share * 1.02;
    look.heat = enhanced ? 0.9 * (1 - share * 0.5) : 0;
    look.base = enhanced ? 0.3 : 0;
    look.soot = enhanced ? 1 : 0;
  }

  // Its own materials back on the body (after a dissolve).
  restoreLook() {
    if (!this.ownMaterials) return;
    this.actor.body.material = this.ownMaterials;
    delete this.actor.body.userData.heated;
    this.ownMaterials = null;
  }

  // One physics step: the intent through the soldier's own movement, then
  // its weapons. The player's unit is stepped by main.js instead.
  tick(h) {
    if (this.removed) return;
    this.tickCount += 1;
    // The occupied control consumes its intent and runs the vehicle physics.
    // Do not consume its edges or step the hidden soldier a second time.
    if (this.seat && this.alive) return;
    // A creature holds him (src/props.js): it moves him, and nothing he
    // does counts until it lets go; the dead body it carries waits too.
    if (this.held) return;
    const intent = this.intent;
    const s = this.soldier;
    const input = this.input;
    const remoteActive = this.combat.remotes?.active;
    if (this.alive && !remoteActive) {
      input.forward = intent.forward;
      input.right = intent.right;
      input.turn = intent.turn;
      input.pitch = intent.pitch;
      input.jump = intent.jump;
      input.crouch = intent.crouch;
      input.prone = intent.prone;
      input.use = intent.use;
      input.jetDirection = intent.jetDirection;
    } else {
      input.forward = input.right = input.turn = input.pitch = 0;
      input.jump = input.crouch = input.prone = input.use = false;
    }
    intent.jump = intent.crouch = intent.prone = false;
    if (!remoteActive) intent.use = false;
    this.presentation.beforeStep();
    s.tick(input, h);
    // What hitting the ground or a wall too hard did to him, or a fall past
    // saving (src/collision-damage.js).
    takeImpacts(this);
    this.presentation.afterStep();
    // The aimer runs between moving and firing (a bot's Classic Aimer).
    if (this.alive) this.onMoved(h);
    this.combat.aimOverride = this.alive ? intent.aim : null;
    this.controls.latch();
    this.remoteStep?.(h, remoteActive ? intent : null, this.controls);
    s.sampleMuzzles?.(h);
    this.combat.update(h, this.controls, Math.max(Math.abs(input.forward), Math.abs(input.right)));
    this.combat.flushMuzzles();
    if (this.dead) {
      this.deadTime += h;
      if (this.deadTime >= DEATH_LINGER) this.remove();
    }
  }

  // Per rendered frame: the body where the smoothing puts it, posed. `view`
  // ({ position, frustum, frame }) picks how often far and unseen bodies
  // are posed.
  present(dt, alpha, light, effects, view = null) {
    // Off the field nothing of it is drawn, whatever its body was last told:
    // a world or a packet that left it shown would stand it, unposed and
    // untouchable, where it was last placed.
    if (this.removed) {
      this.undraw();
      return;
    }
    const state = this.presentation.update(dt, alpha);
    // A droideka gone up as drawn (a lost or late event, a new page) too.
    if (this.walker && goneUp(state)) this.goUp();
    const gone = this.presentGone(dt);
    if (gone < 0) return;

    // Held by a creature: drawn where the creature drawn this frame holds
    // him (LiveObjects.presentAnimated), in his clip.
    const held = this.held?.frame ? this.held : null;
    if (held) {
      this.presentation.position.setFromMatrixPosition(held.frame);
      this.animator.gunner(this.animator.clips.get(held.grab.clip) ?? null, 0, 0, held.time);
    }
    // Recover the aiming stance from sampled shot counters when an expendable
    // shot event is lost. No round, sound, flash, or weapon action is replayed.
    // holdAlert preserves an already-started first-shot blend on normal delivery.
    if (this.presentation.networkFired && !state.dead && !this.seat) this.animator.holdAlert?.();
    if (this.presentation.deathPending && state.dead) {
      this.presentation.deathPending = false;
      this.animator.die();
    }
    this.actor.root.position.copy(this.presentation.position);
    if (!this.presentation.networkPose) state.aimPitch = this.soldier.aimPitch;
    this.poseClock += dt;
    let every = 1;
    if (view && !this.walker) {
      const d2 = view.position.distanceToSquared(this.position);
      const s = this.sphere;
      every = !view.frustum.intersectsSphere(s) ? 6 : d2 > POSE_FAR * POSE_FAR ? 2 : 1;
    }
    // Shadows only for soldiers near the camera: far away a soldier's shadow
    // is a few blurred texels in the outer cascades, at a draw per cascade
    // for the body and each held weapon.
    const near = !gone && (!view || view.position.distanceToSquared(this.position) < SHADOW_NEAR * SHADOW_NEAR);
    if (near !== this.shadowNear) this.castShadows(near);
    if (every === 1 || (view.frame + this.id) % every === 0) {
      this.animator.update(this.poseClock, state);
      this.poseClock = 0;
    }
    if (held) held.frame.decompose(this.actor.root.position, this.actor.root.quaternion, heldScale);
    if (this.seat) this.combat?.clearCharge?.();
    else this.combat?.presentCharge?.(dt);
    this.bubble?.update(dt, this.soldier, { position: this.presentation.position });
    light?.(this.actor, this.presentation.position);
    if (gone) this.dissolve(gone, this.enhancedDeath?.());
    this.updateJetFlame(effects);
  }

  castShadows(near) {
    this.shadowNear = near;
    if (this.actor.shadow) this.actor.shadow.visible = near;
    for (const held of this.combat?.held?.values?.() ?? []) {
      held.group?.traverse((mesh) => {
        if (!mesh.isMesh) return;
        if (!near) { mesh.userData.castShadowNear ??= mesh.castShadow; mesh.castShadow = false; }
        else if (mesh.userData.castShadowNear !== undefined) { mesh.castShadow = mesh.userData.castShadowNear; delete mesh.userData.castShadowNear; }
      });
    }
  }

  updateJetFlame(effects) {
    const name = this.classDef.JetEffect;
    const bone = this.actor.byName[JET_EFFECT_BONE];
    const active = this.alive && this.soldier.air === "jet" && name && bone;
    if (active) {
      bone.updateWorldMatrix(true, false);
      const at = (this.flameAt ||= new THREE.Vector3());
      const turn = (this.flameTurn ||= new THREE.Quaternion());
      const scale = (this.flameScale ||= new THREE.Vector3());
      bone.matrixWorld.decompose(at, turn, scale);
      if (!this.jetFlame) this.jetFlame = effects?.play(name, at, null, this.soldier.velocity, turn) || null;
      else this.jetFlame.move(at, turn, this.soldier.velocity);
      this.jetFlame?.attachNozzles?.(this.actor.jetpackNozzles?.update());
    } else if (this.jetFlame) {
      this.jetFlame.stop();
      this.jetFlame = null;
    }
  }
}

// A class's OverrideTexture takes the place of the texture most of its body
// uses, and OverrideTexture2 of the next: { body texture: override }.
export function textureOverrides(def, bodyDef) {
  const textureUse = new Map();
  for (const part of bodyDef.parts) textureUse.set(part.texture, (textureUse.get(part.texture) || 0) + part.vertexCount);
  const byUse = [...textureUse.keys()].sort((a, b) => textureUse.get(b) - textureUse.get(a));
  const overrides = {};
  ["OverrideTexture", "OverrideTexture2"].forEach((key, i) => {
    if (def[key] && byUse[i]) overrides[byUse[i]] = def[key];
  });
  return overrides;
}

// One body of a class, built the same way for the player and the bots: the
// skinned model (OverrideTexture re-skins it in place of the texture most of
// it uses, OverrideTexture2 the next), its physics (a Soldier, or a Droideka
// walking by its own clips) and its animator. Nothing is added to a scene.
// data: the body's model ({ def, buf, indexBuf }); walkerData: a droideka's
// clips ({ meta, buf }); clips: the shared clips for the body's bones.
export function buildBody({ def, data, walkerData = null, texture, graphics, collision, clips, animMeta, maxFlyHeight = 200 }) {
  const walker = def.base === "walkerdroid";
  const actor = buildActor(data.def, data.buf, data.indexBuf, texture, graphics, textureOverrides(def, data.def));
  if (def.JetEffect) actor.jetpackNozzles = new JetpackNozzles(actor, data.def);
  let soldier;
  let animator;
  if (walker) {
    soldier = new Droideka(collision, def, loadClips(walkerData.meta, walkerData.buf, data.def.bones.map((bone) => bone.name)), actor);
    animator = soldier.animator;
  } else {
    soldier = new Soldier(collision, def);
    animator = new SoldierAnimator(actor, clips, animMeta.sets);
  }
  // Anyone in the air is pushed back down near the mission's ceiling.
  soldier.maxFlyHeight = maxFlyHeight;
  // Getting up from prone holds the soldier still for the get-up clip.
  soldier.getupTime = clips.get("human_rifle_prone_getup_stand")?.duration ?? 0;
  // A side dive lasts three quarters of the dive clip.
  soldier.diveTime = ENGINE.diveLength * (clips.get("human_rifle_diveforward")?.duration ?? 0);
  return { actor, soldier, animator, walker, clips };
}

// Builds units for any class of the mission's sides, and keeps their
// bodies for reuse. Loading is asynchronous (prepare() each class first);
// building and acquiring are not, so a spawn never waits on the network.
export class UnitFactory {
  // sides: name -> { dir, units, items, texture(name), firePoints }
  // context: { graphics, scene, collision, animMeta, animBuf, json, bin,
  //   ordnance, lights, weaponModels, flashModel, anyTexture, effects,
  //   settings, maxFlyHeight, items, live, units (registry), destruction
  //   (Enhanced droid deaths, or null), onBlowApart(unit, health), onDeath(unit) }
  constructor(sides, context) {
    this.sides = sides;
    this.context = context;
    this.loading = new Map(); // key -> Promise
    this.loaded = new Map(); // key -> resolved data
    this.clipSets = new Map(); // bone names -> clips
    this.pool = new Map(); // className -> free units
    this.built = [];
  }

  // The side a class belongs to.
  sideOf(className) {
    for (const [name, side] of Object.entries(this.sides)) if (side.units[className]) return name;
    return null;
  }

  load(key, fetcher) {
    if (!this.loading.has(key)) {
      this.loading.set(key, fetcher().then((value) => {
        this.loaded.set(key, value);
        return value;
      }));
    }
    return this.loading.get(key);
  }

  clipsFor(bones) {
    const key = bones.map((bone) => bone.name).join(",");
    if (!this.clipSets.has(key)) {
      const { animMeta, animBuf } = this.context;
      this.clipSets.set(key, loadClips(animMeta, animBuf, bones.map((bone) => bone.name)));
    }
    return this.clipSets.get(key);
  }

  // Load what a class needs before its first spawn.
  // Side packages also contain non-infantry definitions, such as Hoth's
  // probe droid. Only soldier bodies belong in this factory's prerequisites.
  async prepareSoldiers() {
    const classes = Object.values(this.sides).flatMap(side => Object.entries(side.units)
      .filter(([, def]) => def.base === 'soldier' || def.base === 'walkerdroid')
      .map(([name]) => name));
    await Promise.all([...new Set(classes)].map(name => this.prepare(name)));
  }

  async prepare(className) {
    const sideName = this.sideOf(className);
    if (!sideName) throw new Error(`no side has the class ${className}`);
    const { json, bin } = this.context;
    const def = this.sides[sideName].units[className];
    // Soldiers and droidekas have bodies here; Hoth's probe droid (a droid,
    // its mission's team 4, which never deploys) has none.
    if (def.base !== "soldier" && def.base !== "walkerdroid") throw new Error(`${className} is a ${def.base}, not a soldier`);
    const dir = `${this.sides[sideName].dir}/actors`;
    await this.load(`body:${sideName}/${def.model}`, () =>
      Promise.all([json(`${dir}/${def.model}.json`), bin(`${dir}/${def.model}.bin`), bin(`${dir}/${def.model}_index.bin`)])
        .then(([meta, buf, indexBuf]) => ({ def: meta, buf, indexBuf })));
    if (def.base === "walkerdroid") {
      await this.load(`clips:${sideName}/${def.clips}`, () =>
        Promise.all([json(`${dir}/${def.clips}.json`), bin(`${dir}/${def.clips}.bin`)]).then(([meta, buf]) => ({ meta, buf })));
    }
    return def;
  }

  prepared(className) {
    const sideName = this.sideOf(className);
    const def = sideName && this.sides[sideName].units[className];
    return Boolean(def && this.loaded.has(`body:${sideName}/${def.model}`));
  }

  // A prepared class's model ({ def, buf, indexBuf }), or null.
  bodyData(className) {
    const sideName = this.sideOf(className);
    const def = sideName && this.sides[sideName].units[className];
    return (def && this.loaded.get(`body:${sideName}/${def.model}`)) || null;
  }

  // A prepared droideka's own clips ({ meta, buf }), or null for a soldier.
  walkerData(className) {
    const sideName = this.sideOf(className);
    const def = sideName && this.sides[sideName].units[className];
    return def?.base === "walkerdroid" ? this.loaded.get(`clips:${sideName}/${def.clips}`) || null : null;
  }

  // A free unit of the class, built if none is free. The class must have
  // been prepared.
  acquire(className) {
    const free = this.pool.get(className);
    if (free?.length) return free.pop();
    return this.build(className);
  }

  release(unit) {
    if (!this.pool.has(unit.className)) this.pool.set(unit.className, []);
    const free = this.pool.get(unit.className);
    if (!free.includes(unit)) free.push(unit);
  }

  build(className) {
    const c = this.context;
    const sideName = this.sideOf(className);
    if (!sideName) throw new Error(`no side has the class ${className}`);
    const side = this.sides[sideName];
    const def = side.units[className];
    const body = this.loaded.get(`body:${sideName}/${def.model}`);
    if (!body) throw new Error(`${className} wasn't prepared`);
    const texture = (name) => side.texture(name) || c.anyTexture(name);
    const { actor, soldier, animator, walker } = buildBody({
      def, data: body, walkerData: this.walkerData(className), texture, graphics: c.graphics, collision: c.collision,
      clips: this.clipsFor(body.def.bones), animMeta: c.animMeta, maxFlyHeight: c.maxFlyHeight ?? 200,
    });
    actor.root.visible = false;
    // The body's bind-pose box, for the path planner's clearance: a quarter
    // of its horizontal diagonal (docs/ai-nav.md).
    actor.body.geometry.computeBoundingBox();
    const box = actor.body.geometry.boundingBox;
    c.scene.add(actor.root);
    const unit = new Unit({ side: sideName, className, def, actor, soldier, animator, walker,
      combat: null, presentation: new (c.headless ? HeadlessSoldierPresentation : SoldierPresentation)(soldier) });
    unit.navRadius = Math.fround(0.25 * Math.hypot(box.max.x - box.min.x, box.max.z - box.min.z));
    const combat = new PlayerCombat({
      soldier, def, actor, animator,
      collision: c.collision,
      camera: null,
      ordnance: c.ordnance,
      lights: c.lights,
      weaponModels: c.weaponModels,
      firePoints: side.firePoints || {},
      flashModel: c.flashModel,
      texture,
      hud: null,
      caster: (object) => c.graphics.caster(object),
      owner: unit,
      player: false,
    });
    combat.items = c.items;
    combat.live = c.live;
    combat.effects = c.effects;
    if (c.arcTargets) combat.arcTargets = c.arcTargets;
    if (c.lightning) combat.lightning = c.lightning;
    combat.deferMuzzles = walker;
    combat.onRespawn = () => {};
    combat.onDeath = () => {
      unit.alive = false;
      unit.dead = true;
      unit.deadTime = 0;
      c.onDeath?.(unit);
    };
    combat.onBlowApart = (health) => c.onBlowApart?.(unit, health);
    unit.combat = combat;
    // Its shadow, and its guns', through the crowd's one mesh (crowd-shadows.js).
    unit.crowdSlot = c.graphics.crowdShadows?.add(unit) ?? null;
    if (walker) {
      soldier.arm(def.weapons || []);
      const shieldWeapon = (def.weapons || []).find((w) => w.base === "shield");
      const look = c.effects.shields[shieldWeapon?.ShieldEffect];
      unit.bubble = !c.headless && look ? new ShieldBubble(c.scene, look, c.anyTexture, c.settings) : null;
      // The game's hook flares the bubble (or records it, in a world worker).
      soldier.onShieldHit = (amount, source, kind) => (c.shieldHit ? c.shieldHit(unit, amount, source, kind) : unit.bubble?.hit(source, amount, soldier, kind));
      const blasts = ["explosion", "med_explosion", "small_explosion"];
      soldier.onDeathBlast = () => {
        // The game's hook draws it (or records it, in a world worker).
        if (c.deathBlast) return c.deathBlast(unit);
        if (c.destruction?.deathBlast(unit)) return;
        const bone = actor.bones[Math.floor(Math.random() * actor.bones.length)];
        c.effects.play(blasts[Math.floor(Math.random() * blasts.length)], bone.getWorldPosition(new THREE.Vector3()));
      };
      soldier.onExploded = () => combat.onBlowApart(0);
      // How its body dissolves as it goes up (Unit.dissolve).
      unit.enhancedDeath = () => (c.settings?.get?.("destructionEffects") ?? "enhanced") === "enhanced";
    }
    unit.onRemoved = (u) => this.release(u);
    // What recharge droids serve, and what mines and power-ups see.
    const healthType = (def.HealthType || "person").toLowerCase();
    unit.supply = {
      position: soldier.position,
      healthType,
      get health() { return combat.health; },
      set health(value) { combat.health = value; },
      get maxHealth() { return combat.maxHealth; },
      get weapons() { return combat.arsenal.weapons; },
    };
    const center = new THREE.Vector3();
    unit.itemTarget = {
      position: soldier.position,
      get center() { return center.set(soldier.position.x, soldier.position.y + BODY_MIDDLE, soldier.position.z); },
      radius: BODY_RADIUS,
      velocity: soldier.velocity,
      relation: (team) => c.units.relation(unit.team, team),
      give: (item) => {
        const given = givePowerup(item, {
          get health() { return combat.health; },
          set health(value) { combat.health = value; },
          maxHealth: combat.maxHealth, addHealth: def.AddHealth, weapons: combat.arsenal.weapons, healthType,
        });
        if (given.health) (c.presentEffect || c.effects.play.bind(c.effects))(def.HealthGainEffect || "Powerup_blue", soldier.position);
        if (given.ammo) (c.presentEffect || c.effects.play.bind(c.effects))(def.AmmoGainEffect || "Powerup_red", soldier.position);
        return given.health || given.ammo;
      },
    };
    c.units.add(unit);
    this.built.push(unit);
    c.onBuilt?.(unit);
    return unit;
  }
}
