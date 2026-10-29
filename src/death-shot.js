import * as THREE from "three";
import { makeHit } from "./collision.js";
import { DEATH_BACK, DEATH_BODY, DEATH_BOOM, DEATH_GAP, DEATH_RAY_SPREAD, DEATH_RISE, DEATH_SETTLE, DEATH_TURN, DEATH_TURN_RATE } from "./camera.js";

// The death camera as this page draws it. The world keeps the original's
// (src/camera.js DeathCamera) as the camera its Classic AI schedules from;
// this one is presentation only. It runs on the page in every host (a local
// battle, a world worker's, a server's) from what the page draws: the body
// where it lies, and the killer the world named (DeathCamera.blame) where he
// is drawn. While he is dead it is the drawn camera's only writer, on the
// frame's own clock.
//
// Every move is a function of the time since the death, eased, over points
// that follow what is drawn through critically damped springs. A spring's
// step is exact for any length of frame, so 60, 144 and uneven frames trace
// the same path. The pose it arrives at passes through springs of its own
// as well, so nothing it reads (a killer getting into a hull, a ray starting
// to meet a wall, a change of style) can make it jump.
//
// Classic is the original's framing and timing (docs/formats.md, "Death
// camera"): it rises to look down on the body from 1 m back toward the view
// it took over and 1.5 m up, 5 m back along that line or 1 m short of what
// two rays meet. From 1.5 s it turns to look over the body at the killer and
// follows him, and goes back to the body while he has none. Its blends are
// the original's fractions (t / 1 s, then (t - 1.5 s) / 2 s) applied at the
// original's 60 frames a second, integrated over time.
//
// Enhanced is a replay as a sports broadcast shows one: a hold on the body
// from a high three-quarter angle, drifting slowly round it; a crane move
// round the body to the reverse angle, looking over it at the killer with
// both in frame while the killing shot's trace runs from him to the body;
// then a push in on the killer, the lens narrowing to frame him, following
// him. With no killer it stays on the body, drifting round it. It is over
// in about 3.3 s, before the corpse leaves the world at 4 s.

const UP = new THREE.Vector3(0, 1, 0);
const FRAME = 60; // the original's frames a second, at which its fractions stepped

// Following what is drawn (rad/s; a critically damped spring settles in
// about 5 / rate seconds).
const BODY_RATE = 10;
const KILLER_RATE = 12;
const KILLER_LEAD = 6; // m at most: it aims where he is going by as much as it trails
const SIZE_RATE = 4;
const PRESENCE_RATE = 7; // Classic: turning to the killer, and back, as he has a body or not
const BOOM_IN = 25; // pulled in from a wall quickly,
const BOOM_OUT = 2.5; // let out again slowly
// The drawn pose after the style's: Classic keeps the original's timing,
// Enhanced moves as a crane does.
const FOLLOW = {
  classic: { move: 40, turn: 45, fov: 30 },
  enhanced: { move: 8, turn: 10, fov: 5 },
};
const SWITCH = 0.8; // s: a change of style mid-death blends over this
// The view it takes over glides on with the motion it had and stops, at
// most this fast, dying away at this rate.
const HANDOFF_SPEED = 8; // m/s
const HANDOFF_TURN = 2; // rad/s
const HANDOFF_RATE = 4; // 1/s

// Enhanced (s, m, rad).
const HOLD = 0.9; // to the look down on the body
const HOLD_RADIUS = 3.6;
const HOLD_HEIGHT = 2.6;
const LOOK_DROP = 0.2; // looking this far below the body's middle
const DRIFT = 0.12; // round the body (about 7 degrees a second)
const DRIFT_EASE = 0.6;
const REVEAL_AT = 1; // the move to the reverse angle starts
const REVEAL_DELAY = 0.15; // after a killer named late
const REVEAL = 1.25;
const REVEAL_RADIUS = 3.4;
const REVEAL_HEIGHT = 1.3;
const BODY_FRAME = 0.75; // the body at most this far down the frame's lower half,
const KILLER_EDGE = 0.55; // the killer this far up its upper half
const SETTLE_AT = 1; // after the reveal starts, the push in on the killer
const SETTLE = 1.2;
const SETTLE_RADIUS = 2.6;
const SETTLE_HEIGHT = 1.35;
const KILLER_FRAME = 0.4; // of the frame's height
const MIN_VFOV = THREE.MathUtils.degToRad(11);
const BOOM_RISE = 0.45; // the boom's rays leave from this far above the body
const BOOM_SKIN = 0.35; // and stop this far short of what they meet
const TRACE_RUN = 0.5; // the trace's head from the killer to the body
const TRACE_FADE = [1.7, 2.3]; // then it fades out, after the reveal starts

