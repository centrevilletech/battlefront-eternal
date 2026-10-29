// Exact flyer state with conditional transition/trick history. Targeting,
// collision velocity and telemetry remain exact; presentation history is local.
export const COMPACT_FLIGHT_SCHEMA = 7;
const numbers = 'health team yaw tilt roll yawRate forwardInput rightInput altitude throttle airspeed tiltRate stateTime floorY bank power stickX stickY flightRollRate trickCooldown trickPending gear landingMessageTime flightClock flightAssistId flightLockId flightLockGeneration flightLockTimer flightLostLOS'.split(' ');
const booleans = 'alive sleeping grounded rollLock boosting boostExhausted terrainWarning flightActionHeld trickHeld immelmannHeld'.split(' ');
const messages = ['', 'Takeoff path blocked', 'Landing path blocked', 'Too fast', 'No landing zone', 'Press SPACE to land', 'Flight ceiling', 'Leaving battlefield'];
const enums = {flightState:['Landed','TakingOff','Flying','Landing','Destroyed'],trick:['','barrel','loop','immelmann'],flightLockState:['','Locking','Locked'],flightWarning:messages,landingMessage:messages,landingReason:messages};
const seatNumbers = ['aimYaw','pitch','lookYaw','lookPitch','yawRate','lastYaw'];
function layout(state, trick) {
 const transition = state === 'TakingOff' || state === 'Landing';
 return {
  numbers: [...numbers, ...(trick ? ['trickTime','trickDirection'] : [])],
  vectors: ['position','velocity', ...(transition ? ['flightStart','flightEnd'] : []), ...(trick === 'barrel' ? ['flightTrickRight'] : [])],
  quaternions: ['quaternion', ...(transition ? ['flightStartQ'] : []), ...(trick === 'immelmann' ? ['flightTrickLoopQ','flightTrickEndQ'] : [])],
 };
}
const size = l => 6 + l.numbers.length * 8 + l.vectors.length * 24 + l.quaternions.length * 32 + booleans.length + seatNumbers.length * 9;
function prerequisite(v) { if (!v.flying || !v.flightConfig || !v.seats?.[0]) throw Error('unsupported compact flight correction'); }
export function captureCompactFlight(v) {
 prerequisite(v); const l = layout(v.flightState,v.trick), bytes = new Uint8Array(size(l)), view = new DataView(bytes.buffer); let at = 0;
 const number = (n, nan=false) => { if (!Number.isFinite(n) && !(nan && Number.isNaN(n))) throw Error('nonfinite flight correction'); view.setFloat64(at,n,true); at += 8; };
 for (const [key,values] of Object.entries(enums)) { const n=values.indexOf(v[key]);if(n<0)throw Error(`invalid flight ${key}`);bytes[at++]=n; }
 for (const key of l.numbers) number(v[key],key==='floorY');
 for (const key of [...l.vectors,...l.quaternions]) {
  if (!(l.vectors.includes(key)?v[key]?.isVector3:v[key]?.isQuaternion)) throw Error(`missing flight vector ${key}`);
  for (const n of v[key].toArray()) number(n);
 }
 for (const key of booleans) { const n=v[key];if(n!==undefined&&typeof n!=='boolean')throw Error('invalid flight boolean');bytes[at++]=n===undefined?2:Number(n); }
 for (const key of seatNumbers) { const n=v.seats[0][key];bytes[at++]=n===undefined?2:n===null?1:0;number(n??0); }
 return bytes;
}
export function restoreCompactFlight(v,bytes) {
 prerequisite(v);
 if (!(bytes instanceof Uint8Array) || bytes.length<6) throw Error('invalid compact flight payload');
 const values={}, view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);let at=0;
 for(const [key,items]of Object.entries(enums)){const n=bytes[at++];if(n>=items.length)throw Error('invalid flight enum');values[key]=items[n];}
 const l=layout(values.flightState,values.trick);if(bytes.length!==size(l))throw Error('invalid compact flight length');
 const number=(nan=false)=>{const n=view.getFloat64(at,true);at+=8;if(!Number.isFinite(n)&&!(nan&&Number.isNaN(n)))throw Error('nonfinite flight correction');return n;};
 for(const key of l.numbers)values[key]=number(key==='floorY');
 for(const key of [...l.vectors,...l.quaternions]){
  if(!(l.vectors.includes(key)?v[key]?.isVector3:v[key]?.isQuaternion))throw Error(`missing flight vector ${key}`);
  values[key]=Array.from({length:l.vectors.includes(key)?3:4},()=>number());
  if(l.quaternions.includes(key)&&Math.abs(values[key].reduce((sum,n)=>sum+n*n,0)-1)>1e-6)throw Error('invalid flight orientation');
 }
 for(const key of booleans){const n=bytes[at++];if(n>2)throw Error('invalid flight boolean');values[key]=n===2?undefined:!!n;}
 const seat={};for(const key of seatNumbers){const kind=bytes[at++],n=number();if(kind>2)throw Error('invalid flight seat');seat[key]=kind===2?undefined:kind===1?null:n;}
 for(const key of ['previous','tickFrom'])if(!v[key]?.isVector3)throw Error(`missing flight history ${key}`);
 for(const key of ['flightPreviousQ','flightTickQ'])if(!v[key]?.isQuaternion)throw Error(`missing flight history ${key}`);
 for(const key of [...l.numbers,...booleans,...Object.keys(enums)])v[key]=values[key];
 for(const key of [...l.vectors,...l.quaternions])v[key].fromArray(values[key]);
 Object.assign(v.seats[0],seat);
 v.previous.copy(v.position);v.tickFrom.copy(v.position);v.flightPreviousQ.copy(v.quaternion);v.flightTickQ.copy(v.quaternion);
 for(const axis of ['Yaw','Tilt','Roll'])v[`previous${axis}`]=v[`tickFrom${axis}`]=v[axis.toLowerCase()];
 if(!v.trick){v.trickTime=0;v.trickDirection=1;}
 // Inactive transition/trick vectors are intentionally untouched: their start
 // functions replace them before use. Never move the shared collider here.
}
