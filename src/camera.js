import * as THREE from "three";
import { makeHit } from "./collision.js";

// Third-person chase camera, following the 2004 engine.
//
// The camera looks along the soldier's aim, pitched down by the section's
// TiltValue, and sits at TrackCenter (the pivot, in the soldier's frame) plus
// TrackOffset (in the camera's own frame: right, up, back). With no sideways
// offset that puts the pivot dead center. Rotation eases toward its target
// at 30/s and position at 15/s per camera axis. Five rays from the pivot pull
// the camera in at walls; it snaps in and eases back out.
//
// ThirdPersonFOV is the horizontal field of view: the engine keeps
// tan(fov / 2) on x and scales y by the window's height over width. Zoomed in,
// the camera takes the posture's ZOOM section and the field of view narrows
// by the weapon's zoom factor.
//
// In first person the camera sits at the eye the soldier's aim gives it (see
// PlayerCombat.eye), or at the one its caller gives, and looks straight
// along the aim with no tilt or easing. (A turret's views are its own seat
// camera's, src/gunner.js TurretCamera.) First person, and
// zoomed in from either view, the field of view is FirstPersonFOV.
//
// In first person the eye also plays the game's camera motions (see
// FirstPersonMotion): diving, lying down and getting up, landing, and the
// head bob.
//
// When the soldier dies, DeathCamera takes over from whichever view he had:
// it looks down on the body, then over it at the killer.

const ROTATE_RATE = 30; // 1/s, the section default
const MOVE_RATE = 15; // 1/s per camera axis, the section default
const PRONE_MOVE = 0.5;
const FOV_RATE = 25; // 1/s
const CORNER = 0.4; // m, offset of the four extra wall rays
const CAMERA_SKIN = 0.08; // keep the near plane off collision surfaces
const RETURN_RATE = 6; // 1/s, continuous release from a wall/floor
const SNAP = 80; // m; farther than this from the target and the camera jumps

const SECTIONS = { stand: "STAND", crouch: "CROUCH", prone: "PRONE" };

// The head bob's keys: eye offsets in the camera's frame (x right, y up, z
// back) and their tangents, one per 0.2 s segment.
const BOB_TIME = 0.2; // s per segment
const BOB_VEC = [[0, 0.1, 0], [0, 0, 0], [0, 0.1, 0], [0, 0, 0]];
const BOB_SLOPE = [[0.1, 0.1, 0], [0.2, 0.2, 0], [0.2, 0.1, 0], [0, 0, 0]];
const BOB_START = 1; // m^2/s^2 of horizontal speed^2 that starts a cycle
const BOB_STOP = 0.1; // and below which the bob drops
const BOB_PRONE = 0.5;
const RETURN_TIME = 0.3; // s for the eye to settle after a motion that ends off centre
const ZERO = [0, 0, 0];

// The motions whose arms drop and come back up (handsdown, then jump_land
// for the length of the motion).
const ARMS_CUE = new Set(["RollLeft", "RollRight", "CrouchToProne", "ProneToStand"]);
const POSTURE_MOTIONS = { "stand>prone": "StandToProne", "crouch>prone": "CrouchToProne", "prone>stand": "ProneToStand", "crouch>stand": "CrouchToStand" };

// One cubic Hermite segment at u in [0, 1], and its slope.
function hermite(p0, p1, t0, t1, u) {
  const a = 2 * (p0 - p1) + t0 + t1;
  const b = 3 * (p1 - p0) - 2 * t0 - t1;
  return ((a * u + b) * u + t0) * u + p0;
}

function hermiteSlope(p0, p1, t0, t1, u) {
  const a = 2 * (p0 - p1) + t0 + t1;
  const b = 3 * (p1 - p0) - 2 * t0 - t1;
  return (3 * a * u + 2 * b) * u + t0;
}

