import { gearFoot } from "./flight/gear.js";
import { initFlight, standHeight } from "./flight/model.js";
import { groundMotion } from './vehicle-motion.js';
import { hullQuaternion, restHeight } from './vehicle-hover.js';
import { vehicleMissileDefinition } from "./weapon-targeting.js";
import { waterDamageRule } from './vehicle-water.js';
import * as THREE from "three";
import { vehicleMuzzleTips } from "./muzzles.js";
import { makeHit } from "./collision.js";
import { vehicleBatches, attachVehicleBatches, vehicleShell } from "./vehicle-rig.js";
import { modelPack } from "./models.js";
import { loadClips, Pose } from "./anim.js";
import { Weapon } from "./weapons.js";
import { VehicleTarget, seatWeaponPolicy, dropBody, unseatBody } from "./seats.js";
import { boardingSeat, canBoard, seatControl, teamRelation } from "./boarding.js";
import { makeRiderPoints, riderDropPoint, riderHitShapes, riderPoints } from "./rider-pose.js";

import { VehiclesWorld } from "./vehicles-world.js";
import { SeatsWorld } from "./seats-world.js";

const UP = new THREE.Vector3(0, 1, 0);
const RIGHT = new THREE.Vector3(1, 0, 0);
const DOWN = new THREE.Vector3(0, -1, 0);
const UNIT_SCALE = new THREE.Vector3(1, 1, 1);
const num = (v, fallback = 0) => Number.isFinite(parseFloat(v)) ? parseFloat(v) : fallback;
const clockSeconds = () => (globalThis.performance?.now?.() ?? Date.now()) / 1000;
const angle = (v) => Math.atan2(Math.sin(v), Math.cos(v));
const idle = Object.freeze({ forward: 0, right: 0, turn: 0, brake: false });
// m a wall probe's or spring's cached triangles reach past its sphere: a
// hull at 30 m/s moves 0.25 m a 120 Hz step.
const WALL_CACHE_MARGIN = 1.5;

