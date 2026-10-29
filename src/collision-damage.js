// Fall damage, as the 2004 game deals it: not a fall rule of its own but
// collision damage, from the collision callbacks of the soldier and the
// droideka, the same for the player, the bots and network humans
// (docs/formats.md, "Collision damage").
//
// For each contact the body's velocity, less what it touches moves at, is
// taken along the contact normal n (pointing at the body). Moving into the
// surface faster than a threshold hurts:
//   threshold = T0 + T1 (n . up) + T2 (n . forward)
//   damage    = (S0 + S1 (n . up) + S2 (n . forward)) x (speed - threshold)^2
// T and S are the class's CollisionThreshold and CollisionScale ("%f %f %f";
// a single value sets the first only). A soldier's default is 10 3 0 and
// 1 0 0: landing on level ground hurts past 13 m/s, a fall of about 4.7 m at
// 18 m/s^2, and (v - 13)^2 kills a 300-health trooper from about 30.3 m/s,
// some 25.5 m. The jet trooper, the dark trooper and the Geonosian set the
// scale to 0 0 0 and take none. A droideka's default is 7.5 2.5 0 and 1 0 0.
//
// The soldier's speed into a building or prop is first scaled by n.y, so a
// wall hardly hurts; terrain counts in full. The damage is ordinary damage
// with every health-type scale 1 and nobody credited ("%s DIED"). The
// 2004 game also hurts a soldier that a moving vehicle meets (its own
// horizontal speed dropped, the hull's subtracted, the hull's driver
// credited); the port leaves moving bodies out, as it has no velocity
// exchange with a hull and would repeat the hit every tick.
//
// Besides, each soldier update kills a soldier falling faster than 60 m/s
// (some 100 m of free fall) or further than 1000 m from the origin across,
// or 500 m up or down.

export const SOLDIER_COLLISION = Object.freeze({ threshold: [10, 3, 0], scale: [1, 0, 0] });
export const DROIDEKA_COLLISION = Object.freeze({ threshold: [7.5, 2.5, 0], scale: [1, 0, 0] });
export const FALL_KILL_SPEED = 60; // m/s down
const WORLD_HALF = 1000; // m, x and z
const WORLD_HEIGHT = 500; // m, up and down

// A class's two triples: what its ODF sets over the defaults, as the
// original game reads "%f %f %f" into fields that keep their defaults.
function triple(value, defaults) {
  const values = Array.isArray(value) ? value : value == null ? [] : String(value).trim().split(/\s+/).map(Number);
  return defaults.map((d, i) => (Number.isFinite(values[i]) ? values[i] : d));
}

export function collisionRule(def, defaults = SOLDIER_COLLISION) {
  return { threshold: triple(def?.CollisionThreshold, defaults.threshold), scale: triple(def?.CollisionScale, defaults.scale) };
}

// The damage of one contact. into: the velocity along n (negative moving
// into it); up and forward: n's components along the body's up and facing.
export function contactDamage(rule, into, up, forward) {
  if (!(into < 0)) return 0;
  const [t0, t1, t2] = rule.threshold;
  const threshold = t0 + t1 * up + t2 * forward;
  if (!(-threshold > into)) return 0;
  const [s0, s1, s2] = rule.scale;
  const excess = into + threshold;
  const damage = (s0 + s1 * up + s2 * forward) * excess * excess;
  return damage > 0 ? damage : 0;
}

// A soldier past the world's bounds, or falling too fast to live.
export function outOfWorld(position, velocity) {
  return velocity.y < -FALL_KILL_SPEED || position.y < -WORLD_HEIGHT || position.y > WORLD_HEIGHT
    || Math.abs(position.x) > WORLD_HALF || Math.abs(position.z) > WORLD_HALF;
}

// After a body's physics step (the player's, a bot's, a network human's):
// what its contacts did to it, and the world bounds for a soldier. On a
// seat the body doesn't move by itself.
export function takeImpacts(unit) {
  const body = unit?.soldier;
  const combat = unit?.combat;
  if (!body || !combat || combat.dead || unit.removed || unit.seat) return;
  const damage = body.collisionDamage;
  if (damage > 0) combat.damage(damage);
  if (!combat.dead && !unit.walker && body.position && body.velocity && outOfWorld(body.position, body.velocity)) combat.damage(Infinity);
}
