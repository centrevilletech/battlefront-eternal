import { updateWeaponLock, hasMissileLock } from "./weapon-targeting.js";
import { bindMountedRound } from "./muzzles.js";
import * as THREE from "three";
import { makeHit } from "./collision.js";
import { cameraTension, easeAxes } from "./flight/camera.js";
import { emitSound } from "./audio/events.js";
import { SeatLease, GUNNER, seatWeaponPolicy, poseTurret, unseatBody, dropBody, turretAimPoint } from "./seats.js";

// The player manning a turret, following the 2004 game.
//
// E (use) gets into the turret the original game would pick (props.js) and out
// again. Manning it, the soldier stands at the drum's PilotPosition facing the
// gun, in the turret's pose for its gunner (Turret.riderSample), out of sight
// where the turret hides its gunner; the mouse turns the aim within the
// class's limits, the gun follows the point under the crosshair, and the fire
// button fires the turret's weapon from the gun's fire point. The views are
// the turret seat's own (TurretCamera below; Turret's aim frame, props.js).
//
// Getting out, the soldier is set down beside the turret: behind it, to
// either side, at the corners, or in front, whichever is first clear of it,
// on the floor below, facing the way the turret faces. Killed at the gun, his
// body falls from where he stood instead.
//
// While he mans it the turret holds his seat lease (src/seats.js) and his
// unit's seat is that lease, as for a bot at a turret (TurretSeats): a shown
// gunner meets rounds, swings and blasts as a bot there does.
//
// Aiming, as the original game's player aim target: from his eye in first
// person, otherwise from where the chase camera rests brought forward to the
// eye's plane, along the aim, to the first wall, hull or soldier within
// 512 m (seats.js turretAimPoint). The page's own battle and a server's
// TurretSeats aim a person's turret from the same point, so the gun turns to
// what lies under his crosshair on either. A round leaves the fire point
// straight at that point once the gun has turned as far toward it as it
// can, at it or at its limits (Turret.fireDirection).
//
// Showing it: the world (a page stepping its own battle, or a worker whose
// ticks the page applies, src/runtime/replica.js) owns the aim's yaw and
// pitch and the gun's own angles. The page draws from those: preview() runs
// the aim ahead over the looks the world has not taken yet, as the soldier's
// look is previewed (game.js clientFrame), and updateCamera() poses the drum
// and gun it shows and sets the camera. A page showing a worker's battle
// never calls enter(): its turret arrives as gunner.turret (SeatReader), so
// the view is made for whichever turret is set. P (perspective) looks from
// the eye (EyePointOffset) down the aim; an eye inside the turret's box (the
// Hoth, dish and Geonosian turrets) has the drum and gun kept out of the
// view, as his own body is in first person.

const LOOK_SCALE = 2048; // counts x sensitivity per unit of turn rate, as for soldiers
const LOOK_CLAMP = 16;
const EXIT_EXTRA = 0.684; // m beyond the turret's bounding radius
const EXIT_RADIUS = 2.253; // m, the turret's bounding radius
const SPREAD = Math.PI / 180;
const TURRET_CORE = 1.4; // m around the turret's centre that its own walls fill
// The gunner's pose (Turret.riderSample): scratch, kept off the Gunner, whose
// own fields a world worker sends its page.
const gunnerPose = { clip: null, pitch: 0, turn: 0, time: null };
const gunnerTurn = new THREE.Quaternion();

