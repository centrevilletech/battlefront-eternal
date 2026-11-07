import { wireInputFor } from './wire-input.js';
import {NetProtocol} from './network-layout.js';
// Fixed registries are part of entity-state-v1, not separately negotiated features.
export const INPUT_CODES=Object.freeze([...Array.from({length:26},(_,i)=>`Key${String.fromCharCode(65+i)}`),...Array.from({length:10},(_,i)=>`Digit${i}`),...Array.from({length:12},(_,i)=>`F${i+1}`),...Array.from({length:10},(_,i)=>`Numpad${i}`),'Space','Enter','Escape','Tab','Backquote','CapsLock','ShiftLeft','ShiftRight','ControlLeft','ControlRight','AltLeft','AltRight','ArrowUp','ArrowDown','ArrowLeft','ArrowRight','Backspace','Delete','Insert','Home','End','PageUp','PageDown','Minus','Equal','BracketLeft','BracketRight','Backslash','Semicolon','Quote','Comma','Period','Slash','NumpadEnter','NumpadAdd','NumpadSubtract','NumpadMultiply','NumpadDivide','NumpadDecimal','Mouse0','Mouse1','Mouse2','Mouse3','Mouse4','WheelUp','WheelDown']);
export const DEFAULT_BINDING_PROFILES=new Map([[0,Object.freeze({})]]);
// An action travels as its index here. nextSpawn keeps its place for that:
// no key sends it any more and the world ignores it (the 2004 game has no
// key to move between command posts; the spawn screen picks the post).
export const INPUT_ACTIONS=Object.freeze(['jump','crouch','prone','reload','use','nextPrimary','previousPrimary','nextSecondary','zoom','perspective','map','nextSpawn','stats','squadMoveOut','squadHold','squadFollow','squadAtEase','takeoff','trick','immelmann','targetLock']);
const MAGIC=NetProtocol.INPUT_MAGIC,LIMIT=1100;
class Fields{
 constructor(bytes){this.bytes=bytes;this.view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);this.at=0;}
 reserve(size){if(this.at+size>this.bytes.length)throw Error('input packet exceeds bounds');const at=this.at;this.at+=size;return at;}
 u8(){return this.view.getUint8(this.reserve(1));}
 u16(){return this.view.getUint16(this.reserve(2),true);}
 u32(){return this.view.getUint32(this.reserve(4),true);}
 f64(){return this.view.getFloat64(this.reserve(8),true);}
}
const copyBindings=bindings=>Object.fromEntries(Object.entries(bindings).map(([action,codes])=>[action,Array.isArray(codes)&&codes.every(code=>typeof code==='string')?codes.slice():structuredClone(codes)]));
function copyDecodedView(view){
 // readView has constructed these fixed shapes. Commands must own their nested
 // values because prediction and input consumers may mutate them independently.
 const copy={...view};
 if(view.flightSettings)copy.flightSettings={...view.flightSettings,bindings:copyBindings(view.flightSettings.bindings)};
 if(view.gamepad)copy.gamepad={axes:view.gamepad.axes.slice(),buttons:view.gamepad.buttons.map(button=>({pressed:button.pressed}))};
 return copy;
}
function readView(r,registry){const f=r.u16(),profile=r.u16();if(f>1023||!registry.bindingProfiles.has(profile))throw Error('invalid input view');const v={invertMouse:!!(f&1),firstPerson:!!(f&2)};if(f&32)v.mouseScale=r.f64();if(f&64)v.aspect=r.f64();if(f&128)v.viewTick=r.f64();if(f&4)v.flightSettings={invert:!!(f&8),bindings:copyBindings(registry.bindingProfiles.get(profile))};if(f&256){if(!v.flightSettings)throw Error('invalid flight view');v.flightSettings.sensitivity=r.f64();}if(f&512){if(!v.flightSettings)throw Error('invalid flight view');v.flightSettings.curve=r.f64();}if(f&16){const axes=Array.from({length:4},()=>r.f64()),b=r.u16();v.gamepad={axes,buttons:Array.from({length:16},(_,i)=>({pressed:!!(b&(1<<i))}))};}if(Object.values(v).some(x=>typeof x==='number'&&!Number.isFinite(x))||v.flightSettings&&Object.values(v.flightSettings).some(x=>typeof x==='number'&&!Number.isFinite(x))||v.viewTick!==undefined&&(v.viewTick<0||v.viewTick>0xffffffff)||v.mouseScale!==undefined&&(v.mouseScale<=0||v.mouseScale>2)||v.aspect!==undefined&&(v.aspect<=0||v.aspect>10)||v.flightSettings?.sensitivity!==undefined&&(v.flightSettings.sensitivity<=0||v.flightSettings.sensitivity>10)||v.flightSettings?.curve!==undefined&&(v.flightSettings.curve<.1||v.flightSettings.curve>4)||v.gamepad?.axes.some(x=>!Number.isFinite(x)||Math.abs(x)>1))throw Error('invalid input view range');return v;}
const registryDefault={inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES};
// Edges have explicit IDs independent of movement sequence. Human spawn uses
// class/post registry IDs; post=0 means world-selected spawn.
export function encodeEntityInput(packet,registry=registryDefault,borrowed=false){return wireInputFor(registry).encode(packet,registry,INPUT_ACTIONS,borrowed);}
// A spawn-screen or spectating client with no input still acknowledges
// snapshots: 10 bytes, magic + epoch + tick, every 100 ms.
export function encodeSnapshotAckKeepalive(value={},registry=null,borrowed=false){return wireInputFor(registry).keepalive(value,borrowed);}
export function decodeSnapshotAckKeepalive(bytes){if(!(bytes instanceof Uint8Array)||bytes.length!==10)throw Error('invalid snapshot ack keepalive');const r=new Fields(bytes);if(r.u32()!==NetProtocol.ACK_MAGIC)throw Error('wrong snapshot ack protocol');return {epoch:r.u16(),tick:r.u32()};}
export function isSnapshotAckKeepalive(bytes){return bytes instanceof Uint8Array&&bytes.length===10&&new DataView(bytes.buffer,bytes.byteOffset,4).getUint32(0,true)===NetProtocol.ACK_MAGIC;}
export function decodeEntityInput(bytes,registry=registryDefault){if(!(bytes instanceof Uint8Array)||bytes.length>LIMIT)throw Error('input packet exceeds bounds');const r=new Fields(bytes);if(r.u32()!==MAGIC)throw Error('wrong input protocol');const epoch=r.u32(),generation=r.u32(),nv=r.u8(),nc=r.u8(),ne=r.u8(),na=r.u8();if(nv>8||nc>8||ne>32||na>1)throw Error('input count limit');const views=Array.from({length:nv},()=>readView(r,registry)),commands=[],edges=[];for(let n=0;n<nc;n++){const sequence=r.u32(),tick=r.u32(),vi=r.u8(),down=[];if(!views[vi])throw Error('invalid view reference');for(let byte=0;byte<16;byte++){const b=r.u8();for(let bit=0;bit<8;bit++)if(b&(1<<bit)){const code=registry.inputCodes[byte*8+bit];if(!code)throw Error('unknown held input');down.push(code);}}const lookTotal=[r.f64(),r.f64()];if(!lookTotal.every(Number.isFinite))throw Error('invalid cumulative look');commands.push({epoch,generation,sequence,tick,down,lookTotal,actions:[],look:[],...(views[vi].flightSettings||views[vi].gamepad?copyDecodedView(views[vi]):views[vi])});}
 for(let n=0;n<ne;n++){const id=r.u32(),tick=r.u32(),type=r.u8(),kind=['action','fire','team','spawn','suicide','cancel'][type];if(!id||!kind)throw Error('invalid edge');const e={id,tick,epoch,generation,kind};if(type===0){e.action=INPUT_ACTIONS[r.u8()];if(!e.action)throw Error('unknown action');}else if(type===1){const flags=r.u8();if(flags>3)throw Error('invalid fire flags');Object.assign(e,{button:flags&1?'fireSecondary':'firePrimary',pressed:!!(flags&2),viewTick:r.f64(),aimYaw:r.f64(),aimPitch:r.f64()});if(!Number.isFinite(e.viewTick)||e.viewTick<0||e.viewTick>0xffffffff||!Number.isFinite(e.aimYaw)||!Number.isFinite(e.aimPitch))throw Error('invalid fire aim');}else if(type===2){e.team=r.u8();if(e.team<1||e.team>7)throw Error('invalid team');}else if(type===3){e.classId=r.u16();e.postId=r.u16();}edges.push(e);}
 const snapshotAck=na?{epoch:r.u16(),tick:r.u32()}:null;if(r.at!==bytes.length)throw Error('trailing input bytes');return {epoch,generation,commands,edges,snapshotAck};}
