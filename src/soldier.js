import * as THREE from "three";
import { makeGround, makeHit } from "./collision.js";
import { SOLDIER_COLLISION, collisionRule } from "./collision-damage.js";

// Soldier movement and collision, following the 2004 engine's per-tick soldier
// update.
//
// Velocity is split along the facing and right axes laid onto the ground
// plane. Each axis brakes toward zero, then drives toward MaxSpeed (forward
// and back) or MaxStrafeSpeed (sideways) times the stick. Gravity only acts
// in the air. The class properties come from soldier.json; anything the
// trooper's ODF leaves unset uses the soldier class defaults below.
//
// The jetpack (the jet trooper's): Space pressed again in a jump or fall, with
// fuel at JetFuelMinBorder or more, spends JetFuelInitialCost and kicks the
// soldier by JetJump m/s up and along the stick, and he hovers. A hover lifts
// against gravity and against climbing, so he holds his height; pushing
// forward flies him along the look at up to twice MaxSpeed, climbing as he
// looks up. It burns JetFuelCost a second, and ends when the fuel runs out or
// Space is pressed again (he falls) or he touches down. The fuel refills at
// JetFuelRechargeRate a second all the time, and a spawn starts half full.
// Above the mission's MaxFlyHeight less 5 m anyone in the air is pushed back
// down.
export const CLASS_DEFAULTS = {
  MaxSpeed: 9,
  MaxStrafeSpeed: 7,
  MaxJumpSpeed: 8,
  MaxTurnSpeed: 2,
  MaxPitchSpeed: 0.5,
  Acceleraton: 70,
  PitchLimits: [-0.8, 1.1],
};

export const ENGINE = {
  tick: 1 / 60,
  gravity: 18,
  drive: 1.2, // drive rate = Acceleraton * drive
  airDrive: 0.2, // and that, again, while airborne
  steepDrive: 0.01, // and that on ground steeper than steepNormal
  brake: 0.6, // brake rate = Acceleraton * brake * the posture's brake
  slopePull: 0.4, // downhill pull on the ground = Acceleraton * slopePull
  steepNormal: 0.75, // no braking and almost no control below this normal.y
  groundNormal: 0.6, // a contact counts as ground above this normal.y
  leaving: 0.1, // ...unless the soldier moves away from it faster than this
  jumpKick: 0.25, // forward push = MaxJumpSpeed * jumpKick * forward stick
  landLock: 0.3, // seconds a jump or fall lasts before it can land
  ready: 0.5, // seconds in a stance before Space does anything
  hardLanding: 2, // seconds in the air that make a landing hard
  hardLock: 0.5, // seconds a hard landing holds the soldier still
  diveStick: 0.9, // sideways stick past this turns Space into a dive
  diveLength: 0.75, // share of the dive clip the dive lasts
  thrownGravity: 13.5, // m/s^2 while thrown by an explosion
  jetLift: 13, // hover lift, m/s^2, less jetDamping x (vy - jetLiftSpeed)
  jetDamping: 2.5,
  jetLiftSpeed: 2,
  jetClimb: 4, // JetPush x this x the look's rise x the forward stick
  jetBrake: 0.3, // the hover's brake and drive factor
  jetSpeed: 2, // the hover's stick scale (forward also times the look's cos)
  jetLowSpeed: 0.7, // a jump jet's stick scale
  ceilingBand: 5, // m under MaxFlyHeight where the push down starts
  ceilingPush: 10, // m/s^2 per m into the band
  pronePitch: 0.4,
  lookScale: 2048, // mouse counts per radian per unit of MaxTurnSpeed
  lookClamp: 16, // the turn and pitch rate a mouse may ask for, in stick units
};

// Stick scale per posture and the brake factor handed to the movement step.
const MODES = {
  stand: { forward: 1, strafe: 1, turn: 1, brake: 1.2 },
  crouch: { forward: 0.7, strafe: 0.6, turn: 1, brake: 1 },
  prone: { forward: 0.3, strafe: 0.2, turn: 0.5, brake: 1 },
  air: { forward: 0.2, strafe: 0.2, turn: 0.5, brake: 0 },
  jet: { forward: 1, strafe: 1, turn: 1, brake: 0.3 },
  // A dive drives sideways at 1.7 x MaxStrafeSpeed whatever the stick says.
  dive: { forward: 0.1, strafe: 1.7, turn: 0.6, brake: 1 },
};
const LOCKED_BRAKE = 1.7; // standing brake while getting up or after a hard landing

