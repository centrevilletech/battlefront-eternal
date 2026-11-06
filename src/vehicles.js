import { poseFlightGear, gearFoot } from "./flight/gear.js";
import { initFlight, standHeight } from "./flight/model.js";
import { groundMotion } from './vehicle-motion.js';
import { hullQuaternion, restHeight } from './vehicle-hover.js';
import { updateFlightCamera, easeAxes, cameraTension } from './flight/camera.js';
import { vehicleMissileDefinition } from "./weapon-targeting.js";
import { waterDamageRule } from './vehicle-water.js';
import * as THREE from "three";
import { descendantMuzzles, vehicleMuzzleTips } from "./muzzles.js";
import { makeHit } from "./collision.js";
import { vehicleBatches, attachVehicleBatches } from "./vehicle-rig.js";
import { modelPack } from "./models.js";
import { loadClips, Pose } from "./anim.js";
import { Weapon } from "./weapons.js";
import { givePowerup } from "./items.js";
import { blastDamage, blastDistance, healthScale } from "./ordnance.js";
import { VehicleTarget, seatWeaponPolicy, dropBody, unseatBody } from "./seats.js";
import { boardingSeat, canBoard, seatControl, teamRelation } from "./boarding.js";
import { makeRiderPoints, riderDropPoint, riderHitShapes, riderPoints } from "./rider-pose.js";

import { VehiclesWorld } from "./vehicles-world.js";
import { SeatsWorld } from "./seats-world.js";

