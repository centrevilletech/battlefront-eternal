// Actor pose nodes carry no geometry/material shells. Rust retains source,
// appearance, group and sampler descriptors; this publisher supplies matrices,
// skins, per-object lighting/heat and the existing scene ordering identities.
import * as THREE from 'three';
import {isActorMaterial,claimActor,nativeActorRecord} from './actors.js';
import {ACTOR} from './actor-layout.js';
export function compileFor(renderer, entries, camera, scene) {
  const each = (fn) => { for (const [mesh] of entries) fn(mesh); };
  const held = entries.map(([mesh, materials]) => { const own = mesh.material; mesh.material = materials; return own; });
  try {
    renderer.compile({ traverse: each, traverseVisible: each }, camera, scene);
  } finally {
    entries.forEach(([mesh], i) => { mesh.material = held[i]; });
  }
}

// Whether three has no program for a material (or for one of a mesh's) yet.
export function unbuilt(renderer, material) {
  return [].concat(material).some(m => {
    if(!m || isActorMaterial(m) || m.userData?.bfeFamily)return false;
    const p=renderer.properties.get(m);return !p.currentProgram || p.__version!==undefined && p.__version!==m.version;
  });
}


// Existing instance transport. Immutable descriptor slots are expanded by Rust;
// only pose, lighting, lamp and ordering facts are written by this publisher.
const R={KIND:0,PROGRAM:1,GEOMETRY:2,BONES:8,BONE_AT:9,SPHERE:10,WORLD:20,BIND_INVERSE:36,LOOK:52,BIND:60,EXTRA:76,BLEND:96};
const NO_HEAT={heat:0,base:0,scale:1,soot:0,tear:new THREE.Vector4(0,0,0,.3),dissolve:0,sun:1,darken:0,lighten:0,brightness:1};
function groupOrder(node){for(let p=node.parent;p;p=p.parent)if(p.isGroup)return p.renderOrder;return 0;}
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
 if(record.blended){input[at+R.BLEND+1]=groupOrder(node);input[at+R.BLEND+2]=node.renderOrder;input[at+R.BLEND+3]=node.id;}
 record.k=ctx.records++;return record;
}
class InstancePass {
 constructor(core){this.core=core;this.name='instances';this.phase='near';this.records=[];this.byNode=new WeakMap();this.screen=new THREE.Matrix4();this.frustum=new THREE.Frustum();const weak=new WeakRef(this);core.graphics.settings?.onChange?.(key=>{const owner=weak.deref();if(owner&&(key==='shadows'||key==='lampLighting'))owner.settingsDirty=true;});this.collect=node=>{if(node.userData.actorAsset&&isActorMaterial(node.material)&&!node.userData.actorWhole&&!node.userData.shadowOnly)this.admit(node);};}
 admit(node){let record=this.byNode.get(node);if(record?.appearance===node.material){if(!this.records.includes(record))this.records.push(record);return record;}record=claimActor(this.core,node,()=>nativeActorRecord(this.core,node));if(!record)return null;const old=this.byNode.get(node);if(old){const i=this.records.indexOf(old);if(i>=0)this.records[i]=record;else this.records.push(record);}else this.records.push(record);this.byNode.set(node,record);return record;}
 noteAdded(root){root.traverse(this.collect);}
 noteRemoved(root){for(let i=this.records.length-1;i>=0;i--){let node=this.records[i].node;while(node&&node!==root)node=node.parent;if(node===root)this.records.splice(i,1);}}
 beforeSort(renderer,scene,camera,target,added){const g=this.core.graphics;if(scene!==g.scene)return;if(added)for(const root of added)root.traverse(this.collect);if(!this.ready){scene.traverse(this.collect);this.ready=true;}}
 draw(ctx){if(this.settingsDirty){this.core.rust.actor_settings(this.core.graphics.csm?.lights.length||0,!!this.core.graphics.settings.get('lampLighting'));this.settingsDirty=false;this.core.buffer=null;this.core.views();}const scene=this.core.graphics.scene,camera=ctx.camera;this.screen.multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse);this.frustum.setFromProjectionMatrix(this.screen);
  for(let i=0;i<this.records.length;i++){let record=this.records[i];const node=record.node;if(node.material!==record.appearance){record=this.admit(node);if(!record)continue;}
   if(!node.visible||!node.layers.test(camera.layers))continue;let root=node;while(root.parent&&root.visible)root=root.parent;if(!root.visible||root!==scene)continue;
   if(node.frustumCulled&&!this.frustum.intersectsObject(node))continue;writeActor(this.core,record,ctx);
  }
 }
 // Descriptor ownership replaces hidden-material and temporary-layer mutation.
 // The near validator does not treat a pose-only actor node as a Three mesh.
 reveal(){}
}
export function instancesPass(core){return new InstancePass(core);}
