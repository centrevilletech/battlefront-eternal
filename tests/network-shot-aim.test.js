import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { networkShotDirection, networkShotTarget } from '../src/runtime/network-shot-aim.js';

test('simultaneous primary and secondary edges preserve independent directions',()=>{
 const owner={networkHuman:true,networkShots:[{button:0,id:1,aimYaw:0,aimPitch:0},{button:1,id:2,aimYaw:Math.PI/2,aimPitch:0}]};
 const first=networkShotDirection(owner,new Vector3(),0),second=networkShotDirection(owner,new Vector3(),1);
 assert.ok(first.distanceTo(new Vector3(0,0,1))<1e-12);assert.ok(second.distanceTo(new Vector3(1,0,0))<1e-12);
 owner.networkShots=[];const held=new Vector3(0,1,0);networkShotDirection(owner,held,1);assert.deepEqual(held.toArray(),[0,1,0]);
});
test('delayed fire aims the projectile without rolling back newer mouse movement',()=>{
 const owner={networkHuman:true,soldier:{aimYaw:1,aimPitch:.2},networkShot:{aimYaw:0,aimPitch:0}};
 const direction=new Vector3(1,0,0);assert.equal(networkShotDirection(owner,direction),direction);
 assert.deepEqual(direction.toArray(),[0,0,1]);assert.deepEqual(owner.soldier,{aimYaw:1,aimPitch:.2});
 direction.applyAxisAngle(new Vector3(0,1,0),.1);assert.ok(direction.x>0,'normal weapon spread is still applied after the hint');
 owner.networkShot=null;const current=direction.clone();networkShotDirection(owner,direction);assert.deepEqual(direction,current);
});
test('a converging gun takes the press aim as its eye-ray target, keeping the reach',()=>{
 const owner={networkHuman:true,networkShot:{aimYaw:0,aimPitch:0}};
 const eye=new Vector3(0,1.6,0), target=new Vector3(5,3,100);
 const reach=target.distanceTo(eye);
 networkShotTarget(owner,eye,target);
 assert.ok(Math.abs(target.x)<1e-9&&Math.abs(target.y-1.6)<1e-9,'on the press ray');
 assert.ok(Math.abs(target.distanceTo(eye)-reach)<1e-9,'same reach');
 // Converging an offset barrel on it crosses the reticle ray, where the
 // direction overwrite would leave it parallel and off by the offset.
 const from=new Vector3(.5,1,0), dir=new Vector3().subVectors(target,from).normalize();
 const t=(20-from.z)/dir.z, at=from.clone().addScaledVector(dir,t);
 assert.ok(Math.abs(at.x-0.4)<.05,'closing on the ray, not parallel to it');
 const bot={networkShot:{aimYaw:0,aimPitch:0}}, kept=new Vector3(5,3,100);
 networkShotTarget(bot,eye,kept);assert.deepEqual(kept.toArray(),[5,3,100]);
});
test('mounted control rates, bots and malformed hints retain their authoritative firing direction',()=>{
 for(const owner of [{networkHuman:true,seat:{},networkShot:{aimYaw:0,aimPitch:0}},{networkShot:{aimYaw:0,aimPitch:0}},{networkHuman:true,networkShot:{aimYaw:Infinity,aimPitch:0}}]) {
  const direction=new Vector3(1,0,0);networkShotDirection(owner,direction);assert.deepEqual(direction.toArray(),[1,0,0]);
 }
});
