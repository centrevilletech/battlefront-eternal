import { poseFlightGear, poseGearAt, gearFoot } from './flight/gear.js';
import { updateFlightTarget, assistShot } from './flight/targeting.js';
import { abandonLift, initFlight, stepFlight, standHeight } from './flight/model.js';
import { groundMotion } from './vehicle-motion.js';
import { HOVER_REST, hoverContacts, hoverPhysics, hoverSprings, hullQuaternion, restHeight } from './vehicle-hover.js';
import { updateFlightCamera, easeAxes, cameraTension } from './flight/camera.js';
import { updateWeaponLock, hasMissileLock, vehicleMissileDefinition, seatLockTarget } from "./weapon-targeting.js";
import { waterDamageRule, waterDamageStep } from './vehicle-water.js';
import * as THREE from "three";
import { descendantMuzzles, vehicleMuzzleTips, bindMountedRound } from "./muzzles.js";
import { makeHit } from "./collision.js";
import { vehicleBatches, attachVehicleBatches } from "./vehicle-rig.js";
import { modelPack } from "./models.js";
import { emitSound } from "./audio/events.js";
import { loadClips, Pose } from "./anim.js";
import { Weapon } from "./weapons.js";
import { givePowerup } from "./items.js";
import { blastBlocked, blastDamage, blastDistance, healthScale } from "./ordnance.js";
import { SeatLease, SeatUnit, VehicleTarget, seatRole, seatWeaponPolicy, classicSeatExit, placeOnFoot, dropBody, seatBody, unseatBody } from "./seats.js";
import { boardingSeat, canBoard, claimSeat, inFlight, mayLeave, personLease, seatControl, seatLook, switchSeat, teamRelation } from "./boarding.js";
import { makeRiderPoints, riderDropPoint, riderHitShapes, riderPoints } from "./rider-pose.js";
import { stepPilotless } from "./flight/pilotless.js";

const UP = new THREE.Vector3(0, 1, 0);
const RIGHT = new THREE.Vector3(1, 0, 0);
const NOSE = new THREE.Vector3(0, 0, 1);
const DOWN = new THREE.Vector3(0, -1, 0);
const UNIT_SCALE = new THREE.Vector3(1, 1, 1);
const DEG = Math.PI / 180;
const FIXED_STEP = 1 / 120;
const DEATH_BLEND = 0.3; // s at most a dying walker takes from its last pose into its death clip (the port's)
const num = (v, fallback = 0) => Number.isFinite(parseFloat(v)) ? parseFloat(v) : fallback;
const clockSeconds = () => (globalThis.performance?.now?.() ?? Date.now()) / 1000;
const vector = (v, fallback) => v ? String(v).split(/\s+/).map(Number) : fallback;
const angle = (v) => Math.atan2(Math.sin(v), Math.cos(v));
const approach = (v, target, delta) => v + THREE.MathUtils.clamp(target - v, -delta, delta);
const idle = Object.freeze({ forward: 0, right: 0, turn: 0, brake: false });
const noTrigger = Object.freeze({ held: false, pressed: false });
const ZERO = Object.freeze(new THREE.Vector3());
const vec3 = (v, fallback = null) => {
  const a = v === undefined || v === null ? null : String(v).trim().split(/\s+/).map(Number);
  return a && a.length >= 3 && a.slice(0, 3).every(Number.isFinite) ? new THREE.Vector3(a[0], a[1], a[2]) : fallback && fallback.clone();
};
const degrees = (v) => { const a = vector(v, null); return a && a.length >= 2 && Number.isFinite(a[0]) && Number.isFinite(a[1]) ? [a[0] * DEG, a[1] * DEG] : null; };

// Seats, following the 2004 game (docs/formats.md "Vehicle seats").
// Every controllable's FirstPersonFOV and ThirdPersonFOV default to 48
// degrees across. The chase camera's section defaults ease its turn and each
// of its axes at 30/s (AimTension, MoveTension), and start its wall rays
// 1.5 m up in its subject's frame.
const SEAT_FOV = 48;
const CAMERA_RATE = 30; // 1/s
const RAY_ORIGIN = Object.freeze(new THREE.Vector3(0, 1.5, 0));
const CAMERA_SNAP = 80; // m: farther than this from where it should be, the camera jumps
const CORNER = .4; // m: offset of the four extra wall rays
const RAYS = Object.freeze([0, 0, 1, 1, 1, -1, -1, 1, -1, -1]); // the centre ray and the four corners (right, up)
const CAMERA_SKIN = .08; // m: keeps the near plane off what the rays hit
const RETURN_RATE = 6; // 1/s: a camera pulled in by a wall eases back out
const FOV_RATE = 25; // 1/s
const AIM_REACH = 512; // m along a seat's aim to the point its guns turn to
const AIMED = DEG; // rad: a barrel this close to its target fires at it exactly
const TURRET_PITCH = 1.5; // rad: the port's bound on a turret's pitch when its section sets none

const CHASE_CAMERA = Object.freeze({
  hover: { fov: 85, speedFov: 0, back: 1.7, minBack: 8, up: 1.1, minUp: 2.6, lag: 9, turn: 8, lead: .22, nose: 0, steer: 0, tilt: 7, roll: .2, firstFov: 78 },
  byName: { speederbike: { fov: 82, back: 1.9, minBack: 7, up: 1.6, minUp: 2.2, lag: 10, turn: 9, firstFov: 80 } },
});
const CHASE_SNAP = 80;
const TOW_POINTS = 32; // the cable line's points: the anchor, up to 12 wraps, 16 along the sagging span to the gun

// A seat as its control section describes it: the camera (EyePointOffset,
// TrackCenter, TrackOffset, TiltValue, how fast it turns and moves, and the
// two fields of view), the aim's limits, the turret node, and the rider. Seat 0 is the hull's own control,
// whose pitch is up positive; every other seat is a turret, whose pitch
// (PitchLimits) is down positive and whose yaw (YawLimits) is measured from
// the hull's heading, positive to the left. Limits are kept here in the
// port's terms: yaw from the hull, pitch up positive, and null for none.
function seatView(vehicle, seat, index) {
  const s = seat.settings || {};
  const eye = vec3(s.EyePointOffset, ZERO);
  const view = {
    eye,
    // EyePointOffset sets TrackCenter too; a TrackCenter of its own wins.
    center: vec3(s.TrackCenter) || eye.clone(),
    offset: vec3(s.TrackOffset, ZERO),
    tilt: num(s.TiltValue, 0) * DEG,
    ...cameraTension(s, CAMERA_RATE),
    firstFov: num(s.FirstPersonFOV, SEAT_FOV) * DEG,
    thirdFov: num(s.ThirdPersonFOV, SEAT_FOV) * DEG,
    turret: null, pivot: new THREE.Vector3(), up: new THREE.Vector3(0, 1, 0),
    yawFactor: num(s.YawTurnFactor, 1), pitchFactor: num(s.PitchTurnFactor, 1),
    turnRate: num(s.TurnRate, index > 0 ? 2 : vehicle.turnRate),
    yawLimits: null, wrapYaw: false, pitchLimits: null, rider: null,
  };
  if (index === 0) {
    // The hover class's default pitch limits are half a radian each way.
    view.pitchLimits = vehicle.flying ? [-Math.PI, Math.PI] : degrees(s.PitchLimits) || [-.5, .5];
  } else {
    const pitch = degrees(s.PitchLimits);
    view.pitchLimits = pitch && pitch[0] > -Math.PI && pitch[1] < Math.PI ? [-pitch[1], -pitch[0]] : [-TURRET_PITCH, TURRET_PITCH];
    const yaw = degrees(s.YawLimits);
    // A range from the left round past the back (0..pi to pi..2pi) is kept
    // in 0..2pi; any other one inside +-pi clamps; the rest turns freely.
    view.wrapYaw = Boolean(yaw && yaw[0] > 0 && yaw[0] <= Math.PI && yaw[1] >= Math.PI && yaw[1] < 2 * Math.PI);
    if (yaw && (view.wrapYaw || yaw[0] > -Math.PI + 1e-4 && yaw[1] < Math.PI - 1e-4)) view.yawLimits = yaw;
    // The turret node turns about its pivot, keeping its own up. A section
    // without a node can name the pivot itself, in the hull's frame (the
    // unnamed key f2aa4a90: the AT-ST gunner's is 8 m up, at the head; the
    // cloud car gunner's 15 m behind); else it is the hull's origin.
    const node = vehicle.byName[String(s.TurretNodeName || '').toLowerCase()] || null;
    if (node) {
      view.turret = node;
      node.getWorldPosition(view.pivot);
      view.up.set(0, 1, 0).applyQuaternion(node.getWorldQuaternion(new THREE.Quaternion()));
    } else view.pivot.copy(vec3(s.f2aa4a90, ZERO));
  }
  // The rider shows wherever the seat has a pilot animation (Pilot9Pose or
  // PilotAnimation); enclosed seats have none. It sits at PilotPosition, or
  // at the node the creature classes name instead (hash 11ce2025). Rounds
  // meet a shown rider unless the section sets hash 2b60e217 to 0 (the
  // original game's pilot pose then leaves him out of collision; the
  // snowspeeder, cloud car, gunship ball turrets and fighters do):
  // shootable.
  const anim = s.Pilot9Pose || s.PilotAnimation;
  if (anim) {
    const creature = vehicle.healthType === 'animal';
    const name = s.PilotPosition || (creature && vehicle.byName.hp_active ? 'hp_active' : s['11ce2025']) || '';
    const clip = String(anim).toLowerCase();
    view.rider = { anim: clip, clip: `human_${clip}`, ninePose: Boolean(s.Pilot9Pose), node: vehicle.byName[String(name).toLowerCase()] || null,
      shootable: num(s['2b60e217'], 1) !== 0 };
  }
  return view;
}

// A direction from a yaw (positive to the left, +X) and an up-positive pitch.
function aimVector(yaw, pitch, out) {
  return out.set(Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), Math.cos(yaw) * Math.cos(pitch));
}

// The rotation whose +Z is forward, keeping up as near its +Y as it can (the
// original game's look basis: right = up x forward, then up = forward x right).
const basisX = new THREE.Vector3(), basisY = new THREE.Vector3(), basisZ = new THREE.Vector3(), basisM = new THREE.Matrix4();
function lookBasis(forward, up, out) {
  basisZ.copy(forward).normalize();
  basisX.crossVectors(up, basisZ);
  // Straight along the up axis: any side will do.
  if (basisX.lengthSq() < 1e-10) basisX.set(0, 0, 1).cross(basisZ);
  if (basisX.lengthSq() < 1e-10) basisX.set(1, 0, 0);
  basisX.normalize();
  basisY.crossVectors(basisZ, basisX);
  return out.setFromRotationMatrix(basisM.makeBasis(basisX, basisY, basisZ));
}

// A yaw from the hull's heading in a seat's arc's own terms: 0..2pi for an
// arc round the back, else -pi..pi.
function arcYaw(view, relative) {
  const a = angle(relative);
  return view.wrapYaw && a < 0 ? a + 2 * Math.PI : a;
}

// A turret seat's 9-pose, as the original game picks it: the pitch spread over
// the seat's own limits (-1 at the top, 1 at the bottom), and the turn by
// where the yaw sits in an arc under 270 degrees, or else by how fast it
// turns against the seat's TurnRate. Written to out.
function ninePose(view, yaw, pitch, yawRate, out) {
  const low = view.pitchLimits[0], high = view.pitchLimits[1];
  out.pitch = high > low ? THREE.MathUtils.clamp(2 * ((high - pitch) / (high - low) - .5), -1, 1) : 0;
  if (view.yawLimits && view.yawLimits[1] - view.yawLimits[0] < 1.5 * Math.PI) {
    const a = view.yawLimits[0], b = view.yawLimits[1];
    const y = view.wrapYaw && yaw < 0 ? yaw + 2 * Math.PI : yaw;
    out.turn = b > a ? THREE.MathUtils.clamp(2 * ((y - a) / (b - a) - .5), -1, 1) : 0;
  } else out.turn = THREE.MathUtils.clamp(yawRate / Math.max(view.turnRate, 1e-3), -1, 1);
  return out;
}
const nine = { pitch: 0, turn: 0 };

// What a beam follows while it lasts: the barrel that fired it, its fire
// point's node (the hull without one), pointing where that barrel shoots
// now. Every barrel has its own, so a salvo's beams stay apart.
function beamSource(vehicle, index, node, aimer) {
  return {
    get matrix() { return node?.matrixWorld ?? vehicle.matrix; },
    get alive() { return vehicle.alive; },
    viewDirection: (out) => vehicle.beamDirection(index, node, aimer, out),
  };
}

// Separating axes for two yawed hull rectangles. Spawn pads can fit two
// narrow fighters side by side even when their bounding spheres overlap.
export function vehicleFootprintsOverlap(a, matrixA, b, matrixB, clearance = .3) {
  const ma = matrixA.elements, mb = matrixB.elements;
  const ax = ma[12] + ma[0] * a.center.x + ma[8] * a.center.z;
  const az = ma[14] + ma[2] * a.center.x + ma[10] * a.center.z;
  const bx = mb[12] + mb[0] * b.center.x + mb[8] * b.center.z;
  const bz = mb[14] + mb[2] * b.center.x + mb[10] * b.center.z;
  if (Math.abs(ma[13] + a.center.y - mb[13] - b.center.y) > (a.size.y + b.size.y) * .5 + 2) return false;
  for (const [x, z] of [[ma[0], ma[2]], [ma[8], ma[10]], [mb[0], mb[2]], [mb[8], mb[10]]]) {
    const ra = (Math.abs(x * ma[0] + z * ma[2]) * a.size.x + Math.abs(x * ma[8] + z * ma[10]) * a.size.z) * .5;
    const rb = (Math.abs(x * mb[0] + z * mb[2]) * b.size.x + Math.abs(x * mb[8] + z * mb[10]) * b.size.z) * .5;
    if (Math.abs((bx - ax) * x + (bz - az) * z) >= ra + rb + clearance) return false;
  }
  return true;
}

// The rider's clip, human_ + the seat's pilot animation. The extractor
// doesn't name four of the bank's rider clips yet (man_minigun, standing and
// the two snowspeeder ones); until it does, the nearest named one stands in,
// held on one frame: [clip, frame]. A missing "standing" leaves the body's
// own standing idle.
const RIDER_STANDIN = { man_minigun: ['human_minigun_9pose', 4], drivesnowspeeder: ['human_drive', 0], drivesnowspeedergunner: ['human_drive', 0] };

// A rider's pose on seat index: his clip and where in it (the 9-pose's
// pitch and turn, or a time), written to out { clip, pitch, turn, time };
// null with no clip. poseRider draws it, and his hit shapes are posed from
// the same (riderBody), so a round meets him where he is drawn.
function riderSample(v, index, clips, out) {
  const seat = v.seats[index];
  const rider = seat.view.rider;
  if (!rider) return null;
  let clip = clips?.get(rider.clip) || clips?.get(rider.anim) || null;
  const standin = !clip && RIDER_STANDIN[rider.anim];
  if (standin) clip = clips?.get(standin[0]) || null;
  if (!clip) return null;
  out.clip = clip; out.pitch = 0; out.turn = 0; out.time = null;
  if (standin) out.time = standin[1] / clip.fps;
  else if (!rider.ninePose) out.time = v.age;
  else if (index > 0) {
    const pose = ninePose(seat.view, angle(seat.aimYaw - v.yaw), seat.pitch, seat.yawRate || 0, nine);
    out.pitch = pose.pitch; out.turn = pose.turn;
  } else out.pitch = -seat.pitch / .7;
  return out;
}
const riderPick = { clip: null, pitch: 0, turn: 0, time: null };

// A body at its seat: at the seat's rider node, aimed as the seat aims, in
// the seat's pilot pose, shown only where the seat has a pilot animation
// (exposed riders; enclosed seats hide theirs). The player's body goes
// through this (VehicleDriver.sync) and so do bots'.
function poseRider(v, index, soldier, actor, animator, clips, presentation, point) {
  const seat = v.seats[index];
  const rider = seat.view.rider;
  const node = rider?.node;
  if (node) node.getWorldPosition(point);
  else point.copy(v.position).addScaledVector(UP, v.size.y * .5);
  soldier.position.copy(point); soldier.previous?.copy(point); soldier.settled?.copy(point);
  soldier.aimYaw = seat.aimYaw; soldier.aimPitch = seat.pitch;
  soldier.velocity.copy(v.velocity);
  presentation?.reset();
  actor.root.position.copy(point);
  actor.root.visible = Boolean(node);
  const pick = riderSample(v, index, clips, riderPick);
  if (!pick) { animator?.gunner?.(null); return; }
  if (pick.time !== null) animator?.gunner?.(pick.clip, 0, 0, pick.time);
  else animator?.gunner?.(pick.clip, pick.pitch, pick.turn);
}

// A rider's body on seat index as this tick poses it (src/rider-pose.js
// riderPoints, written to points), and the frame that carries it into the
// world (written to frame): the seat node's, or on a creature the hull's
// turn with his pelvis on the saddle node, as seatRider places them. Null
// where no rider shows or his skeleton lacks the joints. s: scratch
// { point, riderOffset, sample }.
function riderBody(v, index, unit, points, frame, s) {
  const node = v.seats[index]?.view.rider?.node;
  const animator = unit?.animator;
  if (!node || !animator) return null;
  const sample = riderSample(v, index, animator.clips, s.sample);
  if (!sample || !riderPoints(animator, sample, points)) return null;
  if (v.healthType !== 'animal') return frame.copy(node.matrixWorld);
  node.getWorldPosition(s.point);
  s.riderOffset.copy(points.pelvis).applyQuaternion(v.root.quaternion);
  return frame.compose(s.point.sub(s.riderOffset), v.root.quaternion, UNIT_SCALE);
}

// After the rider's animation is sampled: the body takes its node's place
// and turn, so it rides the hull's lean and the turret's swing. On a creature
// the posed pelvis goes on the saddle instead: riding clips already include
// a ground-to-saddle root offset, and placing that root at the node adds the
// height a second time. Returns the rider's new lean. s: scratch { point,
// riderOffset, riderQuaternion }.
function seatRider(v, index, actor, dt, lean, s) {
  const seat = v.seats[index]?.view.rider?.node;
  if (!seat || !actor.root.visible) return lean;
  if (v.healthType !== 'animal') {
    seat.getWorldPosition(actor.root.position);
    seat.getWorldQuaternion(actor.root.quaternion);
    actor.root.updateMatrixWorld(true);
    return lean;
  }
  const pelvis = actor.byName?.bone_pelvis;
  if (!pelvis) return lean;
  actor.root.quaternion.copy(v.root.quaternion);
  const amount = Math.min(1, v.velocity.length() / Math.max(1, v.speed));
  lean += (-v.yawRate * amount * .08 - lean) * (1 - Math.exp(-6 * dt));
  const breath = Math.sin(v.age * 2.2 + v.hoverPhase);
  // Small upper-body adjustments preserve the authored seat/leg pose.
  const chest = actor.byName.bone_ribcage;
  if (chest) {
    chest.quaternion.multiply(s.riderQuaternion.setFromAxisAngle(RIGHT, breath * .008 + amount * .018));
    chest.quaternion.multiply(s.riderQuaternion.setFromAxisAngle(UP, lean));
  }
  actor.root.updateMatrixWorld(true);
  pelvis.getWorldPosition(s.riderOffset).sub(actor.root.position);
  seat.getWorldPosition(s.point);
  actor.root.position.copy(s.point).sub(s.riderOffset);
  actor.root.updateMatrixWorld(true);
  return lean;
}

// A person's exit search (the player's, a network human's): outward from
// the hull, a floor, room for the body, and a clear line from the vehicle.
// A craft in flight needs no floor under the spot: he bails out beside it,
// at its height (the first spot tried is off its side). Writes the spot
// (feet) to out and returns whether one was found. s: scratch { hit,
// target, direction }.
function safeExitPoint(collision, v, out, s) {
  const distance = v.radius + 1.6, airborne = inFlight(v);
  for (let i = 0; i < 12; i++) {
    const a = v.yaw + Math.PI / 2 + i * Math.PI / 6;
    out.set(v.position.x + Math.sin(a) * distance, v.position.y + 3, v.position.z + Math.cos(a) * distance);
    const floor = collision.cast(out, DOWN, 10, s.hit, v.body);
    if (!floor && !airborne || floor && floor.normal.y < .65) continue;
    out.y = floor ? floor.point.y + 1 : v.position.y + 1;
    if (collision.overlapsSphere(out, .65, v.body)) continue;
    s.target.copy(v.position).addScaledVector(UP, 1.5);
    s.direction.subVectors(out, s.target);
    const span = s.direction.length();
    if (collision.cast(s.target, s.direction.divideScalar(span), span, s.hit, v.body)) continue;
    out.y -= 1;
    return true;
  }
  return false;
}

// A person's way out: the search above, or, in flight or forced, above the
// hull when it finds nothing. False: boxed in on the ground, he stays aboard.
function personExitPoint(collision, v, out, s, anywhere) {
  if (safeExitPoint(collision, v, out, s)) return true;
  if (!anywhere) return false;
  out.copy(v.position).addScaledVector(UP, v.size.y + 1);
  return true;
}

