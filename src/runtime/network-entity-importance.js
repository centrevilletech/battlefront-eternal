// Scheduling uses authoritative simulation state only. Client pose/velocity is
// never accepted as an interest hint. This ranks records; it is not visibility
// culling and never removes the low-priority starvation floor.
import {ENTITY_SOLDIER_STATES} from './network-entity-codec.js';
const thrownCorpse=record=>record.kind==='soldier'&&ENTITY_SOLDIER_STATES[record.state]==='thrownDead';
export function entityImportance(record,{position,velocity=[0,0,0],forward,flying=false}={}) {
  if(record.kind==='seat'||record.kind==='tombstone')return 16;
  if(record.kind==='summary')return 4;
  if(!record.position||!position)return record.kind==='vehicle'?6:1;
  const dx=record.position[0]-position[0],dy=record.position[1]-position[1],dz=record.position[2]-position[2];
  const distance=Math.hypot(dx,dy,dz),inverse=distance>1e-6?1/distance:0;
  const vx=record.velocity?.[0]||0,vy=record.velocity?.[1]||0,vz=record.velocity?.[2]||0;
  const speed=Math.hypot(vx,vy,vz);
  const closing=Math.max(0,(dx*((velocity?.[0]||0)-vx)+dy*((velocity?.[1]||0)-vy)+dz*((velocity?.[2]||0)-vz))*inverse);
  const view=forward?Math.max(0,(dx*forward[0]+dy*forward[1]+dz*forward[2])*inverse):0;
  const range=flying?180:60;
  const distanceWeight=1+9/(1+(distance/range)**2);
  const moving=Math.min(6,speed/20)+Math.min(6,closing/20);
  const vehicleFloor=record.kind==='vehicle'?(flying?8:6):1;
  if(record.health===0&&!thrownCorpse(record))return 1;
  return Math.min(48,Math.max(vehicleFloor,distanceWeight)+moving+4*view*view);
}

// Soft delivery deadlines, in simulation ticks. An approaching vehicle or a
// target under the crosshair deserves the same freshness as close infantry.
// These do not change simulation rate or cull distant actors.
export function entityUpdateInterval(record,{position,velocity=[0,0,0],forward}={}) {
  if(['seat','tombstone','combat'].includes(record.kind))return 2;
  if(record.health===0&&!thrownCorpse(record))return 30;
  if(!position||!record.position)return record.kind==='summary'?30:12;
  const delta=record.position.map((n,i)=>n-position[i]);
  const distance=Math.hypot(...delta),inverse=distance>1e-6?1/distance:0;
  const closing=Math.max(0,delta.reduce((sum,n,i)=>sum+n*((velocity?.[i]||0)-(record.velocity?.[i]||0)),0)*inverse);
  const view=forward?delta.reduce((sum,n,i)=>sum+n*forward[i],0)*inverse:0;
  if(distance<=35||closing>0&&distance/closing<=2||distance<=320&&view>=.98)return 3;
  if(distance<=100||record.kind==='vehicle'&&Math.hypot(...(record.velocity||[0,0,0]))>=20)return 6;
  return distance<=250?12:30;
}
