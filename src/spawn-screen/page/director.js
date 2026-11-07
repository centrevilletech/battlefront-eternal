import {CameraFrame as F,CameraOutput as O,CameraLimits as L} from '../../presentation/layout.js';
export const HOLD=L.HOLD_SECONDS;

// The spawn screen's HUD clock is a host fact: it only ticks when the HUD
// prepares a drawn screen, and its cut flash must start in that same paint.
// Numeric shot selection, aim and poses belong to the native camera owner.
export class ShotDirector{
  constructor(owner,shots=[]){
    this.owner=owner;this.shots=shots;this.index=-1;this.age=0;this.cut=false;this.revision=0;
    this.record={director:true,age:0,revision:0,cut:false};
  }
  get active(){return this.shots.length>0;}
  show(index){
    if(!this.active)return;
    this.index=this.owner.x.camera_director_show(this.owner.handle,index);this.owner.refresh();
    this.revision=this.owner.output[O.DIRECTOR_REVISION];this.age=0;this.cut=true;
  }
  bestFor(point){return this.owner.x.camera_director_best(this.owner.handle,point.x??point[0],point.y??point[1],point.z??point[2]);}
  follow(position){
    if(!this.active||!position)return;
    const before=this.index;
    this.index=this.owner.x.camera_director_follow(this.owner.handle,position[0],position[1],position[2]);this.owner.refresh();
    this.revision=this.owner.output[O.DIRECTOR_REVISION];
    if(before!==this.index){this.age=0;this.cut=true;}
  }
  update(dt){
    if(!this.active)return null;
    if(this.index<0)this.show(0);
    this.age+=dt;if(this.age>=HOLD)this.show(this.index+1);
    // Commit by revision. Later UI events preserve this exact selection
    // until the next camera phase consumes it, without another frame call.
    const record=this.record;record.age=this.age;record.revision=this.revision;record.cut=this.cut;this.cut=false;
    this.owner.refresh();this.owner.input[F.DIRECTOR_REVISION]=this.revision;
    return record;
  }
}