// Immutable geometry, materials and collision BVHs are shared by class. A
// vehicle owns only its transform, rig nodes, weapon state and health. Parked
// vehicles sleep; simulation is bounded at 120 Hz with interpolated rendering.
export class Vehicle {
  constructor(system, slot, name) {
    this.system = system;
    this.slot = slot;
    this.name = name;
    this.def = system.data.classes[name];
    this.desc = this.def;
    const s = this.def.settings;
    this.base = this.def.base || 'hover';
    this.flying = this.base === 'flyer';
    this.walking = this.base === 'walker' || this.base === 'commandwalker';
    const template = system.templates.get(this.def.model);
    this.root = new THREE.Group();
    this.root.name = name;
    this.bones = template.rig.bones.map((d) => {
      const bone = new THREE.Bone();
      bone.name = d.name;
      new THREE.Matrix4().fromArray(d.local).decompose(bone.position, bone.quaternion, bone.scale);
      return bone;
    });
    template.rig.bones.forEach((d, i) => (d.parent >= 0 ? this.bones[d.parent] : this.root).add(this.bones[i]));
    this.byName = Object.fromEntries(template.rig.bones.map((d, i) => [d.name.toLowerCase(), this.bones[i]]));
    this.bind = new Map(this.bones.map((b) => [b, b.quaternion.clone()]));
    this.bindQuaternions = this.bones.map((b) => this.bind.get(b));
    this.bindPositions = this.bones.map(b => b.position.clone());
    this.clips = template.clips;
    // A walker's clips pose it (the walk cycle, its death); a flyer's takeoff
    // clip only its gear (src/flight/gear.js).
    this.pose = this.clips?.size && !this.flying ? new Pose(this.bones.length, template.rig.bones.findIndex(b => b.name.toLowerCase() === 'dummyroot')) : null;
    // How far below its origin a craft with gear stands on it (src/flight/
    // gear.js gearFoot; null without a takeoff clip).
    this.gearFoot = this.flying ? template.gearFoot ?? null : null;
    this.walkPhase = 0;
    this.walkBlend = 0;
    this.skeleton = attachVehicleBatches(this.root, this.bones, template.batches, system.caster);
    this.position = new THREE.Vector3();
    this.previous = new THREE.Vector3();
    this.visual = new THREE.Vector3();
    this.velocity = new THREE.Vector3();
    this.quaternion = new THREE.Quaternion();
    this.rotation = this.flying ? null : new THREE.Euler(0, 0, 0, "YXZ");
    this.matrix = new THREE.Matrix4();
    this.body = system.collision.addMovingBody(template.shape, this.matrix, this);
    this.bounds = template.shape.localBounds;
    this.center = this.bounds.getCenter(new THREE.Vector3());
    this.size = this.bounds.getSize(new THREE.Vector3());
    this.radius = Math.max(this.size.x, this.size.z) * .5;
    this.probeRadius = Math.max(.45, Math.min(this.size.x * .48, 2.6));
    const length = Math.max(0, this.size.z * .5 - this.probeRadius);
    this.probes = [];
    const rows = Math.max(2, Math.min(12, Math.ceil(this.size.z / (this.probeRadius * 1.5))));
    const width = Math.max(0, this.size.x * .5 - this.probeRadius);
    const columns = width > this.probeRadius ? [-width, 0, width] : [0];
    for (let row = 0; row <= rows; row++) for (const x of columns) this.probes.push(new THREE.Vector3(this.center.x + x, this.bounds.min.y + this.probeRadius + .12, this.center.z - length + row * length * 2 / rows));
    this.itemTarget = { position: this.position, center: new THREE.Vector3(), velocity: this.velocity,
      radius: this.radius, relation: team => this.system.units?.relation(this.team, team) ?? (team === this.team ? 1 : team === 0 ? 0 : -1),
      give: def => {
        const result = givePowerup(def, this);
        return result.health || result.ammo;
      } };
    this.maxHealth = num(s.MaxHealth, 1000);
    // Water damage (walkers only carry the keys): the class's interval and
    // amount, and the depth over its origin that hurts, its model's
    // bounding sphere's radius (src/vehicle-water.js).
    this.waterRule = this.walking ? waterDamageRule(s) : null;
    this.waterDepth = template.rig.sphere?.[3] ?? this.size.y;
    this.waterClock = this.waterRule?.interval ?? 0;
    this.healthType = (s.HealthType || 'vehicle').toLowerCase();
    this.creature = this.walking && this.healthType === 'animal';
    this.label = this.def.label;
    // speed, reverseSpeed, strafeSpeed, acceleration, deceleration and
    // turnRate, as the original game reads the class (src/vehicle-motion.js):
    // a walker or animal walks at its clips' pace and turns on A/D.
    Object.assign(this, groundMotion(this.def, this.clips));
    // How far over the floor a new hull is set down: a walker's feet, an
    // animal's on it, a craft at its SetAltitude. A hover rides its springs
    // (src/vehicle-hover.js).
    this.hover = this.creature ? 0 : this.walking ? .08 : num(s.SetAltitude, .5);
    this.supportProbes = this.creature
      ? [-1, 0, 1].map(x => new THREE.Vector3(x * this.size.x * .3, 0, 0)) : this.probes;
    this.supportNormal = new THREE.Vector3(0, 1, 0);
    this.ownStart = new THREE.Quaternion(); this.probeStart = new THREE.Vector3(); // a walker's probe a step ago (pushWalls)
    // Each seat also keeps its own aim and its lease (src/seats.js): the
    // player's look moves the aim of the seat he is in (seatIndex, through
    // the aimYaw/pitch/lookYaw/lookPitch accessors below), a bot's analogs
    // the aim of its own seat, and the hull follows seat 0's.
    const vehicle = this;
    // An aimer turns its node toward the point its seat aims at, from the
    // node's rest pose, within AimerYawLimits and AimerPitchLimits (up
    // positive; 30 and 45 degrees each way by default) at AimerYawRate and
    // AimerPitchRate (2 and 1 rad/s by default). Each barrel's beams follow
    // that barrel (beamSource); a weapon without aimers, its fire point; one
    // without either, the hull (the seat's own).
    const allAimers = new Set(this.def.seats.flatMap(seat => seat.weapons.flatMap(w => w.aimers.map(a => this.byName[String(a.node || "").toLowerCase()]))));
    const geometryMuzzles = new Map();
    const muzzleNode = (bone) => {
      if (!bone) return null;
      const name = Object.keys(this.byName).find(n => this.byName[n] === bone);
      const tip = template.muzzleTips?.get(name);
      if (!tip) return bone;
      if (!geometryMuzzles.has(bone)) {
        const marker = new THREE.Object3D(); marker.position.fromArray(tip); bone.add(marker);
        geometryMuzzles.set(bone, marker);
      }
      return geometryMuzzles.get(bone);
    };
    this.seats = this.def.seats.map((seat, index) => ({ ...seat, weapons: seat.weapons.map(w => {
      w = { ...w, def: vehicleMissileDefinition(w.def) };
      const aimers = w.aimers.map(a => {
        const yawLimits = degrees(a.AimerYawLimits) || [-30 * DEG, 30 * DEG];
        return { ...a, bone: this.byName[String(a.node || "").toLowerCase()],
          pitchLimits: (degrees(a.AimerPitchLimits) || [-45 * DEG, 45 * DEG]), yawLimits,
          // An arc from the left round past the back is kept in 0..2pi, as a
          // turret's is.
          wrapYaw: yawLimits[0] > 0 && yawLimits[0] <= Math.PI && yawLimits[1] >= Math.PI && yawLimits[1] < 2 * Math.PI,
          pitchRate: num(a.AimerPitchRate, 1), yawRate: num(a.AimerYawRate, 2),
          yaw: 0, pitch: 0, onTarget: false,
          // Where it points (world), whether it turned its node this frame,
          // and the seat that turns that node as its turret (-1: none).
          facing: new THREE.Vector3(0, 0, 1), posed: true, owner: -1, emitter: null };
      });
      const muzzles = [];
      for (const a of aimers) {
        // A parent aimer only steers when this weapon also defines child aimers.
        if (aimers.some(other => { let b = other.bone?.parent; while (b) { if (b === a.bone) return true; b = b.parent; } return false; })) continue;
        // So does one at a HierarchyLevel above 0 (the AT-ST's cube5): it
        // only turns the aimers under it, and the barrel step skips it.
        if (num(a.HierarchyLevel, 0) !== 0) continue;
        // An aimer with barrels (BarrelNodeName, NextBarrel) fires from each
        // in turn (the 2004 game's barrel step): from the barrel's
        // own FirePointName, else the barrel node itself (BarrelLength
        // further along it), which kicks back BarrelRecoil when it fires.
        // The AT-AT pilot's chin cannons name their tips with a key the
        // 2004 game never reads, so theirs leave from barrel_Lcannon and
        // barrel_Rcannon.
        if (a.barrels?.length) {
          for (const b of a.barrels) {
            const barrel = this.byName[String(b.BarrelNodeName || '').toLowerCase()] || null;
            let node = this.byName[String(b.FirePointName || '').toLowerCase()] || barrel;
            const length = num(b.BarrelLength, 0);
            if (node && length && node === barrel) {
              const marker = new THREE.Object3D(); marker.position.z = length; barrel.add(marker); node = marker;
            }
            if (!node) continue;
            muzzles.push({ fireNode: node, sourceAimer: a, emitter: beamSource(vehicle, index, node, a), barrel,
              recoil: b.BarrelRecoil !== undefined ? num(b.BarrelRecoil, 0) : .25 * length,
              rest: barrel ? barrel.position.clone() : null });
          }
          continue;
        }
        const explicit = this.byName[String(a.FirePointName || '').toLowerCase()];
        const descendants = explicit ? [explicit] : descendantMuzzles(a.bone, this.byName, allAimers);
        // Hailfire launch racks carry individual missile meshes, not hp nodes.
        if (!descendants.length && a.bone) for (const [name, bone] of Object.entries(this.byName)) {
          if (/^missile_\d+$/.test(name) && bone.parent === a.bone) descendants.push(muzzleNode(bone));
        }
        const nodes = descendants.length ? descendants : [muzzleNode(a.bone)].filter(Boolean);
        for (const node of nodes) if (!muzzles.some(m => m.fireNode === node)) {
          muzzles.push({ fireNode: node, sourceAimer: a, emitter: beamSource(vehicle, index, node, a) });
        }
      }
      if (muzzles.length > 2 && aimers.some(a => /^missile_(left|right)$/i.test(a.node))) {
        const racks = [...new Set(muzzles.map(m => m.sourceAimer.bone))].map(bone => muzzles.filter(m => m.sourceAimer.bone === bone));
        muzzles.splice(0, muzzles.length, ...Array.from({ length: Math.max(...racks.map(r => r.length)) }, (_, i) => racks.map(r => r[i]).filter(Boolean)).flat());
      }
      const turret = this.byName[String(seat.settings.TurretNodeName || '').toLowerCase()];
      const fire = this.byName[String(seat.settings.FirePointName || '').toLowerCase()] || this.byName[String(w.def.FirePointName || '').toLowerCase()];
      if (!muzzles.length) {
        const nodes = fire ? [fire] : descendantMuzzles(turret, this.byName);
        if (!nodes.length && turret) nodes.push(muzzleNode(turret));
        // The retail Hailfire has no cannon hardpoint: the visible lower
        // barrel is skinned to its centre body, whose forward rim is its mouth.
        if (!nodes.length && this.def.model === 'cis_tread_hailfire' && w.def.class === 'cis_weap_walk_spider_cannon') {
          const node = muzzleNode(this.byName.cis_hailfire_droid_center);
          if (node) nodes.push(node);
        }
        for (const node of nodes) muzzles.push({ fireNode: node, emitter: beamSource(vehicle, index, node, null) });
      }
      return { ...w, aimers, muzzles, state: new Weapon(w.def), barrel: 0, emitter: fire ? beamSource(vehicle, index, fire, null) : null };
    }), lease: null, aimYaw: 0, pitch: 0, lookYaw: null, lookPitch: null,
    // The point the seat's guns turn to (aimSeats) and how fast its aim
    // turns, for the rider's 9-pose.
    target: null, yawRate: 0, lastYaw: null,
    emitter: beamSource(vehicle, index, null, null) }));
    this.seatIndex = 0;
    this.driver = null;
    // Whether the player looks from the seat's eye, and the camera his aim
    // starts from otherwise (both set by his view).
    this.viewFirstPerson = false;
    this.aimCamera = null;
    this.age = 0;
    this.health = 0;
    this.alive = false;
    // A destroyed walker's death animation still to play, in seconds (0:
    // not dying). Plain: a page showing a worker's battle draws it from it.
    this.dying = 0;
    this.yaw = this.previousYaw = this.aimYaw = this.pitch = 0;
    this.roll = this.tilt = this.previousRoll = this.previousTilt = 0;
    this.yawRate = 0;
    // A hover's pitch and roll rates (rad/s) and its ground factor (1 on its
    // springs, losing 1 a second off them): src/vehicle-hover.js.
    this.tiltRate = this.rollRate = this.support = 0;
    this.forwardInput = this.rightInput = 0;
    this.lookYaw = this.lookPitch = null;
    this.grounded = this.flying;
    this.flightState = this.flying ? 'Landed' : null;
    this.altitude = 0;
    // Flight state, plain so a page showing a worker's battle sees it: the
    // player's throttle (m/s wanted), the speed along the heading, the
    // nose's pitch rate, how long the flight state has held, and the floor
    // the last tick found under the hull (NaN: none).
    this.throttle = 0;
    this.airspeed = 0;
    this.tiltRate = 0;
    this.stateTime = 0;
    this.floorY = NaN;
    this.bank = 0;
    // The tow cable (Vehicles.launchTow): the walker it holds (its index in
    // the system, -1 for none), the anchor on that walker's leg in the
    // walker's frame, the leg's name, the harpoon's angle round the walker
    // when it struck, and how far the cable has wound since (rad; three
    // circuits trip the walker). Plain, so a page showing a worker's
    // battle draws the cable from them (Vehicles.drawTethers).
    this.towTarget = -1;
    this.towLocal = new THREE.Vector3();
    this.towLeg = '';
    this.towAngle = 0;
    this.towWinding = 0;
    this.wheeled = name.includes('hailfire');
    this.repulsor = !this.walking && !this.wheeled;
    this.hoverPhase = (slot.matrix.elements[12] * .13 + slot.matrix.elements[14] * .17) % (Math.PI * 2);
    this.wheels = [];
    // The native rims lie in local XZ: local Y is their axle. Keep the
    // imported cant, and reverse the mirrored left axle's spin direction.
    this.root.updateMatrixWorld(true);
    for (const name of ['wheel_left', 'wheel_right']) {
      const bone = this.byName[name];
      if (!bone) continue;
      const worldAxis = UP.clone().applyQuaternion(bone.getWorldQuaternion(new THREE.Quaternion()));
      const axis = UP.clone().multiplyScalar(Math.sign(worldAxis.dot(RIGHT)) || 1);
      const at = bone.getWorldPosition(new THREE.Vector3());
      this.wheels.push({ bone, axis, x: at.x, radius: template.wheelRadii.get(name) || 4, phase: 0, previous: 0 });
    }
    // Each seat's camera, limits, turret and rider, read in the rest pose.
    this.seats.forEach((seat, index) => { seat.view = seatView(this, seat, index); });
    // A walker's legs, for the tow cable: each leg's thigh, calf and foot
    // (bone_fl_thigh and the like) in the hull's frame at rest, and the
    // height of the pelvis they hang from (a harpoon above it struck the
    // body, not a leg). A walker without named legs offers none.
    this.legs = [];
    this.legTop = this.bounds.min.y + this.size.y * .6;
    if (this.walking) {
      const at = (bone) => bone.getWorldPosition(new THREE.Vector3());
      const named = Object.entries(this.byName);
      const pelvis = named.filter(([n]) => /pelvis$/.test(n)).map(([, b]) => at(b));
      if (pelvis.length) this.legTop = Math.min(...pelvis.map((p) => p.y));
      const groups = new Map();
      for (const [name, bone] of named) {
        const m = /^(.*?)_?(thigh|calf|foot)$/.exec(name);
        if (!m) continue;
        const leg = groups.get(m[1]) || { name: m[1], thigh: null, calf: null, foot: null };
        leg[m[2].toLowerCase()] = at(bone);
        groups.set(m[1], leg);
      }
      for (const leg of groups.values()) if (leg.calf || leg.thigh) this.legs.push(leg);
    }
    // A node one seat turns as its turret while another seat's aimer turns
    // it too (the AT-AT's head: the gunner's turret, the pilot's aimer).
    for (const seat of this.seats) for (const w of seat.weapons) for (const a of w.aimers) {
      a.owner = this.seats.findIndex((other) => other !== seat && other.view.turret !== null && other.view.turret === a.bone);
    }
    this.frameScratch = { q: new THREE.Quaternion(), q2: new THREE.Quaternion(), v: new THREE.Vector3(), m: new THREE.Matrix4(), frame: new THREE.Matrix4(), up: new THREE.Vector3() };
    this.aimHit = makeHit();
    this.unitHit = { unit: null, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), part: null };
    this.beamFrom = new THREE.Vector3();
    this.shotNode = null; this.shotEmitter = null; // the last muzzleOf's
    this.aimQ = new THREE.Quaternion(); this.aimQ2 = new THREE.Quaternion();
    this.point = new THREE.Vector3();
    this.normal = new THREE.Vector3();
    this.nearest = new THREE.Vector3();
    this.from = new THREE.Vector3();
    this.legEuler = new THREE.Euler(0, 0, 0, "YXZ");
    this.recoilAxis = new THREE.Vector3();
    this.recoilPoint = new THREE.Vector3();
    this.direction = new THREE.Vector3();
    this.target = new THREE.Vector3();
    this.muzzle = new THREE.Vector3();
    this.hit = makeHit();
    this.q = new THREE.Quaternion();
    // The bounding sphere (half the collision box's diagonal): the registry's
    // broadphase and the seat exit's radius.
    this.boundRadius = this.size.length() * .5;
    this.lastAttacker = null; // who last damaged it, for kill credit
    this.aiTarget = new VehicleTarget(this); // its entry in the units registry
    this.root.visible = false;
    this.body.enabled = false;
    system.scene.add(this.root);
  }

  get weapons() { return this.seats.flatMap(s => s.weapons.map(w => w.state)); }
  get seat() { return this.seats[this.seatIndex]; }
  get weapon() { return this.seat.weapons[0]?.state; }
  get arsenal() { return this; }
  current(channel) { return this.seat.weapons[channel]?.state || null; }
  viewDirection(out) { return out.set(Math.sin(this.aimYaw) * Math.cos(this.pitch), Math.sin(this.pitch), Math.cos(this.aimYaw) * Math.cos(this.pitch)); }
  sphere(out) { out.copy(this.center).applyMatrix4(this.matrix); return this.radius; }
  // What a blast measures to (src/ordnance.js): the model's bounding sphere,
  // placed with the hull, at least as wide as its collision box.
  blastSphere(out) {
    const sphere = this.system.templates.get(this.def.model)?.rig.sphere;
    if (!sphere) { out.copy(this.center).applyMatrix4(this.matrix); return 0.5 * this.size.length(); }
    out.set(sphere[0], sphere[1], sphere[2]).applyMatrix4(this.matrix);
    return Math.max(sphere[3], 0.5 * this.size.length());
  }

  // The player's aim is the aim of the seat he is in (seatIndex).
  get aimYaw() { return this.seats[this.seatIndex].aimYaw; }
  set aimYaw(v) { this.seats[this.seatIndex].aimYaw = v; }
  get pitch() { return this.seats[this.seatIndex].pitch; }
  set pitch(v) { this.seats[this.seatIndex].pitch = v; }
  get lookYaw() { return this.seats[this.seatIndex].lookYaw; }
  set lookYaw(v) { this.seats[this.seatIndex].lookYaw = v; }
  get lookPitch() { return this.seats[this.seatIndex].lookPitch; }
  set lookPitch(v) { this.seats[this.seatIndex].lookPitch = v; }

  // The spread rule a seat's weapons follow: the player's, or the AI's floor
  // (Weapon { player: false }) while a bot holds it (provisional for vehicle
  // weapons; docs/ai-vehicles.md).
  setSeatPolicy(i, player) {
    for (const w of this.seats[i]?.weapons || []) seatWeaponPolicy(w.state, player);
  }

  // A bot holds seat i.
  crewed(i) { const lease = this.seats[i]?.lease; return Boolean(lease && !lease.human); }
  // Anyone aboard: the player (driver) or a lease on any seat.
  get occupied() {
    if (this.driver) return true;
    for (let i = 0; i < this.seats.length; i++) if (this.seats[i].lease) return true;
    return false;
  }
  seatFree(i) { return Boolean(this.seats[i]) && !this.seats[i].lease && !(this.driver && i === this.seatIndex); }
  firstFreeSeat() { for (let i = 0; i < this.seats.length; i++) if (this.seatFree(i)) return i; return -1; }
  get freeSeats() { let n = 0; for (let i = 0; i < this.seats.length; i++) if (this.seatFree(i)) n++; return n; }
  get occupants() { let n = 0; for (const s of this.seats) if (s.lease) n++; return n; }
  // The team in control: the first occupant's (pilot first), else its own.
  get controllingTeam() { for (const s of this.seats) if (s.lease) return s.lease.team; return this.team; }
  // Whether rounds meet a rider on seat i: one shows there and the section
  // leaves him in the open (seatView: rider.shootable).
  exposedSeat(i) {
    const rider = this.seats[i]?.view.rider;
    return Boolean(this.alive && rider?.node && rider.shootable);
  }
  // Whether this hull's own collision keeps rounds off its riders, as the
  // original game's nearest hit does: a canopy, a turret's ring. A riding
  // animal's doesn't here. Its class names no OrdnanceCollision, so every
  // primitive it has stops rounds, the coarse spheres that keep it and its
  // rider out of walls among them, and those enclose the rider on its back
  // (the tauntaun's, the kaadu's). The port lets a round on through the
  // animal to the rider the player sees (src/ordnance.js riderBehind); the
  // animal takes what misses him.
  shieldsRiders() { return this.healthType !== 'animal'; }
  // A rider's hit shapes on seat i this tick (src/rider-pose.js
  // riderHitShapes): his body as poseRider poses it, where seatRider places
  // it. Written to shapes; returns their count, 0 where none shows.
  riderShapes(i, unit, shapes) {
    const s = this.system.rider;
    const frame = riderBody(this, i, unit, s.points, s.frame, s);
    return frame ? riderHitShapes(s.points, frame, unit.rootScale, shapes) : 0;
  }
  // Where a rider killed on seat i starts to fall from, in the world: out,
  // or null where none shows (src/rider-pose.js riderDropPoint).
  riderDrop(i, unit, out) {
    const s = this.system.rider;
    const frame = riderBody(this, i, unit, s.points, s.frame, s);
    return frame ? riderDropPoint(s.points, out).applyMatrix4(frame) : null;
  }

  // A seat's eye (the 2004 game's controller position): its
  // EyePointOffset in the seat's frame. What the seat sees and aims from.
  seatOrigin(i, out) { return this.seatEye(this.seats[i] ? i : 0, out); }
  seatEye(i, out, presented = false) {
    return out.copy(this.seats[i].view.eye).applyMatrix4(this.seatFrame(i, this.frameScratch.frame, presented));
  }
  // A seat's aim direction.
  seatForward(i, out) {
    if (this.flying && i === 0) return out.set(0, 0, 1).applyQuaternion(this.quaternion);
    const seat = this.seats[i] || this.seats[0];
    return out.set(Math.sin(seat.aimYaw) * Math.cos(seat.pitch), Math.sin(seat.pitch), Math.cos(seat.aimYaw) * Math.cos(seat.pitch));
  }
  // The frame a seat's camera and aim hang from (the original game's camera
  // subject). The pilot's is the hull's own: its position and heading,
  // leaving out the repulsor's lean and bob. A turret seat's is its aim:
  // at the turret node's pivot (the hull's origin without one), looking
  // along the seat's aim, its up the node's. presented: as drawn between
  // ticks; else at the tick.
  seatFrame(i, out, presented = false) {
    if (this.flying && i === 0) return out.compose(presented ? this.root.position : this.position, presented ? this.root.quaternion : this.quaternion, UNIT_SCALE);
    const s = this.frameScratch;
    const yaw = presented ? this.root.rotation.y : this.yaw;
    out.compose(presented ? this.visual : this.position, this.flying ? (presented ? this.root.quaternion : this.quaternion) : s.q.setFromAxisAngle(UP, yaw), UNIT_SCALE);
    if (i === 0) return out;
    const seat = this.seats[i], view = seat.view;
    lookBasis(aimVector(angle(seat.aimYaw - yaw), seat.pitch, s.v), view.up, s.q2);
    return out.multiply(s.m.compose(view.pivot, s.q2, UNIT_SCALE));
  }
  // Holds a seat's aim inside its limits: the pitch always; a turret's yaw
  // when its arc is limited, measured from the hull's heading.
  clampAim(i) {
    if (this.flying && i === 0) return;
    const seat = this.seats[i], view = seat.view;
    const low = view.pitchLimits[0], high = view.pitchLimits[1];
    seat.pitch = THREE.MathUtils.clamp(seat.pitch, low, high);
    if (seat.lookPitch !== null) seat.lookPitch = THREE.MathUtils.clamp(seat.lookPitch, low, high);
    if (!view.yawLimits) return;
    seat.aimYaw = this.holdYaw(view, seat.aimYaw);
    if (seat.lookYaw !== null) seat.lookYaw = seat.aimYaw + angle(this.holdYaw(view, seat.lookYaw) - seat.aimYaw);
  }
  // A yaw held inside a seat's limited arc, measured from the hull's heading.
  holdYaw(view, yaw) {
    return this.yaw + THREE.MathUtils.clamp(arcYaw(view, yaw - this.yaw), view.yawLimits[0], view.yawLimits[1]);
  }
  // A seat's aim as it is taken afresh: along the hull, or the middle of a
  // turret's arc that leaves the hull's heading out; level in a flyer, else a
  // little down (the port's choice).
  restAim(i) {
    const seat = this.seats[i];
    seat.aimYaw = this.restYaw(i);
    seat.pitch = this.flying ? 0 : -.12;
    seat.lookYaw = seat.lookPitch = null;
    this.clampAim(i);
  }
  // The heading a seat's aim rests at (restAim): the hull's, or the middle
  // of an arc that leaves it out (the snowspeeder's rear gun, YawLimits
  // 80 280, rests looking aft).
  restYaw(i) {
    const arc = this.seats[i]?.view.yawLimits;
    return this.yaw + (arc && (arc[0] > 0 || arc[1] < 0) ? (arc[0] + arc[1]) / 2 : 0);
  }
  // The leg a point (world) on this walker is on, for the tow cable: the
  // nearest leg's column at that height, below the pelvis; null for a
  // point on the body or a hull with no legs. Writes the anchor in the
  // hull's frame to out and returns the leg's name.
  legAt(point, out) {
    if (!this.legs.length) return null;
    const local = this.frameScratch.v.copy(point).applyMatrix4(this.body.inverse);
    if (local.y > this.legTop) return null;
    let best = null, span = Infinity;
    for (const leg of this.legs) {
      const column = leg.calf || leg.thigh;
      const d = Math.hypot(local.x - column.x, local.z - column.z);
      if (d < span) { span = d; best = leg; }
    }
    const column = best.calf || best.thigh;
    const low = best.foot?.y ?? this.bounds.min.y, high = best.thigh?.y ?? this.legTop;
    out.set(column.x, THREE.MathUtils.clamp(local.y, low, high), column.z);
    return best.name;
  }
  // The point a leg's column stands at in the world, for a harpoon aimed at
  // it: the leg's calf (or thigh) in the hull's frame now.
  legPoint(name, out) {
    const leg = this.legs.find((l) => l.name === name);
    if (!leg) return null;
    return out.copy(leg.calf || leg.thigh).applyMatrix4(this.matrix);
  }

  // The frame a seat lets its occupant out in: a turret seat's own heading,
  // otherwise the hull's (the occupied control's frame forward; which seats
  // carry their own frame is provisional).
  seatFrameForward(i, out) {
    const seat = this.seats[i] || this.seats[0];
    const yaw = i > 0 && seat.settings.TurretNodeName ? seat.aimYaw : this.yaw;
    return out.set(Math.sin(yaw), 0, Math.cos(yaw));
  }

  spawn(team) {
    this.team = team;
    this.position.setFromMatrixPosition(this.slot.matrix);
    this.yaw = Math.atan2(this.slot.matrix.elements[8], this.slot.matrix.elements[10]);
    this.aimYaw = this.previousYaw = this.yaw;
    this.pitch = this.tilt = this.roll = this.previousTilt = this.previousRoll = 0;
    this.yawRate = this.tiltRate = this.rollRate = this.support = 0;
    this.forwardInput = this.rightInput = 0;
    this.lookYaw = this.lookPitch = null;
    this.grounded = this.flying;
    this.flightState = this.flying ? 'Landed' : null;
    this.altitude = 0;
    this.throttle = this.airspeed = this.tiltRate = this.stateTime = 0;
    this.bank = 0;
    if (this.flying) { this.quaternion.setFromAxisAngle(UP, this.yaw); initFlight(this); }
    this.towTarget = -1; this.towWinding = this.towAngle = 0; this.towLeg = ''; this.towLocal.set(0, 0, 0);
    this.floorY = NaN;
    this.velocity.set(0, 0, 0);
    for (const wheel of this.wheels) wheel.phase = wheel.previous = 0;
    this.health = this.maxHealth;
    this.alive = this.root.visible = this.body.enabled = true;
    this.dying = 0;
    this.driver = null;
    this.seatIndex = 0;
    this.sleeping = false;
    this.lastAttacker = null;
    // A new hull: empty seats, each aimed down its heading, and a new life
    // for the registry entry (references to the old hull go stale).
    for (const seat of this.seats) {
      seat.lease = null;
      seat.aimYaw = this.yaw; seat.pitch = 0; seat.lookYaw = seat.lookPitch = null;
      seat.target = null; seat.yawRate = 0; seat.lastYaw = null;
      for (const w of seat.weapons) for (const a of w.aimers) a.yaw = a.pitch = 0;
    }
    this.aiTarget.generation += 1;
    for (const seat of this.seats) for (const w of seat.weapons) { w.state = new Weapon(w.def); w.barrel = 0; }
    // Placed spawn y is a ground hint. Settle onto the actual collision floor:
    // a hover where its springs carry it, lying with the ground (its physics
    // settle the rest).
    this.from.copy(this.position).addScaledVector(UP, 4);
    const floor = this.system.collision.cast(this.from, DOWN, 12, this.hit, this.body);
    if (floor && !this.walking && !this.flying) {
      this.position.y = floor.point.y + restHeight(this.def);
      this.alignGround(floor.normal, 1);
      this.support = 1;
    } else if (floor) {
      // A craft with gear stands on its feet (src/flight/model.js
      // standHeight); any other hull rests its hover height up.
      const stand = this.flying ? standHeight(this) : null;
      this.position.y = floor.point.y + (this.creature ? 0 : stand !== null ? stand : -this.bounds.min.y + this.hover);
      if (this.creature) this.alignGround(floor.normal, 1);
    }
    this.previousTilt = this.tilt; this.previousRoll = this.roll;
    this.walkPhase = this.walkBlend = 0;
    this.previous.copy(this.position);
    this.visual.copy(this.position);
    this.syncBody();
    this.render(1, 0);
    return true;
  }

  // The collision body and the matrix gameplay reads: a ground hull's frame
  // with its pitch and roll, as drawn. Its quaternion, as replicated, stays
  // its heading alone (the lean travels as tilt and roll).
  syncBody() {
    if (!this.flying) this.quaternion.setFromEuler(this.rotation.set(0, this.yaw, 0));
    this.matrix.compose(this.position, this.flying ? this.quaternion : hullQuaternion(this, this.q), UNIT_SCALE);
    this.system.collision.moveBody(this.body, this.matrix);
    this.itemTarget.center.copy(this.center).applyMatrix4(this.matrix);
  }

  alignGround(normal, blend) {
    const sin = Math.sin(this.yaw), cos = Math.cos(this.yaw);
    const x = cos * normal.x - sin * normal.z, z = sin * normal.x + cos * normal.z;
    this.tilt += (-Math.atan2(z, normal.y) - this.tilt) * blend;
    this.roll += (-Math.atan2(x, Math.hypot(normal.y, z)) - this.roll) * blend;
  }

  // attacker (optional): who is credited, a unit or whatever owned the round.
  damage(amount, attacker = null) {
    if (!this.alive || !(amount > 0)) return;
    if (attacker) this.lastAttacker = attacker;
    this.system.onDamaged?.(this, amount, attacker);
    this.setHealth(this.health - amount);
  }

  setHealth(value) {
    if (!this.alive) return;
    this.health = THREE.MathUtils.clamp(value, 0, this.maxHealth);
    if (this.health > 0) return;
    this.alive = this.body.enabled = false;
    // A walker with a death animation plays it out before it is gone
    // (dyingTick); every other hull is gone at once.
    this.dying = this.def.fx?.dying?.span || 0;
    this.root.visible = this.dying > 0;
    if (this.flying) this.flightState = 'Destroyed';
    this.slot.remaining = this.slot.desc.respawnTime;
    // Everyone aboard dies with it: the bots' seats are let go first, all at
    // once, then they die (credited to whoever destroyed it), then the
    // player's handler runs.
    this.system.destroyedCrew?.(this);
    const humanDriver = this.humanDriver;
    try { this.system.onDestroyed?.(this); }
    finally { if (humanDriver?.vehicle === this) humanDriver.release(); }
    this.system.releaseTow?.(this, 'destroyed');
    this.driver = null;
    for (const seat of this.seats) {
      const lease = seat.lease;
      if (!lease) continue;
      lease.released = true;
      if (lease.unit?.seat === lease) lease.unit.seat = null;
      seat.lease = null;
    }
    this.velocity.set(0, 0, 0);
    // The blast goes off at the class's ExplosionOffset in the hull's frame
    // (the AT-AT's body, the AT-ST's head; tools/zero_vehicle.py vehicle_fx).
    const offset = this.def.fx?.explosionOffset;
    const at = offset ? new THREE.Vector3(offset[0], offset[1], offset[2]).applyAxisAngle(UP, this.yaw).add(this.position) : this.position;
    if (this.def.explosion) this.system.ordnance.explosion(this.def.explosion, at, this);
    // An animal (the tauntaun, the kaadu) has no blast: it falls.
    else if (!this.creature) this.system.effects?.play('explosion', at);
  }

  // A tick of a destroyed walker's dying: its death animation (death01, at
  // its AnimationSpeed: fx.dying.span s in all) plays, its dying explosions
  // being the page's (src/vehicle-effects.js); once it has played,
  // FinalExplosion (else its ExplosionName) goes off at FinalExplosionOffset
  // in the hull's frame, with its damage, and the hull is gone.
  dyingTick(dt) {
    if (!(this.dying > 0)) return;
    this.dying = Math.max(0, this.dying - dt);
    if (this.dying > 0) return;
    this.root.visible = false;
    const explosion = this.def.finalExplosion || this.def.explosion;
    if (!explosion) return;
    const offset = this.def.fx?.dying?.finalOffset || [0, 0, 0];
    const at = new THREE.Vector3(offset[0], offset[1], offset[2]).applyAxisAngle(UP, this.yaw).add(this.position);
    this.system.ordnance.explosion(explosion, at, this);
  }

  tick(dt, input) {
    this.previous.copy(this.position);
    this.previousYaw = this.yaw;
    this.previousTilt = this.tilt; this.previousRoll = this.roll;
    for (const wheel of this.wheels) wheel.previous = wheel.phase;
    if (!this.alive || this.sleeping && !this.occupied) return;
    this.aimCrew(dt);
    if (this.flying) { this.fly(dt, input); return; }
    const oldYaw = this.yaw;
    this.steer(dt, input, this.strafeSpeed === 0);
    const response = 1 - Math.exp(-10 * dt);
    this.forwardInput += ((input.forward || 0) - this.forwardInput) * response;
    this.rightInput += ((input.right || 0) - this.rightInput) * response;
    const driving = (this.seatIndex === 0 || this.crewed(0)) && !input.brake;
    if (!this.walking) {
      // A hover (src/vehicle-hover.js): its physics, then the collision pass
      // (its wall spheres, then its springs), then its springs' forces.
      hoverPhysics(this, dt, driving ? this.forwardInput : 0, driving ? this.rightInput : 0, Boolean(input.brake));
      this.body.enabled = false;
      try {
        this.pushWalls(oldYaw, hullQuaternion(this, this.q), true);
        hoverContacts(this);
      } finally { this.body.enabled = this.alive; }
      hoverSprings(this, dt);
    } else this.walkGround(dt, input, driving, oldYaw);
    if (this.wheeled) {
      const dx = this.position.x - this.previous.x, dz = this.position.z - this.previous.z;
      const distance = dx * Math.sin(this.yaw) + dz * Math.cos(this.yaw);
      const turned = angle(this.yaw - this.previousYaw);
      for (const wheel of this.wheels) wheel.phase += (distance - turned * wheel.x) / wheel.radius;
    }
    this.syncBody();
    if (this.system.inDeathRegion?.(this.position) || this.position.y < -300) this.damage(Infinity);
    // Deep water wears a walker down (src/vehicle-water.js).
    if (this.walking && this.alive) {
      const hurt = waterDamageStep(this, dt, this.system.water, this.waterRule, this.position, this.waterDepth);
      if (hurt > 0) this.damage(hurt);
    }
    this.sleeping = !this.occupied && this.velocity.lengthSq() < (this.walking ? .0001 : HOVER_REST * HOVER_REST)
      && this.previous.distanceToSquared(this.position) < .000001
      && Math.abs(this.tilt - this.previousTilt) + Math.abs(this.roll - this.previousRoll) < 1e-5;
  }

  // A walker's or riding animal's tick on the ground: it moves at its
  // speeds in the plane, its probes find the floor under it (an animal
  // lies with it), and gravity takes over at ledges and map cuts.
  walkGround(dt, input, driving, oldYaw) {
    const c = this.system.collision;
    const forward = driving ? this.forwardInput * (this.forwardInput >= 0 ? this.speed : this.reverseSpeed) : 0;
    const right = driving && this.strafeSpeed ? this.rightInput * this.strafeSpeed : 0;
    const norm = Math.max(1, Math.hypot(this.forwardInput, this.strafeSpeed ? this.rightInput : 0));
    const sin = Math.sin(this.yaw), cos = Math.cos(this.yaw);
    const accel = (forward || right) ? this.acceleration : this.deceleration * (input.brake ? 2 : 1);
    // +Z is forward in the assets; screen-right is -X in that frame.
    this.velocity.x = approach(this.velocity.x, (sin * forward - cos * right) / norm, accel * dt);
    this.velocity.z = approach(this.velocity.z, (cos * forward + sin * right) / norm, accel * dt);
    const legs = !this.creature && this.def.fx?.legs;
    if (!this.creature && !legs) {
      this.roll -= this.roll * (1 - Math.exp(-5 * dt));
      this.tilt -= this.tilt * (1 - Math.exp(-4 * dt));
    }
    this.position.x += this.velocity.x * dt;
    this.position.z += this.velocity.z * dt;
    this.body.enabled = false;
    try {
      this.quaternion.setFromAxisAngle(UP, this.yaw);
      this.pushWalls(oldYaw, this.quaternion, false);
      if (legs) { this.standOnLegs(dt, legs); return; }
      // Multiple suspension probes prevent the hull climbing walls or flying
      // across a missing floor. Gravity takes over at ledges and map cuts.
      let floor = -Infinity;
      this.supportNormal.set(0, 0, 0);
      let supports = 0, heightSum = 0;
      for (const probe of this.supportProbes) {
        this.from.copy(probe).applyQuaternion(this.quaternion).add(this.position);
        this.from.y = this.position.y + this.bounds.min.y + 1.2;
        const hit = c.cast(this.from, DOWN, 6, this.hit);
        if (hit && hit.normal.y > .65) {
          if (this.creature) {
            // Project the contact plane back to the model origin. Taking the
            // highest hull ray instead makes feet hover on sloping snow.
            heightSum += hit.point.y + (hit.normal.x * (hit.point.x - this.position.x) + hit.normal.z * (hit.point.z - this.position.z)) / hit.normal.y;
            this.supportNormal.add(hit.normal); supports++;
          } else floor = Math.max(floor, hit.point.y);
        }
      }
      if (supports) {
        floor = heightSum / supports;
        this.alignGround(this.supportNormal.normalize(), 1 - Math.exp(-14 * dt));
      }
      // A walker wades: the floor under the water holds it, not the surface
      // (src/vehicle-water.js).
      if (Number.isFinite(floor)) {
        const target = floor + (this.creature ? 0 : -this.bounds.min.y + this.hover);
        if (target > this.position.y + 1.1) {
          this.position.x = this.previous.x; this.position.z = this.previous.z;
          this.velocity.x = this.velocity.z = 0;
        } else {
          // Analytic easing is stable at every display and physics rate.
          this.position.y += (target - this.position.y) * (this.creature ? 1 : 1 - Math.exp(-14 * dt));
          this.velocity.y = 0;
        }
      } else {
        this.velocity.y -= 20 * dt;
        this.position.y += this.velocity.y * dt;
      }
    } finally { this.body.enabled = this.alive; }
  }

  // A walker on its legs, as the 2004 game stands one (docs/vehicles.md
  // "Standing on the ground"). Each foot
  // of each LegPair casts straight down from LegRayHitLength (L) above its
  // primitive's bottom, 2L long, onto terrain and buildings: h, the ground
  // over the foot, in [-L, L]. A pair stands at the lower of its two feet,
  // drawn toward the higher once that is more than L/2 up (WalkerOrientRoll:
  // their mean), and keeps how much higher its left foot's ground is than
  // its right's (legSide, -1..1, eased 0.4 a step), which the 2004 game
  // blends its legs toward their _leftup/_rightup poses by. With two pairs
  // or more (the AT-AT, the spider walker): no pair on the ground and it
  // falls (29.4 m/s/s); else it pitches 0.01 of the front pair's height
  // over the back's a step (up to 0.2955), rolls toward the pairs' mean
  // side over WalkerWidth where it is wider than 1 (the spider's 22), and
  // its root rises by the mean of the first two pairs' heights, sinking no
  // more than 0.1 a step. One pair (the AT-ST): on the ground it rises to
  // 0.1 under it; off it, it falls. The 2004 game's constants are per
  // update; the port's steps are 1/60 s, which these scale to.
  standOnLegs(dt, legs) {
    const c = this.system.collision, L = legs.rayLength, k = dt * 60;
    const q = this.frameScratch.q.setFromEuler(this.legEuler.set(-this.tilt, this.yaw, this.roll, 'YXZ'));
    const heights = [];
    this.legSide ||= legs.pairs.map(() => 0);
    legs.pairs.forEach((pair, i) => {
      const h = ['left', 'right'].map((which) => {
        this.from.fromArray(pair[which].at).applyQuaternion(q).add(this.position);
        this.from.y += L;
        const hit = c.cast(this.from, DOWN, 2 * L, this.hit);
        return hit ? L - hit.distance : -L;
      });
      const lo = Math.min(h[0], h[1]), hi = Math.max(h[0], h[1]);
      heights.push(legs.orientRoll ? (h[0] + h[1]) / 2 : lo + (hi - lo) * THREE.MathUtils.clamp((hi - L / 2) / (.4 * L), 0, 1));
      const side = THREE.MathUtils.clamp(-.6 * (h[0] - h[1]), -1, 1);
      this.legSide[i] += (side - this.legSide[i]) * Math.min(1, .4 * k);
    });
    // The rays meet terrain and buildings, not water: a walker wades in on
    // the ground under it (src/vehicle-water.js).
    if (heights.length >= 2) {
      // (A centimetre's grace: a foot exactly on flat ground reads 0 and
      // would drop and rise again every other step.)
      if (!heights.some((h) => h > -.01)) {
        this.velocity.y -= 29.4 * dt;
        this.position.y += this.velocity.y * dt;
        return;
      }
      this.velocity.y = 0;
      const MAX = .2955;
      this.tilt = THREE.MathUtils.clamp(this.tilt + .01 * k * (heights[0] - heights[heights.length - 1]), -MAX, MAX);
      if (legs.width > 1) {
        const side = this.legSide.reduce((a, b) => a + b, 0) / this.legSide.length;
        this.roll += (THREE.MathUtils.clamp(Math.atan(side / legs.width), -MAX, MAX) - this.roll) * Math.min(1, .05 * k);
      }
      const rise = (Math.max(heights[0], -.1 * k) + Math.max(heights[1], -.1 * k)) / 2;
      this.position.y += rise;
      return;
    }
    const h = heights[0];
    if (h >= 0) {
      this.velocity.y = 0;
      this.position.y += h - .1;
    } else {
      this.velocity.y -= 29.4 * dt;
      this.position.y += Math.max(this.velocity.y * dt, h);
    }
  }

  // The hull's sphere chain against walls (local spheres with small fixed
  // steps catch corners and thin walls; no raycasts against visual meshes
  // and no BVH rebuild on motion): each probe sphere, placed in the hull's
  // frame q, is pushed out of the polygons of buildings, props, barriers and
  // other hulls that are no floor, and with terrain of the terrain's steep
  // faces too, and stops moving into them.
  //
  // A walker against another hull (a moving body) gives way only as far as
  // its own probe moved into it this step: the other hull answers for its
  // own motion. The probes are spheres in the walker's box and reach past
  // its body, so a hull stopped by the walker's body still sat inside them,
  // and the walker, pushed fully out every step, was bulldozed at the other
  // hull's speed (a hailfire drove an AT-TE across Geonosis at 8 m/s, and
  // other contacts at up to 30 m/s).
  pushWalls(oldYaw, q, terrain) {
    const c = this.system.collision;
    const own = this.walking ? this.ownStart.setFromAxisAngle(UP, this.previousYaw) : null;
    for (let pass = 0; pass < 2; pass++) for (const probe of this.probes) {
      this.point.copy(probe).applyQuaternion(q).add(this.position);
      if (own) this.probeStart.copy(probe).applyQuaternion(own).add(this.previous);
      for (const polygon of c.gather(this.point, this.probeRadius, true)) for (const tri of polygon.triangles) this.pushOut(tri, oldYaw, own && polygon.moving);
      if (terrain) for (const tri of c.terrainNear(this.point, this.probeRadius)) this.pushOut(tri, oldYaw);
    }
  }

  pushOut(tri, oldYaw, hull = false) {
    tri.getNormal(this.normal);
    if (Math.abs(this.normal.y) > .65) return; // suspension handles floors
    tri.closestPointToPoint(this.point, this.nearest);
    this.normal.subVectors(this.point, this.nearest);
    const distance = this.normal.length();
    if (distance >= this.probeRadius || distance < 1e-6) return;
    this.normal.divideScalar(distance);
    let depth = this.probeRadius - distance + .002;
    if (hull) {
      // How far this probe came toward the other hull this step.
      const came = -(this.normal.x * (this.point.x - this.probeStart.x) + this.normal.y * (this.point.y - this.probeStart.y) + this.normal.z * (this.point.z - this.probeStart.z));
      depth = Math.min(depth, Math.max(0, came));
      if (!(depth > 0)) return;
    }
    this.position.addScaledVector(this.normal, depth);
    this.point.addScaledVector(this.normal, depth);
    const into = this.velocity.dot(this.normal);
    if (into < 0) this.velocity.addScaledVector(this.normal, -into);
    if (depth > .15) this.yaw = oldYaw;
  }

  aimPlayerSeat(dt) {
    // Mouse aim is direct. Hull steering below still has its own turn rate
    // and acceleration; filtering the aim too adds latency to the reticle.
    if (this.lookYaw !== null) {
      this.aimYaw = this.lookYaw;
      this.pitch = this.lookPitch;
    }
    if (this.driver) this.clampAim(this.seatIndex);
  }

  steer(dt, input, keyboardSteers = false) {
    this.aimPlayerSeat(dt);
    // The hull follows the pilot's aim: the player's in seat 0, or a bot's.
    const bot = this.crewed(0);
    if (this.seatIndex !== 0 && !bot) { this.yawRate *= Math.exp(-10 * dt); return; }
    const pilot = this.seats[0];
    const keys = THREE.MathUtils.clamp((input.turn || 0) + (keyboardSteers ? input.right || 0 : 0), -1, 1);
    const delta = -keys * this.turnRate * dt;
    pilot.aimYaw += delta;
    if ((!bot || pilot.lease?.unit?.networkHuman) && this.lookYaw !== null) this.lookYaw += delta;
    const error = angle(pilot.aimYaw - this.yaw);
    const wanted = THREE.MathUtils.clamp(error * 5, -this.turnRate, this.turnRate);
    this.yawRate += (wanted - this.yawRate) * (1 - Math.exp(-9 * dt));
    const step = this.yawRate * dt;
    this.yaw += Math.sign(step) === Math.sign(error) && Math.abs(step) > Math.abs(error) ? error : step;
    // A turret's arc turns with the hull under it.
    if (this.driver && this.seatIndex !== 0) this.clampAim(this.seatIndex);
  }

  fly(dt, input) {
    if (this.driver && this.seatIndex !== 0) this.aimPlayerSeat(dt);
    // Nobody aboard a craft in flight: it comes down (src/flight/pilotless.js).
    if (this.flightState === 'Flying' && !this.occupied) { stepPilotless(this, dt); return; }
    if (!this.occupied) abandonLift(this);
    const networkInput = this.seats[0]?.lease?.unit?.networkHuman && this.seats[0].lease.intent.flight;
    if (networkInput) input = networkInput;
    else if (this.crewed(0)) input = { ...input, stickX: input.turn || 0, stickY: input.pitch || 0, takeoff: input.up || input.down, boost: input.boost, bot: true };
    stepFlight(this, dt, input);
    const request = this.crewed(0) && !networkInput ? this.flightLockId < 0 : input.targetLock;
    updateFlightTarget(this, dt, request);
  }

  // Bot seats turn their aim by their analogs: the turn analog at the seat's
  // TurnRate (positive turns right, as for soldiers), the pitch analog at
  // its PitchRate, within the seat's limits, as the player's look is. Seat
  // 0's turn is the hull's steering, which steer() applies.
  aimCrew(dt) {
    for (let i = 0; i < this.seats.length; i++) {
      const seat = this.seats[i];
      const lease = seat.lease;
      if (!lease || lease.human || lease.unit?.networkHuman) continue;
      const input = lease.input;
      const settings = seat.settings || {};
      if (i > 0) seat.aimYaw -= input.turn * num(settings.TurnRate, num(this.def.settings.TurnRate, this.turnRate)) * dt;
      if (this.flying && i === 0) continue;
      const pitchRate = num(settings.PitchRate, num(this.def.settings.PitchRate, 1));
      seat.pitch += input.pitch * pitchRate * dt;
      this.clampAim(i);
    }
  }

  // The pose at the start of a world tick, for drawing between ticks.
  captureTick() {
    (this.tickFrom ||= new THREE.Vector3()).copy(this.position);
    if (this.flying) (this.flightTickQ ||= new THREE.Quaternion()).copy(this.quaternion);
    this.tickFromYaw = this.yaw; this.tickFromTilt = this.tilt; this.tickFromRoll = this.roll;
    for (const wheel of this.wheels) wheel.tickFrom = wheel.phase;
  }

  // Every frame: the hull and wheels between the last two world ticks
  // (render() poses them only at a tick, so they moved in 60 Hz steps on
  // any faster display), or where its record's sample puts it, and every
  // bone as drawn (poseCrew). Nothing on a page runs aimTick, so this is
  // also how the hull he drives is posed: its walk cycle, its manned seats
  // and its gear. dt, the frame's, advances its age, the clock of what is
  // only drawn (the idle clip, the repulsor's bob, a rider's clip), which no
  // record carries. Gameplay reads the tick pose; see restoreTickPose.
  present(alpha, dt = 0) {
    if (!this.alive) return this.dying > 0 && this.presentDying();
    if (this.deathFrom) this.deathFrom.taken = false;
    const networkPose = this.networkPose;
    if (!networkPose && (!this.tickFrom || this.sleeping)) return false;
    this.age += dt;
    if (networkPose) this.visual.fromArray(networkPose.position);
    else this.visual.lerpVectors(this.tickFrom, this.position, alpha);
    this.root.position.copy(this.visual);
    const hover = this.repulsor && !this.flying;
    const phase = this.age * 1.7 + this.hoverPhase;
    if (hover) this.root.position.y += Math.sin(phase) * .07 + Math.sin(phase * .61) * .025;
    if (networkPose && this.flying) this.drawFlyer(this.q.fromArray(networkPose.quaternion), this.roll);
    else if (networkPose) {
      // A replicated ground hull's quaternion is its heading alone (syncBody);
      // its lean arrives as tilt and roll with the crew pose.
      const q = this.q.fromArray(networkPose.quaternion);
      this.root.rotation.set(-this.tilt, Math.atan2(2 * (q.x * q.z + q.w * q.y), 1 - 2 * (q.y * q.y + q.z * q.z)), this.roll + (hover ? Math.sin(phase * .73) * .006 : 0), 'YXZ');
    }
    else if (this.flying) this.drawFlyer(this.q.slerpQuaternions(this.flightTickQ || this.quaternion, this.quaternion, alpha), THREE.MathUtils.lerp(this.tickFromRoll ?? this.roll, this.roll, alpha));
    else this.root.rotation.set(-THREE.MathUtils.lerp(this.tickFromTilt, this.tilt, alpha),
      this.tickFromYaw + angle(this.yaw - this.tickFromYaw) * alpha,
      this.tickFromRoll + angle(this.roll - this.tickFromRoll) * alpha + (hover ? Math.sin(phase * .73) * .006 : 0), 'YXZ');
    // From the bind pose (poseCrew starts there), so the wheels after it.
    this.poseCrew(networkPose ? networkPose.dt || 0 : dt, networkPose ? 1 : alpha, dt);
    if (this.wheels.length) {
      for (const wheel of this.wheels) {
        wheel.bone.quaternion.copy(this.bind.get(wheel.bone)).multiply(this.q.setFromAxisAngle(wheel.axis, networkPose ? wheel.phase : THREE.MathUtils.lerp(wheel.tickFrom ?? wheel.phase, wheel.phase, alpha)));
      }
      this.root.updateMatrixWorld(true);
    }
    return true;
  }

  // A dying walker as drawn: where it was destroyed, its bones in its death
  // animation at the time it has been dying (at its AnimationSpeed, holding
  // the last frame). Every host that draws calls it every frame from the
  // replicated dying clock.
  presentDying() {
    this.root.visible = true;
    this.root.position.copy(this.position);
    if (!this.flying) this.root.rotation.set(-this.tilt, this.yaw, this.roll, 'YXZ');
    if (this.pose) {
      // The pose it was last drawn in alive (mid-stride, its guns aimed),
      // which the clip takes over from below.
      const from = this.deathFrom ||= { taken: false, q: this.bones.map(() => new THREE.Quaternion()), t: this.bones.map(() => new THREE.Vector3()) };
      if (!from.taken) {
        from.taken = true;
        for (let i = 0; i < this.bones.length; i++) { from.q[i].copy(this.bones[i].quaternion); from.t[i].copy(this.bones[i].position); }
      }
      for (let i = 0; i < this.bones.length; i++) {
        this.bones[i].quaternion.copy(this.bindQuaternions[i]);
        this.bones[i].position.copy(this.bindPositions[i]);
      }
      const clip = this.clips.get('death01');
      const span = this.def.fx?.dying?.span || 0;
      if (clip && span > 0) {
        this.pose.clear();
        this.pose.add(clip, (span - this.dying) * (clip.frames / clip.fps / span), 1, false);
        this.pose.apply(this.bones);
      }
      // Over its first DEATH_BLEND s (an eighth of a shorter clip) the clip
      // takes over from that pose, so a walker destroyed mid-stride does not
      // snap its legs into the clip's first frame (the AT-TE's hind feet
      // jumped 0.6 m). Timed by the dying clock; one first seen dying later
      // (a late joiner) is past it.
      const blend = span > 0 ? Math.min(Math.max((span - this.dying) / Math.min(DEATH_BLEND, span / 8), 0), 1) : 1;
      if (blend < 1) {
        const w = blend * blend * (3 - 2 * blend);
        for (let i = 0; i < this.bones.length; i++) {
          const bone = this.bones[i];
          bone.quaternion.copy(this.q.copy(from.q[i]).slerp(bone.quaternion, w));
          bone.position.lerpVectors(from.t[i], bone.position, w);
        }
      }
    }
    this.root.updateMatrixWorld(true);
    return true;
  }

  // The drawn pose of a hull whose root present() placed: every bone from
  // the walk cycle (replicated, or his own hull's predicted one), the manned
  // seats' turret nodes from their aims, and their aimers turning toward
  // that aim at their own rates (presentation only; the authoritative aimers
  // live in the simulation host). The equivalent of render() without
  // placeRoot(). His own predicted hull is drawn between its last two ticks
  // (alpha), and so are its legs: the replay keeps the cycle as it was
  // before its last tick (walkFromPhase, src/runtime/vehicle-prediction.js),
  // so a planted foot stays where it is while the hull moves over it.
  // frameDt, the frame's: a craft drawn from its records moves its gear by
  // it (src/flight/gear.js).
  poseCrew(dt, alpha = 1, frameDt = dt) {
    if (!this.alive) return;
    for (let i = 0; i < this.bones.length; i++) this.bones[i].quaternion.copy(this.bindQuaternions[i]);
    if (this.pose) for (let i = 0; i < this.bones.length; i++) this.bones[i].position.copy(this.bindPositions[i]);
    let phase = this.walkPhase, blend = this.walkBlend;
    const length = this.walkLength;
    if (this.walkFromPhase !== undefined && alpha < 1 && length > 0) {
      const step = angle((phase - this.walkFromPhase) / length * 2 * Math.PI) / (2 * Math.PI) * length;
      phase = ((this.walkFromPhase + step * alpha) % length + length) % length;
      blend = THREE.MathUtils.lerp(this.walkFromBlend, blend, alpha);
    }
    // The cycle as drawn, on the hull itself (not a prediction copy): its
    // feet come down by it (src/vehicle-feet.js).
    const shown = this.predictionSource || this;
    shown.shownWalkPhase = phase; shown.shownWalkBlend = blend;
    this.feelLegs(dt);
    this.applyPose(phase, blend);
    poseFlightGear(this, frameDt);
    for (let i = 0; i < this.seats.length; i++) if (this.held(i)) this.poseSeat(i, dt);
    this.recoilBarrels();
    this.root.updateMatrixWorld(true);
  }

  // A craft is drawn leaned into its turn, its bank (the roll field) about
  // its nose, over the frame it flies by (flightFrameQ), which its chase
  // camera follows; as the 2004 game draws a flyer.
  drawFlyer(frame, bank) {
    (this.flightFrameQ ||= new THREE.Quaternion()).copy(frame);
    this.root.quaternion.copy(frame).multiply((this.flightLeanQ ||= new THREE.Quaternion()).setFromAxisAngle(NOSE, bank || 0));
  }

  // The hull at a pose between the last two ticks (alpha), with the
  // repulsor's bob, and its wheels.
  placeRoot(alpha) {
    this.visual.lerpVectors(this.previous, this.position, alpha);
    this.root.position.copy(this.visual);
    const hover = this.repulsor && !this.flying;
    const phase = this.age * 1.7 + this.hoverPhase;
    const bob = hover ? Math.sin(phase) * .07 + Math.sin(phase * .61) * .025 : 0;
    this.root.position.y += bob;
    if (this.flying) this.drawFlyer(this.q.slerpQuaternions(this.flightPreviousQ || this.quaternion, this.quaternion, alpha), THREE.MathUtils.lerp(this.previousRoll ?? this.roll, this.roll, alpha));
    else this.root.rotation.set(-THREE.MathUtils.lerp(this.previousTilt, this.tilt, alpha),
      this.previousYaw + angle(this.yaw - this.previousYaw) * alpha,
      this.previousRoll + angle(this.roll - this.previousRoll) * alpha + (hover ? Math.sin(phase * .73) * .006 : 0), 'YXZ');
    for (let i = 0; i < this.bones.length; i++) this.bones[i].quaternion.copy(this.bindQuaternions[i]);
    if (this.pose) for (let i = 0; i < this.bones.length; i++) this.bones[i].position.copy(this.bindPositions[i]);
    for (const wheel of this.wheels) {
      wheel.bone.quaternion.copy(this.bind.get(wheel.bone)).multiply(this.q.setFromAxisAngle(wheel.axis, THREE.MathUtils.lerp(wheel.previous, wheel.phase, alpha)));
    }
  }

  // The walk cycle advanced by a tick (walkPhase, walkBlend: plain, so a
  // page showing a worker's battle poses from them).
  walk(dt) {
    if (!this.pose || dt <= 0) return;
    const speed = Math.hypot(this.velocity.x, this.velocity.z);
    const left = this.clips.get('walk_leftfoot_rightfoot'), right = this.clips.get('walk_rightfoot_leftfoot');
    const length = (left?.duration || 0) + (right?.duration || 0);
    const moving = speed > .05 && left && right && length > 0;
    this.walkBlend += ((moving ? 1 : 0) - this.walkBlend) * (1 - Math.exp(-8 * dt));
    if (this.walkBlend > .0001 && left && right && length > 0) {
      const natural = Math.max(.1, (left.stride + right.stride) / length);
      const forwardSpeed = this.velocity.x * Math.sin(this.yaw) + this.velocity.z * Math.cos(this.yaw);
      const direction = this.creature && forwardSpeed < 0 ? -1 : 1;
      this.walkPhase = ((this.walkPhase + dt * speed / natural * direction) % length + length) % length;
    }
  }

  // The walk cycle's length in seconds (both steps), 0 without one.
  get walkLength() {
    const left = this.clips?.get('walk_leftfoot_rightfoot'), right = this.clips?.get('walk_rightfoot_leftfoot');
    return left && right ? left.duration + right.duration : 0;
  }

  // The bones from the walk cycle: the idle clip at the hull's age under
  // the walk clips at walkPhase, by walkBlend (or at a phase and blend as
  // drawn between ticks, poseCrew).
  applyPose(phase = this.walkPhase, blend = this.walkBlend) {
    if (!this.pose) return;
    const left = this.clips.get('walk_leftfoot_rightfoot'), right = this.clips.get('walk_rightfoot_leftfoot');
    const length = (left?.duration || 0) + (right?.duration || 0);
    this.pose.clear();
    this.pose.add(this.clips.get('idle'), this.age, 1 - blend);
    if (blend > .0001 && left && right && length > 0) {
      this.pose.add(phase < left.duration ? left : right, phase < left.duration ? phase : phase - left.duration, blend, false);
    }
    this.pose.apply(this.bones);
    this.liftLegs();
  }

  // Each leg pair's legs drawn toward the pose with the foot on the higher
  // ground lifted (leftfoot_up or rightfoot_up), by how much higher it is
  // (legSide; the 2004 game's leg blend over the bones under the
  // pair's LegBoneLeft and LegBoneRight). The 2004 game reads the weight
  // off a table it calibrates at spawn; the port uses legSide itself.
  liftLegs() {
    const legs = this.def.fx?.legs, sides = this.legSide;
    if (!legs || !sides || !this.clips) return;
    const up = { left: this.clips.get('leftfoot_up'), right: this.clips.get('rightfoot_up') };
    if (!up.left || !up.right) return;
    this.legPose ||= new Pose(this.bones.length, this.pose.root);
    this.legBones ||= legs.pairs.map((pair) => {
      const under = new Set();
      for (const which of ['left', 'right']) this.byName[pair[which].leg]?.traverse((b) => under.add(b));
      return this.bones.map((b, i) => (under.has(b) ? i : -1)).filter((i) => i >= 0);
    });
    const p = this.legPose, q = this.frameScratch.q2;
    legs.pairs.forEach((pair, n) => {
      const side = sides[n] || 0, weight = Math.min(1, Math.abs(side));
      if (weight < 1e-3) return;
      p.clear();
      p.add(side < 0 ? up.left : up.right, 0, 1, false);
      for (const i of this.legBones[n]) {
        const w = p.w[i];
        if (w <= 1e-6) continue;
        const bone = this.bones[i];
        bone.quaternion.slerp(q.fromArray(p.q, i * 4).normalize(), weight);
        bone.position.lerp(this.frameScratch.v.set(p.t[i * 3] / w, p.t[i * 3 + 1] / w, p.t[i * 3 + 2] / w), weight);
      }
    });
  }

  // Each leg pair's ground under its feet, for a page drawing a hull it
  // does not simulate: legSide from the drawn root, as standOnLegs keeps it.
  feelLegs(dt) {
    const legs = this.def.fx?.legs;
    if (!legs || this.creature || !(dt > 0)) return;
    const c = this.system.collision, L = legs.rayLength;
    const q = this.frameScratch.q.setFromEuler(this.legEuler.set(this.root.rotation.x, this.root.rotation.y, this.root.rotation.z, 'YXZ'));
    this.legSide ||= legs.pairs.map(() => 0);
    const enabled = this.body.enabled;
    this.body.enabled = false;
    try {
      legs.pairs.forEach((pair, i) => {
        const h = ['left', 'right'].map((which) => {
          this.from.fromArray(pair[which].at).applyQuaternion(q).add(this.root.position);
          this.from.y += L;
          const hit = c.cast(this.from, DOWN, 2 * L, this.hit);
          return hit ? L - hit.distance : -L;
        });
        this.legSide[i] += (THREE.MathUtils.clamp(-.6 * (h[0] - h[1]), -1, 1) - this.legSide[i]) * Math.min(1, .4 * dt * 60);
      });
    } finally { this.body.enabled = enabled; }
  }

  // Whether a seat's turret node, an aimer, a fire point or a rider's node
  // hangs from a bone the walk clips animate (the AT-AT's head under its
  // bobbing root): then the walk pose is authoritative and the tick
  // applies it; otherwise only the page does.
  get aimedByPose() {
    if (this.poseAims !== undefined) return this.poseAims;
    let result = false;
    if (this.pose && this.clips) {
      const animated = new Set();
      for (const clip of this.clips.values()) for (const joint of clip.tracks || []) if (joint >= 0) animated.add(this.bones[joint]);
      const hangs = (node) => { for (let n = node; n && n !== this.root; n = n.parent) if (animated.has(n)) return true; return false; };
      for (const seat of this.seats) {
        if (seat.view.turret && hangs(seat.view.turret) || seat.view.rider?.node && hangs(seat.view.rider.node)) result = true;
        for (const w of seat.weapons) {
          for (const a of w.aimers) if (a.bone && hangs(a.bone) || a.FirePointName && hangs(this.byName[a.FirePointName.toLowerCase()])) result = true;
          const fire = this.byName[String(w.def.FirePointName || '').toLowerCase()];
          if (fire && hangs(fire)) result = true;
        }
      }
    }
    // Kept off the plain fields (not enumerable): each host computes it.
    Object.defineProperty(this, 'poseAims', { value: result, enumerable: false, writable: true });
    return result;
  }

  // The tick's part of the pose (docs/runtime-architecture.md "Vehicle.render
  // split"), run in both hosts: the hull placed at the tick's pose, its age
  // and walk cycle advanced, the walk pose applied only where a seat's
  // turret, aimer, fire point or rider hangs from an animated bone, then
  // each manned seat's aim target (aimSeats) and its turret node and aimer
  // angles (poseSeat, which refreshes the matrices of just those chains),
  // and one pass over the hierarchy for the muzzles, eyes and riders. The
  // drawn pose is render's, on the page.
  aimTick(alpha, dt) {
    if (!this.alive) return;
    if (dt > 0 && this.sleeping && !this.repulsor && !this.occupied && !this.pose && this.health >= this.maxHealth * .35) return;
    this.placeRoot(alpha);
    this.walk(dt);
    if (this.aimedByPose) this.applyPose();
    // A craft's gear, doors and foils at its own gear, before its aimers
    // turn: an X-wing's lasers ride its S-foils and the MAF's pilot cannons
    // its flaps (src/flight/gear.js), so their turn is solved, and their fire
    // points read, with the foils where the gear has them, whether or not a
    // render pass poses the hull after this.
    if (this.flying) poseGearAt(this, this.gear ?? 1);
    this.age += dt;
    if (dt > 0) this.aimSeats(dt);
    this.poseSeat(this.seatIndex, dt);
    for (let i = 0; i < this.seats.length; i++) if (i !== this.seatIndex && this.crewed(i)) this.poseSeat(i, dt);
    this.recoilBarrels();
    this.root.updateMatrixWorld(true);
  }

  // The drawn pose (the page's; also the simulation host's after aimTick):
  // the hull between ticks, every bone from the walk cycle, the manned
  // seats' turret nodes and aimer bones from their angles (no integration:
  // dt is 0 on a page showing a worker's battle). Its effects and sounds
  // are the page's, from the hull's state (src/vehicle-effects.js,
  // src/audio/vehicles.js).
  render(alpha, dt) {
    if (!this.alive) return;
    if (dt > 0 && this.sleeping && !this.repulsor && !this.occupied && !this.pose && this.health >= this.maxHealth * .35) return;
    this.placeRoot(alpha);
    this.feelLegs(dt);
    this.applyPose();
    poseFlightGear(this);
    this.poseSeat(this.seatIndex, 0);
    for (let i = 0; i < this.seats.length; i++) if (i !== this.seatIndex && this.crewed(i)) this.poseSeat(i, 0);
    this.recoilBarrels();
    this.root.updateMatrixWorld(true);
  }

  // The chase camera's turn for a seat: along the seat's aim, its up the
  // seat frame's, pitched down by TiltValue (a camera looks along its -Z).
  cameraTurn(i, frame, out) {
    const s = this.frameScratch;
    this.seatForward(i, s.v).negate();
    lookBasis(s.v, s.up.setFromMatrixColumn(frame, 1), out);
    return out.multiply(s.q.setFromAxisAngle(RIGHT, -this.seats[i].view.tilt));
  }

  // Where a seat's chase camera rests, walls and easing aside: TrackCenter in
  // the seat's frame, then TrackOffset (right, up, back) in the camera's.
  cameraRest(i, frame, out) {
    const view = this.seats[i].view;
    this.cameraTurn(i, frame, this.frameScratch.q2);
    return out.copy(view.offset).applyQuaternion(this.frameScratch.q2).add(this.frameScratch.v.copy(view.center).applyMatrix4(frame));
  }

  // The point each manned seat's guns turn to, the original game's aim target:
  // the first thing along the aim within 512 m, else 512 m out. The player's
  // aim runs from his eye in first person, else from his chase camera
  // brought forward to the eye's plane, so it is the point under his
  // crosshair. The original game starts it where the camera rests; the port
  // starts it where his camera is (aimCamera, the simulation's view), walls
  // and easing included, so the round still lands under the drawn
  // crosshair when a wall pulls the camera in. A bot's aim runs along its
  // own aim from the seat's eye. Also how fast each seat's aim turns, for
  // the rider's 9-pose.
  aimSeats(dt) {
    const s = this.frameScratch;
    for (let i = 0; i < this.seats.length; i++) {
      const seat = this.seats[i];
      if (seat.lastYaw !== null) seat.yawRate += (angle(seat.aimYaw - seat.lastYaw) / dt - seat.yawRate) * (1 - Math.exp(-12 * dt));
      seat.lastYaw = seat.aimYaw;
      const player = Boolean(this.driver) && i === this.seatIndex;
      if (!player && !this.crewed(i)) continue;
      const origin = this.seatEye(i, this.from), direction = this.direction;
      if (player) {
        this.seatForward(i, direction);
        if (!this.viewFirstPerson) {
          const camera = this.aimCamera ? this.target.copy(this.aimCamera.position) : this.cameraRest(i, this.seatFrame(i, s.frame), this.target);
          const ahead = Math.max(0, this.point.subVectors(origin, camera).dot(direction));
          origin.copy(camera).addScaledVector(direction, ahead);
        }
      } else if (seat.lease.input.aiming) direction.copy(seat.lease.input.aim);
      else this.seatForward(i, direction);
      const hit = this.system.collision.cast(origin, direction, AIM_REACH, this.aimHit, this.body);
      let far = hit ? hit.distance : AIM_REACH;
      const unit = this.system.units?.raycast?.(origin, direction, far, null, this.unitHit);
      if (unit) far = unit.distance;
      seat.target = (seat.target || new THREE.Vector3()).copy(origin).addScaledVector(direction, far);
    }
  }

  // A seat's turret and guns. The turret node swings to the seat's aim (by
  // its YawTurnFactor and PitchTurnFactor), keeping its own up; then each
  // aimer turns toward the seat's target from its rest pose, within its
  // limits and at its rates, a chain's nodes in order. A barrel is on
  // target once its aimer points at the target from its pivot.
  poseSeat(i, dt) {
    const seat = this.seats[i], view = seat.view, s = this.frameScratch;
    if (view.turret) {
      lookBasis(aimVector(view.yawFactor * angle(seat.aimYaw - this.root.rotation.y), view.pitchFactor * seat.pitch, s.v), view.up, s.q2);
      view.turret.quaternion.copy(view.turret.parent.getWorldQuaternion(s.q).invert()).multiply(this.root.quaternion).multiply(s.q2);
      view.turret.updateMatrixWorld(true);
    }
    const target = seat.target;
    for (const w of seat.weapons) {
      for (const a of w.aimers) {
        const bone = a.bone;
        if (!bone) continue;
        if (bone === view.turret) { a.facing.set(0, 0, 1).transformDirection(bone.matrixWorld); continue; }
        const rest = this.bind.get(bone);
        bone.parent.updateWorldMatrix(true, false);
        if (target) bone.parent.worldToLocal(s.v.copy(target)).sub(bone.position).applyQuaternion(s.q.copy(rest).invert());
        // Nothing to aim at: along the seat's aim.
        else aimVector(seat.aimYaw, seat.pitch, s.v).applyQuaternion(bone.parent.getWorldQuaternion(s.q).multiply(rest).invert());
        if (dt > 0) {
          // As the 2004 game's aimer turns: the
          // shorter way round toward the target at its rate, then held
          // inside its limits (0..2pi for an arc round the back), so a
          // full-circle mount (the spider walker's guns, -360..360) crosses
          // behind instead of unwinding. A step that had to be held back on
          // an axis puts the aimer outside its limits, where a weapon
          // without FireOutsideLimits fires nothing; an axis whose
          // limits are equal (the AT-TE's main cannon, -0 0) is pinned and
          // never counts against it.
          const step = a.yawRate * dt;
          let yaw = a.yaw + THREE.MathUtils.clamp(angle(Math.atan2(s.v.x, s.v.z) - a.yaw), -step, step);
          if (a.wrapYaw && yaw < 0) yaw += 2 * Math.PI;
          const pitch = approach(a.pitch, Math.atan2(s.v.y, Math.hypot(s.v.x, s.v.z)), a.pitchRate * dt);
          const [y0, y1] = a.yawLimits, [p0, p1] = a.pitchLimits;
          a.inLimits = (y0 === y1 || (yaw >= y0 && yaw <= y1)) && (p0 === p1 || (pitch >= p0 && pitch <= p1));
          a.yaw = THREE.MathUtils.clamp(yaw, y0, y1);
          a.pitch = THREE.MathUtils.clamp(pitch, p0, p1);
        }
        // Yaw about the node's up, then pitch about its side (up positive).
        const turn = this.aimQ.copy(rest).multiply(s.q.setFromAxisAngle(UP, a.yaw)).multiply(this.aimQ2.setFromAxisAngle(RIGHT, -a.pitch));
        a.facing.set(0, 0, 1).applyQuaternion(turn).transformDirection(bone.parent.matrixWorld);
        // A node another seat turns as its turret is that seat's while
        // someone holds it, else this aimer's (the port's choice: which of
        // the two the original game draws isn't traced). Either way the aimer
        // keeps its own aim, and its rounds follow that, not the node: the
        // original game fires along an aimer's own direction.
        a.posed = a.owner < 0 || !this.held(a.owner);
        if (a.posed) {
          bone.quaternion.copy(turn);
          bone.updateMatrixWorld(true);
        }
      }
      for (const a of w.aimers) {
        a.onTarget = false;
        if (!target || !a.bone) continue;
        a.bone.getWorldPosition(s.v).sub(target).negate();
        const span = s.v.length();
        a.onTarget = span > 1e-3 && s.v.dot(a.facing) > span * Math.cos(AIMED);
      }
    }
  }
  // Barrels kick back as they fire: BarrelRecoil along the
  // barrel's own z, times a kick that is 1 as it fires and decays at 4 a
  // second (cosmetic, on the clock of whoever draws it).
  recoilBarrels() {
    const now = clockSeconds();
    for (const seat of this.seats) for (const w of seat.weapons) for (const m of w.muzzles) {
      if (!m.barrel || !m.rest) continue;
      const kick = m.kickAt === undefined ? 0 : Math.max(0, 1 - 4 * (now - m.kickAt));
      m.barrel.position.copy(m.rest);
      if (kick > 0 && m.recoil) m.barrel.position.addScaledVector(this.recoilAxis.set(0, 0, 1).applyQuaternion(m.barrel.quaternion), -m.recoil * kick);
    }
  }

  // A shot another host fired (a network record's muzzle point): the barrel
  // of that weapon nearest the point kicks back.
  kickBarrel(weapon, point) {
    for (const seat of this.seats) for (const w of seat.weapons) {
      if (w.state !== weapon) continue;
      let best = null, span = Infinity;
      for (const m of w.muzzles) {
        if (!m.barrel) continue;
        const d = m.fireNode.getWorldPosition(this.recoilPoint).distanceToSquared(point);
        if (d < span) { span = d; best = m; }
      }
      if (best) best.kickAt = clockSeconds();
    }
  }

  // Someone holds seat i: the player or a lease.
  held(i) { return Boolean(this.seats[i]?.lease) || Boolean(this.driver) && this.seatIndex === i; }

  fire(dt, controls) {
    const primary = controls.take('firePrimary'), secondary = controls.take('fireSecondary');
    const reload = controls.take('reload');
    for (let i = 0; i < this.seats.length; i++) {
      // A seat a bot holds fires from the bot's own input (fireSeat).
      if (i !== this.seatIndex && this.crewed(i)) continue;
      for (let channel = 0; channel < this.seats[i].weapons.length; channel++) {
        const w = this.seats[i].weapons[channel];
        const selected = i === this.seatIndex && (!this.flying || this.flightState === 'Flying');
        const pressed = channel === 0 ? primary : secondary;
        const held = controls.held(channel === 0 ? 'firePrimary' : 'fireSecondary');
        if (!w.state.random && this.system.random) w.state.random = this.system.random;
        if (selected && !this.flying && hasMissileLock(w.def)) updateWeaponLock(w.state, dt, { units: this.system.units, collision: this.system.collision,
          origin: this.seatEye(i, this.from), dir: this.reticleRay(i, this.from, this.direction), viewer: this.aiTarget, ignoreBody: this.body });
        const shots = w.state.update(dt, selected, selected && !this.towHeld(w) ? { pressed, held: held || pressed } : noTrigger, reload && selected, { posture: 'stand', stick: 0, zoomed: false });
        for (const shot of shots) {
          if (!this.alive) break;
          // From the barrel's real fire point toward the point under the
          // crosshair (aimSeats), which its aimers turn to.
          const aimer = this.muzzleOfSeat(i, w, this.muzzle);
          if (this.heldByLimits(i, aimer)) continue;
          this.shotDirection(aimer, this.muzzle, this.seats[i].target, null, this.direction);
          if (this.flying) { shot.targetId = this.flightLockState === 'Locked' ? this.flightLockId : -1; shot.targetGeneration = this.flightLockGeneration; }
          this.spread(shot, this.direction);
          this.emitShot(shot, i, null, this.shotEmitter);
        }
      }
    }
  }

  // A bot's seat fires from its frozen input: each channel's trigger, from
  // the weapon's real fire points toward the point along the bot's aim (its
  // intent.aim, the Classic aimer's output, or else the seat's own), as its
  // aimers turn to it. Rounds are the bot's.
  fireSeat(i, dt) {
    if (this.flying && this.flightState !== 'Flying') return;
    const seat = this.seats[i];
    const lease = seat.lease;
    const input = lease.input;
    const reload = input.takeReload();
    for (let channel = 0; channel < seat.weapons.length; channel++) {
      const w = seat.weapons[channel];
      const pressed = input.take(channel);
      if (!this.flying && hasMissileLock(w.def)) updateWeaponLock(w.state, dt, { units: this.system.units, collision: this.system.collision,
        origin: this.seatEye(i, this.from), dir: this.reticleRay(i, this.from, this.direction), viewer: this.aiTarget, ignoreBody: this.body,
        target: seatLockTarget(lease) });
      const shots = w.state.update(dt, true, this.towHeld(w) ? noTrigger : { pressed, held: Boolean(input.held[channel]) || pressed }, reload, { posture: 'stand', stick: 0, zoomed: false });
      for (const shot of shots) {
        if (!this.alive) break;
        const aimer = this.muzzleOfSeat(i, w, this.muzzle);
        if (this.heldByLimits(i, aimer)) continue;
        this.shotDirection(aimer, this.muzzle, seat.target, this.towAim(i, w, this.from) || this.botAim(i, this.from), this.direction);
        this.spread(shot, this.direction);
        this.emitShot(shot, i, lease, this.shotEmitter);
      }
    }
  }

  // A tow cable's gun takes no trigger while the cable it fired holds: the
  // 2004 game's WeaponTowCable answers its fire test no while its cable is
  // attached, so a second harpoon never takes the place of the
  // first (and never starts the walker's three circuits over).
  towHeld(w) {
    return w.def.ordnance?.base === 'towcable' && this.towTarget >= 0;
  }

  // A barrel whose aimer cannot turn to the point (it is past the aimer's
  // limits) fires no round unless the seat says FireOutsideLimits (the
  // AT-AT's chin cannons); its turn passes all the same.
  heldByLimits(i, aimer) {
    if (!aimer?.bone || aimer.inLimits !== false) return false;
    return num(aimer.FireOutsideLimits ?? this.seats[i].settings?.FireOutsideLimits, 0) === 0;
  }

  // Seat i's reticle line from `from` (its eye): through the point under the
  // reticle (aimSeats: from the player's camera, a bot's eye), else along
  // the seat's aim. A launcher's lock cone runs along it.
  reticleRay(i, from, out) {
    const target = this.seats[i].target;
    if (target && out.subVectors(target, from).lengthSq() > 1e-6) return out.normalize();
    return this.seatForward(i, out);
  }

  // A bot's aim in seat i: its intent's, else the seat's own.
  botAim(i, out) {
    const input = this.seats[i].lease?.input;
    return input?.aiming ? out.copy(input.aim) : this.seatForward(i, out);
  }

  // The weapon's next muzzle (barrels take turns): writes its fire point in
  // the world to out, keeps its node (this.shotNode) and what a beam from it
  // follows (this.shotEmitter; null: the seat's own), and returns its aimer.
  muzzleOf(w, out) {
    const aimers = w.muzzles;
    const aimer = aimers.length ? aimers[w.barrel++ % aimers.length] : w.aimers[0];
    if (aimer?.barrel) aimer.kickAt = clockSeconds();
    const node = aimer?.fireNode || this.byName[(aimer?.FirePointName || aimer?.node || w.def.FirePointName || '').toLowerCase()];
    this.shotNode = node || null;
    this.shotEmitter = aimer?.emitter ?? w.emitter;
    if (node) node.getWorldPosition(out);
    else this.nose(out);
    return aimer?.sourceAimer || (aimer?.bone ? aimer : null);
  }
  // A seat's weapon's muzzle (muzzleOf), or, for a weapon with no fire point
  // on the model, the seat's own turret node: the snowspeeder's rear gun
  // fires from towcablegun, aft, not from the hull's nose.
  muzzleOfSeat(i, w, out) {
    const aimer = this.muzzleOf(w, out);
    const turret = this.seats[i]?.view.turret;
    if (!this.shotNode && turret) { this.shotNode = turret; turret.getWorldPosition(out); }
    return aimer;
  }
  // A bot firing the tow cable aims the harpoon at the nearest leg of the
  // walker it is after (its seat's target, else the walker its tow pattern
  // circles); null when the shot is not a tow or there is no walker.
  towAim(i, w, out) {
    if (w.def.ordnance?.base !== 'towcable') return null;
    const seat = this.seats[i];
    const owner = this.system.collision.cast(this.seatEye(i, this.from), this.botAim(i, this.direction), Math.max(20, w.def.LockOnRange || 0), this.hit, this.body)?.body?.owner;
    const walker = owner instanceof Vehicle && owner.base === 'commandwalker' ? owner : this.system.nearestWalker(this, Math.max(20, w.def.LockOnRange || 0));
    if (!walker || !walker.legs.length) return null;
    let best = null, span = Infinity;
    for (const leg of walker.legs) {
      walker.legPoint(leg.name, this.point);
      const d = this.point.distanceToSquared(this.muzzle);
      if (d < span) { span = d; best = leg; }
    }
    walker.legPoint(best.name, this.point);
    return out.subVectors(this.point, this.muzzle).normalize();
  }
  // Where a weapon with no fire point node fires from: the hull's nose.
  nose(out) {
    return out.set(this.center.x, this.center.y + .5, this.center.z + this.size.z * .5).applyMatrix4(this.matrix);
  }

  // Where a round from a barrel's fire point goes. A barrel on its aimers
  // fires where they point: straight at the seat's target once it points
  // there, so the round lands under the crosshair; along the barrel while it
  // is still turning or held at a limit (along the aimer's own aim when a
  // turret seat has its node). A weapon with no aimer node fires along
  // fallback (a bot's aim) when given, else at the target.
  shotDirection(aimer, from, target, fallback, out) {
    if (aimer?.bone) {
      if (aimer.onTarget && target) return out.subVectors(target, from).normalize();
      if (!aimer.posed) return out.copy(aimer.facing);
      const node = this.byName[(aimer.FirePointName || '').toLowerCase()] || aimer.bone;
      return out.set(0, 0, 1).transformDirection(node.matrixWorld);
    }
    if (fallback) return out.copy(fallback);
    if (target) return out.subVectors(target, from).normalize();
    return this.viewDirection(out);
  }

  // Where a beam from one of seat i's barrels (its fire point's node and its
  // aimer; the hull's nose for neither) points while it lasts: as that
  // barrel shoots now.
  beamDirection(i, node, aimer, out) {
    const from = node ? node.getWorldPosition(this.beamFrom) : this.nose(this.beamFrom);
    return this.shotDirection(aimer, from, this.seats[i].target, this.crewed(i) ? this.botAim(i, out) : null, out);
  }

  spread(shot, direction) {
    if (shot.yaw) direction.applyAxisAngle(UP, shot.yaw);
    if (shot.pitch) {
      this.normal.crossVectors(direction, UP).normalize();
      direction.applyAxisAngle(this.normal, shot.pitch);
    }
  }

  // The shot leaves this.muzzle along this.direction. The player's rounds
  // stay the vehicle's (as before); a bot's are credited to the bot, still
  // passing through the hull's own body, and a beam follows its barrel
  // (emitter, from muzzleOf; else the seat's own, on the hull).
  emitShot(shot, seatIndex, lease, emitter = null) {
    if (this.flying) { shot.targetId = this.flightLockState === 'Locked' ? this.flightLockId : -1; shot.targetGeneration = this.flightLockGeneration; }
    if(this.flying && seatIndex===0) assistShot(this,this.direction,this.muzzle);
    const def = shot.weapon.def;
    const owner = lease?.unit ?? this.seats[seatIndex].lease?.unit ?? this;
    if (def.ordnance?.base === 'towcable') this.system.launchTow(this, def, this.muzzle, this.direction, seatIndex, lease ?? this.seats[seatIndex].lease);
    else if (def.ordnance) {
      const round = this.system.ordnance.fire(def.ordnance, this.muzzle, this.direction, owner, null, this.body, emitter ?? this.seats[seatIndex].emitter, shot);
      if (round && this.flying && round.kind === 'missile') { round.flightMissile = true; round.flightClass=this.flightClass; round.life=this.flightConfig.missileLifetime; round.velocity.normalize().multiplyScalar(this.flightConfig.missileSpeed); }
      const weapons = this.seats[seatIndex].weapons, index = weapons.findIndex(w => w.state === shot.weapon);
      const weapon = weapons[index];
      if (weapon?.muzzles.length) bindMountedRound(round, this, seatIndex, index, (weapon.barrel - 1) % weapon.muzzles.length);
    }
    emitSound("turretFire", this, shot);
    if (def.MuzzleFlashEffect) this.system.effects?.play(def.MuzzleFlashEffect, this.muzzle);
    this.system.ordnance.lights.flash(this.muzzle, [1, .55, .2], 5, .06);
    this.system.onFired?.(this, seatIndex, shot, owner, this.muzzle, this.direction);
  }
}

