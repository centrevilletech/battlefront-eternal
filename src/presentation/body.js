import * as THREE from "three";
// Cold body mirrors and the page's immediate look share the native unit
// implementation. Replay owns ticks; this phase only consumes pointer samples.
import { BodyPreviewFrameLayout as F, BodyLookLayout as L, BodySpawnLayout as S,
  Posture_NAMES, Air_NAMES, syncSoldier, syncBodySpawnOutput, BodySpawnOutputMirror,
  publishBodySpawnOutput, DroidekaSpawnLayout as D, DroidekaSpawnOutputMirror,
  syncDroidekaSpawnOutput, publishDroidekaSpawnOutput } from '../sim/gen/units.js';
import { BlockHeaderLayout as H } from '../sim/gen/boundary.js';

// Each body retains its battle module outside the correction payload. A
// correction can replace every scalar without replacing that native owner.
const owners=new WeakMap();
export function registerBody(body,native){owners.set(body,native);}
export function bodyNative(body) { return owners.get(body); }
// These short-lived views belong to cold births only. Preview frames keep
// their views and refresh them only after another resource grows memory.
function result(x, ptr) {
  const words = new Float64Array(x.memory.buffer), at = ptr / 8;
  return { words, at:at + H.words, heap:at + words[at + H.heap] };
}
// Soldier.constructor supplies authored content once. Defaults and tuning
// are resolved by the same class reader used for authoritative units.
export function constructSoldier(body, world, def, x) {
  owners.set(body,x);
  const bytes = new TextEncoder().encode(JSON.stringify(def)), ptr = x.sim_scratch(bytes.length);
  new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
  const out = result(x, x.body_construct(ptr, bytes.length));
  syncSoldier(null,out.words,out.at,out.heap,body);
  // Keep the optional flag in the fixed mirror shape. Deleting an earlier
  // field would make every subsequent numeric publication allocate boxes.
  body.dead=undefined;
}
export function spawnSoldier(body, x, y, z, yaw) {
  const world = body.world, native = owners.get(body), ptr = native.sim_call_room(S.words,0);
  let input = new Float64Array(native.memory.buffer), at = ptr / 8 + H.words;
  input[at+S.position]=x; input[at+S.position+1]=y; input[at+S.position+2]=z;
  input[at+S.yaw]=yaw; input[at+S.recharge]=body.jet.recharge;
  // Births query the same native world as movement. There is no host ray,
  // or separate grounding formula in the page constructor.
  world?.rayBodies?.();
  if (world?.movement) world.movement.prepare();
  const out = result(native,native.body_spawn(world?.rayWorld ?? 0xffffffff));
  const mirror = syncBodySpawnOutput(null,out.words,out.at,out.heap,new BodySpawnOutputMirror());
  publishBodySpawnOutput(body,mirror);
  body.passThrough=null;
}

// Droideka.spawn is a cold lifecycle operation, using the native body reset.
// Animator handles remain resource commands rather than another pose solver.
export function spawnDroideka(body,x,y,z,yaw) {
  const world=body.world,native=owners.get(body),ptr=native.sim_call_room(D.words,0);
  const input=new Float64Array(native.memory.buffer),at=ptr/8+H.words;
  input[at+D.position]=x;input[at+D.position+1]=y;input[at+D.position+2]=z;input[at+D.yaw]=yaw;
  input[at+D.hasShield]=body.shield?1:0;input[at+D.shieldMax]=body.shield?.max??0;
  world?.movement?.prepare();
  const out=result(native,native.body_spawn_droideka(world?.rayWorld??0xffffffff));
  const mirror=syncDroidekaSpawnOutput(null,out.words,out.at,out.heap,new DroidekaSpawnOutputMirror());
  publishDroidekaSpawnOutput(body,mirror);
  if(body.shield){body.shield.value=mirror.shieldValue;body.shield.on=mirror.shieldOn;}
  body.animator.reset(yaw);body.muzzleAnimator?.reset(yaw);
}

