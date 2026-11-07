import * as THREE from "three";
import { categoryOf, relationBit, weaponAI } from "./weapon-policy.js";

const EMPTY_DEFINITION = Object.freeze({});
export const unitDefinition = (unit) => {
  const def = unit?.classDef || unit?.def || EMPTY_DEFINITION;
  return def.settings || def;
};
export const unitsArray = (units) => units?.all ? units.all() : units || [];
export const defaultRelation = (a, b) => !a || !b ? 0 : a === b ? 1 : -1;

export function relationName(value) {
  return typeof value === "string" ? value : value > 0 ? "friendly" : value < 0 ? "enemy" : "neutral";
}

// Page targeting and command-post relationships remain gameplay inputs to
// the HUD; native HUD presentation does not execute these target queries.
// How a team looks to the viewer's, as the original's team colour table has
// it: a locals team (faction Locals; without the mission, any team above 2)
// in the locals' colour whether it is friend or foe, any other by relation.
// The viewer's own team is always friendly.
export const defaultLocals = (team) => team > 2;
export function radarRelation(viewerTeam, team, relation = defaultRelation, locals = defaultLocals) {
  if (!team) return "neutral";
  if (team !== viewerTeam && locals(team)) return "locals";
  return relationName(relation(viewerTeam, team));
}

// Exact parse-order range can be supplied by extraction as ReticleRange.
// Older extracts omit order; projectile lifetime is their fallback.
export function reticleRange(def) {
  if (def.ReticleRange !== undefined && Number.isFinite(Number(def.ReticleRange))) return Math.max(0, Number(def.ReticleRange));
  return Number(def.ordnance?.Velocity) * Number(def.ordnance?.LifeSpan) || Number(def.LockOnRange) || 0;
}

const spanLocal = new THREE.Vector3(), spanForward = new THREE.Vector3(), spanNear = new THREE.Vector3();
const spanA = new THREE.Vector3(), spanB = new THREE.Vector3(), spanEdge = new THREE.Vector3(), spanCentre = new THREE.Vector3();
const spanInverse = new THREE.Matrix4();
const AXES = ["x", "y", "z"];

// How far off a view (origin, unit dir) a unit lies: the smallest angle
// (radians) between the view and any point of it, and the distance to its
// nearest point. A vehicle is its collision box in its own frame, anything
// else its bounding sphere. 0 when the view runs into it.
export function lockSpan(origin, dir, unit, out = { angle: Math.PI, distance: Infinity }) {
  const vehicle = unit.vehicle;
  if (vehicle?.bounds && vehicle.matrix) {
    spanInverse.copy(vehicle.matrix).invert();
    const o = spanLocal.copy(origin).applyMatrix4(spanInverse), d = spanForward.copy(dir).transformDirection(spanInverse);
    const { min, max } = vehicle.bounds;
    out.distance = spanNear.copy(o).clamp(min, max).distanceTo(o);
    // The slabs: does the view itself pass through the box?
    let enter = 0, leave = Infinity;
    for (const k of AXES) {
      if (Math.abs(d[k]) < 1e-9) { if (o[k] < min[k] || o[k] > max[k]) { leave = -1; break; } continue; }
      const t0 = (min[k] - o[k]) / d[k], t1 = (max[k] - o[k]) / d[k];
      enter = Math.max(enter, Math.min(t0, t1));
      leave = Math.min(leave, Math.max(t0, t1));
      if (enter > leave) break;
    }
    if (enter <= leave) { out.angle = 0; return out; }
    // Else the nearest direction lies on one of the twelve edges (a face's
    // interior has no nearer point unless the view meets that face).
    let best = -1;
    for (let i = 0; i < 8; i++) for (const bit of [1, 2, 4]) {
      if (i & bit) continue;
      boxCorner(spanA, i, min, max).sub(o);
      spanEdge.subVectors(boxCorner(spanB, i | bit, min, max).sub(o), spanA);
      best = Math.max(best, nearestCosine(spanA, spanEdge, d));
    }
    out.angle = Math.acos(Math.min(1, best));
    return out;
  }
  const sphere = unit.sphere;
  const radius = Math.max(0, sphere?.radius ?? 0);
  if (sphere?.center) spanCentre.copy(sphere.center);
  else if (unit.aimPoint) unit.aimPoint(spanCentre);
  else spanCentre.copy(unit.position);
  const span = spanCentre.sub(origin).length();
  out.distance = Math.max(0, span - radius);
  if (span <= radius) { out.angle = 0; return out; }
  out.angle = Math.max(0, Math.acos(THREE.MathUtils.clamp(spanCentre.dot(dir) / span, -1, 1)) - Math.asin(radius / span));
  return out;
}

