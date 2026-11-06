import * as THREE from "three";

const NETWORK_POSE = ['posture', 'air', 'dive', 'dead'];
const WALKER_POSE = ["roll", "lean", "legs", "time"];

// Dedicated servers never interpolate a rendered frame. Keep the interface
// used by spawn/seat transitions, with live authoritative coordinates, without
// copying every soldier's pose before and after each physics step.
export class HeadlessSoldierPresentation {
  constructor(soldier) {
    this.soldier = this.state = soldier;
    this.position = soldier.position;
    this.velocity = soldier.velocity;
    this.previousYaw = soldier.aimYaw;
    this.tickYaw = 0;
  }
  reset() { this.previousYaw = this.soldier.aimYaw; }
  beforeStep() { this.previousYaw = this.soldier.aimYaw; }
  afterStep() {}
  update() { return this.state; }
}

// Rendering samples the last simulation interval without changing collision,
// weapon aim, or input. Mouse look remains immediate; only turns produced by
// the fixed physics tick are interpolated. Small stair corrections get a
// separate visual offset shared by the body, held weapon, and camera.
export class SoldierPresentation {
  constructor(soldier) {
    this.soldier = soldier;
    this.position = new THREE.Vector3();
    this.velocity = new THREE.Vector3();
    this.previousPosition = new THREE.Vector3();
    this.previousVelocity = new THREE.Vector3();
    this.previousPose = {};
    this.state = Object.create(soldier);
    this.state.position = this.position;
    this.state.velocity = this.velocity;
    this.reset();
  }

  reset() {
    const s = this.soldier;
    this.networkPose = null;
    this.networkFire = null;
    this.networkFired = false;
    this.deathPending = false;
    this.clearNetworkState();
    this.position.copy(s.position);
    this.velocity.copy(s.velocity);
    this.previousPosition.copy(s.position);
    this.previousVelocity.copy(s.velocity);
    this.previousYaw = s.aimYaw;
    this.tickYaw = 0;
    this.previousStep = this.lastStep = s.groundAdjustment || 0;
    this.stepOffset = 0;
    this.state.aimYaw = s.aimYaw;
    this.state.aimPitch = s.aimPitch;
    this.capturePose();
    if (s.mode !== undefined) for (const key of WALKER_POSE) this.state[key] = s[key];
  }

  capturePose() {
    const s = this.soldier;
    this.previousMode = s.mode;
    if (s.mode !== undefined) for (const key of WALKER_POSE) this.previousPose[key] = s[key];
  }

  clearNetworkState() {
    for (const key of NETWORK_POSE) delete this.state[key];
  }

  beforeStep() {
    const s = this.soldier;
    this.previousPosition.copy(s.position);
    this.previousVelocity.copy(s.velocity);
    this.previousYaw = s.aimYaw;
    this.previousStep = s.groundAdjustment || 0;
    this.capturePose();
  }

  afterStep() {
    const s = this.soldier;
    this.tickYaw = Math.atan2(Math.sin(s.aimYaw - this.previousYaw), Math.cos(s.aimYaw - this.previousYaw));
    if (s.position.distanceToSquared(this.previousPosition) > 100) this.reset();
  }

  update(dt, alpha = 1) {
    const s = this.soldier;
    this.networkFired = false;
    alpha = THREE.MathUtils.clamp(alpha, 0, 1);
    this.position.lerpVectors(this.previousPosition, s.position, alpha);
    this.velocity.lerpVectors(this.previousVelocity, s.velocity, alpha);
    this.state.aimYaw = s.aimYaw - this.tickYaw * (1 - alpha);
    this.state.aimPitch = s.aimPitch;
    if (s.mode !== undefined) for (const key of WALKER_POSE) {
      const previous = this.previousPose[key], current = s[key];
      // Roll is an accumulated angle: wrapping it reverses fast spins.
      // A new mode owns a different clip clock and must not interpolate from
      // the fold/unfold clip that just ended.
      const same = this.previousMode === s.mode && Number.isFinite(previous) && Number.isFinite(current);
      const restart = key === "time" && (s.reverse ? current > previous : current < previous);
      this.state[key] = same && !restart ? THREE.MathUtils.lerp(previous, current, alpha) : current;
    }
    const step = THREE.MathUtils.lerp(this.previousStep, s.groundAdjustment || 0, alpha);
    this.stepOffset = THREE.MathUtils.clamp(this.stepOffset + step - this.lastStep, -0.45, 0.45);
    this.lastStep = step;
    this.stepOffset *= Math.exp(-Math.max(dt, 0) / 0.075);
    this.position.y -= this.stepOffset;
    return this.state;
  }
}
