// Exact flyer motion replay state (schema2). Ground vehicles are deliberately
// unsupported until their wheel/support/contact state has its own audit.
// Dependency audit: stepFlight, Vehicle.fly/tick/captureTick and flight targeting.
// Excluded collision rays/matrices are overwritten before use by sweep/landingSite/
// syncBody; class geometry/config and the world registry come from loaded content.
export const FLIGHT_CORRECTION_SCHEMA=2;
const numbers='health team yaw previousYaw tilt previousTilt roll previousRoll yawRate forwardInput rightInput altitude throttle airspeed tiltRate stateTime floorY bank power stickX stickY flightRollRate trickTime trickCooldown trickPending trickDirection gear landingMessageTime flightClock flightAssistId flightLockId flightLockGeneration flightLockTimer flightLostLOS tickFromYaw tickFromTilt tickFromRoll'.split(' ');
const vectors='position previous velocity tickFrom flightStart flightEnd flightTrickRight'.split(' ');
const quaternions='quaternion flightPreviousQ flightTickQ flightStartQ flightTrickLoopQ flightTrickEndQ'.split(' ');
const booleans='alive sleeping grounded rollLock boosting boostExhausted terrainWarning flightActionHeld trickHeld immelmannHeld'.split(' ');
const messages=['','Takeoff path blocked','Landing path blocked','Too fast','No landing zone','Press SPACE to land','Flight ceiling','Leaving battlefield'];
const enums={flightState:['Landed','TakingOff','Flying','Landing','Destroyed'],trick:['','barrel','loop','immelmann'],flightLockState:['','Locking','Locked'],flightWarning:messages,landingMessage:messages,landingReason:messages};
const seatNumbers=['aimYaw','pitch','lookYaw','lookPitch','yawRate','lastYaw'];
export const flightCorrectionBytes=numbers.length*8+vectors.length*24+quaternions.length*32+booleans.length+Object.keys(enums).length+seatNumbers.length*9;
export function captureFlightCorrection(vehicle){
 if(!vehicle.flying||!vehicle.flightConfig||!vehicle.seats?.[0])throw Error('unsupported vehicle correction');
 const bytes=new Uint8Array(flightCorrectionBytes),view=new DataView(bytes.buffer);let at=0;
 const number=(v,allowNaN=false)=>{if(!Number.isFinite(v)&&!(allowNaN&&Number.isNaN(v)))throw Error('nonfinite flight correction');view.setFloat64(at,v,true);at+=8;};
 for(const key of numbers)number(vehicle[key],key==='floorY');
 for(const key of [...vectors,...quaternions]){const v=vehicle[key];if(!(vectors.includes(key)?v?.isVector3:v?.isQuaternion))throw Error(`missing flight vector ${key}`);for(const x of v.toArray())number(x);}
 for(const key of booleans){const value=vehicle[key];if(value!==undefined&&typeof value!=='boolean')throw Error(`invalid flight boolean ${key}`);bytes[at++]=value===undefined?2:Number(value);}
 for(const [key,values]of Object.entries(enums)){const index=values.indexOf(vehicle[key]);if(index<0)throw Error(`invalid flight ${key}: ${vehicle[key]}`);bytes[at++]=index;}
 for(const key of seatNumbers){const value=vehicle.seats[0][key];bytes[at++]=value===undefined?2:value===null?1:0;number(value??0);}
 return bytes;
}
export function restoreFlightCorrection(vehicle,payload,schema=FLIGHT_CORRECTION_SCHEMA){
 if(schema!==FLIGHT_CORRECTION_SCHEMA||!vehicle.flying||!vehicle.flightConfig||!(payload instanceof Uint8Array)||payload.length!==flightCorrectionBytes)throw Error('unsupported flight correction');
 const values={},view=new DataView(payload.buffer,payload.byteOffset,payload.byteLength);let at=0;
 const number=(allowNaN=false)=>{const v=view.getFloat64(at,true);at+=8;if(!Number.isFinite(v)&&!(allowNaN&&Number.isNaN(v)))throw Error('nonfinite flight correction');return v;};
 for(const key of numbers)values[key]=number(key==='floorY');
 for(const key of [...vectors,...quaternions]){if(!(vectors.includes(key)?vehicle[key]?.isVector3:vehicle[key]?.isQuaternion))throw Error(`missing flight vector ${key}`);values[key]=Array.from({length:vectors.includes(key)?3:4},()=>number());}
 for(const key of booleans){const n=payload[at++];if(n>2)throw Error('invalid flight boolean');values[key]=n===2?undefined:!!n;}
 for(const [key,items]of Object.entries(enums)){const n=payload[at++];if(n>=items.length)throw Error('invalid flight enum');values[key]=items[n];}
 const seat={};for(const key of seatNumbers){const kind=payload[at++],value=number();if(kind>2)throw Error('invalid flight seat field');seat[key]=kind===2?undefined:kind===1?null:value;}
 if(!vehicle.seats?.[0])throw Error('missing pilot seat');
 for(const key of [...numbers,...booleans,...Object.keys(enums)])vehicle[key]=values[key];
 for(const key of [...vectors,...quaternions])vehicle[key].fromArray(values[key]);
 Object.assign(vehicle.seats[0],seat);
 // Do not syncBody here: client replay must not move the authoritative collider.
}
