// A soldier's movement (Soldier.tick, src/soldier.js) in WebAssembly: the
// ray kernel's module (rust/sim/src/movement) runs the whole tick, the
// collision pass included, against its own copies of the collision world,
// with the same answers as the JavaScript to the last bit. ?movement=wasm
// turns it on (it needs the ray kernel, which it loads); the world's ticks
// (the world worker, a dedicated server) and the page's replay of its own
// soldier then both run here, each with its own kernel.
//
// Each tick copies the soldier's state into the module's exchange slots, and
// the new state back out. The soldier object stays the only copy between
// ticks, so whatever else reads or sets its fields (bots, seats, corrections,
// explosions) goes on as before. Math.sin and Math.cos stay JavaScript's: the
// module takes the yaw's (and a hovering jet's pitch's) from here, and stops
// after a turn for the turned yaw's. Bodies are switched on and off by a
// plain field, so their `enabled` goes in before each tick.

// The exchange slots (rust/sim/src/movement/mod.rs).
const POSITION = 0, SETTLED = 3, VELOCITY = 6, NORMAL = 9, AVOIDANCE = 12, YAW = 15, PITCH = 16, STATE_TIME = 17, LOCK = 18,
  FUEL = 19, DIVE = 20, POSTURE = 21, AIR = 22, DIVE_FROM = 23, GROUNDED = 24, GROUND_ADJUSTMENT = 25, DAMAGE = 26,
  MAX_FLY = 27, GETUP = 28, DIVE_TIME = 29, PASS_THROUGH = 30, DT = 31, FORWARD = 32, RIGHT = 33, TURN = 34,
  PITCH_STICK = 35, JUMP = 36, CROUCH = 37, PRONE = 38, JET_AIMED = 39, JET_DIRECTION = 40, SIN0 = 43, COS0 = 44,
  SIN_PITCH = 45, COS_PITCH = 46, SIN1 = 47, COS1 = 48, TURNED_YAW = 49, CLASS = 50, IO_LEN = 80;
const POSTURES = ["stand", "crouch", "prone"];
const AIRS = [null, "jump", "fall", "thrown", "jet"];

function postureCode(posture) {
  return posture === "stand" ? 0 : posture === "crouch" ? 1 : posture === "prone" ? 2 : -1;
}

function airCode(air) {
  return air === null ? 0 : air === "jump" ? 1 : air === "fall" ? 2 : air === "thrown" ? 3 : air === "jet" ? 4 : -1;
}

export class MovementKernel {
  // collision: a CollisionWorld built with a ray kernel (src/ray-kernel.js).
  constructor(collision) {
    if (!collision.rays) throw Error("The movement kernel needs the collision's ray kernel");
    this.collision = collision;
    this.rays = collision.rays;
    this.x = collision.rays.x;
    this.world = collision.rayWorld;
    this.io = null;
    this.flags = null;
    this.refresh();
  }

  // Views of the module's memory; growing it detaches the old ones.
  refresh() {
    const buffer = this.x.memory.buffer;
    this.io = new Float64Array(buffer, this.x.movement_io_ptr(), IO_LEN);
    this.flags = new Uint8Array(buffer, this.x.movement_flags_ptr(), 1 << 16);
  }

  view() {
    if (this.io.byteLength === 0) this.refresh();
    return this.io;
  }

  // The bodies as the tick will meet them: the module's list in the world's
  // order (each one's polygon numbers went in with it, src/ray-kernel.js),
  // and whether each is enabled now.
  prepare() {
    const collision = this.collision, bodies = collision.bodies;
    collision.rayBodies();
    let flags = this.flags;
    if (flags.byteLength === 0) { this.refresh(); flags = this.flags; }
    for (let i = 0; i < bodies.length; i++) flags[i] = bodies[i].enabled ? 1 : 0;
  }

  // A soldier's id in the module, with its class's numbers.
  add(s) {
    const io = this.view();
    const jet = s.jet, rule = s.collisionRule;
    io[CLASS] = s.maxSpeed; io[CLASS + 1] = s.maxStrafe; io[CLASS + 2] = s.maxJump; io[CLASS + 3] = s.maxTurn;
    io[CLASS + 4] = s.maxPitch; io[CLASS + 5] = s.accel; io[CLASS + 6] = s.pitchLimits[0]; io[CLASS + 7] = s.pitchLimits[1];
    io[CLASS + 8] = jet.jump; io[CLASS + 9] = jet.push; io[CLASS + 10] = jet.recharge; io[CLASS + 11] = jet.cost;
    io[CLASS + 12] = jet.initial; io[CLASS + 13] = jet.minBorder; io[CLASS + 14] = jet.hover ? 1 : 0;
    io[CLASS + 15] = rule.threshold[0]; io[CLASS + 16] = rule.threshold[1]; io[CLASS + 17] = rule.threshold[2];
    io[CLASS + 18] = rule.scale[0]; io[CLASS + 19] = rule.scale[1]; io[CLASS + 20] = rule.scale[2];
    const id = this.x.mover_new(this.world);
    this.refresh();
    return id;
  }