export class Vehicles {
  // Optional hooks for the simulation's event journal and the AI:
  // onBoard(unit, vehicle, seat), onUnboard(unit, vehicle, seat, post,
  // reason), onMove(unit, vehicle, seat) (a bot moved to another seat),
  // onFired(vehicle, seat, shot, owner, muzzle, direction),
  // onDamaged(vehicle, amount, attacker). units: the registry (src/registry.js)
  // to enter every hull in as a target (or call attachUnits later).
  constructor({ scene, data, buffer, indexBuffer, animationBuffer = new ArrayBuffer(0), collision, water, audio, maxFlyHeight = 200, texture, caster, uncaster, camera, ordnance, effects, posts, inDeathRegion, onDestroyed, units = null, onBoard = null, onUnboard = null, onFired = null, onDamaged = null, flightEnvironment = {} }) {
    Object.assign(this, { scene, data, collision, water, audio, maxFlyHeight, camera, caster, uncaster, ordnance, effects, posts, inDeathRegion, onDestroyed, onBoard, onUnboard, onFired, onDamaged });
    const mapBounds = collision.bvh?.getBoundingBox(new THREE.Box3());
    if (mapBounds && collision.terrain) mapBounds.union(collision.terrain.getBoundingBox(new THREE.Box3()));
    this.flightEnvironment = { ceiling: maxFlyHeight, ...(mapBounds && !mapBounds.isEmpty() ? { bounds: { min: [mapBounds.min.x, -300, mapBounds.min.z], max: [mapBounds.max.x, maxFlyHeight, mapBounds.max.z] } } : {}), ...flightEnvironment };
    this.units = null;
    // Team relations for boarding (src/boarding.js): the registry's, with
    // its alliances, once attached; else by team alone.
    this.relation = (a, b) => (this.units?.relation ? this.units.relation(a, b) : teamRelation(a, b));
    this.time = 0; // s of vehicle updates
    this.ticks = 0; // input batches so far
    this.pendingExits = [];
    this.exitSpot = { position: new THREE.Vector3(), forward: new THREE.Vector3(), candidate: -1 };
    this.riderPoint = new THREE.Vector3();
    this.rider = { point: new THREE.Vector3(), riderOffset: new THREE.Vector3(), riderQuaternion: new THREE.Quaternion(),
      points: makeRiderPoints(), frame: new THREE.Matrix4(), sample: { clip: null, pitch: 0, turn: 0, time: null } };
    this.safeScratch = { hit: makeHit(), target: new THREE.Vector3(), direction: new THREE.Vector3() };
    this.templates = new Map();
    const pack = modelPack(Object.fromEntries(Object.entries(data.models).map(([k, m]) => [k, m.parts])), buffer, indexBuffer);
    for (const [name, rig] of Object.entries(data.models)) {
      const col = data.collision[name];
      const shape = collision.createBodyShape(new Float32Array(col.positions), new Uint32Array(col.indices), new Uint32Array(col.polygons));
      const parts = pack.get(name);
      const wheelBounds = new Map(), wheelRadii = new Map();
      for (const part of parts) {
        const bone = part.bone?.toLowerCase();
        if (bone !== 'wheel_left' && bone !== 'wheel_right') continue;
        part.geometry.computeBoundingBox();
        if (!wheelBounds.has(bone)) wheelBounds.set(bone, new THREE.Box3());
        wheelBounds.get(bone).union(part.geometry.boundingBox);
      }
      for (const [bone, bounds] of wheelBounds) wheelRadii.set(bone, Math.max(bounds.max.x - bounds.min.x, bounds.max.z - bounds.min.z) * .5);
      const muzzleTips = vehicleMuzzleTips(rig, parts);
      const batches = vehicleBatches(rig, parts, texture);
      const clips = data.animations?.[name] ? loadClips(data.animations[name], animationBuffer, rig.bones.map(b => b.name)) : null;
      // Walker banks contain an initial reference record and then the actual
      // animated record for the same joint. Only the last record drives it;
      // accumulating both halves root motion and lifts the planted feet.
      for (const clip of clips?.values() || []) {
        const seen = new Set();
        for (let i = clip.tracks.length - 1; i >= 0; i--) {
          const joint = clip.tracks[i];
          if (seen.has(joint)) clip.tracks[i] = -1;
          else seen.add(joint);
        }
      }
      // A flyer with a takeoff clip stands on its gear (src/flight/gear.js).
      const takeoff = clips?.get('takeoff');
      this.templates.set(name, { rig, batches, shape, clips, wheelRadii, muzzleTips, gearFoot: takeoff ? gearFoot(rig, batches, takeoff) : null });
    }
    if (data.blockers?.indices.length) {
      const b = data.blockers;
      collision.vehicleBlockers = collision.createBodyShape(new Float32Array(b.positions), new Uint32Array(b.indices), new Uint32Array(b.polygons));
    }
    this.slots = data.spawns.map(desc => ({ desc, matrix: new THREE.Matrix4().fromArray(desc.matrix), remaining: 0, vehicle: null, variants: new Map() }));
    this.vehicles = [];
    this.acc = 0;
    this.tethers = new Map();
    this.vanished = new Map(); // drawing only: crew who died hidden in their hull (presentRiders)
    this.hit = makeHit();
    this.center = new THREE.Vector3();
    this.direction = new THREE.Vector3();
    this.updateSpawns(0);
    if (units) this.attachUnits(units);
  }