export class Gunner {
  constructor({ camera, collision, ordnance, combat, animator, hiddenLayer = 1 }) {
    this.camera = camera;
    this.collision = collision;
    this.ordnance = ordnance;
    this.combat = combat;
    this.animator = animator;
    this.hiddenLayer = hiddenLayer;
    this._turret = null;
    this.view = null;
    this.simulationView = null;
    this.viewTurret = null; // the turret the views were made for
    this.hiddenRig = null; // the turret whose drum and gun are hidden (first person)
    // What the page shows over the world's aim: the looks it has not taken
    // yet and the way through the tick (preview()).
    this.previewYaw = 0;
    this.previewPitch = 0;
    this.previewState = { yaw: 0, pitch: 0, yawVel: 0, pitchVel: 0 };
    this.shownYaw = 0;
    this.shownPitch = 0;
    this.eye = new THREE.Vector3();
    this.firstPerson = false; // the world's view: where its aim starts (updateCamera)
    this.viewFirstPerson = false; // the page's view: where its reticle's aim starts
    this.reticlePoint = new THREE.Vector3();
    this.lookX = 0;
    this.lookY = 0;
    this.seat = new THREE.Vector3();
    this.dir = new THREE.Vector3();
    this.target = new THREE.Vector3();
    this.from = new THREE.Vector3();
    this.muzzle = new THREE.Vector3();
    this.gunDir = new THREE.Vector3();
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
  }

  get active() {
    return Boolean(this.turret);
  }

  // The turret manned, set here by enter() and on a page showing a worker's
  // battle by the seat packet (SeatReader.setGunner). Another turret, or
  // none: what the page showed of the last one is let go.
  get turret() {
    return this._turret;
  }
  set turret(turret) {
    if (turret === this._turret) return;
    this._turret = turret;
    this.hideRig(null);
    this.previewYaw = this.previewPitch = 0;
    // Off the gun on a page (the seat packet let it go), his body leaves
    // the gunner's pose poseBody gave it.
    if (!turret && this.bodyPosed) this.animator?.gunner?.(null);
    this.bodyPosed = false;
  }

  // Mouse counts while manning go to the drum.
  look(dx, dy, sensitivity, invert) {
    this.lookX += dx * sensitivity;
    this.lookY += (invert ? dy : -dy) * sensitivity;
  }

  enter(turret, soldier) {
    this.turret = turret;
    turret.gunner = soldier;
    turret.humanGunner = this;
    const owner = this.combat?.owner;
    const unit = owner && typeof owner === "object" ? owner : null;
    turret.lease = new SeatLease({ unit, human: true, team: unit?.team ?? this.combat?.team ?? 0, turret, index: 0, role: GUNNER });
    seatWeaponPolicy(turret.weapon, true);
    if (this.combat) this.combat.mounted = true;
    if (unit) {
      unit.seat = turret.lease;
      unit.targetable = false;
    }
    turret.yaw = 0;
    turret.pitch = 0;
    turret.yawVel = 0;
    turret.pitchVel = 0;
    this.viewTurret = null; // fresh views: the camera starts at the turret
    this.viewsFor(turret);
    this.lookX = 0;
    this.lookY = 0;
    soldier.velocity.set(0, 0, 0);
  }

  // Out of the turret. killed: he died at the gun (src/boarding.js
  // leaveKilled): no spot is sought; his body falls from where he stood,
  // its death under way, as a vehicle rider's (VehicleDriver.release). A
  // dead gunner let go later (a page showing a server's battle, after his
  // death arrived) is left where he is too.
  exit(soldier, { killed = false } = {}) {
    const turret = this.turret;
    if (!turret) return;
    const lease = turret.lease;
    const unit = lease?.human ? lease.unit : null;
    const died = killed || Boolean(this.combat?.dead);
    const fall = killed && unit?.soldier ? turret.riderDrop?.(0, unit, this.seat) ?? null : null;
    turret.gunner = null;
    if (turret.humanGunner === this) turret.humanGunner = null;
    if (lease?.human) {
      lease.released = true;
      if (lease.unit?.seat === lease) {
        lease.unit.seat = null;
        lease.unit.targetable = true;
      }
      turret.lease = null;
    }
    this.turret = null;
    if (this.combat) this.combat.mounted = false;
    this.animator.gunner(null);
    // Out of sight at the gun (Turret.hidesGunner), he shows again; a droid
    // blown apart stays gone.
    const root = this.animator.actor?.root;
    if (turret.hidesGunner && root && !this.combat?.blowApart) root.visible = true;
    if (died) {
      if (unit) unseatBody(unit);
      if (fall) dropBody(unit, fall);
      return;
    }
    // The first clear spot around the turret, in its facing's frame.
    const d = EXIT_RADIUS + EXIT_EXTRA;
    const q = 0.707 * d;
    const yaw = turret.facing + turret.yaw;
    const sin = Math.sin(yaw);
    const cos = Math.cos(yaw);
    const centre = this.from.set(0, 1.65, 0.32).applyMatrix4(turret.matrix);
    const spots = [[0, -d], [-d, 0], [d, 0], [-q, -q], [q, -q], [q, q], [-q, q], [0, d], [0, 0]];
    let chosen = null;
    for (const [x, z] of spots) {
      const point = new THREE.Vector3(turret.position.x + cos * x + sin * z, turret.position.y + 1, turret.position.z - sin * x + cos * z);
      // The line in from the spot, stopping short of the turret's own walls
      // (the original game leaves the turret out of this test).
      const toward = centre.clone().sub(point);
      const length = toward.length() - TURRET_CORE;
      if (length <= 0) continue;
      if (!this.collision.cast(point, toward.normalize(), length, this.hit)) {
        chosen = point;
        break;
      }
    }
    chosen ||= turret.position.clone().add(new THREE.Vector3(0, 1, 0));
    soldier.spawn(chosen.x, chosen.y + 0.5, chosen.z, yaw);
  }

