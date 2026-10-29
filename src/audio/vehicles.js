import { hashKey } from "./hash.js";
import { classNumber, prop } from "./props.js";
import { Footfalls } from "../vehicle-feet.js";

// Every vehicle's own sounds, as the 2004 game plays them from a
// hull's sound class (docs/vehicles.md, "Effects and sounds"): the engine,
// turning on and off, taking off and landing, being hurt and destroyed,
// its feet and its turrets. Read off each hull every frame, so the hulls
// the page simulates, a worker's and a server's sound alike: all this needs
// is whether it is alive, its health, flight state, velocity, where it is
// drawn, who holds which seat and where each seat aims.
//
// - On: someone takes the pilot's seat of a hull that is off or turning
//   off. TurnOnSound plays and the engine (EngineSound, a parameterized
//   loop) starts.
// - Turning off: the pilot's seat is empty, and a flyer is landed (one
//   still in the air keeps its engine). TurningOffSound plays (R2's
//   whistle) and, TurnOffTime (3 s unless set) later, TurnOffSound, and the
//   engine stops. Meanwhile the engine's TurnOffTime input counts up, which
//   fades its layers out and drops their pitch.
// - A flyer that is on plays TakeoffSound (WindUpSound) as it starts to
//   take off and LandSound (WindDownSound) as it starts to land.
// - Each sound restarts the clock the engine's TurnOnTime, TurnOffTime,
//   WindUpTime and WindDownTime inputs read; an input the hull's state
//   doesn't drive is passed as a raw 1, which leaves its layers alone.
// - Speed is the speed over MaxSpeed, Health the share of MaxHealth left.
// - HurtSound when a hit takes more than 2% of the health left (the
//   soldier's rule, provisional for hulls); DeathSound when it is destroyed,
//   and its engine stops at once.
// - A walker's FootstepSound<i> where its i-th foot (TerrainCollision
//   primitive) comes down, and HydraulicSound as a foot lifts
//   (src/vehicle-feet.js; when the original game plays them isn't traced).
// - VehicleCollisionSound when the hull's speed drops by more than its
//   CollisionThreshold in one simulation tick (1/60 s): a crash, read off the
//   velocity every host replicates (the original game's own collision callback
//   isn't traced). A page drawing slowly sees several ticks' braking at once,
//   so the drop is measured against the time it took.
// - Turret seats (every seat after the pilot's): TurretActivateSound as
//   someone takes one and TurretDeactivateSound as the seat empties (the
//   original game compares each turret's controller with last frame's). While
//   manned, its aim's turning whirs: TurretYawSound and TurretPitchSound at
//   the sound's gain times the turn rate over the seat's top rate
//   (TurnRate, PitchRate), pitched from TurretYawSoundPitch
//   (TurretPitchSoundPitch) up to the sound's own, fading by WHIR_FADE a
//   frame once it stops.
// - Placed turrets (src/props.js, the attached game's live objects) are the
//   same turret, by their class (updateBuildings): whoever mans one, the
//   player, a bot or another player online, its activation, whirs (the
//   drum's yaw and pitch, at its MaxTurnSpeed) and deactivation. A
//   destroyed one falls silent without a sound.
//
// An engine, the whirs and the footfalls sound only within the engine
// layers' MaxDistance of the camera (AUDIBLE until one has played; a placed
// turret's whirs, AUDIBLE); out there a hull's state still runs, and its
// engine comes back without a TurnOnSound.

