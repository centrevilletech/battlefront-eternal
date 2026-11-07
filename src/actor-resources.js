// Authored model bytes are also read by muzzle, debris and collision queries.
// Rendering sees only a native source identity; no BufferGeometry/Attribute or
// material descriptor is retained in this cold asset I/O and pose-node boundary.
import * as THREE from 'three';
import {actor_source_description} from './rust-pkg/bfe_render.js';
import {EFFECT} from './rust-render/effect-layout.js';
import {ACTOR} from './rust-render/actor-layout.js';
const EMPTY_F32=new Float32Array(0),EMPTY_U32=new Uint32Array(0);
export function actorAsset(authored,vertices=EMPTY_F32,indices=EMPTY_U32){return {authored,vertices,indices,description:null};}
// Native rig construction publishes Float32 negative zero too. JSON's default
// conversion erases it; retain those exact stored bits at the cold boundary.
function authoredJson(asset){return asset.nativeAuthored??=JSON.stringify(asset.authored,(_,v)=>Object.is(v,-0)?'~negative zero~':v).replaceAll('"~negative zero~"','-0.0');}
export function actorDescription(asset){return asset.description??=JSON.parse(actor_source_description(authoredJson(asset),asset.vertices,asset.indices));}
// A look contains authored facts and six live lighting values, not defines,
// shader uniforms, sampler descriptions, side/depth/blend or version counters.
export function actorLook(kind,options={}){
  const {map=null,bumpMap=null,...authored}=options,values=new Float64Array(ACTOR.LOOK.LEN);
  values[ACTOR.LOOK.SUN]=values[ACTOR.LOOK.BRIGHTNESS]=1;
  if(kind==='shock')values[ACTOR.LOOK.INFLATE]=.012;
  return {actorAppearance:true,authored:{kind,...authored,bump:Boolean(bumpMap)},map,bumpMap,values};
}
export function lightActor(look,sun,darken,lighten,brightness){const v=look.values;v[ACTOR.LOOK.SUN]=sun;v[ACTOR.LOOK.DARKEN]=darken;v[ACTOR.LOOK.LIGHTEN]=lighten;v[ACTOR.LOOK.BRIGHTNESS]=brightness;}
// Object3D/Bone/Skeleton retain the pose boundary until the matrix owner joins
// it. They have no mesh resources. Attached bind inverses use Three's existing
// math method, exactly as they did on the former SkinnedMesh pose shell.
export function actorNode(asset,appearance,skin=false){
  const node=new THREE.Object3D();node.userData.actorAsset=asset;node.material=appearance;node.frustumCulled=true;
  const facts=actorDescription(asset),b=facts.bounds;node.boundingSphere=new THREE.Sphere(new THREE.Vector3(b[0],b[1],b[2]),b[3]);
  node.userData.bindBounds=new THREE.Box3(new THREE.Vector3(b[4],b[5],b[6]),new THREE.Vector3(b[7],b[8],b[9]));
  node.getVertexPosition=skinnedActorPoint;
  if(skin){node.isSkinnedMesh=true;node.bindMode=THREE.AttachedBindMode;node.bindMatrix=new THREE.Matrix4();node.bindMatrixInverse=new THREE.Matrix4();node.bind=bindActor;node.updateMatrixWorld=THREE.SkinnedMesh.prototype.updateMatrixWorld;}
  return node;
}
// CPU-only authored positions serve real non-draw model queries. Their order
// stays the packed file order; GPU streams and index rebasing live only in Rust.
export function actorPointCount(asset){const r=asset.authored;return r.kind==='rig'?r.positions.length/3:r.parts.reduce((n,p)=>n+p.vertexCount,0);}
export function actorPoint(asset,index,out){
  const r=asset.authored;if(r.kind==='rig')return out.fromArray(r.positions,index*3);
  const width=r.kind==='body'?9:8;
  for(const part of r.parts){if(index<part.vertexCount)return out.fromArray(asset.vertices,part.vertexOffset/4+index*width);index-=part.vertexCount;}
  return out.set(0,0,0);
}
// Cold render admission resolves one native source per asset and context.
// All subsequent actor records publish that handle and their live pose only.
export function nativeActorSource(core,asset){
  if(asset.nativeSource!==undefined)return {id:asset.nativeSource,facts:asset.description};
  const cache=core.actorSources??=new WeakMap();let source=cache.get(asset);if(source)return source;
  const id=core.rust.actor_source(authoredJson(asset),asset.vertices,asset.indices);
  source={id:id??0xffffffff,facts:actorDescription(asset)};cache.set(asset,source);return source;
}
export function actorGeometry(core,asset,key,whole=false){return core.rust.actor_geometry(nativeActorSource(core,asset).id,key,whole);}