// The first-person camera motions, as the game plays them: a few segments of
// cubic Hermite curves for the eye's offset (Vec and Slope, in the camera's
// frame, m) and the view's turn (Ore and RSlope, Euler x, y, z in rad), each
// segment lasting Time[i] seconds. The globals script sets up the ones for a
// dive to either side, lying down and getting up, and landing; the head bob
// is built in: a 0.8 s cycle lifting the eye 0.1 m twice (0.05 m prone),
// started whenever the soldier moves at 1 m/s or more on his feet with no
// other motion playing, and settled gently when he stops. A motion starts
// from wherever the last one left the eye, at the same velocity.
export class FirstPersonMotion {
  constructor(tables = {}) {
    this.tables = tables;
    this.offset = new THREE.Vector3();
    this.turn = new THREE.Euler(0, 0, 0, "ZYX");
    this.keys = null;
    this.segment = 0;
    this.left = 0;
    this.value = [0, 0, 0, 0, 0, 0];
    this.slope = [0, 0, 0, 0, 0, 0];
    this.start = [0, 0, 0, 0, 0, 0];
    this.startSlope = [0, 0, 0, 0, 0, 0];
    this.bobbing = false;
    this.bobWanted = false;
    this.bobScale = 1;
    this.airTime = 0;
    this.impactSpeed = 0;
    this.was = null;
    this.onCue = null; // (seconds) for motions that lower and raise the arms
  }

  // keys: { Vec, Slope, Ore, RSlope, Time }.
  play(keys, bob = false) {
    this.keys = keys;
    this.bobbing = bob;
    this.segment = 0;
    this.left = Math.max(keys.Time[0], 1e-4);
    this.start = [...this.value];
    // Keep velocity continuous even when the new segment has another length.
    this.startSlope = this.slope.map((speed) => speed * this.left);
  }

  reset() {
    this.keys = null;
    this.bobbing = false;
    this.bobWanted = false;
    this.value.fill(0);
    this.slope.fill(0);
    this.offset.set(0, 0, 0);
    this.turn.set(0, 0, 0);
  }

  bob(scale) {
    this.bobScale = scale;
    const vec = BOB_VEC.map(([x, y, z]) => [x * scale, y * scale, z * scale]);
    const slope = BOB_SLOPE.map((v) => v.map((x) => x * scale));
    this.play({ Vec: vec, Slope: slope, Ore: [ZERO, ZERO, ZERO, ZERO], RSlope: [ZERO, ZERO, ZERO, ZERO], Time: [BOB_TIME, BOB_TIME, BOB_TIME, BOB_TIME] }, true);
  }

  settle() {
    this.play({ Vec: [ZERO], Slope: [ZERO], Ore: [ZERO], RSlope: [ZERO], Time: [RETURN_TIME] });
  }

  // The named motion from the globals script, if it has one.
  trigger(name, scale = 1) {
    let keys = this.tables[name];
    if (!keys) return;
    if (scale !== 1) keys = { ...keys, Vec: keys.Vec.map((v) => v.map((x) => x * scale)), Slope: keys.Slope.map((v) => v.map((x) => x * scale)) };
    this.play(keys);
    if (ARMS_CUE.has(name)) this.onCue?.(keys.Time.reduce((sum, t) => sum + t, 0));
  }

  // active: the camera is the soldier's own eyes. The soldier's changes of
  // state are followed either way, so none is replayed on switching views.
  update(dt, soldier, active) {
    const now = { posture: soldier.posture, air: soldier.air, dive: soldier.dive };
    const was = this.was;
    this.was = now;
    if (now.air) {
      this.airTime += dt;
      this.impactSpeed = Math.max(this.impactSpeed, -soldier.velocity.y);
    }
    if (!active) {
      this.reset();
      return;
    }
    if (was) {
      if (now.dive && !was.dive) this.trigger(now.dive < 0 ? "RollLeft" : "RollRight");
      else if (was.air && !now.air && (was.air === "jump" || this.impactSpeed >= 3 || this.airTime > 0.35)) {
        // The retail motion drops the eye a full meter, even after a shallow
        // step. Retain a modest impact response for actual jumps and falls.
        this.trigger("Land", THREE.MathUtils.clamp(this.impactSpeed * 0.04, 0.12, 0.35));
      }
      else if (!now.dive && !was.dive && !now.air && !was.air && was.posture !== now.posture) {
        const name = POSTURE_MOTIONS[`${was.posture}>${now.posture}`];
        if (name) this.trigger(name);
      }
    }
    if (!now.air) this.airTime = this.impactSpeed = 0;
    const v2 = soldier.velocity.x ** 2 + soldier.velocity.z ** 2;
    this.bobWanted = !soldier.air && !soldier.dive && v2 >= (this.bobbing ? BOB_STOP : BOB_START);
    this.bobScale = soldier.posture === "prone" ? BOB_PRONE : 1;
    if (this.bobbing && !this.bobWanted) this.settle();
    if (!this.keys && this.bobWanted) this.bob(this.bobScale);
    this.step(dt);
  }

