import { weaponAI } from './weapon-policy.js';
import { derivedFrom } from './combat-sources.js';
import {targetFrame,runTargetFrame,vehicleMissilePolicy} from './targeting-facts.js';
import {PageTargetFrameLayout as F,VehicleMissilePolicyLayout as P} from './sim/gen/page-targeting.js';

export const hasMissileLock = def => def?.base === 'launcher' && def.ordnance?.base === 'missile';

// These tank launchers ship as cannon/bullet weapons with zeroed targeting
// fields. Enable the same acquisition and guidance path as infantry launchers
// without changing their damage, ammunition, speed or firing schedules. The
// Imperial IFT-T carries the same concussion missile as the Republic IFT-X.
export function vehicleMissileDefinition(def) {
  const p=vehicleMissilePolicy(def?.class);if(!p[P.enabled])return def;
  return derivedFrom({...def,base:'launcher',LockOnRange:p[P.lockOnRange],LockOnAngle:p[P.lockOnAngle],LockOffAngle:p[P.lockOffAngle],LockTime:p[P.lockTime],ClearLockOnFire:p[P.clearLockOnFire],
    TargetEnemy:p[P.targetEnemy],TargetFriendly:p[P.targetFriendly],TargetNeutral:p[P.targetNeutral],TargetPerson:p[P.targetPerson],TargetAnimal:p[P.targetAnimal],TargetDroid:p[P.targetDroid],TargetVehicle:p[P.targetVehicle],TargetBuilding:p[P.targetBuilding],
    ai:{...weaponAI(def),relationMask:p[P.relationMask],categoryMask:p[P.categoryMask],extremeRange:p[P.extremeRange]},
    ordnance:derivedFrom({...def.ordnance,base:'missile',TurnRate:p[P.turnRate],ScaleTime:p[P.scaleTime]},def.ordnance)},def);
}

export function targetingWeapon(arsenal) {
  const primary = arsenal?.current(0), secondary = arsenal?.current(1);
  return hasMissileLock(primary?.def)?primary:hasMissileLock(secondary?.def)?secondary:primary;
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
  const angle=Number(def?.LockOffAngle)||0;
  return tickRadii(angle,angle,camera,height);
}
const radii=[0,0];
function tickRadii(yaw,pitch,camera,height){
  const f=targetFrame();f[F.fov]=camera?.fov??46.826;f[F.aspect]=camera?.aspect??4/3;f[F.height]=height;f[F.yaw]=yaw;f[F.pitch]=pitch;
  runTargetFrame(4);radii[0]=f[F.x];radii[1]=f[F.y];return radii;
}

// A reticle without art uses the native spread angles in the same view.
export function spreadTickRadii(weapon,camera,height=768){
  const spread=weapon?.spread?.(0,'stand',false);
  return tickRadii(spread?.[0]??0,spread?.[1]??0,camera,height);
}
