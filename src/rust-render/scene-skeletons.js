import {skinOwners} from "./skin-joins.js";
// Utility rigs without a native pose source declare their bone identities once.
// Their current skins are solved from retained native worlds when selected.
const published=()=>{};
export function admitSkeleton(registry,node){
  const object=node.object,skeleton=object.skeleton;
  if(skeleton)skinOwners.set(skeleton,object);
  if(!skeleton||object.userData.presentationRow||object.userData.poseStream||node.skinAdmitted===skeleton)return;
  node.skinAdmitted=skeleton;
  const controls=new Uint32Array(skeleton.bones.length),bones=new Uint32Array(skeleton.bones.length),generations=new Uint32Array(bones.length),inverses=new Float64Array(bones.length*16);
  for(let i=0;i<bones.length;i++){
    const bone=registry.admitIfNew(skeleton.bones[i]);bones[i]=bone?.slot??0xffffffff;generations[i]=bone?.generation??0;
    inverses.set(skeleton.boneInverses[i].elements,i*16);
  }
  const crowd=registry.core.graphics.crowdShadows;
  if(crowd?.mesh===object&&crowd.skeleton===skeleton){
    for(const slot of crowd.slots){
      for(let i=0;i<slot.boneCount;i++){inverses.set(slot.actor.skeleton.boneInverses[i].elements,(slot.boneBase+i)*16);controls[slot.boneBase+i]=slot.span+1;}
      for(const weapon of slot.weapons){const at=weapon.entry*16;inverses.fill(0,at,at+16);for(let i=0;i<4;i++)inverses[at+i*5]=1;controls[weapon.entry]=weapon.span+1;}
    }
  }
  registry.worlds.flush();
  if(!registry.core.rust.scene_skeleton_register(node.slot,node.generation,bones,generations,inverses,controls))registry.clipped++;
  skeleton.update=published;
}
