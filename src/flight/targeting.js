// OrdnanceWorld's remaining foreign seeker service. Hull assist and lock
// acquisition run inside the native world/prediction owners.
import { FLIGHT_CLASSES } from './config.js';
import * as THREE from 'three';
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