const PAIR = [0, 0];
// One step of a critically damped spring: x (and its velocity v) after dt,
// following a goal that moved evenly from a0 to a1 over the step. Exact for
// any dt, so how the time is cut into frames doesn't change the path.
export function spring(x, v, a0, a1, rate, dt, out = PAIR) {
  if (!(dt > 0)) {
    out[0] = x;
    out[1] = v;
    return out;
  }
  const speed = (a1 - a0) / dt;
  const lag = (-2 * speed) / rate; // behind a goal moving at that speed
  const c1 = x - a0 - lag;
  const c2 = v - speed + rate * c1;
  const e = Math.exp(-rate * dt);
  out[0] = a1 + lag + (c1 + c2 * dt) * e;
  out[1] = speed + (c2 - rate * (c1 + c2 * dt)) * e;
  return out;
}

// 0 to 1 over [0, 1], starting and stopping without a jolt (smootherstep).
export function ease(u) {
  if (u <= 0) return 0;
  if (u >= 1) return 1;
  return u * u * u * (u * (u * 6 - 15) + 10);
}

// The original's blend over s seconds: each frame it moved a fraction f of
// the way, f growing at rate per second from 0. At its 60 frames a second
// that leaves exp(-60 * integral of -ln(1 - f)) of the way to go.
export function classicBlend(s, rate) {
  if (s <= 0) return 0;
  const f = rate * s;
  if (f >= 1) return 1;
  return 1 - Math.exp(-FRAME * (((1 - f) * Math.log(1 - f)) / rate + s));
}

const wrap = (a) => a - 2 * Math.PI * Math.round(a / (2 * Math.PI));
const lerp = THREE.MathUtils.lerp;
const clamp = THREE.MathUtils.clamp;

// A number following its goal (spring), and an angle doing so the short
// way round.
export class Follow {
  constructor(x = 0) {
    this.reset(x);
  }

  reset(x, v = 0) {
    this.x = this.goal = x;
    this.v = v;
    return this;
  }

  step(goal, rate, dt) {
    spring(this.x, this.v, this.goal, goal, rate, dt);
    this.x = PAIR[0];
    this.v = PAIR[1];
    this.goal = goal;
    return this.x;
  }

  stepAngle(goal, rate, dt) {
    return this.step(this.goal + wrap(goal - this.goal), rate, dt);
  }
}

export class FollowVector {
  constructor() {
    this.value = new THREE.Vector3();
    this.axes = [new Follow(), new Follow(), new Follow()];
  }

  reset(p, velocity = null) {
    this.value.copy(p);
    this.axes[0].reset(p.x, velocity?.x ?? 0);
    this.axes[1].reset(p.y, velocity?.y ?? 0);
    this.axes[2].reset(p.z, velocity?.z ?? 0);
    return this;
  }

  step(goal, rate, dt) {
    const [x, y, z] = this.axes;
    this.value.set(x.step(goal.x, rate, dt), y.step(goal.y, rate, dt), z.step(goal.z, rate, dt));
    return this.value;
  }

  // Where it will be in this many seconds at the speed it has (at most
  // limit metres on).
  ahead(seconds, limit, out) {
    const [x, y, z] = this.axes;
    out.set(x.v, y.v, z.v).multiplyScalar(seconds).clampLength(0, limit);
    return out.add(this.value);
  }
}

// A rotation following its goal: the same spring on the turn between them
// (axis times angle), the goal turning evenly over the step.
const turnA = new THREE.Quaternion();
const turnB = new THREE.Quaternion();
function logTurn(q, out) {
  // The shorter way round.
  const s = q.w < 0 ? -1 : 1;
  const x = q.x * s, y = q.y * s, z = q.z * s, w = q.w * s;
  const length = Math.hypot(x, y, z);
  if (length < 1e-12) return out.set(0, 0, 0);
  const angle = 2 * Math.atan2(length, w);
  return out.set((x / length) * angle, (y / length) * angle, (z / length) * angle);
}
function expTurn(v, out) {
  const angle = v.length();
  if (angle < 1e-12) return out.set(0, 0, 0, 1);
  const s = Math.sin(angle / 2) / angle;
  return out.set(v.x * s, v.y * s, v.z * s, Math.cos(angle / 2));
}
export class FollowTurn {
  constructor() {
    this.quaternion = new THREE.Quaternion();
    this.goal = new THREE.Quaternion();
    this.velocity = new THREE.Vector3(); // rad/s, about world axes
    this.offset = new THREE.Vector3();
    this.goalTurn = new THREE.Vector3();
  }