const TURN_OFF_TIME = 3; // s, the sound class's default TurnOffTime
const HURT_SHARE = 0.02;
const COLLISION_DROP = 5; // m/s, CollisionThreshold's default (the port's reading)
const COLLISION_TICK = 1 / 60; // s, the step a crash's drop happens in
const BUMP_INTERVAL = 0.3; // s between collision sounds of one hull
const AUDIBLE = 160; // m
const REACH_MARGIN = 10; // m past an engine's reach before it is let go
const RETRY = 1; // s between attempts at an engine that wouldn't start
const WHIR_FADE = 0.6; // per frame, a stopped turret's whir (as src/audio/scene.js)
const WHIR_RISE = 0.1; // per frame, of the whir's gain, while turning
const WHIR_STEP = 1000; // an aim turns when it moves by a step of 1/1000 rad
// s an aim may rest and still be turning: a worker's or a server's aim
// reaches the page a tick at a time, not every drawn frame.
const WHIR_HOLD = 0.1;
const SPEED = hashKey("Speed");
const ACCELERATION = hashKey("Acceleration");
const HEALTH = hashKey("Health");
const TURN_ON_TIME = hashKey("TurnOnTime");
const TURN_OFF_TIME_INPUT = hashKey("TurnOffTime");
const WIND_UP_TIME = hashKey("WindUpTime");
const WIND_DOWN_TIME = hashKey("WindDownTime");
// Names the sound export may still give by hash (before it knew them).
const ALIASES = {
  TurningOffSound: ["7d2c2fa2"],
  TakeoffSound: ["WindUpSound", "205b8d59"],
  LandSound: ["WindDownSound", "fc64a1f2"],
  HydraulicSound: ["069134e5"],
  FootstepSound0: ["97157af6"],
  FootstepSound1: ["98157c89"],
  FootstepSound2: ["951577d0"],
  FootstepSound3: ["96157963"],
  FootstepSound4: ["9b158142"],
  FootstepSound5: ["9c1582d5"],
};

// A turret's sound state: whether it was manned, where it is, each axis's
// last aim step, rest, rate, gain and voice.
const turretState = (held) => ({ held, at: { x: 0, y: 0, z: 0 }, yaw: { q: null, rest: 0, rate: 0, gain: 0, voice: null },
  pitch: { q: null, rest: 0, rate: 0, gain: 0, voice: null } });

export class VehicleSounds {
  constructor(audio) {
    this.audio = audio;
    this.states = new WeakMap();
    this.buildings = new WeakMap(); // placed turret -> its turretState
    this.reaches = new Map(); // class -> its engine's farthest layer MaxDistance
    this.inputs = {};
    for (const key of [SPEED, ACCELERATION, HEALTH, TURN_ON_TIME, TURN_OFF_TIME_INPUT, WIND_UP_TIME, WIND_DOWN_TIME]) this.inputs[key] = { value: 1, raw: true };
  }

  get game() {
    return this.audio.game;
  }

  // A class's sound for a key or one of its other names.
  ref(v, key) {
    const sound = this.audio.classSound(v.name, key);
    if (sound) return key;
    for (const other of ALIASES[key] || []) if (this.audio.classSound(v.name, other)) return other;
    return null;
  }

  play(v, key, options) {
    const found = this.ref(v, key);
    return found ? this.audio.playClass(v.name, found, { category: "foley", ...options }) : null;
  }

  number(v, key, fallback) {
    const value = Number(this.audio.data?.classSounds(v.name)?.[key]);
    return Number.isFinite(value) ? value : fallback;
  }

  // The hulls of the attached game's vehicle system (src/vehicles.js
  // Vehicles; src/game.js adds it to the game's objects with retarget).
  get hulls() {
    return this.game?.vehicles?.vehicles || [];
  }

  update(dt) {
    for (const v of this.hulls) this.updateVehicle(v, dt);
    this.updateBuildings(dt);
  }

  state(v) {
    let s = this.states.get(v);
    if (!s) this.states.set(v, (s = { life: -1 }));
    if (s.life !== v.aiTarget?.generation) {
      // A new hull (a spawn): nothing carries over from the last one.
      s.engine?.stop(0.1);
      this.stopTurrets(s);
      Object.assign(s, { life: v.aiTarget?.generation, alive: v.alive, on: false, turningOff: false, mode: null, clock: 0,
        engine: null, retry: 0, flight: v.flightState, health: v.health, speed: v.velocity?.length() ?? 0, bump: BUMP_INTERVAL, feet: new Footfalls(),
        turrets: [] });
    }
    return s;
  }

