import {captureArsenalWeapon,validateArsenalWeapon} from './network-arsenal-state.js';
import {Vector3} from 'three';
class Fields {
 constructor(bytes=new Uint8Array(900)){this.bytes=bytes;this.view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);this.at=0;}
 n(type,value,provided){const size={Uint8:1,Uint16:2,Uint32:4,Float64:8}[type];if(this.at+size>this.bytes.length)throw Error('recon correction overflow/truncation');const at=this.at;this.at+=size;if(provided)this.view['set'+type](at,value,true);else return this.view['get'+type](at,true);}
 u8(value){if(arguments.length&&(!Number.isInteger(value)||value<0||value>255))throw Error('invalid recon byte');return this.n('Uint8',value,arguments.length);}
 u16(value){if(arguments.length&&(!Number.isInteger(value)||value<0||value>65535))throw Error('invalid recon integer');return this.n('Uint16',value,arguments.length);}
 raw(bytes){if(this.at+bytes.length>this.bytes.length)throw Error('recon correction overflow/truncation');this.bytes.set(bytes,this.at);this.at+=bytes.length;}
 take(n){if(this.at+n>this.bytes.length)throw Error('recon correction overflow/truncation');const result=this.bytes.slice(this.at,this.at+n);this.at+=n;return result;}
 u32(value){if(arguments.length&&(!Number.isSafeInteger(value)||value<0||value>0xffffffff))throw Error('invalid recon integer');return this.n('Uint32',value,arguments.length);}
 f(value,kind='finite'){if(arguments.length){if(value===null&&kind==='nullable')value=NaN;if(!Number.isFinite(value)&&!(kind==='infinite'&&value===Infinity)&&!(kind==='nullable'&&Number.isNaN(value)))throw Error('invalid recon number');this.n('Float64',value,true);}else return this.n('Float64');}
 read(kind='finite'){const value=this.f();if(kind==='nullable'&&Number.isNaN(value))return null;if(!Number.isFinite(value)&&!(kind==='infinite'&&value===Infinity))throw Error('invalid recon number');return value;}
}
const base=['yaw','pitch','health','countdown','lookX','lookY'];
function writeVector(w,array,length){if(!Array.isArray(array)||array.length!==length)throw Error('invalid recon vector');for(const n of array)w.f(n);}
function readVector(r,length){return Array.from({length},()=>r.read());}
// This block is independent of the own-player envelope. References such as the
// droid definition and slot/generation belong to its enclosing control mode.
export function encodeRemoteCorrection(state,droid){
 if(state.weapons.length>2||state.weapons.length!==droid.arsenal.weapons.length||state.selected.length!==2)throw Error('unsupported recon arsenal');
 if(!Number.isInteger(state.zoomStep)||state.zoomStep<0||state.zoomStep>1)throw Error('invalid recon zoom');for(let channel=0;channel<2;channel++)if(!Number.isInteger(state.selected[channel])||state.selected[channel]<0||state.selected[channel]>=Math.max(1,droid.arsenal.channels[channel].length))throw Error('invalid recon selected weapon');
 if(typeof state.dead!=='boolean'||typeof state.piloted!=='boolean'||state.dead&&state.piloted)throw Error('invalid recon control state');
 const w=new Fields();w.u8(2);w.u8(Number(state.dead)|(Number(state.piloted)<<1)|(Number(Boolean(state.firstPerson))<<2));for(const key of base)w.f(state[key]);w.u8(state.zoomStep);w.u32(state.randomSeed);
 for(const key of ['position','velocity'])writeVector(w,state[key],3);for(const n of state.selected)w.u8(n);w.u8(state.weapons.length);
 for(const [index,stateWeapon] of state.weapons.entries()){
  const weapon=droid.arsenal.weapons[index];if(!weapon.levels[stateWeapon.level])throw Error('invalid recon weapon level');
  const payload=captureArsenalWeapon({...weapon,...stateWeapon,def:weapon.levels[stateWeapon.level]});w.u16(payload.length);w.raw(payload);
 }
 w.u8(state.salvo?1:0);if(state.salvo){const index=droid.arsenal.weapons.findIndex(weapon=>weapon.levels.includes(state.salvo.def));if(index<0||!state.salvo.def.strike)throw Error('recon salvo definition missing');if(state.salvo.left>Math.max(1,Math.floor(state.salvo.def.SalvoCount||1)))throw Error('invalid recon salvo count');w.u8(index);w.u8(droid.arsenal.weapons[index].levels.indexOf(state.salvo.def));w.u32(state.salvo.left);w.f(state.salvo.wait);for(const key of ['at','x','y','z'])writeVector(w,state.salvo[key].toArray(),3);}
 w.u8(state.camera?1:0);if(state.camera){if(typeof state.camera.fresh!=='boolean'||!(state.camera.aspect>0)||!Array.isArray(state.camera.quaternion)||Math.abs(Math.hypot(...state.camera.quaternion)-1)>.001)throw Error('invalid recon camera');writeVector(w,state.camera.position,3);writeVector(w,state.camera.quaternion,4);for(const key of ['aspect','fov','reach'])w.f(state.camera[key]);w.u8(Number(state.camera.fresh));}
 return w.bytes.slice(0,w.at);
}
export function decodeRemoteCorrection(bytes,droid){
 if(!(bytes instanceof Uint8Array))throw Error('invalid recon correction');const r=new Fields(bytes);if(r.u8()!==2)throw Error('unsupported recon correction');const bits=r.u8();if(bits>7)throw Error('invalid recon state flags');const state={dead:Boolean(bits&1),piloted:Boolean(bits&2),firstPerson:Boolean(bits&4)};if(state.dead&&state.piloted)throw Error('invalid recon control state');for(const key of base)state[key]=r.read();state.zoomStep=r.u8();if(state.zoomStep>1)throw Error('invalid recon zoom');state.randomSeed=r.u32();for(const key of ['position','velocity'])state[key]=readVector(r,3);state.selected=[r.u8(),r.u8()];const count=r.u8();if(count>2||count!==droid.arsenal.weapons.length)throw Error('recon arsenal mismatch');state.weapons=[];
 for(let i=0;i<count;i++)state.weapons.push(validateArsenalWeapon(droid.arsenal.weapons[i],r.take(r.u16())).state);
 for(let channel=0;channel<2;channel++)if(state.selected[channel]>=Math.max(1,droid.arsenal.channels[channel].length))throw Error('invalid recon selected weapon');
 const salvo=r.u8();if(salvo>1)throw Error('invalid recon salvo flag');state.salvo=null;if(salvo){const index=r.u8(),level=r.u8(),def=droid.arsenal.weapons[index]?.levels[level];if(!def?.strike)throw Error('recon salvo prerequisite missing');state.salvo={def,left:r.u32(),wait:r.read()};if(state.salvo.left>Math.max(1,Math.floor(def.SalvoCount||1)))throw Error('invalid recon salvo count');for(const key of ['at','x','y','z'])state.salvo[key]=new Vector3().fromArray(readVector(r,3));}
 const camera=r.u8();if(camera>1)throw Error('invalid recon camera flag');if(camera){state.camera={position:readVector(r,3),quaternion:readVector(r,4),aspect:r.read(),fov:r.read(),reach:r.read()};const fresh=r.u8();if(fresh>1||state.camera.aspect<=0||Math.abs(Math.hypot(...state.camera.quaternion)-1)>.001)throw Error('invalid recon camera');state.camera.fresh=Boolean(fresh);}
 if(r.at!==bytes.length)throw Error('trailing recon correction bytes');return state;
}