// The engine builds each posture's collision body from spheres in the
// soldier's frame (y up, z forward). Against the world only these spheres
// count: the lower body carries the ground, and contacts are resolved one at
// a time, each push moving the soldier before the next is found.
export const BODY = {
  parts: {
    stand: [
      { role: "lower", y: 0.57, z: 0.15, r: 0.57 },
      { role: "upper", y: 1.48, z: 0.15, r: 0.57 },
    ],
    crouch: [
      { role: "lower", y: 0.57, z: 0.15, r: 0.57 },
      { role: "upper", y: 1.18, z: 0.15, r: 0.57 },
    ],
    prone: [
      { role: "medium", y: 0.72, z: 0.2023, r: 0.8 },
      { role: "upper", y: 0.3465, z: 0.6075, r: 0.385 },
      { role: "lower", y: 0.3465, z: -0.6075, r: 0.385 },
      { role: "medium2", y: 0.3465, z: -0.2023, r: 0.385 },
    ],
  },
  // Motion is cut into steps no longer than this, so a sphere cannot skip
  // through a wall in one tick.
  step: { stand: 0.57, crouch: 0.45, prone: 0.1375 },
  slack: 0.02, // upward pushes stop this short, so the floor is still touched next tick
  standClear: [0.55, 1.35], // headroom ray to stand up: start height, length
  stepHeight: 0.45, // walk up stairs and platform lips without jumping
  groundSnap: 0.25, // follow shallow descending steps without a fall/land cycle
};

const DOWN = new THREE.Vector3(0, -1, 0);

// The jetpack's class defaults.
const JET_DEFAULTS = { JetJump: 7, JetPush: 14, JetFuelRechargeRate: 0, JetFuelCost: 0.2, JetFuelInitialCost: 0, JetFuelMinBorder: 0.1 };

export class Soldier {
  constructor(world, def) {
    this.world = world;
    const value = (key) => def[key] ?? CLASS_DEFAULTS[key] ?? JET_DEFAULTS[key];
    this.maxSpeed = value("MaxSpeed");
    this.maxStrafe = value("MaxStrafeSpeed");
    this.maxJump = value("MaxJumpSpeed");
    this.maxTurn = value("MaxTurnSpeed");
    this.maxPitch = value("MaxPitchSpeed");
    this.accel = value("Acceleraton");
    this.pitchLimits = value("PitchLimits");
    this.jet = {
      jump: value("JetJump"),
      push: value("JetPush"),
      recharge: value("JetFuelRechargeRate"),
      cost: value("JetFuelCost"),
      initial: value("JetFuelInitialCost"),
      minBorder: value("JetFuelMinBorder"),
      hover: String(def.JetType || "").toLowerCase() === "hover",
    };
    this.fuel = 0;
    // Hitting things too fast hurts (src/collision-damage.js): the class's
    // CollisionThreshold and CollisionScale, and what this tick's contacts
    // did, for whoever owns the body to take.
    this.collisionRule = collisionRule(def, SOLDIER_COLLISION);
    this.collisionDamage = 0;
    this.maxFlyHeight = 200; // the mission's SetMaxFlyHeight
    this.getupTime = 0; // length of the prone get-up clip; the soldier is held still that long
    this.diveTime = 0; // seconds a dive lasts (a share of the dive clip); 0 turns dives off
    this.position = new THREE.Vector3();
    this.previous = new THREE.Vector3();
    this.settled = new THREE.Vector3(); // where the last collision pass left the soldier
    this.velocity = new THREE.Vector3();
    this.normal = new THREE.Vector3(0, 1, 0); // last ground normal; kept while airborne
    this.aimYaw = 0;
    this.aimPitch = 0;
    this.posture = "stand";
    this.air = null; // "jump", "fall", "thrown", or "jet" while in those states
    this.dive = 0; // 1 diving right, -1 diving left
    this.diveFrom = "stand";
    this.stateTime = 0;
    this.lock = 0; // seconds left with movement and look held still
    this.grounded = false;
    this.lookX = 0; // mouse turn already taken this tick, in counts x sensitivity
    this.lookY = 0;
    this.tickLength = ENGINE.tick; // the latest simulation step
    this.contactCount = 0;
    this.contactMean = new THREE.Vector3();
    this.forwardAxis = new THREE.Vector3();
    this.rightAxis = new THREE.Vector3();
    this.left = new THREE.Vector3();
    this.facing = new THREE.Vector3();
    this.pull = new THREE.Vector3();
    this.motion = new THREE.Vector3();
    this.center = new THREE.Vector3();
    this.closest = new THREE.Vector3();
    this.point = new THREE.Vector3();
    this.contactNormal = new THREE.Vector3();
    this.surfaceNormal = new THREE.Vector3();
    this.pushDir = new THREE.Vector3();
    this.clipDir = new THREE.Vector3();
    this.push = new THREE.Vector3();
    this.before = new THREE.Vector3();
    this.origin = new THREE.Vector3();
    this.hit = makeHit();
    this.groundHit = makeGround();
    this.groundAdjustment = 0; // cumulative grounded collision correction for presentation smoothing
    // The sum of near-horizontal building and prop contact normals since a
    // bot's path executor last read it: it steers round what it bumps into.
    this.avoidance = new THREE.Vector3();
    this.stepBlocked = false;
    this.stepStart = new THREE.Vector3();
    this.stepTarget = new THREE.Vector3();
    this.stepVelocity = new THREE.Vector3();
    this.stepResolved = new THREE.Vector3();
    // A body this one passes through (a rider's mount, while his body falls
    // from its back: src/seats.js dropBody), until it spawns again.
    this.passThrough = null;
    // Each body sphere's nearby-triangle cache (gatherCache), made on the
    // first collision pass; and this soldier's id in the world's movement
    // kernel (src/movement-kernel.js), -1 until its first tick there. Both
    // here from the start, so every soldier keeps one shape.
    this.gatherCaches = undefined;
    this.mover = -1;
  }