  step(dt) {
    let remaining = Math.max(dt, 0);
    while (this.keys) {
      const keys = this.keys;
      const i = this.segment;
      const duration = Math.max(keys.Time[i], 1e-4);
      const consumed = Math.min(remaining, this.left);
      this.left -= consumed;
      remaining -= consumed;
      const u = THREE.MathUtils.clamp(1 - this.left / duration, 0, 1);
      for (let c = 0; c < 6; c++) {
        const [vec, slope] = c < 3 ? [keys.Vec, keys.Slope] : [keys.Ore, keys.RSlope];
        const k = c % 3;
        const p0 = i > 0 ? vec[i - 1][k] : this.start[c];
        const t0 = i > 0 ? slope[i - 1][k] * duration / Math.max(keys.Time[i - 1], 1e-4) : this.startSlope[c];
        this.value[c] = hermite(p0, vec[i][k], t0, slope[i][k], u);
        this.slope[c] = hermiteSlope(p0, vec[i][k], t0, slope[i][k], u) / duration;
      }
      if (this.left > 1e-10) break;
      if (i < keys.Time.length - 1) {
        this.segment++;
        this.left = Math.max(keys.Time[this.segment], 1e-4);
      } else {
        this.keys = null;
        this.bobbing = false;
        if (this.value.some((v) => Math.abs(v) > 1e-4) || this.slope.some((v) => Math.abs(v) > 1e-4)) this.settle();
        else if (this.bobWanted) this.bob(this.bobScale);
      }
      // Carry leftover frame time into the next segment/motion/bob cycle.
      if (remaining <= 1e-10) break;
    }
    this.offset.fromArray(this.value, 0);
    this.turn.set(this.value[3], this.value[4], this.value[5], "ZYX");
  }
}

export class SoldierCamera {
  constructor(camera, world, def, { responsive = false } = {}) {
    this.camera = camera;
    this.world = world;
    this.sections = def.cameras || {};
    this.responsive = responsive;
    this.followCenter = new THREE.Vector3();
    this.followOffset = new THREE.Vector3();
    this.hfov = THREE.MathUtils.degToRad(def.ThirdPersonFOV ?? 65);
    this.firstFov = THREE.MathUtils.degToRad(def.FirstPersonFOV ?? 70);
    this.eye = new THREE.Vector3();
    this.fov = this.hfov;
    camera.rotation.order = "YXZ";
    this.center = new THREE.Vector3();
    this.offset = new THREE.Vector3();
    this.tilt = 0;
    this.pivot = new THREE.Vector3();
    this.target = new THREE.Vector3();
    this.boom = new THREE.Vector3();
    this.local = new THREE.Vector3();
    this.right = new THREE.Vector3();
    this.up = new THREE.Vector3();
    this.dir = new THREE.Vector3();
    this.want = new THREE.Quaternion();
    this.inverse = new THREE.Quaternion();
    this.euler = new THREE.Euler(0, 0, 0, "YXZ");
    this.hit = makeHit();
    this.motion = new FirstPersonMotion(def.cameraMotions);
    this.reset();
  }

  reset() {
    this.fresh = true;
    this.reach = 1;
    this.fov = this.hfov;
  }

  // Another class's camera sections and fields of view (the player's next
  // body); the first-person motions stay.
  setDef(def) {
    this.sections = def.cameras || {};
    this.hfov = THREE.MathUtils.degToRad(def.ThirdPersonFOV ?? 65);
    this.firstFov = THREE.MathUtils.degToRad(def.FirstPersonFOV ?? 70);
    this.reset();
  }

