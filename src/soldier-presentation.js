import * as THREE from "three";
import {ENTITY_POSTURES,ENTITY_SOLDIER_STATES} from './runtime/network-entity-codec.js';

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
    for (const key of ['posture','air','dive','dead']) delete this.state[key];
    this.networkState = false;
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
    // Independent remote entity samples already interpolate their own two
    // timestamps. Applying the world's alpha again would introduce a second
    // interpolation and would advance actors absent from the latest packet.
    if (this.networkPose) {
      this.position.fromArray(this.networkPose.position);
      this.velocity.fromArray(this.networkPose.velocity);
      this.state.aimYaw = this.networkPose.yaw;
      this.state.aimPitch = this.networkPose.pitch;
      if (this.networkPose.kind === 'soldier') {
        // A lost shot event must not leave a firing replica in its relaxed
        // stance. Consume only counters at the rendered sample's tick, never
        // the newer collision state. Joining/respawning establishes a baseline.
        const pose = this.networkPose, previous = this.networkFire;
        if (Number.isSafeInteger(pose.fireCounter) && Number.isSafeInteger(pose.fromTick)) {
          if (!previous || previous.generation !== pose.generation) {
            this.networkFire = { generation: pose.generation, tick: pose.fromTick, counter: pose.fireCounter };
          } else if (pose.fromTick > previous.tick) {
            const shots = pose.fireCounter - previous.counter;
            this.networkFired = shots >= 1 && shots <= 3;
            previous.tick = pose.fromTick;
            previous.counter = Math.max(previous.counter, pose.fireCounter);
          }
        }
        // Animation follows the same historical sample as the drawn body,
        // not the newer collision state already received from the server.
        const mode = ENTITY_SOLDIER_STATES[this.networkPose.state];
        this.state.posture = ENTITY_POSTURES[this.networkPose.posture];
        this.state.air = mode === 'thrownDead' ? 'thrown' : ['jump','fall','thrown','jet'].includes(mode) ? mode : null;
        this.state.dive = mode === 'diveRight' ? 1 : mode === 'diveLeft' ? -1 : 0;
        this.state.dead = mode === 'dead' || mode === 'thrownDead';
        this.networkState = true;
      }
      if (this.networkPose.kind === 'droideka') {
        for (const key of [...WALKER_POSE, 'mode', 'reverse', 'dead']) this.state[key] = this.networkPose[key];
      }
      this.stepOffset = 0;
      return this.state;
    }
    this.networkFire = null;
    if (this.networkState) {
      this.clearNetworkState();
      // Off records (a body at rest stops sending them): no tick has been
      // stepped here since, so the last tick's start is wherever it was
      // before them (a dead gunner's, his seat 7 m up on an AT-TE). Start
      // from where the body is.
      this.previousPosition.copy(s.position);
      this.previousVelocity.copy(s.velocity);
    }
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