  // One frame of manning: steer, aim, fire, and set the soldier and camera.
  update(dt, soldier, controls, zoom = 1) {
    const turret = this.turret;
    if (!turret) return [];
    // Mouse rates as the soldier reads them: counts x sensitivity / (2048 dt),
    // clamped to 16.
    const rate = (counts) => THREE.MathUtils.clamp(counts / (LOOK_SCALE * Math.max(dt, 1e-4)), -LOOK_CLAMP, LOOK_CLAMP);
    turret.steer(dt, -rate(this.lookX), rate(this.lookY), zoom);
    this.lookX = 0;
    this.lookY = 0;

    // The point under the crosshair: along the aim from where his view
    // aims from (his eye in first person, else the chase camera's rest at
    // the eye's plane), past the turret itself and him.
    turret.viewDirection(this.dir);
    turretAimPoint(turret, this.firstPerson, this.dir, this.collision, this.ordnance?.units, turret.lease?.unit ?? null, this.target, this.hit);
    turret.pose(dt, this.target);

    // The soldier stands at the seat, turned as the seat node is (along the
    // gun, Turret.seatTurn), in the turret's pose for its gunner, out of
    // sight where it hides him. His hit shapes follow the drum
    // (Unit.updateSpheres) in the same pose and frame.
    if (turret.seatPoint) turret.seatPoint(this.seat);
    else (turret.seat || turret.rig.root).getWorldPosition(this.seat);
    soldier.position.copy(this.seat);
    soldier.previous.copy(this.seat);
    soldier.settled.copy(this.seat);
    soldier.velocity.set(0, 0, 0);
    soldier.aimYaw = turret.seatYaw ? turret.seatYaw() : turret.facing + turret.yaw;
    soldier.aimPitch = turret.pitch;
    const pose = turret.riderSample?.(this.animator.clips, gunnerPose);
    this.animator.gunner(pose?.clip ?? null, pose?.pitch, pose?.turn, pose?.time, turret.seatTurn?.(gunnerTurn) ?? null);
    if (this.animator.actor?.root) this.animator.actor.root.visible = !turret.hidesGunner;
    if (turret.lease?.unit?.tickCount !== undefined) turret.lease.unit.tickCount += 1;

    // The turret's weapon.
    const shots = [];
    const weapon = turret.weapon;
    if (weapon) {
      if (!weapon.random && this.random) weapon.random = this.random;
      const pressed = controls.take("firePrimary");
      const trigger = { held: controls.held("firePrimary") || pressed, pressed };
      // The lock cone runs from the muzzle through the point under the crosshair.
      turret.muzzle(this.muzzle);
      if (hasMissileLock(weapon.def)) updateWeaponLock(weapon, dt, { units: this.ordnance.units, collision: this.collision,
        origin: this.muzzle, dir: this.gunDir.subVectors(this.target, this.muzzle).normalize(), viewer: turret.aiTarget, ignoreBody: turret.bodies.built });
      for (const shot of weapon.update(dt, true, trigger, false, { stick: 0, posture: "stand", zoomed: zoom > 1 })) {
        turret.muzzle(this.muzzle, this.gunDir, true);
        turret.fireDirection?.(this.muzzle, this.target, this.gunDir, this.gunDir);
        this.gunDir.applyAxisAngle(THREE.Object3D.DEFAULT_UP, ((this.random || Math.random)() - (this.random || Math.random)()) * 0.1 * SPREAD);
        const round = this.ordnance.fire(weapon.def.ordnance, this.muzzle, this.gunDir, this.combat?.owner ?? "player", null, turret.bodies.built, turret.lease?.emitter ?? turret, shot);
        bindMountedRound(round, turret, -1, -1, (turret.barrel - 1) % (turret.firePoints?.length || 1));
        emitSound("turretFire", turret, shot);
        shots.push(shot);
      }
    }
    return shots;
  }