  reset(q, velocity = null) {
    this.quaternion.copy(q);
    this.goal.copy(q);
    if (velocity) this.velocity.copy(velocity);
    else this.velocity.set(0, 0, 0);
    return this;
  }

  step(goal, rate, dt) {
    if (!(dt > 0)) return this.quaternion;
    // Where it is, and where the goal went, from the goal's last place.
    const inverse = turnA.copy(this.goal).invert();
    logTurn(turnB.multiplyQuaternions(this.quaternion, inverse), this.offset);
    logTurn(turnB.multiplyQuaternions(goal, inverse), this.goalTurn);
    const v = this.velocity, o = this.offset, g = this.goalTurn;
    spring(o.x, v.x, 0, g.x, rate, dt); o.x = PAIR[0]; v.x = PAIR[1];
    spring(o.y, v.y, 0, g.y, rate, dt); o.y = PAIR[0]; v.y = PAIR[1];
    spring(o.z, v.z, 0, g.z, rate, dt); o.z = PAIR[0]; v.z = PAIR[1];
    this.quaternion.multiplyQuaternions(expTurn(o, turnB), this.goal).normalize();
    this.goal.copy(goal);
    return this.quaternion;
  }
}

// The killer where this page draws him, from the world's record of him
// (DeathCamera.blame): { point (his eyes, or a hull's or turret's), size (m,
// for framing), living (he has a body now), friendly }, into out, or null.
// follow: once his life has ended, look for his Character's next body (as
// the original does); otherwise a corpse still lying there is shown.
// refs: { units, characters, vehicles, relation(team) }.
const killerScratch = new THREE.Vector3();
export function drawnKiller(death, refs, follow, out) {
  if (!death || !refs) return null;
  if (death.killerHull >= 0) {
    // A hull credited by itself: while it is the same hull's life.
    const hull = refs.vehicles?.[death.killerHull];
    if (!hull?.alive || (hull.aiTarget?.generation ?? 0) !== death.killerHullLife) return null;
    hull.seatEye(0, out.point, true);
    out.size = 2 * (hull.radius || 3);
    out.living = true;
    out.friendly = (refs.relation?.(hull.team) ?? -1) > 0;
    return out;
  }
  if (death.killerUnit < 0) return null;
  let unit = refs.units?.byId(death.killerUnit) ?? null;
  if (unit && unit.generation !== death.killerLife) unit = null;
  let living = Boolean(unit?.alive && !unit.removed);
  if (!living && follow && death.killerCharacter >= 0) {
    const character = refs.characters?.[death.killerCharacter];
    const next = character?.unit;
    if (next && next.alive && !next.removed && (next.character ?? character) === character) {
      unit = next;
      living = true;
    }
  }
  if (!unit || unit.removed) return null;
  const seat = living ? unit.seat : null;
  if (seat?.vehicle) {
    // His seat's eye, as the hull is drawn.
    seat.vehicle.seatEye(seat.index ?? 0, out.point, true);
    out.size = 2 * (seat.vehicle.radius || 3);
  } else if (seat?.turret) {
    seat.origin(out.point);
    out.size = 3;
  } else {
    // His body as drawn: the eyes standing, crouched or prone, the middle
    // of a corpse.
    const feet = unit.actor?.root?.position ?? unit.presentation?.position ?? unit.position;
    const posture = unit.soldier?.posture;
    const eyes = !living ? 0.5 : unit.walker ? 1 : posture === "prone" ? 0.6 : posture === "crouch" ? 1.2 : 1.7;
    out.point.copy(feet ?? killerScratch.set(0, 0, 0)).y += eyes;
    out.size = unit.walker ? 1.6 : 1.9;
  }
  out.living = living;
  out.friendly = (refs.relation?.(unit.team) ?? -1) > 0;
  return out;
}