  // Every hull, now and later, as a target in the units registry
  // (VehicleTarget: kind 2 walker, 5 flyer, 6 hover; no hit spheres).
  attachUnits(units) {
    this.units = units;
    for (const v of this.vehicles) if (units.byId(v.aiTarget.id) !== v.aiTarget) units.add(v.aiTarget);
  }

  updateSpawns(dt) {
    for (const slot of this.slots) {
      if (slot.vehicle?.alive) continue;
      slot.remaining = Math.max(0, slot.remaining - dt);
      // A walker still dying keeps its pad until it is gone.
      if (slot.remaining > 0 || slot.vehicle?.dying > 0) continue;
      const post = slot.desc.post && this.posts?.posts.find(p => p.desc.name.toLowerCase() === slot.desc.post);
      const choices = Object.keys(slot.desc.classes);
      const team = post ? post.team : slot.desc.team || (choices.length === 1 ? Number(choices[0]) : 0);
      const name = slot.desc.classes[team];
      if (!name) continue; // neutral command posts do not manufacture vehicles
      const v = this.variant(slot, name);
      const position = this.center.setFromMatrixPosition(slot.matrix);
      const blocked = this.vehicles.some(other => other !== v && other.alive && vehicleFootprintsOverlap(v, slot.matrix, other, other.matrix));
      if (blocked || this.spawnOccupant && this.spawnOccupant.distanceToSquared(position) < (v.radius + 1) ** 2) { slot.remaining = 1; continue; }
      slot.vehicle = v;
      v.spawn(team);
    }
  }