  // The views for a turret (TurretCamera, its own camera section), afresh
  // for each turret manned: the page's, and the world's on simulation's
  // camera where it has one.
  viewsFor(turret) {
    if (this.viewTurret === turret && this.view) return;
    this.viewTurret = turret;
    this.view = new TurretCamera(this.camera, this.collision, turret);
    this.simulationView = this.aimCamera ? new TurretCamera(this.aimCamera, this.collision, turret) : null;
  }

  // The drum ahead of the world: from the aim the world last gave, steered
  // by each command the world has not taken yet (its looks, a tick each) and
  // the looks not yet in a command (null: none), then back to the way
  // through the tick shown (alpha: the input clock's while commands are
  // pending, else the playout's). Sets previewYaw/previewPitch, what the
  // page shows over the turret's own yaw and pitch. A page stepping its own
  // world has nothing pending; it may still interpolate (preview([], null,
  // 0, false, alpha)).
  preview(pending, looks, scale, invert, alpha = 1) {
    const turret = this.turret;
    if (!turret) { this.previewYaw = this.previewPitch = 0; return; }
    const p = this.previewState;
    p.yaw = turret.yaw; p.pitch = turret.pitch; p.yawVel = turret.yawVel; p.pitchVel = turret.pitchVel;
    previewSteer(turret, p, pending, looks, scale, invert);
    // Between the tick before and the one shown: the tick's turn undone by
    // the part of it not yet reached (steer adds yawVel x dt a tick).
    const back = TICK * (1 - THREE.MathUtils.clamp(alpha, 0, 1));
    const pitch = THREE.MathUtils.clamp(p.pitch - p.pitchVel * back, turret.pitchLimits?.[0] ?? -Infinity, turret.pitchLimits?.[1] ?? Infinity);
    const yaw = p.yaw - p.yawVel * back;
    this.previewYaw = wrap((turret.yawLimits && turret.holdYaw ? turret.holdYaw(yaw) : yaw) - turret.yaw);
    this.previewPitch = pitch - turret.pitch;
  }

  // The aim as shown: the world's, with the preview over it.
  shownAim() {
    const turret = this.turret;
    this.shownYaw = turret.facing + turret.yaw + this.previewYaw;
    this.shownPitch = turret.pitch + this.previewPitch;
  }

  // Where the HUD's reticle goes: from the drawn camera toward the point the
  // shown aim meets (seats.js turretAimPoint, from the view's own aim
  // origin), so it stays on what the gun turns to and the rounds go to even
  // while walls pull the chase camera in off its rest, or it eases after a
  // turn; the shown aim itself where nothing is shown.
  reticleDirection(out) {
    const turret = this.turret;
    if (!turret?.aimOrigin) return this.viewDirection(out);
    this.viewDirection(this.dir);
    turretAimPoint(turret, this.viewFirstPerson, this.dir, this.collision, this.ordnance?.units, turret.lease?.unit ?? null, this.reticlePoint, this.hit,
      this.shownYaw - turret.facing, this.shownPitch);
    out.subVectors(this.reticlePoint, this.camera.position);
    return out.lengthSq() > 1e-8 ? out.normalize() : out.copy(this.dir);
  }

