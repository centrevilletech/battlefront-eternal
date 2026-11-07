// Actor pose nodes carry no geometry/material shells. Rust retains source,
// appearance, group and sampler descriptors; this publisher supplies matrices,
// skins, per-object lighting/heat and the existing scene ordering identities.
import * as THREE from 'three';
import {isActorMaterial,claimActor,nativeActorRecord} from './actors.js';
import {Family,Mode,S} from './scene.js';
import {ACTOR} from './actor-layout.js';
// Existing instance transport. Immutable descriptor slots are expanded by Rust;
// only pose, lighting, lamp and ordering facts are written by this publisher.
const R={KIND:0,PROGRAM:1,GEOMETRY:2,BONES:8,BONE_AT:9,SPHERE:10,WORLD:20,BIND_INVERSE:36,LOOK:52,BIND:60,EXTRA:76,BLEND:96};
const NO_HEAT={heat:0,base:0,scale:1,soot:0,tear:new THREE.Vector4(0,0,0,.3),dissolve:0,sun:1,darken:0,lighten:0,brightness:1};
export function writeActor(core,record,ctx){
 if(ctx.records>=256){core.skip(record.node,record.appearance,'actor instance capacity');return;}
 const node=record.node,skeleton=node.skeleton;let boneAt=0,count=0;
 if(skeleton){count=skeleton.bones.length;boneAt=ctx.skeletons.get(skeleton);if(boneAt===undefined){if(ctx.bones+count>core.bones.length/16){core.skip(node,record.appearance,'actor bone capacity');return;}skeleton.update();boneAt=ctx.bones;ctx.skeletons.set(skeleton,boneAt);ctx.bones+=count;core.bones.set(skeleton.actorPoseSkin||skeleton.boneMatrices,boneAt*16);}}
 const input=core.input,at=core.recordsAt+ctx.records*core.stride;
 input[at+R.PROGRAM]=record.handle;input[at+R.GEOMETRY]=ACTOR.KIND.RETAINED;input[at+R.BONES]=count;input[at+R.BONE_AT]=boneAt;
 input.set(node.matrixWorld.elements,at+R.WORLD);
 if(skeleton){input.set(node.bindMatrix.elements,at+R.BIND);input.set(node.bindMatrixInverse.elements,at+R.BIND_INVERSE);}
 const sphere=node.boundingSphere;input[at+R.SPHERE]=sphere.center.x;input[at+R.SPHERE+1]=sphere.center.y;input[at+R.SPHERE+2]=sphere.center.z;input[at+R.SPHERE+3]=sphere.radius;
 const look=record.look.values,heat=record.heated?(node.userData.heated||NO_HEAT):null;
 if(heat){input[at+R.LOOK]=heat.sun;input[at+R.LOOK+1]=heat.darken;input[at+R.LOOK+2]=heat.lighten;input[at+R.LOOK+3]=heat.brightness;}else for(let i=0;i<4;i++)input[at+R.LOOK+i]=look[i];
 const e=at+R.EXTRA;for(let i=4;i<13;i++)input[e+i]=0;
 if(heat){input[e+4]=heat.heat;input[e+5]=heat.base||0;input[e+6]=heat.scale||1;input[e+7]=heat.soot;input[e+8]=heat.tear.x;input[e+9]=heat.tear.y;input[e+10]=heat.tear.z;input[e+11]=heat.tear.w;input[e+12]=heat.dissolve;}
 if(record.look.authored.kind==='shock'){const a=ACTOR.LOOK;input[e+4]=look[a.SHOCK_TIME];input[e+5]=look[a.INTENSITY];input[e+6]=look[a.SEED];input[e+7]=look[a.INFLATE];for(let i=0;i<3;i++)input[e+8+i]=look[a.COLOR+i];}
 const lamps=record.lamps?core.lamps(node,record.view):null;input[e+13]=lamps?lamps.count:0;input[e+14]=+!!node.receiveShadow;input[e+15]=0;for(let i=0;i<4;i++)input[e+16+i]=lamps?lamps.indices[i]:-1;
 if(record.blended){input[at+R.BLEND+1]=record.groupOrder||0;input[at+R.BLEND+2]=node.renderOrder;input[at+R.BLEND+3]=node.id;}
 record.k=ctx.records++;return record;
}
class InstancePass {
 constructor(core){this.core=core;this.name='instances';this.phase='near';this.records=[];this.byNode=new WeakMap();const weak=new WeakRef(this);core.graphics.settings?.onChange?.(key=>{const owner=weak.deref();if(owner&&(key==='shadows'||key==='lampLighting'))owner.settingsDirty=true;});}
 // Native descriptors replace material/geometry shells at cold admission.
 // The scene id retains membership; no record array is scanned per view.
 admit(object){
  if(!object.userData.actorAsset||!isActorMaterial(object.material)||object.userData.actorWhole||object.userData.shadowOnly)return null;
  const retained=this.core.registry.admitIfNew(object),row=object.userData.presentationRow;
  const order=row?(row.actor.root===this.core.graphics.playerRoot?1e9:row.id*10000)+(object.isSkinnedMesh?0:retained.slot+1):2000000000+retained.sibling;
  let record=this.byNode.get(object);
  if(record?.appearance===object.material){if(retained.instanceOrder!==order){retained.instanceOrder=order;this.core.registry.touch(retained);}return record;}
  record=claimActor(this.core,object,()=>nativeActorRecord(this.core,object));if(!record)return null;
  // Appearance replacement keeps one retained row per scene identity.
  // Paused swaps cannot append another live list entry for the same anchor.
  const previous=this.byNode.get(object);if(previous){const at=this.records.indexOf(previous);if(at>=0)this.records[at]=record;else this.records.push(record);}else this.records.push(record);
  this.byNode.set(object,record);
  // The native descriptor supplies blending; no material shell defines it.
  // Capture the old body/part order while the presentation id is available.
  const node=this.core.registry.bind(object,Family.INSTANCE,record,record.handle);
  if(node){node.blended=record.blended;node.instanceOrder=order;this.core.registry.touch(node);}
  return record;
 }
 draw(ctx){const core=this.core;if(this.settingsDirty){core.rust.actor_settings(core.graphics.csm?.lights.length||0,!!core.graphics.settings.get('lampLighting'));this.settingsDirty=false;core.buffer=null;core.views();}
  // A view consumes the selected prefix only. Detached retained records
  // remain cold until their qualified scene identity is attached again.
  const registry=core.registry,count=registry.select(core.graphics.scene,ctx.camera,Family.INSTANCE,Mode.INSTANCE);
  for(let i=0;i<count;i++){core.views();const at=i*S.STRIDE,node=registry.nodes[core.sceneOutput[at+S.SLOT]],record=node.records[Family.INSTANCE];if(record){record.groupOrder=core.sceneOutput[at+S.GROUP_ORDER];writeActor(core,record,ctx);}}
 }
}
export function instancesPass(core){return new InstancePass(core);}