export class DeathShot {
  // world: collision for the camera's rays (cast); refs: the battle's units,
  // characters and hulls (drawnKiller); trace: the killing shot's trace
  // (src/death-trace.js), or null.
  constructor({ world, refs = null, trace = null } = {}) {
    this.world = world;
    this.refs = refs;
    this.trace = trace;
    this.active = false;
    this.serial = -1;
    this.time = 0;
    this.style = "enhanced";
    this.switchFrom = null; // the style it is blending from, since switchAt
    this.switchAt = 0;
    // The view it took over.
    this.from = { position: new THREE.Vector3(), quaternion: new THREE.Quaternion(), yaw: 0, pitch: 0, roll: 0, hfov: 1, bearing: 0, radius: 0, height: 0 };
    this.startBody = new THREE.Vector3(); // the body's middle when it started
    this.anchor = new THREE.Vector3(); // Classic: the first near point
    // What it frames, following what is drawn.
    this.body = new FollowVector();
    this.killer = new FollowVector();
    this.killerAim = new THREE.Vector3(); // where it aims: ahead of him by the springs' lag
    this.killerSize = new Follow(1.9);
    this.presence = new Follow(0);
    this.known = false; // a killer named
    this.knownAt = 0;
    this.living = false;
    this.friendly = false;
    this.shotFrom = new THREE.Vector3(); // where the killer stood when named
    this.bearingR = null; // Enhanced: the reverse angle's bearing, kept the same way round
    // Booms (fractions of the reach clear of walls).
    this.boomA = new Follow(1);
    this.boomK = new Follow(1);
    this.boom = new Follow(1);
    this.fresh = { boomA: true, boomK: true, boom: true };
    // Each style's pose this frame (Enhanced's with the boom's foot and how
    // much the boom holds yet), the two mixed while the style changes, and
    // the drawn pose following them.
    const pose = () => ({ position: new THREE.Vector3(), quaternion: new THREE.Quaternion(), hfov: 1, top: new THREE.Vector3(), boom: 0 });
    this.wants = { classic: pose(), enhanced: pose() };
    this.mixed = pose();
    this.position = new FollowVector();
    this.turn = new FollowTurn();
    this.hfov = new Follow(1);
    this.drawn = { position: new THREE.Vector3(), quaternion: new THREE.Quaternion() };
    // The view before the death, for the motion it hands over.
    this.last = { valid: false, position: new THREE.Vector3(), quaternion: new THREE.Quaternion(), velocity: new THREE.Vector3(), spin: new THREE.Vector3() };
    this.handoff = { velocity: new THREE.Vector3(), spin: new THREE.Vector3() };
    // Scratch.
    this.input = { serial: 0, hfov: 1, style: "enhanced", feet: null, chest: null, killer: null };
    this.killerOut = { point: new THREE.Vector3(), size: 1.9, living: false, friendly: false };
    this.chestAt = new THREE.Vector3();
    this.rates = { move: 0, turn: 0, fov: 0 };
    this.glide = new THREE.Vector3();
    this.posA = new THREE.Vector3();
    this.posK = new THREE.Vector3();
    this.quatA = new THREE.Quaternion();
    this.quatK = new THREE.Quaternion();
    this.point = new THREE.Vector3();
    this.pivot = new THREE.Vector3();
    this.dir = new THREE.Vector3();
    this.back = new THREE.Vector3();
    this.origin = new THREE.Vector3();
    this.toBody = new THREE.Vector3();
    this.toKiller = new THREE.Vector3();
    this.aim = new THREE.Vector3();
    this.euler = new THREE.Euler(0, 0, 0, "YXZ");
    this.basis = new THREE.Matrix4();
    this.spinTurn = new THREE.Quaternion();
    this.hit = makeHit();
  }

  // The page's frame while he is dead: the inputs from what is drawn, then
  // the step. death: the world's death camera (its count, field of view and
  // killer); feet: the body's feet as drawn; chest: its ribcage bone, or
  // null; style: the deathCamera setting.
  frame(dt, camera, death, feet, chest, style) {
    const input = this.input;
    input.serial = death.deaths;
    input.hfov = death.hfov;
    input.style = style === "classic" ? "classic" : "enhanced";
    input.feet = feet;
    input.chest = null;
    if (chest) {
      chest.updateWorldMatrix(true, false);
      input.chest = this.chestAt.setFromMatrixPosition(chest.matrixWorld);
    }
    input.killer = drawnKiller(death, this.refs, input.style === "classic", this.killerOut);
    return this.update(dt, camera, input);
  }