  // The direction under the crosshair as shown.
  viewDirection(out) {
    const turret = this.turret;
    if (!turret) return out.set(0, 0, 1);
    this.shownAim();
    return out.set(Math.sin(this.shownYaw) * Math.cos(this.shownPitch), Math.sin(this.shownPitch), Math.cos(this.shownYaw) * Math.cos(this.shownPitch));
  }

  // The first-person eye: EyePointOffset in the turret's aim frame
  // (Turret.eyePoint), at a world yaw and pitch (the shown ones).
  eyePoint(out, yaw = this.shownYaw, pitch = this.shownPitch) {
    const turret = this.turret;
    return turret.eyePoint(out, yaw - turret.facing, pitch);
  }

  // The drum and gun out of the view (first person), as his own body is:
  // on the hidden layer, where they still cast their shadows.
  hideRig(turret) {
    if (this.hiddenRig === turret) return;
    const layer = (t, value) => {
      if (!t?.rig?.root) return;
      const parts = [t.drum, t.gun].filter(Boolean);
      for (const part of parts.length ? parts : [t.rig.root]) part.traverse((object) => { if (object.isMesh) object.layers.set(value); });
    };
    layer(this.hiddenRig, 0);
    layer(turret, this.hiddenLayer);
    this.hiddenRig = turret;
  }

  // His own body at the gun as the page draws it, before the animator runs
  // (game.js frame): a page showing a worker's or a server's battle never
  // runs update(), and was leaving its own soldier in his walking clips at
  // the replicated seat point, arms down, while every other page drew him
  // (TurretSeats.pose) and the world struck him (Turret.riderShapes) in the
  // gunner's pose. Here he takes that pose at the aim the page shows (the
  // world's with the preview over it, as the drum and gun are drawn: the
  // previewed pitch and turn speed), at the seat node of the drum as drawn,
  // turned with it. state: the soldier state the animator reads (aimYaw
  // and aimPitch are set); root: his body's root. Returns whether he mans
  // a turret.
  poseBody(state, root) {
    const turret = this.turret;
    if (!turret?.seatPoint || !this.animator) return false;
    this.shownAim();
    poseTurret(turret, turret.yaw + this.previewYaw, this.shownPitch);
    turret.seatPoint(this.seat);
    if (root) {
      root.position.copy(this.seat);
      root.visible = !turret.hidesGunner;
    }
    state.aimYaw = turret.seatYaw();
    state.aimPitch = this.shownPitch;
    const pose = turret.riderSample(this.animator.clips, gunnerPose, this.shownPitch, this.previewState.yawVel);
    this.animator.gunner(pose?.clip ?? null, pose?.pitch, pose?.turn, pose?.time, turret.seatTurn(gunnerTurn));
    this.bodyPosed = true;
    return true;
  }

  // The turret's camera along its aim, and the drum and gun as the page
  // shows them: the drum at the previewed yaw, the gun at its own angles
  // (the world's, or the worker's as replicated). simulation: the world's
  // own view, never previewed; which perspective it has is the one the
  // world's aim starts from (update()).
  updateCamera(dt, zoom, firstPerson, simulation = false) {
    const turret = this.turret;
    if (!turret) return;
    this.viewsFor(turret);
    const view = simulation ? this.simulationView : this.view;
    if (simulation) this.firstPerson = Boolean(firstPerson);
    if (!view) return;
    if (simulation) {
      view.update(dt, turret, turret.yaw, turret.pitch, firstPerson, zoom);
      return;
    }
    this.shownAim();
    this.viewFirstPerson = Boolean(firstPerson);
    const yaw = turret.yaw + this.previewYaw;
    poseTurret(turret, yaw, this.shownPitch);
    // First person from inside the turret's box: the drum and gun leave the
    // view (an eye above or behind it, the Yavin tower's or the AAA's,
    // keeps them, as a saddle keeps its hull).
    this.hideRig(firstPerson && turret.encloses?.(this.eyePoint(this.eye)) ? turret : null);
    // His body out of sight where the turret hides its gunner, on any page
    // (update() keeps the world's so; a server sends no such thing).
    const root = this.animator?.actor?.root;
    if (root) root.visible = !turret.hidesGunner;
    view.update(dt, turret, yaw, this.shownPitch, firstPerson, zoom);
  }
}

