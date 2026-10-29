import * as THREE from "three";
import { makeGround, makeHit } from "./collision.js";
import { SOLDIER_COLLISION, collisionRule, contactDamage } from "./collision-damage.js";

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

const UP = new THREE.Vector3(0, 1, 0);
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
  tick(input, dt) {
    this.previous.copy(this.position);
    this.stateTime += dt;
    this.tickLength = dt;
    this.collisionDamage = 0;
    this.lookX = 0;
    this.lookY = 0;
    const locked = this.lock > 0;
    this.lock = Math.max(this.lock - dt, 0);
    // The fuel refills every tick, jetting or not; with no ground under him
    // last pass, the ground normal is straight up again.
    this.fuel = Math.min(this.fuel + this.jet.recharge * dt, 1);
    if (!this.grounded) this.normal.copy(UP);

    if (this.air === "jet") {
      if (this.jetTick(input, dt)) {
        this.ceiling(dt);
        if (this.grounded) this.land();
      }
      this.collide();
      return;
    }

    const mode = this.mode;
    let f = locked ? 0 : THREE.MathUtils.clamp(input.forward, -1, 1);
    let s = locked ? 0 : THREE.MathUtils.clamp(input.right, -1, 1);
    const len = Math.hypot(f, s);
    if (len > 1) {
      f /= len;
      s /= len;
    }
    const slope = Math.abs(this.normal.y);
    f *= mode.forward * slope;
    s = (this.dive ? this.dive : s) * mode.strafe * slope;
    const brake = this.posture === "stand" && locked && !this.air ? LOCKED_BRAKE : mode.brake;
    this.drive(f, s, brake, dt);
    this.position.addScaledVector(this.velocity, dt);
    if (!locked) this.aimYaw -= this.maxTurn * mode.turn * (input.turn || 0) * dt;
    // A controller's pitch stick (a bot's; the mouse pitches through look()).
    if (!locked && input.pitch) {
      const [low, high] = this.pitchRange();
      this.aimPitch = THREE.MathUtils.clamp(this.aimPitch + this.maxPitch * input.pitch * dt, low, high);
    }

    // The state step runs after the move, on the ground contact the last
    // collision pass found. In a jump or fall, Space lights the jetpack.
    if (this.air) {
      this.ceiling(dt);
      const canJet = (this.air === "jump" || this.air === "fall") && (this.stateTime <= ENGINE.landLock || !this.grounded);
      if (!(input.jump && canJet && this.startJet(input)) && this.stateTime > ENGINE.landLock && this.grounded) this.land();
    } else if (this.dive) {
      if (this.stateTime > this.diveTime) {
        this.dive = 0;
        this.posture = this.diveFrom;
        this.stateTime = 0;
      }
    } else {
      if (this.posture === "stand") this.waitOutDrop();
      if (!locked && !this.air) this.act(input);
    }

    this.collide();
  }

  // Standing, the state timer doubles as a drop timer. Leaving the ground
  // jumps it to 10.3 (or 10 after 5 s of standing); past 11 the soldier falls.
  // Touching down again from there sets it to 5.
  waitOutDrop() {
    let t = this.stateTime;
    if (!this.grounded) {
      if (t >= 5) {
        if (t < 10) t = 10;
        if (t > 11) {
          this.setAir("fall");
          return;
        }
      } else {
        t = 10.3;
      }
    } else if (t >= 10) {
      t = 5;
    }
    this.stateTime = t;
  }

  act(input) {
    if (input.prone) this.setPosture(this.posture === "prone" ? "stand" : "prone");
    if (input.crouch) this.setPosture(this.posture === "crouch" ? "stand" : "crouch");
    if (!input.jump) return;
    const t = this.stateTime;
    // Space with the stick hard to one side dives that way instead, from a
    // stand or a crouch. The diagonal is scaled to length 1 first, so
    // forward-and-sideways still jumps.
    let f = THREE.MathUtils.clamp(input.forward, -1, 1);
    let s = THREE.MathUtils.clamp(input.right, -1, 1);
    const len = Math.hypot(f, s);
    if (len > 1) {
      f /= len;
      s /= len;
    }
    if (this.posture !== "prone" && t < 10.3 && this.diveTime > 0 && Math.abs(s) > ENGINE.diveStick && Math.abs(f) < Math.abs(s)) {
      this.dive = Math.sign(s);
      this.diveFrom = this.posture;
      this.posture = "crouch";
      this.stateTime = 0;
      return;
    }
    if (this.posture === "stand") {
      if (t > ENGINE.ready && t < 10.3) this.jump(input.forward);
    } else if (t > ENGINE.ready) {
      // From a crouch or prone, Space stands the soldier up.
      this.setPosture("stand");
    }
  }

  setPosture(next) {
    if (next === this.posture) return;
    if (next === "stand" && !this.headroom()) return;
    if (this.posture === "prone" && next === "stand") this.lock = this.getupTime;
    this.posture = next;
    this.stateTime = 0;
  }

  setAir(kind) {
    this.air = kind;
    this.posture = "stand";
    this.stateTime = 0;
  }

  land() {
    if (this.stateTime > ENGINE.hardLanding) this.lock = ENGINE.hardLock;
    this.air = null;
    this.posture = "stand";
    this.stateTime = 0;
  }

  // The jetpack's kick: JetJump m/s along (0, 1.1, 0) plus the facing times
  // the forward stick (no less than -0.1) plus a tenth of the strafe.
  startJet(input) {
    const jet = this.jet;
    if (this.fuel < jet.minBorder) return false;
    this.fuel = Math.max(this.fuel - jet.initial, 0);
    const [f, s] = this.stick(input);
    const sin = Math.sin(this.aimYaw);
    const cos = Math.cos(this.aimYaw);
    const forward = Math.max(f, -0.1);
    // The soldier's left is (cos, 0, -sin); strafe right is away from it.
    const dir = this.push.set(sin * forward - 0.1 * s * cos, 1.1, cos * forward + 0.1 * s * sin).normalize();
    // A bot's jump jet launches along the direction its path asks for, as
    // given (not normalized).
    const aimed = input.jetDirection;
    if (aimed && (aimed.x || aimed.y || aimed.z)) dir.copy(aimed);
    this.velocity.addScaledVector(dir, jet.jump);
    this.setAir("jet");
    return true;
  }

  // One tick of the jet. Returns false when it ends this tick (fuel out or
  // Space again): he falls, and doesn't move this tick.
  jetTick(input, dt) {
    const jet = this.jet;
    const v = this.velocity;
    this.posture = "stand";
    const [f, s] = this.stick(input);
    const pitch = this.aimPitch;
    let forward;
    let strafe;
    let brake;
    if (jet.hover) {
      v.y += (ENGINE.jetLift - ENGINE.jetDamping * (v.y - ENGINE.jetLiftSpeed)) * dt;
      v.y += jet.push * ENGINE.jetClimb * Math.sin(pitch) * f * dt;
      forward = ENGINE.jetSpeed * Math.cos(pitch) * f;
      strafe = ENGINE.jetSpeed * s;
      brake = ENGINE.jetBrake;
      this.fuel -= jet.cost * dt;
      if (this.fuel < 0) {
        this.fuel = 0;
        this.setAir("fall");
        return false;
      }
      if (input.jump) {
        this.setAir("fall");
        return false;
      }
    } else {
      // A jump jet just pushes up, with no fuel to burn, until he lands.
      v.y += jet.push * dt;
      forward = ENGINE.jetLowSpeed * f;
      strafe = ENGINE.jetLowSpeed * s;
      brake = 0;
    }
    this.drive(forward, strafe, brake, dt, true);
    this.position.addScaledVector(v, dt);
    this.aimYaw -= this.maxTurn * MODES.jet.turn * (input.turn || 0) * dt;
    return true;
  }

  // Too high: pushed back down within ceilingBand of MaxFlyHeight.
  ceiling(dt) {
    const h = this.maxFlyHeight - this.position.y;
    if (h < ENGINE.ceilingBand) this.velocity.y -= ENGINE.ceilingPush * (ENGINE.ceilingBand - h) * dt;
  }

  // The stick, clamped and scaled down to length 1.
  stick(input) {
    let f = THREE.MathUtils.clamp(input.forward, -1, 1);
    let s = THREE.MathUtils.clamp(input.right, -1, 1);
    const len = Math.hypot(f, s);
    if (len > 1) {
      f /= len;
      s /= len;
    }
    return [f, s];
  }

  jump(forward) {
    const kick = this.maxJump * ENGINE.jumpKick * THREE.MathUtils.clamp(forward, -1, 1);
    this.velocity.y = this.maxJump;
    this.velocity.x += Math.sin(this.aimYaw) * kick;
    this.velocity.z += Math.cos(this.aimYaw) * kick;
    this.setAir("jump");
  }

  // jet: the jetpack's drive, which brakes in the air too and drives at
  // jetBrake of the usual rate however steep the last ground was.
  drive(f, s, brake, dt, jet = false) {
    const v = this.velocity;
    const n = this.normal;
    const sin = Math.sin(this.aimYaw);
    const cos = Math.cos(this.aimYaw);
    // Project facing onto the ground and build an orthonormal tangent basis.
    // Unnormalised cross products shorten drive and mix strafe into forward
    // as the view turns diagonally across a slope.
    this.left.set(cos, 0, -sin);
    this.facing.set(sin, 0, cos);
    this.forwardAxis.copy(this.facing).addScaledVector(n, -this.facing.dot(n)).normalize();
    this.rightAxis.crossVectors(this.forwardAxis, n).normalize();
    if (this.grounded) {
      this.pull.set(0, -this.accel * ENGINE.slopePull * dt, 0);
      this.pull.addScaledVector(n, -n.dot(this.pull));
      v.add(this.pull);
      const into = v.dot(n);
      if (into < 0 || !this.air) v.addScaledVector(n, -into);
    }
    let vf = v.dot(this.forwardAxis);
    let vr = v.dot(this.rightAxis);
    v.addScaledVector(this.forwardAxis, -vf).addScaledVector(this.rightAxis, -vr);
    if (brake > 0 && (jet || (this.grounded && n.y > ENGINE.steepNormal))) {
      const slow = this.accel * ENGINE.brake * brake * dt;
      vf = vf > 0 ? Math.max(vf - slow, 0) : Math.min(vf + slow, 0);
      vr = vr > 0 ? Math.max(vr - slow, 0) : Math.min(vr + slow, 0);
    }
    let gain = this.accel * ENGINE.drive;
    if (jet) {
      gain *= ENGINE.jetBrake;
    } else {
      if (!this.grounded) gain *= ENGINE.airDrive;
      if (n.y < ENGINE.steepNormal) gain *= ENGINE.steepDrive;
    }
    gain *= dt;
    const wantF = this.maxSpeed * f;
    const wantR = this.maxStrafe * s;
    if (f > 0 && vf < wantF) vf = Math.min(vf + gain, wantF);
    if (f < 0 && vf > wantF) vf = Math.max(vf - gain, wantF);
    if (s > 0 && vr < wantR) vr = Math.min(vr + gain, wantR);
    if (s < 0 && vr > wantR) vr = Math.max(vr - gain, wantR);
    v.addScaledVector(this.forwardAxis, vf).addScaledVector(this.rightAxis, vr);
    if (!this.grounded) v.y -= (this.air === "thrown" ? ENGINE.thrownGravity : ENGINE.gravity) * dt;
  }

  // One collision pass: walk from where the last pass left the soldier to the
  // new position in short steps, pushing each sphere out of the terrain and
  // then out of the nearest polygons, one contact at a time.
  collide() {
    const parts = BODY.parts[this.air ? "stand" : this.posture];
    const walking = this.grounded && !this.air && this.posture !== "prone";
    const intendedY = this.position.y;
    this.grounded = false;
    this.motion.subVectors(this.position, this.settled);
    const length = this.motion.length();
    if (length <= 100) {
      const count = Math.floor(1 + length / BODY.step[this.air ? "stand" : this.posture]);
      this.motion.divideScalar(count);
      this.position.copy(this.settled);
      for (let i = 0; i < count; i++) {
        this.stepStart.copy(this.position);
        this.stepBlocked = false;
        this.stepVelocity.copy(this.velocity);
        this.position.add(this.motion);
        this.stepTarget.copy(this.position);
        this.touchTerrain(parts);
        this.touchPolygons(parts);
        if (walking) {
          this.stepUp(parts);
          this.snapGround();
        }
      }
    }
    this.settled.copy(this.position);

    // With no single ground contact, the mean of the lower body's upward
    // contacts can still stand in for one (a soldier wedged in a crease).
    const mean = this.contactMean;
    const lengthSq = mean.lengthSq();
    if (lengthSq > 0.0001) {
      mean.divideScalar(Math.sqrt(lengthSq));
      if (mean.y > ENGINE.groundNormal && !this.grounded) {
        this.normal.copy(mean);
        this.grounded = true;
      }
    }
    mean.set(0, 0, 0);
    this.contactCount = 0;
    if (walking && this.grounded) this.groundAdjustment += this.position.y - intendedY;
  }

  // A low riser presents a wall normal to the sphere before its center is
  // above the tread. Probe the leading edge, then sweep both body spheres up
  // and across before accepting the step. Tall walls and low ceilings remain
  // solid, and jumping never acquires an artificial grounded contact.
  stepUp(parts) {
    // A slope also pushes the lower sphere slightly backward. Treating that
    // as a riser lifts the body to the floor half a metre ahead, then drops
    // it next tick: a repeatable 20 to 30 cm stair/ramp pogo. Step only after a
    // lower-body riser/edge contact blocked the horizontal movement.
    if (!this.stepBlocked) return;
    const dx = this.stepTarget.x - this.stepStart.x;
    const dz = this.stepTarget.z - this.stepStart.z;
    const distance = Math.hypot(dx, dz);
    if (distance < 1e-5) return;
    const lost = (this.stepTarget.x - this.position.x) * dx + (this.stepTarget.z - this.position.z) * dz;
    if (lost < distance * 0.005) return;
    const lower = parts.find((part) => part.role === "lower");
    this.partCenter(lower, this.origin);
    this.origin.x = this.stepTarget.x + Math.sin(this.aimYaw) * lower.z + dx / distance * lower.r;
    this.origin.z = this.stepTarget.z + Math.cos(this.aimYaw) * lower.z + dz / distance * lower.r;
    this.origin.y = this.stepStart.y + BODY.stepHeight + BODY.slack;
    const hit = this.world.cast(this.origin, DOWN, BODY.stepHeight + BODY.slack, this.hit);
    if (!hit || hit.normal.y <= ENGINE.groundNormal) return;
    const rise = hit.point.y - this.stepStart.y;
    if (rise <= BODY.slack || rise > BODY.stepHeight) return;
    this.stepResolved.copy(this.position);
    const height = hit.point.y;
    const clear = () => parts.every((part) => !this.world.overlapsSphere(this.partCenter(part, this.center), part.r - BODY.slack));
    const samples = Math.ceil((rise + distance) / 0.1);
    for (let i = 1; i <= samples; i++) {
      this.position.copy(this.stepStart);
      this.position.y += rise * i / samples;
      if (!clear()) { this.position.copy(this.stepResolved); return; }
    }
    for (let i = 1; i <= samples; i++) {
      this.position.lerpVectors(this.stepStart, this.stepTarget, i / samples);
      this.position.y = height;
      if (!clear()) { this.position.copy(this.stepResolved); return; }
    }
    this.velocity.copy(this.stepVelocity);
    this.velocity.y = Math.min(this.velocity.y, 0);
    this.touchTerrain(parts);
    this.touchPolygons(parts);
  }

  snapGround() {
    if (this.grounded) return;
    this.origin.set(this.position.x + Math.sin(this.aimYaw) * 0.15, this.position.y + BODY.slack * 2,
      this.position.z + Math.cos(this.aimYaw) * 0.15);
    const hit = this.world.cast(this.origin, DOWN, BODY.groundSnap + BODY.slack * 2, this.hit);
    if (!hit || hit.normal.y <= ENGINE.groundNormal) return;
    // Polygon floors support the sphere along the face normal. A feet-only
    // snap embeds the sphere in a slope and the next contact lifts it again.
    const height = hit.point.y + (hit.terrain ? 0 : BODY.parts.stand[0].r / hit.normal.y - BODY.parts.stand[0].y);
    const drop = this.position.y - height;
    if (drop < -BODY.slack || drop > BODY.groundSnap) return;
    this.position.y = height - BODY.slack * 0.25;
    this.normal.copy(hit.normal);
    this.grounded = true;
    const into = this.velocity.dot(this.normal);
    this.velocity.addScaledVector(this.normal, -into);
  }

  partCenter(part, out) {
    const sin = Math.sin(this.aimYaw);
    const cos = Math.cos(this.aimYaw);
    return out.set(this.position.x + sin * part.z, this.position.y + part.y, this.position.z + cos * part.z);
  }

  // The terrain is a height field: a sphere touches it when its bottom is below
  // the ground under its center, by no more than its diameter. That depth,
  // taken along the ground normal, is pushed out.
  touchTerrain(parts) {
    for (const part of parts) {
      const c = this.partCenter(part, this.center);
      const ground = this.world.ground(c.x, c.z, 1e4, this.groundHit);
      if (!ground) continue;
      const sink = ground.height - c.y + part.r;
      if (sink < 0 || sink > 2 * part.r) continue;
      this.contactNormal.copy(ground.normal);
      this.contact(part, this.contactNormal, sink * ground.normal.y, this.contactNormal, "terrain");
    }
  }

  // Against polygons each sphere takes the nearest one it overlaps, is pushed
  // out along the line from that polygon's closest point to its center, and
  // looks again from where it ended up. Each polygon is used once a step.
  touchPolygons(parts) {
    for (const part of parts) {
      const c = this.partCenter(part, this.center);
      const polygons = this.world.gather(c, part.r, false, this.gatherCache(part), this.passThrough);
      for (;;) {
        let best = null;
        let bestTriangle = null;
        let distanceSq = part.r * part.r;
        for (const polygon of polygons) {
          if (polygon.done) continue;
          for (const tri of polygon.triangles) {
            tri.closestPointToPoint(c, this.closest);
            const dSq = this.closest.distanceToSquared(c);
            if (dSq < distanceSq) {
              distanceSq = dSq;
              best = polygon;
              bestTriangle = tri;
              this.point.copy(this.closest);
            }
          }
        }
        if (!best) break;
        const distance = Math.sqrt(distanceSq);
        best.done = true;
        bestTriangle.getNormal(this.surfaceNormal);
        if (distance >= 1e-4) this.contactNormal.subVectors(c, this.point).divideScalar(distance);
        else {
          this.contactNormal.copy(this.surfaceNormal);
          // Exactly on a face, the closest-point direction has zero length.
          // Resolve toward the side occupied before this movement step.
          this.closest.subVectors(this.stepStart, this.position);
          if (this.contactNormal.dot(this.closest) < 0) this.contactNormal.negate();
        }
        if (this.surfaceNormal.y < 0) this.surfaceNormal.negate();
        this.before.copy(this.position);
        if (Math.abs(this.contactNormal.y) < 0.3) this.avoidance.add(this.contactNormal);
        this.contact(part, this.contactNormal, part.r - distance, this.surfaceNormal, best.moving ? null : "building");
        c.add(this.before.subVectors(this.position, this.before));
      }
    }
  }

  // Each body sphere keeps its own nearby-triangle cache (collision.js).
  gatherCache(part) {
    if (!this.world.gatherCache) return null;
    this.gatherCaches ??= new Map();
    let cache = this.gatherCaches.get(part);
    if (!cache) this.gatherCaches.set(part, cache = this.world.gatherCache());
    return cache;
  }

  // The engine's soldier contact rule. `n` points from the world toward the
  // sphere and `depth` is how far to push along it. kind: "terrain" or
  // "building" for what can hurt him (src/collision-damage.js), else null.
  contact(part, n, depth, surface = n, kind = null) {
    const v = this.velocity;
    const g = this.normal;
    const vn = v.dot(n);
    const push = this.pushDir.copy(n);
    const clip = this.clipDir.copy(n);
    // A rounded sphere on the edge of a horizontal tread has a diagonal
    // separation normal. Use that to clear geometry, but use the actual
    // supporting face for movement: otherwise each stair edge becomes a
    // little launch ramp and changes the driving axes every tick.
    const support = n.y > ENGINE.groundNormal && surface.y > ENGINE.groundNormal ? surface : n;
    if (part.role === "lower" && support !== n) clip.copy(support);
    if (part.role === "lower" && n.y >= 0 && (surface.y <= ENGINE.groundNormal || n.dot(surface) < 0.98)
      && n.x * this.motion.x + n.z * this.motion.z < -1e-5) this.stepBlocked = true;
    let d = depth;
    if (this.posture === "prone" && !this.air) {
      // Prone, ground contacts average into the ground normal, and walls push
      // along the ground and stop only sideways motion.
      if (n.y > ENGINE.groundNormal) {
        if (vn < ENGINE.leaving && !(this.grounded && n.y <= g.y)) {
          g.lerp(support, 0.5).normalize();
          this.grounded = true;
        }
      } else {
        const into = g.dot(push);
        if (into < 0) d = alongPlane(push, g, -into, d);
        flatten(clip);
      }
    } else if (part.role === "lower") {
      // The lower body stands on whatever faces up more than groundNormal,
      // keeping the most level contact. Anything steeper is a wall: it stops
      // only horizontal motion, and from below it pushes only sideways.
      if (n.y > 0) {
        this.contactCount++;
        this.contactMean.lerp(support, 1 / this.contactCount);
      }
      if (n.y > ENGINE.groundNormal) {
        if (vn < ENGINE.leaving && !(this.grounded && n.y <= g.y)) {
          g.copy(support);
          this.grounded = true;
        }
      } else {
        if (n.y < 0) {
          push.y = 0;
          const len = push.length();
          if (len > 0) {
            push.divideScalar(len);
            d /= len;
          }
        }
        flatten(clip);
      }
    } else if (part.role === "upper") {
      // The upper body never stops vertical motion on a ledge, and a ceiling
      // over sloped ground pushes along the slope.
      if (n.y > 0) flatten(clip);
      else if (n.y < 0 && this.grounded) d = alongPlane(push, g, -g.dot(push), d);
    }

    const along = clip.dot(v);
    // Stopped by it too fast, it hurts (src/collision-damage.js): the speed
    // into it along n, a building's or prop's upward share only. A contact
    // that stops none of his motion (the upper body over a ledge, a slope
    // too steep to stand on) does none: there his fall goes on.
    if (kind && along < 0) {
      const forward = n.x * Math.sin(this.aimYaw) + n.z * Math.cos(this.aimYaw);
      this.collisionDamage += contactDamage(this.collisionRule, kind === "building" ? vn * n.y : vn, n.y, forward);
    }
    if (along < 0) v.addScaledVector(clip, -along);
    if (Math.abs(v.x) < 0.1) v.x = 0;
    if (Math.abs(v.z) < 0.1) v.z = 0;
    if (d > 2 && d > depth) d = 0; // a push that grew this much is a squeeze, not a contact
    this.push.copy(push).multiplyScalar(d);
    if (this.push.y > 0) this.push.y = Math.max(this.push.y - BODY.slack, 0);
    this.position.add(this.push);
  }

  // Room to stand: a ray up from knee height.
  headroom() {
    const [from, length] = BODY.standClear;
    this.origin.set(this.position.x, this.position.y + from, this.position.z);
    return !this.world.cast(this.origin, UP, length, this.hit);
  }
}

// Zero a direction's height and rescale it to unit length (or leave it zero).
function flatten(v) {
  v.y = 0;
  const len = Math.hypot(v.x, v.z);
  if (len > 0) v.divideScalar(len);
}

// Remove `into` x plane from a push direction, renormalize it, and stretch the
// depth so the push still clears the same distance. Returns the new depth.
function alongPlane(push, plane, into, depth) {
  push.addScaledVector(plane, into);
  const len = push.length();
  if (len <= 0) return depth;
  push.divideScalar(len);
  return depth / len;
}