  // The slot's hull of a class, made the first time it is wanted. A page
  // showing a worker's battle makes the same ones when the worker does
  // (src/runtime/replica.js).
  variant(slot, name) {
    let v = slot.variants.get(name);
    if (!v) {
      v = new Vehicle(this, slot, name); slot.variants.set(name, v); this.vehicles.push(v);
      if (this.units) this.units.add(v.aiTarget);
    }
    return v;
  }

  // Every frame, the vehicles drawn between the last two world ticks; dt
  // is the frame's (Vehicle.present).
  present(alpha, dt = 0) {
    for (const v of this.vehicles) if (v.present(alpha, dt)) this.presented = true;
    this.drawTethers(true);
  }

  // Before a world tick: the exact pose gameplay read at the last tick, so
  // drawing between ticks never changes what the next tick sees.
  restoreTickPose() {
    if (!this.presented) return;
    for (const v of this.vehicles) if (v.alive && v.tickFrom) v.render(this.tickAlpha ?? 1, 0);
    this.presented = false;
  }

  // driven/input: the player's hull and keys, as before. Bots' seats read
  // their own units' intents: one batch per update, frozen before the
  // substeps (so every substep sees the same held values) with each edge
  // taken once; their exits happen before the substeps, their weapons fire
  // after them, and their bodies follow their seats at the end.
  update(dt, driven = null, input = idle) {
    dt = THREE.MathUtils.clamp(dt, 0, .1);
    this.time += dt;
    this.updateSpawns(dt);
    this.latchCrews();
    for (const v of this.vehicles) if (v.alive && !v.flying) v.captureTick();
    this.flightAcc = (this.flightAcc || 0) + dt;
    this.flightEdges ||= {};
    for (const key of ['takeoff','trick','immelmann','targetLock']) this.flightEdges[key] ||= Boolean(input[key]);
    while (this.flightAcc + 1e-9 >= 1/60) {
      for (const v of this.vehicles) if (v.alive && v.flying) { v.captureTick(); v.tick(1/60, v.crewed(0) ? v.seats[0].lease.input : v === driven ? { ...input, ...this.flightEdges } : idle); }
      this.flightEdges = {}; this.flightAcc -= 1/60;
    }
    this.acc += dt;
    while (this.acc + 1e-9 >= FIXED_STEP) {
      for (const v of this.vehicles) if (v.alive && !v.flying) v.tick(FIXED_STEP, v.crewed(0) ? v.seats[0].lease.input : v === driven ? input : idle);
      this.acc -= FIXED_STEP;
    }
    for (const v of this.vehicles) if (!v.alive && v.dying > 0) v.dyingTick(dt);
    this.tickAlpha = Math.max(0, this.acc / FIXED_STEP);
    // The tick's pose in both hosts; the drawn one only where something
    // is drawn (a world worker leaves it to the page showing it).
    for (const v of this.vehicles) v.aimTick(v.flying ? 1 : this.tickAlpha, dt);
    if (this.cosmetic !== false) for (const v of this.vehicles) v.render(v.flying ? 1 : this.tickAlpha, dt);
    this.presented = false;
    this.fireCrews(dt);
    this.updateTethers();
    if (this.cosmetic !== false) this.drawTethers(false);
    this.syncCrews();
  }

