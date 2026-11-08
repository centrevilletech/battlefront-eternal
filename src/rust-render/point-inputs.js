// Explicit light writers publish colour/range facts once per phase. Native
// scene links own discovery, visibility and the first-person source snapshot.
import {sceneNodes} from './scene-changes.js';
import {SceneCapacity} from './scene-layout.js';
import {POINT_CHANGE as P} from './camera-layout.js';
export function publishPointLight(light){const node=sceneNodes.get(light);if(node)node.owner.points.touch(node);}
export class PointInputs{
 constructor(registry){
  this.registry=registry;this.count=0;this.ids=new Uint32Array(SceneCapacity.NODES);this.queued=new Uint8Array(SceneCapacity.NODES);
  const core=registry.core;core.addView('scenePointChanges',Float64Array,()=>core.rust.scene_point_changes_ptr(),()=>core.rust.scene_point_changes_len());
 }
 admit(node){const object=node.object;if(object.isScene)this.registry.core.rust.scene_point_root_frame(node.slot,node.generation);if(object.isPointLight)this.touch(node);}
 touch(node){
  const light=node.object;let row=node.pointInput,changed=false;
  if(!row){row=node.pointInput=new Float64Array(P.LEN);row.fill(NaN);}
  if(!Object.is(row[P.COLOR],light.color.r)){row[P.COLOR]=light.color.r;changed=true;}
  if(!Object.is(row[P.COLOR+1],light.color.g)){row[P.COLOR+1]=light.color.g;changed=true;}
  if(!Object.is(row[P.COLOR+2],light.color.b)){row[P.COLOR+2]=light.color.b;changed=true;}
  if(!Object.is(row[P.INTENSITY],light.intensity)){row[P.INTENSITY]=light.intensity;changed=true;}
  if(!Object.is(row[P.RANGE],light.distance)){row[P.RANGE]=light.distance;changed=true;}
  if(!Object.is(row[P.GENERATION],node.generation)){row[P.SLOT]=node.slot;row[P.GENERATION]=node.generation;changed=true;}
  if(changed&&!this.queued[node.slot]){this.queued[node.slot]=1;this.ids[this.count++]=node.slot;}
 }
 flush(){
  if(!this.count)return;const core=this.registry.core;core.views();
  for(let i=0;i<this.count;i++){const id=this.ids[i];core.scenePointChanges.set(this.registry.nodes[id].pointInput,i*P.LEN);this.queued[id]=0;}
  core.rust.scene_points_apply_frame(this.count);this.count=0;
 }
}