  spawn(x, y, z, yaw) {
    this.passThrough = null;
    this.position.set(x, y, z);
    this.origin.set(x, y + 1.5, z);
    const hit = this.world.cast(this.origin, DOWN, 50, this.hit);
    if (hit) this.position.y = hit.point.y;
    this.velocity.set(0, 0, 0);
    this.normal.set(0, 1, 0);
    this.aimYaw = yaw;
    this.aimPitch = -0.07;
    this.posture = "stand";
    this.air = null;
    this.dive = 0;
    this.stateTime = 0;
    this.lock = 0;
    this.fuel = this.jet.recharge > 0 ? 0.5 : 0;
    this.grounded = Boolean(hit);
    this.groundAdjustment = 0;
    this.previous.copy(this.position);
    this.settled.copy(this.position);
  }

  get crouched() {
    return this.posture === "crouch";
  }

  get mode() {
    if (this.air === "jet") return MODES.jet;
    if (this.air) return MODES.air;
    return this.dive ? MODES.dive : MODES[this.posture];
  }

  pitchRange() {
    if (this.posture === "prone" && !this.air) return [-ENGINE.pronePitch, ENGINE.pronePitch];
    return this.pitchLimits;
  }

  // Mouse look in raw counts. The engine reads the mouse as a turn and pitch
  // rate of counts x sensitivity / (2048 x frame time), clamped to 16, and
  // turns at MaxTurnSpeed times that: one count is MaxTurnSpeed x sensitivity
  // / 2048 radians whatever the frame rate. Turning takes the posture's turn
  // scale (half prone or in the air); pitch does not. Moving the mouse up
  // looks up unless `invert` is set.
  look(dx, dy, sensitivity = 1, invert = false) {
    if (this.lock > 0) return;
    const cap = ENGINE.lookClamp * ENGINE.lookScale * this.tickLength;
    const x = THREE.MathUtils.clamp(this.lookX + dx * sensitivity, -cap, cap);
    const y = THREE.MathUtils.clamp(this.lookY + (invert ? dy : -dy) * sensitivity, -cap, cap);
    const rate = this.maxTurn / ENGINE.lookScale;
    this.aimYaw -= (x - this.lookX) * rate * this.mode.turn;
    this.aimPitch += (y - this.lookY) * rate;
    this.lookX = x;
    this.lookY = y;
    const [low, high] = this.pitchRange();
    this.aimPitch = THREE.MathUtils.clamp(this.aimPitch, low, high);
  }

  // Raw pointer counts are displacement, not a turn rate. Do not discard
  // counts based on how many render callbacks happened between DOM events.
  // The original capped look() remains available for Classic AI/fixtures.
  lookRaw(dx, dy, sensitivity = 1, invert = false) {
    if (this.lock > 0) return;
    const rate = this.maxTurn / ENGINE.lookScale;
    this.aimYaw -= dx * sensitivity * rate * this.mode.turn;
    const [low, high] = this.pitchRange();
    this.aimPitch = THREE.MathUtils.clamp(this.aimPitch + (invert ? dy : -dy) * sensitivity * rate, low, high);
  }

  // input: forward, right, turn in [-1, 1]; jump, crouch, prone are presses.
  // Rust owns the complete movement tick for worlds and prediction.
  tick(input, dt) {
    this.world.movement.tick(this, input, dt);
  }

  setAir(kind) {
    this.air = kind;
    this.posture = "stand";
    this.stateTime = 0;
  }

  // One tick of the jet. Returns false when it ends this tick (fuel out or
  // Too high: pushed back down within ceilingBand of MaxFlyHeight.

  // jet: the jetpack's drive, which brakes in the air too and drives at
  // One collision pass: walk from where the last pass left the soldier to the
  // A low riser presents a wall normal to the sphere before its center is
  // The terrain is a height field: a sphere touches it when its bottom is below
  // Against polygons each sphere takes the nearest one it overlaps, is pushed
  // Each body sphere keeps its own nearby-triangle cache (collision.js).

  // Room to stand: a ray up from knee height.

}

// Zero a direction's height and rescale it to unit length (or leave it zero).