import {decodePresentation,PRESENTATION_SCHEMA} from './entity-presentation-events.js';
import { ProjectileCapture, ProjectilePresentation, decodeProjectile } from './network-combat-projectile.js';
import { Vector3 } from 'three';
import { feedbackRecord, decodeFeedback } from './network-combat-feedback.js';
import { explosionRecord, decodeExplosion } from './network-combat-explosion.js';
import { emitSound } from '../audio/events.js';

// Projectile identity is independent of trigger identity: pellets and automatic
// rounds each have a lifetime. Terminal records retain the same identity.
export const COMBAT_LAUNCH = 1, COMBAT_TERMINAL = 2;
export const LAUNCH_PRESENTATION_TICKS = 24;
const uint = n => { if (!Number.isSafeInteger(n) || n < 0 || n > 0xffffffff) throw Error('invalid combat integer'); return n; };
const vector = v => [v.x, v.y, v.z];
export function combatRecord({ id, generation, tick, schema, expiryTick, definitionId, ownerId=0, ownerGeneration=0, fireId=0, channel=0, launchTick=tick, executionTick=tick, position, direction, offset=[0,0,0], speed=0, life=0, merge=0, surface=5 }) {
  if (![1,2].includes(schema)) throw Error('unsupported combat schema');
  for(const vector of [position,direction,offset])if(!Array.isArray(vector)||vector.length!==3)throw Error('invalid combat vector');
  const payload=new Uint8Array(schema===1?130:82), view=new DataView(payload.buffer);let at=0;
  const u=n=>{view.setUint32(at,uint(n),true);at+=4;};
  const f=n=>{if(!Number.isFinite(n))throw Error('nonfinite combat value');view.setFloat64(at,n,true);at+=8;};
  for(const n of [expiryTick,definitionId,ownerId,ownerGeneration,fireId])u(n);f(launchTick);
  for(const n of position)f(n);for(const n of direction)f(n);
  view.setUint8(at++,surface);
  if(schema===1){for(const n of offset)f(n);for(const n of [speed,life,merge])f(n);}
  // Physical flight can start at a fractional, rewound view tick. The
  // execution tick identifies the round across its shot, launch and impact.
  u(executionTick);
  if(channel!==0&&channel!==1)throw Error('invalid combat channel');view.setUint8(at++,channel);
  return {kind:'combat',id:uint(id),generation:uint(generation),tick:uint(tick),schema,payload:payload.slice(0,at)};
}
export function decodeCombat(record){
  if(record.schema===PRESENTATION_SCHEMA)return decodePresentation(record);
  if(record.schema===6||record.schema===7)return decodeProjectile(record);
  if(record.schema===5)return decodeExplosion(record);
  if(record.schema===3||record.schema===4)return decodeFeedback(record);
  const {schema,payload}=record;if(record.kind!=='combat'||![1,2].includes(schema)||!(payload instanceof Uint8Array)||payload.length!==(schema===1?130:82))throw Error('invalid combat payload');
  const view=new DataView(payload.buffer,payload.byteOffset,payload.byteLength);let at=0;
  const u=()=>{const n=view.getUint32(at,true);at+=4;return n;},f=()=>{const n=view.getFloat64(at,true);at+=8;if(!Number.isFinite(n))throw Error('nonfinite combat payload');return n;},v=()=>[f(),f(),f()];
  const out={...record,expiryTick:u(),definitionId:u(),ownerId:u(),ownerGeneration:u(),fireId:u(),launchTick:f(),position:v(),direction:v(),surface:view.getUint8(at++)};
  if(schema===1)Object.assign(out,{offset:v(),speed:f(),life:f(),merge:f()});
  out.executionTick=u();
  out.channel=view.getUint8(at++);
  if(out.channel>1||out.launchTick<0||out.executionTick<out.launchTick||out.executionTick>record.tick)throw Error('invalid combat execution tick');
  if(out.launchTick>record.tick||out.expiryTick<record.tick||out.expiryTick-record.tick>7320||out.surface>5||schema===1&&(out.life<0||out.life>120||out.speed<0||out.merge<=0))throw Error('invalid combat lifetime');
  return out;
}