  // Every frame he is alive: the drawn view, for the motion it hands over.
  watch(camera, dt) {
    if (this.active) this.stop();
    const last = this.last;
    if (last.valid && dt > 0) {
      last.velocity.subVectors(camera.position, last.position).divideScalar(dt);
      logTurn(this.spinTurn.copy(last.quaternion).invert().premultiply(camera.quaternion), last.spin).divideScalar(dt);
    } else {
      last.velocity.set(0, 0, 0);
      last.spin.set(0, 0, 0);
    }
    last.position.copy(camera.position);
    last.quaternion.copy(camera.quaternion);
    last.valid = dt > 0 || last.valid;
  }

  stop() {
    this.active = false;
    this.last.valid = false;
    this.trace?.hide();
  }

  // One frame. input: { serial (the world's death count), hfov (the death
  // camera's field of view across), style, feet, chest (Vector3s, as
  // drawn; chest may be null), killer (drawnKiller's, or null) }.
  update(dt, camera, input) {
    if (!this.active || input.serial !== this.serial) this.begin(camera, input);
    if (dt > 0) {
      this.time += dt;
      if (input.style !== this.style) {
        this.switchFrom = this.style;
        this.switchAt = this.time;
        this.style = input.style;
      }
      if (this.switchFrom !== null && this.time - this.switchAt >= SWITCH) this.switchFrom = null;
      this.track(dt, input);
      let want = this.pose(this.style, dt, input, camera);
      let rates = FOLLOW[this.style];
      if (this.switchFrom !== null) {
        // A change of style: from the old one's pose to the new one's.
        const share = ease((this.time - this.switchAt) / SWITCH);
        const old = this.pose(this.switchFrom, dt, input, camera), mixed = this.mixed;
        mixed.position.lerpVectors(old.position, want.position, share);
        mixed.quaternion.slerpQuaternions(old.quaternion, want.quaternion, share);
        mixed.hfov = lerp(old.hfov, want.hfov, share);
        mixed.top.lerpVectors(old.boom ? old.top : want.top, want.boom ? want.top : old.top, share);
        mixed.boom = lerp(old.boom, want.boom, share);
        const from = FOLLOW[this.switchFrom], to = rates;
        rates = this.rates;
        rates.move = lerp(from.move, to.move, share);
        rates.turn = lerp(from.turn, to.turn, share);
        rates.fov = lerp(from.fov, to.fov, share);
        want = mixed;
      }
      this.position.step(want.position, rates.move, dt);
      this.turn.step(want.quaternion, rates.turn, dt);
      this.hfov.step(want.hfov, rates.fov, dt);
      this.resolve(dt, want);
      this.showTrace(this.style, camera);
    }
    this.apply(camera);
    return true;
  }

  pose(style, dt, input, camera) {
    return style === "classic" ? this.classic(dt, input, this.wants.classic) : this.enhanced(dt, input, camera, this.wants.enhanced);
  }

  // The drawn pose: where the springs are, clear of walls (Enhanced's boom
  // comes in after its slow follow, so it never lags into a wall, as the
  // chase camera resolves the place it draws), gliding on with the motion
  // the view it took over had.
  resolve(dt, want) {
    const drawn = this.drawn;
    drawn.position.copy(this.position.value);
    drawn.quaternion.copy(this.turn.quaternion);
    if (want.boom > 0) {
      const top = want.top;
      const offset = this.dir.subVectors(drawn.position, top);
      const length = offset.length();
      let clear = 1;
      if (length > 1e-3) {
        this.back.copy(offset).divideScalar(length);
        const hit = this.world.cast(top, this.back, length + BOOM_SKIN, this.hit, null, true);
        if (hit) clear = clamp((hit.distance - BOOM_SKIN) / length, 0.1, 1);
      }
      drawn.position.copy(top).addScaledVector(offset, lerp(1, this.reach("boom", clear, dt), want.boom));
    }
    const glide = this.time * Math.exp(-HANDOFF_RATE * this.time);
    drawn.position.addScaledVector(this.handoff.velocity, glide);
    drawn.quaternion.premultiply(expTurn(this.glide.copy(this.handoff.spin).multiplyScalar(glide), turnB)).normalize();
  }

