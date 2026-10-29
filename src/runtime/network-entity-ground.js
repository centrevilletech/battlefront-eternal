// Exact pilot correction for hover, wheeled and walking ground hulls. Geometry,
// class constants, support probes and seat limits are verified prerequisites.
// Schema 10 adds a hover's pitch and roll rates and ground factor
// (src/vehicle-hover.js).
export const GROUND_CORRECTION_SCHEMA = 10;
const numbers = ['health','team','yaw','tilt','roll','yawRate','forwardInput','rightInput','tiltRate','rollRate','support'];
const seatFields = ['aimYaw','pitch','lookYaw','lookPitch','yawRate','lastYaw'];
export const groundCorrectionBytes = numbers.length*8 + 24*2 + 32 + 2 + seatFields.length*9;
function prerequisite(v) { if(v.flying||!v.seats?.[0]||!v.position?.isVector3||!v.velocity?.isVector3||!v.quaternion?.isQuaternion)throw Error('unsupported ground correction'); }
export function captureGroundCorrection(v) {
 prerequisite(v); const bytes=new Uint8Array(groundCorrectionBytes),view=new DataView(bytes.buffer);let at=0;
 const number=n=>{if(!Number.isFinite(n))throw Error('nonfinite ground correction');view.setFloat64(at,n,true);at+=8;};
 for(const key of numbers)number(v[key]);
 for(const key of ['position','velocity','quaternion'])for(const n of v[key].toArray())number(n);
 for(const key of ['alive','sleeping']){if(typeof v[key]!=='boolean')throw Error(`invalid ground ${key}`);bytes[at++]=Number(v[key]);}
 for(const key of seatFields){const n=v.seats[0][key];bytes[at++]=n===undefined?2:n===null?1:0;number(n??0);}
 return bytes;
}
export function restoreGroundCorrection(v,bytes) {
 prerequisite(v);if(!(bytes instanceof Uint8Array)||bytes.length!==groundCorrectionBytes)throw Error('invalid ground correction length');
 const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),values={},seat={};let at=0;
 const number=()=>{const n=view.getFloat64(at,true);at+=8;if(!Number.isFinite(n))throw Error('nonfinite ground correction');return n;};
 for(const key of numbers)values[key]=number();
 for(const key of ['position','velocity','quaternion'])values[key]=Array.from({length:key==='quaternion'?4:3},number);
 if(Math.abs(values.quaternion.reduce((sum,n)=>sum+n*n,0)-1)>1e-6)throw Error('invalid ground orientation');
 for(const key of ['alive','sleeping']){const n=bytes[at++];if(n>1)throw Error('invalid ground flags');values[key]=!!n;}
 for(const key of seatFields){const kind=bytes[at++],n=number();if(kind>2)throw Error('invalid ground seat');seat[key]=kind===2?undefined:kind===1?null:n;}
 if(!v.previous?.isVector3||!v.tickFrom?.isVector3)throw Error('missing ground presentation history');
 for(const key of [...numbers,'alive','sleeping'])v[key]=values[key];
 for(const key of ['position','velocity','quaternion'])v[key].fromArray(values[key]);
 Object.assign(v.seats[0],seat);v.previous.copy(v.position);v.tickFrom.copy(v.position);
 for(const axis of ['Yaw','Tilt','Roll'])v[`previous${axis}`]=v[`tickFrom${axis}`]=v[axis.toLowerCase()];
 // Suspension/contact scratch is overwritten by tick. Preserve local wheel and
 // walk presentation phase; do not move the authoritative collider during replay.
}
