// Explicit scene writers publish coalesced authored facts. Rust owns matrix
// composition, qualified propagation and attached/detached bind inverses.
import * as THREE from 'three';
import {sceneNodes,transformNodes} from './scene-changes.js';
import {SceneCapacity} from './scene-layout.js';
import {TransformChange as C,TransformState as S,TransformPhase as P,TransformFlags as F,TransformRotation as RT,RotationOrder as RO} from './scene-transform-layout.js';
const rotationOrders={XYZ:RO.XYZ,YXZ:RO.YXZ,ZXY:RO.ZXY,ZYX:RO.ZYX,YZX:RO.YZX,XZY:RO.XZY};
const baseWorld=THREE.Object3D.prototype.updateMatrixWorld;
const baseParents=THREE.Object3D.prototype.updateWorldMatrix;
function updateWorld(force){const node=sceneNodes.get(this);if(node)node.owner.worlds.update(node,force);}
function updateParents(parents,children){const node=sceneNodes.get(this);if(node)node.owner.worlds.parents(node,parents,children);}
function updateLocal(){const node=sceneNodes.get(this);if(node){node.owner.worlds.dirty(node,true);node.owner.worlds.flush();this.matrixWorldNeedsUpdate=true;}}
export class SceneWorlds {
  constructor(registry){
    this.registry=registry;this.core=registry.core;this.count=0;
    this.core.addView("sceneRotation",Float64Array,()=>this.core.rust.scene_rotation_ptr(),()=>this.core.rust.scene_rotation_len());
    this.dirtyNodes=new Uint32Array(SceneCapacity.NODES);this.queued=new Uint8Array(SceneCapacity.NODES);
    for(const name of ['changes','values','phase']){
      const key='sceneTransform'+name[0].toUpperCase()+name.slice(1);
      this.core.addView(key,Float64Array,()=>this.core.rust[`scene_transform_${name}_ptr`](),()=>this.core.rust[`scene_transform_${name}_len`]());
    }
  }
  get updates(){return this.core.rust.scene_transform_updates();}
  get visits(){return this.core.rust.scene_transform_visits();}
  touch(node){if(this.queued[node.slot])return;this.queued[node.slot]=1;this.dirtyNodes[this.count++]=node.slot;}
  dirty(node,local=false){node.worldDirty=true;node.localDirty=node.localDirty||local;this.touch(node);}
  // The registry owns topology. These cold hooks retain only the insertion
  // index needed by the explicit admission protocol, without JS child walks.
  reserveChildren(){}
  topology(){}
  admit(node){
    const object=node.object;node.worldVersion=0;node.worldDirty=true;node.localDirty=true;
    node.transformStop=object.updateMatrixWorld!==baseWorld && !(object.isSkinnedMesh&&object.updateMatrixWorld===THREE.SkinnedMesh.prototype.updateMatrixWorld);
    node.transformNative=!!(object.userData.presentationRow||object.userData.poseStream);node.transformInput=new Float64Array(C.STRIDE);node.transformRows=null;node.trs=new Float64Array(10);node.trs.fill(NaN);
    node.transformInput.set(object.matrix.elements,C.LOCAL);node.transformInput.set(object.matrixWorld.elements,C.WORLD);if(object.bindMatrix)node.transformInput.set(object.bindMatrix.elements,C.BIND);
    for(const value of [object.position,object.quaternion,object.rotation,object.scale,object.matrix,object.matrixWorld,object.bindMatrix,object.layers])if(value)transformNodes.set(value,node);
    if(!node.transformStop){object.updateMatrixWorld=updateWorld;object.updateMatrix=updateLocal;}
    if(object.updateWorldMatrix===baseParents)object.updateWorldMatrix=updateParents;
    this.values(node,S.LOCAL);
    if(!node.transformNative){
      const owner=this;
      for(const [matrix,field]of [[object.matrix,S.LOCAL],[object.matrixWorld,S.WORLD],[object.bindMatrixInverse,S.BIND_INVERSE],[object.bindMatrix,S.BIND]])if(matrix){
        Object.defineProperty(matrix,'elements',{configurable:true,get(){return owner.values(node,field);}});
      }
    }
    const core=this.core,q=object.quaternion,euler=object.rotation;
    q._onChange(()=>{core.rust.scene_rotation_frame(q._x,q._y,q._z,q._w,rotationOrders[euler._order]);core.views();euler._x=core.sceneRotation[RT.X];euler._y=core.sceneRotation[RT.Y];euler._z=core.sceneRotation[RT.Z];});
    this.dirty(node,true);
  }
  // Only native output arrays back generic matrix facades. No per-frame
  // element copy or component observer is needed by the drawing path.
  values(node,field){
    const core=this.core;core.views();const values=core.sceneTransformValues;
    let rows=node.transformRows;
    if(!rows||rows.buffer!==values.buffer||rows.address!==values.byteOffset){
      rows={buffer:values.buffer,address:values.byteOffset,values:[]};
      for(const at of [S.LOCAL,S.WORLD,S.BIND_INVERSE,S.BIND])rows.values[at]=new Float64Array(values.buffer,values.byteOffset+(node.slot*S.STRIDE+at)*8,16);
      if(!node.transformBound){rows.values[S.LOCAL].set(node.transformInput.subarray(C.LOCAL,C.LOCAL+16));rows.values[S.WORLD].set(node.transformInput.subarray(C.WORLD,C.WORLD+16));rows.values[S.BIND].set(node.transformInput.subarray(C.BIND,C.BIND+16));node.transformBound=true;}
      node.transformRows=rows;
    }
    return rows.values[field];
  }
  changed(node,world=false,bind=false){
    if(node.transformStop&&!world&&!bind)return;
    if(world){node.worldVersion++;node.nativeWorld=false;}
    if(!world&&!bind&&node.object.matrixAutoUpdate){
      const o=node.object,p=o.position,q=o.quaternion,z=o.scale,t=node.trs;
      if(Object.is(t[0],p.x)&&Object.is(t[1],p.y)&&Object.is(t[2],p.z)&&Object.is(t[3],q.x)&&Object.is(t[4],q.y)&&Object.is(t[5],q.z)&&Object.is(t[6],q.w)&&Object.is(t[7],z.x)&&Object.is(t[8],z.y)&&Object.is(t[9],z.z))return;
    }
    this.dirty(node,!world&&!bind);
  }
  flush(){
    const core=this.core,registry=this.registry;
    core.views();let descriptors=0;registry.groupChanged=0;if(registry.changed)descriptors=registry.flush();const count=this.count,out=core.sceneTransformChanges;
    for(let i=0;i<count;i++){
      const node=registry.nodes[this.dirtyNodes[i]],object=node.object,row=node.transformInput;
      row[C.SLOT]=node.slot;row[C.GENERATION]=node.generation;
      row[C.FLAGS]=(object.matrixAutoUpdate?F.AUTO_LOCAL:0)|(object.matrixWorldAutoUpdate?F.AUTO_WORLD:0)|F.DIRTY
        |(node.transformPositionNative?F.NATIVE_POSITION:0)|(node.transformStop?F.STOP:0)|(node.transformNative?F.NATIVE:0)|(object.isSkinnedMesh?F.SKINNED:0)
        |(object.bindMode===THREE.DetachedBindMode?F.DETACHED:0)|(object.bindMode===THREE.AttachedBindMode?F.ATTACHED:0)|F.RETAINED;
      object.position.toArray(row,C.POSITION);object.quaternion.toArray(row,C.QUATERNION);object.scale.toArray(row,C.SCALE);
      const t=node.trs,p=object.position,q=object.quaternion,z=object.scale;t[0]=p.x;t[1]=p.y;t[2]=p.z;t[3]=q.x;t[4]=q.y;t[5]=q.z;t[6]=q.w;t[7]=z.x;t[8]=z.y;t[9]=z.z;
      out.set(row,i*C.STRIDE);this.queued[node.slot]=0;node.localDirty=node.worldDirty=false;
    }
    this.count=0;const worlds=registry.worldChanged;registry.lastWorldChanged=worlds;
    if(descriptors||registry.groupChanged||worlds||count)core.rust.scene_transform_apply_frame(descriptors,registry.groupChanged,worlds,count);registry.points.flush();
    for(let i=0;i<worlds;i++)registry.poseQueued[registry.poseDirty[i]]=0;
    registry.worldChanged=0;if(descriptors||worlds||count)registry.lastValid=false;return count;
  }
  // Custom native roots keep their own lazy placement; generic propagation
  // asks the native owner once and stops at those boundaries.
  update(node,force=false,children=true){
    if(node.transformStop&&node.object.updateMatrixWorld!==updateWorld){node.object.updateMatrixWorld(force);return;}
    this.flush();const core=this.core;core.views();const phase=core.sceneTransformPhase;
    phase[P.ROOT]=node.slot;phase[P.GENERATION]=node.generation;phase[P.FORCE]=+force;phase[P.CHILDREN]=+children;
    if(core.rust.scene_transform_frame())this.registry.lastValid=false;
  }
  // Named audio, muzzle/target and UI getters may request current ancestors.
  // Their native-root lazy calls do not recompose a matrix through Three.
  parents(node,parents,children){
    if(parents&&node.parent)this.parents(node.parent,true,false);
    if(node.object.updateWorldMatrix!==updateParents){node.object.updateWorldMatrix(parents,children);return;}
    this.update(node,false,children);
  }
}
