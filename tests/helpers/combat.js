// Isolated presentation fixtures publish the native phase explicitly. Live
// PlayerCombat has no numeric aim/eye methods or per-object WASM adapter.
import * as THREE from 'three';
import {PlayerCombat as Combat} from '../../src/combat.js';
import {BodyPreview} from '../../src/presentation/body.js';
import {owner} from './body.js';
const phases=new WeakMap();
function phase(combat){
  let p=phases.get(combat);
  if(!p){p=new BodyPreview(combat.collision,owner(combat.collision));p.viewCombat={zoom:1};phases.set(combat,p);}
  return p;
}
export class PlayerCombat extends Combat {
  aimBasis(yaw=this.soldier.aimYaw,pitch=this.soldier.aimPitch){
    const p=phase(this);p.begin(this.soldier,this);p.aimYaw=yaw;p.aimPitch=pitch;p.run();
    p.sourceYaw=yaw;p.sourcePitch=pitch;
    for(const key of ['origin','pivot','left','up','aimDir'])this[key].copy(p.basis[key]);
    (this.eyePoint??=new THREE.Vector3()).copy(p.basis.eyePoint);this.viewAim=p.basis;
  }
  eye(feet,out){
    const p=phase(this);p.begin(this.soldier,this,feet);
    p.aimYaw=p.sourceYaw??this.soldier.aimYaw;p.aimPitch=p.sourcePitch??this.soldier.aimPitch;
    p.run();return out.copy(p.basis.eyePoint);
  }
  viewBasis(pose){const p=phase(this);p.viewCombat.zoom=this.zoom;p.begin(pose,p.viewCombat);p.run();this.viewAim=p.basis;return p.basis;}
  updateHud(turret=null,pose=null,preview=null){if(pose)this.viewBasis(pose);return super.updateHud(turret,pose,preview);}
  convergedAim(yaw,pitch){
    if(typeof yaw==='object')return super.convergedAim(yaw);
    const p=phase(this);p.begin(this.soldier,this);p.aimYaw=yaw;p.aimPitch=pitch;p.run();
    // Isolated assertions retain answers across later preview calls.
    return {...super.convergedAim(p)};
  }
}