  // Whether a hull is within its engine's reach of the camera (plus a margin).
  near(v) {
    const camera = this.game.camera;
    if (!camera) return true;
    const reach = (this.reaches.get(v.name) ?? AUDIBLE) + REACH_MARGIN;
    return (v.root?.position ?? v.position).distanceToSquared(camera.position) < reach * reach;
  }

  updateVehicle(v, dt) {
    const s = this.state(v);
    if (!v.alive) {
      if (s.alive) {
        s.engine?.stop(0.1);
        s.engine = null;
        s.on = s.turningOff = false;
        this.stopTurrets(s);
        this.play(v, "DeathSound", { position: v.root?.position ?? v.position, category: "explosions" });
      }
      s.alive = false;
      return;
    }
    s.alive = true;
    if (dt <= 0) return;
    const at = { follow: () => v.root?.position ?? v.position };
    // Pilot's seat: a bot's lease, or the player in seat 0.
    const piloted = v.held ? v.held(0) : v.occupied;
    const landed = !v.flying || v.flightState === "Landed";
    if (piloted && (!s.on || s.turningOff)) {
      this.play(v, "TurnOnSound", at);
      s.on = true;
      s.turningOff = false;
      s.mode = "on";
      s.clock = 0;
    } else if (!piloted && s.on && !s.turningOff && landed) {
      this.play(v, "TurningOffSound", at);
      s.turningOff = true;
      s.mode = "off";
      s.clock = 0;
    }
    // Flight: the takeoff and the landing as they begin.
    const flight = v.flightState;
    if (v.flying && flight !== s.flight && s.on) {
      if (flight === "TakingOff" && s.flight === "Landed") {
        this.play(v, "TakeoffSound", at);
        s.mode = "up";
        s.clock = 0;
      } else if (flight === "Landing" && s.flight !== "Landing") {
        this.play(v, "LandSound", at);
        s.mode = "down";
        s.clock = 0;
      }
    }
    // A collision: its speed falls by more than its CollisionThreshold
    // (5 m/s unless set) in a tick, more than any brake or turn takes off
    // (a flyer only in level flight; its takeoff and landing steer it).
    const speed = v.velocity ? v.velocity.length() : 0;
    const threshold = Number(v.def?.settings?.CollisionThreshold) || COLLISION_DROP;
    s.bump += dt;
    const drop = ((s.speed - speed) * COLLISION_TICK) / Math.max(dt, COLLISION_TICK);
    if (drop > threshold && (!v.flying || flight === "Flying" && s.flight === "Flying") && s.bump > BUMP_INTERVAL) {
      this.play(v, "VehicleCollisionSound", at);
      s.bump = 0;
    }
    s.flight = flight;
    // Hurt by a hit worth more than 2% of what was left.
    const lost = s.health - v.health;
    if (lost > HURT_SHARE * s.health) this.play(v, "HurtSound", at);
    s.health = v.health;
    const near = this.near(v);
    // Its feet are followed out of earshot too, so none sounds coming back.
    const feet = v.def?.fx?.feet;
    if (feet) s.feet.update(v, feet, dt, (i, point, step) => near && this.play(v, step === "down" ? `FootstepSound${i}` : "HydraulicSound", { position: { x: point.x, y: point.y, z: point.z } }));
    if (s.turningOff && s.clock >= this.number(v, "TurnOffTime", TURN_OFF_TIME)) {
      this.play(v, "TurnOffSound", at);
      s.engine?.stop(0.05);
      s.engine = null;
      s.on = s.turningOff = false;
      s.mode = null;
    }
    this.updateEngine(v, s, dt, at, near);
    this.updateTurrets(v, s, dt, near);
    s.clock += dt;
  }