const TOW_POINTS = 32; // the cable line's points: the anchor, up to 12 wraps, 16 along the sagging span to the gun

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
// null with no clip. the body phase draws it, and his hit shapes are posed from
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
    const motion = groundMotion(this.def, template.clips);
    const shell = vehicleShell(this, template, motion.turnRate);
    this.root = shell.root; this.bones = shell.bones; this.byName = shell.byName;
    this.bind = shell.bind; this.bindQuaternions = shell.bindQuaternions; this.bindPositions = shell.bindPositions;
    this.clips = template.clips;
    // A walker's clips pose it (the walk cycle, its death); a flyer's takeoff
    // clip only its gear (src/flight/gear.js).
    this.pose = this.clips?.size && !this.flying ? new Pose(this.bones.length, shell.records.poseRoot ?? -1) : null;
    // How far below its origin a craft with gear stands on it (src/flight/
    // gear.js gearFoot; null without a takeoff clip).
    this.gearFoot = this.flying ? template.gearFoot ?? null : null;
    this.walkPhase = 0;
    this.walkBlend = 0;
    this.skeleton = attachVehicleBatches(this.root, this.bones, template.batches, system.caster, { staticBone: shell.staticBone, inverses: shell.inverses });
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
    };
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
    Object.assign(this, motion);
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
    // new Vehicle and seatView come back as native records. These facades
    // retain weapons, effects and camera consumers until their resource
    // owners use numeric handles directly; they do no rig construction math.
    const node = id => id == null ? null : shell.nodes[id];
    this.seats = this.def.seats.map((seat, index) => {
      const record = shell.records.seats[index], view = record.view;
      for (const key of ['eye','center','offset','pivot','up']) view[key] = new THREE.Vector3().fromArray(view[key]);
      view.turret = node(view.turret);
      if (view.rider) view.rider.node = node(view.rider.node);
      const weapons = seat.weapons.map((w, i) => {
        const rig = record.weapons[i], def = vehicleMissileDefinition(w.def);
        const aimers = w.aimers.map((a, j) => ({ ...a, ...rig.aimers[j], bone: node(rig.aimers[j].bone),
          yaw: 0, pitch: 0, onTarget: false, facing: new THREE.Vector3(0,0,1), posed: true, emitter: null }));
        const muzzles = rig.muzzles.map(m => {
          const fireNode = node(m.fireNode), sourceAimer = m.sourceAimer == null ? null : aimers[m.sourceAimer];
          const muzzle = { fireNode, sourceAimer, emitter: beamSource(vehicle,index,fireNode,sourceAimer) };
          if (m.barrel != null || m.rest != null || m.recoil != null) {
            muzzle.barrel = node(m.barrel); muzzle.rest = m.rest ? new THREE.Vector3().fromArray(m.rest) : null; muzzle.recoil = m.recoil;
          }
          return muzzle;
        });
        return { ...w, def, aimers, muzzles, state: new Weapon(def), barrel: 0, emitter: rig.fire == null ? null : beamSource(vehicle,index,node(rig.fire),null) };
      });
      return { ...seat, view, weapons, lease: null, aimYaw: 0, pitch: 0, lookYaw: null, lookPitch: null,
        target: null, yawRate: 0, lastYaw: null, emitter: beamSource(vehicle,index,null,null) };
    });
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
    // Wheels and tow-leg geometry are cold native rig records too. The
    // facade only connects handles and keeps the phase/prediction mirrors.
    this.wheels = shell.records.wheels.map(w => ({ ...w, bone: shell.nodes[w.bone], axis: new THREE.Vector3().fromArray(w.axis), phase: 0, previous: 0 }));
    this.legTop = shell.records.legTop;
    this.legs = shell.records.legs.map(leg => ({ name: leg.name,
      thigh: leg.thigh ? new THREE.Vector3().fromArray(leg.thigh) : null,
      calf: leg.calf ? new THREE.Vector3().fromArray(leg.calf) : null,
      foot: leg.foot ? new THREE.Vector3().fromArray(leg.foot) : null }));
    this.frameScratch = { q: new THREE.Quaternion(), q2: new THREE.Quaternion(), v: new THREE.Vector3(), m: new THREE.Matrix4(), frame: new THREE.Matrix4(), up: new THREE.Vector3() };
    this.beamFrom = new THREE.Vector3();
    this.from = new THREE.Vector3();
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
  // riderHitShapes): his body as the native rider phase poses and places
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
    // A page draws it at once in its native hull phase; a world's native
    // import seeds the pose itself (veh_pull, Hull::seed).
    if (this.system.page) this.system.page.spawned(this);
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

  // Every frame: the hull queued for the page's native hull phase
  // (src/presentation/vehicles.js, rust/bfe-presentation vehicles.rs),
  // which draws it between the last two world ticks, or where its record's
  // sample puts it, every bone as drawn: its walk cycle, manned seats, gear,
  // barrels and wheels, or a dying walker's death clip. Nothing on a page
  // runs aimTick, so this is also how the hull he drives is posed (its
  // predicted view, queued after). dt, the frame's, advances its age, the
  // clock of what is only drawn. Whether it is drawn this frame.
  present(alpha, dt = 0) {
    const page = this.system.page;
    if (!this.alive) return this.dying > 0 && page.dying(this);
    if (!this.nativeNetwork && (!this.tickFrom || this.sleeping)) return false;
    return page.alive(this, alpha, dt);
  }

  // The walk cycle's length in seconds (both steps), 0 without one.
  get walkLength() {
    const left = this.clips?.get('walk_leftfoot_rightfoot'), right = this.clips?.get('walk_rightfoot_leftfoot');
    return left && right ? left.duration + right.duration : 0;
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
  constructor({ scene, data, buffer, indexBuffer, animationBuffer = new ArrayBuffer(0), collision, water, audio, maxFlyHeight = 200, texture, caster, uncaster, camera, ordnance, effects, posts, inDeathRegion, onDestroyed, units = null, onBoard = null, onUnboard = null, onFired = null, onDamaged = null, flightEnvironment = {}, page = null }) {
    // The page's native hull phase (src/presentation/vehicles.js); a world
    // has none.
    this.page = page;
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
    this.rider = { point: new THREE.Vector3(), riderOffset: new THREE.Vector3(),
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

  // Every frame, the vehicles drawn between the last two world ticks,
  // queued for the page's hull phase; dt is the frame's (Vehicle.present).
  // The tethers follow once that phase has posed them (game.js).
  present(alpha, dt = 0) {
    for (const v of this.vehicles) v.present(alpha, dt);
  }

  // Vehicles.update delegates the complete authoritative phase to Rust.
  update(dt) { this.f2.step(dt, this.ticks + 1); }

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
    this.lease = null;
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