  // The seat contract's input batch: stale leases are let go, every live
  // bot's intent is frozen, and exit requests (an intent's use edge, or
  // requestExit) are carried out.
  latchCrews() {
    this.ticks += 1;
    const exits = this.pendingExits;
    exits.length = 0;
    for (const v of this.vehicles) for (let i = 0; i < v.seats.length; i++) {
      const lease = v.seats[i].lease;
      if (!lease || lease.human) continue;
      if (!v.alive || !lease.live) { this.vacate(lease, { placement: false, promote: false, reason: 'stale' }); continue; }
      lease.input.latch(lease);
      if (lease.input.exit || lease.exitRequest) exits.push(lease);
    }
    for (const lease of exits) {
      if (lease.released) continue;
      const request = lease.exitRequest || {};
      this.vacate(lease, { placement: request.placement ?? true, promote: request.promote, post: request.post ?? null });
    }
    exits.length = 0;
  }

  fireCrews(dt) {
    for (const v of this.vehicles) {
      if (!v.alive) continue;
      for (let i = 0; i < v.seats.length; i++) if (v.crewed(i) && v.seats[i].weapons.length) v.fireSeat(i, dt);
    }
  }

  // Bots' bodies at their seats, posed as riders (enclosed ones hidden).
  syncCrews() {
    for (const v of this.vehicles) {
      if (!v.alive) continue;
      for (const seat of v.seats) if (seat.lease && !seat.lease.human) this.syncRider(seat.lease);
    }
  }

  syncRider(lease) {
    const unit = lease.unit;
    if (!unit?.soldier || !unit.actor) return;
    poseRider(lease.vehicle, lease.index, unit.soldier, unit.actor, unit.animator, unit.animator?.clips, unit.presentation, this.riderPoint);
    // The hull has moved under him: his hit shapes follow (Unit.updateSpheres).
    unit.tickCount += 1;
  }

  // After the units are posed for the frame: each bot's shown rider takes
  // its node's place and turn, or its saddle on an animal (seatRider; the
  // player's rider is VehicleDriver.presentRider). A seat with no rider
  // node is enclosed: its body stays hidden, enforced here every frame
  // because the one hide at seat time (poseRider) races lease setup on a
  // remote client, after which unit presentation went on drawing the
  // body at its last soldier position, outside the fleeing hull.
  //
  // Whoever sat hidden inside a hull when it was destroyed dies with it out
  // of sight: his body never shows (the hull's blast threw the AT-TE's crew
  // flailing 6 m above it). Only the drawing: the body still dies, falls
  // and is thrown as the world has it. He shows again in his next life.
  presentRiders(dt) {
    for (const [unit, generation] of this.vanished) {
      if (unit.generation !== generation || unit.removed) this.vanished.delete(unit);
      else if (unit.actor?.root) unit.actor.root.visible = false;
    }
    for (const v of this.vehicles) {
      if (!v.alive) {
        // The crew it was last drawn with, inside.
        for (const unit of v.insideCrew || []) if (unit.generation === v.insideCrewGenerations.get(unit)) {
          this.vanished.set(unit, unit.generation);
          if (unit.actor?.root) unit.actor.root.visible = false;
        }
        v.insideCrew = null;
        continue;
      }
      const inside = v.insideCrew ||= [];
      const generations = v.insideCrewGenerations ||= new Map();
      inside.length = 0; generations.clear();
      for (const seat of v.seats) {
        const unit = seat.lease?.unit;
        if (unit && !seat.lease.released && !seat.view.rider?.node) { inside.push(unit); generations.set(unit, unit.generation); }
      }
      for (const seat of v.seats) {
        const lease = seat.lease;
        if (!lease || lease.human || !lease.unit?.actor) continue;
        if (!seat.view.rider?.node) { lease.unit.actor.root.visible = false; continue; }
        if (lease.unit.alive && !lease.unit.removed) lease.unit.actor.root.visible = true;
        lease.lean = seatRider(v, lease.index, lease.unit.actor, dt, lease.lean, this.rider);
      }
    }
  }

  // What the player in `vehicle` is aiming at: the vehicle or turret (its
  // registry entry) or soldier under his view, out to 600 m, or null. The
  // Classic AI shares it with bot gunners in a walker or hover he drives
  // (the original game shares the pilot's weapon target; the reticle is the
  // port's stand-in, provisional).
  humanTarget(vehicle) {
    if (!vehicle?.alive || !vehicle.driver || !this.camera) return null;
    vehicle.viewDirection(this.direction);
    const from = this.camera.position;
    const hit = this.collision.cast(from, this.direction, 600, this.hit, vehicle.body);
    const far = hit ? hit.distance : 600;
    const unitHit = this.units?.raycast?.(from, this.direction, far, null);
    if (unitHit?.unit && unitHit.unit.alive) return unitHit.unit;
    const owner = hit?.body?.owner;
    return owner?.aiTarget?.alive ? owner.aiTarget : null;
  }

  // The seat a unit holds in one of these vehicles, or null.
  seatOf(unit) {
    const lease = unit?.seat;
    return lease && !lease.released && lease.vehicle?.system === this ? lease : null;
  }

  // The 2004 game's boarding eligibility for a bot or a network human, by
  // the shared rules (src/boarding.js boardingSeat): who holds the hull,
  // its team, the unit's class (NoEnterVehicles), a seat for him. Returns
  // the seat he would take, or -1. The player boards through VehicleDriver.
  eligibleSeat(unit, vehicle, seatIndex = -1) {
    if (!unit || !vehicle || vehicle.system !== this || (unit.human && !unit.networkHuman)) return -1;
    const person = Boolean(unit.networkHuman);
    if (!canBoard(unit, person)) return -1;
    return boardingSeat(vehicle, { team: unit.team, person, relation: this.relation, preferred: seatIndex });
  }