  updateEngine(v, s, dt, at, near) {
    const speed = v.velocity ? v.velocity.length() : 0;
    const acceleration = (speed - s.speed) / dt;
    s.speed = speed;
    if (!s.on || !near) {
      s.engine?.stop(0.15);
      s.engine = null;
      return;
    }
    s.retry -= dt;
    if ((!s.engine || s.engine.done) && s.retry <= 0) {
      const ref = this.audio.classSound(v.name, "EngineSound");
      s.engine = ref ? this.audio.engine.playParameterized(ref.name, { ...at, loop: true, category: "foley" }) : null;
      s.retry = RETRY;
      const layers = s.engine?.layers?.map((l) => l.handle?.reach || 0) ?? [s.engine?.reach || 0];
      if (Math.max(...layers) > 0) this.reaches.set(v.name, Math.max(...layers));
    }
    if (!s.engine?.apply) return;
    const settings = v.def?.settings || {};
    const top = Number(settings.MaxSpeed) || Number(settings.ForwardSpeed) || 0;
    const inputs = this.inputs;
    inputs[SPEED].value = top > 0 ? speed / top : 0;
    inputs[SPEED].raw = false;
    inputs[ACCELERATION].value = acceleration;
    inputs[ACCELERATION].raw = false;
    inputs[HEALTH].value = v.maxHealth > 0 ? v.health / v.maxHealth : 1;
    inputs[HEALTH].raw = false;
    for (const [key, mode] of [[TURN_ON_TIME, "on"], [TURN_OFF_TIME_INPUT, "off"], [WIND_UP_TIME, "up"], [WIND_DOWN_TIME, "down"]]) {
      const input = inputs[key];
      input.raw = s.mode !== mode;
      input.value = input.raw ? 1 : s.clock;
    }
    s.engine.apply(inputs);
  }

  // Each turret seat's activation and its aim's whirs.
  updateTurrets(v, s, dt, near) {
    const seats = v.seats;
    if (!seats || seats.length < 2) return;
    const sounds = this.audio.data?.classSounds(v.name);
    if (!sounds) return;
    for (let i = 1; i < seats.length; i++) {
      const seat = seats[i];
      const held = v.held ? Boolean(v.held(i)) : Boolean(seat.lease);
      let t = s.turrets[i];
      if (!t) {
        // First seen: as it is, without a sound.
        s.turrets[i] = t = turretState(held);
        continue;
      }
      this.turretPoint(v, seat, t.at);
      const settings = seat.settings || {};
      const yawTop = seat.view?.turnRate || Number(settings.TurnRate) || 2;
      const pitchTop = Number(settings.PitchRate) || Number(v.def?.settings?.PitchRate) || 1;
      this.turret(v.name, sounds, t, held, near, seat.aimYaw, seat.pitch, yawTop, pitchTop, dt);
    }
  }

  // The placed turrets: each sounds as a hull's turret seat does, from what
  // every host gives it (built or not, its lease, the drum's yaw and pitch).
  updateBuildings(dt) {
    const camera = this.game?.camera;
    for (const turret of this.game?.live?.turrets || []) {
      const cls = turret.desc?.class;
      const sounds = cls && this.audio.data?.classSounds(cls);
      if (!sounds) continue;
      const held = turret.alive && Boolean(turret.lease);
      let t = this.buildings.get(turret);
      if (!t) {
        this.buildings.set(turret, (t = turretState(held)));
        continue;
      }
      // Destroyed: its whirs stop and it is let go of without a sound.
      if (!turret.alive) t.held = false;
      this.buildingPoint(turret, t.at);
      const near = !camera || (t.at.x - camera.position.x) ** 2 + (t.at.y - camera.position.y) ** 2 + (t.at.z - camera.position.z) ** 2 < (AUDIBLE + REACH_MARGIN) ** 2;
      const top = turret.turnRate || 2;
      this.turret(cls, sounds, t, held, near, turret.yaw, turret.pitch, top, top, dt);
    }
  }

