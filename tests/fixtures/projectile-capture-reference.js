// ProjectileCapture.sample as recorded before canonical publication. Client
// presentation tests use fake ordnance lists; production never imports this.
import { CombatCapture as Capture } from '../../src/runtime/network-entity-combat.js';
import { projectileRecord } from '../../src/runtime/network-combat-projectile.js';
function sample(){
  const c=this.capture,tick=c.tick(),live=new Set();
  for(const[type,list]of[['grenade',this.ordnance.grenades],['round',this.ordnance.rounds],['beam',this.ordnance.beams]])for(const p of list){
   if(p.gone||type==='beam'&&p.life<=0)continue;live.add(p);this.launch(p,type);const entry=this.active.get(p);if(entry.terminal)continue;
   c.emit(projectileRecord({...entry,tick,position:p.position.toArray(),velocity:p.velocity?.toArray()??[0,0,0],quaternion:(p.quaternion??p.mesh?.quaternion)?.toArray()??[0,0,0,1],end:p.end?.toArray()??p.position.toArray(),life:Math.max(0,p.life??p.fuse??0),native:c.native}));
  }
  for(const[p,entry]of this.active)if(!live.has(p)){if(!entry.terminal)c.emit(projectileRecord({...entry,tick,terminal:true,native:c.native}));this.active.delete(p);}
 }
export class CombatCapture extends Capture {
 drain(){try{if(this.dynamic)sample.call(this.dynamic);}catch(error){this.fault(error);}return super.drain();}
}