// Bone texture descriptors and padded dimensions are native policy. The retained
// Float32 array is pose transport shared with the existing publication owner.
export function actorSkin(core,skeleton){
 const cache=core.actorSkins??=new WeakMap();let entry=cache.get(skeleton);if(cache.has(skeleton))return entry;
 const count=skeleton.bones.length,words=core.rust.actor_skin_words(count),data=new Float32Array(words);for(let i=0;i<Math.min(words,skeleton.boneMatrices.length);i++)data[i]=skeleton.boneMatrices[i];skeleton.boneMatrices=data;skeleton.actorPoseSkin=data.subarray(0,count*16);
 const sample=core.resources.gpu.skin_image(count,data),texture=core.texture(sample),id=core.rust.actor_skin(texture,count);
 if(id===undefined){core.resources.gpu.release_skin_image(sample);cache.set(skeleton,null);return null;}skeleton.actorGpuOwner=core;entry={id,slot:core.rust.actor_skin_slot(id),sample,texture,data,words,offset:core.rust.actor_skin_offset(id),serial:NaN};cache.set(skeleton,entry);return entry;
}
// Glow colour/opacity are live appearance facts used by combat and the native
// effect-model publisher. Their geometry, program and raster policy are Rust's.
export function glowLook(map,color=0xffffff,opacity=1){return {actorGlow:true,map,color:new THREE.Color(color),opacity};}
export function glowNode(asset,look){
 const node=actorNode(asset,look),row=look.basic?EFFECT.ACTOR_BASIC:EFFECT.ACTOR_GLOW;
 const source={mesh:node,asset,owner:look,family:'actorEffect',visibility:'anchor',values:new Float64Array(row.LENGTH),nativeKind:row.ID,center:null};
 node.userData.effectModel=source;return node;
}

export function actorJoint(asset,index){const r=asset.authored;if(r.kind==='rig')return r.joints[index*4];for(const p of r.parts){if(index<p.vertexCount)return asset.vertices[p.vertexOffset/4+index*9+8];index-=p.vertexCount;}return 0;}
export function actorWeight(asset,index){return asset.authored.kind==='rig'?asset.authored.weights[index*4]:1;}
const skinVertex=new THREE.Vector3(),skinPoint=new THREE.Vector3(),skinMatrix=new THREE.Matrix4();
// Droideka's axle/muzzle and debris shape queries are real CPU consumers. This
// accessor reads authored points and the existing pose; no drawing calls it.
function skinnedActorPoint(index,out){const asset=this.userData.actorAsset;actorPoint(asset,index,out);if(!this.skeleton)return out;skinVertex.copy(out).applyMatrix4(this.bindMatrix);out.set(0,0,0);const r=asset.authored;
 for(let i=0;i<4;i++){const weight=r.kind==='rig'?r.weights[index*4+i]:i===0?1:0;if(weight===0)continue;const joint=r.kind==='rig'?r.joints[index*4+i]:actorJoint(asset,index);skinMatrix.multiplyMatrices(this.skeleton.bones[joint].matrixWorld,this.skeleton.boneInverses[joint]);skinPoint.copy(skinVertex).applyMatrix4(skinMatrix);out.addScaledVector(skinPoint,weight);}return out.applyMatrix4(this.bindMatrixInverse);
}

export function publishActorSkin(core,skeleton,serial){const skin=actorSkin(core,skeleton);if(!skin)return null;if(!core.actorSkinViews){core.addView('actorBones',Float32Array,()=>core.rust.actor_bones_ptr(),()=>core.rust.actor_bones_len());core.addView('actorSkinInput',Float64Array,()=>core.rust.actor_skin_input_ptr(),()=>core.rust.actor_skin_input_len());core.actorSkinViews=true;}core.views();if(skin.serial!==serial){core.actorBones.set(skeleton.boneMatrices,skin.offset);core.actorSkinInput[skin.slot]=serial;skin.serial=serial;}return skin;}

function bindActor(skeleton,matrix){THREE.SkinnedMesh.prototype.bind.call(this,skeleton,matrix);skeleton.actorPoseSkin??=skeleton.boneMatrices;}

export function releaseActorSkin(core,skeleton){const skin=core.actorSkins?.get(skeleton);if(!skin)return;core.rust.actor_skin_release(skin.id);core.resources.gpu.release_skin_image(skin.sample);core.actorSkins.delete(skeleton);if(skeleton.actorGpuOwner===core)skeleton.actorGpuOwner=null;}