  // One turret (a seat, or a placed one; cls: whose sounds): its
  // activation as it is taken and emptied, its aim's whirs while manned.
  turret(cls, sounds, t, held, near, yaw, pitch, yawTop, pitchTop, dt) {
    if (held !== t.held) {
      t.held = held;
      this.audio.playClass(cls, held ? "TurretActivateSound" : "TurretDeactivateSound", { position: t.at, category: "foley" });
    }
    if (!held || !near) {
      this.quiet(t);
      return;
    }
    this.whir(t, t.yaw, sounds, "TurretYawSound", "TurretYawSoundPitch", yaw, yawTop, dt, cls);
    this.whir(t, t.pitch, sounds, "TurretPitchSound", "TurretPitchSoundPitch", pitch, pitchTop, dt, cls);
  }

  quiet(t) {
    for (const a of [t.yaw, t.pitch]) {
      a.voice?.stop(0.05);
      a.voice = null;
      a.q = null;
    }
  }

  // One axis of a manned turret: whirring while its aim turns.
  whir(t, a, sounds, key, lowKey, angle, top, dt, cls) {
    const q = Math.round((Number(angle) || 0) * WHIR_STEP);
    let turning = false;
    if (a.q === null) a.q = q;
    else if (q !== a.q) {
      // Radians since the last move, over the time since it.
      a.rate = Math.abs(q - a.q) / WHIR_STEP / (a.rest + dt);
      a.q = q;
      a.rest = 0;
      turning = true;
    } else {
      a.rest += dt;
      turning = a.rest <= WHIR_HOLD && a.rate > 0;
    }
    const ref = this.audio.classSound(cls, key);
    const sound = ref && this.audio.data?.sound?.(ref.name);
    if (!sound) return;
    const base = Math.max(prop(sound, "Gain"), 1e-6);
    const pitch = prop(sound, "Pitch");
    const low = classNumber(sounds[lowKey], 0.8);
    if (turning) {
      const f = Math.min(a.rate / Math.max(top, 1e-3), 1);
      if (!a.voice || a.voice.done) {
        a.voice = this.audio.engine.play(ref.name, { follow: () => t.at, loop: true, category: "foley" });
        a.gain = base;
      } else a.gain = Math.min(a.gain + WHIR_RISE * base, base);
      a.voice?.setGain(Math.min(Math.max((a.gain * f) / base, 0), 1 / base));
      a.voice?.setAbsolutePitch?.(Math.min(Math.max((low + (1 - low) * f) * pitch, low), pitch));
    } else if (a.voice) {
      a.gain *= WHIR_FADE;
      a.voice.setGain(a.gain / base);
      if (a.gain < 1e-4) {
        a.voice.stop(0.01);
        a.voice = null;
        a.rate = 0;
      }
    }
  }

  // Where a turret seat's sounds are: its turret node, else the hull.
  turretPoint(v, seat, out) {
    const node = seat.view?.turret;
    if (node?.matrixWorld) {
      node.updateWorldMatrix?.(true, false);
      const e = node.matrixWorld.elements;
      out.x = e[12];
      out.y = e[13];
      out.z = e[14];
      return out;
    }
    const p = v.root?.position ?? v.position;
    out.x = p.x;
    out.y = p.y;
    out.z = p.z;
    return out;
  }

  // Where a placed turret's sounds are: its drum, else where it stands.
  buildingPoint(turret, out) {
    const node = turret.drum;
    const p = node?.matrixWorld ? (node.updateWorldMatrix?.(true, false), node.matrixWorld.elements) : null;
    const at = turret.position;
    out.x = p ? p[12] : at.x;
    out.y = p ? p[13] : at.y;
    out.z = p ? p[14] : at.z;
    return out;
  }

  stopTurrets(s) {
    for (const t of s.turrets || []) if (t) this.quiet(t);
  }

  stop() {
    for (const v of this.hulls) {
      const s = this.states.get(v);
      s?.engine?.stop(0.05);
      if (s) {
        s.engine = null;
        this.stopTurrets(s);
      }
    }
    for (const turret of this.game?.live?.turrets || []) {
      const t = this.buildings.get(turret);
      if (t) this.quiet(t);
    }
  }
}
