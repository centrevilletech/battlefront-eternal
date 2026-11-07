import {sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import {EFFECT} from './rust-render/effect-layout.js';
import * as THREE from 'three';
import {lighting} from './materials.js';
import {particleDepth} from './particle-depth.js';
import {presentationModule} from './presentation/module.js';
import {SniperFrame as F,SniperCommand as C,SniperKind as K,SniperEvent as E,
  SniperEventKind as EK,SniperMatch as M,SniperCounts as H,SniperLimits as L,SniperStreak as SS,SniperFlare as SF} from './presentation/sniper-layout.js';
export const sniperLook={enhanced:false};
// The page owns shader resources and callback handles. Flight, vapour,
// matching and draw coordinates belong to one retained native owner.
const STREAK_ATTRIBUTES = [["iHead", 3, SS.HEAD], ["iTail", 3, SS.TAIL], ["iColor", 4, SS.COLOR], ["iSize", 4, SS.SIZE], ["iMisc", 4, SS.MISC]];
const STREAK_STRIDE = SS.STRIDE;
const FLARE_ATTRIBUTES = [["iCenter", 3, SF.CENTER], ["iAxis", 3, SF.AXIS], ["iColor", 4, SF.COLOR], ["iParam", 4, SF.PARAM]];
const FLARE_STRIDE = SF.STRIDE;





// Native SniperTrace publishes borrowed interleaved rows. Resource admission
// owns capsule/flare topology and material policy, with no Three CPU shell.
function instanced(attributes,stride,layout,data,name,order){
 const mesh=new THREE.Object3D();mesh.name=name;sceneProperty(mesh,"frustumCulled",false);sceneProperty(mesh,"renderOrder",order);sceneProperty(mesh,"visible",false);
 const batch={mesh,data:null,arrays:{interleaved:null},capacity:0,count:0,attributes,stride,nativeKind:layout.ID,variant:0,texture:null,values:new Float64Array(layout.LENGTH),family:layout===EFFECT.SNIPER_STREAKS?'sniperStreaks':'sniperFlares'};
 mesh.userData.effectBatch=batch;bind(batch,data);return batch;
}
function bind(batch,data){batch.data=batch.arrays.interleaved=data;batch.capacity=data.length/batch.stride;}
function upload(batch){sceneProperty(batch.mesh,"visible",batch.count>0);}

export class SniperTrace {
  constructor(lights=null,view=null,muzzle=null){
    this.lights=lights;this.view=view;this.muzzle=muzzle;
    // Native arrays are also the draw buffers. Resource creation can grow
    // shared memory, so refresh rebinds the borrowed interleaved rows when needed.
    this.loaded=presentationModule();this.kernel=new this.loaded.module.PageSniper();
    this.sourceIds=new WeakMap();this.nextSource=0;this.nextCallback=0;this.callbacks=new Map();
    this.matchStates=new Array(L.COMMANDS);this.matchCount=0;this.count=0;this.begun=false;this.prepared=false;this.dropped=0;
    this.position=new THREE.Vector3();this.rgb=[0,0,0];this.refresh();
    this.camera = new THREE.Vector3(0, 1e6, 0);
    this.streaks=instanced(STREAK_ATTRIBUTES,STREAK_STRIDE,EFFECT.SNIPER_STREAKS,this.nativeStreaks,'sniper-trace',11);
    this.flares=instanced(FLARE_ATTRIBUTES,FLARE_STRIDE,EFFECT.SNIPER_FLARES,this.nativeFlares,'sniper-flares',12);
    this.streaks.values[EFFECT.SNIPER_STREAKS.PIXEL_ANGLE]=.002;
    this.mesh = new THREE.Group();
    this.mesh.name = "sniper-shots";
    this.mesh.add(this.streaks.mesh, this.flares.mesh);
    const size = new THREE.Vector2();
    this.streaks.mesh.onBeforeRender = (renderer, scene, camera) => {
      particleDepth.capture(renderer, scene, camera);
      if (!camera.isPerspectiveCamera) return;
      this.camera.setFromMatrixPosition(camera.matrixWorld);
      const target = renderer.getRenderTarget();
      const height = target ? target.height : renderer.getDrawingBufferSize(size).y;
      this.streaks.values[EFFECT.SNIPER_STREAKS.PIXEL_ANGLE]= (2 * Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2)) / Math.max(height, 1) / (camera.zoom || 1);
    };
  }

  // The two native resource clients borrow output directly. Linear-memory growth
  // invalidates all old views together; rebind their arrays at that boundary.
  refresh(){const b=this.loaded.memory.buffer;if(b===this.buffer)return;this.buffer=b;const k=this.kernel;
    this.input=new Float64Array(b,k.input_ptr(),F.STRIDE+L.COMMANDS*C.STRIDE);
    this.events=new Float64Array(b,k.events_ptr(),L.EVENTS*E.STRIDE);
    this.matches=new Float64Array(b,k.matches_ptr(),L.COMMANDS*M.STRIDE);
    this.counts=new Uint32Array(b,k.counts_ptr(),H.STRIDE);
    this.nativeStreaks=new Float32Array(b,k.streaks_ptr(),L.STREAKS*STREAK_STRIDE);
    this.nativeFlares=new Float32Array(b,k.flares_ptr(),L.FLARES*FLARE_STRIDE);
    if(this.streaks){bind(this.streaks,this.nativeStreaks);bind(this.flares,this.nativeFlares);}
  }
  // Include native draw/event overflow, whose lifetime counter survives a
  // quiet frame. Refresh first so diagnostics survive linear-memory growth.
  get droppedCount(){this.refresh();return this.dropped+this.counts[H.DROPPED];}
  // Camera and muzzle are host resources. Pack their published transforms
  // only while a native flight record can use them; no scene query is needed
  // for an empty trace. Time still advances through the coarse draw phase.
  facts(){this.refresh();const x=this.input,active=this.count||this.counts[H.RECORDS],camera=active?this.view?.():null,muzzle=active?this.muzzle?.():null;x[F.TIME]=lighting.time.value;
    x[F.HAS_CAMERA]=camera?1:0;x[F.HAS_MUZZLE]=muzzle?1:0;
    if(camera){const m=camera.matrixWorld.elements,inv=camera.matrixWorldInverse.elements;
      x[F.EYE]=m[12];x[F.EYE+1]=m[13];x[F.EYE+2]=m[14];
      for(let i=0;i<16;i++){x[F.CAMERA+i]=m[i];x[F.INVERSE+i]=inv[i];}}
    else vector(x,F.EYE,this.camera);
    if(muzzle)vector(x,F.MUZZLE,muzzle);
  }
  // The bounded command store retains its allocation through long pauses.
  // Overflow drops extra visual work without throwing on a frame path.
  command(kind){this.refresh();if(this.count===L.COMMANDS){this.dropped++;return -1;}const at=F.STRIDE+this.count++*C.STRIDE;
    this.input.fill(0,at,at+C.STRIDE);this.input[at+C.KIND]=kind;return at;}
  // Source identity is cold metadata in a WeakMap. Commands carry numeric
  // ids, so native record compaction never invalidates a host source.
  begin(){this.begun=true;this.prepared=false;this.matchCount=0;}
  // Native collection owns merging and source retirement. The resource host
  // supplies authored color and range alongside this round's current facts.
  touch(source,def,start,head,direction,ended,_fade,origin=start){const at=this.command(K.TOUCH);if(at<0)return;
    let id=this.sourceIds.get(source);if(id===undefined){id=++this.nextSource;this.sourceIds.set(source,id);}
    const x=this.input,color=def.LaserGlowColor||def.LightColor||DEFAULT_COLOR;x[at+C.SOURCE]=id;
    vector(x,at+C.START,start);vector(x,at+C.HEAD,head);vector(x,at+C.DIRECTION,direction);vector(x,at+C.ORIGIN,origin);
    x[at+C.COLOR]=color[0];x[at+C.COLOR+1]=color[1];x[at+C.COLOR+2]=color[2];
    x[at+C.REACH]=(def.Velocity||0)*(def.LifeSpan||0);x[at+C.ENDED]=ended?1:0;
  }
  // Arrival order remains the order in this command block, including a
  // headshot delivered before its visible round has reached the target.
  headshot(point){const at=this.command(K.HEADSHOT);if(at>=0)vector(this.input,at+C.POINT,point);}
  // A match result was computed for every due terminal before settlement.
  // Out-of-band arrivals join the next collection batch by their point.
  defer(point,play,state=null,seedEffect=false){if(state?.sniperRecord===0||this.callbacks.size===L.COMMANDS)return false;
    const at=this.command(K.DEFER);if(at<0)return false;const token=this.nextCallback=this.nextCallback===0xffffffff?1:this.nextCallback+1;
    this.callbacks.set(token,play);this.input[at+C.TOKEN]=token;this.input[at+C.RECORD]=state?.sniperRecord??-1;
    this.input[at+C.SEED_EFFECT]=seedEffect?1:0;vector(this.input,at+C.POINT,point);return true;
  }
  prepare(){this.refresh();
    // Empty frames still advance native time in end. With no record to
    // mark untouched, collection has no commands or resource facts to read.
    if(!this.count&&!this.counts[H.RECORDS]){this.begun=false;this.prepared=true;return;}
    this.facts();this.input[F.BEGIN]=this.begun?1:0;this.input[F.COMMANDS]=this.count;
    this.kernel.collect();this.refresh();this.count=0;this.begun=false;this.prepared=true;this.apply();}
  // CombatPresentation's existing record walk packs due points. Matching
  // adds no second Map iterator or transient command object to a frame.
  queueMatch(state,tick){
    state.sniperRecord=undefined;const impact=state.impact,bolt=state.bolt;
    if(!impact||!(impact.def.FadeOutTime>0))return;
    const at=impact.at??(bolt?.started?bolt.networkFlightTick+bolt.life*60:null);
    if(at===null||tick<at||tick-at>18)return;
    if(this.matchCount===L.COMMANDS){state.sniperRecord=0;return;}
    this.refresh();const count=this.matchCount++;this.matchStates[count]=state;
    this.matches.set(state.position,count*M.STRIDE+M.POINT);
  }
  // Terminal callbacks consume these stable record ids after collection.
  // This separates matching from resource settlement without a WASM call
  // for each impact or a native-to-page callback during numeric work.
  matchImpacts(){this.facts();this.kernel.match_impacts(this.matchCount);this.refresh();
    for(let i=0;i<this.matchCount;i++){this.matchStates[i].sniperRecord=this.matches[i*M.STRIDE+M.RECORD];this.matchStates[i]=null;}
    this.matchCount=0;
  }
  // Callback ids survive record compaction and source aliases. A reserved
  // effect seed preserves random order while resource births replay here.
  apply(){for(let i=0;i<this.counts[H.EVENTS];i++){const at=i*E.STRIDE,kind=this.events[at+E.KIND];
    if(kind===EK.RELEASE){this.callbacks.delete(this.events[at+E.TOKEN]);continue;}
    if(kind===EK.CALLBACK){const token=this.events[at+E.TOKEN],play=this.callbacks.get(token);this.callbacks.delete(token);play?.(this.events[at+E.SEED]<0?null:this.events[at+E.SEED]);this.refresh();continue;}
    sceneTransform(this.position.fromArray(this.events,at+E.POSITION));for(let k=0;k<3;k++)this.rgb[k]=this.events[at+E.COLOR+k];
    if(kind===EK.LIGHT)this.lights?.add?.(this.position,this.rgb,this.events[at+E.REACH]);
    else this.lights?.flash?.(this.position,this.rgb,this.events[at+E.REACH],this.events[at+E.TIME]);
  }}
  // Output counts describe prefixes of retained instance buffers. The two
  // publications keep their native resource identities for every frame.
  end(){if(!this.prepared||this.count)this.prepare();this.facts();this.kernel.frame();this.refresh();this.apply();
    this.streaks.count=this.counts[H.STREAKS];this.flares.count=this.counts[H.FLARES];
    upload(this.streaks);upload(this.flares);
  }
}
const DEFAULT_COLOR=[120,160,255];
function vector(x,at,p){x[at]=p.x;x[at+1]=p.y;x[at+2]=p.z;}
