import { stepTurretPreview } from './runtime/turret-prediction.js';
import * as THREE from "three";

import { unseatBody, turretAimPoint } from "./seats.js";

// Page view of the player at a turret. EntitySeats installs the world's lease;
// preview() supplies unacknowledged look to the native prop and body phases;
// the native camera phase follows their published turret and rider.
// First-person eyes inside the hull hide its rig just as a cockpit does.

const LOOK_SCALE = 2048; // counts x sensitivity per unit of turn rate, as for soldiers
const LOOK_CLAMP = 16;

export class Gunner {
  constructor({ camera, collision, ordnance, combat, animator, hiddenLayer = 1 }) {
    this.camera = camera;
    this.collision = collision;
    this.ordnance = ordnance;
    this.combat = combat;
    this.animator = animator;
    this.hiddenLayer = hiddenLayer;
    this._turret = null;
    this.hiddenRig = null; // the turret whose drum and gun are hidden (first person)
    // What the page shows over the world's aim: the looks it has not taken
    // yet and the way through the tick (preview()).
    this.previewYaw = 0;
    this.previewPitch = 0;
    this.previewState = { yaw: 0, pitch: 0, yawVel: 0, pitchVel: 0 };
    this.shownYaw = 0;
    this.shownPitch = 0;
    this.eye = new THREE.Vector3();
    this.firstPerson = false; // the world's view: where its aim starts (the camera phase)
    this.viewFirstPerson = false; // the page's view: where its reticle's aim starts
    this.reticlePoint = new THREE.Vector3();
    this.lookX = 0;
    this.lookY = 0;
    this.dir = new THREE.Vector3();
    this.target = new THREE.Vector3();
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
  }

  get active() {
    return Boolean(this.turret);
  }

  // The turret manned, supplied by the seat packet (SeatReader.setGunner). Another turret, or
  // none: what the page showed of the last one is let go.
  get turret() {
    return this._turret;
  }
  set turret(turret) {
    if (turret === this._turret) return;
    const previous = this._turret;
    if (previous?.humanGunner === this) { previous.humanGunner = null; previous.gunner = null; }
    this._turret = turret;
    if (turret) { turret.humanGunner = this; turret.gunner = this; }
    this.hideRig(null);
    this.previewYaw = this.previewPitch = 0;
  }

  // SeatReader.clearSeat and page respawn only release presentation. The
  // native crew owner has already decided the exit and the body's placement.
  exit(soldier) {
    const turret = this.turret;
    if (!turret) return;
    const lease = turret.lease;
    if (lease?.human) {
      lease.released = true;
      if (lease.unit?.seat === lease) { lease.unit.seat = null; unseatBody(lease.unit); }
      turret.lease = null;
    }
    turret.gunner = null; turret.humanGunner = null;
    this.turret = null;
    if (this.combat) this.combat.mounted = false;
    this.animator?.gunner?.(null);
    const root = this.animator?.actor?.root;
    if (turret.hidesGunner && root && !this.combat?.blowApart) root.visible = true;
    this.hideRig(null);
  }

  // Mouse counts while manning go to the drum.
  look(dx, dy, sensitivity, invert) {
    this.lookX += dx * sensitivity;
    this.lookY += (invert ? dy : -dy) * sensitivity;
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
  const scratch = { yaw: state.yaw, pitch: state.pitch, yawVel: state.yawVel, pitchVel: state.pitchVel,
    turnRate: turret.turnRate, filter: turret.filter, pitchLimits: turret.pitchLimits, yawLimits: turret.yawLimits ?? null, wrapYaw: Boolean(turret.wrapYaw) };
  const step = (look, s, inv) => {
    let x = 0, y = 0;
    for (const [dx, dy] of look) { x += dx * s; y += (inv ? dy : -dy) * s; }
    stepTurretPreview(scratch, dt, -rateOf(x, dt), rateOf(y, dt), 1);
  };
  for (const command of pending) step(command.look || [], command.mouseScale ?? scale, command.invertMouse ?? invert);
  if (looks) step(looks, scale, invert);
  state.yaw = scratch.yaw; state.pitch = scratch.pitch; state.yawVel = scratch.yawVel; state.pitchVel = scratch.pitchVel;
  return state;
}
