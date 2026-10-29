// Fixed registries are part of entity-state-v1, not separately negotiated features.
export const INPUT_CODES=Object.freeze([...Array.from({length:26},(_,i)=>`Key${String.fromCharCode(65+i)}`),...Array.from({length:10},(_,i)=>`Digit${i}`),...Array.from({length:12},(_,i)=>`F${i+1}`),...Array.from({length:10},(_,i)=>`Numpad${i}`),'Space','Enter','Escape','Tab','Backquote','CapsLock','ShiftLeft','ShiftRight','ControlLeft','ControlRight','AltLeft','AltRight','ArrowUp','ArrowDown','ArrowLeft','ArrowRight','Backspace','Delete','Insert','Home','End','PageUp','PageDown','Minus','Equal','BracketLeft','BracketRight','Backslash','Semicolon','Quote','Comma','Period','Slash','NumpadEnter','NumpadAdd','NumpadSubtract','NumpadMultiply','NumpadDivide','NumpadDecimal','Mouse0','Mouse1','Mouse2','Mouse3','Mouse4','WheelUp','WheelDown']);
export const DEFAULT_BINDING_PROFILES=new Map([[0,Object.freeze({})]]);
export const INPUT_ACTIONS=Object.freeze(['jump','crouch','prone','reload','use','nextPrimary','previousPrimary','nextSecondary','zoom','perspective','map','nextSpawn','stats','squadMoveOut','squadHold','squadFollow','squadAtEase','takeoff','trick','immelmann','targetLock']);
const MAGIC=0x31494e42,LIMIT=1100;
class Fields{
 constructor(bytes=new Uint8Array(LIMIT)){this.bytes=bytes;this.view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);this.at=0;}
 // Scalar access is the input decoder's hot loop. Direct methods avoid a
 // temporary rest array and dynamic DataView method lookup for every field.
 reserve(size){if(this.at+size>this.bytes.length)throw Error('input packet exceeds bounds');const at=this.at;this.at+=size;return at;}
 u8(value){const at=this.reserve(1);if(!arguments.length)return this.view.getUint8(at);checkUint(value,255);this.view.setUint8(at,value);}
 u16(value){const at=this.reserve(2);if(!arguments.length)return this.view.getUint16(at,true);checkUint(value,65535);this.view.setUint16(at,value,true);}
 u32(value){const at=this.reserve(4);if(!arguments.length)return this.view.getUint32(at,true);checkUint(value,0xffffffff);this.view.setUint32(at,value,true);}
 f64(value){const at=this.reserve(8);if(!arguments.length)return this.view.getFloat64(at,true);if(!Number.isFinite(value))throw Error('invalid input scalar');this.view.setFloat64(at,value,true);}
}
const checkUint=(value,max)=>{if(!Number.isInteger(value)||value<0||value>max)throw Error('invalid input scalar');};
const bindingKey=value=>JSON.stringify(Object.entries(value||{}).sort(([a],[b])=>a.localeCompare(b)));
const viewKeys=['mouseScale','aspect','viewTick','invertMouse','firstPerson','flightSettings','gamepad'];
const context=command=>Object.fromEntries(viewKeys.filter(k=>command[k]!==undefined).map(k=>[k,command[k]]));
const copyBindings=bindings=>Object.fromEntries(Object.entries(bindings).map(([action,codes])=>[action,Array.isArray(codes)&&codes.every(code=>typeof code==='string')?codes.slice():structuredClone(codes)]));
function copyDecodedView(view){
 // readView has constructed these fixed shapes. Commands must own their nested
 // values because prediction and input consumers may mutate them independently.
 const copy={...view};
 if(view.flightSettings)copy.flightSettings={...view.flightSettings,bindings:copyBindings(view.flightSettings.bindings)};
 if(view.gamepad)copy.gamepad={axes:view.gamepad.axes.slice(),buttons:view.gamepad.buttons.map(button=>({pressed:button.pressed}))};
 return copy;
}
function writeView(w,v,registry){
 const fs=v.flightSettings,profile=[...registry.bindingProfiles].find(([,b])=>bindingKey(b)===bindingKey(fs?.bindings));if(!profile)throw Error('unknown input binding profile');
 let flags=(v.invertMouse?1:0)|(v.firstPerson?2:0)|(fs?4:0)|(fs?.invert?8:0)|(v.gamepad?16:0);for(const[k,bit]of [['mouseScale',32],['aspect',64],['viewTick',128]])if(v[k]!==undefined)flags|=bit;if(fs?.sensitivity!==undefined)flags|=256;if(fs?.curve!==undefined)flags|=512;
 w.u16(flags);w.u16(profile[0]);if(flags&32)w.f64(v.mouseScale);if(flags&64)w.f64(v.aspect);if(flags&128)w.f64(v.viewTick);if(flags&256)w.f64(fs.sensitivity);if(flags&512)w.f64(fs.curve);if(flags&16){for(let i=0;i<4;i++)w.f64(v.gamepad.axes?.[i]??0);let buttons=0;for(let i=0;i<16;i++)if(v.gamepad.buttons?.[i]?.pressed)buttons|=1<<i;w.u16(buttons);}
}
function readView(r,registry){const f=r.u16(),profile=r.u16();if(f>1023||!registry.bindingProfiles.has(profile))throw Error('invalid input view');const v={invertMouse:!!(f&1),firstPerson:!!(f&2)};if(f&32)v.mouseScale=r.f64();if(f&64)v.aspect=r.f64();if(f&128)v.viewTick=r.f64();if(f&4)v.flightSettings={invert:!!(f&8),bindings:copyBindings(registry.bindingProfiles.get(profile))};if(f&256){if(!v.flightSettings)throw Error('invalid flight view');v.flightSettings.sensitivity=r.f64();}if(f&512){if(!v.flightSettings)throw Error('invalid flight view');v.flightSettings.curve=r.f64();}if(f&16){const axes=Array.from({length:4},()=>r.f64()),b=r.u16();v.gamepad={axes,buttons:Array.from({length:16},(_,i)=>({pressed:!!(b&(1<<i))}))};}if(Object.values(v).some(x=>typeof x==='number'&&!Number.isFinite(x))||v.flightSettings&&Object.values(v.flightSettings).some(x=>typeof x==='number'&&!Number.isFinite(x))||v.viewTick!==undefined&&(v.viewTick<0||v.viewTick>0xffffffff)||v.mouseScale!==undefined&&(v.mouseScale<=0||v.mouseScale>2)||v.aspect!==undefined&&(v.aspect<=0||v.aspect>10)||v.flightSettings?.sensitivity!==undefined&&(v.flightSettings.sensitivity<=0||v.flightSettings.sensitivity>10)||v.flightSettings?.curve!==undefined&&(v.flightSettings.curve<.1||v.flightSettings.curve>4)||v.gamepad?.axes.some(x=>!Number.isFinite(x)||Math.abs(x)>1))throw Error('invalid input view range');return v;}
const registryDefault={inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES};
// Edges have explicit IDs independent of movement sequence. Human spawn uses
// class/post registry IDs; post=0 means world-selected spawn.
export function encodeEntityInput({epoch,generation,commands=[],edges=[],snapshotAck=null},registry=registryDefault){
 if(commands.length>8||edges.length>32)throw Error('input count limit');
 if(snapshotAck!==null&&(!Number.isSafeInteger(snapshotAck.epoch)||snapshotAck.epoch<0||snapshotAck.epoch>65535||!Number.isSafeInteger(snapshotAck.tick)||snapshotAck.tick<0||snapshotAck.tick>0xffffffff))throw Error('invalid snapshot ack');
 const w=new Fields(),views=[],indices=[];
 for(const c of commands){const v=context(c),s=JSON.stringify(v);let i=views.findIndex(x=>x.s===s);if(i<0){i=views.length;views.push({s,v});}indices.push(i);}
 w.u32(MAGIC);w.u32(epoch);w.u32(generation);w.u8(views.length);w.u8(commands.length);w.u8(edges.length);w.u8(snapshotAck?1:0);for(const{v}of views)writeView(w,v,registry);
 for(let n=0;n<commands.length;n++){const c=commands[n];if(c.epoch!==undefined&&c.epoch!==epoch||c.generation!==undefined&&c.generation!==generation)throw Error('mixed input life');w.u32(c.sequence);w.u32(c.tick);w.u8(indices[n]);const bits=new Uint8Array(16);for(const code of c.down||[]){const i=registry.inputCodes.indexOf(code);if(i<0||i>=128)throw Error('unknown input code');bits[i>>3]|=1<<(i&7);}for(const b of bits)w.u8(b);w.f64(c.lookTotal?.[0]??0);w.f64(c.lookTotal?.[1]??0);}
 for(const e of edges){if(e.epoch!==undefined&&e.epoch!==epoch||e.generation!==undefined&&e.generation!==generation)throw Error('mixed edge life');w.u32(e.id);w.u32(e.tick);const type=['action','fire','team','spawn','suicide','cancel'].indexOf(e.kind);if(type<0)throw Error('unknown input edge');w.u8(type);if(type===0){const i=INPUT_ACTIONS.indexOf(e.action);if(i<0)throw Error('unknown input action');w.u8(i);}else if(type===1){if(!['firePrimary','fireSecondary'].includes(e.button)||typeof e.pressed!=='boolean')throw Error('invalid fire edge');w.u8((e.button==='fireSecondary'?1:0)|(e.pressed?2:0));w.f64(e.viewTick);w.f64(e.aimYaw);w.f64(e.aimPitch);}else if(type===2){if(e.team<1||e.team>7)throw Error('invalid team');w.u8(e.team);}else if(type===3){w.u16(e.classId);w.u16(e.postId||0);}}
 if(snapshotAck){w.u16(snapshotAck.epoch);w.u32(snapshotAck.tick);}const result=w.bytes.slice(0,w.at);decodeEntityInput(result,registry);return result;
}
// A spawn-screen or spectating client with no input still acknowledges
// snapshots: 10 bytes, magic + epoch + tick, every 100 ms.
export const SNAPSHOT_ACK_MAGIC=0x31414e42; // 'BNA1'
export function encodeSnapshotAckKeepalive({epoch,tick}={}){const w=new Fields(new Uint8Array(10));w.u32(SNAPSHOT_ACK_MAGIC);w.u16(epoch);w.u32(tick);return w.bytes.slice(0,10);}
export function decodeSnapshotAckKeepalive(bytes){if(!(bytes instanceof Uint8Array)||bytes.length!==10)throw Error('invalid snapshot ack keepalive');const r=new Fields(bytes);if(r.u32()!==SNAPSHOT_ACK_MAGIC)throw Error('wrong snapshot ack protocol');return {epoch:r.u16(),tick:r.u32()};}
export function isSnapshotAckKeepalive(bytes){return bytes instanceof Uint8Array&&bytes.length===10&&new DataView(bytes.buffer,bytes.byteOffset,4).getUint32(0,true)===SNAPSHOT_ACK_MAGIC;}
export function decodeEntityInput(bytes,registry=registryDefault){if(!(bytes instanceof Uint8Array)||bytes.length>LIMIT)throw Error('input packet exceeds bounds');const r=new Fields(bytes);if(r.u32()!==MAGIC)throw Error('wrong input protocol');const epoch=r.u32(),generation=r.u32(),nv=r.u8(),nc=r.u8(),ne=r.u8(),na=r.u8();if(nv>8||nc>8||ne>32||na>1)throw Error('input count limit');const views=Array.from({length:nv},()=>readView(r,registry)),commands=[],edges=[];for(let n=0;n<nc;n++){const sequence=r.u32(),tick=r.u32(),vi=r.u8(),down=[];if(!views[vi])throw Error('invalid view reference');for(let byte=0;byte<16;byte++){const b=r.u8();for(let bit=0;bit<8;bit++)if(b&(1<<bit)){const code=registry.inputCodes[byte*8+bit];if(!code)throw Error('unknown held input');down.push(code);}}const lookTotal=[r.f64(),r.f64()];if(!lookTotal.every(Number.isFinite))throw Error('invalid cumulative look');commands.push({epoch,generation,sequence,tick,down,lookTotal,actions:[],look:[],...(views[vi].flightSettings||views[vi].gamepad?copyDecodedView(views[vi]):views[vi])});}
 for(let n=0;n<ne;n++){const id=r.u32(),tick=r.u32(),type=r.u8(),kind=['action','fire','team','spawn','suicide','cancel'][type];if(!id||!kind)throw Error('invalid edge');const e={id,tick,epoch,generation,kind};if(type===0){e.action=INPUT_ACTIONS[r.u8()];if(!e.action)throw Error('unknown action');}else if(type===1){const flags=r.u8();if(flags>3)throw Error('invalid fire flags');Object.assign(e,{button:flags&1?'fireSecondary':'firePrimary',pressed:!!(flags&2),viewTick:r.f64(),aimYaw:r.f64(),aimPitch:r.f64()});if(!Number.isFinite(e.viewTick)||e.viewTick<0||e.viewTick>0xffffffff||!Number.isFinite(e.aimYaw)||!Number.isFinite(e.aimPitch))throw Error('invalid fire aim');}else if(type===2){e.team=r.u8();if(e.team<1||e.team>7)throw Error('invalid team');}else if(type===3){e.classId=r.u16();e.postId=r.u16();}edges.push(e);}
 const snapshotAck=na?{epoch:r.u16(),tick:r.u32()}:null;if(r.at!==bytes.length)throw Error('trailing input bytes');return {epoch,generation,commands,edges,snapshotAck};}
