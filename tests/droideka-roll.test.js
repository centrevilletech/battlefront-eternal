import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import * as THREE from 'three';
import {buildActor} from '../src/actor.js';
import {loadClips} from '../src/anim.js';
import {Droideka} from '../src/droideka.js';
const base=new URL('../public/sides/cis/actors/cis_walk_droideka',import.meta.url).pathname;
const available=existsSync(base+'.json');
function buffer(path){const b=readFileSync(path);return b.buffer.slice(b.byteOffset,b.byteOffset+b.byteLength);}
export function fixture(){
 const model=JSON.parse(readFileSync(base+'.json'));
 const actor=buildActor(model,buffer(base+'.bin'),buffer(base+'_index.bin'),()=>null,{caster(){}});
 const clips=loadClips(JSON.parse(readFileSync(base+'_anims.json')),buffer(base+'_anims.bin'),model.bones.map(b=>b.name));
 const d=new Droideka({}, {AnimationSpeed:2,BallRadius:0.65},clips,actor);
 return {d,actor,a:d.animator};
}
function vertices(actor){
 actor.root.updateMatrixWorld(true);
 const result=[];
 for(let i=0;i<actor.body.geometry.attributes.position.count;i++)result.push(actor.body.getVertexPosition(i,new THREE.Vector3()));
 return result;
}
function maxDisplacement(before,after){return Math.max(...before.map((v,i)=>v.distanceTo(after[i])));}
export function revolutionMetrics(){
 const {d,a,actor}=fixture();d.mode=12;d.roll=0;a.update(0,d);
 const centers=new THREE.Box3(),means=new THREE.Box3();let floor=Infinity,maxRadius=0;
 for(let step=0;step<=120;step++){
  d.roll=step*Math.PI*2/120;a.update(1/60,d);
  const points=vertices(actor),bounds=new THREE.Box3().setFromPoints(points);
  centers.expandByPoint(bounds.getCenter(new THREE.Vector3()));
  const mean=points.reduce((sum,p)=>sum.add(p),new THREE.Vector3()).divideScalar(points.length);means.expandByPoint(mean);
  floor=Math.min(floor,bounds.min.y);
  for(const p of points)maxRadius=Math.max(maxRadius,Math.hypot(p.y-a.rollCenter.y,p.z));
 }
 return {centerExcursion:centers.getSize(new THREE.Vector3()).length(),meanExcursion:means.getSize(new THREE.Vector3()).length(),floor,maxRadius};
}
test('retail rolled shell rotates around its geometry center and clears the contact plane',{skip:!available},()=>{
 const m=revolutionMetrics();
 assert.ok(m.centerExcursion<0.18,`bounds center excursion ${m.centerExcursion}`);
 assert.ok(m.meanExcursion<0.18,`vertex centroid excursion ${m.meanExcursion}`);
 assert.ok(m.floor>-1e-5,`shell clips below contact plane by ${-m.floor}`);
 if(process.env.DROIDEKA_METRICS)console.log(m);
});
test('fold endpoint matches a fresh rolling revolution even after earlier revolutions',{skip:!available},()=>{
 const {d,a,actor}=fixture();d.roll=17.3;d.mode=11;d.time=d.length();a.update(1/60,d);
 const folded=vertices(actor);d.mode=12;d.time=0;a.update(1/60,d);
 assert.ok(maxDisplacement(folded,vertices(actor))<0.001);
 d.roll+=Math.PI;a.update(1/60,d);d.mode=13;d.time=0;a.update(1/60,d);
 d.mode=11;d.time=d.length();a.update(1/60,d);const refolded=vertices(actor);
 d.mode=12;a.update(1/60,d);assert.ok(maxDisplacement(refolded,vertices(actor))<0.001);
});
test('unfold starts continuously from arbitrary spin and lean and reaches the authored stand pose',{skip:!available},()=>{
 for(const roll of [0,1.4,3.1,5.8]){
  const {d,a,actor}=fixture();d.mode=12;d.roll=0;a.update(0,d);d.roll=roll;d.lean=.15;a.update(1/60,d);
  const previous=vertices(actor);d.mode=13;d.time=0;a.update(1/144,d);
  assert.ok(maxDisplacement(previous,vertices(actor))<0.03,'unfold must not snap out of live spin');
  for(let i=0;i<120;i++){d.time=Math.min(d.length(),i/60);a.update(1/60,d);}
  const end=vertices(actor);d.mode=0;d.time=0;d.legs=0;a.update(1/60,d);
  assert.ok(maxDisplacement(end,vertices(actor))<0.001,'unfold must finish at authored standing pose');
 }
});
test('steering lean rotates about the shell axle instead of tilting around its feet',{skip:!available},()=>{
 const {d,a,actor}=fixture();d.mode=12;a.update(0,d);const points=vertices(actor);
 d.lean=.17;a.update(1/60,d);const leaned=vertices(actor),rotation=new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0,0,1),d.lean);
 for(let i=0;i<points.length;i++){
  const expected=points[i].clone().sub(a.rollCenter).applyQuaternion(rotation).add(a.rollCenter);
  assert.ok(expected.distanceTo(leaned[i])<1e-6);
 }
});
