import {Arsenal} from '../weapons.js';
import {prepareMelee} from '../heroes.js';
import {captureArsenalState,restoreArsenalState} from './network-arsenal-state.js';

const random=()=>.5;
// A private presentation view for the Rust session output.
export class PredictionArsenal {
  constructor(){this.life=null;this.presentedThrough=-1;}
  restore(){if(!this.saved)return;this.combat.arsenal=this.saved.arsenal;this.combat.zoomStep=this.saved.zoomStep;this.saved=null;}
  dispose(){this.restore();}
  begin(unit,{epoch=1,tick}={}){
    this.restore();
    const combat=unit.combat,authority=combat.arsenal,life=`${epoch}:${unit.id}:${unit.generation}`;
    if(this.life!==life||this.source!==authority){
      this.life=life;this.source=authority;this.presentedThrough=tick;
      this.arsenal=prepareMelee(new Arsenal(authority.weapons.map(w=>w.levels[0]),{random}));
    }
    this.unit=unit;this.combat=combat;this.saved={arsenal:authority,zoomStep:combat.zoomStep};
    const state={...captureArsenalState(authority),zoomStep:combat.zoomStep};
    // Captured states reference their source weapons; rebind to private slots.
    restoreArsenalState(this.arsenal,{selected:state.selected,weapons:state.weapons.map((entry,i)=>({...entry,weapon:this.arsenal.weapons[i]}))});
    combat.arsenal=this.arsenal;combat.zoomStep=state.zoomStep;
    this.presentedThrough=Math.max(this.presentedThrough,tick);
  }
}
