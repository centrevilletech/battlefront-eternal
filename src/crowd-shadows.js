import * as THREE from 'three';
import {ACTOR} from './rust-render/actor-layout.js';
import {actorNode,actorLook,nativeActorSource,releaseActorSkin} from './actor-resources.js';
// CrowdShadows retains unit/hand pose identities until the matrix owner joins
// them. Native sources own geometry growth, transformed weapon vertices, skin
// attributes, index ranges, dirty prefixes and GPU restoration descriptors.
const ZERO=new THREE.Matrix4().set(0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0),IDENTITY=new THREE.Matrix4();
export class CrowdShadows {
 constructor(graphics){this.graphics=graphics;this.slots=[];this.bones=[];this.inverses=[];this.mesh=null;this.skeleton=null;this.draws=0;this.core=null;}
 begin(){if(this.core)return;const core=this.graphics.rust,id=core.rust.actor_source_crowd();this.core=core;
  const asset={nativeSource:id,description:JSON.parse(core.rust.actor_source_facts(id))};
  this.mesh=actorNode(asset,actorLook('dynamic'),true);this.mesh.name='crowd-shadows';this.mesh.userData.shadowOnly=true;this.mesh.frustumCulled=false;this.mesh.layers.set(this.graphics.hiddenLayer??1);this.graphics.scene.add(this.mesh);this.graphics.caster(this.mesh);
  core.addView('actorCrowdInput',Float64Array,()=>core.rust.actor_crowd_input_ptr(),()=>core.rust.actor_crowd_input_len());
 }
 // One cold native admission owns all of this unit's body/weapon streams.
 // Source ids and hand-local matrices are semantic facts, not GPU descriptors.
 add(unit){const actor=unit.actor;if(!actor?.body?.userData.actorAsset||!actor.skeleton)return null;const core=this.graphics.rust,body=nativeActorSource(core,actor.body.userData.actorAsset),weapons=[],sources=[],entries=[],groups=[],matrices=[];
  const boneBase=this.bones.length;let boneCount=actor.bones.length;
  for(const held of unit.combat?.held?.values()||[]){const meshes=[];for(const node of held.group.children)if(node.userData.actorAsset&&node.material?.actorAppearance&&node.material.authored.kind==='dynamic')meshes.push(node);if(!meshes.length||!actor.hand)continue;
   const entry=boneBase+boneCount++,group=weapons.length;weapons.push({group:held.group,meshes,entry,shown:false});
   for(const node of meshes){node.updateMatrix();held.group.updateMatrix();const local=new THREE.Matrix4().multiplyMatrices(held.group.matrix,node.matrix);sources.push(nativeActorSource(core,node.userData.actorAsset).id);entries.push(entry);groups.push(group);matrices.push(...local.elements);}
  }
  this.begin();const facts=core.rust.actor_source_crowd_add(body.id,boneBase,boneBase+boneCount,Uint32Array.from(sources),Uint32Array.from(entries),Uint32Array.from(groups),Float64Array.from(matrices));if(!facts.length)return null;
  const slot={unit,actor,weapons,boneBase,boneCount:actor.bones.length,shown:false,span:facts[1]};for(let i=0;i<weapons.length;i++)weapons[i].span=facts[2+i];
  for(const bone of actor.bones){this.bones.push(bone);this.inverses.push(ZERO);}for(const weapon of weapons){this.bones.push(actor.hand);this.inverses.push(ZERO);}
  const retire=node=>{if(!node)return;node.castShadow=false;this.graphics.casters.delete(node);};retire(actor.shadow);for(const w of weapons)for(const node of w.meshes)retire(node);
  this.slots.push(slot);this.rebindSkeleton();if(facts[0]){this.mesh.removeFromParent();this.graphics.scene.add(this.mesh);this.graphics.casters.delete(this.mesh);this.graphics.caster(this.mesh);}core.buffer=null;core.views();return slot;
 }
 // Bone inverses remain actual pose data. They do not allocate or describe a
 // rendering resource; the existing Skeleton math is owned by the next join.
 rebindSkeleton(){if(this.skeleton)releaseActorSkin(this.core,this.skeleton);this.skeleton=new THREE.Skeleton(this.bones,this.inverses.map(m=>m.clone()));this.mesh.bind(this.skeleton,IDENTITY);this.core.buffer=null;this.core.views();const input=this.core.actorCrowdInput;input[ACTOR.CROWD_INPUT.DIRTY]=1;for(const slot of this.slots){slot.shown=false;input[ACTOR.CROWD_INPUT.FLAGS+slot.span]=0;for(const w of slot.weapons){w.shown=false;input[ACTOR.CROWD_INPUT.FLAGS+w.span]=0;}}}
 update(){if(!this.skeleton)return;const core=this.core;core.views();const input=core.actorCrowdInput,inverses=this.skeleton.boneInverses;let drawn=0;
  for(let s=0;s<this.slots.length;s++){const slot=this.slots[s],actor=slot.actor,unit=slot.unit,shown=!!(actor.root.visible&&actor.body.visible&&unit.shadowNear&&!unit.removed);
   if(shown!==slot.shown){slot.shown=shown;input[ACTOR.CROWD_INPUT.FLAGS+slot.span]=+shown;input[ACTOR.CROWD_INPUT.DIRTY]=1;for(let i=0;i<slot.boneCount;i++)inverses[slot.boneBase+i].copy(shown?actor.skeleton.boneInverses[i]:ZERO);}
   if(shown)drawn++;
   for(let i=0;i<slot.weapons.length;i++){const w=slot.weapons[i],on=shown&&w.group.visible;if(on!==w.shown){w.shown=on;input[ACTOR.CROWD_INPUT.FLAGS+w.span]=+on;input[ACTOR.CROWD_INPUT.DIRTY]=1;inverses[w.entry].copy(on?IDENTITY:ZERO);}}
  }
  core.rust.actor_crowd_frame();this.draws=drawn;this.mesh.castShadow=drawn>0&&!!this.graphics.csm;
 }
}
