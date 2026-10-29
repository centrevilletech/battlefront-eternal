import * as THREE from "three";

// Every combatant in the battle: the player, the bots, and (later) anything
// else that can be shot, targeted, or counted. Units are pooled and keep
// their id for the match; each spawn bumps the unit's generation, so a
// reference held across a death ({ unit, generation }) can tell it has gone
// stale. The registry answers the questions ordnance, capture, and the AI
// ask about units: who is near a point, and what a segment passes through.
//
// Teams: the mission's armies are 1 and 2; 0 is neutral; locals (3 and up)
// are hostile to both armies unless the mission says otherwise. The
// mission's table (setTeams: its SetTeamAs* relations, each one team's view
// of another, the factions and the capture teams) decides; without one every
// other team is an enemy.

const LOCALS = 5; // the faction SetTeamName gives a name it doesn't know

const scratch = new THREE.Vector3();
const toCenter = new THREE.Vector3();
const local = new THREE.Vector3();
const localDir = new THREE.Vector3();
const losHit = { unit: null, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), part: null };

export class Units {
  constructor() {
    this.list = [];
    this.byIdMap = new Map();
    this.nextId = 1;
    this.alliances = new Map(); // "a:b" -> 1 / 0 / -1 overrides
    this.teams = null; // { relations, factions, capture } by team (src/ai/classic/spawning.js teamTable)
  }

  setTeams(table) {
    this.teams = table || null;
  }

  add(unit) {
    unit.id = this.nextId++;
    unit.generation = unit.generation || 0;
    this.list.push(unit);
    this.byIdMap.set(unit.id, unit);
    return unit;
  }

  remove(unit) {
    const i = this.list.indexOf(unit);
    if (i >= 0) this.list.splice(i, 1);
    this.byIdMap.delete(unit.id);
  }

  all() {
    return this.list;
  }

  *living() {
    for (const unit of this.list) if (unit.alive) yield unit;
  }

  byId(id) {
    return this.byIdMap.get(id) || null;
  }

  ref(unit) {
    return unit ? { unit, generation: unit.generation } : null;
  }

  // The unit a reference names, while it is the same life of that unit.
  resolve(ref) {
    if (!ref?.unit) return null;
    return ref.unit.generation === ref.generation && ref.unit.alive ? ref.unit : null;
  }

  // Team a's relation to team b: 1 friend, 0 neutral, -1 enemy.
  relation(a, b) {
    if (a === b) return a ? 1 : 0;
    if (!a || !b) return 0;
    const set = this.alliances.get(a < b ? `${a}:${b}` : `${b}:${a}`);
    return set ?? this.teams?.relations[a]?.[b] ?? -1;
  }

  setRelation(a, b, value) {
    this.alliances.set(a < b ? `${a}:${b}` : `${b}:${a}`, value);
  }

  // A locals team (faction Locals): the HUD draws it in the locals' colour.
  // Without the mission's table, any team above 2.
  locals(team) {
    return this.teams ? this.teams.factions[team] === LOCALS : team > 2;
  }

  // Whom a soldier of `team` counts for at a command post (0 nobody).
  captureTeam(team) {
    return this.teams?.capture[team] ?? team;
  }

  // Living units whose broadphase sphere meets the ball.
  query(center, radius, out = [], filter = null) {
    out.length = 0;
    for (const unit of this.list) {
      if (!unit.alive) continue;
      const s = unit.sphere;
      const reach = radius + s.radius;
      if (s.center.distanceToSquared(center) > reach * reach) continue;
      if (filter && !filter(unit)) continue;
      out.push(unit);
    }
    return out;
  }