export class CombatCapture {
  constructor({tick,definitionId,generation=1,maxRecords=4096}){Object.assign(this,{tick,definitionId,generation,maxRecords});this.nextId=1;this.projectiles=new WeakMap();this.pending=new Map();this.faults=[];}
  // A newer record replaces a pending one of the same identity (a dynamic
  // projectile's newest sample), except a bolt's terminal: it queues beside
  // its undrained launch. A bolt that ends before the next drain (a short
  // flight, or one flown to its end inside its lag-compensated launch) would
  // otherwise reach every client as an impact with no bolt to draw.
  emit(record){const tick=this.tick();if(this.prunedTick!==tick){for(const [id,event]of this.pending)if(event.expiresTick<tick)this.pending.delete(id);this.prunedTick=tick;}const key=record.schema===2&&this.pending.get(record.id)?.record.schema===1?-record.id:record.id;if(!this.pending.has(key)&&this.pending.size>=this.maxRecords)throw Error('combat capture capacity exceeded');this.pending.set(key,{record,expiresTick:decodeCombat(record).expiryTick,retention:[3,4,5,PRESENTATION_SCHEMA].includes(record.schema)?'cosmetic':'projectile'});}
  launch(bolt,{tick:sampleTick}={}){
    if(this.nextId>0xffffffff)throw Error('combat identity exhausted');
    const tick=this.tick(),launchTick=sampleTick??tick,state={launchTick,executionTick:tick,id:this.nextId++,generation:this.generation,expiryTick:Math.ceil(launchTick+bolt.life*60)+120,definitionId:this.definitionId(bolt.def),ownerId:bolt.networkFireUnit,ownerGeneration:bolt.networkFireGeneration,fireId:bolt.networkFireId,channel:bolt.networkFireChannel??0};
    this.projectiles.set(bolt,state);
    this.emit(combatRecord({...state,tick,schema:1,position:vector(bolt.position),direction:vector(bolt.direction),offset:bolt.offset?vector(bolt.offset):[0,0,0],speed:bolt.speed,life:bolt.life,merge:bolt.merge}));
  }
  impact(bolt,hit){const dynamic=this.dynamic?.active.get(bolt),state=this.projectiles.get(bolt)??dynamic;if(!state)return;this.projectiles.delete(bolt);if(dynamic)dynamic.terminal=true;this.emit(combatRecord({...state,executionTick:state.executionTick??state.launchTick,expiryTick:state.expiryTick??this.tick()+18,tick:this.tick(),schema:2,position:vector(hit.point),direction:vector(hit.normal),surface:hit.surface}));}
  explosion(definition,position,{wet=false}={}){if(this.nextId>0xffffffff)throw Error('combat identity exhausted');this.emit(explosionRecord({id:this.nextId++,generation:this.generation,tick:this.tick(),definitionId:this.definitionId(definition),position:vector(position),wet}));}
  event(event){if(!['damage','death'].includes(event.kind))return;if(this.nextId>0xffffffff)throw Error('combat identity exhausted');this.emit(feedbackRecord({id:this.nextId++,generation:this.generation,tick:this.tick(),event}));}
  fault(error){this.faults.push(String(error?.message??error).slice(0,160));if(this.faults.length>8)this.faults.shift();}
  drainFaults(){return this.faults.splice(0);}
  drain(){try{this.dynamic?.sample();}catch(error){this.fault(error);}const records=[...this.pending.values()];this.pending.clear();return records;}
}

// A linear bolt's drawn flight cut at `point`, where its round stopped short
// of the end of its launch ray (a soldier, a hull).
function stopBolt(bolt,point){
  const along=Math.max(0,(point[0]-bolt.position.x)*bolt.direction.x+(point[1]-bolt.position.y)*bolt.direction.y+(point[2]-bolt.position.z)*bolt.direction.z);
  bolt.life=Math.min(bolt.life,along/Math.max(bolt.speed,1e-6));
  bolt.merge=Math.max(Math.min(bolt.merge,along),1e-3);
  return bolt;
}

