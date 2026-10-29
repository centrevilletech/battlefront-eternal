import * as THREE from "three";

// The battle behind the page spawn screen, through the mission's camera
// shots (its AddCameraShot views, the original's backdrop for the screen):
// each one held for a while, drifting (a slow pan and a push in) so the
// battle is never a still, then a cut to the next. A post picked brings up
// the shot that best sees it, turned partway toward it.
//
// Only turns and a short push along the shot's own view, so the camera
// never leaves the space the mission's author chose.

export const HOLD = 11; // s a shot is held
const PAN = THREE.MathUtils.degToRad(4.5); // either side of the shot's view
const PUSH = 2.5; // m along its view over the hold
const AIM = 0.4; // of the way from the shot's view to a post picked

const UP = new THREE.Vector3(0, 1, 0);
const ease = (t) => t * t * (3 - 2 * t);

export class ShotDirector {
  // shots: [{ position: [x, y, z], quaternion: [x, y, z, w] }].
  constructor(shots = []) {
    this.shots = shots.map((shot) => ({
      position: new THREE.Vector3().fromArray(shot.position),
      quaternion: new THREE.Quaternion().fromArray(shot.quaternion).normalize(),
    }));
    this.index = -1;
    this.age = 0;
    this.turn = 1; // the pan's way, alternating
    this.aim = new THREE.Quaternion(); // toward a post picked, or none
    this.cut = false;
  }

  get active() {
    return this.shots.length > 0;
  }

  // Start on a shot (an index, wrapping).
  show(index) {
    if (!this.active) return;
    this.index = ((index % this.shots.length) + this.shots.length) % this.shots.length;
    this.age = 0;
    this.turn = -this.turn;
    this.aim.identity();
    this.cut = true;
  }

  // The shot that best sees a world point: near it, looking its way.
  bestFor(point) {
    let best = -1;
    let score = Infinity;
    const to = new THREE.Vector3();
    const forward = new THREE.Vector3();
    this.shots.forEach((shot, i) => {
      to.subVectors(point, shot.position);
      const distance = Math.max(to.length(), 1);
      forward.set(0, 0, -1).applyQuaternion(shot.quaternion);
      const facing = forward.dot(to.normalize()); // 1 looking at it
      const value = distance * (2.2 - facing);
      if (value < score) {
        score = value;
        best = i;
      }
    });
    return best;
  }

  // A post picked at a world point ([x, y, z]): the shot that sees it best,
  // turned partway toward it (cutting only if that is another shot).
  follow(position) {
    if (!this.active || !position) return;
    const point = new THREE.Vector3().fromArray(position);
    const index = this.bestFor(point);
    if (index !== this.index) this.show(index);
    const shot = this.shots[this.index];
    const forward = new THREE.Vector3(0, 0, -1).applyQuaternion(shot.quaternion);
    const to = point.sub(shot.position).normalize();
    // Never past straight up or down, and only within a half turn.
    if (forward.dot(to) < -0.2) return;
    this.aim.setFromUnitVectors(forward, to);
    this.aim.slerp(new THREE.Quaternion(), 1 - AIM);
  }

  // The view this frame, or null without shots: { position, quaternion }
  // as arrays, cut true on the first frame of a shot.
  update(dt) {
    if (!this.active) return null;
    if (this.index < 0) this.show(0);
    this.age += dt;
    if (this.age >= HOLD) this.show(this.index + 1);
    const shot = this.shots[this.index];
    const t = ease(Math.min(this.age / HOLD, 1));
    const pan = new THREE.Quaternion().setFromAxisAngle(UP, this.turn * PAN * (t * 2 - 1));
    const quaternion = pan.multiply(this.aim).multiply(shot.quaternion);
    const forward = new THREE.Vector3(0, 0, -1).applyQuaternion(quaternion);
    const position = shot.position.clone().addScaledVector(forward, PUSH * t);
    const cut = this.cut;
    this.cut = false;
    return { position: position.toArray(), quaternion: quaternion.toArray(), cut };
  }
}