  // The nearest soldier on foot a segment from `origin` along unit
  // `direction` for `far` meets, by its hit shapes (the body, and the head,
  // whose `part` is "head"). A body still lying where it fell stops rounds
  // too. `exclude` (a unit or array of units) is skipped. riders: rounds
  // also meet a soldier shown on an exposed seat (strikeable); sight tests
  // leave him out, as they leave out everyone seated. Returns `out` filled
  // { unit, distance, point, normal, part }, or null.
  raycast(origin, direction, far, exclude = null, out = makeUnitHit(), filter = null, riders = false) {
    let best = far;
    let found = null;
    let part = null;
    let shape = null;
    for (const unit of this.list) {
      // Soldiers on foot, and for rounds the riders they can see: anyone
      // else seated is inside his vehicle or turret, and vehicles and
      // turrets are struck through their collision bodies.
      if (!(unit.alive || (unit.dead && !unit.removed)) || !unit.hitShapes || unit.seat && !(riders && unit.seat.exposed)) continue;
      if (unit === exclude || (Array.isArray(exclude) && exclude.includes(unit))) continue;
      if (filter && !filter(unit)) continue;
      // Broadphase first: the unit's whole sphere.
      const s = unit.sphere;
      if (segmentSphere(origin, direction, best, s.center, s.radius) === null) continue;
      for (const candidate of unit.hitShapes()) {
        const t = segmentShape(origin, direction, best, candidate);
        if (t !== null && t < best) {
          best = t;
          found = unit;
          part = candidate.part;
          shape = candidate;
        }
      }
    }
    if (!found) return null;
    out.unit = found;
    out.distance = best;
    out.part = part;
    out.point.copy(origin).addScaledVector(direction, best);
    shapeNormal(out.point, shape, direction, out.normal);
    return out;
  }
}

export function makeUnitHit() {
  return { unit: null, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), part: null };
}

// Whether a round can meet this unit's body: a soldier on foot or a body
// lying where it fell, or a rider shown on an exposed seat (SeatLease
// exposed); not one inside a vehicle or turret.
export function strikeable(unit) {
  return Boolean(unit?.hitShapes && (unit.alive || (unit.dead && !unit.removed)) && (!unit.seat || unit.seat.exposed));
}

// The outward normal of a shape at a point on it (the way the round came
// back, for a point at its middle).
export function shapeNormal(point, shape, direction, out) {
  if (shape.type === "capsule") {
    // From the nearest point of its axis.
    axisPoint.subVectors(point, shape.center);
    const length = shape.half.lengthSq();
    const along = length > 1e-12 ? THREE.MathUtils.clamp(axisPoint.dot(shape.half) / length, -1, 1) : 0;
    out.subVectors(point, axisPoint.copy(shape.center).addScaledVector(shape.half, along));
  } else out.subVectors(point, shape.center);
  if (out.lengthSq() < 1e-12) out.copy(direction).negate();
  return out.normalize();
}
const axisPoint = new THREE.Vector3();

// A hit shape: { type: "sphere", center, radius } | { type: "cylinder",
// center, radius, half } (upright, half its height either way of the centre)
// | { type: "box", center, half: Vector3, yaw } (turned about the vertical)
// | { type: "capsule", center, half: Vector3, radius } (the segment from
// center - half to center + half, swept by the radius: a rider's limbs).
export function segmentShape(origin, dir, far, shape) {
  if (shape.type === "sphere") return segmentSphere(origin, dir, far, shape.center, shape.radius);
  if (shape.type === "cylinder") return segmentCylinder(origin, dir, far, shape.center, shape.radius, shape.half);
  if (shape.type === "box") return segmentBox(origin, dir, far, shape.center, shape.half, shape.yaw);
  if (shape.type === "capsule") return segmentCapsule(origin, dir, far, shape.center, shape.half, shape.radius);
  return null;
}

// The entry distance into a capsule (center, half: its axis's half, radius),
// or null; 0 from inside.
const capEnd = new THREE.Vector3();
const capAxis = new THREE.Vector3();
const capFrom = new THREE.Vector3();
export function segmentCapsule(origin, dir, far, center, half, radius) {
  capEnd.subVectors(center, half);
  capAxis.copy(half).multiplyScalar(2);
  capFrom.subVectors(origin, capEnd);
  const axis2 = capAxis.lengthSq(), axisDir = capAxis.dot(dir), axisFrom = capAxis.dot(capFrom), from2 = capFrom.lengthSq();
  // Inside: the origin within the radius of the nearest point of the axis.
  const s = axis2 > 1e-12 ? THREE.MathUtils.clamp(axisFrom / axis2, 0, 1) : 0;
  if (from2 - 2 * s * axisFrom + s * s * axis2 <= radius * radius) return 0;
  let best = null;
  const a = axis2 - axisDir * axisDir;
  if (a > 1e-12) {
    // The side: the ray against the infinite cylinder, kept between the ends.
    const b = axis2 * dir.dot(capFrom) - axisFrom * axisDir;
    const c = axis2 * from2 - axisFrom * axisFrom - radius * radius * axis2;
    const h = b * b - a * c;
    // Clear of the infinite cylinder, it is clear of the capsule inside it.
    if (h < 0) return null;
    const t = (-b - Math.sqrt(h)) / a;
    const y = axisFrom + t * axisDir;
    if (y > 0 && y < axis2 && t >= 0 && t <= far) best = t;
  }
  // The rounded ends.
  for (let end = -1; end <= 1; end += 2) {
    capEnd.copy(center).addScaledVector(half, end);
    const t = segmentSphere(origin, dir, best ?? far, capEnd, radius);
    if (t !== null && (best === null || t < best)) best = t;
  }
  return best;
}

