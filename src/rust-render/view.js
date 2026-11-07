// First-person parts use the same native source/appearance descriptors as world
// actors. This publisher retains only view ordering, poses and live glow values;
// native instances/effects own all geometry, material, sampler and raster state.
import * as THREE from 'three';
import {ACTOR} from './actor-layout.js';
import {isActorMaterial,claimActor,nativeActorRecord} from './actors.js';
import {writeActor} from './instances.js';
const D={PROGRAM:0,GEOMETRY:1,RANGE:14,UNIFORMS:16},DRAW=17+32*18,MAX_DRAWS=16;
const order=(a,b)=>a.groupOrder-b.groupOrder||a.renderOrder-b.renderOrder||b.z-a.z||a.id-b.id;
class ViewPass {
 constructor(core){this.core=core;this.name='view';this.phase='view';this.parts=new WeakMap();this.records=[];this.blends=[];this.screen=new THREE.Matrix4();this.frustum=new THREE.Frustum();this.point=new THREE.Vector4();this.scratch=new Float64Array(1+MAX_DRAWS*DRAW);this.complete=false;
  core.addView('viewInput',Float64Array,()=>core.rust.view_input_ptr(),()=>core.rust.view_input_len());
 }
 admit(node){let record=this.parts.get(node);if(record?.appearance===node.material)return record;
  record=claimActor(this.core,node,()=>nativeActorRecord(this.core,node,true));if(record){this.parts.set(node,record);this.records.push(record);}return record;
 }
 draw(ctx){this.complete=true;this.blends.length=0;this.blending=0;this.screen.multiplyMatrices(ctx.camera.projectionMatrix,ctx.camera.matrixWorldInverse);this.frustum.setFromProjectionMatrix(this.screen);this.visit(ctx.scene,ctx,0);this.publish(ctx.camera);}
 // Hidden parents suppress descendants; layer mismatch affects only that node.
 // Scene membership and matrix production stay with their existing owners.
 visit(node,ctx,group){if(!node.visible)return;if(node.isGroup)group=node.renderOrder;
  if(node.layers.test(ctx.camera.layers)){
   if(node.userData.actorAsset&&isActorMaterial(node.material)){
    const record=this.admit(node);if(record&&(!node.frustumCulled||this.frustum.intersectsObject(node))){if(writeActor(this.core,record,ctx)&&record.blended){record.k=this.blending++;this.blend(record,node,group);}}
   }else if(node.userData.effectBatch){let record=this.parts.get(node);if(!record){record={node,fx:this.core.byName.effects.viewBatch(node)};this.parts.set(node,record);}this.blend(record,node,group);
   }else if(node.userData.effectModel?.family==='actorEffect'){let record=this.parts.get(node);if(!record){record={node,model:this.core.byName.effects.viewModel(node)};this.parts.set(node,record);}if(!node.frustumCulled||this.frustum.intersectsObject(node))this.blend(record,node,group);
   }else if(node.isMesh||node.isSprite||node.isPoints||node.isLine)this.core.skip(node,node.material,'view resource has no native descriptor');
  }
  for(let i=0;i<node.children.length;i++)this.visit(node.children[i],ctx,group);
 }
 // Retained sort rows preserve the old clip-Z arithmetic and stable identities.
 // A full view store clips later effects without throwing during a slow frame.
 blend(record,node,group){if(this.blends.length>=MAX_DRAWS){this.core.rust.frame_overflow_frame();return;}const row=record.sort??={record,groupOrder:0,renderOrder:0,z:0,id:node.id};
  const center=node.boundingSphere?.center;this.point.set(center?.x||0,center?.y||0,center?.z||0,1).applyMatrix4(node.matrixWorld).applyMatrix4(this.screen);row.groupOrder=group;row.renderOrder=node.renderOrder;row.z=this.point.z;this.blends.push(row);
 }
 publish(camera){const core=this.core,input=this.scratch;this.blends.sort(order);let n=0,at=0,matrixAt=0;
  for(let i=0;i<this.blends.length;i++){const record=this.blends[i].record,o=1+n*DRAW;
   if(record.fx){const batch=record.fx.batch,end=core.byName.effects.writeViewBatch(record.fx,camera,at);input[o+D.PROGRAM]=-2;input[o+D.GEOMETRY]=record.fx.id;input[o+D.RANGE]=at;input[o+D.RANGE+1]=end===at?0:Math.min(batch.count,batch.capacity);input[o+D.UNIFORMS]=batch.capacity;at=end;
   }else if(record.model){const end=core.byName.effects.writeViewModel(record.model,camera,at,matrixAt);if(end===at)continue;input[o+D.PROGRAM]=ACTOR.KIND.VIEW_MODEL;input[o+D.GEOMETRY]=record.model.model;input[o+D.RANGE]=matrixAt;input[o+D.RANGE+1]=at;at=end;matrixAt+=16;
   }else{input[o+D.PROGRAM]=-1;input[o+D.GEOMETRY]=record.k;}
   n++;
  }
  input[0]=n;core.views();for(let i=0;i<1+n*DRAW;i++)core.viewInput[i]=input[i];
 }
 reveal(){}
}
export function viewPass(core){return new ViewPass(core);}
