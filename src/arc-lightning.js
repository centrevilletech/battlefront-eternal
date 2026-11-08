import {sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import {actorNode,actorLook} from './actor-resources.js';
import {ACTOR} from './rust-render/actor-layout.js';
import {EFFECT} from './rust-render/effect-layout.js';
import * as THREE from 'three';
import {lighting} from './materials.js';
import {particleDepth} from './particle-depth.js';
import {presentationModule} from './presentation/module.js';
import {LightningFrame as F,LightningUnit as U,LightningNode as N,LightningSpawn as S,
 LightningBolt as B,LightningShock as O,LightningJoint as J,LightningLight as I,
 LightningCapsule as A,LightningCamera as C,LightningReaders as R,LightningCounts as H,LightningLimits as L} from './presentation/lightning-layout.js';
const STRIDE=A.STRIDE;
const EMPTY=[];
export const arcBatches=new WeakMap();
const TWITCH={bone_b_spine:.08,bone_ribcage:.1,bone_neck:.12,bone_head:.18,
 bone_l_clavicle:.1,bone_r_clavicle:.1,bone_l_upperarm:.26,bone_r_upperarm:.26,
 bone_l_forearm:.32,bone_r_forearm:.32};
const WHITE=[130,170,255,255];
function vector(a,o,p){a[o]=p.x;a[o+1]=p.y;a[o+2]=p.z;}
function bind(batch,data){batch.data=batch.arrays.interleaved=data;batch.capacity=L.CAPSULES;}
export class ArcBatch {
  // soft: faded against the scene's depth (not for the first-person view's
  // own scene, which has a depth range of its own).
  constructor(capacity = 512, { soft = true, data = null } = {}) {
    this.cameraLoaded=presentationModule();this.cameraKernel=new this.cameraLoaded.module.PageArcCamera();this.cameraAddress=this.cameraKernel.output_ptr();this.cameraBuffer=this.cameraLoaded.memory.buffer;this.cameraOutput=new Float64Array(this.cameraBuffer,this.cameraAddress,C.STRIDE);
    this.soft = soft;
    this.count = 0;
    this.external = false;
    this.capacity = 0;
    this.data = null;
    this.range = { start: 0, count: 0 };
    // ArcBatch's two depth ranges have distinct native descriptors. Only the
    // live endpoints, camera facts and scene anchor remain on the page.
    const layout=soft?EFFECT.ARC:EFFECT.ARC_VIEW;
    this.nativeKind=layout.ID;this.variant=0;this.family=soft?'arc':'arcView';
    this.texture=null;this.values=new Float64Array(layout.LENGTH);this.stride=STRIDE;
    this.values[layout.EYE_FADE]=soft?0.12:0;this.values[layout.EYE_FADE+1]=soft?0.5:0;
    this.values[layout.CLOSE_RANGE]=soft?2.5:0;this.values[layout.PIXEL_ANGLE]=0.002;this.values[layout.NEAR_DEPTH]=0.51;
    this.arrays={};Object.defineProperty(this.arrays,"interleaved",{get:()=>this.data,set:data=>{this.data=data;}});
    this.mesh = new THREE.Object3D(); this.mesh.userData.effectBatch=this;
    this.mesh.name = "arc-lightning";
    sceneProperty(this.mesh,"frustumCulled",false);
    sceneProperty(this.mesh,"renderOrder",12);
    sceneProperty(this.mesh,"visible",false);
    arcBatches.set(this.mesh, this);
    const size = new THREE.Vector2();
    this.mesh.onBeforeRender = (renderer, scene, camera) => {
      // Shown empty only to have its program built while a battle loads.
      if (!this.count) return;
      if (this.soft) particleDepth.capture(renderer, scene, camera);
      else {this.values[layout.FADE]=lighting.fade.value.x;this.values[layout.FADE+1]=lighting.fade.value.y;}
      if (camera.isPerspectiveCamera) {
        // The pixels being drawn: under an upscaler, the 3D frame's.
        const target = renderer.getRenderTarget();
        const height = target ? target.height : renderer.getDrawingBufferSize(size).y;
        this.cameraKernel.step(camera.fov,camera.zoom,camera.near,height);
        const buffer=this.cameraLoaded.memory.buffer;if(this.cameraBuffer!==buffer){this.cameraBuffer=buffer;this.cameraOutput=new Float64Array(buffer,this.cameraAddress,C.STRIDE);}
        this.values[layout.PIXEL_ANGLE]=this.cameraOutput[C.PIXEL_ANGLE];
        this.values[layout.NEAR_DEPTH]=this.cameraOutput[C.NEAR_DEPTH];
      }
    };
    if(data)bind(this,data);else this.grow(Math.max(capacity,16384));
  }

  get data(){if(this.owner)this.owner.refresh();return this._data;}
  set data(data){this._data=data;}

  grow(capacity) {
    const data = new Float32Array(capacity * STRIDE);
    if (this.data) data.set(this.data);
    this.data = data;
    this.arrays.interleaved = data;
    this.capacity = capacity;
  }

  // Room for `more` instances after those pushed so far.
  reserve(more) {
    // Capacity is admitted once; push clips any excess segments.
  }

  begin() {
    this.count = 0;
  }

  // One segment from a to b. na / nb: the joint planes' normals at its ends
  // (null at a line's end); rgb the sheath colour with its largest channel at
  // 1; hot how white its core runs (0 to 1).
  push(a, b, na, nb, rgb, brightness, core, glow, hot = 0.6) {
    if (brightness <= 0.002) return;
    if (this.count === this.capacity) return;
    const d = this.data, o = this.count++ * STRIDE;
    d[o] = a.x; d[o + 1] = a.y; d[o + 2] = a.z;
    d[o + 3] = b.x; d[o + 4] = b.y; d[o + 5] = b.z;
    if (na) { d[o + 6] = na.x; d[o + 7] = na.y; d[o + 8] = na.z; } else d[o + 6] = d[o + 7] = d[o + 8] = 0;
    if (nb) { d[o + 9] = nb.x; d[o + 10] = nb.y; d[o + 11] = nb.z; } else d[o + 9] = d[o + 10] = d[o + 11] = 0;
    d[o + 12] = rgb[0]; d[o + 13] = rgb[1]; d[o + 14] = rgb[2]; d[o + 15] = brightness;
    d[o + 16] = core; d[o + 17] = glow; d[o + 18] = hot; d[o + 19] = 0;
  }

  // keep: shown even empty (its program is built by the first draw or the
  // loading's warm-up).
  end() {
    sceneProperty(this.mesh,"visible",this.count>0);
  }
}

// Scene handles, muzzle callbacks and actor-resource identities stay on the
// page. Numeric state and both draw families share one required Rust owner.
export class ArcLightning {
 constructor(scene,{lights=null,units=null,collision=null,effects=null,viewScene=null,bubbleOf=unit=>unit?.bubble??null}={},draw=true){
  this.scene=scene;this.lights=lights;this.units=units;this.collision=collision;this.effects=effects;this.viewScene=viewScene;this.bubbleOf=bubbleOf;
  this.loaded=presentationModule();this.kernel=new this.loaded.module.PageLightning();this.buffer=null;
  this.overlays=new WeakMap();this.unitIds=new WeakMap();this.unitRows=new Array(L.UNITS).fill(null);this.unitCount=0;
  this.actorRows=new Array(L.UNITS).fill(null);this.definitions=new WeakMap();this.defs=[];
  this.handles=Array.from({length:L.BOLTS},()=>({def:null,follow:null,start:new THREE.Vector3(),end:new THREE.Vector3()}));
  this.aimUnits=Object.create(Array.prototype);for(let i=0;i<L.UNITS;i++)this.aimUnits[i]=null;this.aimUnits.length=0;this.batch=null;this.viewBatch=null;
  this.position=new THREE.Vector3();this.direction=new THREE.Vector3();this.rgb=[0,0,0];
  this.hit={distance:0,point:new THREE.Vector3(),normal:new THREE.Vector3()};this.warming=90;this.refresh();
  for(const name in this.effects?.lightning){const def=this.effects.lightning[name];if(def&&typeof def==='object')this.definition(def);}
  if(draw)this.enable();
 }
 enable(){if(this.batch)return;this.refresh();this.batch=new ArcBatch(L.CAPSULES,{data:this.capsules});this.batch.owner=this;this.scene.add(this.batch.mesh);
  if(this.viewScene){this.viewBatch=new ArcBatch(L.CAPSULES,{soft:false,data:this.viewCapsules});this.viewBatch.owner=this;this.viewBatch.mesh.name='arc-lightning-view';this.viewScene.add(this.viewBatch.mesh);}
  sceneProperty(this.batch.mesh,"visible",true);const before=this.batch.mesh.onBeforeRender;
  this.batch.mesh.onBeforeRender=(renderer,scene,camera,geometry,material,group)=>{if(this.warming>0)this.warming--;before(renderer,scene,camera,geometry,material,group);};
 }
 // Cold resource admission can grow shared memory. Rebind every borrowed
 // input and draw array together before either host or renderer reads it.
 refresh(){const b=this.loaded.memory.buffer;if(this.buffer===b)return;this.buffer=b;const k=this.kernel;
  this.frame=new Float64Array(b,k.frame_ptr(),F.STRIDE);this.facts=new Float64Array(b,k.units_ptr(),L.UNITS*U.STRIDE);
  this.nodes=new Float64Array(b,k.nodes_ptr(),k.nodes_len());this.birth=new Float64Array(b,k.spawn_ptr(),S.STRIDE);
  this.held=new Float64Array(b,k.held_ptr(),L.BOLTS*3);this.output=new Float64Array(b,k.output_ptr(),L.BOLTS*B.STRIDE);
  this.shockOutput=new Float64Array(b,k.shocks_output_ptr(),L.UNITS*O.STRIDE);this.jointOutput=new Float64Array(b,k.joints_ptr(),L.UNITS*L.JOINTS*J.STRIDE);
  this.lightOutput=new Float64Array(b,k.lights_ptr(),L.LIGHTS*I.STRIDE);this.counts=new Uint32Array(b,k.counts_ptr(),H.STRIDE);
  this.used=new Uint32Array(b,k.used_ptr(),L.UNITS);this.unitOrder=new Uint32Array(b,k.unit_order_ptr(),L.UNITS);this.aim=new Uint32Array(b,k.aim_ptr(),L.UNITS);this.readers=new Uint32Array(b,k.readers_ptr(),L.UNITS);this.follows=new Uint32Array(b,k.follows_ptr(),L.BOLTS);
  this.capsules=new Float32Array(b,k.capsules_ptr(),L.CAPSULES*A.STRIDE);this.viewCapsules=new Float32Array(b,k.view_capsules_ptr(),L.CAPSULES*A.STRIDE);
  this.positions=new Float32Array(b,k.positions_ptr(),L.VERTICES*3);this.uvs=new Float32Array(b,k.uvs_ptr(),L.VERTICES*2);this.colors=new Float32Array(b,k.colors_ptr(),L.VERTICES*4);this.indices=new Uint32Array(b,k.indices_ptr(),L.VERTICES*3);
  if(this.batch)bind(this.batch,this.capsules);if(this.viewBatch)bind(this.viewBatch,this.viewCapsules);
 }
 definition(def){let id=this.definitions.get(def);if(id!==undefined)return id;id=this.kernel.definition(JSON.stringify(def));this.definitions.set(def,id);this.defs[id]=def;this.refresh();return id;}
 // Unit ids are host leases, independent of bolt compaction. Removed facade
 // objects release their slot before another body can use it.
 unitId(unit){if(!unit)return -1;let id=this.unitIds.get(unit);if(id!==undefined)return id;
  id=this.unitRows.indexOf(null);if(id<0)return -1;this.unitRows[id]=unit;this.unitIds.set(unit,id);if(id>=this.unitCount)this.unitCount=id+1;return id;
 }
 // Only cold topology is discovered here. Local transforms are copied into
 // generated rows; Rust composes and places the bones used by shock effects.
 actor(id){const unit=this.unitRows[id],actor=unit?.actor;if(!actor?.root)return null;let row=this.actorRows[id];if(row?.actor===actor)return row;
  if(row?.overlay)sceneProperty(row.overlay,"visible",false);
  const wanted=new Set([actor.root]);for(const bone of actor.bones||EMPTY){let node=bone;while(node&&node!==actor.root){wanted.add(node);node=node.parent;}}
  const nodes=[],parents=[];actor.root.traverse(node=>{if(wanted.has(node)&&nodes.length<L.NODES_PER_UNIT){parents.push(nodes.indexOf(node.parent));nodes.push(node);}});
  const bones=[],low=[];for(const bone of actor.bones||EMPTY){const at=nodes.indexOf(bone);if(at<0||/^hp_|dummyroot|^bone_root$/i.test(bone.name))continue;bones.push(at);if(/foot|toe|calf|hand/i.test(bone.name))low.push(at);}
  const twitch=[],joints=[];for(const name in TWITCH){const bone=actor.byName?.[name],at=nodes.indexOf(bone);if(at<0)continue;twitch.push([at,TWITCH[name]]);joints.push(bone);}
  const chest=actor.byName?.bone_ribcage||(actor.bones||EMPTY).find(b=>/ribcage|spine/i.test(b.name));const index=nodes.indexOf(chest);
  const nodeStart=this.kernel.actor(id,JSON.stringify({parents,bones,low,chest:index<0?null:index,twitch}));this.buffer=null;this.refresh();
  row={actor,nodes,joints,nodeStart,overlay:null,view:null,wrist:null};this.actorRows[id]=row;return row;
 }
 // All fields are foreign observations, without distance, lifetime or shape
 // decisions. The native prepare phase selects the expensive pose readers.
 publishUnits(detailed=false){this.refresh();const x=this.facts;
  for(let id=0;id<this.unitCount;id++){const unit=this.unitRows[id];x[id*U.STRIDE+U.LISTED]=0;x[id*U.STRIDE+U.REMOVED]=unit?.removed?1:0;if(unit?.removed&&!this.used[id]){this.unitIds.delete(unit);this.unitRows[id]=null;x[id*U.STRIDE+U.ALIVE]=0;}}
  let orderCount=0;for(const unit of this.units?.all?.()||EMPTY){const id=this.unitId(unit);if(id<0)continue;if(orderCount<L.UNITS)this.unitOrder[orderCount++]=id;
   const o=id*U.STRIDE,actor=unit.actor,combat=unit.combat,weapon=combat?.arsenal?.current?.(0);
   x[o+U.LISTED]=1;x[o+U.FIRST_PERSON]=combat?.firstPerson?1:0;x[o+U.GENERATION]=unit.generation||0;x[o+U.ALIVE]=actor?.root?1:0;x[o+U.SOLDIER]=unit.soldier?1:0;x[o+U.REMOVED]=unit.removed?1:0;
   const ownView=combat?.firstPerson?combat.view:null;x[o+U.OWN]=ownView?.root?.visible&&ownView.bones?.length?1:0;
   x[o+U.COMBAT]=combat?1:0;x[o+U.DEAD]=unit.dead||combat?.dead?1:0;x[o+U.MOUNTED]=combat?.mounted?1:0;x[o+U.CHARGING]=weapon?.charging&&weapon.def?.ordnance?.LightningEffect?1:0;
   if(detailed)this.publishDetails(id);
  }
  for(let id=0;id<this.unitCount;id++)if(this.unitRows[id]&&!x[id*U.STRIDE+U.LISTED]&&!this.used[id]){this.unitIds.delete(this.unitRows[id]);this.unitRows[id]=null;x[id*U.STRIDE+U.ALIVE]=0;}
  this.frame[F.UNITS]=this.unitCount;this.frame[F.ORDER_COUNT]=orderCount;
 }
 // Idle Enhanced frames publish charging observations only. Rust requests the
 // remaining resource facts for the bodies and guns it will actually read.
 publishDetails(id,camera=null){const unit=this.unitRows[id];if(!unit)return;const o=id*U.STRIDE,x=this.facts,actor=unit.actor,combat=unit.combat,weapon=combat?.arsenal?.current?.(0),def=weapon?.def;
   if(actor?.root)vector(x,o+U.ROOT,actor.root.position);
   x[o+U.BODY_VISIBLE]=actor?.body?.visible?1:0;x[o+U.WALKER]=unit.walker?1:0;x[o+U.SEAT]=unit.seat?1:0;x[o+U.HELD]=unit.held?.frame?1:0;
   x[o+U.SHOWN]=actor?.root.visible&&actor.body?.visible!==false&&(!camera||camera.layers.test(actor.body?.layers??actor.root.layers))?1:0;
   const view=combat?.firstPerson?combat.view:null;x[o+U.OWN]=view?.root?.visible&&view.bones?.length?1:0;
   let lightningWeapon=null;for(const entry of combat?.arsenal?.weapons||EMPTY)if(entry.def?.ordnance?.LightningEffect){lightningWeapon=entry;break;}x[o+U.HAS_WEAPON]=lightningWeapon?1:0;x[o+U.CONE]=lightningWeapon?.def?.ordnance?.ConeLength??30;
   x[o+U.PRESSED]=weapon?.held||0;x[o+U.MAX_PRESSED]=weapon?.levels?.[0]?.MaxPressedTime||def?.MaxPressedTime||1;
   const bolt=this.effects?.lightning?.[def?.ordnance?.LightningEffect],color=bolt?.Color||WHITE;for(let i=0;i<4;i++)x[o+U.COLOR+i]=color[i];
   const bubble=this.bubbleOf(unit),shield=bubble?.on&&bubble.mesh?.visible?bubble:null;x[o+U.SHIELD]=shield?1:0;if(shield){vector(x,o+U.CENTER,shield.mesh.position);vector(x,o+U.RADIUS,shield.mesh.scale);}
 }
 publishOrigin(id){const unit=this.unitRows[id],o=id*U.STRIDE,combat=unit?.combat;const origin=combat?.drawnGunOrigin?.(this.position);this.refresh();this.facts[o+U.HAS_ORIGIN]=origin?1:0;if(origin)vector(this.facts,o+U.ORIGIN,origin);}
 publishPose(id,camera,flags){const unit=this.unitRows[id];if(!unit?.actor)return;const row=flags&(R.POSE|R.VIEW)?this.actor(id):this.actorRows[id];this.publishDetails(id,camera);const o=id*U.STRIDE,actor=unit.actor,combat=unit.combat;let x=this.facts;
  // Copy retained local matrices before asking unrelated gun resources to
  // refresh their lazy pose. A body-only shock never queries a muzzle.
  if(flags&R.POSE){const parent=actor.root.parent?.matrixWorld?.elements||IDENTITY;for(let i=0;i<16;i++)x[o+U.PARENT+i]=parent[i];
   if(row)for(let i=0;i<row.nodes.length;i++){const node=row.nodes[i],at=row.nodeStart+i*N.STRIDE,q=node.quaternion;this.nodes[at+N.MATRIX_AUTO_UPDATE]=node.matrixAutoUpdate?1:0;
    this.nodes[at+N.QUATERNION]=q.x;this.nodes[at+N.QUATERNION+1]=q.y;this.nodes[at+N.QUATERNION+2]=q.z;this.nodes[at+N.QUATERNION+3]=q.w;
    if(node.matrixAutoUpdate){vector(this.nodes,at+N.POSITION,node.position);vector(this.nodes,at+N.SCALE,node.scale);}
    else{const matrix=node.matrix.elements;for(let k=0;k<16;k++)this.nodes[at+N.MATRIX+k]=matrix[k];}
   }
  }
  if(flags&R.CASTER){let weapon=null;for(const entry of combat?.arsenal?.weapons||EMPTY)if(entry.def?.ordnance?.LightningEffect){weapon=entry;break;}
   const at=weapon?combat?.firePoint?.(weapon):null;this.refresh();x=this.facts;x[o+U.HAS_MUZZLE]=at?.position?1:0;if(at?.position)vector(x,o+U.MUZZLE,at.position);
  }
  if(flags&R.CURRENT){const at=combat?.firePoint?.(combat.arsenal?.current?.(0));this.refresh();x=this.facts;x[o+U.HAS_CURRENT_MUZZLE]=at?.position?1:0;if(at?.position)vector(x,o+U.CURRENT_MUZZLE,at.position);}
  if(flags&R.ORIGIN){this.publishOrigin(id);x=this.facts;}
  if(!(flags&R.VIEW))return;const view=combat?.firstPerson?combat.view:null;x[o+U.HAS_HAND]=view?.hand?1:0;x[o+U.HAS_WRIST]=0;
  if(view?.root&&row){if(row.view!==view){row.view=view;row.wrist=view.bones?.find(b=>/l_wrist|l_hand/i.test(b.name))||null;}
   if(view.hand){view.hand.getWorldPosition(this.position);vector(x,o+U.HAND,this.position);}
   (view.hand||view.root).getWorldPosition(this.position);vector(x,o+U.GRIP,this.position);
   if(row.wrist){row.wrist.getWorldPosition(this.position);vector(x,o+U.WRIST,this.position);x[o+U.HAS_WRIST]=1;}
  }
 }

 // Birth is a producer command, not a displayed per-bolt call. PrepareAim's
 // complete search is needed only for a newly admitted held bolt.
 spawn(def,start,end,charge,follow=null,classic=false,enhanced=true){const definition=this.definition(def);if(enhanced){if(follow)this.units?.prepareAim?.();this.publishUnits(true);}
  // Admission needs roots and gun origins only. Bone and muzzle readers are
  // selected by the native prepare phase once their qualified ids are known.
  if(enhanced&&follow){vector(this.birth,S.FOLLOW_AT,follow(this.position)||start);for(let id=0;id<this.unitCount;id++)if(this.unitRows[id])this.publishOrigin(id);}
  this.refresh();const x=this.birth;vector(x,S.START,start);vector(x,S.END,end);x[S.CHARGE]=charge;x[S.FOLLOW]=follow?1:0;
  x[S.FOLLOW_UNIT]=this.unitId(follow?.unit);
  const id=this.kernel.add(definition,classic,enhanced,this);this.refresh();if(id>=L.BOLTS)return -1;const handle=this.handles[id];handle.def=def;handle.follow=follow;handle.start.copy(start);handle.end.copy(end);return id;
 }
 add(bolt){return this.spawn(bolt.def,bolt.start,bolt.end,bolt.charge,bolt.follow,false,true);}
 // The browser only answers collision memory queries already constructed by
 // Rust. Reused vectors and hit storage keep these callbacks allocation-free.
 cast(x,y,z,dx,dy,dz,length){if(!this.collision)return -1;this.position.set(x,y,z);this.direction.set(dx,dy,dz);const hit=this.collision.cast(this.position,this.direction,length,this.hit);return hit?hit.distance:-1;}
 normal(axis){return axis===0?this.hit.normal.x:axis===1?this.hit.normal.y:this.hit.normal.z;}
 update(dt,camera,enhanced=true){this.refresh();const wasEnhanced=Boolean(this.counts[H.ENHANCED_ACTIVE]);this.frame[F.DT]=dt;this.frame[F.ENHANCED]=enhanced?1:0;this.frame[F.HAS_VIEW]=this.viewScene?1:0;
  vector(this.frame,F.EYE,camera.position);const q=camera.quaternion;this.frame[F.CAMERA]=q.x;this.frame[F.CAMERA+1]=q.y;this.frame[F.CAMERA+2]=q.z;this.frame[F.CAMERA+3]=q.w;this.frame[F.LAYERS]=camera.layers.mask;
  if(enhanced||wasEnhanced)this.publishUnits();this.kernel.prepare();this.refresh();
  // An ordinary array-like length keeps the reserved indexed storage. Clear
  // old references in place so the reader list creates no frame garbage.
  for(let i=0;i<this.aimUnits.length;i++)this.aimUnits[i]=null;this.aimUnits.length=0;for(let i=0;i<this.counts[H.AIM];i++){const unit=this.unitRows[this.aim[i]];if(unit)this.aimUnits[this.aimUnits.length++]=unit;}this.units?.prepareAim?.(this.aimUnits);
  for(let i=0;i<this.counts[H.FOLLOW];i++){const id=this.follows[i],handle=this.handles[id];if(handle.follow){handle.follow(handle.start);vector(this.held,id*3,handle.start);}}
  for(let id=0;id<this.unitCount;id++)if(this.readers[id])this.publishPose(id,camera,this.readers[id]);
  this.kernel.step(this);this.refresh();if(enhanced||wasEnhanced)this.apply();
 }
 // Shock material and quaternion rows are native results. The host writes
 // them onto existing scene resources without recomputing the visual effect.
 apply(){for(let id=0;id<this.unitCount;id++){const row=this.actorRows[id];if(!row)continue;const o=id*O.STRIDE,x=this.shockOutput;
   for(let i=0;i<row.joints.length;i++){const at=(id*L.JOINTS+i)*J.STRIDE;if(this.jointOutput[at+J.ON])sceneTransform(row.joints[i].quaternion.fromArray(this.jointOutput,at+J.QUATERNION));}
   if(x[o+O.ACTIVE]){const overlay=this.overlay(row.actor);row.overlay=overlay;if(overlay){sceneProperty(overlay,"visible",Boolean(x[o+O.SHOW]));sceneProperty(overlay.layers,"mask",row.actor.body.layers.mask);const v=overlay.material.values,r=ACTOR.LOOK;v[r.SHOCK_TIME]=x[o+O.TIME];v[r.INTENSITY]=x[o+O.OVERLAY_INTENSITY];v[r.SEED]=x[o+O.SEED];for(let i=0;i<3;i++)v[r.COLOR+i]=x[o+O.COLOR+i];}}
   else if(row.overlay)sceneProperty(row.overlay,"visible",false);
  }
  if(this.lights)for(let i=0;i<this.counts[H.LIGHTS];i++){const o=i*I.STRIDE,x=this.lightOutput;this.position.fromArray(x,o+I.POSITION);for(let k=0;k<3;k++)this.rgb[k]=x[o+I.COLOR+k];if(x[o+I.KIND])this.lights.flash(this.position,this.rgb,x[o+I.RADIUS],x[o+I.SCALE]);else this.lights.add(this.position,this.rgb,x[o+I.RADIUS],x[o+I.SCALE]);}
  if(this.batch){this.batch.count=this.counts[H.CAPSULES];this.batch.end();}if(this.viewBatch){this.viewBatch.count=this.counts[H.VIEW_CAPSULES];this.viewBatch.end();}
 }
 clear(){this.refresh();this.kernel.clear();this.refresh();this.apply();}
 // This is resource creation only. Rust decides whether, when and how strongly
 // to show the retained overlay; it shares the body's owned asset and skeleton.
 overlay(actor){const cached=this.overlays.get(actor);if(cached!==undefined)return cached;const body=actor.body;if(!body?.isSkinnedMesh||!body.skeleton){this.overlays.set(actor,null);return null;}
  const overlay=actorNode(body.userData.actorAsset,actorLook('shock'),true);overlay.name='arc-shock';sceneProperty(overlay,"boundingSphere",body.boundingSphere);sceneProperty(overlay,"renderOrder",6);sceneProperty(overlay,"visible",false);sceneProperty(overlay,"castShadow",false);overlay.receiveShadow=false;
  (body.parent||actor.root).add(overlay);sceneTransform(overlay.position.copy(body.position));sceneTransform(overlay.quaternion.copy(body.quaternion));sceneTransform(overlay.scale.copy(body.scale));overlay.bind(body.skeleton,body.bindMatrix);this.overlays.set(actor,overlay);return overlay;
 }
}
const IDENTITY=[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1];

// Cold inspection preserves the effect tools' producer/fixture surface. None
// of these snapshots participates in the displayed frame or renderer upload.
class NativeSet {
 constructor(owner,key){this.owner=owner;this.key=key;}
 get size(){return this.owner.inspect()[this.key].length;}
 has(unit){const id=this.owner.unitIds.get(unit);return id!==undefined&&this.owner.inspect()[this.key].includes(id);}
 set(unit,record){const owner=this.owner,id=owner.unitId(unit);if(id<0)return this;owner.actor(id);owner.kernel.inject_shock(id,JSON.stringify(record,(key,value)=>key==='unit'?undefined:value));return this;}
}
Object.defineProperties(ArcLightning.prototype,{
 bolts:{get(){const owner=this;return owner.inspect().bolts.map(id=>{let record=owner.handles[id].inspection;if(record)return record;record={};
  for(const [key,field] of [['age',B.AGE],['delay',B.DELAY],['life',B.CLASSIC_LIFE]])Object.defineProperty(record,key,{get(){owner.refresh();return owner.output[id*B.STRIDE+field];}});
  for(const [key,field] of [['arrived',B.ARRIVED],['air',B.AIR]])Object.defineProperty(record,key,{get(){owner.refresh();return Boolean(owner.output[id*B.STRIDE+field]);}});
  for(const [key,field] of [['shooter',B.SHOOTER],['source',B.SOURCE],['target',B.TARGET]])Object.defineProperty(record,key,{get(){owner.refresh();return owner.unitRows[owner.output[id*B.STRIDE+field]]||null;}});
  for(const [key,field] of [['from',B.FROM],['to',B.TO],['start',B.START],['end',B.END]])Object.defineProperty(record,key,{get(){owner.refresh();return new THREE.Vector3().fromArray(owner.output,id*B.STRIDE+field);}});
  owner.handles[id].inspection=record;return record;});}},
 shocks:{get(){return this.shockSet||(this.shockSet=new NativeSet(this,'shocks'));}},
 shields:{get(){return this.shieldSet||(this.shieldSet=new NativeSet(this,'shields'));}},
 charges:{get(){return this.chargeSet||(this.chargeSet=new NativeSet(this,'charges'));}},
 sparks:{get(){const values=this.inspect().sparks,pool=this.sparkInspections||(this.sparkInspections=[]);for(let i=0;i<values.length;i++){const a=values[i],s=pool[i]||(pool[i]={p:new THREE.Vector3(),v:new THREE.Vector3()});s.p.fromArray(a.p);s.v.fromArray(a.v);s.age=a.age;s.life=a.life;s.bounced=a.bounced;}return pool.slice(0,values.length);}},
 sparkPool:{get(){const n=this.inspect().sparks.length;return (this.sparkInspections||EMPTY).slice(n);}}
});
ArcLightning.prototype.inspect=function(){this.refresh();return JSON.parse(this.kernel.inspect());};
ArcLightning.prototype.line=function(path,shown,rgb,brightness,core,glow,hot,taper){this.kernel.fixture(JSON.stringify({kind:'line',points:path.points.slice(0,path.count).map(p=>p.toArray()),shown,rgb,brightness,core,glow,hot,taper,count:this.batch.count}),this);this.refresh();this.batch.count=this.counts[H.CAPSULES];};
ArcLightning.prototype.spray=function(record,at,count,speed){this.kernel.fixture(JSON.stringify({kind:'spray',from:record.from.toArray(),to:record.to.toArray(),at:at.toArray(),floor:record.floor,surface:record.surface?.toArray()||null,rgb:record.rgb,count,speed}),this);this.refresh();};
ArcLightning.prototype.drawSparks=function(dt){this.kernel.fixture(JSON.stringify({kind:'sparks',dt,count:this.batch.count}),this);this.refresh();this.batch.count=this.counts[H.CAPSULES];};
let fixtureOwner;
export function displace(path,a,b,depth,u,w,options){fixtureOwner||=new ArcLightning(new THREE.Scene(),{},false);const points=JSON.parse(fixtureOwner.kernel.fixture(JSON.stringify({kind:'displace',a:a.toArray(),b:b.toArray(),depth,u:u.toArray(),w:w.toArray(),...options}),fixtureOwner));path.count=points.length;for(let i=0;i<points.length;i++)path.points[i].fromArray(points[i]);return path;}