// Presentation never calls Ordnance.fire/stepBolt/collision or a damage hook.
export class CombatPresentation {
  // muzzle(event, def): where the shooter's gun is drawn now, or null: a
  // bolt leaves the gun as this page draws it (the server's headless body
  // stands in its bind pose) and joins its authoritative path over MERGE.
  // onExplosion(def, position): an explosion shown here (the view's shake).
  constructor({ordnance,definition,predictedShots,muzzle=null,maxRecords=4096,onLaunch=()=>{},onFeedback=()=>{},onPresentation=()=>{},onExplosion=()=>{}}){Object.assign(this,{ordnance,definition,predictedShots,muzzle,maxRecords,onLaunch,onFeedback,onPresentation,onExplosion});this.records=new Map();this.flashes=new Map();this.epoch=null;this.dynamic=new ProjectilePresentation(ordnance,definition);}
  reset(epoch){this.dynamic.reset();this.ordnance.clearProjectilePresentation();this.records.clear();this.flashes.clear();this.epoch=epoch;this.tick=0;this.presentedTick=0;this.prunedTick=null;}
  // Once per visual tick: newer snapshots must not expire accepted events
  // before delayed playout reaches them. Incoming stale records are still rejected.
  prune(tick){if(tick===this.prunedTick)return;this.prunedTick=tick;for(const [key,expiry]of this.flashes)if(expiry<tick)this.flashes.delete(key);for(const [id,state]of this.records)if(state.expiryTick<tick&&!state.explosion)this.records.delete(id);}
  // Records arrive ahead of the playout clock this page draws at (tick, in
  // receive, is the newest snapshot's). A bolt flies on that clock from its
  // launch to the point its terminal names, and the impact, an explosion
  // and a dynamic projectile's end are shown when the clock reaches them,
  // so nothing is cut short or played early by the playout delay; a bolt
  // whose flight was shorter than that delay used to be gone before it was
  // drawn. The player's own predicted rounds keep their immediate impacts.
  receive(record,{epoch,tick}){
    if(this.epoch!==null&&epoch<this.epoch)return false;
    if(epoch!==this.epoch)this.reset(epoch);tick=this.tick=Math.max(this.tick,tick);this.prune(this.presentedTick??0);
    const event=decodeCombat(record);if(event.expiryTick<tick)return false;
    const old=this.records.get(record.id);
    // A launch repeated after its terminal (its first copy lost) still flies
    // to that terminal's impact while it can start in time.
    if(old&&event.schema===1&&old.schema===2&&old.generation===event.generation&&!old.bolt&&!old.predicted&&tick-event.executionTick<=LAUNCH_PRESENTATION_TICKS+event.life*60){
      const def=this.definition(event.definitionId);if(!def)throw Error('unknown combat definition');
      if(this.launchPresentation(event,def))return false;
      old.bolt=this.bolt(event,def);reach(old.bolt,old.position);if(old.impact)old.impact.at=null;return true;
    }
    if(old&&(old.generation>event.generation||old.generation===event.generation&&([2,7].includes(old.schema)||old.tick>event.tick||old.tick===event.tick&&!([2,7].includes(event.schema)&&event.schema!==old.schema))))return false;
    if(!old&&this.records.size>=this.maxRecords)throw Error('combat presentation capacity exceeded');
    if(event.schema===PRESENTATION_SCHEMA){event.pending=this.onPresentation(event)===false;this.records.set(record.id,event);return true;}
    if(event.schema===6||event.schema===7){this.records.set(record.id,event);this.dynamic.receive(event);if(event.schema===6)this.launchPresentation(event,this.definition(event.definitionId));return true;}
    if(event.schema===3||event.schema===4){this.records.set(record.id,event);this.onFeedback(event);return true;}
    if(event.schema===5){const def=this.definition(event.definitionId);if(!def)throw Error('unknown explosion definition');this.records.set(record.id,event);event.explosion={def,at:event.tick};return true;}
    if(event.schema===2)this.dynamic.receive(event);
    const def=this.definition(event.definitionId);if(!def)throw Error('unknown combat definition');
    const state={...event};this.records.set(record.id,state);
    const predicted=this.launchPresentation(event,def);
    // A class drawn on where it stops (the sniper rifles' FadeOutTime,
    // Ordnance.drawBolt) keeps its streak: the terminal cuts its flight at
    // the impact instead of ending it there and then.
    if(event.schema===2&&def.FadeOutTime>0){
      if(predicted)this.predictedShots.stop?.({playerId:event.ownerId,generation:event.ownerGeneration,fireId:event.fireId,channel:event.channel},{tick:event.executionTick,point:event.position});
      else if(old?.bolt)state.bolt=stopBolt(old.bolt,event.position);
      // Its launch never reached this page, the terminal having replaced it
      // first (it struck within the tick it was fired, or a lag-compensated
      // shot caught up to its target at once): from the shooter's gun as
      // this page draws it to where it stopped.
      else if(!old)state.bolt=this.landedBolt(event,def);
    }
    if(event.schema===1){
      // The player's own round, predicted from the trigger press, flies as
      // the cosmetic drew it; its authoritative copy is never drawn twice.
      if(predicted)state.predicted=true;
      else state.bolt=this.bolt(event,def);
    }else{
      state.predicted=predicted;
      // A bolt in flight here goes on to the terminal's point, and its impact
      // shows when it gets there; without one, the impact shows at its tick.
      if(old?.bolt&&old.generation===event.generation){state.bolt=old.bolt;reach(state.bolt,event.position);}
      if(event.surface===5);
      else if(predicted&&!state.bolt&&!this.dynamic.objects.has(record.id)){if(tick-event.tick<=18)this.impact(state,def);}
      else state.impact={def,at:state.bolt?null:event.tick};
    }
    return true;
  }
  bolt(event,def){return {def,position:new Vector3(...event.position),direction:new Vector3(...event.direction),offset:new Vector3(...event.offset),speed:event.speed,life:event.life,merge:event.merge,flown:0,drawn:0,networkFlightTick:event.launchTick,executionTick:event.executionTick,visualMuzzle:this.muzzle?()=>this.muzzle(event,def):null};}
  impact(state,def){
    const point=new Vector3(...state.position),name=state.surface===4?def.ImpactEffectWater??'watersplash_ord':state.surface===0?def.ImpactEffectTerrain:state.surface===2?def.ImpactEffectSoft||def.ImpactEffectStatic:state.surface===3?def.ImpactEffectRigid||def.ImpactEffectStatic:def.ImpactEffectStatic||def.ImpactEffectTerrain;
    const play=()=>{if(name)this.ordnance.effects.play(name,point);emitSound(state.surface===4?'splash':'impact',def,point,state.surface===0);};
    // An Enhanced sniper shot plays its impact when its drawn bolt gets
    // there (sniper-trace.js), not before it.
    if(this.ordnance.deferSniperImpact?.(def,point,play))return;
    play();
  }
  landedBolt(event,def){
    const start=this.muzzle?.(event,def);if(!start||!(def.Velocity>0))return undefined;
    const direction=new Vector3(...event.position).sub(start),distance=direction.length();if(!(distance>1e-3))return undefined;
    return {def,position:start,direction:direction.divideScalar(distance),offset:new Vector3(),speed:def.Velocity,life:distance/def.Velocity,merge:Math.min(8,distance),flown:0,drawn:0,networkFlightTick:event.launchTick,visualMuzzle:null};
  }
  launchPresentation(event,def){
    const tick=this.tick;
    // Held rounds have no trigger edge ID. Use the tick the weapon fired,
    // shared with the shot presentation event, not a rewound flight tick or
    // a later impact/sample tick. Every pellet and update claims one cosmetic.
    const executionTick=event.executionTick??event.launchTick;
    const predicted=!!this.predictedShots?.claim({playerId:event.ownerId,generation:event.ownerGeneration,fireId:event.fireId,channel:event.channel},{tick:executionTick});
    const flashKey=`${event.ownerId}:${event.ownerGeneration}:${event.fireId}:${executionTick}:${event.channel}:${event.definitionId}`;
    if(tick-executionTick<=LAUNCH_PRESENTATION_TICKS&&!this.flashes.has(flashKey)){this.flashes.set(flashKey,executionTick+LAUNCH_PRESENTATION_TICKS);this.onLaunch(event,def,predicted);}
    return predicted;
  }
  present(tick){this.tick=Math.max(this.tick??0,tick);this.presentedTick=Math.max(this.presentedTick??0,tick);this.prune(this.presentedTick);this.dynamic.present(tick);
    for(const state of this.records.values())if(state.pending)state.pending=this.onPresentation(state)===false;
    for(const state of this.records.values()){
      if(state.explosion&&tick>=state.explosion.at){const at=new Vector3(...state.position);this.ordnance.presentExplosion(state.explosion.def,at,state.wet);this.onExplosion(state.explosion.def,at);state.explosion=null;}
      const bolt=state.bolt;
      if(bolt){
        if(!bolt.started){
          // A launch this clock has already passed (a lag-compensated shot
          // rewound further back than this page's playout delay, or a late
          // or repeated delivery) starts from the gun now, up to
          // LAUNCH_PRESENTATION_TICKS after the tick it was fired on, not
          // mid-flight or not at all.
          bolt.started=true;
          if(tick>bolt.networkFlightTick)bolt.networkFlightTick=Math.min(tick,Math.max(bolt.networkFlightTick,bolt.executionTick??0)+LAUNCH_PRESENTATION_TICKS);
        }
        if(bolt.visualMuzzle){
          // Resolved at its first draw, against the shooter's body as drawn
          // now; a muzzle far from the launch (a respawn, a seat) is ignored.
          const muzzle=bolt.visualMuzzle();bolt.visualMuzzle=null;
          if(muzzle){bolt.offset.copy(muzzle).sub(bolt.position);if(bolt.offset.lengthSq()>400)bolt.offset.set(0,0,0);}
        }
        this.ordnance.drawBolt(bolt,1,tick);
      }
      if(state.impact){
        const at=state.impact.at??(bolt?.started?bolt.networkFlightTick+bolt.life*60:null);
        if(at!==null&&tick>=at){if(tick-at<=18)this.impact(state,state.impact.def);state.impact=null;}
      }
    }}
}
// A replicated bolt's flight, cut where its terminal says it ended.
function reach(bolt,point){if(bolt.speed>0)bolt.life=Math.min(bolt.life,Math.max(0,(point[0]-bolt.position.x)*bolt.direction.x+(point[1]-bolt.position.y)*bolt.direction.y+(point[2]-bolt.position.z)*bolt.direction.z)/bolt.speed);}