  // Seats a bot or a network human now (the AI has brought the bot to the
  // hull; the original game's enter callback has no reach test of its own).
  // seatIndex -1 takes the seat the rules give (eligibleSeat). A network
  // human takes a seat a bot holds as the player does; the bot moves on.
  // Returns whether it boarded.
  board(unit, vehicle, seatIndex = -1) {
    const index = this.eligibleSeat(unit, vehicle, seatIndex);
    if (index < 0 || seatIndex >= 0 && index !== seatIndex || !claimSeat(vehicle, index)) return false;
    const seat = vehicle.seats[index];
    const lease = new SeatLease({ unit, team: unit.team, vehicle, index, role: seatRole(vehicle.seats, index), time: this.time, tick: this.ticks });
    seat.lease = lease;
    // At the seat's rest heading: a gun whose arc leaves the hull's heading
    // out (the snowspeeder's rear gun) takes its seat looking into its arc,
    // not pinned at a limit; the bot's controller keeps that heading as its
    // home look (src/ai/classic/controller.js).
    seat.aimYaw = vehicle.restYaw(index);
    // A bot takes a hover's aim level: the 2004 game zeroes the hover's aim
    // angles with the hull (hull-relative, and the hull rests
    // level) and a pilot's entry leaves them. The little-down rest the
    // player's view takes (restAim) is the port's choice; a bot riding with
    // it, at the hover's narrow pitch limits and its slow nonlinear pitch
    // analog, shot into the ground in front of its target.
    seat.pitch = index === 0 && !vehicle.flying && (vehicle.base !== 'hover' || unit.networkHuman) ? -.12 : 0;
    seat.lookYaw = seat.lookPitch = null;
    vehicle.setSeatPolicy(index, Boolean(unit.networkHuman));
    if (!unit.networkHuman) lease.controllable = new SeatUnit(lease);
    unit.seat = lease;
    seatBody(unit);
    vehicle.sleeping = false;
    this.syncRider(lease);
    this.onBoard?.(unit, vehicle, index);
    return true;
  }

  // Asks for the unit's exit at the next batch (the 2004 game's exit
  // request). post: the command post the AI assigns once out (passed to
  // onUnboard).
  requestExit(unit, { post = null, promote, placement = true } = {}) {
    const lease = this.seatOf(unit);
    if (!lease) return false;
    lease.exitRequest = { post, promote, placement };
    return true;
  }

  // Out now. policy 'classic' (bots): the original game's placement, which
  // always finds a spot (the hull's origin at worst). policy 'safe' (always
  // for a network human, as for the player): the person's search (clear
  // floor, room, a clear line out), which refuses when boxed in on the
  // ground unless force; out of a craft in flight, beside it in the air.
  // placement false: out without moving the body. promote: hand the vacated
  // seat to the last bot aboard; by default, as the original game does unless
  // the unit is boarding something else (controller state 5). Only a person
  // gets out of a craft in flight (src/boarding.js mayLeave) unless force.
  unboard(unit, { force = false, placement = true, promote, post = null, policy = 'classic' } = {}) {
    const lease = this.seatOf(unit);
    if (!lease) return false;
    return this.vacate(lease, { placement, promote, post, policy, force });
  }

  // reason 'killed' (src/boarding.js leaveKilled): he died on the seat. His
  // body falls from where it rode, and the seat is promoted as after an exit.
  vacate(lease, { placement = true, promote, post = null, policy = 'classic', force = false, reason = 'exit' } = {}) {
    if (lease.released) return true;
    const v = lease.vehicle;
    const index = lease.index;
    const unit = lease.unit;
    if (reason === 'exit' && !force && !mayLeave(v, lease)) return false;
    const airborne = inFlight(v);
    const fall = reason === 'killed' && unit?.soldier ? v.riderDrop(index, unit, this.riderPoint) : null;
    let spot = null;
    let yaw = 0;
    if (placement && !lease.human && unit?.soldier && unit.alive && !unit.dead) {
      if (policy === 'safe' || personLease(lease)) {
        if (!personExitPoint(this.collision, v, this.exitSpot.position, this.safeScratch, airborne || force)) return false;
        spot = this.exitSpot.position;
        yaw = v.seats[index]?.aimYaw ?? v.aimYaw;
      } else {
        v.seatFrameForward(index, this.direction);
        v.sphere(this.center);
        classicSeatExit(this.collision, { origin: v.position, forward: this.direction, center: this.center, radius: v.boundRadius, body: v.body }, this.exitSpot);
        spot = this.exitSpot.position;
        yaw = Math.atan2(this.exitSpot.forward.x, this.exitSpot.forward.z);
      }
    }
    lease.released = true;
    lease.exitRequest = null;
    if (v.seats[index]?.lease === lease) v.seats[index].lease = null;
    if (unit && unit.seat === lease) {
      unit.seat = null;
      unseatBody(unit);
    }
    if (fall) dropBody(unit, fall, v.shieldsRiders() ? null : v.body);
    if (reason === 'killed' ? promote ?? true : reason === 'exit' && (promote ?? unit?.controller?.state !== 5)) this.promote(v, index);
    if (spot) placeOnFoot(unit, spot, yaw, { midair: airborne && personLease(lease) });
    this.onUnboard?.(unit, v, index, post, reason);
    return true;
  }

  // The 2004 game's seat promotion after an exit: the last bot aboard (in
  // seat order) moves into the vacated seat; a person aboard is never moved.
  // Its batch's edges are dropped (they were for the other seat); held values
  // carry on. Only a bot in a later seat moves up: the loop's eligibility
  // test (a field that must equal 2) is not resolved, and taking any seat
  // moved an AT-AT's pilot into the gun seat each time its idle gunner left
  // (every 10 s on Hoth), leaving the walker standing driverless until
  // someone boarded.
  promote(v, index) {
    if (!v.alive || !v.seatFree(index)) return null;
    let from = -1;
    for (let i = index + 1; i < v.seats.length; i++) if (v.crewed(i) && !personLease(v.seats[i].lease) && v.seats[i].lease.live) from = i;
    if (from < 0) return null;
    return this.move(v.seats[from].lease, index);
  }

  move(lease, index) {
    const v = lease.vehicle;
    if (v.seats[lease.index]?.lease === lease) v.seats[lease.index].lease = null;
    v.seats[index].lease = lease;
    lease.index = index;
    lease.role = seatRole(v.seats, index);
    const input = lease.input;
    input.pressed[0] = input.pressed[1] = false;
    input.reload = input.exit = false;
    v.setSeatPolicy(index, lease.human || Boolean(lease.unit?.networkHuman));
    // A new control for the AI to drive (the original game attaches the pilot
    // to the vacated control afresh).
    if (!lease.human && !lease.unit?.networkHuman) lease.controllable = new SeatUnit(lease);
    this.onMove?.(lease.unit, v, index);
    return lease;
  }

  // The player takes a seat a bot holds: the bot moves to the next free
  // seat after its own, or, with none, gets out (no promotion), as the
  // original game's enter callback does.
  displace(lease) {
    const v = lease.vehicle;
    for (let k = 1; k < v.seats.length; k++) {
      const i = (lease.index + k) % v.seats.length;
      if (v.seatFree(i)) return this.move(lease, i);
    }
    this.vacate(lease, { placement: true, promote: false, reason: 'displaced' });
    return null;
  }

  // A hull destroyed: every bot aboard is let go of its seat, all at once,
  // then dies, credited to whoever destroyed it.
  destroyedCrew(v) {
    const dying = [];
    for (const seat of v.seats) {
      const lease = seat.lease;
      if (!lease || lease.human) continue;
      this.vacate(lease, { placement: false, promote: false, reason: 'destroyed' });
      if (lease.unit?.alive && !lease.unit.combat?.dead) dying.push(lease.unit);
    }
    for (const unit of dying) unit.combat?.damage?.(Infinity, Infinity, null, v.lastAttacker ?? null);
  }

  // The snowspeeder's harpoon (docs/feature-audit.md "Hoth"): a towcable
  // round cast LockOnRange along the gun from the rear seat's turret node.
  // It holds only a leg of an enemy command walker (the AT-AT's thigh,
  // calf or foot; a hit on the body glances off), where the cable anchors.
  // A bot's harpoon is re-aimed at the nearest leg when its aim struck the
  // body. The cable's state lives on the source hull as plain fields; the
  // line that draws it is the page's (drawTethers). def: the weapon's.
  launchTow(source, def, origin, direction, seatIndex = 0, lease = null) {
    const range = Math.max(20, def.LockOnRange || 0);
    let hit = this.collision.cast(origin, direction, range, this.hit, source.body);
    let target = hit?.body?.owner;
    const walker = (v) => v instanceof Vehicle && v.alive && v.base === 'commandwalker' && v.team !== source.team;
    if (!walker(target)) return false;
    let leg = target.legAt(hit.point, this.center);
    if (!leg && lease && target.legs.length) {
      // The bot's harpoon: at the leg nearest the gun instead.
      let best = null, span = Infinity;
      for (const l of target.legs) {
        target.legPoint(l.name, this.direction);
        const d = this.direction.distanceToSquared(origin);
        if (d < span) { span = d; best = l; }
      }
      target.legPoint(best.name, this.direction).sub(origin);
      const far = this.direction.length();
      hit = far > 1e-3 && this.collision.cast(origin, this.direction.divideScalar(far), Math.min(range, far + 2), this.hit, source.body);
      if (hit?.body?.owner === target) leg = target.legAt(hit.point, this.center);
    }
    if (!leg) return false;
    if (source.towTarget >= 0) this.releaseTow(source, 'retow');
    let tether = this.tethers.get(source);
    if (!tether) {
      tether = { source, target: null, def: null, line: null, start: new THREE.Vector3(), end: new THREE.Vector3(), seat: 0 };
      this.tethers.set(source, tether);
    }
    tether.target = target; tether.def = def; tether.seat = seatIndex;
    tether.length = Math.max(10, def.ordnance?.CableLength || 60);
    source.towTarget = this.vehicles.indexOf(target);
    source.towLocal.copy(this.center);
    source.towLeg = leg;
    source.towAngle = Math.atan2(source.position.x - target.position.x, source.position.z - target.position.z);
    source.towWinding = 0;
    emitSound("towAttach", source, def);
    return true;
  }

  // The cable let go: the walker fell, the cable snapped (stretched too
  // far) or one end died. (A new harpoon cannot fly while it holds:
  // Vehicle.towHeld.)
  releaseTow(source, reason = 'released') {
    if (source.towTarget < 0) return;
    const tether = this.tethers.get(source);
    source.towTarget = -1; source.towWinding = 0; source.towLeg = '';
    if (tether?.line) tether.line.visible = false;
    if (tether?.def && reason !== 'destroyed') emitSound("towRelease", source, tether.def, reason);
  }

  // The enemy command walker nearest a hull within range, or null.
  nearestWalker(source, range) {
    let best = null, span = range * range;
    for (const v of this.vehicles) {
      if (!v.alive || v.base !== 'commandwalker' || v.team === source.team) continue;
      const d = v.position.distanceToSquared(source.position);
      if (d < span) { span = d; best = v; }
    }
    return best;
  }

  // The gun end of a hull's cable: its tow seat's turret node, else its
  // centre. presented: as drawn between ticks.
  towGun(source, out, presented = false) {
    const tether = this.tethers.get(source);
    const turret = source.seats[tether?.seat ?? 1]?.view.turret || source.seats.find((s) => s.view.turret)?.view.turret;
    if (turret) return turret.getWorldPosition(out);
    return out.copy(source.center).applyMatrix4(presented ? source.root.matrixWorld : source.matrix);
  }

  // The tick: each held cable's winding, breaking and the walker's fall.
  // Opposite turns unwind the cable; three complete circuits bring a
  // command walker down; distance and destruction break it. Nothing else
  // does: the 2004 game's cable tests its run only against the
  // walker it holds, wrapping round its legs, so another walker, a craft or
  // the ground between does not cut it. (The port's cable used to snap on
  // anything between: on Hoth, where the two AT-ATs walk close together,
  // the bots' cables were cut by the other walker before three circuits.)
  updateTethers() {
    for (const tether of this.tethers.values()) {
      const { source, target } = tether;
      if (source.towTarget < 0) continue;
      this.towGun(source, tether.start);
      tether.end.copy(source.towLocal).applyMatrix4(target.matrix);
      const span = tether.start.distanceTo(tether.end);
      if (!source.alive || !target.alive) { this.releaseTow(source, 'destroyed'); continue; }
      if (span > tether.length * 1.2) { this.releaseTow(source, 'snapped'); continue; }
      const around = Math.atan2(source.position.x - target.position.x, source.position.z - target.position.z);
      source.towWinding += angle(around - source.towAngle);
      source.towAngle = around;
      if (Math.abs(source.towWinding) >= 6 * Math.PI) {
        this.releaseTow(source, 'tripped');
        target.damage(Infinity, source.seats[tether.seat]?.lease?.unit ?? source);
      }
    }
  }

  // The page's frame: each held cable as a line from the anchor on the
  // walker's leg, round the legs once per quarter circuit wound, to the
  // gun, the last span sagging. Built from the hulls' plain tow fields, so
  // a page showing a worker's battle draws the same cable.
  drawTethers(presented = true) {
    if (this.cosmetic === false) return;
    for (const v of this.vehicles) {
      if (v.towTarget < 0 && !this.tethers.get(v)?.line?.visible) continue;
      let tether = this.tethers.get(v);
      if (!tether) { tether = { source: v, target: null, def: null, line: null, start: new THREE.Vector3(), end: new THREE.Vector3(), seat: 1 }; this.tethers.set(v, tether); }
      const target = this.vehicles[v.towTarget];
      if (v.towTarget < 0 || !target?.alive) { if (tether.line) tether.line.visible = false; continue; }
      if (!tether.line) {
        const geometry = new THREE.BufferGeometry();
        tether.position = new THREE.BufferAttribute(new Float32Array(TOW_POINTS * 3), 3).setUsage(THREE.DynamicDrawUsage);
        geometry.setAttribute('position', tether.position);
        tether.line = new THREE.Line(geometry, new THREE.LineBasicMaterial({ color: 0xb7b5ab }));
        tether.line.frustumCulled = false;
        this.scene.add(tether.line);
      }
      tether.target = target;
      const frame = presented ? target.root.matrixWorld : target.matrix;
      tether.end.copy(v.towLocal).applyMatrix4(frame);
      this.towGun(v, tether.start, presented);
      // The wraps: a point on the legs' circle every quarter circuit, at
      // the anchor's height, in the direction the cable has wound.
      const quarters = Math.min(TOW_POINTS - 18, Math.floor(Math.abs(v.towWinding) / (Math.PI / 2)));
      const radius = Math.max(target.size.x, target.size.z) * .5;
      let n = 0;
      const put = (p) => { tether.position.setXYZ(n++, p.x, p.y, p.z); };
      put(tether.end);
      for (let k = 1; k <= quarters; k++) {
        const a = v.towAngle - Math.sign(v.towWinding) * (Math.abs(v.towWinding) - k * Math.PI / 2);
        this.center.set(target.center.x, v.towLocal.y, target.center.z).applyMatrix4(frame);
        this.center.x += Math.sin(a) * radius; this.center.z += Math.cos(a) * radius;
        put(this.center);
      }
      const from = quarters ? this.center.clone() : tether.end;
      const span = from.distanceTo(tether.start);
      for (let i = 1; i <= 16; i++) {
        const t = i / 16;
        this.center.lerpVectors(from, tether.start, t);
        this.center.y -= Math.sin(t * Math.PI) * Math.min(2, span * .025);
        put(this.center);
      }
      tether.line.geometry.setDrawRange(0, n);
      tether.position.needsUpdate = true;
      tether.line.visible = true;
    }
  }

  // The nearest hull in reach ahead of an eye, in sight of it. may(v): only
  // hulls the soldier may board (src/boarding.js mountInReach); without it,
  // any hull the player does not drive.
  enterable(eye, direction, may = null) {
    let best = null, distance = Infinity;
    for (const v of this.vehicles) {
      if (!v.alive || (may ? !may(v) : v.driver)) continue;
      v.sphere(this.center);
      this.direction.subVectors(this.center, eye);
      const span = this.direction.length();
      if (span > v.radius + 3.5 || this.direction.dot(direction) < 0) continue;
      const hit = this.collision.cast(eye, this.direction.normalize(), span, this.hit);
      if (hit && hit.body !== v.body && hit.distance < span) continue;
      if (span < distance) { distance = span; best = v; }
    }
    return best;
  }

  // A round that struck a hull: its Damage by the hull's health type.
  // owner (optional): whoever fired, for kill credit; teamScale(vehicle):
  // the battle's friendly fire factor for the owner.
  struck(body, def, owner = null, teamScale = null) {
    const v = body?.owner;
    if (v instanceof Vehicle) v.damage((def.Damage || 0) * healthScale(def, v.healthType) * (teamScale ? teamScale(v) : 1), owner);
  }

  // An explosion's area damage to every hull in its reach (src/ordnance.js);
  // a hull shields nothing, its own body included.
  blast(def, position, owner = null, teamScale = null) {
    // Snapshot positions is unnecessary: destruction disables bodies before
    // callbacks, so chained explosions cannot kill the same vehicle twice.
    for (const v of this.vehicles) {
      if (!v.alive) continue;
      const radius = v.blastSphere(this.center);
      const damage = blastDamage(def, blastDistance(position, this.center, radius), v.healthType);
      if (!(damage > 0) || blastBlocked(this.collision, position, this.center, v.body)) continue;
      v.damage(damage * (teamScale ? teamScale(v) : 1), owner);
    }
  }

  dispose() {
    for (const tether of this.tethers.values()) if (tether.line) { this.scene.remove(tether.line); tether.line.geometry.dispose(); tether.line.material.dispose(); }
    this.tethers.clear();
    for (const v of this.vehicles) for (const seat of v.seats) if (seat.lease && !seat.lease.human) this.vacate(seat.lease, { placement: false, promote: false, reason: 'removed' });
    if (this.units) for (const v of this.vehicles) this.units.remove(v.aiTarget);
    for (const v of this.vehicles) {
      v.root.traverse(object => { if (object.isMesh) this.uncaster?.(object); });
      this.scene.remove(v.root);
      v.skeleton.dispose();
      this.collision.removeBody(v.body);
    }
    for (const t of this.templates.values()) {
      for (const p of t.batches) { p.geometry.dispose(); p.material.dispose(); }
      t.shape.bvh.geometry.dispose();
    }
    this.collision.vehicleBlockers?.bvh.geometry.dispose();
    this.collision.vehicleBlockers = null;
    this.templates.clear(); this.vehicles.length = this.slots.length = 0;
  }
}

