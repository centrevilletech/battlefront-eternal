import * as THREE from 'three';
import { unitsArray, defaultRelation, lockSpan } from './hud-units.js';
import { weaponAI, categoryOf, relationBit } from './ai/classic/weapons-ai.js';

export const hasMissileLock = def => def?.base === 'launcher' && def.ordnance?.base === 'missile';

// These tank launchers ship as cannon/bullet weapons with zeroed targeting
// fields. Enable the same acquisition and guidance path as infantry launchers
// without changing their damage, ammunition, speed or firing schedules. The
// Imperial IFT-T carries the same concussion missile as the Republic IFT-X.
const tankMissiles = new Set(['rep_weap_hover_fightertank_missile', 'imp_weap_hover_fightertank_missile', 'all_weap_hover_combatspeeder_missile']);
export function vehicleMissileDefinition(def) {
  if (!tankMissiles.has(def?.class)) return def;
  return { ...def, base: 'launcher', LockOnRange: 150, LockOnAngle: 6,
    LockOffAngle: 10, LockTime: 1, ClearLockOnFire: 1,
    TargetEnemy: 1, TargetFriendly: 0, TargetNeutral: 0,
    TargetPerson: 0, TargetAnimal: 0, TargetDroid: 0, TargetVehicle: 1, TargetBuilding: 0,
    ai: { ...weaponAI(def), relationMask: 1, categoryMask: 8, extremeRange: 150 },
    ordnance: { ...def.ordnance, base: 'missile', TurnRate: 6, ScaleTime: 1 } };
}

export function targetingWeapon(arsenal) {
  const primary = arsenal?.current(0), secondary = arsenal?.current(1);
  return [primary, secondary].find(w => hasMissileLock(w?.def)) || primary;
}

export function lockedUnit(weapon, units) {
  const unit = units?.byId?.(weapon?.lockTargetId);
  return unit?.alive && (unit.generation ?? 0) === weapon.lockGeneration ? unit : null;
}

// Who picks a launcher's target. A bot's controller hands its weapon its
// target, wherever the bot looks: on foot the unit's own, in a
// seat or at a turret the seat's (lease.controllable, src/seats.js), which
// the bot's own controller is not. A human (the player, or a network human
// on a server) locks only what his own view puts in the weapon's cone
// (undefined: no target is handed over).
export function botLockTarget(unit, controller = unit?.controller) {
  if (!unit || unit.human || unit.networkHuman || unit.combat?.player) return undefined;
  const ref = controller?.perception?.fire?.weaponTarget;
  return ref?.unit && (ref.unit.generation ?? 0) === ref.generation ? ref.unit : null;
}
// A seat's or turret's bot: its lease's.
export const seatLockTarget = lease => lease && !lease.human ? botLockTarget(lease.unit, lease.controllable?.controller ?? lease.unit?.controller) : undefined;

// The weapon's own masks pass the target (a bot's target is chosen for the
// weapon it has selected, which may not be this one).
function allowed(def, viewer, unit, relation = defaultRelation) {
  const masks = weaponAI(def);
  return Boolean(masks.relationMask & relationBit(relation(viewer?.team, unit.team))) && Boolean(masks.categoryMask & (1 << categoryOf(unit)));
}

const span = { angle: Math.PI, distance: Infinity };
const view = new THREE.Vector3(), centre = new THREE.Vector3();

