import * as THREE from "three";

// The page spawn screen's camera (stage.js): it looks at the soldier from a
// little above, framed so that his body fills the page's place for it
// (frame()), and it moves:
// - opening, it sweeps in from his side, high and far back;
// - it turns about him a little with the pointer, and drifts;
// - another class or team swings it round; a drag on him tilts it;
// - SPAWN pushes it in.
// The framing is a lens shift: whatever the camera does, the soldier stays
// in his place in the page while the room and the map move behind him.
//
// The scene is in metres, his feet at the origin, facing +z.

export const FOV = 32; // degrees up
export const BODY = 1.9; // m: the height framed, feet to the top of the head
export const PIVOT = 0.95; // m: the height turned about
const ELEVATION = THREE.MathUtils.degToRad(8); // looking down on him
const PARALLAX = [0.07, 0.035]; // rad at the page's edges: yaw, pitch
const DRAG_PITCH = 0.2; // rad either way
const SPRING = 2.4; // the springs' rate (1/s): about a second and a half to settle

// A critically damped spring toward its target.
export class Spring {
  constructor(value = 0, rate = SPRING) {
    this.value = value;
    this.target = value;
    this.velocity = 0;
    this.rate = rate;
  }

  step(dt) {
    const w = this.rate;
    const x = this.value - this.target;
    const c = this.velocity + w * x;
    const e = Math.exp(-w * dt);
    this.value = this.target + (x + c * dt) * e;
    this.velocity = (this.velocity - w * c * dt) * e;
  }

  set(value) {
    this.value = this.target = value;
    this.velocity = 0;
  }
}

export class CameraRig {
  constructor() {
    this.camera = new THREE.PerspectiveCamera(FOV, 16 / 9, 0.05, 600);
    this.aspect = 16 / 9;
    this.distance = 5.2; // m from the pivot, at rest
    this.shift = [0, 0]; // the lens shift, in NDC
    this.yaw = new Spring();
    this.pitch = new Spring();
    this.dolly = new Spring(1);
    this.crane = new Spring();
    this.pointer = [0, 0]; // -1..1 across and up the page
    this.dragPitch = 0;
    this.time = 0;
    this.pivot = new THREE.Vector3();
    this.at = new THREE.Vector3();
  }

  // The rest framing: the body (BODY tall, its feet at the origin) fills
  // figure = { x, top, bottom } (NDC: x across, y up) on a canvas of this
  // aspect. Returns false for no place.
  frame(aspect, figure) {
    const height = figure.top - figure.bottom;
    if (!(aspect > 0 && height > 0)) return false;
    this.aspect = aspect;
    const t = Math.tan(THREE.MathUtils.degToRad(FOV / 2));
    let distance = BODY / (height * t);
    const feet = new THREE.Vector3();
    const head = new THREE.Vector3();
    for (let i = 0; i < 4; i++) {
      this.distance = distance;
      this.shift = [0, 0];
      this.pose(0, 0, 1, 0);
      feet.set(0, 0, 0).project(this.camera);
      head.set(0, BODY, 0).project(this.camera);
      const seen = head.y - feet.y;
      if (!(seen > 0)) return false;
      distance *= seen / height;
    }
    this.distance = distance;
    this.pose(0, 0, 1, 0);
    feet.set(0, 0, 0).project(this.camera);
    head.set(0, BODY, 0).project(this.camera);
    this.shift = [figure.x - feet.x, (figure.top + figure.bottom) / 2 - (head.y + feet.y) / 2];
    this.pose(0, 0, 1, 0);
    return true;
  }

  // The camera at rest: for measuring what is placed by the page.
  rest() {
    this.pose(0, 0, 1, 0);
    return this.camera;
  }

  // Opening: from his right, high and far back, sweeping round to rest.
  sweepIn() {
    this.yaw.value = 0.62;
    this.yaw.velocity = 0;
    this.pitch.value = 0.2;
    this.pitch.velocity = 0;
    this.dolly.set(1);
    this.dolly.value = 1.75;
    this.crane.set(0);
    this.crane.value = 0.9;
    this.dragPitch = 0;
  }

  // A change swings it: "class" a little, "team" further; "spawn" pushes
  // in for the drop.
  kick(kind) {
    if (kind === "class") {
      this.yaw.velocity += 0.85;
      this.dolly.velocity -= 0.3;
    } else if (kind === "team") {
      this.yaw.velocity -= 2.4;
      this.crane.velocity += 0.6;
    } else if (kind === "spawn") {
      this.dolly.target = 0.55;
      this.crane.target = -0.25;
      this.dolly.rate = this.crane.rate = 4.5;
    }
  }

  // Back to choosing (a spawn the world didn't take).
  settle() {
    this.dolly.target = 1;
    this.crane.target = 0;
    this.dolly.rate = this.crane.rate = SPRING;
  }

  // The pointer's place on the page (-1..1, y up), for the parallax.
  look(x, y) {
    this.pointer[0] = Math.max(-1, Math.min(1, x));
    this.pointer[1] = Math.max(-1, Math.min(1, y));
  }

  // A vertical drag on him tilts it (radians, kept within DRAG_PITCH).
  tilt(delta) {
    this.dragPitch = Math.max(-DRAG_PITCH, Math.min(DRAG_PITCH, this.dragPitch + delta));
  }

  update(dt) {
    this.time += dt;
    this.yaw.target = -this.pointer[0] * PARALLAX[0];
    this.pitch.target = -this.pointer[1] * PARALLAX[1] + this.dragPitch;
    for (const spring of [this.yaw, this.pitch, this.dolly, this.crane]) spring.step(dt);
    const t = this.time;
    this.pose(
      this.yaw.value + 0.022 * Math.sin(t * 0.23),
      this.pitch.value + 0.012 * Math.sin(t * 0.17 + 1.3),
      this.dolly.value,
      this.crane.value + 0.025 * Math.sin(t * 0.29 + 0.4),
    );
  }

  // The camera at yaw and pitch (radians) about the pivot, its distance
  // times dolly, raised by crane (m), with the framing's lens shift.
  pose(yaw, pitch, dolly, crane) {
    const camera = this.camera;
    const elevation = ELEVATION + pitch;
    const d = this.distance * dolly;
    this.pivot.set(0, PIVOT + crane, 0);
    camera.position.set(
      Math.sin(yaw) * Math.cos(elevation) * d,
      PIVOT + crane + Math.sin(elevation) * d,
      Math.cos(yaw) * Math.cos(elevation) * d,
    );
    camera.up.set(0, 1, 0);
    camera.lookAt(this.pivot);
    if (camera.aspect !== this.aspect) camera.aspect = this.aspect;
    camera.updateProjectionMatrix();
    const m = camera.projectionMatrix.elements;
    m[8] -= this.shift[0];
    m[9] -= this.shift[1];
    camera.projectionMatrixInverse.copy(camera.projectionMatrix).invert();
    camera.updateMatrixWorld(true);
  }
}