export class VehicleDriver {
  // hiddenLayer: the layer drawn only into shadows, for the hull he looks
  // out of in first person.
  constructor({ vehicles, soldier, combat, actor, animator, clips, camera, collision, hud, presentation, hiddenLayer = 1 }) {
    Object.assign(this, { vehicles, soldier, combat, actor, animator, clips, camera, collision, hud, presentation, hiddenLayer });
    this.vehicle = null;
    this.point = new THREE.Vector3(); this.target = new THREE.Vector3(); this.direction = new THREE.Vector3();
    this.hit = makeHit(); this.cameraSet = false;
    this.speedEffect = 0;
    this.riderOffset = new THREE.Vector3();
    this.riderQuaternion = new THREE.Quaternion();
    this.riderLean = 0;
    this.lease = null; // the player's seat lease while he sits
    this.hiddenHull = null; // the hull left out of his first-person view
  }
  get active() { return Boolean(this.vehicle); }
  // Mouse counts on his seat (src/boarding.js seatLook): a craft's stick at
  // the controls, else the seat's aim, held inside its limits.
  look(dx, dy, sensitivity, invert) {
    const v = this.vehicle;
    if (!v) return;
    seatLook(v, v.seatIndex, dx, dy, { scale: sensitivity, invert, flightScale: this.flightSensitivity ?? 1, flightInvert: Boolean(this.flightInvert) });
  }
  // Preview unacknowledged mouse counts for the reticle without changing
  // the authoritative seat or its weapon ray. Rebuild from the current seat
  // every frame so acknowledgements, seat changes and clear() cannot double
  // apply or retain input. Aircraft use their flight controller instead.
  preview(pending, looks, scale, invert) {
    const v = this.vehicle;
    this.previewVehicle = null;
    if (!v || seatControl(v, v.seatIndex) === 'flight') return;
    let yaw = v.lookYaw ?? v.aimYaw, pitch = v.lookPitch ?? v.pitch;
    const view = v.seat.view;
    const apply = (counts, sensitivity, inverted) => {
      for (const [dx, dy] of counts || []) {
        yaw -= dx * sensitivity / 2048;
        pitch = THREE.MathUtils.clamp(pitch + (inverted ? dy : -dy) * sensitivity / 2048,
          view.pitchLimits[0], view.pitchLimits[1]);
        if (view.yawLimits) yaw = v.holdYaw(view, yaw);
      }
    };
    for (const command of pending || []) apply(command.look, command.mouseScale ?? scale, command.invertMouse ?? invert);
    apply(looks, scale, invert);
    this.previewVehicle = v;
    this.previewSeat = v.seatIndex;
    this.previewYaw = yaw; this.previewPitch = pitch;
  }
  viewDirection(out) {
    const v = this.vehicle;
    if (this.previewVehicle !== v || this.previewSeat !== v?.seatIndex) return v.viewDirection(out);
    const yaw = this.previewYaw, pitch = this.previewPitch;
    return out.set(Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), Math.cos(yaw) * Math.cos(pitch));
  }
  // Where his reticle points: a craft's pilot fires along its nose, as drawn
  // this frame (the frame its chase camera follows, src/flight/camera.js);
  // any other seat along its view.
  reticleDirection(out) {
    const v = this.vehicle;
    if (seatControl(v, v?.seatIndex) === 'flight') return out.set(0, 0, 1).applyQuaternion(v.flightFrameQ || v.quaternion);
    return this.viewDirection(out);
  }
  // The player's body as a unit (main.js makes it combat.owner), or null.
  get unit() {
    const owner = this.combat?.owner;
    return owner && typeof owner === 'object' ? owner : null;
  }
  enter(v) {
    if (this.active || !v?.alive || v.driver || this.combat.dead) return false;
    // The seat he last used, if it is free; else, as the original game seats a
    // player, the first seat that is free or a bot holds (the bot moves on
    // or gets out). Never a hull held by anyone but his friends (src/
    // boarding.js boardingSeat).
    const team = this.unit?.team ?? this.combat?.team ?? 0;
    const index = boardingSeat(v, { team, person: true, relation: v.system?.relation, preferred: v.seatIndex });
    if (index < 0 || !claimSeat(v, index)) return false;
    v.seatIndex = index;
    this.vehicle = v; v.humanDriver = this; v.driver = this.soldier; v.sleeping = false;
    v.restAim(index);
    this.takeSeat(v, index);
    this.combat.mounted = true;
    this.combat.zoomed = false;
    this.combat.gunUnderwater = false;
    this.soldier.air = null;
    this.soldier.velocity.set(0, 0, 0);
    this.cameraSet = false;
    this.sync();
    return true;
  }
  // The player's lease on seat index (src/seats.js); his unit's seat while
  // he sits, so soldier raycasts pass him by as they do seated bots.
  takeSeat(v, index) {
    const unit = this.unit;
    const lease = new SeatLease({ unit, human: true, team: unit?.team ?? this.combat?.team ?? 0, vehicle: v, index, role: seatRole(v.seats, index),
      time: v.system?.time ?? 0, tick: v.system?.ticks ?? 0 });
    v.seats[index].lease = lease;
    if (unit) {
      unit.seat = lease;
      unit.targetable = false; // perception targets the vehicle while he sits
    }
    v.setSeatPolicy(index, true);
    this.lease = lease;
  }
  sync() {
    const v = this.vehicle;
    if (!v) return;
    poseRider(v, v.seatIndex, this.soldier, this.actor, this.animator, this.clips, this.presentation, this.point);
    // His hit shapes follow the hull (Unit.updateSpheres).
    if (this.unit) this.unit.tickCount += 1;
  }

  // After the soldier's animation is sampled: the body at its seat node, or
  // its posed pelvis on a saddle (seatRider). On an enclosed seat (no rider
  // node) his body stays hidden, enforced every frame as presentRiders does
  // for everyone else's: each own correction shows the body again
  // (EntityGameState.correct), which left it standing on top of the hull
  // (the droid starfighter on Bespin: Platforms).
  presentRider(dt) {
    const v = this.vehicle;
    if (!v) return;
    if (!v.seats[v.seatIndex]?.view.rider?.node) { this.actor.root.visible = false; return; }
    this.riderLean = seatRider(v, v.seatIndex, this.actor, dt, this.riderLean, this);
  }
  update(dt, controls) {
    const v = this.vehicle;
    if (!v) return;
    if (!v.alive) { this.release(); return; }
    // The camera eases over to the new seat's view, as the chase camera
    // does whenever it is less than 80 m from where it should be.
    if (controls.take('nextPrimary') && v.seats.length > 1) this.nextSeat(v);
    if (controls.take('previousPrimary') && v.seats.length > 1) this.nextSeat(v, -1);
    controls.take('nextSecondary'); controls.take('zoom');
    v.fire(dt, controls);
    this.sync();
  }
  // F (1 in a craft): on to the next seat, 2 back (src/boarding.js
  // switchSeat). A bot holding that seat trades places, as the 2004 game's
  // seat switch does. That switch hands each seat's control to its new
  // controller with the controls cleared and turns nothing, so back at the
  // pilot's seat he aims along the hull, which never swings to where a gun
  // looked. Into a gunner's seat his view goes with him, inside its arc (the
  // port's choice).
  nextSeat(v, direction = 1) {
    const lease = this.lease?.vehicle === v && !this.lease.released ? this.lease : null;
    const to = switchSeat(v.system, v, lease, v.seatIndex, direction);
    if (to < 0) return;
    v.seatIndex = to;
    v.setSeatPolicy(to, true);
  }
  // E: out (src/boarding.js mayLeave), at a clear spot beside the hull. In
  // a craft in flight he bails out beside it and falls from there, at rest,
  // as the original game's seat exit leaves a soldier (the craft comes down
  // without him: src/flight/pilotless.js). Boxed in on the ground he stays
  // aboard rather than being put somewhere he could not reach.
  exit(force = false) {
    const v = this.vehicle;
    if (!v) return true;
    const airborne = inFlight(v);
    if (!personExitPoint(this.collision, v, this.point, this, airborne || force)) { this.hud.message('Exit blocked: move to open ground'); return false; }
    const yaw = v.aimYaw;
    this.release(true);
    placeOnFoot({ soldier: this.soldier, presentation: this.presentation, animator: this.animator, actor: this.actor }, this.point, yaw, { midair: airborne });
    return true;
  }
  // Out of the seat: on a death, a respawn or the hull's destruction, or
  // (voluntary) getting out, when the last bot aboard takes over his seat
  // as the original game's exit does. killed: he died on the seat (src/
  // boarding.js leaveKilled): his body falls from where it rode, its death
  // under way, and the seat is promoted as after an exit.
  release(voluntary = false, killed = false) {
    const v = this.vehicle || this.lease?.vehicle || this.unit?.seat?.vehicle;
    if (!v) return;
    const lease = this.lease || (this.unit?.seat?.vehicle === v ? this.unit.seat : null);
    const fall = killed && this.unit && lease ? v.riderDrop(lease.index, this.unit, this.point) : null;
    v.driver = null; v.humanDriver = null;
    this.vehicle = null;
    if (lease) {
      lease.released = true;
      if (v.seats[lease.index]?.lease === lease) v.seats[lease.index].lease = null;
      if (lease.unit?.seat === lease) {
        lease.unit.seat = null;
        lease.unit.targetable = true;
      }
      this.lease = null;
      if (voluntary || killed) v.system?.promote?.(v, lease.index);
    }
    const unit = this.unit || { soldier: this.soldier, combat: this.combat, actor: this.actor, animator: this.animator };
    // Repair either end even if a replica/respawn already replaced the lease.
    if (unit.seat?.vehicle === v) { unit.seat.released = true; unit.seat = null; }
    for (const seat of v.seats) if (seat.lease?.human && (seat.lease.unit === unit || seat.lease === lease)) { seat.lease.released = true; seat.lease = null; }
    unseatBody(unit);
    if (fall) dropBody(unit, fall, v.shieldsRiders() ? null : v.body);
    this.lease = null; this.riderLean = 0;
    this.presentation?.reset(); this.onRelease?.();
    this.speedEffect = 0;
    this.combat.mounted = false;
    this.actor.root.visible = true;
    this.actor.root.rotation.set(0, this.soldier.aimYaw, 0);
    this.animator.gunner?.(null);
    // Dead, his death clip plays on (a page showing a world worker's or a
    // server's battle lets go of the seat after his death arrived).
    if (!killed && !this.combat?.dead) this.animator.reset?.(this.soldier.aimYaw);
    this.hideHull(null);
    v.viewFirstPerson = false;
    v.aimCamera = null;
    this.cameraSet = false;
    if (this.aimView) this.aimView.cameraSet = false;
  }
  updateCamera(dt, firstPerson) {
    const v = this.vehicle;
    const view = this.cameraView ||= new VehicleCamera(this.camera, this.collision);
    view.cameraSet = this.cameraSet;
    view.previewDirection = this.previewVehicle === v && this.previewSeat === v.seatIndex
      ? this.viewDirection(this.direction) : null;
    // On an exposed seat (a rider shows) his eyes are his posed body's head,
    // which presentRider has just placed; enclosed, the seat's EyePointOffset.
    view.eyeNode = v.seats[v.seatIndex].view.rider && this.actor?.root.visible ? this.actor.byName?.bone_head || null : null;
    view.update(v, dt, firstPerson);
    this.cameraSet = view.cameraSet;
    this.speedEffect = view.speedEffect;
    this.hideHull(firstPerson && view.enclosed ? v : null);
  }
  // In first person the hull he looks out of leaves the view and keeps its
  // shadow, as his body does: the original game doesn't draw a vehicle for the
  // player seeing from inside it (it drew a cockpit model instead, which the
  // port doesn't import). A hull his eyes are outside of (a speeder bike,
  // a skiff) stays.
  hideHull(v) {
    if (this.hiddenHull === v) return;
    const layer = (hull, value) => hull?.root.traverse((object) => { if (object.isMesh) object.layers.set(value); });
    layer(this.hiddenHull, 0);
    layer(v, this.hiddenLayer);
    this.hiddenHull = v;
  }
}


const chaseSpecs = new WeakMap();
function chaseSpec(v) {
  let spec = chaseSpecs.get(v);
  if (!spec) { spec = { ...CHASE_CAMERA.hover }; for (const [name, values] of Object.entries(CHASE_CAMERA.byName)) if (v.name.includes(name)) Object.assign(spec, values); chaseSpecs.set(v, spec); }
  return spec;
}

export class VehicleCamera {
  constructor(camera, collision, simulation = false) {
    this.camera = camera; this.collision = collision; this.simulation = simulation;
    this.cameraSet = false; this.speedEffect = 0; this.reach = 1; this.fov = 0;
    this.mode = 'seat'; this.eyeNode = null; this.enclosed = false;
    this.frame = new THREE.Matrix4(); this.inverseFrame = new THREE.Matrix4(); this.want = new THREE.Quaternion(); this.inverse = new THREE.Quaternion();
    this.origin = new THREE.Vector3(); this.pivot = new THREE.Vector3(); this.target = new THREE.Vector3(); this.rest = new THREE.Vector3();
    this.boom = new THREE.Vector3(); this.local = new THREE.Vector3(); this.dir = new THREE.Vector3(); this.lookAt = new THREE.Vector3();
    this.right = new THREE.Vector3(); this.up = new THREE.Vector3(); this.axis = new THREE.Vector3(); this.q = new THREE.Quaternion();
    this.euler = new THREE.Euler(0, 0, 0, 'YXZ'); this.hit = makeHit();
  }
  update(v, dt, firstPerson) {
    if (!v) return;
    if (v.flying && v.seatIndex === 0) return updateFlightCamera(this, v, dt, firstPerson);
    const mode = v.seatIndex === 0 && !v.walking ? 'chase' : 'seat';
    const fresh = !this.cameraSet || this.firstPerson !== firstPerson || this.lastVehicle !== v || this.mode !== mode;
    this.lastVehicle = v;
    this.firstPerson = firstPerson;
    this.mode = mode;
    v.viewFirstPerson = firstPerson;
    if (this.simulation) v.aimCamera = this.camera;
    const speed = v.velocity.length();
    // The motion blur option's streaks: not aboard a craft (a gunner's seat
    // included), whose views the 2004 game draws sharp at any speed.
    const intensity = v.flying ? 0 : THREE.MathUtils.smoothstep(speed, 12, Math.max(25, v.speed * .85));
    this.speedEffect += (intensity - this.speedEffect) * (1 - Math.exp(-4 * dt));
    const cam = this.camera, i = v.seatIndex, view = v.seats[i].view;
    const spec = mode === 'chase' ? chaseSpec(v) : null;
    // The view widens with speed (speedFov at MaxSpeed, and on
    // past it on a boost), in the chase and the cockpit alike.
    const widen = spec ? spec.speedFov * THREE.MathUtils.clamp((v.flying ? Math.abs(v.airspeed) : speed) / Math.max(1, v.speed), 0, 1.5) : 0;
    const fov = spec ? ((firstPerson ? spec.firstFov : spec.fov) + widen) * DEG : firstPerson ? view.firstFov : view.thirdFov;
    this.fov = fresh ? fov : this.fov + (fov - this.fov) * (1 - Math.exp(-FOV_RATE * dt));
    const vertical = THREE.MathUtils.radToDeg(2 * Math.atan(Math.tan(this.fov / 2) / (cam.aspect || 1)));
    if (cam.fov !== vertical) { cam.fov = vertical; cam.updateProjectionMatrix(); }
    if (firstPerson) this.eyeView(v, i, view, cam);
    else if (spec) this.chase(v, spec, cam, dt, fresh);
    else this.seatChase(v, i, view, cam, dt, fresh);
    cam.updateMatrixWorld();
    this.cameraSet = true;
  }

  // The hull's frame with its lean: as drawn between ticks on the page, at
  // the tick in the simulation.
  hullFrame(v, out) {
    if (!this.simulation) return out.copy(v.root.matrixWorld);
    if (v.flying) return out.compose(v.position, v.quaternion, UNIT_SCALE);
    this.euler.set(-v.tilt, v.yaw, v.roll);
    return out.compose(v.position, this.q.setFromEuler(this.euler), UNIT_SCALE);
  }

  eyeView(v, i, view, cam) {
    const hull = i === 0 || !view.turret;
    if (hull) this.hullFrame(v, this.frame); else v.seatFrame(i, this.frame, !this.simulation);
    this.up.setFromMatrixColumn(this.frame, 1);
    if (this.eyeNode) this.eyeNode.getWorldPosition(cam.position).addScaledVector(this.up, .1);
    else cam.position.copy(view.eye).applyMatrix4(this.frame);
    // Inside the hull's box (a cockpit) the hull leaves the view; outside
    // it (a saddle) it stays.
    this.local.copy(cam.position).applyMatrix4(this.inverseFrame.copy(this.frame).invert());
    const b = v.bounds, m = .15;
    this.enclosed = this.local.x > b.min.x + m && this.local.x < b.max.x - m && this.local.y > b.min.y + m && this.local.y < b.max.y - m && this.local.z > b.min.z + m && this.local.z < b.max.z - m;
    if (v.flying && i === 0) this.dir.setFromMatrixColumn(this.frame, 2).normalize();
    else if (this.previewDirection) this.dir.copy(this.previewDirection);
    else v.seatForward(i, this.dir);
    lookBasis(this.dir.negate(), this.up, cam.quaternion);
  }

  chase(v, spec, cam, dt, fresh) {
    const presented = !this.simulation;
    const yaw = presented ? v.root.rotation.y : v.yaw;
    const tilt = presented ? -v.root.rotation.x : v.tilt;
    const roll = presented ? v.root.rotation.z : v.roll;
    this.pivot.copy(v.center).applyAxisAngle(UP, yaw).add(presented ? v.visual : v.position);
    const back = Math.max(spec.minBack, spec.back * v.size.z), up = Math.max(spec.minUp, spec.up * v.size.y);
    // Where it rests: behind the heading (half the nose's pitch) and above.
    const pitch = tilt * .5;
    this.dir.set(Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), Math.cos(yaw) * Math.cos(pitch));
    this.rest.copy(this.pivot).addScaledVector(this.dir, -back).addScaledVector(UP, up);
    const snap = fresh || cam.position.distanceTo(this.rest) > CHASE_SNAP;
    if (snap) cam.position.copy(this.rest);
    else cam.position.lerp(this.rest, 1 - Math.exp(-spec.lag * dt));
    // The walls: the boom from the hull's centre to the camera.
    this.origin.copy(this.pivot);
    this.boom.subVectors(cam.position, this.origin);
    const reach = this.clearance(v, cam.quaternion);
    if (snap || reach < this.reach) this.reach = reach;
    else this.reach += (reach - this.reach) * (1 - Math.exp(-RETURN_RATE * dt));
    cam.position.copy(this.origin).addScaledVector(this.boom, this.reach);
    // What it looks at: a point ahead of the nose (a flyer: nose metres
    // along the heading, so the view stays behind the tail looking a
    // little past the nose) or ahead along the velocity (a hover), a
    // little above the hull, swung a share of the way toward the look the
    // nose is following (steer).
    this.dir.set(Math.sin(yaw) * Math.cos(tilt), Math.sin(tilt), Math.cos(yaw) * Math.cos(tilt));
    this.lookAt.copy(this.pivot).addScaledVector(this.dir, spec.nose).addScaledVector(v.velocity, spec.lead).addScaledVector(UP, up * .2);
    this.dir.subVectors(this.lookAt, cam.position);
    if (this.dir.lengthSq() < 1e-6) this.dir.set(Math.sin(yaw), 0, Math.cos(yaw));
    const pilot = v.seats[0];
    if (spec.steer && v.flying && v.driver && pilot.lookYaw !== null) {
      this.dir.applyAxisAngle(UP, angle(pilot.lookYaw - v.yaw) * spec.steer);
      this.axis.crossVectors(this.dir, UP).normalize();
      if (this.axis.lengthSq() > 1e-6) this.dir.applyAxisAngle(this.axis, -(pilot.lookPitch - tilt) * spec.steer);
    }
    this.dir.normalize();
    this.up.copy(UP);
    // The bank rolls the view a share of the way; the barrel roll (spin)
    // takes it all the way round with the hull.
    const bankRoll = (roll - (v.spin || 0)) * spec.roll + (v.spin || 0);
    if (bankRoll) this.up.applyAxisAngle(this.dir, bankRoll);
    lookBasis(this.axis.copy(this.dir).negate(), this.up, this.want);
    this.want.multiply(this.q.setFromAxisAngle(RIGHT, -spec.tilt * DEG));
    if (snap) cam.quaternion.copy(this.want);
    else cam.quaternion.slerp(this.want, 1 - Math.exp(-spec.turn * dt));
  }

  seatChase(v, i, view, cam, dt, fresh) {
    v.seatFrame(i, this.frame, !this.simulation);
    v.cameraTurn(i, this.frame, this.want);
    if (this.previewDirection) {
      lookBasis(this.dir.copy(this.previewDirection).negate(), this.up.setFromMatrixColumn(this.frame, 1), this.want);
      this.want.multiply(this.q.setFromAxisAngle(RIGHT, -view.tilt));
    }
    this.pivot.copy(view.center).applyMatrix4(this.frame);
    const snap = fresh || cam.position.distanceTo(this.pivot) > CAMERA_SNAP;
    // Human mouse aim already has its final direction. Keep positional
    // follow and wall recovery smooth without filtering the sight a second time.
    if (snap || v.driver) cam.quaternion.copy(this.want);
    else cam.quaternion.slerp(this.want, 1 - Math.exp(-view.aimTension * dt));
    // Where it should be, then as far along the rays from the seat frame as
    // the walls allow (pulled in at once, eased back out).
    this.origin.copy(RAY_ORIGIN).applyMatrix4(this.frame);
    this.target.copy(view.offset).applyQuaternion(cam.quaternion).add(this.pivot);
    this.boom.subVectors(this.target, this.origin);
    const reach = this.clearance(v, cam.quaternion);
    if (snap || reach < this.reach) this.reach = reach;
    else this.reach += (reach - this.reach) * (1 - Math.exp(-RETURN_RATE * dt));
    this.target.copy(this.origin).addScaledVector(this.boom, this.reach);
    if (snap) cam.position.copy(this.target);
    else {
      // Each camera axis eases separately, at its section's MoveTension.
      easeAxes(cam, this.target, view.moveTension, dt);
      // The eased spot can lag behind a new wall: check where it will draw.
      this.boom.subVectors(cam.position, this.origin);
      const clear = this.clearance(v, cam.quaternion);
      if (clear < 1) cam.position.copy(this.origin).addScaledVector(this.boom, clear);
    }
  }

  // The share of the boom (this.boom from this.origin) that is clear: the
  // nearest hit of the centre ray and four to the camera's corners, the
  // hull itself left out.
  clearance(v, q) {
    if (this.boom.lengthSq() < 1e-8) return 1;
    this.right.set(CORNER, 0, 0).applyQuaternion(q);
    this.up.set(0, CORNER, 0).applyQuaternion(q);
    let reach = 1;
    for (let k = 0; k < RAYS.length; k += 2) {
      this.dir.copy(this.boom).addScaledVector(this.right, RAYS[k]).addScaledVector(this.up, RAYS[k + 1]);
      const far = this.dir.length();
      this.dir.divideScalar(far);
      const hit = this.collision.cast(this.origin, this.dir, far + CAMERA_SKIN, this.hit, v.body);
      if (hit) reach = Math.min(reach, Math.max(0, hit.distance - CAMERA_SKIN) / far);
    }
    return reach;
  }
}