function boxCorner(out, i, min, max) {
  return out.set(i & 1 ? max.x : min.x, i & 2 ? max.y : min.y, i & 4 ? max.z : min.z);
}

// The largest cosine between d and a point of the segment a..a+e (relative
// to the view's origin): at an end, or where its derivative vanishes.
function nearestCosine(a, e, d) {
  const a0 = a.dot(d), a1 = e.dot(d), qa = e.lengthSq(), qb = 2 * a.dot(e), qc = a.lengthSq();
  const at = (s) => (a0 + a1 * s) / Math.sqrt(Math.max(qa * s * s + qb * s + qc, 1e-12));
  let best = Math.max(at(0), at(1));
  const denominator = a1 * qb / 2 - a0 * qa;
  if (Math.abs(denominator) > 1e-12) {
    const s = (a0 * qb / 2 - a1 * qc) / denominator;
    if (s > 0 && s < 1) best = Math.max(best, at(s));
  }
  return best;
}

const reticleSpan = { angle: Math.PI, distance: Infinity };

// The reticle's target (its colour and the enemy health bar), in the
// original's cone (widened by the target's bounding radius,
// scored 16 d^2 + t). A hull counts only once part of it is inside the
// cone (lockSpan), as for a launcher's lock (src/weapon-targeting.js):
// a vehicle's bounding radius took it well off the reticle up close.
export function findReticleTarget({ origin, dir, units, weapon, viewer = null, relation = defaultRelation, los = () => true, range, angle: coneAngle }) {
  const def = weapon?.def || weapon;
  if (!def || !origin || !dir) return null;
  const far = range ?? reticleRange(def);
  const angle = (Number(coneAngle ?? def.LockOnAngle) || 0) * Math.PI / 180;
  const point = new THREE.Vector3(), delta = new THREE.Vector3();
  const direction = new THREE.Vector3().copy(dir).normalize();
  if (!direction.lengthSq()) return null;
  const masks = weaponAI(def);
  let best = null, count = 0;
  for (const unit of unitsArray(units)) {
    if (unit === viewer || !unit.alive || unit.seat || (unit.health ?? unit.combat?.health ?? 1) <= 0 || unit.kind === 10) continue;
    if (unit.aimPoint) unit.aimPoint(point);
    else point.copy(unit.sphere?.center || unit.position);
    delta.subVectors(point, origin);
    const t = delta.dot(direction);
    const radius = unit.box?.half?.length?.() ?? unit.sphere?.radius ?? 0;
    if (t < 0 || t > far) continue;
    if (++count > 300) break;
    const d2 = Math.max(0, delta.lengthSq() - t * t);
    if (unit.vehicle?.bounds ? lockSpan(origin, direction, unit, reticleSpan).angle > angle : d2 > (radius + t * angle) ** 2) continue;
    const rel = relation(viewer?.team, unit.team);
    const lockRange = Number(def.LockOnRange) || 0;
    const lockable = t <= lockRange && Boolean(masks.relationMask & relationBit(rel)) && Boolean(masks.categoryMask & (1 << categoryOf(unit)));
    const score = 16 * d2 + t;
    if (best && (best.lockable && !lockable || best.lockable === lockable && best.score <= score)) continue;
    if (!los(origin, point, viewer, unit)) continue;
    best = { unit, point: point.clone(), lockable, score, distance: t };
  }
  return best;
}