  // From the view as it is drawn now.
  begin(camera, input) {
    this.active = true;
    this.serial = input.serial;
    this.time = 0;
    this.style = input.style;
    this.switchFrom = null;
    const from = this.from;
    from.position.copy(camera.position);
    from.quaternion.copy(camera.quaternion);
    this.euler.setFromQuaternion(camera.quaternion, "YXZ");
    from.yaw = this.euler.y;
    from.pitch = this.euler.x;
    from.roll = this.euler.z;
    from.hfov = 2 * Math.atan(Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2) * (camera.aspect || 1));
    const body = this.bodyPoint(input, this.point);
    this.startBody.copy(body);
    this.body.reset(body);
    // Where the view stood round the body: bearing (the way from the body,
    // as a heading), distance across and height. From his eyes, his heading.
    const dx = from.position.x - body.x, dz = from.position.z - body.z;
    from.radius = Math.hypot(dx, dz);
    const bearing = from.radius > 0.3 ? Math.atan2(dx, dz) : from.yaw;
    from.bearing = from.yaw + wrap(bearing - from.yaw);
    from.height = from.position.y - body.y;
    // Classic's first near point: 1 m from the body toward where the view
    // looked from, level, and 1.5 m above the feet.
    const back = this.back.set(0, 0, 1).applyQuaternion(camera.quaternion).setY(0);
    if (back.lengthSq() < 1e-8) back.set(0, 0, 1);
    back.normalize();
    const feet = input.feet ?? body;
    this.anchor.set(feet.x + back.x * DEATH_BACK, feet.y + DEATH_RISE, feet.z + back.z * DEATH_BACK);
    this.known = this.living = false;
    this.presence.reset(0);
    this.bearingR = null;
    this.fresh.boomA = this.fresh.boomK = this.fresh.boom = true;
    // The view glides on with the motion it had for a moment, as a camera on
    // a crane would; the poses start from rest where it is.
    const last = this.last;
    if (last.valid) {
      this.handoff.velocity.copy(last.velocity).clampLength(0, HANDOFF_SPEED);
      this.handoff.spin.copy(last.spin).clampLength(0, HANDOFF_TURN);
    } else this.handoff.velocity.set(0, 0, 0), this.handoff.spin.set(0, 0, 0);
    this.position.reset(from.position);
    this.turn.reset(from.quaternion);
    this.hfov.reset(from.hfov);
    this.drawn.position.copy(from.position);
    this.drawn.quaternion.copy(from.quaternion);
    this.trace?.hide();
  }

  // The body's middle: Classic, the original's 0.8 m above the feet;
  // Enhanced, the ribcage where it is drawn.
  bodyPoint(input, out) {
    if (input.style === "enhanced" && input.chest) return out.copy(input.chest);
    const feet = input.feet ?? this.startBody;
    return out.set(feet.x, feet.y + DEATH_BODY, feet.z);
  }

  track(dt, input) {
    this.body.step(this.bodyPoint(input, this.point), BODY_RATE, dt);
    const killer = input.killer;
    if (killer) {
      if (!this.known) {
        this.known = true;
        this.knownAt = this.time;
        this.killer.reset(killer.point);
        this.killerSize.reset(killer.size);
        this.shotFrom.copy(killer.point);
      } else {
        this.killer.step(killer.point, KILLER_RATE, dt);
        this.killerSize.step(killer.size, SIZE_RATE, dt);
      }
      this.living = killer.living;
      this.friendly = killer.friendly;
    } else this.living = false;
    this.presence.step(this.living ? 1 : 0, PRESENCE_RATE, dt);
    // A critically damped follower trails a steady mover by 2 / rate of his
    // speed: aiming that far ahead of him keeps a runner in the middle.
    this.killer.ahead(2 / KILLER_RATE + 2 / FOLLOW[this.style].turn, KILLER_LEAD, this.killerAim);
  }

  // Classic: the original's two poses, blended as it blended them.
  classic(dt, input, want) {
    const t = this.time, from = this.from, body = this.body.value;
    want.boom = 0;
    const rise = classicBlend(t, 1 / DEATH_SETTLE);
    this.place(this.anchor, body, "boomA", dt, this.posA, this.quatA);
    want.position.lerpVectors(from.position, this.posA, rise);
    want.quaternion.slerpQuaternions(from.quaternion, this.quatA, rise);
    want.hfov = lerp(from.hfov, input.hfov, rise);
    const presence = clamp(this.presence.x, 0, 1);
    if (this.known && presence > 0) {
      const turn = classicBlend(t - Math.max(DEATH_TURN, this.knownAt), DEATH_TURN_RATE) * presence;
      if (turn > 0) {
        this.place(body, this.killerAim, "boomK", dt, this.posK, this.quatK);
        want.position.lerp(this.posK, turn);
        want.quaternion.slerp(this.quatK, turn);
      }
    }
    return want;
  }

  // The original's pose from a near point toward a far one: 5 m back along
  // the line, or 1 m short of what the two rays meet (terrain, buildings
  // and props), looking along it with the world's up. The rays' reach is
  // followed in fast and out slowly.
  place(near, far, boom, dt, position, quaternion) {
    const dir = this.dir.subVectors(far, near);
    if (dir.lengthSq() < 1e-10) dir.set(0, 0, -1).applyQuaternion(this.turn.quaternion);
    dir.normalize();
    this.back.copy(dir).negate();
    let clear = 1;
    for (const lift of [DEATH_RAY_SPREAD, -DEATH_RAY_SPREAD]) {
      this.origin.set(near.x, near.y + lift, near.z);
      const hit = this.world.cast(this.origin, this.back, DEATH_BOOM, this.hit, null, true);
      if (hit) clear = Math.min(clear, hit.distance / DEATH_BOOM);
    }
    const reach = this.reach(boom, clear, dt);
    position.copy(near).addScaledVector(dir, DEATH_GAP - DEATH_BOOM * reach);
    this.basis.lookAt(position, this.aim.addVectors(position, dir), UP);
    quaternion.setFromRotationMatrix(this.basis);
  }

  // A boom's clear fraction: in quickly when something comes between, out
  // slowly when it clears.
  reach(name, clear, dt) {
    const boom = this[name];
    if (this.fresh[name]) {
      this.fresh[name] = false;
      return boom.reset(clear).x;
    }
    return clamp(boom.step(clear, clear < boom.x ? BOOM_IN : BOOM_OUT, dt), 0, 1);
  }

  // Enhanced: the hold, the reveal and the push in, each a place round the
  // body (bearing, distance across, height) and a look (yaw, pitch), blended
  // round the body as a crane moves.
  enhanced(dt, input, camera, want) {
    const t = this.time, from = this.from, body = this.body.value;
    const aspect = camera.aspect || 1;
    // The hold: from the view it took over to a high three-quarter look at
    // the body, on the side it looked from, drifting round it.
    const hold = ease(t / HOLD);
    const drift = DRIFT * (t - DRIFT_EASE * (1 - Math.exp(-t / DRIFT_EASE)));
    const bearingA = from.yaw + drift;
    const pitchA = -Math.atan2(HOLD_HEIGHT + LOOK_DROP, HOLD_RADIUS);
    let bearing = lerp(from.bearing, bearingA, hold);
    let radius = lerp(from.radius, HOLD_RADIUS, hold);
    let height = lerp(from.height, HOLD_HEIGHT, hold);
    let yaw = lerp(from.yaw, bearingA, hold);
    let pitch = lerp(from.pitch, pitchA, hold);
    const roll = lerp(from.roll, 0, hold);
    let hfov = lerp(from.hfov, input.hfov, hold);
    const pivot = this.pivot.lerpVectors(this.startBody, body, hold);
    if (this.known) {
      const start = Math.max(REVEAL_AT, this.knownAt + REVEAL_DELAY);
      const reveal = ease((t - start) / REVEAL);
      const settle = ease((t - start - SETTLE_AT) / SETTLE);
      if (reveal > 0) {
        const killer = this.killer.value, aimAt = this.killerAim;
        // The reverse angle: across the body from the killer, kept the same
        // way round as he moves (and turning the way it drifts if he is
        // right behind it).
        const kx = body.x - killer.x, kz = body.z - killer.z;
        const across = Math.hypot(kx, kz);
        let goal = across > 1e-3 ? Math.atan2(kx, kz) : bearingA;
        goal = lerp(bearingA, bearingA + wrap(goal - bearingA), ease((across - 0.5) / 2.5));
        if (this.bearingR === null) {
          let turn = wrap(goal - bearingA);
          if (Math.abs(turn) > Math.PI * 0.9 && turn < 0) turn += 2 * Math.PI;
          this.bearingR = bearingA + turn;
        } else this.bearingR += wrap(goal - this.bearingR);
        const bearingR = this.bearingR;
        // Looking over the body at him: him in frame, the body below him as
        // far as the frame allows.
        this.place2(pivot, bearingR, REVEAL_RADIUS, REVEAL_HEIGHT, this.posA);
        const half = Math.atan(Math.tan(input.hfov / 2) / aspect);
        const toBody = this.toBody.subVectors(body, this.posA).normalize();
        const toKiller = this.toKiller.subVectors(aimAt, this.posA).normalize();
        const apart = Math.max(Math.acos(clamp(toBody.dot(toKiller), -1, 1)), 1e-3);
        // The share of the way from the body to him to look: him within
        // KILLER_EDGE of the frame's half, the body within BODY_FRAME of
        // it, else him first.
        const most = (BODY_FRAME * half) / apart, least = 1 - (KILLER_EDGE * half) / apart;
        const share = least > most ? clamp(least, 0, 1) : clamp(0.5, least, most);
        const aim = this.aim.copy(toBody).lerp(toKiller, share).normalize();
        const yawR = this.unwrapYaw(Math.atan2(-aim.x, -aim.z), bearingR);
        const pitchR = Math.asin(clamp(aim.y, -1, 1));
        // The push in: closer, looking at him, the lens framing him.
        this.place2(pivot, bearingR, SETTLE_RADIUS, SETTLE_HEIGHT, this.posK);
        const toward = this.toKiller.subVectors(aimAt, this.posK);
        const distance = Math.max(toward.length(), 0.5);
        toward.divideScalar(distance);
        const yawK = this.unwrapYaw(Math.atan2(-toward.x, -toward.z), bearingR);
        const pitchK = Math.asin(clamp(toward.y, -1, 1));
        const base = 2 * half;
        const vfov = clamp(2 * Math.atan(this.killerSize.x / KILLER_FRAME / 2 / distance), Math.min(MIN_VFOV, base), base);
        const hfovK = 2 * Math.atan(Math.tan(vfov / 2) * aspect);
        bearing = lerp(bearing, bearingR, reveal);
        radius = lerp(radius, REVEAL_RADIUS, reveal);
        height = lerp(height, REVEAL_HEIGHT, reveal);
        yaw = lerp(yaw, yawR, reveal);
        pitch = lerp(pitch, pitchR, reveal);
        radius = lerp(radius, SETTLE_RADIUS, settle);
        height = lerp(height, SETTLE_HEIGHT, settle);
        yaw = lerp(yaw, yawK, settle);
        pitch = lerp(pitch, pitchK, settle);
        hfov = lerp(hfov, hfovK, settle);
      }
    }
    this.place2(pivot, bearing, radius, height, want.position);
    want.quaternion.setFromEuler(this.euler.set(clamp(pitch, -1.45, 1.45), yaw, roll, "YXZ"));
    want.hfov = hfov;
    // Clear of walls: a boom from just above the body (resolve), holding
    // as the view leaves the place it took over (which was where it drew).
    want.top.copy(pivot).addScaledVector(UP, BOOM_RISE);
    want.boom = hold;
    return want;
  }

  // A place round the pivot: on the bearing (a heading from it), radius
  // across, height up.
  place2(pivot, bearing, radius, height, out) {
    return out.set(pivot.x + Math.sin(bearing) * radius, pivot.y + height, pivot.z + Math.cos(bearing) * radius);
  }

  unwrapYaw(yaw, near) {
    return near + wrap(yaw - near);
  }

  // Enhanced: the killing shot's trace, from where the killer stood when he
  // was named to the body where it lies, as the reveal starts.
  showTrace(style, camera) {
    const trace = this.trace;
    if (!trace) return;
    const start = Math.max(REVEAL_AT, this.knownAt + REVEAL_DELAY);
    const since = this.time - start;
    if (style !== "enhanced" || !this.known || since < 0 || since > TRACE_FADE[1]) {
      trace.hide();
      return;
    }
    const head = ease(since / TRACE_RUN);
    const fade = 1 - ease((since - TRACE_FADE[0]) / (TRACE_FADE[1] - TRACE_FADE[0]));
    trace.show(this.shotFrom, this.body.value, this.friendly, head, fade * ease(since / 0.12), since, camera);
  }

  // Into the drawn camera, with its field of view for the camera's shape.
  apply(camera) {
    camera.position.copy(this.drawn.position);
    camera.quaternion.copy(this.drawn.quaternion);
    const fov = THREE.MathUtils.radToDeg(2 * Math.atan(Math.tan(this.hfov.x / 2) / (camera.aspect || 1)));
    if (camera.fov !== fov) {
      camera.fov = fov;
      camera.updateProjectionMatrix();
    }
  }
}
