import { setting } from "./seats.js";

// How fast each 2004 vehicle class moves, read from its ODF the way the
// original game reads it (docs/vehicles.md "Movement"). Every path that builds
// a Vehicle takes its numbers from here: the page, the world worker, the
// dedicated server and desktop host, and a client's prediction copies.

// A class key. The original game files every key into the class whatever
// section it follows, so one authored after a later seat's section still
// counts (the AT-ST's AnimationSpeed); the body's own comes first.
export function classSetting(def, name) {
  const value = setting(def?.settings, name);
  if (value !== undefined) return value;
  for (const seat of def?.seats?.slice(1) || []) {
    const later = setting(seat.settings, name);
    if (later !== undefined) return later;
  }
  return undefined;
}
const number = (def, name, fallback, read = classSetting) => {
  const n = parseFloat(read(def, name));
  return Number.isFinite(n) ? n : fallback;
};
// A turn rate is the body section's own: each seat's section has its own.
const body = (def, name) => setting(def?.settings, name);

// Flyers: the throttle's target is MidSpeed with the stick released,
// MaxSpeed held forward and MinSpeed held back (in between in proportion),
// and the speed moves to it at Acceleraton m/s^2. A MinSpeed below zero
// flies backward (the LAAT/i and MAF hover at a MidSpeed of 0). Keys a
// class leaves unset take the original game's defaults.
export function flightSpeeds(def) {
  return {
    minSpeed: number(def, "MinSpeed", 10), cruiseSpeed: number(def, "MidSpeed", 20),
    maxSpeed: number(def, "MaxSpeed", 30), acceleration: number(def, "Acceleraton", 5),
  };
}

// How a flyer handles (src/flight/model.js): the turn and pitch rates its
// stick asks for (rad/s) and how fast its rates follow the stick
// (TurnFilter, PitchFilter, 1/s); how far its drawn hull leans into a turn
// (BankAngle, rad, at TurnRate and MaxSpeed) and how fast (BankFilter); how
// fast its wings level (LevelFilter); its lift off a pad: TakeoffSpeed up
// (m/s) until it is TakeoffHeight above where it rests (LandedHeight, m),
// then over to flight within TakeoffTime (s); and its landing, the lift run
// back: out of flight within LandingTime (s), sinking at LandingSpeed (m/s).
// The turn and pitch keys are the body section's own (a gunner's section
// has its own); the original game raises TurnRate to at least 1.5. Keys a
// class leaves unset take the original game's defaults.
export function flightHandling(def) {
  return {
    turnRate: Math.max(number(def, "TurnRate", 2, body), 1.5), turnFilter: number(def, "TurnFilter", 10, body),
    pitchRate: number(def, "PitchRate", 1, body), pitchFilter: number(def, "PitchFilter", 10, body),
    bankAngle: number(def, "BankAngle", .5), bankFilter: number(def, "BankFilter", 2), levelFilter: number(def, "LevelFilter", 3),
    takeoffHeight: number(def, "TakeoffHeight", 5), takeoffTime: number(def, "TakeoffTime", 5),
    takeoffSpeed: number(def, "TakeoffSpeed", 3), landedHeight: number(def, "LandedHeight", 0),
    landingTime: number(def, "LandingTime", 5), landingSpeed: number(def, "LandingSpeed", 3),
  };
}

// Walkers and the riding animals have no speed key that moves them: they
// walk by their walk clips, the hull following DummyRoot's travel, each clip
// lasting its frames at 30 fps over AnimationSpeed, and backward plays the
// same steps in reverse. (MaxSpeed only tells the AI what to expect.)
export function gaitSpeed(clips, animationSpeed = 1) {
  const left = clips?.get("walk_leftfoot_rightfoot"), right = clips?.get("walk_rightfoot_leftfoot");
  const frames = (left?.frames || 0) + (right?.frames || 0);
  return frames > 0 ? (left.stride + right.stride) * 30 / frames * animationSpeed : 0;
}

// Ground movement (Vehicle.tick): top speed forward and back, sideways,
// acceleration with the stick held and deceleration released (m/s, m/s^2),
// and the turn rate (rad/s). Hovers take ForwardSpeed, ReverseSpeed,
// StrafeSpeed, Acceleration (spelled right) and Deceleration. A walker
// can't strafe: its A/D turn it.
export function groundMotion(def, clips) {
  const base = def?.base || "hover";
  if (base === "flyer") return { speed: 0, reverseSpeed: 0, strafeSpeed: 0, acceleration: 0, deceleration: 0, turnRate: 0 };
  if (base === "walker" || base === "commandwalker") {
    const turnRate = number(def, "MaxTurnSpeed", 1.7, body);
    const gait = gaitSpeed(clips, number(def, "AnimationSpeed", 1));
    const animal = String(classSetting(def, "HealthType") || "").toLowerCase() === "animal";
    // A walker without walk clips falls back on what its class tells the AI.
    const speed = gait || number(def, "MaxSpeed", 12);
    // The port's ramps: a machine starts and stops at its Acceleraton; an
    // animal reaches its gallop in 1/Acceleraton s and stops as quickly.
    const acceleration = number(def, "Acceleraton", 8) * (animal && gait ? speed : 1);
    return { speed, reverseSpeed: speed, strafeSpeed: 0, acceleration,
      deceleration: animal && gait ? acceleration : number(def, "Deceleration", 10), turnRate };
  }
  return {
    speed: number(def, "ForwardSpeed", 20), reverseSpeed: number(def, "ReverseSpeed", 10), strafeSpeed: number(def, "StrafeSpeed", 15),
    acceleration: number(def, "Acceleration", 10), deceleration: number(def, "Deceleration", 5), turnRate: number(def, "TurnRate", number(def, "MaxTurnSpeed", 1.7, body), body),
  };
}