  read(posture, zoomed) {
    const name = SECTIONS[posture] || "STAND";
    const s = (zoomed && this.sections[`${name}ZOOM`]) || this.sections[name] || this.sections.STAND || {};
    this.eye.fromArray(s.EyePointOffset || s.TrackCenter || [0, 1.8, 0]);
    this.center.fromArray(s.TrackCenter || s.EyePointOffset || [0, 1.8, 0]);
    this.offset.fromArray(s.TrackOffset || [0, 0, 3.5]);
    this.tilt = THREE.MathUtils.degToRad(s.TiltValue ?? 5);
  }

  // The renderer's vertical FOV for a horizontal one at this window shape.
  applyFov(aspect) {
    this.camera.fov = THREE.MathUtils.radToDeg(2 * Math.atan(Math.tan(this.fov / 2) / aspect));
    this.camera.updateProjectionMatrix();
  }

  // The field of view eases toward FirstPersonFOV (first person or zoomed) or
  // ThirdPersonFOV, narrowed by the zoom.
  updateFov(dt, zoom, firstPerson) {
    const base = firstPerson || zoom > 1 ? this.firstFov : this.hfov;
    const want = 2 * Math.atan(Math.tan(base / 2) / zoom);
    this.fov += (want - this.fov) * (1 - Math.exp(-FOV_RATE * dt));
    this.applyFov(this.camera.aspect);
  }

  // eye: the first-person eye, if the caller has one.
  update(dt, feet, yaw, pitch, posture, zoom = 1, firstPerson = false, eye = null) {
    const cam = this.camera;
    this.read(posture, zoom > 1);
    this.updateFov(dt, zoom, firstPerson);
    if (firstPerson) {
      this.updateFirstPerson(feet, yaw, pitch, eye);
      return;
    }

    if (this.responsive) {
      this.updateResponsive(dt, feet, yaw, pitch, posture);
      return;
    }

    // Pivot: TrackCenter in the soldier's frame (position and facing).
    const sin = Math.sin(yaw);
    const cos = Math.cos(yaw);
    this.pivot.set(
      feet.x + sin * this.center.z - cos * this.center.x,
      feet.y + this.center.y,
      feet.z + cos * this.center.z + sin * this.center.x,
    );

    // Look along the aim, tipped down by the tilt; ease toward it.
    this.euler.set(pitch - this.tilt, yaw + Math.PI, 0);
    this.want.setFromEuler(this.euler);
    if (this.fresh || cam.position.distanceTo(this.pivot) > SNAP) {
      cam.quaternion.copy(this.want);
    } else {
      cam.quaternion.slerp(this.want, 1 - Math.exp(-ROTATE_RATE * dt));
    }

    // Wanted position: the pivot plus TrackOffset in the camera's frame.
    this.boom.copy(this.offset).applyQuaternion(cam.quaternion);
    this.target.copy(this.pivot).add(this.boom);
    const reach = this.clearance(cam.quaternion);
    if (reach < this.reach) this.reach = reach;
    else this.reach += (reach - this.reach) * (1 - Math.exp(-RETURN_RATE * dt));
    this.target.copy(this.pivot).addScaledVector(this.boom, this.reach);

    if (this.fresh || cam.position.distanceTo(this.target) > SNAP) {
      cam.position.copy(this.target);
      this.fresh = false;
      return;
    }
    // Ease each camera-frame axis of the move separately.
    const k = 1 - Math.exp(-MOVE_RATE * (posture === "prone" ? PRONE_MOVE : 1) * dt);
    this.inverse.copy(cam.quaternion).invert();
    this.local.subVectors(this.target, cam.position).applyQuaternion(this.inverse).multiplyScalar(k);
    cam.position.add(this.local.applyQuaternion(cam.quaternion));
    // Easing from last frame can leave the actual view behind a new wall even
    // though the desired boom is clear. Resolve the position we will render.
    this.boom.subVectors(cam.position, this.pivot);
    const clear = this.clearance(cam.quaternion);
    if (clear < 1) cam.position.copy(this.pivot).addScaledVector(this.boom, clear);
  }

