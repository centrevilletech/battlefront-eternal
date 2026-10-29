// What navigation needs to know about a unit: its classifier kind, traversal
// capability mask, clearance radius and nominal forward speed.

// The classifier IDs, as docs/ai.md's unit.kind.
export const KIND = {
  soldier: 0,
  droid: 1,
  walker: 2,
  droideka: 3,
  flyer: 5,
  hover: 6,
  turret: 7,
  armedBuilding: 8,
  building: 9,
  item: 10,
};

// AISizeType, compared case-insensitively; anything else is
// Soldier.
export const SIZE_BITS = { soldier: 1, hover: 2, small: 4, medium: 8, huge: 16, flyer: 32 };

export function traversalMask(unit) {
  if (Number.isInteger(unit.navMask)) return unit.navMask >>> 0;
  const raw = unit.classDef?.AISizeType;
  if (typeof raw === "string") return SIZE_BITS[raw.trim().toLowerCase()] ?? 1;
  if (Number.isInteger(raw)) return raw >>> 0;
  return 1;
}

// The planning clearance: a quarter of the horizontal diagonal of
// the class model's bounding box, half of it for a Hover. Integrators set
// unit.navRadius, or put the model box on the class as `bounds: [[min], [max]]`.
// The fallback is the stock Republic trooper's box (rep_inf_trooper: a
// 1.859 x 0.4 footprint, 0.476 m); Provisional until boxes are extracted.
export const DEFAULT_RADIUS = 0.476;

export function traversalRadius(unit) {
  if (typeof unit.navRadius === "number") return unit.navRadius;
  const box = unit.classDef?.bounds;
  if (Array.isArray(box) && box.length === 2) {
    const dx = box[1][0] - box[0][0];
    const dz = box[1][2] - box[0][2];
    const diagonal = Math.fround(Math.sqrt(Math.fround(Math.fround(dx * dx) + Math.fround(dz * dz))));
    return Math.fround(diagonal * (unit.kind === KIND.hover ? 0.5 : 0.25));
  }
  return DEFAULT_RADIUS;
}

// The class's nominal forward speed: Soldier
// MaxSpeed, Hover ForwardSpeed, Droideka its MaxSpeed. Not current velocity.
export function nominalSpeed(unit) {
  const def = unit.classDef ?? {};
  if (unit.kind === KIND.hover) return def.ForwardSpeed ?? 0;
  return def.MaxSpeed ?? unit.soldier?.maxSpeed ?? 9;
}

// Soldier-family bodies the movement helpers treat as infantry: the direct
// (state 3) goto and the follow state are for Soldier and Droid; jet launches
// are Soldier only.
export function isSoldier(unit) {
  return unit.kind === KIND.soldier;
}

export function isSoldierOrDroid(unit) {
  return unit.kind === KIND.soldier || unit.kind === KIND.droid;
}

// The class has a jetpack: JetFuelRechargeRate > 0.
export function hasJetpack(unit) {
  return (unit.classDef?.JetFuelRechargeRate ?? unit.soldier?.jet?.recharge ?? 0) > 0;
}