// The target a launcher acquires (LockOnAngle) or keeps (LockOffAngle) along
// a view: alive, allowed by its masks, part of it inside the cone and within
// LockOnRange, and in sight. The one nearest the view's line wins, then the
// nearer. The original widens its cone by the target's bounding radius,
// which at 15 m took an AAT with the reticle 30 degrees off its
// centre, 8 degrees clear of its hull; the port wants the hull itself
// inside the cone (docs/weapon-targeting.md).
export function findLockTarget({ origin, dir, units, weapon, viewer = null, relation = defaultRelation, los = () => true, angle }) {
  const def = weapon?.def || weapon;
  const range = Number(def?.LockOnRange) || 0;
  if (!def || !origin || !dir || !(range > 0) || !view.copy(dir).normalize().lengthSq()) return null;
  const cone = Math.max(0, Number(angle) || 0) * Math.PI / 180;
  let best = null;
  for (const unit of unitsArray(units)) {
    if (unit === viewer || !unit.alive || unit.seat || (unit.health ?? unit.combat?.health ?? 1) <= 0 || unit.kind === 10) continue;
    if (!allowed(def, viewer, unit, relation)) continue;
    lockSpan(origin, view, unit, span);
    if (span.angle > cone || span.distance > range) continue;
    if (best && (best.angle < span.angle || best.angle === span.angle && best.distance <= span.distance)) continue;
    if (unit.aimPoint) unit.aimPoint(centre);
    else centre.copy(unit.sphere?.center || unit.position);
    if (!los(origin, centre, viewer, unit)) continue;
    best = { unit, angle: span.angle, distance: span.distance };
  }
  return best;
}

const direction = new THREE.Vector3();
const hit = { point: new THREE.Vector3(), normal: new THREE.Vector3() };

// The one lock rule for soldiers, vehicle seats and turrets, the player's,
// a bot's and a network human's alike. Acquisition and retention use
// separate cones around the view (origin, dir: where the reticle points).
// A cannon's targeting fields still colour its reticle, but never start
// launcher state 4.
export function updateWeaponLock(weapon, dt, { units, collision, origin, dir, viewer, ignoreBody = null, target } = {}) {
  if (!hasMissileLock(weapon.def)) return;
  if (!units || !origin || !dir) { weapon.clearLock(); return; }
  const relation = units.relation ? (a, b) => units.relation(a, b) : undefined;
  const los = (from, to, source, candidate) => {
    direction.subVectors(to, from);
    const length = direction.length();
    if (!collision || length < 1e-6) return true;
    const contact = collision.cast(from, direction.divideScalar(length), length, hit, ignoreBody);
    return !contact || Boolean(candidate.vehicle && contact.body?.owner === candidate.vehicle) || Boolean(candidate.turret && contact.body?.owner === candidate.turret)
      || Boolean(candidate.building && contact.body?.owner === candidate.building);
  };
  const options = { origin, dir, weapon, viewer, relation, los };
  const current = lockedUnit(weapon, units);
  let picked;
  // A bot's controller supplies its own target (botLockTarget).
  if (target !== undefined) picked = target?.alive && allowed(weapon.def, viewer, target, relation) ? { unit: target } : null;
  else {
    picked = weapon.lockComplete && current && findLockTarget({ ...options, units: [current], angle: weapon.def.LockOffAngle || weapon.def.LockOnAngle });
    picked ||= findLockTarget({ ...options, units, angle: weapon.def.LockOnAngle });
  }
  weapon.updateLock(dt, picked?.unit);
}

// The original HUD's eight five-unit radial ticks. The horizontal camera
// FOV already includes zoom in the port.
export function lockTickRadii(def, camera, height = 768) {
  const vfov = (camera?.fov ?? 46.826) * Math.PI / 180;
  const hfov = 2 * Math.atan(Math.tan(vfov / 2) * (camera?.aspect ?? 4 / 3));
  const angle = Math.max(0, Number(def?.LockOffAngle) || 0) * Math.PI / 180;
  return [1024 * angle / hfov, height * angle / (hfov * .75)];
}

// A reticle without art (a seat's weapon with no ReticuleTexture): its four
// ticks sit as far out as the weapon's spread reaches now, yaw across and
// pitch up and down, by the same measure.
export function spreadTickRadii(weapon, camera, height = 768) {
  const [yaw, pitch] = weapon?.spread?.(0, "stand", false) ?? [0, 0];
  const across = lockTickRadii({ LockOffAngle: yaw }, camera, height)[0];
  const up = lockTickRadii({ LockOffAngle: pitch }, camera, height)[1];
  return [across, up];
}