// Soldier.look/lookRaw are a complete preview phase, not body methods.
// Pending samples never mutate the correction that replay restores next.
// Body coordinates and displayed feet have independent clocks. Keeping both
// lets the native eye follow correction smoothing without moving gameplay.
function aimFields(f,at,body,combat,feet) {
    f[at+F.aimYaw]=body.aimYaw; f[at+F.aimPitch]=body.aimPitch;
    f[at+F.sourceYaw]=body.aimYaw;f[at+F.sourcePitch]=body.aimPitch;
    f[at+F.maxTurn]=body.maxTurn; f[at+F.pitchLimits]=body.pitchLimits?.[0]??-.8; f[at+F.pitchLimits+1]=body.pitchLimits?.[1]??1.1;
    f[at+F.posture]=Posture_NAMES.indexOf(body.posture); f[at+F.air]=body.air==null?1:0;
    f[at+F.air+1]=Air_NAMES.indexOf(body.air); f[at+F.dive]=body.dive; f[at+F.lock]=body.lock;
    f[at+F.tickLength]=body.tickLength; f[at+F.lookX]=body.lookX; f[at+F.lookY]=body.lookY;
    f[at+F.count]=0; f[at+F.skipped]=0;
    f[at+F.position]=body.position.x;f[at+F.position+1]=body.position.y;f[at+F.position+2]=body.position.z;
    f[at+F.feet]=feet.x;f[at+F.feet+1]=feet.y;f[at+F.feet+2]=feet.z;
    f[at+F.zoom]=combat?.zoom??1;f[at+F.walker]=body.mode!==undefined?1:0;
    f[at+F.eyeHeight]=body.eyeHeight??0;f[at+F.rolled]=body.mode>=11?1:0;
    // These are authored section values, not camera-space offsets computed
    // by the host. Native selection applies airborne and prone conventions.
    const camera=combat?.hudCamera||combat?.camera, sections=combat?.def?.cameras;
    f[at+F.hasCamera]=camera?1:0;f[at+F.usesEye]=combat?.usesEyeView?1:0;
    if(camera){f[at+F.camera]=camera.position.x;f[at+F.camera+1]=camera.position.y;f[at+F.camera+2]=camera.position.z;}
    f[at+F.trackOffsets]=sections?.STAND?.TrackOffset?.[2]??3.5;
    f[at+F.trackOffsets+1]=sections?.CROUCH?.TrackOffset?.[2]??sections?.STAND?.TrackOffset?.[2]??3.5;
    f[at+F.trackOffsets+2]=sections?.PRONE?.TrackOffset?.[2]??sections?.STAND?.TrackOffset?.[2]??3.5;
 }
