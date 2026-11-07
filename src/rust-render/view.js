// First-person parts use the same native source/appearance descriptors as world
// actors. This publisher retains only view ordering, poses and live glow values;
// native instances/effects own all geometry, material, sampler and raster state.
import {Family,Mode,S} from './scene.js';
import {ACTOR} from './actor-layout.js';
import {isActorMaterial,claimActor,nativeActorRecord} from './actors.js';
import {writeActor} from './instances.js';
const D={PROGRAM:0,GEOMETRY:1,RANGE:14,UNIFORMS:16},DRAW=17+32*18,MAX_DRAWS=16;
class ViewPass {
 constructor(core){this.core=core;this.name='view';this.phase='view';this.parts=new WeakMap();this.records=[];this.blends=[];this.scratch=new Float64Array(1+MAX_DRAWS*DRAW);this.complete=false;
  core.addView('viewInput',Float64Array,()=>core.rust.view_input_ptr(),()=>core.rust.view_input_len());
 }
 // View anchors are admitted once with their native actor/effect owner.
 // Rust emits the complete opaque/transparent order for each view camera.
 admit(node){let record=this.parts.get(node);if(record&&(record.fx||record.model||record.appearance===node.material))return record;
  if(node.userData.actorAsset&&isActorMaterial(node.material))record=claimActor(this.core,node,()=>nativeActorRecord(this.core,node,true));
  else if(node.userData.effectBatch)record={node,fx:this.core.byName.effects.viewBatch(node),blended:true};
  else if(node.userData.effectModel?.family==='actorEffect')record={node,model:this.core.byName.effects.viewModel(node),blended:true};
  else return null;
  if(record){const previous=this.parts.get(node);if(previous){const at=this.records.indexOf(previous);if(at>=0)this.records[at]=record;else this.records.push(record);}else this.records.push(record);this.parts.set(node,record);const entry=this.core.registry.bind(node,Family.VIEW,record);if(entry){entry.blended=!!record.blended;this.core.registry.touch(entry);}}return record;
 }
 // Native order contains both opaque actors and transparent contributions.
 // Their retained rows feed the same packet without a JS sort or child walk.
 draw(ctx){this.complete=true;this.blends.length=0;this.blending=0;
  const core=this.core,registry=core.registry,count=registry.select(ctx.scene,ctx.camera,Family.VIEW,Mode.VIEW);
  for(let i=0;i<count;i++){core.views();const record=registry.nodes[core.sceneOutput[i*S.STRIDE+S.SLOT]].records[Family.VIEW];if(!record)continue;
   if(!record.fx&&!record.model){if(!writeActor(core,record,ctx))continue;if(!record.blended)continue;record.k=this.blending++;}
   if(this.blends.length<MAX_DRAWS)this.blends.push(record);else core.rust.frame_overflow_frame();
  }
  this.publish(ctx.camera);
 }
 // A view packet snapshots live effect values after selection; every
 // source retains its native geometry, raster policy and sampler identity.
 publish(camera){const core=this.core,input=this.scratch;let n=0,at=0,matrixAt=0;
  for(let i=0;i<this.blends.length;i++){const record=this.blends[i],o=1+n*DRAW;
   if(record.fx){const batch=record.fx.batch,end=core.byName.effects.writeViewBatch(record.fx,camera,at);input[o+D.PROGRAM]=-2;input[o+D.GEOMETRY]=record.fx.id;input[o+D.RANGE]=at;input[o+D.RANGE+1]=end===at?0:Math.min(batch.count,batch.capacity);input[o+D.UNIFORMS]=batch.capacity;at=end;
   }else if(record.model){const end=core.byName.effects.writeViewModel(record.model,camera,at,matrixAt);if(end===at)continue;input[o+D.PROGRAM]=ACTOR.KIND.VIEW_MODEL;input[o+D.GEOMETRY]=record.model.model;input[o+D.RANGE]=matrixAt;input[o+D.RANGE+1]=at;at=end;matrixAt+=16;
   }else{input[o+D.PROGRAM]=-1;input[o+D.GEOMETRY]=record.k;}
   n++;
  }
  input[0]=n;core.views();for(let i=0;i<1+n*DRAW;i++)core.viewInput[i]=input[i];
 }
}
export function viewPass(core){return new ViewPass(core);}