export function attachCombatCapture(game,capture){
  const ordnance=game.ordnance,launch=ordnance.onLaunch,impact=ordnance.onImpact,explode=ordnance.onExplode,projectile=ordnance.onProjectile;
  capture.dynamic=new ProjectileCapture(capture,ordnance);
  const observe=fn=>{try{fn();}catch(error){capture.fault(error);}};
  const onLaunch=(bolt,context)=>{launch?.(bolt,context);observe(()=>capture.launch(bolt,context));},onImpact=(bolt,hit)=>{impact?.(bolt,hit);observe(()=>capture.impact(bolt,hit));};
  const onExplode=(definition,position,owner,context)=>{explode?.(definition,position,owner,context);observe(()=>capture.explosion(definition,position,context));};
  const onProjectile=(p,type)=>{projectile?.(p,type);observe(()=>capture.dynamic.launch(p,type));};
  ordnance.onProjectile=onProjectile;ordnance.onLaunch=onLaunch;ordnance.onImpact=onImpact;ordnance.onExplode=onExplode;
  const unlisten=game.simEvents.listen(event=>observe(()=>capture.event(event)));
  return()=>{if(ordnance.onLaunch===onLaunch)ordnance.onLaunch=launch;if(ordnance.onImpact===onImpact)ordnance.onImpact=impact;if(ordnance.onExplode===onExplode)ordnance.onExplode=explode;if(ordnance.onProjectile===onProjectile)ordnance.onProjectile=projectile;capture.dynamic=null;unlisten();};
}