// A turret seat's camera, as the 2004 game's (the seat's camera on the
// camera section; docs/formats.md "Vehicle seats"). Behind the
// gun, it looks along the aim with the aim frame's up, pitched down by
// TiltValue, from TrackCenter in the aim frame plus TrackOffset in its own
// (Turret.cameraRest); its turn follows the mouse at once (a player's aim
// already has its final direction, as a vehicle seat's), and each of its
// axes eases toward where it should be at MoveTension, 30/s unless the
// class sets it. Five rays from 1.5 m up in the aim frame, the turret
// itself left out, pull it in at walls at once; it eases back out at 6/s.
// More than 80 m from where it should be, it jumps. In first person it is
// at the eye looking straight down the aim. Both fields of view are the
// class's (48 degrees across); zoomed, the first person's, narrowed by the
// zoom.
const RAY_ORIGIN = Object.freeze(new THREE.Vector3(0, 1.5, 0));
const CAMERA_RATE = 30; // 1/s
const CAMERA_SNAP = 80; // m
const CORNER = 0.4; // m: offset of the four corner rays
const CAMERA_SKIN = 0.08; // m: keeps the near plane off what the rays hit
const RETURN_RATE = 6; // 1/s
const FOV_RATE = 25; // 1/s
const RAYS = Object.freeze([0, 0, 1, 1, 1, -1, -1, 1, -1, -1]);

export class TurretCamera {
  constructor(camera, collision, turret) {
    this.camera = camera;
    this.collision = collision;
    const tension = cameraTension(turret?.desc?.settings || {}, CAMERA_RATE);
    this.moveTension = tension.moveTension;
    this.fov = null;
    this.reach = 1;
    this.fresh = true;
    this.pivot = new THREE.Vector3();
    this.target = new THREE.Vector3();
    this.origin = new THREE.Vector3();
    this.boom = new THREE.Vector3();
    this.right = new THREE.Vector3();
    this.up = new THREE.Vector3();
    this.dir = new THREE.Vector3();
    this.hit = makeHit();
    camera.rotation.order = "YXZ";
  }

  reset() {
    this.fresh = true;
    this.reach = 1;
  }

  // The field of view across eases toward the view's (25/s), narrowed by a
  // zoom; the renderer's vertical one follows the window's shape.
  updateFov(dt, turret, zoom, firstPerson) {
    const base = firstPerson || zoom > 1 ? turret.camera.firstFov : turret.camera.thirdFov;
    const want = 2 * Math.atan(Math.tan(base / 2) / Math.max(zoom, 1));
    this.fov = this.fov === null ? want : this.fov + (want - this.fov) * (1 - Math.exp(-FOV_RATE * dt));
    const cam = this.camera;
    const vertical = THREE.MathUtils.radToDeg(2 * Math.atan(Math.tan(this.fov / 2) / (cam.aspect || 1)));
    if (cam.fov !== vertical) {
      cam.fov = vertical;
      cam.updateProjectionMatrix?.();
    }
  }

