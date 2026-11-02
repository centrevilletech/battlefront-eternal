import { FLIGHT_CLASSES } from './config.js';
import * as THREE from 'three';
import { unitsArray } from '../hud-units.js';
const f=new THREE.Vector3(),d=new THREE.Vector3(),point=new THREE.Vector3();
const hit={point:new THREE.Vector3(),normal:new THREE.Vector3()};
// A target within the cone, in range and an enemy: its score (how near the
// nose it is), else false. Leaves the direction to it in d and returns its
// distance through `reach`.
const reach={distance:0};
function inCone(v,t,cone) {
  if(!t?.alive || t===v.aiTarget)return false;
  const units=v.system.units;
  const relation=units?.relation?.(v.aiTarget,t) ?? (t.team===v.team?1:-1);
  if(relation>=0)return false;
  if(t.aimPoint)t.aimPoint(point);else point.copy(t.position);
  d.subVectors(point,v.position);const distance=d.length();
  if(distance>v.flightConfig.lockRange||distance<1)return false;
  const dot=d.divideScalar(distance).dot(f);
  if(dot<Math.cos(cone*Math.PI/180))return false;
  reach.distance=distance;
  return dot;
}
// Nothing but the target itself, or something within 2 m short of it,
// stands between the craft and the target inCone has just put in d.
function clear(v,t) {
  const contact=v.system.collision.cast(v.position,d,reach.distance,hit,v.body);
  return !(contact && contact.body?.owner!==t.vehicle && contact.distance<reach.distance-2);
}
function eligible(v,t,cone) {
  const dot=inCone(v,t,cone);
  return dot!==false && clear(v,t) ? dot : false;
}
// The best target in the cone that the craft can see: the highest score, the
// earlier unit of equal ones, as testing each unit in turn found it. Each
// unit takes the cheap tests; the sight ray, a cast through the map, goes
// only to the candidates in score order until one passes, not to every
// candidate (each fighter looked along a ray to every enemy in its cone,
// every tick, a fifth of a slow CPU's world step over Bespin's platforms).
const candidates=[],scores=[];
function bestTarget(v,cone) {
  candidates.length=scores.length=0;
  for(const t of unitsArray(v.system.units)){const score=inCone(v,t,cone);if(score!==false){candidates.push(t);scores.push(score);}}
  const order=candidates.map((_,i)=>i).sort((a,b)=>scores[b]-scores[a]||a-b);
  for(const i of order){const t=candidates[i];inCone(v,t,cone);if(clear(v,t))return t;}
  return null;
}
export function updateFlightTarget(v,dt,press=false) {
  const c=v.flightConfig;if(!c)return;
  f.set(0,0,1).applyQuaternion(v.quaternion);
  v.flightAssistId=-1;
  if(v.system.flightAssists){const t=bestTarget(v,c.assistCone);if(t)v.flightAssistId=t.id;}
  let current=v.system.units?.byId?.(v.flightLockId);
  if(press){
    if(current){v.flightLockId=-1;current=null;}
    else {
      current=bestTarget(v,c.lockCone);
      if(current){v.flightLockId=current.id;v.flightLockGeneration=current.generation??0;}
    }
    v.flightLockTimer=0;v.flightLostLOS=0;
  }
  if(current && (!current.alive || (current.generation??0)!==v.flightLockGeneration)){current=null;v.flightLockId=-1;}
  if(current){
    if(eligible(v,current,c.retainCone)!==false){v.flightLostLOS=0;v.flightLockTimer+=dt;}
    else v.flightLostLOS+=dt;
    if(v.flightLostLOS>c.losGrace){v.flightLockId=-1;current=null;}
  }
  v.flightLockState=current?(v.flightLockTimer>=c.lockDwell?'Locked':'Locking'):'';
  if(!current)v.flightLockTimer=0;
  for(const seat of v.seats)for(const w of seat.weapons){w.state.lockTargetId=current?.id??-1;w.state.lockGeneration=current?.generation??-1;w.state.lockElapsed=v.flightLockTimer;w.state.lockComplete=v.flightLockState==='Locked';}
}
// Proportional navigation: acceleration follows LOS angular velocity.
export function navigateMissile(r,target,dt) {
  const los=target.position.clone().sub(r.position),distance=los.length(),speed=r.velocity.length();
  if(distance<1||speed<1)return;
  const tuning=FLIGHT_CLASSES[r.flightClass || 'fighter'];
  const hull=target.vehicle || target;
  // The powered roll changes aspect faster than the close-range seeker can
  // track. Guidance remains PN until tracking is lost; it never reacquires.
  if(distance<tuning.missileEvadeRange && Math.abs(hull.flightRollRate||0)>tuning.missileRollBreakRate){r.targetId=-1;return;}
  if(r.velocity.clone().normalize().dot(los.clone().normalize())<Math.cos(tuning.missileSeeker*Math.PI/180)){r.targetId=-1;return;}
  const rel=(target.velocity||new THREE.Vector3()).clone().sub(r.velocity);
  const omega=los.clone().cross(rel).divideScalar(distance*distance);
  const closing=Math.max(0,-rel.dot(los)/distance);
  const acceleration=omega.cross(r.velocity.clone().normalize()).multiplyScalar(tuning.missileNavigation*closing);
  const wanted=r.velocity.clone().addScaledVector(acceleration,dt).normalize();
  const from=r.velocity.clone().normalize(),angle=from.angleTo(wanted),cap=tuning.missileTurnRate*dt;
  if(angle>1e-7){const q=new THREE.Quaternion().setFromUnitVectors(from,wanted);q.slerpQuaternions(new THREE.Quaternion(),q.clone(),Math.min(1,cap/angle));r.velocity.applyQuaternion(q);}

}

export function assistShot(v, direction, origin) {
  if(!v.system.flightAssists || v.flightAssistId<0)return;
  const t=v.system.units?.byId?.(v.flightAssistId);if(!t?.alive)return;
  if(t.aimPoint)t.aimPoint(point);else point.copy(t.position);
  d.subVectors(point,origin).normalize();
  if(direction.dot(d)>Math.cos(v.flightConfig.assistCone*Math.PI/180))direction.lerp(d,v.flightConfig.assistStrength).normalize();
}
