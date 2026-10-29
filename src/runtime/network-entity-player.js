import {captureDroidekaState,restoreDroidekaState,DROIDEKA_BODY_SCHEMA} from './network-droideka-state.js';
import {NETWORK_CONTRACT} from './network-contract.js';
import {captureMountedControl,decodeMountedControl,validateMountedControl,applyValidatedMountedControl,VEHICLE_PILOT_CONTROL} from './network-mounted-control.js';
import {reconRandom} from './network-remote-world.js';
import {Vector3} from 'three';
import {Arsenal} from '../weapons.js';
import {captureArsenalWeapon,validateArsenalWeapon,applyArsenalWeapon} from './network-arsenal-state.js';
import {captureRemoteReplay,restoreRemoteReplay} from './network-remote-replay.js';
import {encodeRemoteCorrection,decodeRemoteCorrection} from './network-remote-codec.js';
import {captureGroundCorrection,restoreGroundCorrection,GROUND_CORRECTION_SCHEMA} from './network-entity-ground.js';
import {captureCompactSoldier,restoreCompactSoldier,COMPACT_SOLDIER_SCHEMA} from './network-entity-soldier-compact.js';
import {captureCompactFlight,restoreCompactFlight,COMPACT_FLIGHT_SCHEMA} from './network-entity-flight-compact.js';
import {checkCorrectionRecordFits} from './network-entity-codec.js';
import {PlayerCombat} from '../combat.js';
import {emitSound} from '../audio/events.js';
// One entity-state-v1 correction envelope. Content, key-code and binding-profile
// registries must already match the authenticated world's content epoch.
export const OWN_PLAYER_SCHEMA=NETWORK_CONTRACT.ownCorrectionSchema;
export class OwnPlayerPrerequisiteError extends Error {}
const requireState=(value,message)=>{if(!value)throw new OwnPlayerPrerequisiteError(message);};
const uint=(v,max)=>{if(!Number.isSafeInteger(v)||v<0||v>max)throw Error('invalid own-player integer');return v;};
class Fields {
 constructor(bytes=new Uint8Array(1100)){this.bytes=bytes;this.view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);this.at=0;}
 field(type,value){const size={Uint8:1,Uint16:2,Uint32:4,Float64:8}[type];if(this.at+size>this.bytes.length)throw Error('own-player envelope exceeds budget');const at=this.at;this.at+=size;if(arguments.length===1)return this.view['get'+type](at,true);this.view['set'+type](at,value,true);}
 u8(v){return arguments.length?this.field('Uint8',uint(v,255)):this.field('Uint8');}u16(v){return arguments.length?this.field('Uint16',uint(v,65535)):this.field('Uint16');}u32(v){return arguments.length?this.field('Uint32',uint(v,0xffffffff)):this.field('Uint32');}
 number(value,infinite=false){const v=arguments.length?value:this.field('Float64');if(!Number.isFinite(v)&&!(infinite&&v===Infinity))throw Error('invalid own-player number');if(arguments.length)this.field('Float64',v);return v;}
 readNumber(infinite=false){const value=this.field('Float64');if(!Number.isFinite(value)&&!(infinite&&value===Infinity))throw Error('invalid own-player number');return value;}
 raw(bytes){if(this.at+bytes.length>this.bytes.length)throw Error('own-player envelope exceeds budget');this.bytes.set(bytes,this.at);this.at+=bytes.length;}
 take(n){if(this.at+n>this.bytes.length)throw Error('truncated own-player payload');const bytes=this.bytes.slice(this.at,this.at+n);this.at+=n;return bytes;}
}
// Registries are content-fixed once built; validating the input-code list
// (Set construction included) on every checkpoint was pure overhead at
// 64 clients x 30 Hz. Validation still runs for each distinct array.
const validatedInputCodes=new WeakSet();
function registryChecks(registry,contentEpoch,classId,unit){
 requireState(registry?.contentEpoch===contentEpoch,'content epoch mismatch');
 requireState(registry.classes?.get(classId)===unit.className,'player class prerequisite missing');
 const codes=registry.inputCodes;
 if(Array.isArray(codes)&&validatedInputCodes.has(codes))return;
 requireState(Array.isArray(codes)&&codes.length<=128&&codes.every(code=>typeof code==='string'&&code.length<=64)&&new Set(codes).size===codes.length,'invalid input-code registry');
 validatedInputCodes.add(codes);
}
// Binding objects reach checkpoints as fresh decode copies or fixed
// registry profiles and are never mutated in place, so the sorted-JSON
// comparison form can be cached per object instead of rebuilt per capture.
const canonicalCache=new WeakMap();
const canonical=value=>{
 if(!value||typeof value!=='object')return JSON.stringify(Object.entries(value||{}).sort(([a],[b])=>a.localeCompare(b)));
 let text=canonicalCache.get(value);
 if(text===undefined){text=JSON.stringify(Object.entries(value).sort(([a],[b])=>a.localeCompare(b)));canonicalCache.set(value,text);}
 return text;
};
function weaponId(registry,weapon){
 if(registry.weaponIds){const id=registry.weaponIds.get(weapon.name);requireState(id!==undefined&&registry.weapons.get(id)===weapon.name,'weapon definition prerequisite missing');return id;}
 for(const[id,name]of registry.weapons)if(name===weapon.name)return id;
 throw new OwnPlayerPrerequisiteError('weapon definition prerequisite missing');
}
export function captureOwnPlayer({unit,vehicle=null,vehicleId=0,vehicleGeneration=0,tick,epoch,registry,classId,bindingProfileId=0,held=[],view={},discardedInput=0}){
 requireState(Number.isInteger(unit.team)&&unit.team>=1&&unit.team<=2,'invalid own team');registryChecks(registry,registry.contentEpoch,classId,unit);const mounted=unit.seat?captureMountedControl(unit):null;requireState(vehicle?unit.seat?.vehicle===vehicle&&unit.seat.index===0:!unit.seat||mounted,'pilot lease prerequisite missing');requireState(vehicle?vehicleId>0&&vehicleGeneration>0:vehicleId===0&&vehicleGeneration===0,'vehicle lease prerequisite missing');const combat=unit.combat,arsenal=combat.arsenal;
 requireState(Boolean(vehicle)===(mounted?.[0]===VEHICLE_PILOT_CONTROL),'pilot control mode mismatch');
 requireState(!vehicle||mounted&&vehicle.aiTarget?.id===vehicleId&&vehicle.aiTarget.generation===vehicleGeneration,'pilot weapon identity mismatch');
 requireState(registry.bindingProfiles?.has(bindingProfileId),'binding profile prerequisite missing');
 requireState(canonical(view.flightSettings?.bindings)===canonical(registry.bindingProfiles.get(bindingProfileId)),'binding profile mismatch');
 const body=vehicle?(vehicle.flying?captureCompactFlight(vehicle):captureGroundCorrection(vehicle)):unit.walker?captureDroidekaState(unit.soldier):captureCompactSoldier(unit.soldier),bodySchema=vehicle?(vehicle.flying?COMPACT_FLIGHT_SCHEMA:GROUND_CORRECTION_SCHEMA):unit.walker?DROIDEKA_BODY_SCHEMA:COMPACT_SOLDIER_SCHEMA,w=new Fields();
 w.u32(epoch);w.u32(registry.contentEpoch);w.u16(classId);w.u8(uint(unit.team,2));w.u8(bodySchema);w.u32(vehicleId);w.u32(vehicleGeneration);w.u16(body.length);w.raw(body);
 // Both channels and every inventory slot form the weapon replay checkpoint.
 w.number(combat.health);w.u8(combat.dead?1:0);w.number(combat.deathTime??0);w.number(unit.deadTime??0);w.u8(unit.removed?1:0);for(const key of ['rate','hold','offset'])w.number(combat.kick[key]);w.u8(combat.zoomStep);w.number(combat.zoom);w.u8((combat.firstPerson?1:0)|(combat.gunUnderwater?2:0));
 for(let channel=0;channel<2;channel++)w.u8(arsenal.current(channel)?uint(arsenal.selected[channel],254):255);
 w.u8(arsenal.weapons.length);
 for(const weapon of arsenal.weapons){const state=captureArsenalWeapon(weapon);w.u16(weaponId(registry,weapon));w.u16(state.length);w.raw(state);}
 const bits=new Uint8Array(16);for(const code of held){const index=registry.inputCodes.indexOf(code);requireState(index>=0,'held code missing from registry');bits[index>>3]|=1<<(index&7);}w.raw(bits);
 const fields=['mouseScale','aspect','viewTick'],fs=view.flightSettings;
 let flags=(view.invertMouse?1:0)|(view.firstPerson?2:0)|(fs?4:0)|(fs?.invert?8:0)|(view.gamepad?16:0);
 for(let i=0;i<fields.length;i++)if(view[fields[i]]!==undefined)flags|=1<<(5+i);
 if(fs?.sensitivity!==undefined)flags|=256;if(fs?.curve!==undefined)flags|=512;if(fs?.bindings!==undefined)flags|=1024;
 w.u16(flags);for(const field of fields)if(view[field]!==undefined)w.number(view[field]);
 w.u16(bindingProfileId);if(flags&256)w.number(fs.sensitivity);if(flags&512)w.number(fs.curve);
 if(view.gamepad){for(let i=0;i<4;i++)w.number(view.gamepad.axes?.[i]??0);let buttons=0;for(let i=0;i<16;i++)if(view.gamepad.buttons?.[i]?.pressed)buttons|=1<<i;w.u16(buttons);}
 w.u32(discardedInput);
 const remotes=combat.remotes,droid=remotes?.piloted;requireState(!(droid&&mounted),'conflicting own control modes');w.u8(droid?1:mounted?mounted[0]:0);if(mounted){w.u16(mounted.length);w.raw(mounted);}if(droid){requireState(!vehicle&&typeof remotes.identityFor==='function','recon identity prerequisite missing');const identity=remotes.identityFor(droid);requireState(identity.id<4096,'recon slot out of range');w.u16(identity.id);w.u32(identity.generation);w.u16(registry.combat.definitionId(droid.def));const payload=encodeRemoteCorrection(captureRemoteReplay(remotes,droid),droid);w.u16(payload.length);w.raw(payload);}
 const record={kind:'correction',id:unit.id,generation:unit.generation,tick,schema:OWN_PLAYER_SCHEMA,payload:w.bytes.slice(0,w.at)};
 // Both the record and its mandatory containing message must fit. No fragments.
 // The check reserves the enclosing packet header and length, as encoding did.
 checkCorrectionRecordFits(record.payload);return record;
}
export function decodeOwnPlayer(record){
 if(record.kind!=='correction'||record.schema!==OWN_PLAYER_SCHEMA||!(record.payload instanceof Uint8Array))throw Error('unsupported own-player correction');
 const r=new Fields(record.payload),result={epoch:r.u32(),contentEpoch:r.u32(),classId:r.u16(),team:r.u8(),bodySchema:r.u8(),vehicleId:r.u32(),vehicleGeneration:r.u32()};if(result.team<1||result.team>2)throw Error('invalid own team');result.body=r.take(r.u16());
 result.health=r.readNumber();const dead=r.u8();result.deathTime=r.readNumber();result.unitDeadTime=r.readNumber();const removed=r.u8();if(dead>1||removed>1)throw Error('invalid life flags');result.dead=!!dead;result.removed=!!removed;result.kick={};for(const key of ['rate','hold','offset'])result.kick[key]=r.readNumber();result.zoomStep=r.u8();result.zoom=r.readNumber();const first=r.u8();result.firstPerson=!!(first&1);result.gunUnderwater=!!(first&2);if(first>3)throw Error('invalid weapon view flags');
 result.selected=[r.u8(),r.u8()];result.weapons=Array.from({length:r.u8()},()=>({id:r.u16(),payload:r.take(r.u16())}));
 result.heldBits=r.take(16);const flags=r.u16();if(flags>2047)throw Error('unknown own-player flags');const view={invertMouse:!!(flags&1),firstPerson:!!(flags&2)};
 if(flags&32)view.mouseScale=r.readNumber();if(flags&64)view.aspect=r.readNumber();if(flags&128)view.viewTick=r.readNumber();result.bindingProfileId=r.u16();
 if(flags&4)view.flightSettings={invert:!!(flags&8)};if(flags&256){if(!view.flightSettings)throw Error('invalid flight context');view.flightSettings.sensitivity=r.readNumber();}if(flags&512){if(!view.flightSettings)throw Error('invalid flight context');view.flightSettings.curve=r.readNumber();}
 result.hasBindings=!!(flags&1024);if(result.hasBindings&&!view.flightSettings)throw Error('invalid flight bindings context');
 if(flags&16){const axes=Array.from({length:4},()=>r.readNumber()),buttons=r.u16();view.gamepad={axes,buttons:Array.from({length:16},(_,i)=>({pressed:!!(buttons&(1<<i))}))};}
 result.discardedInput=r.u32();const remote=r.u8();if(remote>4)throw Error('invalid own control mode');result.controlMode=remote;result.mounted=null;if(remote>=2){result.mounted=r.take(r.u16());if(decodeMountedControl(result.mounted).mode!==remote)throw Error('mounted control mode mismatch');}result.remote=null;if(remote===1){result.remote={id:r.u16(),generation:r.u32(),definitionId:r.u16()};if(result.remote.id>=4096||!result.remote.generation)throw Error('invalid recon identity');result.remote.payload=r.take(r.u16());}
 if(r.at!==record.payload.length)throw Error('trailing own-player payload');
 if(view.viewTick!==undefined&&(!Number.isFinite(view.viewTick)||view.viewTick<0||view.viewTick>0xffffffff))throw Error('invalid replay view tick');
 if(view.mouseScale!==undefined&&view.mouseScale<=0||view.aspect!==undefined&&view.aspect<=0||view.flightSettings?.sensitivity!==undefined&&(view.flightSettings.sensitivity<=0||view.flightSettings.sensitivity>10)||view.flightSettings?.curve!==undefined&&(view.flightSettings.curve<.1||view.flightSettings.curve>4)||view.gamepad?.axes.some(x=>Math.abs(x)>1))throw Error('invalid replay view');
 result.view=view;return result;
}
// His own hurt and death online, as combat.damage() reports them offline:
// the authority's health falling within one life, once per newer tick.
const heard=new WeakMap();
function hearOwn(unit,record,data){
 const was=heard.get(unit);
 if(was&&was.generation===record.generation&&record.tick>was.tick&&!was.dead){
  if(data.dead)emitSound('death',unit.combat,Math.max(0,was.health-data.health));
  else if(data.health<was.health)emitSound('hurt',unit.combat,was.health-data.health);
 }
 if(!was||was.generation!==record.generation||record.tick>was.tick)heard.set(unit,{generation:record.generation,tick:record.tick,health:data.health,dead:data.dead});
}
export function applyOwnPlayer({unit,vehicle=null,vehicleId=0,vehicleGeneration=0,epoch,registry,record,previous=null,remoteLifecycle=null}){
 const data=decodeOwnPlayer(record);let mounted=null;if(data.mounted){requireState(Boolean(vehicle)===(data.controlMode===VEHICLE_PILOT_CONTROL),'conflicting mounted vehicle body');try{mounted=validateMountedControl(unit,data.mounted);}catch(error){throw new OwnPlayerPrerequisiteError(error.message);}}requireState(vehicle?unit.seat?.vehicle===vehicle&&unit.seat.index===0:!unit.seat||mounted,'pilot lease prerequisite missing');requireState(data.epoch===epoch,'world epoch mismatch');registryChecks(registry,data.contentEpoch,data.classId,unit);
 requireState(record.id===unit.id&&record.generation===unit.generation,'player life prerequisite missing');
 requireState(!(previous&&previous.epoch===epoch&&previous.id===unit.id&&previous.generation===unit.generation&&record.tick<previous.tick),'stale own-player correction');
 requireState(data.bodySchema===(vehicle?(vehicle.flying?COMPACT_FLIGHT_SCHEMA:GROUND_CORRECTION_SCHEMA):unit.walker?DROIDEKA_BODY_SCHEMA:COMPACT_SOLDIER_SCHEMA)&&data.vehicleId===vehicleId&&data.vehicleGeneration===vehicleGeneration,'vehicle lease prerequisite missing');
 requireState(!vehicle||data.mounted&&vehicle.aiTarget?.id===vehicleId&&vehicle.aiTarget.generation===vehicleGeneration,'pilot weapon identity mismatch');
 requireState(registry.bindingProfiles.has(data.bindingProfileId),'binding profile prerequisite missing');
 const combat=unit.combat,arsenal=combat.arsenal,selected=[];requireState(combat.kick&&typeof combat.kick==='object','combat recoil prerequisite missing');
 requireState(data.weapons.length===arsenal.weapons.length,'weapon inventory prerequisite mismatch');
 const weaponStates=data.weapons.map((state,index)=>{const weapon=arsenal.weapons[index];requireState(registry.weapons.get(state.id)===weapon.name,'weapon prerequisite mismatch');try{return validateArsenalWeapon(weapon,state.payload);}catch(error){throw new OwnPlayerPrerequisiteError(error.message);}});
 for(let channel=0;channel<2;channel++){const index=data.selected[channel];if(index===255){requireState(!arsenal.channels[channel]?.length,'weapon channel prerequisite mismatch');selected.push(null);continue;}const weapon=arsenal.channels[channel]?.[index];requireState(weapon,'weapon prerequisite mismatch');selected.push(weapon);}
 const primary=selected[0],primaryState=primary&&weaponStates.find(entry=>entry.weapon===primary).state,primaryDef=primary?.levels[primaryState?.level];
 requireState(data.zoomStep<(primary?PlayerCombat.zooms(primaryDef).length:1),'invalid zoom step');const zoom=primary&&data.zoomStep?(PlayerCombat.zooms(primaryDef)[data.zoomStep]||1):1;
 requireState(zoom===data.zoom,'zoom definition mismatch');
 const held=[];for(let bit=0;bit<128;bit++)if(data.heldBits[bit>>3]&(1<<(bit&7))){requireState(bit<registry.inputCodes.length,'held registry mismatch');held.push(registry.inputCodes[bit]);}
 if(data.hasBindings)data.view.flightSettings.bindings=structuredClone(registry.bindingProfiles.get(data.bindingProfileId));
 let remoteState=null,remoteDef=null,remoteDroid=null;const remotes=combat.remotes;
 // A later lifecycle packet can retire control before this older body/input
 // checkpoint arrives. Keep the body ACK progressing without reviving a droid.
 const remoteCurrent=!data.remote||!remoteLifecycle||remoteLifecycle.id!==data.remote.id||remoteLifecycle.generation<data.remote.generation||remoteLifecycle.generation===data.remote.generation&&(remoteLifecycle.active||remoteLifecycle.tick<record.tick);
 const effectiveRemote=remoteCurrent?data.remote:null,controlMode=data.remote&&!remoteCurrent?0:data.controlMode;
 if(data.remote){requireState(remotes&&!vehicle,'recon controller prerequisite missing');remoteDef=registry.combat?.definitions.get(data.remote.definitionId);requireState(remoteDef?.base==='droid','recon content prerequisite missing');remoteDroid=remotes.droids.find(d=>d.networkSlot===data.remote.id&&d.networkGeneration===data.remote.generation);requireState(!remoteDroid||remoteDroid.def===remoteDef,'recon definition changed within generation');remoteState=decodeRemoteCorrection(data.remote.payload,{arsenal:new Arsenal((remoteDef.weapons||[]).map(w=>({...w,ammo:w.ammo})))});requireState(remoteState.piloted&&!remoteState.dead,'invalid own recon state');}
 // Each body restore validates its complete payload before mutation. Everything
 // else has been validated above, so a failed prerequisite cannot half-apply.
 if(vehicle){if(vehicle.flying)restoreCompactFlight(vehicle,data.body);else restoreGroundCorrection(vehicle,data.body);}else if(unit.walker)restoreDroidekaState(unit.soldier,data.body);else restoreCompactSoldier(unit.soldier,data.body);
 if(mounted)applyValidatedMountedControl(mounted);
 hearOwn(unit,record,data);
 unit.team=combat.team=data.team;combat.health=data.health;combat.dead=data.dead;combat.deathTime=data.deathTime;unit.deadTime=data.unitDeadTime;unit.removed=data.removed;Object.assign(combat.kick,data.kick);combat.zoomStep=data.zoomStep;combat.firstPerson=data.firstPerson;combat.gunUnderwater=data.gunUnderwater;
 for(let channel=0;channel<2;channel++)arsenal.selected[channel]=data.selected[channel]===255?0:data.selected[channel];
 for(const state of weaponStates)applyArsenalWeapon(state);
 if(remoteState&&remoteCurrent){let droid=remoteDroid;if(!droid){for(const old of remotes.droids)if(old.networkSlot===data.remote.id){if(remotes.piloted===old)remotes.exit();if(old.mesh)remotes.scene.remove(old.mesh);}remotes.droids=remotes.droids.filter(old=>old.networkSlot!==data.remote.id);remotes.strikes=remotes.strikes.filter(old=>old.networkSlot!==data.remote.id);droid=remotes.createDroid(remoteDef,null,new Vector3().fromArray(remoteState.position),new Vector3().fromArray(remoteState.velocity),remoteState.yaw,unit.team);droid.networkSlot=data.remote.id;droid.networkGeneration=data.remote.generation;remotes.droids.push(droid);}if(!remotes.random||!('seed' in remotes.random))remotes.random=reconRandom(0,'prediction');restoreRemoteReplay(remotes,droid,remoteState);}else if(remotes&&data.remote&&!remoteCurrent){
  const stale=droid=>droid.networkSlot===data.remote.id&&droid.networkGeneration<=data.remote.generation;
  for(const droid of remotes.droids)if(stale(droid)){if(remotes.piloted===droid)remotes.exit();if(droid.mesh)remotes.scene.remove(droid.mesh);}
  remotes.droids=remotes.droids.filter(droid=>!stale(droid));remotes.strikes=remotes.strikes.filter(droid=>!stale(droid));
 }else if(remotes){remotes.exit();remotes.strikes.length=0;}
 const mount=data.mounted?decodeMountedControl(data.mounted):null;
 return {team:data.team,controlMode,mountId:mount?.id??0,mountGeneration:mount?.generation??0,mountSeat:mount?.index??0,epoch,contentEpoch:data.contentEpoch,id:unit.id,generation:unit.generation,tick:record.tick,classId:data.classId,vehicleId,vehicleGeneration,remoteId:effectiveRemote?.id??null,remoteGeneration:effectiveRemote?.generation??0,held,view:data.view,discardedInput:data.discardedInput,reset:!previous||previous.team!==data.team||previous.controlMode!==controlMode||previous.mountId!==(mount?.id??0)||previous.mountGeneration!==(mount?.generation??0)||previous.mountSeat!==(mount?.index??0)||previous.epoch!==epoch||previous.contentEpoch!==data.contentEpoch||previous.id!==unit.id||previous.generation!==unit.generation||previous.classId!==data.classId||previous.vehicleId!==vehicleId||previous.vehicleGeneration!==vehicleGeneration||previous.remoteId!==(effectiveRemote?.id??null)||previous.remoteGeneration!==(effectiveRemote?.generation??0)};
}