  // The player's aim/orbit and interpolated feet already have their own
  // timing. Filtering those again adds mouse lag and makes the character
  // slide across the screen on every start/stop/turn. Ease only changes to
  // the stance's local rig and the boom's release from an obstruction.
  updateResponsive(dt, feet, yaw, pitch, posture) {
    const cam = this.camera;
    const fresh = this.fresh || cam.position.distanceTo(feet) > SNAP;
    if (fresh) {
      this.followCenter.copy(this.center);
      this.followOffset.copy(this.offset);
      this.reach = 1;
    } else {
      const blend = 1 - Math.exp(-MOVE_RATE * (posture === 'prone' ? PRONE_MOVE : 1) * dt);
      this.followCenter.lerp(this.center, blend);
      this.followOffset.lerp(this.offset, blend);
    }
    const sin = Math.sin(yaw), cos = Math.cos(yaw), center = this.followCenter;
    this.pivot.set(feet.x + sin * center.z - cos * center.x, feet.y + center.y,
      feet.z + cos * center.z + sin * center.x);
    this.euler.set(pitch - this.tilt, yaw + Math.PI, 0);
    cam.quaternion.setFromEuler(this.euler);
    this.boom.copy(this.followOffset).applyQuaternion(cam.quaternion);
    const reach = this.clearance(cam.quaternion);
    if (fresh || reach < this.reach) this.reach = reach;
    else this.reach += (reach - this.reach) * (1 - Math.exp(-RETURN_RATE * dt));
    cam.position.copy(this.pivot).addScaledVector(this.boom, this.reach);
    this.fresh = false;
  }

  // At the eye, looking along the aim; the chase camera starts afresh after.
  updateFirstPerson(feet, yaw, pitch, eye) {
    const cam = this.camera;
    this.euler.set(pitch, yaw + Math.PI, 0);
    cam.quaternion.setFromEuler(this.euler);
    if (eye) {
      cam.position.copy(eye);
    } else {
      const sin = Math.sin(yaw);
      const cos = Math.cos(yaw);
      cam.position.set(
        feet.x + sin * this.eye.z - cos * this.eye.x,
        feet.y + this.eye.y,
        feet.z + cos * this.eye.z + sin * this.eye.x,
      );
    }
    // The camera motion: the eye moved in the view's frame, then the view
    // turned in its own.
    cam.position.add(this.local.copy(this.motion.offset).applyQuaternion(cam.quaternion));
    cam.quaternion.multiply(this.want.setFromEuler(this.motion.turn));
    this.fresh = true;
  }

  // Fraction of the boom that is clear: the nearest hit of the center ray and
  // four rays to the camera's corners. The engine runs the corner rays
  // parallel to the boom; here they fan out from the pivot, so a ray never
  // starts inside a wall the soldier is pressed against.
  clearance(q) {
    if (this.boom.lengthSq() < 1e-8) return 1;
    this.right.set(CORNER, 0, 0).applyQuaternion(q);
    this.up.set(0, CORNER, 0).applyQuaternion(q);
    let reach = 1;
    for (const [a, b] of [[0, 0], [1, 1], [1, -1], [-1, 1], [-1, -1]]) {
      this.dir.copy(this.boom).addScaledVector(this.right, a).addScaledVector(this.up, b);
      const far = this.dir.length();
      this.dir.divideScalar(far);
      // Include the safety skin in the query. Testing only to the endpoint
      // then subtracting skin after a hit makes the camera jump by 8 cm each
      // time a pitched corner crosses a floor/wall (and repeat on easing out).
      const hit = this.world.cast(this.pivot, this.dir, far + CAMERA_SKIN, this.hit);
      if (hit) reach = Math.min(reach, Math.max(0, hit.distance - CAMERA_SKIN) / far);
    }
    return reach;
  }
}