const UP = new THREE.Vector3(0, 1, 0);
const RIGHT = new THREE.Vector3(1, 0, 0);
const NOSE = new THREE.Vector3(0, 0, 1);
const DOWN = new THREE.Vector3(0, -1, 0);
const UNIT_SCALE = new THREE.Vector3(1, 1, 1);
const DEG = Math.PI / 180;
const DEATH_BLEND = 0.3; // s at most a dying walker takes from its last pose into its death clip (the port's)
const num = (v, fallback = 0) => Number.isFinite(parseFloat(v)) ? parseFloat(v) : fallback;
const clockSeconds = () => (globalThis.performance?.now?.() ?? Date.now()) / 1000;
const vector = (v, fallback) => v ? String(v).split(/\s+/).map(Number) : fallback;
const angle = (v) => Math.atan2(Math.sin(v), Math.cos(v));
const approach = (v, target, delta) => v + THREE.MathUtils.clamp(target - v, -delta, delta);
const idle = Object.freeze({ forward: 0, right: 0, turn: 0, brake: false });
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
const TO_LOCAL = new THREE.Matrix4(), DECOMPOSED_AT = new THREE.Vector3(), DECOMPOSED_SCALE = new THREE.Vector3();
// m a wall probe's or spring's cached triangles reach past its sphere: a
// hull at 30 m/s moves 0.25 m a 120 Hz step.
const WALL_CACHE_MARGIN = 1.5;
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
    node, aimer,
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
function riderSample(v, index, clips, out, ninePitch = null, nineTurn = 0) {
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
  // (A 9-pose the module worked out comes in as ninePitch, nineTurn.)
  else if (ninePitch !== null) { out.pitch = ninePitch; out.turn = nineTurn; }
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
  const native=actor.root.userData.presentationRow;
  if(native){const lease=v.seats[index]?.lease;if(native.riderLease!==lease){native.riderReset=true;native.riderLease=lease;}native.riderFrame=native.handle.owner.frame;native.riderVehicle=v;native.riderNode=seat;native.riderLean=lean;return lean;}
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
    // Each wall probe and hover spring keeps its own nearby-triangle cache
    // (CollisionWorld.gather): they query around the same spots tick after
    // tick, twice a tick at 120 Hz, and the probes twice a step.
    this.wallCaches = this.probes.map(() => system.collision.gatherCache?.(WALL_CACHE_MARGIN) ?? null);
    this.springCaches = (this.def.springs || []).map(() => system.collision.gatherCache?.(WALL_CACHE_MARGIN) ?? null);
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
    this.beamFrom = new THREE.Vector3();
    this.aimQ = new THREE.Quaternion(); this.aimQ2 = new THREE.Quaternion();
    this.from = new THREE.Vector3();
    this.legEuler = new THREE.Euler(0, 0, 0, "YXZ");
    this.recoilAxis = new THREE.Vector3();
    this.recoilPoint = new THREE.Vector3();
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

  // Its spawn pad's place in the battle's list (the store names a pad so).
  get slotIndex() { return this.system.slots.indexOf(this.slot); }
  // What the store reads of its seats beside their aim (rust/sim/schema/
  // vehicles.mjs): each one's limits, and its lease (an empty seat's an
  // object of its own with nothing in it), and its item target's centre,
  // kept in place.
  get seatViews() { return this.seats.map((seat) => seat.view); }
  get seatLeases() { return this.seats.map((seat) => seat.lease ?? {}); }
  get itemCenter() { return this.itemTarget.center; }
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
  // weapons; docs/ai-vehicles.md); random, their generator (seatWeaponPolicy).
  setSeatPolicy(i, player, random = null) {
    for (const w of this.seats[i]?.weapons || []) seatWeaponPolicy(w.state, player, random);
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
    if (this.system.f2) return this.system.f2.riders.shapes(unit, shapes);
    const s = this.system.rider;
    const frame = riderBody(this, i, unit, s.points, s.frame, s);
    return frame ? riderHitShapes(s.points, frame, unit.rootScale, shapes) : 0;
  }
  // Where a rider killed on seat i starts to fall from, in the world: out,
  // or null where none shows (src/rider-pose.js riderDropPoint).
  riderDrop(i, unit, out) {
    if (this.system.f2) return this.system.f2.riders.drop(unit, out);
    const s = this.system.rider;
    const frame = riderBody(this, i, unit, s.points, s.frame, s);
    return frame ? riderDropPoint(s.points, out).applyMatrix4(frame) : null;
  }

  // A seat's eye (the 2004 game's controller position): its
  // EyePointOffset in the seat's frame. What the seat sees and aims from.
  seatOrigin(i, out) { return this.seatEye(this.seats[i] ? i : 0, out); }
  seatEye(i, out, presented = false) {
    if (this.system.f2 && !presented) return out.copy(this.f2Pose.seats[i].eye);
    return out.copy(this.seats[i].view.eye).applyMatrix4(this.seatFrame(i, this.frameScratch.frame, presented));
  }
  // A seat's aim direction.
  seatForward(i, out) {
    if (this.system.f2) return out.copy(this.f2Pose.seats[this.seats[i] ? i : 0].forward);
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
    if (this.system.f2 && !presented) return out.copy(this.f2Pose.seats[i].frame);
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
    this.system.world.enableBody(this.body, true);
    this.alive = this.root.visible = true;
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
    const floor = this.system.world.cast(this.from, DOWN, 12, this.hit, this.body);
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
    this.system.world.moveBody(this.body, this.matrix);
    this.itemTarget.center.copy(this.center).applyMatrix4(this.matrix);
  }

  alignGround(normal, blend) {
    const sin = Math.sin(this.yaw), cos = Math.cos(this.yaw);
    const x = cos * normal.x - sin * normal.z, z = sin * normal.x + cos * normal.z;
    this.tilt += (-Math.atan2(z, normal.y) - this.tilt) * blend;
    this.roll += (-Math.atan2(x, Math.hypot(normal.y, z)) - this.roll) * blend;
  }

  // The world owns health; combat and repair send synchronous commands.
  damage(amount, attacker = null) { return this.system.f2.command(this, 0, amount, attacker); }

  // The world owns health; combat and repair send synchronous commands.
  setHealth(value) { return this.system.f2.command(this, 1, value); }

  // Replica birth seeds renderer history before its first pilot correction.
  // Ordinary tick endpoints come from the native hull session or F2 phase.
  resetPresentationHistory() {
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
  // before its last tick (walkFromPhase, the native prediction session),
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
    const w = this.system.world, L = legs.rayLength;
    const q = this.frameScratch.q.setFromEuler(this.legEuler.set(this.root.rotation.x, this.root.rotation.y, this.root.rotation.z, 'YXZ'));
    this.legSide ||= legs.pairs.map(() => 0);
    const enabled = w.bodyEnabled(this.body);
    w.enableBody(this.body, false);
    try {
      legs.pairs.forEach((pair, i) => {
        const h = ['left', 'right'].map((which) => {
          this.from.fromArray(pair[which].at).applyQuaternion(q).add(this.root.position);
          this.from.y += L;
          const hit = w.cast(this.from, DOWN, 2 * L, this.hit);
          return hit ? L - hit.distance : -L;
        });
        this.legSide[i] += (THREE.MathUtils.clamp(-.6 * (h[0] - h[1]), -1, 1) - this.legSide[i]) * Math.min(1, .4 * dt * 60);
      });
    } finally { w.enableBody(this.body, enabled); }
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

  // The drawn pose (the page's; also the simulation host's after aimTick):
  // the hull between ticks, every bone from the walk cycle, the manned
  // seats' turret nodes and aimer bones from their angles (no integration:
  // dt is 0 on a page showing a worker's battle). Its effects and sounds
  // are the page's, from the hull's state (src/vehicle-effects.js,
  // rust/audio-control/src/vehicles.rs).
  //
  // ticked: aimTick has just posed the hull at this alpha (Vehicles.update).
  // A craft in flight whose flight this host steps, with no walk pose and
  // no legs, is then drawn as aimTick posed it: its root has no bob, its
  // gear is posed at its own, its seats from the same angles, and nothing
  // else of it moves; only its barrels' kick, on the clock, is read again.
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
        // The parent's world matrix, from the root down; read as three's
        // worldToLocal and getWorldQuaternion read it, without each walking
        // the same chain to the same matrix again.
        bone.parent.updateWorldMatrix(true, false);
        const parentWorld = bone.parent.matrixWorld;
        if (dt > 0) {
          // Where to turn, in the node's rest frame: the target (only a
          // turn reads it; a pose with no time, dt 0, keeps the angles).
          if (target) s.v.copy(target).applyMatrix4(TO_LOCAL.copy(parentWorld).invert()).sub(bone.position).applyQuaternion(s.q.copy(rest).invert());
          // Nothing to aim at: along the seat's aim.
          else {
            parentWorld.decompose(DECOMPOSED_AT, s.q, DECOMPOSED_SCALE);
            aimVector(seat.aimYaw, seat.pitch, s.v).applyQuaternion(s.q.multiply(rest).invert());
          }
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
  // second (on the clock of whoever poses it: Vehicles.clock).
  recoilBarrels() {
    const now = this.system.world.clock();
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
      if (best) best.kickAt = this.system.world.clock();
    }
  }

  // Someone holds seat i: the player or a lease.
  held(i) { return Boolean(this.seats[i]?.lease) || Boolean(this.driver) && this.seatIndex === i; }

  // Where a weapon with no fire point node fires from: the hull's nose.
  nose(out) {
    return out.set(this.center.x, this.center.y + .5, this.center.z + this.size.z * .5).applyMatrix4(this.matrix);
  }

  // Where a beam from one of seat i's barrels (its fire point's node and its
  // aimer; the hull's nose for neither) points while it lasts: as that
  // barrel shoots now.
  botAim(i, out) {
    const input = this.seats[i].lease?.input;
    return input?.aiming ? out.copy(input.aim) : this.seatForward(i, out);
  }

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

  beamDirection(i, node, aimer, out) {
    const from = node ? node.getWorldPosition(this.beamFrom) : this.nose(this.beamFrom);
    return this.shotDirection(aimer, from, this.seats[i].target, this.crewed(i) ? this.botAim(i, out) : null, out);
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
    // Everything the hulls ask of the rest of the battle (src/vehicles-world.js),
    // and what their seats ask (src/seats-world.js).
    this.world = new VehiclesWorld(this);
    this.seatsWorld = new SeatsWorld(this);
    const mapBounds = collision.bvh?.getBoundingBox(new THREE.Box3());
    if (mapBounds && collision.terrain) mapBounds.union(collision.terrain.getBoundingBox(new THREE.Box3()));
    this.flightEnvironment = { ceiling: maxFlyHeight, ...(mapBounds && !mapBounds.isEmpty() ? { bounds: { min: [mapBounds.min.x, -300, mapBounds.min.z], max: [mapBounds.max.x, maxFlyHeight, mapBounds.max.z] } } : {}), ...flightEnvironment };
    this.units = null;
    // Team relations for boarding (src/boarding.js): the registry's, with
    // its alliances, once attached; else by team alone.
    this.relation = (a, b) => (this.units?.relation ? this.units.relation(a, b) : teamRelation(a, b));
    this.time = 0; // s of vehicle updates
    this.ticks = 0; // input batches so far
    this.seatRandom = null; // () => [0, 1): the world's seat stream (seatWeaponPolicy)
    // s: the clock barrels kick back on. A page's wall clock; a world's is
    // its tick clock (src/game.js), as no step reads a clock of its own.
    this.clock = clockSeconds;
    this.riderPoint = new THREE.Vector3();
    this.rider = { point: new THREE.Vector3(), riderOffset: new THREE.Vector3(), riderQuaternion: new THREE.Quaternion(),
      points: makeRiderPoints(), frame: new THREE.Matrix4(), sample: { clip: null, pitch: 0, turn: 0, time: null } };
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
    this.constructSpawns();
    if (units) this.attachUnits(units);
  }

  // Every hull, now and later, as a target in the units registry
  // (VehicleTarget: kind 2 walker, 5 flyer, 6 hover; no hit spheres).
  attachUnits(units) {
    this.units = units;
    for (const v of this.vehicles) if (units.byId(v.aiTarget.id) !== v.aiTarget) units.add(v.aiTarget);
  }

  // Construct the page facades before the initial native import. Replicas
  // use the same initial list; later births and all clocks belong to Rust.
  constructSpawns() {
    for (const slot of this.slots) {
      if (slot.vehicle?.alive) continue;
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

  // Vehicles.update delegates the complete authoritative phase to Rust.
  update(dt) { this.f2.step(dt, this.ticks + 1); }

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
  // EntityGameState installs a seat packet on the page before its next frame.
  // Place that replica's body from its drawn rig; the world phase uses native
  // rider writeback and never calls this presentation reader.
  placeReplicaRider(lease) {
    const unit = lease.unit;
    if (!unit?.soldier || !unit.actor) return;
    poseRider(lease.vehicle, lease.index, unit.soldier, unit.actor, unit.animator, unit.animator?.clips, unit.presentation, this.riderPoint);
    // The hull has moved under him: his hit shapes follow (Unit.updateSpheres).
    unit.tickCount += 1;
  }

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
    return boardingSeat(vehicle, { team: unit.team, person, relation: this.seatsWorld.relate, preferred: seatIndex });
  }

  // The gun end of a hull's cable: its tow seat's turret node, else its
  // centre. presented: as drawn between ticks.
  towGun(source, out, presented = false) {
    const tether = this.tethers.get(source);
    const turret = source.seats[tether?.seat ?? 1]?.view.turret || source.seats.find((s) => s.view.turret)?.view.turret;
    if (turret) return turret.getWorldPosition(out);
    return out.copy(source.center).applyMatrix4(presented ? source.root.matrixWorld : source.matrix);
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
      if (!(damage > 0) || this.world.blastBlocked(position, this.center, v.body)) continue;
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
    this.lookSettings = { scale: 1, invert: false, flightScale: 1, flightInvert: false };
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
    const settings = this.lookSettings;
    settings.scale = sensitivity; settings.invert = invert;
    settings.flightScale = this.flightSensitivity ?? 1; settings.flightInvert = Boolean(this.flightInvert);
    v.system.f2.seats.look(v, v.seatIndex, dx, dy, settings);
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
  
  // The player's lease on seat index (src/seats.js); his unit's seat while
  // he sits, so soldier raycasts pass him by as they do seated bots.
  

  // After the soldier's animation is sampled: the body at its seat node, or
  // its posed pelvis on a saddle (seatRider). On an enclosed seat (no rider
  // node) his body stays hidden, enforced every frame as presentRiders does
  // for everyone else's: each own correction shows the body again
  // (EntityGameState.correct), which left it standing on top of the hull
  // (the droid starfighter on Bespin: Platforms).
  // The page's own replicated lease arrives independently of its body packet.
  // Attach the view body immediately; native world crews are never driven here.
  syncReplica() {
    const v = this.vehicle;
    if (!v) return;
    poseRider(v, v.seatIndex, this.soldier, this.actor, this.animator, this.clips, this.presentation, this.point);
    // His hit shapes follow the hull (Unit.updateSpheres).
    if (this.unit) this.unit.tickCount += 1;
  }

  presentRider(dt) {
    const v = this.vehicle;
    if (!v) return;
    if (!v.seats[v.seatIndex]?.view.rider?.node) { this.actor.root.visible = false; return; }
    this.riderLean = seatRider(v, v.seatIndex, this.actor, dt, this.riderLean, this);
  }
  
  // EntitySeats releases the page view after a seat packet, death or respawn.
  // The authoritative owner has already chosen promotion and exit placement.
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
    this.lookSettings = { scale: 1, invert: false, flightScale: 1, flightInvert: false };
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
