import { weaponAI } from './weapon-policy.js';
import { derivedFrom } from './combat-sources.js';

export const hasMissileLock = def => def?.base === 'launcher' && def.ordnance?.base === 'missile';

// These tank launchers ship as cannon/bullet weapons with zeroed targeting
// fields. Enable the same acquisition and guidance path as infantry launchers
// without changing their damage, ammunition, speed or firing schedules. The
// Imperial IFT-T carries the same concussion missile as the Republic IFT-X.
const tankMissiles = new Set(['rep_weap_hover_fightertank_missile', 'imp_weap_hover_fightertank_missile', 'all_weap_hover_combatspeeder_missile']);
export function vehicleMissileDefinition(def) {
  if (!tankMissiles.has(def?.class)) return def;
  return derivedFrom({ ...def, base: 'launcher', LockOnRange: 150, LockOnAngle: 6,
    LockOffAngle: 10, LockTime: 1, ClearLockOnFire: 1,
    TargetEnemy: 1, TargetFriendly: 0, TargetNeutral: 0,
    TargetPerson: 0, TargetAnimal: 0, TargetDroid: 0, TargetVehicle: 1, TargetBuilding: 0,
    ai: { ...weaponAI(def), relationMask: 1, categoryMask: 8, extremeRange: 150 },
    ordnance: derivedFrom({ ...def.ordnance, base: 'missile', TurnRate: 6, ScaleTime: 1 }, def.ordnance) }, def);
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