// The death camera, following the 2004 engine's. From the soldier's death
// until he has a body again it takes over from whatever view he had: the
// chase camera, his own eyes, a vehicle's, a turret's or a recon droid's.
//
// Each step (each frame in the original, each 60 Hz world tick here) it works
// out a pose that looks from a near point toward a far one, then moves a
// fraction t of the way there and turns the same fraction, so it eases in:
// - At first the near point is 1 m from the body toward where the camera
//   looked from and 1.5 m above the feet, and the far point is the body's
//   middle, 0.8 m up. So it rises and backs off to look down on the body at
//   about 35 degrees, keeping its heading. t grows from 0 to 1 over the
//   first second, then stays 1.
// - After 1.5 s, if someone else killed him, the near point is the body's
//   middle and the far point the killer's eyes (his vehicle's or turret's
//   while he sits in one). It looks over the body at the killer and follows
//   him. t starts again from 0 at 1.5 s and reaches 1 at 3.5 s. While the
//   killer has no body (he died too) it goes on looking down on the body.
// It sits 5 m behind the near point, on the line from the far one, or 1 m
// short of what two rays find: they run 6 m back from 0.75 m above and below
// the near point and, like the original's camera rays, meet only terrain,
// buildings and props, not vehicles or soldiers. The world's up stays up.
// The body's middle follows the body while it is in the world; the first
// near point stays where it started. The field of view stays as the chase
// camera left it (see deadFov), unzoomed, and it doesn't shake.
//
// This one is the world's camera (the one the Classic AI schedules from);
// what the page draws is src/death-shot.js, which reads who the killer is
// from here (blame) and draws these rules smoothly as its Classic style.
export const DEATH_SETTLE = 1; // s for the first fraction to reach 1
export const DEATH_TURN = 1.5; // s before it turns to the killer
export const DEATH_TURN_RATE = 0.5; // 1/s: then the fraction reaches 1 in 2 s
export const DEATH_BOOM = 6; // m of ray back from the near point
export const DEATH_GAP = 1; // m kept short of what the rays hit
export const DEATH_RAY_SPREAD = 0.75; // m above and below the near point
export const DEATH_BODY = 0.8; // m: the body's middle above the feet
export const DEATH_RISE = 1.5; // m: the first near point above the feet
export const DEATH_BACK = 1; // m: and from the body toward the camera
const WORLD_UP = new THREE.Vector3(0, 1, 0);
// What a dead soldier reports to the chase camera as its field of view: the
// camera's default, 48 degrees.
const DEAD_FOV = 0.837758; // rad

// The field of view the death camera keeps, from the chase camera's (hfov):
// the fatal hit marks the soldier dead at once, but he is only killed, and
// the death camera started, on his next update. Meanwhile the chase camera
// has one more step of dt with him dead, easing toward DEAD_FOV at 25/s.
export function deadFov(hfov, dt) {
  return DEAD_FOV + (hfov - DEAD_FOV) * Math.exp(-FOV_RATE * dt);
}

export class DeathCamera {
  constructor(world) {
    this.world = world;
    this.active = false;
    this.time = 0; // s since the death
    this.killed = false; // someone else killed him
    this.hfov = THREE.MathUtils.degToRad(65);
    this.position = new THREE.Vector3();
    this.quaternion = new THREE.Quaternion();
    this.previousPosition = new THREE.Vector3();
    this.previousQuaternion = new THREE.Quaternion();
    this.body = new THREE.Vector3(); // the body's middle
    this.anchor = new THREE.Vector3(); // the first near point
    this.dir = new THREE.Vector3();
    this.back = new THREE.Vector3();
    this.origin = new THREE.Vector3();
    this.target = new THREE.Vector3();
    this.ahead = new THREE.Vector3();
    this.basis = new THREE.Matrix4();
    this.want = new THREE.Quaternion();
    this.hit = makeHit();
    // Who killed him, as plain numbers, so a world worker's copy reaches
    // its page with the rest of this record: his unit's id and life, his
    // Character's slot (whose next body the original follows), or a hull
    // credited by itself and its life; -1 for none. deaths counts the
    // starts, so a page knows a new death from the one it shows.
    this.killerUnit = -1;
    this.killerLife = 0;
    this.killerCharacter = -1;
    this.killerHull = -1;
    this.killerHullLife = 0;
    this.deaths = 0;
  }

