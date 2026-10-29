import * as THREE from 'three';
const cache=new WeakMap();
// These are spawn choices only, never capture objectives or ticket-bleed posts.
export function transportPosts(system) {
  if(!system)return [];
  const posts=[];
  for(const v of system.vehicles){
    if(!v.alive||v.flightClass!=='transport'||v.flightState!=='Landed'||!v.team)continue;
    const point=new THREE.Vector3(v.bounds.max.x+2,1,0).applyQuaternion(v.quaternion).add(v.position);
    const hit={point:new THREE.Vector3(),normal:new THREE.Vector3()};
    const floor=system.collision.cast(point,new THREE.Vector3(0,-1,0),12,hit,v.body);
    if(!floor||floor.normal.y<.8)continue;
    point.y=floor.point.y+.1;
    let post=cache.get(v);
    if(!post){post={name:`transport-${system.slots?.indexOf(v.slot) ?? system.vehicles.indexOf(v)}`,label:v.label,position:new THREE.Vector3(),desc:{},transport:v};cache.set(v,post);}
    post.team=v.team;post.active=true;post.position.copy(point);
    post.ai={spawnPath:{present:true,points:[[point.x,point.y,point.z,v.yaw]]},classSpawnPoints:0};posts.push(post);
  }
  return posts;
}
