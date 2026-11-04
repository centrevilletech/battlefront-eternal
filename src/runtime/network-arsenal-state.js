import {Weapon} from '../weapons.js';
import {captureWeaponState,decodeWeaponState,validateWeaponState,applyValidatedWeaponState,WEAPON_STATE_NUMBERS,WEAPON_STATE_FLAGS} from './network-weapon-state.js';

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
// A changed weapon is written here and its bytes sliced out; its cache entry
// is updated in place.
const out=new Uint8Array(2+WEAPON_STATE_NUMBERS.length*8+14),outView=new DataView(out.buffer);
// The fields are read by name, in WEAPON_STATE_FLAGS and WEAPON_STATE_NUMBERS
// order, with the same checks: reads by key (27 names a weapon, every
// checkpoint) were most of the work.
if(WEAPON_STATE_FLAGS.join()!=='deployed,firing,overheated,released,hidden,charging,lockComplete'
  ||WEAPON_STATE_NUMBERS.join()!=='timer,clip,reserve,heat,kick,spreadScale,sniperScale,yawSpreadOverride,wait,reloadLeft,throwTime,held,charge,pattern,salvosLeft,salvoWait,lockTargetId,lockGeneration,lockElapsed,limit')
  throw Error('weapon state fields changed: update captureArsenalWeapon');
const flag=(value,bit)=>{if(typeof value!=='boolean')throw Error('invalid weapon state flag');return Number(value)<<bit;};
const finite=value=>{if(!Number.isFinite(value))throw Error('invalid weapon state number');return value;};
export function captureArsenalWeapon(weapon,native=null){
  if(native)return native.arsenal(weapon,()=>baseValues(weapon));
  const level=weapon.levels.indexOf(weapon.def);
  if(level<0||level>255)throw Error('weapon state definition mismatch');
  const w=weapon;
  const flags=flag(w.deployed,0)|flag(w.firing,1)|flag(w.overheated,2)|flag(w.released,3)|flag(w.hidden,4)|flag(w.charging,5)|flag(w.lockComplete,6);
  scratch[0]=finite(w.timer);scratch[1]=finite(w.clip);
  const reserve=w.reserve;if(!Number.isFinite(reserve)&&reserve!==Infinity)throw Error('invalid weapon state number');scratch[2]=reserve;
  scratch[3]=finite(w.heat);scratch[4]=finite(w.kick);scratch[5]=finite(w.spreadScale);scratch[6]=finite(w.sniperScale);
  const override=w.yawSpreadOverride;if(!Number.isFinite(override)&&override!==null)throw Error('invalid weapon state number');scratch[7]=override===null?Number.NaN:override;
  scratch[8]=finite(w.wait);scratch[9]=finite(w.reloadLeft);scratch[10]=finite(w.throwTime);scratch[11]=finite(w.held);scratch[12]=finite(w.charge);
  scratch[13]=finite(w.pattern);scratch[14]=finite(w.salvosLeft);scratch[15]=finite(w.salvoWait);scratch[16]=finite(w.lockTargetId);
  scratch[17]=finite(w.lockGeneration);scratch[18]=finite(w.lockElapsed);scratch[19]=finite(w.limit);
  const melee=weapon.melee?weapon.melee.captureState():null;
  const cached=captureCache.get(weapon);
  let same=!!cached&&cached.level===level&&cached.flags===flags;
  for(let i=0;same&&i<scratch.length;i++)same=Object.is(cached.values[i],scratch[i]);
  if(same
    &&(melee===null?cached.melee===null:cached.melee!==null&&Object.is(cached.melee.time,melee.time)&&cached.melee.index===melee.index&&cached.melee.active===melee.active))return cached.bytes;
  const base=baseValues(weapon),view=outView;
  out[0]=level;out[1]=flags;let mask=0,at=6;
  for(let i=0;i<WEAPON_STATE_NUMBERS.length;i++){
    if(Object.is(scratch[i],base[i]))continue;
    mask|=1<<i;view.setFloat64(at,scratch[i],true);at+=8;
  }
  view.setUint32(2,mask,true);
  if(melee){view.setFloat64(at,melee.time,true);out[at+8]=melee.index;out[at+9]=Number(melee.active);at+=10;}
  const bytes=out.slice(0,at);
  if(cached){cached.level=level;cached.flags=flags;cached.values.set(scratch);cached.melee=melee?{...melee}:null;cached.bytes=bytes;}
  else captureCache.set(weapon,{level,flags,values:Float64Array.from(scratch),melee:melee?{...melee}:null,bytes});
  return bytes;
}
export function validateArsenalWeapon(weapon,bytes){
  if(!(bytes instanceof Uint8Array)||bytes.length<6)throw Error('invalid arsenal weapon state');
  const mask=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength).getUint32(2,true);
  if(mask>>>WEAPON_STATE_NUMBERS.length)throw Error('invalid arsenal weapon mask');
  const full=initial(weapon).slice();full.set(bytes.subarray(0,2));let at=6;
  for(let i=0;i<WEAPON_STATE_NUMBERS.length;i++)if(mask&(1<<i)){
    if(at+8>bytes.length)throw Error('truncated arsenal weapon state');
    full.set(bytes.subarray(at,at+8),2+i*8);at+=8;
  }
  let melee;
  if(weapon.melee){
    if(at+10!==bytes.length)throw Error('invalid melee weapon state');
    const time=new DataView(bytes.buffer,bytes.byteOffset+at,10).getFloat64(0,true),index=bytes[at+8],active=bytes[at+9];
    if(!Number.isFinite(time)||time< -1||index>=Math.min(3,weapon.def.swings?.length||1)||active>1)throw Error('invalid melee weapon state');
    melee={time,index,active:!!active};at+=10;
  }
  if(at!==bytes.length)throw Error('trailing arsenal weapon state');
  return {...validateWeaponState(weapon,decodeWeaponState(full)),melee};
}
export function applyArsenalWeapon(entry){applyValidatedWeaponState(entry);if(entry.melee)entry.weapon.melee.restoreState(entry.melee);}
export function captureArsenalState(arsenal){
  return {selected:arsenal.selected.slice(),weapons:arsenal.weapons.map(weapon=>({weapon,state:decodeWeaponState(captureWeaponState(weapon)),melee:weapon.melee?.captureState()}))};
}
export function restoreArsenalState(arsenal,state){
  arsenal.selected=state.selected.slice();
  for(const entry of state.weapons)applyArsenalWeapon(entry);
}