// The entry distance into an upright cylinder, or null; 0 from inside.
export function segmentCylinder(origin, dir, far, center, radius, half) {
  let near = 0;
  let farT = far;
  const oy = origin.y - center.y;
  if (Math.abs(dir.y) < 1e-12) {
    if (Math.abs(oy) > half) return null;
  } else {
    let t1 = (-half - oy) / dir.y;
    let t2 = (half - oy) / dir.y;
    if (t1 > t2) [t1, t2] = [t2, t1];
    near = Math.max(near, t1);
    farT = Math.min(farT, t2);
    if (near > farT) return null;
  }
  const ox = origin.x - center.x;
  const oz = origin.z - center.z;
  const a = dir.x * dir.x + dir.z * dir.z;
  const c = ox * ox + oz * oz - radius * radius;
  if (a < 1e-12) return c > 0 ? null : near;
  const b = ox * dir.x + oz * dir.z;
  const disc = b * b - a * c;
  if (disc < 0) return null;
  const root = Math.sqrt(disc);
  near = Math.max(near, (-b - root) / a);
  farT = Math.min(farT, (-b + root) / a);
  return near <= farT ? near : null;
}

// The entry distance into a box turned by `yaw` about the vertical, or null.
export function segmentBox(origin, dir, far, center, half, yaw) {
  const sin = Math.sin(yaw);
  const cos = Math.cos(yaw);
  // Into the box's frame: its z along (sin yaw, 0, cos yaw), x along (cos yaw, 0, -sin yaw).
  local.subVectors(origin, center);
  const lx = local.x * cos - local.z * sin;
  const lz = local.x * sin + local.z * cos;
  localDir.set(dir.x * cos - dir.z * sin, dir.y, dir.x * sin + dir.z * cos);
  const o = [lx, local.y, lz];
  const d = [localDir.x, localDir.y, localDir.z];
  const h = [half.x, half.y, half.z];
  let near = 0;
  let farT = far;
  for (let i = 0; i < 3; i++) {
    if (Math.abs(d[i]) < 1e-12) {
      if (Math.abs(o[i]) > h[i]) return null;
      continue;
    }
    let t1 = (-h[i] - o[i]) / d[i];
    let t2 = (h[i] - o[i]) / d[i];
    if (t1 > t2) [t1, t2] = [t2, t1];
    near = Math.max(near, t1);
    farT = Math.min(farT, t2);
    if (near > farT) return null;
  }
  return near;
}

// The first distance in [0, far] at which a ray from `origin` along unit
// `dir` is inside the sphere, or null. A ray starting inside hits at 0.
export function segmentSphere(origin, dir, far, center, radius) {
  toCenter.subVectors(center, origin);
  const c = toCenter.lengthSq() - radius * radius;
  if (c <= 0) return 0;
  const b = toCenter.dot(dir);
  if (b < 0) return null;
  const disc = b * b - c;
  if (disc < 0) return null;
  const t = b - Math.sqrt(disc);
  return t <= far ? t : null;
}

// Whether a segment from `from` to `to` passes clear of every living unit
// other than those excluded (for sight tests).
export function unitsClear(units, from, to, exclude) {
  scratch.subVectors(to, from);
  const span = scratch.length();
  if (span < 1e-6) return true;
  scratch.divideScalar(span);
  return !units.raycast(from, scratch, span, exclude, losHit);
}