  // from: the camera it takes over, as it is now; feet: where the body lies;
  // killed: whether someone else killed him; hfov: the field of view across,
  // unzoomed.
  start(from, feet, killed, hfov) {
    this.active = true;
    this.time = 0;
    this.killed = killed;
    this.hfov = hfov;
    this.deaths += 1;
    this.blame(null);
    this.position.copy(from.position);
    this.quaternion.copy(from.quaternion);
    this.previousPosition.copy(this.position);
    this.previousQuaternion.copy(this.quaternion);
    this.body.set(feet.x, feet.y + DEATH_BODY, feet.z);
    // The camera's back, level: from the body toward where it looked from.
    const back = this.back.set(0, 0, 1).applyQuaternion(from.quaternion).setY(0);
    if (back.lengthSq() < 1e-8) back.set(0, 0, 1);
    back.normalize();
    this.anchor.set(feet.x + back.x * DEATH_BACK, feet.y + DEATH_RISE, feet.z + back.z * DEATH_BACK);
  }

  stop() {
    this.active = false;
  }

  // The killer, as the world names him (game.js killerOf): { unit,
  // generation, character } for a soldier (a unit, or a launch's owner
  // record), { vehicle, life } for a hull credited by itself, or null.
  // characters and vehicles: the battle's, for their slots.
  blame(killer, characters = [], vehicles = []) {
    this.killerUnit = this.killerCharacter = this.killerHull = -1;
    this.killerLife = this.killerHullLife = 0;
    if (!killer) return;
    if (killer.vehicle) {
      this.killerHull = vehicles.indexOf(killer.vehicle);
      this.killerHullLife = killer.life ?? 0;
      return;
    }
    this.killerUnit = Number.isInteger(killer.unit?.id) ? killer.unit.id : -1;
    this.killerLife = killer.generation ?? 0;
    this.killerCharacter = killer.character ? characters.indexOf(killer.character) : -1;
  }

  // One step of dt. feet: where the body lies, or null once it has left the
  // world; killer: the killer's eyes, or null while he has no body.
  update(dt, feet, killer) {
    this.previousPosition.copy(this.position);
    this.previousQuaternion.copy(this.quaternion);
    this.time += dt;
    if (feet) this.body.set(feet.x, feet.y + DEATH_BODY, feet.z);
    let near = this.anchor;
    let far = this.body;
    let t = 1;
    if (this.time < DEATH_SETTLE) t = Math.min(this.time / DEATH_SETTLE, 1);
    else if (this.time > DEATH_TURN && this.killed) {
      t = Math.min((this.time - DEATH_TURN) * DEATH_TURN_RATE, 1);
      if (killer) {
        near = this.body;
        far = killer;
      }
    }
    const dir = this.dir.subVectors(far, near);
    // With nothing to look along (the killer's eyes on the body), it keeps
    // looking the way it does.
    if (dir.lengthSq() < 1e-10) dir.set(0, 0, -1).applyQuaternion(this.quaternion);
    dir.normalize();
    // How far back it can sit: the nearer of the two rays' hits.
    this.back.copy(dir).negate();
    let clear = 1;
    for (const lift of [DEATH_RAY_SPREAD, -DEATH_RAY_SPREAD]) {
      this.origin.set(near.x, near.y + lift, near.z);
      const hit = this.world.cast(this.origin, this.back, DEATH_BOOM, this.hit, null, true);
      if (hit) clear = Math.min(clear, hit.distance / DEATH_BOOM);
    }
    this.target.copy(near).addScaledVector(dir, DEATH_GAP - DEATH_BOOM * clear);
    this.basis.lookAt(this.target, this.ahead.addVectors(this.target, dir), WORLD_UP);
    this.want.setFromRotationMatrix(this.basis);
    this.position.lerp(this.target, t);
    this.quaternion.slerp(this.want, t);
  }

  // Into a camera: the pose between the last two steps (alpha 0 at the one
  // before, 1 at the last), and the field of view for the camera's shape.
  apply(camera, alpha = 1) {
    camera.position.lerpVectors(this.previousPosition, this.position, alpha);
    camera.quaternion.slerpQuaternions(this.previousQuaternion, this.quaternion, alpha);
    const fov = THREE.MathUtils.radToDeg(2 * Math.atan(Math.tan(this.hfov / 2) / (camera.aspect || 1)));
    if (camera.fov !== fov) {
      camera.fov = fov;
      camera.updateProjectionMatrix();
    }
  }
}
