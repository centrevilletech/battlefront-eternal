// Frozen reference: the own player's checkpoint capture as it was before
// its writer, weapon and soldier captures were sped up (engine 4ffa3a3).
// Copied unchanged: captureArsenalWeapon (network-arsenal-state.js),
// captureCompactSoldier (network-entity-soldier-compact.js), captureOwnPlayer
// with its writer (network-entity-player.js) and capturePlayerCorrection
// (network-entity-capture.js), each calling the copies here. A checkpoint
// must keep its exact bytes; tests/own-checkpoint-identity.test.js and
// tests/snapshot-capture-identity.test.js compare them. Never edit this copy
// to follow a capture change: a deliberate format change replaces the
// identity tests instead.
import {Weapon} from '../../src/weapons.js';
import {captureWeaponState,WEAPON_STATE_NUMBERS,WEAPON_STATE_FLAGS} from '../../src/runtime/network-weapon-state.js';
import {captureDroidekaState,DROIDEKA_BODY_SCHEMA} from '../../src/runtime/network-droideka-state.js';
import {captureMountedControl,VEHICLE_PILOT_CONTROL} from '../../src/runtime/network-mounted-control.js';
import {captureRemoteReplay} from '../../src/runtime/network-remote-replay.js';
import {encodeRemoteCorrection} from '../../src/runtime/network-remote-codec.js';
import {captureGroundCorrection,GROUND_CORRECTION_SCHEMA} from '../../src/runtime/network-entity-ground.js';
import {captureCompactFlight,COMPACT_FLIGHT_SCHEMA} from '../../src/runtime/network-entity-flight-compact.js';
import {checkCorrectionRecordFits} from '../../src/runtime/network-entity-codec.js';
import {OWN_PLAYER_SCHEMA,OwnPlayerPrerequisiteError} from '../../src/runtime/network-entity-player.js';

