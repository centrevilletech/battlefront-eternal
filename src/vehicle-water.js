// Water damage for walkers and the droideka, as the 2004 game deals it
// (docs/vehicles.md "Water").
//
// Walkers (in the walker update) and the droideka (in its update) take the
// same test each update: over a water cell with the surface at or above the
// origin's height plus a depth, a clock runs down from WaterDamageInterval;
// once it is below zero, every update deals WaterDamageAmount, no one
// credited (an empty damage record), until the hull leaves the water, which
// sets the clock back to the interval. Nothing resets it after a hit, so the
// amount is per update, not per interval: at the 2004 game's 60 updates a
// second a walker in deep water takes 600 a second from a second in. The
// port deals it at that rate per second of simulation, whatever its step.
//
// The depth: a walker's is the radius of its model's bounding sphere, an
// AT-ST 5.5 m, so the water must reach its cockpit. A droideka's is
// UprightWaterDamageHeight standing (1.5) and BallWaterDamageHeight rolled
// (0.5). The classes' defaults: interval 1, amount 0; a droideka's upright
// height 1.5. Hovers and flyers have no such keys and take none.
//
// Walkers wade: their legs stand on the ground under the water (the leg
// rays meet terrain and buildings only), and a foot within 0.3 m of the
// surface or under it is a foot in the water.

export const UPDATES_PER_SECOND = 60;

const num = (value, fallback) => {
  const n = Number(Array.isArray(value) ? value[0] : value);
  return Number.isFinite(n) ? n : fallback;
};

// A class's water damage keys: { interval, amount }, or null when it deals
// none (no amount, or a zero one).
export function waterDamageRule(settings = {}) {
  const amount = num(settings.WaterDamageAmount, 0);
  if (!(amount > 0)) return null;
  return { interval: num(settings.WaterDamageInterval, 1), amount };
}

// One step of the test for a body whose origin is `position`: `depth` m
// of water over it hurts. state keeps the clock (state.waterClock).
// Returns the damage to deal this step.
export function waterDamageStep(state, dt, water, rule, position, depth) {
  if (!rule || !water || !(dt > 0)) return 0;
  const under = water.over(position.x, position.z) && water.level >= position.y + depth;
  if (!under) {
    state.waterClock = rule.interval;
    return 0;
  }
  state.waterClock = (state.waterClock ?? rule.interval) - dt;
  return state.waterClock < 0 ? rule.amount * UPDATES_PER_SECOND * dt : 0;
}

// The droideka's depth for its pose (upright or rolled).
export function droidekaWaterDepth(def = {}, rolled = false) {
  return rolled ? num(def.BallWaterDamageHeight, 0) : num(def.UprightWaterDamageHeight, 1.5);
}
