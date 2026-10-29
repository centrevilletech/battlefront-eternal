import { flightSpeeds, flightHandling } from '../vehicle-motion.js';
// The port's own tuning per class of craft, shared live by every craft of
// the class: A/D roll, boost, tricks and power, the landing prompt, the
// assists, lock and missiles, collisions and boundary recovery. What the
// 2004 game reads from a craft's ODF is the craft's own (src/vehicle-
// motion.js): its speeds, its turn and pitch rates and how fast they follow
// the stick, its lean into a turn, how fast its wings level, its lift off a
// pad and its landing (src/flight/model.js). Its chase camera and fields of
// view are its pilot section's (src/flight/camera.js). Boost holds MaxSpeed.
// The landing prompt (TO LAND, and where a bot sets down) shows over ground
// within landingHeight of the hull flat enough for the craft to settle on
// (maxSlope, the 2004 game's 0.9375); the key lands the craft anywhere.
const common = {
  boostAcceleration: 2, boostTurn: .62, stickReturn: 3.5, responseCurve: 1.35,
  landingHeight: 18,
  maxSlope: .9375, powerDrain: 25, powerRecharge: 12, trickCost: 30, trickCooldown: 1,
  barrelDisplacement: 12, missileTurnRate: 1.1, missileSeeker: 15, missileSpeed: 180, missileLifetime: 8, missileNavigation: 3.5, missileRollBreakRate: 8, missileEvadeRange: 300,
  barrelTime: .6, loopTime: 1.8, immelmannTime: 1.5, trickWindow: .3,
  terrainLookahead: 1.5, recoverySharpness: 1.8, lethalImpactSpeed: 60, lethalImpactCosine: .8,
  scrapeDamageLimit: .45, impactDamageSpeed: 120,
  assistCone: 3, assistStrength: .45, stickyScale: .8,
  cameraBlendTime: .25,
  lockRange: 1600, lockCone: 15, retainCone: 25, lockDwell: 1, losGrace: .3,
};
export const FLIGHT_CLASSES = {
  fighter: { ...common, rollRate: 2.8 },
  scout: { ...common, rollRate: 3.4 },
  bomber: { ...common, rollRate: 1.8 },
  transport: { ...common, rollRate: 1.3 },
};
export function flightClass(name) {
  if (/gunship|transport|maf|laat/i.test(name)) return 'transport';
  if (/bomber|ywing|bwing/i.test(name)) return 'bomber';
  if (/awing|interceptor|geofighter|tiefighter|snowspeeder/i.test(name)) return 'scout';
  return 'fighter';
}
// A craft's configuration: its class's tuning (live, as the prototype) under
// its own speeds and handling. One per class definition.
const configs = new WeakMap();
export function flightConfig(def, kind) {
  let config = configs.get(def);
  if (!config) {
    const speeds = flightSpeeds(def);
    config = Object.assign(Object.create(FLIGHT_CLASSES[kind]), speeds, flightHandling(def), { boostSpeed: speeds.maxSpeed });
    if (def) configs.set(def, config);
  }
  return config;
}
// The stick's speed target: MidSpeed released, MaxSpeed forward, MinSpeed
// back. throttleFor is its inverse, for a pilot after a given speed.
export function throttleSpeed(c, throttle) {
  return throttle >= 0 ? c.cruiseSpeed + (c.maxSpeed - c.cruiseSpeed) * throttle : c.cruiseSpeed + (c.cruiseSpeed - c.minSpeed) * throttle;
}
export function throttleFor(c, speed) {
  const span = speed >= c.cruiseSpeed ? c.maxSpeed - c.cruiseSpeed : c.cruiseSpeed - c.minSpeed;
  return span > 0 ? Math.max(-1, Math.min(1, (speed - c.cruiseSpeed) / span)) : 0;
}