  // Soldier.tick(input, dt) for soldier `s`; false leaves the tick to the
  // JavaScript (a state the module does not know).
  tick(s, input, dt) {
    const posture = postureCode(s.posture), air = airCode(s.air), diveFrom = postureCode(s.diveFrom);
    if (posture < 0 || air < 0 || diveFrom < 0) return false;
    if (s.mover < 0) s.mover = this.add(s);
    this.prepare();
    const io = this.view();
    const position = s.position, settled = s.settled, velocity = s.velocity, normal = s.normal, avoidance = s.avoidance;
    io[POSITION] = position.x; io[POSITION + 1] = position.y; io[POSITION + 2] = position.z;
    io[SETTLED] = settled.x; io[SETTLED + 1] = settled.y; io[SETTLED + 2] = settled.z;
    io[VELOCITY] = velocity.x; io[VELOCITY + 1] = velocity.y; io[VELOCITY + 2] = velocity.z;
    io[NORMAL] = normal.x; io[NORMAL + 1] = normal.y; io[NORMAL + 2] = normal.z;
    io[AVOIDANCE] = avoidance.x; io[AVOIDANCE + 1] = avoidance.y; io[AVOIDANCE + 2] = avoidance.z;
    const yaw = s.aimYaw;
    io[YAW] = yaw; io[PITCH] = s.aimPitch; io[STATE_TIME] = s.stateTime; io[LOCK] = s.lock; io[FUEL] = s.fuel; io[DIVE] = s.dive;
    io[POSTURE] = posture; io[AIR] = air; io[DIVE_FROM] = diveFrom; io[GROUNDED] = s.grounded ? 1 : 0;
    io[GROUND_ADJUSTMENT] = s.groundAdjustment; io[MAX_FLY] = s.maxFlyHeight; io[GETUP] = s.getupTime; io[DIVE_TIME] = s.diveTime;
    io[PASS_THROUGH] = s.passThrough?.raySlot ?? -1;
    io[DT] = dt;
    io[FORWARD] = input.forward; io[RIGHT] = input.right; io[TURN] = input.turn || 0; io[PITCH_STICK] = input.pitch || 0;
    io[JUMP] = input.jump ? 1 : 0; io[CROUCH] = input.crouch ? 1 : 0; io[PRONE] = input.prone ? 1 : 0;
    const aimed = input.jetDirection;
    if (aimed && (aimed.x || aimed.y || aimed.z)) {
      io[JET_AIMED] = 1; io[JET_DIRECTION] = aimed.x; io[JET_DIRECTION + 1] = aimed.y; io[JET_DIRECTION + 2] = aimed.z;
    } else io[JET_AIMED] = 0;
    io[SIN0] = Math.sin(yaw); io[COS0] = Math.cos(yaw);
    if (air === 4 && s.jet.hover) { io[SIN_PITCH] = Math.sin(s.aimPitch); io[COS_PITCH] = Math.cos(s.aimPitch); }
    let code = this.x.mover_tick(s.mover);
    if (code === 1) {
      const turned = this.view(), to = turned[TURNED_YAW];
      turned[SIN1] = Math.sin(to); turned[COS1] = Math.cos(to);
      code = this.x.mover_resume(s.mover);
    }
    if (code !== 0) return false;
    const out = this.view();
    s.previous.copy(position);
    position.set(out[POSITION], out[POSITION + 1], out[POSITION + 2]);
    settled.set(out[SETTLED], out[SETTLED + 1], out[SETTLED + 2]);
    velocity.set(out[VELOCITY], out[VELOCITY + 1], out[VELOCITY + 2]);
    normal.set(out[NORMAL], out[NORMAL + 1], out[NORMAL + 2]);
    avoidance.set(out[AVOIDANCE], out[AVOIDANCE + 1], out[AVOIDANCE + 2]);
    s.aimYaw = out[YAW]; s.aimPitch = out[PITCH]; s.stateTime = out[STATE_TIME]; s.lock = out[LOCK]; s.fuel = out[FUEL]; s.dive = out[DIVE];
    s.posture = POSTURES[out[POSTURE]]; s.air = AIRS[out[AIR]]; s.diveFrom = POSTURES[out[DIVE_FROM]];
    s.grounded = out[GROUNDED] !== 0; s.groundAdjustment = out[GROUND_ADJUSTMENT]; s.collisionDamage = out[DAMAGE];
    s.tickLength = dt; s.lookX = 0; s.lookY = 0;
    return true;
  }
}
