// Graphics.updateVisible's existing matrix producer retains dirty subtrees.
// Matrix ownership stays with its current presentation owner at custom roots;
// this cache removes repeated work without changing placement or view policy.
import * as THREE from 'three';
import {sceneNodes,transformNodes} from './scene-changes.js';
// Shared entry points stay distinguishable from presentation-owned roots.
const baseWorld=THREE.Object3D.prototype.updateMatrixWorld;
const baseParents=THREE.Object3D.prototype.updateWorldMatrix;
// Ordinary fields remain fast; only matrix method entry points change.
function updateWorld(force){const node=sceneNodes.get(this);if(node)node.owner.worlds.update(node,force);}
function updateParents(parents,children){const node=sceneNodes.get(this);if(node)node.owner.worlds.parents(node,parents,children);}
export class SceneWorlds {
  // Admission owns scratch; paused bursts only coalesce existing bits.
  constructor(registry,publish){this.registry=registry;this.publish=publish;this.updates=0;this.visits=0;}
  admit(node) {
    const object=node.object;
    node.worldDirty=true;node.subtreeDirty=true;node.localDirty=true;node.worldVersion=0;node.parentWorldVersion=-1;node.childrenWorldVersion=-1;
    // Sibling ordinals follow insertion order, including cold reparenting.
    node.childBits=new Uint32Array(1);node.childIndex=node.parent?node.parent.object.children.indexOf(object):0;
    // Snapshot f64 TRS so unchanged writer calls never compose again.
    node.trs=new Float64Array(10);node.trs.fill(NaN);
    // Binding receivers once avoids component getters on bones and vectors.
    for(const value of [object.position,object.quaternion,object.rotation,object.scale,object.matrix,object.matrixWorld,object.bindMatrix,object.layers])if(value)transformNodes.set(value,node);
    if(object.updateMatrixWorld===baseWorld||object.isSkinnedMesh&&object.updateMatrixWorld===THREE.SkinnedMesh.prototype.updateMatrixWorld)object.updateMatrixWorld=updateWorld;
    if(object.updateWorldMatrix===baseParents)object.updateWorldMatrix=updateParents;
    if(node.parent)this.reserveChildren(node.parent);
    this.dirty(node,true);
  }
  // A hidden dirty descendant remains pending. Visibility and reparent writers
  // mark the ancestor path again when that subtree can next be reached.
  dirty(node,local=false) {
    node.worldDirty=true;if(local)node.localDirty=true;
    let current=node;
    for(let steps=0;current&&steps<this.registry.nodes.length;steps++){
      current.subtreeDirty=true;
      const parent=current.parent;if(!parent)break;
      if(current.childIndex>=0&&(current.childIndex>>>5)<parent.childBits.length)parent.childBits[current.childIndex>>>5]|=1<<(current.childIndex&31);
      if(parent.subtreeDirty)break;
      current=parent;
    }
  }
  // Child admission/removal updates these words cold. Ordinary dirty marking
  // only sets a bit; it cannot allocate even during a long paused burst.
  reserveChildren(node) {
    const words=Math.max(1,Math.ceil(node.object.children.length/32));
    if(node.childBits.length<words){const next=new Uint32Array(words);next.set(node.childBits);node.childBits=next;}
  }
  // Removing a sibling shifts ordinals only at this topology mutation.
  topology(node) {
    this.reserveChildren(node);node.childBits.fill(0);
    const children=node.object.children;
    for(let i=0;i<children.length;i++){
      const child=sceneNodes.get(children[i]);if(!child)continue;
      child.childIndex=i;
      if(child.worldDirty||child.subtreeDirty)node.childBits[i>>>5]|=1<<(i&31);
    }
  }

