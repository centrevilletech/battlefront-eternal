import {EntityRemoteCapture} from './network-entity-remotes.js';
import {EntityItemCapture} from './network-entity-items.js';
import {captureOwnPlayer} from './network-entity-player.js';
import {chargeStep} from '../weapons.js';
import {ENTITY_POSTURES,ENTITY_SOLDIER_STATES} from './network-entity-codec.js';
export {ENTITY_POSTURES,ENTITY_SOLDIER_STATES} from './network-entity-codec.js';
// Dying: a destroyed walker playing out its death animation (Vehicle.dying);
// its record's power carries the milliseconds of it still to play.
export const ENTITY_VEHICLE_STATES=Object.freeze(['Landed','TakingOff','Flying','Landing','Destroyed','Ground','Dying']);
const enumerate=(values,name)=>{const index=values.indexOf(name);if(index<0)throw Error(`unsupported entity state ${name}`);return index;};
const reverse=map=>{const result=new Map();for(const[id,name]of map){if(result.has(name))throw Error('ambiguous entity registry');result.set(name,id);}return result;};
export class EntityCaptureUnsupported extends Error {
 constructor(id,reason){super(reason);this.name='EntityCaptureUnsupported';this.entityId=id;}
}
const boundedHealth=value=>{if(!Number.isFinite(value)||value>65535)throw Error('entity health outside uint16 representation');return Math.round(Math.max(0,value));};
const required=(map,name)=>{const id=map.get(name);if(id===undefined)throw Error(`entity registry missing ${name}`);return id;};
export function captureEntitySoldier(unit,{tick,classId,weaponId=0,fireCounter=0}){
 const s=unit.soldier,state=unit.dead?(s.air==='thrown'?'thrownDead':'dead'):s.dive?(s.dive>0?'diveRight':'diveLeft'):s.air||'ground';
 return {kind:'soldier',id:unit.id,generation:unit.generation,tick,classId,team:unit.team,position:s.position.toArray(),velocity:s.velocity.toArray(),yaw:s.aimYaw,pitch:s.aimPitch,health:boundedHealth(unit.combat.health),posture:enumerate(ENTITY_POSTURES,s.posture),state:enumerate(ENTITY_SOLDIER_STATES,state),weapon:weaponId,fireCounter,charge:unit.dead?0:chargeStep(unit.combat.arsenal?.current(0))};
}
export function captureEntityVehicle(vehicle,{tick,classId}){
 return {kind:'vehicle',id:vehicle.aiTarget.id,generation:vehicle.aiTarget.generation,tick,classId,team:vehicle.team,position:vehicle.position.toArray(),velocity:vehicle.velocity.toArray(),quaternion:vehicle.quaternion.toArray(),health:boundedHealth(vehicle.health),state:enumerate(ENTITY_VEHICLE_STATES,vehicle.dying>0?'Dying':vehicle.flightState||'Ground'),
  power:vehicle.dying>0?Math.min(65535,Math.ceil(vehicle.dying*1000)):Math.round(vehicle.power||0)};
}
// A hull's crew pose: the lean its physics gave it, the walk cycle a walker is
// on, the slot that manufactures it (so a client can build an unseen class) and
// every seat's aim. Presentation and seat inputs only; the hull's exact replay
// state stays in the pilot's correction.
export function captureEntityMount(vehicle,{tick,slot=-1}){
 return {kind:'mount',id:vehicle.aiTarget.id,generation:vehicle.aiTarget.generation,tick,slot:slot>=0&&slot<255?slot:null,tilt:vehicle.tilt||0,roll:vehicle.roll||0,walkPhase:vehicle.walkPhase||0,walkBlend:vehicle.walkBlend||0,seats:(vehicle.seats||[]).map(seat=>({yaw:seat.aimYaw||0,pitch:seat.pitch||0}))};
}
// A stationary armed building as its units-registry target, posed by the drum
// and gun angles the tick left. A turret without a registry target (never
// attached to the units registry) cannot be named by a seat and is skipped.
export function captureEntityTurret(turret,{tick,building=-1}){
 const target=turret.aiTarget;if(!target||building<0||building>65535||!['unbuilt','built','destroyed'].includes(turret.state))return null;
 return {kind:'turret',id:target.id,generation:target.generation,tick,building,state:turret.state,team:Math.max(0,Math.min(255,target.team||0)),health:Math.round(Math.max(0,Math.min(65535,Number.isFinite(turret.health)?turret.health:65535))),yaw:turret.yaw||0,pitch:turret.pitch||0,gunYaw:turret.gunYaw||0,gunPitch:turret.gunPitch||0};
}
// Fixed actor roster capture. World summaries use network-entity-summary.js;
// projectile/effect lifecycle coverage remains separate work.
export class EntityWorldCapture {
 constructor(game,registry,{maxEntities=256,fireCounter=()=>0}={}){this.game=game;this.classes=reverse(registry.classes);this.weapons=reverse(registry.weapons);this.maxEntities=maxEntities;this.fireCounter=fireCounter;this.known=new Map();this.seats=new Map();this.leaseSerial=0;this.unsupported=[];this.items=game.items?new EntityItemCapture(game.items,registry.combat):null;this.remotes=new EntityRemoteCapture(game,registry.combat);}
 capture(tick){
  const live=new Map(),records=[...(this.items?.capture(tick)??[]),...this.remotes.capture(tick)];this.unsupported=[];
  for(const unit of this.game.units.all()){
   if(!unit.soldier||!unit.combat||unit.removed)continue;
   const weapon=unit.combat.arsenal.current(0),record=captureEntitySoldier(unit,{tick,classId:required(this.classes,unit.className),weaponId:weapon?required(this.weapons,weapon.name):0,fireCounter:this.fireCounter(unit)});
   if(unit.walker){const d=unit.soldier;Object.assign(record,{kind:'droideka',mode:d.mode,reverse:d.reverse,dead:d.dead,shieldOn:!!d.shield?.on,shield:d.shield?.value||0,time:d.time,roll:d.roll,lean:d.lean,legs:d.legs});}
   live.set(unit.id,record);records.push(record);
   // A seat names its mount by registry id: a hull or a stationary turret.
   // The same lease record covers both; the client resolves the id.
   const lease=unit.seat,mount=(lease?.vehicle||lease?.turret)?.aiTarget||null;
   const signature=`${unit.generation}:${mount?.id||0}:${mount?.generation||0}:${lease?.index||0}`,old=this.seats.get(unit.id);
   if(mount||old){if(old?.signature!==signature)this.seats.set(unit.id,{signature,record:{kind:'seat',id:unit.id,generation:unit.generation,tick,vehicleId:mount?.id||0,vehicleGeneration:mount?.generation||0,seatIndex:lease?.index||0,leaseVersion:++this.leaseSerial}});records.push(this.seats.get(unit.id).record);}
  }
  const system=this.game.vehicles;
  // A dying walker stays (its record says so); its crew pose has nothing to say.
  for(const vehicle of system?.vehicles||[]){if(!vehicle.alive&&!(vehicle.dying>0))continue;const record=captureEntityVehicle(vehicle,{tick,classId:required(this.classes,vehicle.name)});live.set(record.id,record);records.push(record);if(vehicle.alive)records.push(captureEntityMount(vehicle,{tick,slot:system.slots?.indexOf(vehicle.slot)??-1}));}
  for(const turret of this.game.turretSeats?.turrets||[]){const record=captureEntityTurret(turret,{tick,building:this.game.live?.buildings?.indexOf(turret)??-1});if(record)records.push(record);}
  for(const[id,old]of this.known)if(!live.has(id)){
   const tombstone=old.kind==='tombstone'?old:{kind:'tombstone',id,generation:old.generation,tick};records.push(tombstone);live.set(id,tombstone);
  }
  if(live.size>this.maxEntities)throw Error('semantic fixed-roster capacity exceeded');
  this.known=live;for(const id of this.seats.keys())if(!live.has(id)||live.get(id).kind==='tombstone')this.seats.delete(id);return records;
 }
}
export function capturePlayerCorrection(game,world,playerId,{tick=game.worldClock.tick,epoch=1,registry,bindingProfileId=0}={}){
 const player=world.players.get(playerId);if(!player?.unit||player.spectator)throw Error('own spectator correction not implemented');
 const unit=player.unit;
 // Authenticated content registries already contain the reverse index. Custom
 // callers may still supply only the forward table; never cache mutable tables.
 const vehicle=unit.seat?.index===0?unit.seat.vehicle||null:null,classId=required(registry.classIds??reverse(registry.classes),unit.className);
 return captureOwnPlayer({unit,vehicle,vehicleId:vehicle?.aiTarget.id||0,vehicleGeneration:vehicle?.aiTarget.generation||0,tick,epoch,registry,classId,bindingProfileId,held:player.mailbox.down,view:player.mailbox.view,discardedInput:player.mailbox.discarded||0});
}
// Unsupported own actor classes remain explicit. Supported infantry, recon,
// vehicle and stationary turret controls always receive an exact own checkpoint.
export function tryCapturePlayerCorrection(game,world,playerId,options){
 try{return capturePlayerCorrection(game,world,playerId,options);}
 catch(error){if(error instanceof EntityCaptureUnsupported)return null;throw error;}
}
