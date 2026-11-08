// Native displayed poses remain authoritative for named non-draw queries.
// PageAudio removes the audio facade; unit muzzle and vehicle firing owners
// remove muzzle/prediction reads; native target queries remove targeting.
// UnitCollision removes UI hit geometry, and Destruction removes limb reads.
import {UnitNode as N} from './layout.js';
function scalar(units,id,target,key,field){
  let value=target[key],written=false,version=0;
  Object.defineProperty(target,key,{configurable:true,enumerable:true,
    get(){
      units.refresh();const at=id*N.STRIDE,current=units.nodes[at+N.LOCAL_VERSION];
      if(!current||written&&version===current)return value;
      written=false;return units.nodes[at+field];
    },
    set(next){units.refresh();value=next;version=units.nodes[id*N.STRIDE+N.LOCAL_VERSION];written=true;}
  });
}
function matrix(units,id,target,field,version){
  const initial=target.elements;let buffer=null,address=-1,view=null;
  Object.defineProperty(target,'elements',{configurable:true,get(){
    units.refresh();const values=units.nodes,at=id*N.STRIDE;
    if(!values[at+version])return initial;
    const next=values.byteOffset+(at+field)*8;
    if(buffer!==values.buffer||address!==next){buffer=values.buffer;address=next;view=new Float64Array(buffer,address,16);}
    return view;
  }});
}
export function bindPoseReadback(units,row,object,id,index){
  matrix(units,id,object.matrix,N.MATRIX,N.LOCAL_VERSION);
  matrix(units,id,object.matrixWorld,N.WORLD,N.WORLD_VERSION);
  // Root TRS is an explicit gameplay/camera publication. Bone scalar reads
  // are rare query inputs; drawing does not load these component properties.
  if(index!==0){
    for(let i=0;i<3;i++){scalar(units,id,object.position,['x','y','z'][i],N.POSITION+i);scalar(units,id,object.scale,['x','y','z'][i],N.SCALE+i);}
    for(let i=0;i<4;i++)scalar(units,id,object.quaternion,['_x','_y','_z','_w'][i],N.QUATERNION+i);
  }
}
export function bindSkinReadback(units,row){
  const body=row.actor.body,skin=row.actor.skeleton,initial=skin.boneMatrices;
  const bodyId=body.userData.presentationNode;
  Object.defineProperty(skin,'boneMatrices',{configurable:true,get(){units.refresh();return units.nodes[bodyId*N.STRIDE+N.WORLD_VERSION]?row.skin:initial;}});
  Object.defineProperty(skin,'actorPoseSkin',{configurable:true,get(){units.refresh();return units.nodes[bodyId*N.STRIDE+N.WORLD_VERSION]?row.skin:initial;}});
  for(const mesh of [body,row.actor.shadow])if(mesh){
    const initial=mesh.bindMatrixInverse.elements;
    Object.defineProperty(mesh.bindMatrixInverse,'elements',{configurable:true,get(){units.refresh();return units.nodes[bodyId*N.STRIDE+N.WORLD_VERSION]?row.bindInverse:initial;}});
  }
}
