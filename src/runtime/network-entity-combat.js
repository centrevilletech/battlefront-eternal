import {decodePresentation,PRESENTATION_SCHEMA} from './entity-presentation-events.js';
import { ProjectileCapture, ProjectilePresentation } from './network-combat-projectile.js';
import { Vector3 } from 'three';
import { feedbackRecord } from './network-combat-feedback.js';
import { emitSound } from '../audio/events.js';
import { WireCombat } from './wire-combat.js';
import { SimBridge } from '../sim/runtime.js';
import { resourceHost } from '../ordnance-world.js';
import { OrdnanceFlag as FL, putCombatHost, CombatDispatch as CD, CombatInput as CI, CombatResult as CR, CombatOutput as CO, CombatOutputKind as CK } from '../presentation/ordnance-layout.js';
import { presentationModule } from '../presentation/module.js';

// Projectile identity is independent of trigger identity: pellets and automatic
// rounds each have a lifetime. Terminal records retain the same identity.
export const COMBAT_LAUNCH = 1, COMBAT_TERMINAL = 2;
export const LAUNCH_PRESENTATION_TICKS = 24;
// Standalone tools use the same native services as a battle. This is cold
// admission, not an alternate encoder or queue implementation.
let standaloneBridge,encoder;
export function combatBridge(){return standaloneBridge??=new SimBridge({exports:resourceHost(null).x});}
export function combatEncoder(){return encoder??=new WireCombat({bridge:combatBridge()});}
const vector=v=>[v.x,v.y,v.z];
export function combatRecord({id,generation,tick,schema,expiryTick,definitionId,ownerId=0,ownerGeneration=0,fireId=0,channel=0,launchTick=tick,executionTick=tick,position,direction,offset=[0,0,0],speed=0,life=0,merge=0,surface=5,native=null}) {
  return (native??combatEncoder()).bolt({id,generation,tick,schema,expiryTick,definitionId,ownerId,ownerGeneration,fireId,channel,launchTick,executionTick,position,direction,offset,speed,life,merge,surface});
}
export function decodeCombat(record){
  if(record.schema===PRESENTATION_SCHEMA)return decodePresentation(record);
  return {kind:record.kind,id:record.id,generation:record.generation,tick:record.tick,schema:record.schema,payload:record.payload,...decodeFields(record)};
}

const EMPTY_BYTES=new Uint8Array();
export function decodeFields(record){
  try{return JSON.parse(presentationModule().module.PageCombat.decode_record(String(record.kind),record.schema,record.tick,record.payload instanceof Uint8Array?record.payload:EMPTY_BYTES,record.payload instanceof Uint8Array));}
  catch(error){throw Error(error);}
}

export function allocateCapture(capture){
  let values;try{values=presentationModule().module.PageCombat.capture_identity(capture.nextId);}catch(error){throw Error(error);}
  capture.nextId=values[1];return values[0];
}

export class CombatCapture {
  constructor({tick,definitionId,generation=1,maxRecords=4096,instance=null,bridge=null}){
    Object.assign(this,{tick,definitionId,generation,maxRecords});this.projectiles=new WeakMap();
    this.native=new WireCombat({bridge:bridge??(instance?undefined:combatBridge()),instance});Object.defineProperties(this,{
      pending:{enumerable:true,configurable:true,get:()=>this.native.saved(decodeCombat)},
      nextId:{enumerable:true,configurable:true,get:()=>this.native.nextId,set:id=>{this.native.nextId=id;}},
      faults:{enumerable:true,configurable:true,get:()=>this.native.faults},
    });
  }
  allocate(){return allocateCapture(this);}
  emit(record){return this.native.emit(this,record,decodeCombat);}
  launch(bolt,{tick:sampleTick,sample=null}={}){
    const view=sample??bolt,tick=this.tick();
    const state=JSON.parse(presentationModule().module.PageCombat.capture_bolt(JSON.stringify({sample:!!sample,sampleTick,tick,id:this.allocate(),generation:this.generation,life:view.life,definitionId:this.definitionId(view.def),sampleOwner:[sample?.ownerId,sample?.ownerGeneration,sample?.fireId,sample?.channel],boltOwner:[bolt.networkFireUnit,bolt.networkFireGeneration,bolt.networkFireId,bolt.networkFireChannel]})));
    this.projectiles.set(bolt,state);
    this.emit(combatRecord({
      ...state,tick,schema:1,
      position:vector(view.position),direction:vector(view.direction),
      offset:view.offset?vector(view.offset):[0,0,0],
      speed:view.speed,life:view.life,merge:view.merge,native:this.native,
    }));
  }
  impact(bolt,hit){const dynamic=this.dynamic?.active.get(bolt),state=this.projectiles.get(bolt)??dynamic;if(!state)return;this.projectiles.delete(bolt);if(dynamic){dynamic.terminal=true;this.native.dynamic(dynamic); }this.emit(combatRecord({...state,...JSON.parse(presentationModule().module.PageCombat.capture_terminal(JSON.stringify(state),this.tick())),tick:this.tick(),schema:2,position:vector(hit.point),direction:vector(hit.normal),surface:hit.surface,native:this.native}));}
  explosion(definition,position,{wet=false}={}){this.emit(this.native.explosion({id:this.allocate(),generation:this.generation,tick:this.tick(),definitionId:this.definitionId(definition),position:vector(position),wet}));}
  event(event){if(!presentationModule().module.PageCombat.capture_feedback(event.kind))return;this.emit(feedbackRecord({id:this.allocate(),generation:this.generation,tick:this.tick(),event,native:this.native}));}
  fault(error){this.native.fault(error);}
  drainFaults(){return this.native.drainFaults();}
  drain(){return this.native.drain(this,decodeCombat);}
  dispose(){this.native.dispose();}
}

