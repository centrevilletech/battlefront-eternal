import * as THREE from "three";
import { categoryOf, relationBit, weaponAI } from "./ai/classic/weapons-ai.js";
import { humanName } from "./bot-names.js";

// Unit-facing HUD rules. Colours are normalized texture multipliers.
export const UNIT_COLORS = {
  friendly: [32 / 255, 223 / 255, 32 / 255, 1],
  enemy: [223 / 255, 32 / 255, 32 / 255, 1],
  neutral: [192 / 255, 192 / 255, 192 / 255, 1],
  locals: [223 / 255, 223 / 255, 32 / 255, 1],
};
const clamp = (n) => Math.max(0, Math.min(1, n));
export const unitDefinition = (unit) => {
  const def = unit?.classDef || unit?.def || {};
  return def.settings || def;
};
export const unitsArray = (units) => units?.all ? units.all() : units || [];
export const defaultRelation = (a, b) => !a || !b ? 0 : a === b ? 1 : -1;

const mapForward = new THREE.Vector3();

// Drawing only. Radar discovery and sensor ranges keep using unit.position.
export const mapPosition = unit => unit.vehicle?.root?.position ?? unit.vehicle?.visual ?? unit.presentation?.position ?? unit.position ?? unit.vehicle?.position;

// World yaw is measured from +Z toward +X. A vehicle marker follows its
// hull, independent of the pilot's weapon or freelook direction.
export function mapHeading(unit, previous) {
  const vehicle = unit.vehicle;
  const yaw = unit.presentation?.state?.aimYaw ?? unit.soldier?.aimYaw ?? vehicle?.yaw ?? unit.yaw ?? unit.turret?.yaw;
  const fallback = Number.isFinite(previous) ? previous : Number.isFinite(yaw) ? yaw : 0;
  if (vehicle?.root?.quaternion || vehicle?.flying && vehicle.quaternion) {
    mapForward.set(0, 0, 1).applyQuaternion(vehicle.root?.quaternion ?? vehicle.quaternion);
  } else if (vehicle || unit.soldier || typeof unit.forward !== "function") {
    return Number.isFinite(yaw) ? yaw : fallback;
  } else {
    unit.forward(mapForward.set(0, 0, 0));
  }
  // A vertical nose has no map heading. Keep the last horizontal heading
  // instead of letting floating-point noise spin the icon at the pole.
  return Number.isFinite(mapForward.x) && Number.isFinite(mapForward.z) &&
    mapForward.x ** 2 + mapForward.z ** 2 > 1e-8
    ? Math.atan2(mapForward.x, mapForward.z) : fallback;
}

// Imported directional icons point up (-Y); battlefield +Z points down.
export const mapIconRotation = (yaw, turn = 0) => Math.PI - yaw + turn;

export function relationName(value) {
  return typeof value === "string" ? value : value > 0 ? "friendly" : value < 0 ? "enemy" : "neutral";
}

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

export function unitColor(relation, dead = false, flash = 0, opacity = 1) {
  const base = UNIT_COLORS[relationName(relation)] || UNIT_COLORS.neutral;
  const color = base.map((v, i) => i === 3 ? clamp(opacity) : dead ? (v + 64 / 255) / 2 : v);
  for (let i = 0; i < 3; i++) color[i] += (1 - color[i]) * clamp(flash);
  return color;
}

const distanceSquared = (a, b) => (a.x - b.x) ** 2 + (a.y - b.y) ** 2 + (a.z - b.z) ** 2;

// Sensors are strictly inside their circles; a transmitter must also reach
// the viewer. Positions may differ when the player controls a vehicle.
export function radarSensors(objects, viewer, controllable = viewer) {
  if (!viewer || !controllable?.position) return [];
  const sensors = [];
  const own = Number(unitDefinition(controllable).ScanningRange) || 0;
  if (own > 0) sensors.push({ position: controllable.position, radius: own });
  for (const object of objects) {
    if (sensors.length >= 64) break;
    if (object === controllable || object.team !== viewer.team || !object.alive) continue;
    const def = unitDefinition(object);
    const scan = Number(def.ScanningRange) || 0, transmit = Number(def.TransmitRange) || 0;
    if (scan > 0 && transmit > 0 && distanceSquared(object.position, controllable.position) <= transmit ** 2) {
      sensors.push({ position: object.position, radius: scan });
    }
  }
  return sensors;
}

export function radarAlpha(unit, { friendly = false, easy = false, tickets = Infinity, living = 0, sensors = [], flash = 0 } = {}, previous = 0, dt = 0) {
  if (friendly || tickets <= living || easy) return 1;
  const revealed = sensors.some(({ position, radius }) => {
    const dx = position.x - unit.position.x, dz = position.z - unit.position.z;
    return dx * dx + dz * dz < radius * radius;
  });
  return Math.max(revealed ? 1 : clamp(flash), clamp(previous - Math.max(dt, 0) * 32 / 255));
}