  // yaw: the aim's from the building's heading; pitch up positive.
  update(dt, turret, yaw, pitch, firstPerson = false, zoom = 1) {
    const cam = this.camera;
    this.updateFov(dt, turret, zoom, firstPerson);
    if (firstPerson) {
      turret.cameraTurn(cam.quaternion, yaw, pitch, 0);
      turret.eyePoint(cam.position, yaw, pitch);
      cam.updateMatrixWorld();
      this.fresh = true;
      return;
    }
    turret.cameraRest(this.target, yaw, pitch, this.pivot);
    turret.cameraTurn(cam.quaternion, yaw, pitch);
    this.origin.copy(RAY_ORIGIN).applyMatrix4(turret.frameMatrix);
    const snap = this.fresh || cam.position.distanceTo(this.target) > CAMERA_SNAP;
    this.boom.subVectors(this.target, this.origin);
    const reach = this.clearance(turret, cam.quaternion);
    if (snap || reach < this.reach) this.reach = reach;
    else this.reach += (reach - this.reach) * (1 - Math.exp(-RETURN_RATE * dt));
    this.target.copy(this.origin).addScaledVector(this.boom, this.reach);
    if (snap) cam.position.copy(this.target);
    else {
      easeAxes(cam, this.target, this.moveTension, dt);
      // The eased spot can lag behind a new wall: check where it will draw.
      this.boom.subVectors(cam.position, this.origin);
      const clear = this.clearance(turret, cam.quaternion);
      if (clear < 1) cam.position.copy(this.origin).addScaledVector(this.boom, clear);
    }
    this.fresh = false;
    cam.updateMatrixWorld();
  }

  // The share of the boom (from origin) that is clear: the nearest hit of
  // the centre ray and four to the camera's corners, the turret left out.
  clearance(turret, q) {
    if (this.boom.lengthSq() < 1e-8 || !this.collision) return 1;
    this.right.set(CORNER, 0, 0).applyQuaternion(q);
    this.up.set(0, CORNER, 0).applyQuaternion(q);
    let reach = 1;
    for (let k = 0; k < RAYS.length; k += 2) {
      this.dir.copy(this.boom).addScaledVector(this.right, RAYS[k]).addScaledVector(this.up, RAYS[k + 1]);
      const far = this.dir.length();
      this.dir.divideScalar(far);
      const hit = this.collision.cast(this.origin, this.dir, far + CAMERA_SKIN, this.hit, turret.bodies?.built || null);
      if (hit) reach = Math.min(reach, Math.max(0, hit.distance - CAMERA_SKIN) / far);
    }
    return reach;
  }
}

const TICK = 1 / 60;
const wrap = (angle) => Math.atan2(Math.sin(angle), Math.cos(angle));

// A mouse count sum as a rate, as update() reads it for the tick.
const rateOf = (counts, dt) => THREE.MathUtils.clamp(counts / (LOOK_SCALE * dt), -LOOK_CLAMP, LOOK_CLAMP);

// Steers a copy of a turret's aim (state: yaw, pitch, yawVel, pitchVel) by
// the pending commands, a tick each (one without a look still lets the
// drum's turn ease off), then by the loose looks (null: none) as the tick
// of the command they will go in, with the turret's own steer (its rates,
// filter and pitch limits). Pure: the turret itself is not changed.
// Returns state.
export function previewSteer(turret, state, pending = [], looks = null, scale = 1, invert = false, dt = TICK) {
  if (typeof turret.steer !== "function") return state;
  const scratch = { yaw: state.yaw, pitch: state.pitch, yawVel: state.yawVel, pitchVel: state.pitchVel,
    turnRate: turret.turnRate, filter: turret.filter, pitchLimits: turret.pitchLimits, yawLimits: turret.yawLimits ?? null, wrapYaw: Boolean(turret.wrapYaw) };
  const step = (look, s, inv) => {
    let x = 0, y = 0;
    for (const [dx, dy] of look) { x += dx * s; y += (inv ? dy : -dy) * s; }
    turret.steer.call(scratch, dt, -rateOf(x, dt), rateOf(y, dt), 1);
  };
  for (const command of pending) step(command.look || [], command.mouseScale ?? scale, command.invertMouse ?? invert);
  if (looks) step(looks, scale, invert);
  state.yaw = scratch.yaw; state.pitch = scratch.pitch; state.yawVel = scratch.yawVel; state.pitchVel = scratch.pitchVel;
  return state;
}
