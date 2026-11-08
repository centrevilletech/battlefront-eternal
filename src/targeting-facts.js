// Browser identities and collision I/O for the native page targeting phase.
import * as THREE from 'three';
import {categoryOf,weaponAI} from './weapon-policy.js';
import {clientExports as x} from './runtime/fixed-clock.js';
import {PageTargetFrameLayout as F,PageTargetRowLayout as R,VehicleMissilePolicyLayout as P} from './sim/gen/page-targeting.js';
const EMPTY_DEFINITION=Object.freeze({}),EMPTY_UNITS=Object.freeze([]);
export const unitDefinition=unit=>{const def=unit?.classDef||unit?.def||EMPTY_DEFINITION;return def.settings||def;};
export const unitsArray=units=>units?.all?units.all():units||EMPTY_UNITS;
export const defaultRelation=(a,b)=>x.page_relation(a??NaN,b??NaN);
export function relationName(value){return typeof value==='string'?value:RELATIONS[x.page_radar_relation(1,1,value,0)];}
export const defaultLocals=team=>team>2;
const RELATIONS=['neutral','friendly','enemy','locals'];
export const radarRelation=(viewer,team,relation=defaultRelation,locals=defaultLocals)=>RELATIONS[x.page_radar_relation(viewer,team,relation(viewer,team),+!!locals(team))];
export const reticleRange=def=>x.page_reticle_range(def.ReticleRange===undefined?NaN:Number(def.ReticleRange),Number(def.ordnance?.Velocity),Number(def.ordnance?.LifeSpan),Number(def.LockOnRange));

const CAPACITY=x.page_target_capacity(),handle=x.page_target_create(),framePtr=x.page_target_frame(handle),rowsPtr=x.page_target_rows(handle),utilityPtr=x.page_target_utility(handle),volumePtr=x.page_target_volume(handle);
let frame,rows,utility,geometry;
const refs=new Array(CAPACITY),point=new THREE.Vector3();
// A seat can query both weapon channels before consuming either result.
// Registry-scoped rows retain those independent publications without keeping
// a disposed battle alive through a module-global unit reference.
const resultPools=new WeakMap();
export function prepareTargeting(units){
  if(!units||typeof units!=='object')return null;
  let pool=resultPools.get(units);
  if(!pool){pool={at:0,rows:Array.from({length:8},()=>({unit:null,point:new THREE.Vector3(),lockable:false,score:0,distance:0}))};resultPools.set(units,pool);}
  return pool;
}
function refresh(){if(!frame||frame.length===0){frame=new Float64Array(x.memory.buffer,framePtr,F.words);rows=new Float64Array(x.memory.buffer,rowsPtr,CAPACITY*R.words);utility=new Float64Array(x.memory.buffer,utilityPtr,F.words);geometry=new Float64Array(x.memory.buffer,volumePtr,R.words);}}
export function targetFrame(){refresh();return utility;}
export function runTargetFrame(op){x.page_target_run(handle,op,0);refresh();return utility;}
function putVector(out,at,v){out[at]=v.x;out[at+1]=v.y;out[at+2]=v.z;}
function putVolume(at,unit,out=rows){
  const sphere=unit.sphere,vehicle=unit.vehicle;out[at+R.radius]=sphere?.radius??0;
  if(sphere?.center)point.copy(sphere.center);else if(unit.aimPoint)unit.aimPoint(point);else point.copy(unit.position);
  putVector(out,at+R.center,point);
  out[at+R.hull]=vehicle?.bounds&&vehicle.matrix?1:0;
  if(out[at+R.hull]){putVector(out,at+R.min,vehicle.bounds.min);putVector(out,at+R.max,vehicle.bounds.max);const m=vehicle.matrix.elements;for(let j=0;j<16;j++)out[at+R.matrix+j]=m[j];}
}
// Cold definition transport. Rust recognizes the content class and publishes
// the launcher fields; derived definition identity remains a browser handle.
export function vehicleMissilePolicy(name){
  const ptr=x.page_target_name(handle),bytes=new Uint8Array(x.memory.buffer,ptr,128);
  if(typeof name!=='string')name='';for(let i=0;i<name.length&&i<128;i++)bytes[i]=name.charCodeAt(i);
  const at=x.page_target_policy(handle,name.length);return new Float64Array(x.memory.buffer,at,P.words);
}
export function lockSpan(origin,dir,unit,out={angle:Math.PI,distance:Infinity}){
  refresh();putVector(utility,F.origin,origin);putVector(utility,F.direction,dir);putVolume(0,unit,geometry);runTargetFrame(2);out.angle=utility[F.angle];out.distance=utility[F.distance];return out;
}
const clearSight=()=>true;
// Facts cross once. Native traversal requests only the collision rays that
// can improve its current winner, then publishes the completed target record.
export function findReticleTarget({origin,dir,units,weapon,viewer=null,relation=defaultRelation,los=clearSight,range,angle}){
  const def=weapon?.def||weapon;if(!def||!origin||!dir)return null;
  refresh();putVector(frame,F.origin,origin);putVector(frame,F.direction,dir);
  frame[F.far]=range??reticleRange(def);frame[F.angle]=0;frame[F.yaw]=Number(angle??def.LockOnAngle)||0;
  const masks=weaponAI(def);frame[F.lockRange]=Number(def.LockOnRange)||0;frame[F.relationMask]=masks.relationMask;frame[F.categoryMask]=masks.categoryMask;
  let count=0;
  for(const unit of unitsArray(units)){
    if(count===CAPACITY)break;refs[count]=unit;const at=count++*R.words;
    rows[at+R.viewer]=+(unit===viewer);rows[at+R.alive]=+!!unit.alive;rows[at+R.seated]=+!!unit.seat;rows[at+R.health]=unit.health??unit.combat?.health??1;rows[at+R.kind]=unit.kind??NaN;
  }
  frame[F.count]=count;x.page_target_run(handle,0,0);
  for(let i=0;i<count;i++){
    const at=i*R.words;if(!rows[at+R.eligible])continue;const unit=refs[i];
    if(unit.aimPoint)unit.aimPoint(point);else point.copy(unit.sphere?.center||unit.position);putVector(rows,at+R.point,point);
    putVolume(at,unit);const half=unit.box?.half;rows[at+R.hasBox]=+!!half;if(half)putVector(rows,at+R.half,half);rows[at+R.boxRadius]=unit.sphere?.radius??0;
    rows[at+R.relation]=relation(viewer?.team,unit.team);rows[at+R.category]=categoryOf(unit);
  }
  let visible=0,index;
  while((index=x.page_target_run(handle,1,visible))>=0){refresh();point.set(frame[F.point],frame[F.point+1],frame[F.point+2]);visible=+!!los(origin,point,viewer,refs[index]);}
  refresh();index=frame[F.best];if(index<0){for(let i=0;i<count;i++)refs[i]=null;return null;}
  const pool=prepareTargeting(units),result=pool.rows[pool.at];pool.at=(pool.at+1)%pool.rows.length;
  result.unit=refs[index];result.point.set(frame[F.point],frame[F.point+1],frame[F.point+2]);result.lockable=!!frame[F.lockable];result.score=frame[F.score];result.distance=frame[F.distance];for(let i=0;i<count;i++)refs[i]=null;return result;
}