export class RadarUnits {
  constructor() {
    this.states = new WeakMap();
    this.icons = [];
  }

  state(unit) {
    let state = this.states.get(unit);
    if (!state || state.generation !== unit.generation) {
      state = { generation: unit.generation, alpha: 0, shots: [] };
      this.states.set(unit, state);
    }
    return state;
  }

  shot(unit, def = {}, time = 0) {
    if (!unit) return;
    const state = this.state(unit);
    state.alpha = 1;
    // Shot-state duration is approximated by the fired weapon's ShotDelay.
    const duration = Math.max(Number(def.ShotDelay) || 0, 1 / 60);
    state.shots = state.shots.filter((shot) => shot.until > time);
    state.shots.push({ until: time + duration, duration });
  }

  update(dt, time, units, context = {}) {
    const { viewer, controllable = viewer, viewerTeam = viewer?.team, relation = defaultRelation, locals = defaultLocals } = context;
    const objects = [...new Set([...unitsArray(units), ...unitsArray(context.objects)])];
    const living = new Map();
    for (const unit of objects) if (unit.alive && !unit.hero && (unit.kind === undefined || [0, 1, 3].includes(unit.kind))) {
      living.set(unit.team, (living.get(unit.team) || 0) + 1);
    }
    const sensors = context.sensors?.slice(0, 64) || radarSensors(objects, viewer, controllable);
    this.icons.length = 0;
    let count = 0;
    for (const unit of objects) {
      if (unit === viewer || unit.removed || unit.seat || !(unit.alive || unit.dead) || !unit.position) continue;
      const def = unitDefinition(unit);
      const soldier = [0, 1, 3].includes(unit.kind) || Boolean(unit.soldier);
      const texture = def.MapTexture ?? (soldier ? "troop_icon" : null);
      if (!texture) continue;
      if (++count > 450) break;
      const state = this.state(unit);
      state.shots = state.shots.filter((shot) => shot.until > time);
      const flash = state.shots.reduce((f, shot) => Math.max(f, clamp((shot.until - time) / shot.duration)), 0);
      const rel = radarRelation(viewerTeam, unit.team, relation, locals);
      state.alpha = radarAlpha(unit, { friendly: relation(viewerTeam, unit.team) > 0,
        easy: context.easy, tickets: context.tickets?.(unit.team) ?? Infinity,
        living: context.living?.(unit.team) ?? living.get(unit.team) ?? 0, sensors, flash }, state.alpha, dt);
      if (state.alpha <= 0) continue;
      const yaw = state.yaw = mapHeading(unit, state.yaw);
      const defaultScale = soldier ? unit.kind === 3 ? 1.6 : 1.4 : 1;
      const scale = Number(def.MapScale ?? defaultScale);
      this.icons.push({ unit, texture, size: 6 * Math.max(0, Number.isFinite(scale) ? scale : defaultScale),
        yaw,
        color: unitColor(rel, !unit.alive, flash, state.alpha) });
    }
    return this.icons;
  }
}

export class HitRing {
  constructor() { this.time = { enemy: 0, friendly: 0, neutral: 0 }; }
  hit(relation) {
    const name = relationName(relation);
    this.time[name === "locals" ? "enemy" : name] += 0.5;
  }
  update(dt) { for (const key of Object.keys(this.time)) this.time[key] = Math.max(0, this.time[key] - dt); }
  color() {
    const e = clamp(10 * this.time.enemy), f = clamp(10 * this.time.friendly), n = clamp(10 * this.time.neutral);
    const m = Math.max(e, f, n);
    return m ? [(32 + 192 * e / m) / 255, (32 + 192 * f / m) / 255, (32 + 192 * n / m) / 255, 128 * m / 255] : [0, 0, 0, 0];
  }
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

export function killMessage({ killer = null, victim, relationToViewer = "neutral" }, strings = {}) {
  if (!victim || !(victim.human || victim.character?.human || killer?.human || killer?.character?.human || killer?.hero)) return null;
  // A Character's name: the player's is "Player 1" offline (src/bot-names.js),
  // as the end-of-match table lists him.
  const name = (unit) => unit?.character?.name || (unit?.human ? humanName() : null) || unit?.name || unit?.classDef?.Label || "Soldier";
  const type = !killer ? "died" : killer === victim ? "suicide" : killer.team === victim.team ? "teamkill" : "killed";
  const fallback = { died: "%s DIED", suicide: "%s KILLED HIMSELF", teamkill: "%s KILLED TEAMMATE %s", killed: "%s KILLED %s" };
  const args = type === "died" || type === "suicide" ? [name(victim)] : [name(killer), name(victim)];
  let text = strings[`game.infomsg.${type}`] || fallback[type];
  for (const arg of args) text = text.replace("%s", () => arg);
  return { text, relation: relationName(relationToViewer) };
}