  changed(node,world=false,bind=false) {
    const object=node.object;
    // Bind-only edits invalidate the inverse while leaving local TRS intact.
    if(bind){this.dirty(node);return;}
    // Late depth-copy and motion producers publish explicit worlds directly.
    if(world){this.publish(object,object.matrixWorld.elements,0);node.worldVersion++;return;}
    // Native unit/hull/prop publishers already send changed drawable worlds.
    // A gameplay root write must not restart their pose or traverse their rig.
    if(object.updateMatrixWorld!==updateWorld)return;
    const p=object.position,q=object.quaternion,s=object.scale,t=node.trs;
    if(Object.is(t[0],p.x)&&Object.is(t[1],p.y)&&Object.is(t[2],p.z)
      &&Object.is(t[3],q.x)&&Object.is(t[4],q.y)&&Object.is(t[5],q.z)&&Object.is(t[6],q.w)
      &&Object.is(t[7],s.x)&&Object.is(t[8],s.y)&&Object.is(t[9],s.z)
      &&object.matrixAutoUpdate)return;
    this.dirty(node,true);
  }
  // Matrix4 keeps its existing f64 arithmetic and evaluation order.
  remember(node) {
    const object=node.object,p=object.position,q=object.quaternion,s=object.scale,t=node.trs;
    t[0]=p.x;t[1]=p.y;t[2]=p.z;t[3]=q.x;t[4]=q.y;t[5]=q.z;t[6]=q.w;t[7]=s.x;t[8]=s.y;t[9]=s.z;
  }
  // The successful matrix version, rather than a camera count, controls
  // propagation. A second request with no writer between performs no walk.
  update(node,force=false,children=true) {
    // Parent-only getters can leave descendants at an earlier world version.
    if(node.parent&&node.parentWorldVersion!==node.parent.worldVersion)force=true;
    if(!force&&!node.worldDirty&&(!children||!node.subtreeDirty))return;
    this.visits++;
    const object=node.object;
    // Custom roots preserve native lazy placement without recursing here.
    if(object.updateMatrixWorld!==updateWorld){object.updateMatrixWorld(force);node.subtreeDirty=node.worldDirty=node.localDirty=false;node.parentWorldVersion=node.parent?.worldVersion??-1;node.childrenWorldVersion=node.worldVersion;return;}
    const update=force||node.worldDirty;
    if(node.localDirty&&object.matrixAutoUpdate){object.updateMatrix();this.remember(node);}
    node.localDirty=false;
    if(update){
      if(object.matrixWorldAutoUpdate===true){
        if(object.parent===null)object.matrixWorld.copy(object.matrix);
        else object.matrixWorld.multiplyMatrices(object.parent.matrixWorld,object.matrix);
      }
      object.matrixWorldNeedsUpdate=false;node.worldDirty=false;node.worldVersion++;this.updates++;
      this.publish(object,object.matrixWorld.elements,0);
      // Attached/detached inverses retain the updateVisible placement boundary.
      if(object.isSkinnedMesh){
        if(object.bindMode===THREE.AttachedBindMode)object.bindMatrixInverse.copy(object.matrixWorld).invert();
        else if(object.bindMode===THREE.DetachedBindMode)object.bindMatrixInverse.copy(object.bindMatrix).invert();
      }
    }
    node.parentWorldVersion=node.parent?.worldVersion??-1;
    // A parent-only reader leaves child propagation pending for preparation.
    if(!children)return;
    node.subtreeDirty=false;
    const descendants=object.children,bits=node.childBits;
    // A changed ancestor forces children whose own local pose stayed static.
    if(update||node.childrenWorldVersion!==node.worldVersion){
      bits.fill(0);
      for(let i=0;i<descendants.length;i++){
        const child=descendants[i],retained=sceneNodes.get(child);if(!retained)continue;
        if(child.visible)this.update(retained,true);
        else{retained.worldDirty=true;child.matrixWorldNeedsUpdate=true;}
      }
    // Dirty words visit siblings in authored order without sorting or arrays.
    }else for(let word=0;word<bits.length;word++){
      // Clear in place so a later writer can queue this node again.
      let pending=bits[word];bits[word]=0;
      while(pending){
        const bit=31-Math.clz32(pending&-pending),index=word*32+bit;pending&=pending-1;
        const child=descendants[index],retained=sceneNodes.get(child);
        if(retained&&child.visible)this.update(retained,false);
      }
    }
    node.childrenWorldVersion=node.worldVersion;
  }
  // Named gameplay/audio getters ask for current ancestors, as Object3D's
  // updateWorldMatrix did. They share the same dirty versions as drawing.
  parents(node,parents,children) {
    if(parents&&node.parent)this.parents(node.parent,true,false);
    if(node.object.updateWorldMatrix!==updateParents){node.object.updateWorldMatrix(parents,children);return;}
    this.update(node,false,children);
  }
}