// network-arsenal-state.js
// Lossless differences from the authenticated definition's initial state.
// Keep float64 timers/ammunition; inactive weapons usually need no numbers.
const defaults=new WeakMap();
function initial(weapon){
  const definition=weapon.levels[0];
  let bytes=defaults.get(definition);
  if(!bytes){bytes=captureWeaponState(new Weapon(definition));defaults.set(definition,bytes);}
  return bytes;
}
const initialValues=new WeakMap();
function baseValues(weapon){
  const definition=weapon.levels[0];
  let values=initialValues.get(definition);
  if(!values){
    const bytes=initial(weapon),view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
    values=new Float64Array(WEAPON_STATE_NUMBERS.length);
    for(let i=0;i<values.length;i++)values[i]=view.getFloat64(2+i*8,true);
    initialValues.set(definition,values);
  }
  return values;
}
// Own-player checkpoints capture every weapon every snapshot, but most
// weapons are idle most of the time. The per-weapon cache re-validates the
// raw fields on every call (same throws as captureWeaponState) and only
// re-serializes when a field actually changed; callers copy the bytes out
// (writer raw()), so returning the cached array cannot alias into a packet.
const captureCache=new WeakMap();
const scratch=new Float64Array(WEAPON_STATE_NUMBERS.length);
export function captureArsenalWeapon(weapon){
  const level=weapon.levels.indexOf(weapon.def);
  if(level<0||level>255)throw Error('weapon state definition mismatch');
  let flags=0;
  for(let i=0;i<WEAPON_STATE_FLAGS.length;i++){
    const value=weapon[WEAPON_STATE_FLAGS[i]];
    if(typeof value!=='boolean')throw Error('invalid weapon state flag');
    flags|=Number(value)<<i;
  }
  for(let i=0;i<WEAPON_STATE_NUMBERS.length;i++){
    const key=WEAPON_STATE_NUMBERS[i],value=weapon[key];
    if(!Number.isFinite(value)&&!(key==='reserve'&&value===Infinity)&&!(key==='yawSpreadOverride'&&value===null))throw Error('invalid weapon state number');
    scratch[i]=value===null?Number.NaN:value;
  }
  const melee=weapon.melee?weapon.melee.captureState():null;
  const cached=captureCache.get(weapon);
  if(cached&&cached.level===level&&cached.flags===flags
    &&cached.values.every((value,i)=>Object.is(value,scratch[i]))
    &&(melee===null?cached.melee===null:cached.melee!==null&&Object.is(cached.melee.time,melee.time)&&cached.melee.index===melee.index&&cached.melee.active===melee.active))return cached.bytes;
  const base=baseValues(weapon),out=new Uint8Array(2+WEAPON_STATE_NUMBERS.length*8+14),view=new DataView(out.buffer);
  out[0]=level;out[1]=flags;let mask=0,at=6;
  for(let i=0;i<WEAPON_STATE_NUMBERS.length;i++){
    if(Object.is(scratch[i],base[i]))continue;
    mask|=1<<i;view.setFloat64(at,scratch[i],true);at+=8;
  }
  view.setUint32(2,mask,true);
  if(melee){view.setFloat64(at,melee.time,true);out[at+8]=melee.index;out[at+9]=Number(melee.active);at+=10;}
  const bytes=out.slice(0,at);
  captureCache.set(weapon,{level,flags,values:Float64Array.from(scratch),melee:melee?{...melee}:null,bytes});
  return bytes;
}
// network-entity-soldier-compact.js
export const COMPACT_SOLDIER_SCHEMA = 6;
const numbers = ['fuel', 'aimYaw', 'aimPitch', 'dive', 'stateTime', 'lock'];
const vectors = ['position', 'velocity', 'normal'];
const postures = ['stand', 'crouch', 'prone'];
const airs = [null, 'jump', 'fall', 'thrown', 'jet'];
export function captureCompactSoldier(s) {
  if (s.contactCount !== 0 || s.contactMean.lengthSq() !== 0) throw Error('Soldier correction requires completed collision pass');
  const settled = !['x','y','z'].every(key => Object.is(s.settled[key], s.position[key])), diving = s.dive !== 0;
  const bytes = new Uint8Array(124 + (settled ? 24 : 0) + (diving ? 1 : 0));
  const view = new DataView(bytes.buffer); let at = 4;
  const number = n => { if (!Number.isFinite(n)) throw Error('nonfinite Soldier correction'); view.setFloat64(at, n, true); at += 8; };
  const posture = postures.indexOf(s.posture), air = airs.indexOf(s.air);
  if (posture < 0 || air < 0 || typeof s.grounded !== 'boolean') throw Error('invalid Soldier state');
  bytes[0] = Number(settled) | (Number(diving) << 1); bytes[1] = posture; bytes[2] = air; bytes[3] = Number(s.grounded);
  for (const key of numbers) number(s[key]);
  for (const key of [...vectors, ...(settled ? ['settled'] : [])]) {
    if (!s[key]?.isVector3) throw Error(`missing Soldier vector ${key}`);
    for (const n of s[key].toArray()) number(n);
  }
  if (diving) { const n = postures.indexOf(s.diveFrom); if (n < 0) throw Error('invalid dive posture'); bytes[at++] = n; }
  return bytes;
}
// network-entity-player.js
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
// network-entity-capture.js
const reverse=map=>{const result=new Map();for(const[id,name]of map){if(result.has(name))throw Error('ambiguous entity registry');result.set(name,id);}return result;};
const required=(map,name)=>{const id=map.get(name);if(id===undefined)throw Error(`entity registry missing ${name}`);return id;};
export function capturePlayerCorrection(game,world,playerId,{tick=game.worldClock.tick,epoch=1,registry,bindingProfileId=0}={}){
 const player=world.players.get(playerId);if(!player?.unit||player.spectator)throw Error('own spectator correction not implemented');
 const unit=player.unit;
 // Authenticated content registries already contain the reverse index. Custom
 // callers may still supply only the forward table; never cache mutable tables.
 const vehicle=unit.seat?.index===0?unit.seat.vehicle||null:null,classId=required(registry.classIds??reverse(registry.classes),unit.className);
 return captureOwnPlayer({unit,vehicle,vehicleId:vehicle?.aiTarget.id||0,vehicleGeneration:vehicle?.aiTarget.generation||0,tick,epoch,registry,classId,bindingProfileId,held:player.mailbox.down,view:player.mailbox.view,discardedInput:player.mailbox.discarded||0});
}
