import {Arsenal} from '../weapons.js';
import {prepareMelee} from '../heroes.js';
import {PlayerCombat} from '../combat.js';
import {infantryFireBlocked} from '../combat-input.js';
import {readWeaponInput,seedPresses,selectCombatWeapons} from '../combat-weapon-input.js';
import {CommandControls} from './commands.js';
import {captureArsenalState,restoreArsenalState} from './network-arsenal-state.js';

const random=()=>.5;
// Advance the actual Weapon/Arsenal rules on private state. Only returned
// cosmetic shots leave this module: no combat.fire, world events or damage.
// A round of a press the authority will know (presses: each channel's press
// id this tick, or null) takes the press's seeded spread, the authority's
// own draws (weapons.js spreadSeed), so a salvo's pellets fan out where the
// authority's fly; any other round keeps the middle of the spread.
export class WeaponPrediction {
  constructor(){this.controls=new CommandControls();this.life=null;this.presentedThrough=-1;}
  restore(){if(!this.saved)return;this.combat.arsenal=this.saved.arsenal;this.combat.zoomStep=this.saved.zoomStep;this.saved=null;}
  begin(unit,{epoch=1,tick,checkpoint=null}={}){
    this.restore();
    const combat=unit.combat,authority=combat.arsenal,life=`${epoch}:${unit.id}:${unit.generation}`;
    if(this.life!==life||this.source!==authority){
      this.life=life;this.source=authority;this.presentedThrough=tick;
      this.arsenal=prepareMelee(new Arsenal(authority.weapons.map(w=>w.levels[0]),{random}));
    }
    this.unit=unit;this.combat=combat;this.saved={arsenal:authority,zoomStep:combat.zoomStep};
    const state=checkpoint||{...captureArsenalState(authority),zoomStep:combat.zoomStep};
    // Captured states reference their source weapons; rebind to private slots.
    restoreArsenalState(this.arsenal,{selected:state.selected,weapons:state.weapons.map((entry,i)=>({...entry,weapon:this.arsenal.weapons[i]}))});
    combat.arsenal=this.arsenal;combat.zoomStep=state.zoomStep;
    this.presentedThrough=Math.max(this.presentedThrough,tick);
  }
  step(command,tick,onShot=()=>{},presses=null){
    const combat=this.combat,controls=this.controls;
    controls.down=new Set(command.down);controls.queued.clear();
    for(const action of command.actions)controls.queued.add(action);
    const input=seedPresses(readWeaponInput(controls),this.unit,presses);
    selectCombatWeapons(combat,controls,PlayerCombat.zooms);
    const stick=Math.max(Math.abs(controls.axis('forward')),Math.abs(controls.axis('right')));
    const blocked=infantryFireBlocked(combat.soldier,combat.gunUnderwater,stick);
    // Deployed remote control and shield effects belong to their existing
    // prediction modules. Their weapon timers still use the common rules.
    for(const channel of [0,1]){
      const weapon=this.arsenal.current(channel);
      if(weapon?.deployed&&weapon.def.droid||weapon?.def.base==='shield'){
        input[channel?'secondary':'primary']={held:false,pressed:false};
      }
    }
    const shots=this.arsenal.update(1/60,input,{stick,posture:combat.soldier.posture,zoomed:combat.zoomed,blocked,repairTarget:null});
    if(tick>this.presentedThrough)for(const shot of shots)onShot(shot,command,tick);
    this.presentedThrough=Math.max(this.presentedThrough,tick);
    return shots;
  }
  checkpoint(){return {...captureArsenalState(this.arsenal),zoomStep:this.combat.zoomStep};}
}

// Only attribute a cosmetic shot to the command(s) consumed on this tick.
// Collapsed same-channel taps have no unique predicted authority identity.
export function predictedPress(edges,sequences,button){
  const matches=edges.filter(edge=>edge.button===button&&sequences.includes(edge.sequence));
  return matches.length>1?false:matches[0]??null;
}