export class BodyPreview {
  constructor(world, native=world.rays.x) {
    this.x=native;
    // Discover addresses while cold; no rendered actor asks WASM for them.
    this.frameAt=this.x.body_preview_frame_ptr(); this.looksAt=this.x.body_preview_looks_ptr();
    this.capacity=this.x.body_preview_capacity(); this.memory=null;
    this.aimYaw=0; this.aimPitch=0;
    this.basis={origin:new THREE.Vector3(),pivot:new THREE.Vector3(),left:new THREE.Vector3(),up:new THREE.Vector3(),aimDir:new THREE.Vector3(),eyePoint:new THREE.Vector3()};
    this.from=new THREE.Vector3();this.direction=new THREE.Vector3();this.angles={aimYaw:0,aimPitch:0};
    this.refresh();
  }
  refresh() {
    const buffer=this.x.memory.buffer;
    if(this.memory===buffer)return;
    this.memory=buffer;this.frame=new Float64Array(buffer,this.frameAt,F.words);
    this.looks=new Float64Array(buffer,this.looksAt,this.capacity*L.words);
  }
  // Class and posture facts come from the corrected source. The same source
  // may have changed class or air state since the preceding displayed frame.
  begin(body,combat=null,feet=body.position) {
    this.refresh();aimFields(this.frame,0,body,combat,feet);this.combat=combat;this.count=0;
  }
  append(looks,zoom,scale,invert,raw=true) {
    // Paused input bursts are bounded. The native side independently clamps
    // the count, so a malformed frame cannot read past its reserved block.
    for(let i=0;i<looks.length;i++) {
      const index=this.count++;
      if(index>=this.capacity)continue;
      const at=index*L.words, row=this.looks, look=looks[i];
      row[at+L.dx]=look[0]/zoom; row[at+L.dy]=look[1]/zoom;
      row[at+L.scale]=scale; row[at+L.invert]=invert?1:0; row[at+L.raw]=raw?1:0;
    }
  }
  run(out=this) {
    // One call consumes the ordered prefix. Returning this retained view
    // lets both frame preview and command aim reuse their result storage.
    this.frame[F.count]=this.count;
    this.x.body_preview(); this.refresh();
    out.aimYaw=this.frame[F.aimYaw]; out.aimPitch=this.frame[F.aimPitch];
    const f=this.frame,b=this.basis,at=F.basis;
    b.origin.fromArray(f,at);b.pivot.fromArray(f,at+3);b.left.fromArray(f,at+6);
    b.up.fromArray(f,at+9);b.aimDir.fromArray(f,at+12);b.eyePoint.fromArray(f,F.eye);
    this.from.fromArray(f,F.from);this.direction.copy(b.aimDir);this.needsReach=!!f[F.needsReach];
    this.angles.aimYaw=f[F.shotYaw];this.angles.aimPitch=f[F.shotPitch];
    if(this.combat&&this.needsReach){const c=this.combat,at=F.sourceBasis;
      c.origin.fromArray(f,at);c.pivot.fromArray(f,at+3);c.left.fromArray(f,at+6);c.up.fromArray(f,at+9);c.aimDir.fromArray(f,at+12);
    }
    return out;
  }
  resolve(reach) {
    // A cold collision resource can grow the shared module while answering.
    this.refresh();this.frame[F.reach]=reach;this.x.body_preview_resolve();this.refresh();
    this.angles.aimYaw=this.frame[F.shotYaw];this.angles.aimPitch=this.frame[F.shotPitch];
    return this.angles;
  }
}

// All replayed shots are resolved as one presentation block. No callback or
// per-shot native call runs while the prediction owner is advancing ticks.
export class BodyAimBatch {
  constructor(world) {
    this.x=world.rays.x;this.ptr=this.x.body_aim_ptr();this.capacity=this.x.body_aim_capacity();
    this.skipped=0;this.warned=false;this.memory=null;this.from=new THREE.Vector3();this.direction=new THREE.Vector3();this.refresh();
  }
  refresh(){const buffer=this.x.memory.buffer;if(this.memory===buffer)return;
    this.memory=buffer;this.data=new Float64Array(buffer,this.ptr,this.capacity*F.words);}
  // Notice actors are already retained in the prediction result. Copy their
  // scalar facts directly without adopting them as the live page soldier.
  write(index,body,combat){if(index>=this.capacity)return false;
    this.refresh();aimFields(this.data,index*F.words,body,combat,body.position);return true;}
  run(count,resume=false){this.x.body_aim_batch(count,resume?1:0);this.refresh();
    this.skipped=this.data[F.skipped];
    if(this.skipped&&!this.warned){this.warned=true;console.warn("Shot aim capacity exceeded; extra cosmetic shots were skipped.");}}
  // Collision remains a foreign query until its unit/world owners share
  // memory. Every answer is ready before the one native convergence resume.
  answer(index,combat){const f=this.data,at=index*F.words;
    if(f[at+F.needsReach]){this.from.fromArray(f,at+F.from);this.direction.fromArray(f,at+F.basis+12);
      const reach=combat.crosshairReach(this.from,this.direction);
      this.refresh();this.data[at+F.reach]=reach;}}
  // A callback may construct a cold resource and grow the module. Refresh
  // this borrowed block before publishing the next shot's retained result.
  publish(index,combat,notice){this.refresh();const f=this.data,at=index*F.words,b=at+F.basis;
    if(!f[at+F.walker]||f[at+F.needsReach]){combat.origin.fromArray(f,b);combat.pivot.fromArray(f,b+3);combat.left.fromArray(f,b+6);
    combat.up.fromArray(f,b+9);combat.aimDir.fromArray(f,b+12);}
    combat.eyePoint.fromArray(f,at+F.eye);
    notice.aimYaw=f[at+F.shotYaw];notice.aimPitch=f[at+F.shotPitch];
  }
}