// Rust retains the event and flight clocks. This map keeps only the host
// resources and public readback objects consumed by trails, prediction and tests.
export class CombatPresentation {
  constructor({ordnance,definition,predictedShots,muzzle=null,maxRecords=4096,onLaunch=()=>{},onFeedback=()=>{},onPresentation=()=>{},onExplosion=()=>{}}) {
    Object.assign(this,{ordnance,definition,predictedShots,muzzle,onLaunch,onFeedback,onPresentation,onExplosion});
    this.loaded=presentationModule();this.kernel=new this.loaded.module.PageCombat(maxRecords);
    this.records=new Map();this.definitions=new WeakMap();this.epoch=null;this.dynamic=new ProjectilePresentation(ordnance,definition);
    this.refresh();
  }
  refresh() {
    const b=this.loaded.memory.buffer,k=this.kernel;if(this.buffer===b)return;
    this.buffer=b;this.input=new Float64Array(b,k.input_ptr(),k.input_len());
    this.result=new Float64Array(b,k.result_ptr(),k.result_len());
    this.output=new Float64Array(b,k.output_ptr(),k.output_len());
  }
  clear(epoch) {
    this.dynamic.reset();this.ordnance.clearProjectilePresentation();this.records.clear();
    this.epoch=epoch;this.tick=0;this.presentedTick=0;
  }
  reset(epoch) { this.kernel.reset(epoch);this.clear(epoch); }
  // Each output names an ID, including removals: reused native slots never
  // accidentally inherit a different round's mesh or deferred impact.
  changes(epoch=this.epoch) {
    const a=this.output;
    for(let n=0;n<this.result[CR.ROWS];n++) {
      const at=n*CO.STRIDE,id=a[at+CO.ID],kind=a[at+CO.KIND];
      if(kind===CK.RESET)this.clear(epoch);
      else if(kind===CK.REMOVE)this.records.delete(id);
    }
    this.tick=this.result[CR.TICK];this.presentedTick=this.result[CR.PRESENTED_TICK];
  }
  receive(record,{epoch,tick}) {
    const event=decodeCombat(record),def=this.definition(event.definitionId);
    let definition=def?this.definitions.get(def):'{}';
    if(definition===undefined){const data={};for(const key of DEFINITION_FIELDS)data[key]=def[key];definition=JSON.stringify(data);this.definitions.set(def,definition);}
    this.refresh();putCombatHost(this.input,0,event,def);
    let accepted;try{accepted=this.kernel.receive_facts(epoch,tick,definition,!!def);}catch(error){throw Error(error);}
    this.refresh();this.changes(epoch);
    if(!accepted)return false;
    const revive=this.result[CR.REVIVE]!==0;
    const predicted=this.result[CR.CLAIM]!==0&&!!this.predictedShots?.claim({playerId:event.ownerId,generation:event.ownerGeneration,fireId:event.fireId,channel:event.channel},{tick:event.executionTick??event.launchTick});
    if(this.result[CR.LAUNCH])this.onLaunch(event,def,predicted);
    if(this.kernel.muzzle_request(predicted)) {
      const muzzle=this.muzzle?.(event,def);this.refresh();
      if(muzzle){this.input[CI.HAS_MUZZLE]=1;this.input[CI.MUZZLE]=muzzle.x;this.input[CI.MUZZLE+1]=muzzle.y;this.input[CI.MUZZLE+2]=muzzle.z;}
    }
    this.input[CI.DYNAMIC]=+this.dynamic.objects.has(record.id);
    const resolved=this.kernel.resolve(predicted);this.refresh();
    if(!resolved)return false;
    const old=this.records.get(record.id);
    if(revive) {
      old.bolt=this.readBolt(old,event,def);if(old.impact)old.impact.at=null;return true;
    }
    const state=event,dispatch=this.result[CR.DISPATCH];state.impactEffect=this.result[CR.IMPACT_EFFECT];
    // Effects retain their own point/callback. Reserve them at event admission,
    // so a delayed impact or explosion creates no numeric frame scratch.
    if(this.result[CR.POINT])state.point=new Vector3(...state.position);
    if(dispatch===CD.TERMINAL){
      const point=state.point,name=this.loaded.module.PageCombat.impact_name(state.impactEffect,definition),sound=this.result[CR.SPLASH]?'splash':'impact',character=!!this.result[CR.CHARACTER];
      state.impactName=name;
      state.playImpact=(seed=null)=>{if(name)this.ordnance.effects.play(name,point,null,null,null,seed);emitSound(sound,def,point,character);};
    }
    this.records.set(record.id,state);
    if(dispatch===CD.PRESENTATION)state.pending=this.onPresentation(event)===false;
    else if(dispatch===CD.DYNAMIC)this.dynamic.receive(event);
    else if(dispatch===CD.FEEDBACK)this.onFeedback(event);
    else if(dispatch===CD.EXPLOSION)state.explosion={def,at:event.tick};
    else {
      if(dispatch===CD.TERMINAL)this.dynamic.receive(event);
      if(this.result[CR.STOP_PREDICTED])this.predictedShots.stop?.({playerId:event.ownerId,generation:event.ownerGeneration,fireId:event.fireId,channel:event.channel},{tick:event.executionTick,point:event.position});
      state.predicted=predicted;
      if(this.result[CR.BOLT])state.bolt=this.readBolt(old,event,def);
      if(this.result[CR.PENDING_IMPACT])state.impact={def,at:Number.isNaN(this.result[CR.IMPACT_AT])?null:this.result[CR.IMPACT_AT]};
      if(this.result[CR.IMPACT])this.impact(state,def);
    }
    return true;
  }
  // New resource facades are made only at event admission. The native phase
  // publishes existing bolt numbers in place on subsequent displayed frames.
  readBolt(old,event,def) {
    let bolt=this.result[CR.REUSE_BOLT]?old?.bolt:undefined;
    if(!bolt)bolt={def,position:new Vector3(),direction:new Vector3(),offset:new Vector3(),flown:0,drawn:0,
      visualMuzzle:this.result[CR.FOLLOW_MUZZLE]&&this.muzzle?()=>this.muzzle(event,def):null};
    this.copyBolt(bolt,0);return bolt;
  }
  copyBolt(bolt,at) {
    const a=this.output;
    bolt.position.set(a[at+CO.POSITION],a[at+CO.POSITION+1],a[at+CO.POSITION+2]);
    bolt.direction.set(a[at+CO.DIRECTION],a[at+CO.DIRECTION+1],a[at+CO.DIRECTION+2]);
    // The draw phase can resolve a muzzle offset once; later flight clock
    // publications leave that native-draw readback on its resource facade.
    if(a[at+CO.COPY_OFFSET])bolt.offset.set(a[at+CO.OFFSET],a[at+CO.OFFSET+1],a[at+CO.OFFSET+2]);
    bolt.speed=a[at+CO.SPEED];bolt.life=a[at+CO.LIFE];bolt.merge=a[at+CO.MERGE];
    bolt.networkFlightTick=a[at+CO.FLIGHT_TICK];bolt.executionTick=a[at+CO.EXECUTION_TICK];
    bolt.started=!!a[at+CO.STARTED];
  }
  impact(state,def) {
    const point=state.point,name=state.impactName,play=state.playImpact;
    // Sniper trace owns its delayed impact resources after this handoff.
    if(this.ordnance.deferSniperImpact?.(def,point,play,state,Boolean(name&&this.ordnance.effects.library?.[name]&&this.ordnance.effects.freeCount)))return;
    play();
  }
  present(tick) {
    this.kernel.present(tick);this.refresh();this.changes();this.dynamic.present(tick);this.refresh();
    // Retry resource admission before drawing, preserving the old callback order.
    for(let n=0;n<this.result[CR.ROWS];n++) {
      const at=n*CO.STRIDE;if(this.output[at+CO.KIND]!==CK.DRAW)continue;
      const state=this.records.get(this.output[at+CO.ID]);if(state?.pending)state.pending=this.onPresentation(state)===false;
    }
    for(let n=0;n<this.result[CR.ROWS];n++) {
      const at=n*CO.STRIDE;if(this.output[at+CO.KIND]!==CK.DRAW)continue;
      const state=this.records.get(this.output[at+CO.ID]);if(!state)continue;
      if(this.output[at+CO.BOLT]) {
        const bolt=state.bolt;
        // Flight geometry changes only on admission/terminal. The frame can
        // change the first-draw clock, so publish just that touched value.
        bolt.networkFlightTick=this.output[at+CO.FLIGHT_TICK];bolt.started=!!this.output[at+CO.STARTED];
        let muzzle=null;if(this.output[at+CO.RESOLVE_MUZZLE]&&bolt.visualMuzzle){muzzle=bolt.visualMuzzle();bolt.visualMuzzle=null;}
        this.ordnance.draw.bolt(bolt,1,tick,muzzle,FL.RESOLVE_SHOWN);
      }
      this.ordnance.queueSniperImpact?.(state,tick);
    }
  }
  settle(tick) {
    this.ordnance.matchSniperImpacts?.();this.kernel.settle(tick);this.refresh();
    for(let n=0;n<this.result[CR.ROWS];n++) {
      const at=n*CO.STRIDE,state=this.records.get(this.output[at+CO.ID]);if(!state)continue;
      if(this.output[at+CO.KIND]===CK.EXPLODE) {
        const def=state.explosion.def,point=state.point;
        this.ordnance.presentExplosion(def,point,state.wet);this.onExplosion(def,point);state.explosion=null;
      } else if(this.output[at+CO.KIND]===CK.IMPACT){this.impact(state,this.definition(state.definitionId));state.impact=null;}
    }
  }
}
// Authored effect names remain resource handles. Rust selects their precedence.
const DEFINITION_FIELDS=['FadeOutTime','Velocity','ImpactEffectTerrain','ImpactEffectStatic','ImpactEffectSoft','ImpactEffectRigid','ImpactEffectWater'];
export function attachCombatCapture(game,capture){
  const ordnance=game.ordnance,launch=ordnance.onLaunch,impact=ordnance.onImpact,explode=ordnance.onExplosion,projectile=ordnance.onProjectile;
  capture.dynamic=new ProjectileCapture(capture,ordnance);
  const observe=fn=>{try{fn();}catch(error){capture.fault(error);}};
  const onLaunch=(bolt,context)=>{launch?.(bolt,context);observe(()=>capture.launch(bolt,context));},onImpact=(bolt,hit)=>{impact?.(bolt,hit);observe(()=>capture.impact(bolt,hit));};
  const onExplode=(definition,position,owner,context)=>{explode?.(definition,position,owner,context);observe(()=>capture.explosion(definition,position,context));};
  const onProjectile=(p,type,sample)=>{projectile?.(p,type,sample);observe(()=>capture.dynamic.launch(p,type,sample));};
  ordnance.onProjectile=onProjectile;ordnance.onLaunch=onLaunch;ordnance.onImpact=onImpact;ordnance.onExplosion=onExplode;
  const unlisten=game.simEvents.listen(event=>observe(()=>capture.event(event)));
  return()=>{if(ordnance.onLaunch===onLaunch)ordnance.onLaunch=launch;if(ordnance.onImpact===onImpact)ordnance.onImpact=impact;if(ordnance.onExplosion===onExplode)ordnance.onExplosion=explode;if(ordnance.onProjectile===onProjectile)ordnance.onProjectile=projectile;capture.dynamic=null;unlisten();};
}
