// Native hull/prop packets cross memories as retained typed views. Drawing
// uses qualified scene references rather than a Three matrix publication.
// PageAudio removes audio readbacks; mounted muzzle/firing query owners remove
// muzzle/prediction reads; UnitCollision removes UI hit-geometry readbacks.
import {sceneNodes} from './scene-changes.js';
import {ScenePoseRef as P} from './scene-layout.js';
import {PoseTreeHeader as H,PoseTreeKind as K,PoseTreeNode as T,PoseVehicleNode as V} from './pose-layout.js';
const published=()=>{};
export class TreePoseOwner {
  constructor(registry,source){
    this.registry=registry;this.source=source;this.id=registry.poseOwners.length+1;this.models=0;this.nodes=0;this.skins=0;
    if(!registry.treeViews){
      const core=registry.core;
      for(const name of ['input','info'])core.addView('sceneTree'+name[0].toUpperCase()+name.slice(1),Float64Array,()=>core.rust[`scene_tree_${name}_ptr`](),()=>core.rust[`scene_tree_${name}_len`]());
      core.addView('scenePoseSkins',Float32Array,()=>core.rust.scene_pose_skins_ptr(),()=>core.rust.scene_pose_skins_len());registry.treeViews=true;
    }
  }
  model(packet){
    const registry=this.registry,core=registry.core,objects=packet.objects,id=this.models++,start=this.nodes,skinStart=this.skins;
    this.nodes+=objects.length;packet.owner=this;packet.id=id;packet.start=start;
    const skins=new WeakMap(),rows=[];
    for(let i=0;i<objects.length;i++){
      const skeleton=objects[i].skeleton;if(!skeleton)continue;
      let skin=skins.get(skeleton);if(!skin){skin={start:this.skins,count:skeleton.bones.length};this.skins+=skin.count;skins.set(skeleton,skin);}
      rows.push({object:i,skeleton,skin});
    }
    const count=this.skins-skinStart;
    const valid=core.rust.scene_pose_register(this.id,1,id,1,start,objects.length,skinStart,count);
    core.rust.scene_tree_reserve(packet.words);core.buffer=null;core.views();
    for(let i=0;i<objects.length;i++){
      const object=objects[i],node=registry.admitIfNew(object);if(!node)continue;
      node.transformNative=node.transformStop=true;
      const pose=node.pose??=new Float64Array(P.STRIDE);pose[P.OWNER]=this.id;pose[P.ID]=id;pose[P.GENERATION]=valid?1:0;
      pose[P.NODE_START]=start+i;pose[P.NODE_COUNT]=1;pose[P.SKIN_START]=0;pose[P.SKIN_COUNT]=0;
      if(object.skeleton){const skin=skins.get(object.skeleton);pose[P.SKIN_START]=skin.start;pose[P.SKIN_COUNT]=skin.count;}
      registry.touch(node);registry.worlds.touch(node);
    }
    for(let i=0;i<rows.length;i++){
      const entry=rows[i],skeleton=entry.skeleton,bones=new Uint32Array(skeleton.bones.length),inverse=new Float64Array(bones.length*16);
      for(let j=0;j<bones.length;j++){const index=objects.indexOf(skeleton.bones[j]);bones[j]=index<0?0xffffffff:start+index;inverse.set(skeleton.boneInverses[j].elements,j*16);}
      core.rust.scene_pose_skin_register(this.id,id,1,entry.skin.start,start+entry.object,packet.skinned.indexOf(objects[entry.object]),bones,inverse);
      const offset=core.rust.scene_pose_skin_offset(this.id,id,1,entry.skin.start),initial=skeleton.boneMatrices;
      let buffer=null,address=-1,view=null;
      const read=()=>{
        core.views();const values=core.scenePoseSkins;
        if(offset>values.length||entry.skin.count*16>values.length-offset)return initial;
        const at=values.byteOffset+offset*4;
        if(buffer!==values.buffer||address!==at){buffer=values.buffer;address=at;view=new Float32Array(buffer,address,entry.skin.count*16);}
        return view;
      };
      Object.defineProperty(skeleton,'boneMatrices',{configurable:true,get:read});Object.defineProperty(skeleton,'actorPoseSkin',{configurable:true,get:read});skeleton.update=published;
    }
    core.buffer=null;core.views();if(packet.ready)packet.publish(packet.records,packet.base);
  }
}
export class TreePublication {
  constructor(source,objects,kind){
    this.source=source;this.objects=objects;this.kind=kind;this.skinned=[];this.owner=null;this.ready=false;this.buffer=null;this.address=-1;this.rawViews=new Map();
    this.stride=kind===K.TREE?T.STRIDE:V.STRIDE;this.world=kind===K.TREE?T.WORLD:V.WORLD;this.matrix=kind===K.TREE?T.MATRIX:V.MATRIX;
    for(let i=0;i<objects.length;i++)if(objects[i].skeleton)this.skinned.push(objects[i]);
    this.words=objects.length*this.stride+(kind===K.VEHICLE?this.skinned.length*16:0);
    this.info=new Float64Array(H.STRIDE);this.info[H.WORDS]=this.words;this.info[H.COUNT]=objects.length;this.info[H.KIND]=kind;
    for(let i=0;i<objects.length;i++){
      const object=objects[i];object.userData.poseStream=this;
      for(const [matrix,offset]of [[object.matrix,i*this.stride+this.matrix],[object.matrixWorld,i*this.stride+this.world]])this.readback(matrix,offset);
      if(object.skeleton){const offset=kind===K.TREE?i*this.stride+T.BIND_INVERSE:objects.length*this.stride+this.skinned.indexOf(object)*16;this.readback(object.bindMatrixInverse,offset);}
    }
  }
  // Readback arrays are rebuilt only after cold source-memory growth or a
  // different retained hull slot. Ordinary phases never copy a matrix to Three.
  readback(matrix,offset){
    const initial=matrix.elements,packet=this,views=new Map();let buffer=null,address=-1;
    Object.defineProperty(matrix,'elements',{configurable:true,get(){
      if(!packet.ready)return initial;
      packet.source?.refresh?.();const values=packet.kind===K.TREE?packet.source?.nodes||packet.records:packet.source?.streamOut||packet.records;
      if(buffer!==values.buffer||address!==values.byteOffset){buffer=values.buffer;address=values.byteOffset;views.clear();}
      let view=views.get(packet.base);if(!view){view=new Float64Array(buffer,address+(packet.base+offset)*8,16);views.set(packet.base,view);}
      return view;
    }});
  }
  publish(records,base){
    this.records=records;this.base=base;this.ready=true;
    if(!this.owner){const node=sceneNodes.get(this.objects[0]);if(!node)return;node.owner.treePoseOwner(this.source||this).model(this);}
    const owner=this.owner,core=owner.registry.core;core.views();
    if(this.buffer!==records.buffer||this.address!==records.byteOffset){this.buffer=records.buffer;this.address=records.byteOffset;this.rawViews.clear();}
    let raw=this.rawViews.get(base);if(!raw){raw=new Float64Array(records.buffer,records.byteOffset+base*8,this.words);this.rawViews.set(base,raw);}
    this.info[H.OWNER]=owner.id;this.info[H.MODEL]=this.id;this.info[H.GENERATION]=1;core.sceneTreeInfo.set(this.info);
    core.sceneTreeInput.set(raw);
    if(core.rust.scene_tree_frame())owner.registry.lastValid=false;
  }
}
