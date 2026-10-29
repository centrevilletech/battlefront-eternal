// Exact Soldier save/restore payload, separate from quantized remote poses.
// Explicitly mirrors Soldier's own plain fields, including collision scratch;
// immutable class/world objects are supplied by the verified content manifest.
// Flyer correction uses schema2 in network-entity-flight.js; ground vehicles
// remain unsupported. Never substitute a remote visual record for correction.
export const SOLDIER_CORRECTION_SCHEMA=1;
const numbers='maxSpeed maxStrafe maxJump maxTurn maxPitch accel fuel maxFlyHeight getupTime diveTime aimYaw aimPitch dive stateTime lock lookX lookY tickLength contactCount groundAdjustment'.split(' ');
const fullVectors='position previous settled velocity normal contactMean forwardAxis rightAxis left facing pull motion center closest point contactNormal surfaceNormal pushDir clipDir push before origin avoidance stepStart stepTarget stepVelocity stepResolved'.split(' ');
const enums={posture:['stand','crouch','prone'],air:[null,'jump','fall','thrown','jet'],diveFrom:['stand','crouch','prone']};
const booleans=['grounded','stepBlocked'];
export const SOLDIER_REPLAY_SCHEMA=4;
// Solver dependency audit: these seven vectors persist across calls. All other
// vectors are assigned before being read in drive/collide/stepUp/contact; omit
// their scratch values without reducing precision or omitting class scalars.
const replayVectors='position previous settled velocity normal contactMean avoidance'.split(' ');
const byteCount=vectors=>(numbers.length+vectors.length*3)*8+Object.keys(enums).length+booleans.length;
export const soldierCorrectionBytes=byteCount(fullVectors),soldierReplayBytes=byteCount(replayVectors);
const fieldsFor=schema=>{if(schema===SOLDIER_CORRECTION_SCHEMA)return fullVectors;if(schema===SOLDIER_REPLAY_SCHEMA)return replayVectors;throw Error('unsupported soldier correction');};
export function captureSoldierCorrection(soldier,schema=SOLDIER_CORRECTION_SCHEMA){
 const vectors=fieldsFor(schema),bytes=new Uint8Array(byteCount(vectors)),view=new DataView(bytes.buffer);let at=0;
 const number=v=>{if(!Number.isFinite(v))throw Error('nonfinite soldier correction');view.setFloat64(at,v,true);at+=8;};
 for(const key of numbers)number(soldier[key]);
 for(const key of vectors){const v=soldier[key];if(!v?.isVector3)throw Error(`missing soldier vector ${key}`);number(v.x);number(v.y);number(v.z);}
 for(const [key,values]of Object.entries(enums)){const index=values.indexOf(soldier[key]);if(index<0)throw Error(`invalid soldier ${key}`);bytes[at++]=index;}
 for(const key of booleans){if(typeof soldier[key]!=='boolean')throw Error(`invalid soldier ${key}`);bytes[at++]=Number(soldier[key]);}
 return bytes;
}
export function restoreSoldierCorrection(soldier,payload,schema=SOLDIER_CORRECTION_SCHEMA){
 const vectors=fieldsFor(schema);
 if(!(payload instanceof Uint8Array)||payload.length!==byteCount(vectors))throw Error('unsupported soldier correction');
 const view=new DataView(payload.buffer,payload.byteOffset,payload.byteLength),values={};let at=0;
 const number=()=>{const v=view.getFloat64(at,true);at+=8;if(!Number.isFinite(v))throw Error('nonfinite soldier correction');return v;};
 for(const key of numbers)values[key]=number();
 for(const key of vectors){if(!soldier[key]?.isVector3)throw Error(`missing soldier vector ${key}`);values[key]=[number(),number(),number()];}
 for(const [key,items]of Object.entries(enums)){const n=payload[at++];if(n>=items.length)throw Error('invalid correction enum');values[key]=items[n];}
 for(const key of booleans){const n=payload[at++];if(n>1)throw Error('invalid correction boolean');values[key]=!!n;}
 // Validate the complete payload before mutating any live simulation state.
 for(const key of numbers)soldier[key]=values[key];
 for(const key of vectors)soldier[key].fromArray(values[key]);
 for(const key of [...Object.keys(enums),...booleans])soldier[key]=values[key];
}
