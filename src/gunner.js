import * as THREE from "three";

import { unseatBody, turretAimPoint } from "./seats.js";

// Page view of the player at a turret. EntitySeats installs the world's lease;
// The native network frame supplies unacknowledged look to prop and body phases;
// the native camera phase follows their published turret and rider.
// First-person eyes inside the hull hide its rig just as a cockpit does.

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
    // yet and the way through the tick, published by the native frame.
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
