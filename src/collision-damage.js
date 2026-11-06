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

// A class's two triples: what its ODF sets over the defaults, as the
// original game reads "%f %f %f" into fields that keep their defaults.
function triple(value, defaults) {
  const values = Array.isArray(value) ? value : value == null ? [] : String(value).trim().split(/\s+/).map(Number);
  return defaults.map((d, i) => (Number.isFinite(values[i]) ? values[i] : d));
}

export function collisionRule(def, defaults = SOLDIER_COLLISION) {
  return { threshold: triple(def?.CollisionThreshold, defaults.threshold), scale: triple(def?.CollisionScale, defaults.scale) };
}

