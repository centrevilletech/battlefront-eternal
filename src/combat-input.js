// Immediate cosmetic feedback reads the same native fire and direction rules.
import {PageFireFlag_VALUE as Flag} from './sim/gen/page-targeting.js';
import {clientExports as x} from './runtime/fixed-clock.js';
export function infantryFireBlocked(soldier,gunUnderwater,stick=0){
  let flags=gunUnderwater?1<<Flag.underwater:0;
  if(soldier){if(soldier.air&&soldier.air!=='jet')flags|=1<<Flag.air;if(soldier.dive)flags|=1<<Flag.dive;if(soldier.rolled)flags|=1<<Flag.rolled;if(soldier.posture==='prone')flags|=1<<Flag.prone;}
  return !!x.page_fire_blocked(flags,stick,soldier?.lock??0);
}
export const damageDirection=(xValue,z,yaw)=>x.page_damage_direction(xValue,z,yaw);
