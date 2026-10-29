import {spreadSeed} from './weapons.js';

// Shared input mapping for authority and cosmetic Weapon/Arsenal replay.
export function readWeaponInput(controls){
  const primary=controls.take('firePrimary'),secondary=controls.take('fireSecondary');
  return {primary:{held:controls.held('firePrimary')||primary,pressed:primary},secondary:{held:controls.held('fireSecondary')||secondary,pressed:secondary},reload:controls.take('reload')};
}
// A press with an id (presses: each channel's, or null) gives its trigger
// the press's spread seed (weapons.js spreadSeed). The authority takes the
// id from the trigger edge the press arrived with, the page predicting his
// rounds from its own edge, so both draw the same spread.
export function seedPresses(input,unit,presses){
  for(const channel of [0,1]){
    const trigger=channel?input.secondary:input.primary,press=presses?.[channel];
    if(press&&trigger.pressed)trigger.seed=spreadSeed(unit,channel,press);
  }
  return input;
}
// How late each channel's press reached the authority (late: ticks past
// the tick it was stamped for), in seconds on its trigger (weapons.js).
export function latePresses(input,lates){
  for(const channel of [0,1]){
    const trigger=channel?input.secondary:input.primary,late=lates?.[channel];
    if(trigger.pressed&&late>0)trigger.late=Math.min(late,15)/60;
  }
  return input;
}
export function selectCombatWeapons(combat,controls,zooms){
  if(controls.take('nextPrimary')&&combat.arsenal.next(0))combat.zoomed=false;
  if(controls.take('previousPrimary')&&combat.arsenal.next(0,-1))combat.zoomed=false;
  if(controls.take('nextSecondary'))combat.arsenal.next(1);
  const primary=combat.arsenal.current(0);
  if(controls.take('zoom')&&primary)combat.zoomStep=(combat.zoomStep+1)%zooms(primary.def).length;
  const s=combat.soldier;
  if(s.air&&s.air!=='jet'||s.dive||primary?.reloading)combat.zoomed=false;
  return primary;
}
