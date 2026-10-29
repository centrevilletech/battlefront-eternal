import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import * as T from 'three';
import {buildActor} from '../src/actor.js';
import {loadClips} from '../src/anim.js';
import {modelPack} from '../src/models.js';
import {Droideka} from '../src/droideka.js';
import {FirstPersonView,DROIDEKA_OFFSET} from '../src/firstperson.js';
import {PlayerCombat} from '../src/combat.js';
const root=new URL('../public/',import.meta.url);
const json=path=>JSON.parse(readFileSync(new URL(path,root)));
const buffer=path=>{const b=readFileSync(new URL(path,root));return b.buffer.slice(b.byteOffset,b.byteOffset+b.byteLength);};
const available=existsSync(new URL('sides/cis/firstperson/cis_1st_droideka.json',root));
function fixture(){
 const name='sides/cis/actors/cis_walk_droideka',model=json(name+'.json');
 const actor=buildActor(model,buffer(name+'.bin'),buffer(name+'_index.bin'),()=>null,{caster(){}});
 const def=json('sides/cis/units.json').units.cis_inf_droideka;
 const d=new Droideka({},def,loadClips(json(name+'_anims.json'),buffer(name+'_anims.bin'),model.bones.map(b=>b.name)),actor);
 d.arm(def.weapons);d.animator.update(0,d);actor.root.updateMatrixWorld(true);
 const fp='sides/cis/firstperson/cis_1st_droideka',rig=json(fp+'.json');
 const flashModel=()=>{const flash=new T.Group();flash.userData.material={color:new T.Color(),opacity:1};return flash;};
 const view=new FirstPersonView({rig,parts:modelPack({[rig.model]:rig.parts},buffer(fp+'.bin'),buffer(fp+'_index.bin')),clips:loadClips(json('common/anims_fp.json'),buffer('common/anims_fp.bin'),rig.bones.map(b=>b.name)),texture:()=>null,weaponModels:new Map(),firePoints:{},flashModel,set:'droideka',offset:DROIDEKA_OFFSET});
 const camera=new T.PerspectiveCamera(60,1.6,.1,100);camera.position.set(0,1.8,0);camera.lookAt(0,1.8,1);camera.updateMatrixWorld();
 const weapon={def:def.weapons[0],bank:'bdroid'};
 for(let i=0;i<90;i++)view.update(1/60,{active:true,camera,soldier:d,shown:weapon,zoomed:false});
 return {d,actor,view,camera,weapon};
}

test('native world and first-person rounds cycle through four actual barrel apertures',{skip:!available},()=>{
 const {d,actor,view}=fixture(),point=new T.Vector3(),vertex=new T.Vector3(),selected=[];
 for(let shot=0;shot<4;shot++){
  d.nextGun(point);selected.push(`${d.lastGun.index}:${d.lastGun.barrel}`);
  let nearest=Infinity;
  for(let i=0;i<actor.body.geometry.attributes.position.count;i++){actor.body.getVertexPosition(i,vertex);nearest=Math.min(nearest,vertex.distanceTo(point));}
  assert.ok(nearest<.025,`world muzzle is ${nearest}m away from the aperture rim`);
  view.muzzlePoint(d.lastGun,point);nearest=Infinity;
  for(const mesh of view.rigParts){const positions=mesh.geometry.attributes.position;for(let i=0;i<positions.count;i++){vertex.fromBufferAttribute(positions,i);mesh.localToWorld(vertex);nearest=Math.min(nearest,vertex.distanceTo(point));}}
  assert.ok(nearest<.025,`first-person muzzle is ${nearest}m away from the aperture rim`);
 }
 assert.equal(new Set(selected).size,4);
});

test('deferred emission samples the current world and view poses without changing physical muzzle collision',{skip:!available},()=>{
 const {d,actor,view,camera,weapon}=fixture(),c=Object.create(PlayerCombat.prototype),emitted=[];
 for(const name of ['origin','target','aimDir','shotDir','fireDir','side','gunPoint','muzzlePoint'])c[name]=new T.Vector3();
 Object.assign(c,{soldier:d,firstPerson:true,view,held:new Map(),deferMuzzles:true,pendingMuzzles:[],kick:{offset:0},animator:{weaponAction(){},fire(){}},flash(){},ordnance:{fire(def,from,dir){const round={from:from.clone(),dir:dir.clone()};emitted.push(round);return round;}}});
 c.aimDir.set(0,0,1);c.shotDir.copy(c.aimDir);c.target.set(0,1.8,100);
 c.fire({weapon,yaw:0,pitch:0});assert.equal(emitted.length,0);
 actor.root.position.x=3;actor.root.rotation.y=.4;actor.root.updateMatrixWorld(true);
 camera.position.x=3;camera.updateMatrixWorld();view.update(1/60,{active:true,camera,soldier:d,shown:weapon,zoomed:false});
 c.flushMuzzles();assert.equal(emitted.length,1);
 const e=emitted[0];assert.ok(e.from.distanceTo(d.gunAt(new T.Vector3()))<1e-8);
 const physical=e.from.clone();
 // Presentation can move again after the shot has entered the world.
 camera.position.x=4;camera.updateMatrixWorld();view.update(1/60,{active:true,camera,soldier:d,shown:weapon,zoomed:false});
 const visual=e.visualMuzzle().clone();
 assert.ok(visual.distanceTo(view.muzzlePoint(d.lastGun,new T.Vector3()))<1e-8);
 assert.ok(e.from.distanceTo(visual)>.3,'FP visuals must not move gameplay collision into the overlay rig');
 assert.deepEqual(e.from,physical);
 assert.ok(e.dir.dot(c.aimDir)>.9);assert.equal(c.pendingMuzzles.length,0);
});

test('droideka flash uses only the selected rendered barrel and survives its first low-FPS frame',{skip:!available},()=>{
 const {view,camera,d,weapon}=fixture();view.flash(weapon,{index:1,barrel:0});
 view.update(1/30,{active:true,camera,soldier:d,shown:weapon,zoomed:false});
 const flashes=view.builtInGuns.flat().filter(m=>m.flash.visible);
 assert.equal(flashes.length,1);assert.equal(flashes[0],view.builtInGuns[1][0]);
 view.update(1/30,{active:true,camera,soldier:d,shown:weapon,zoomed:false});
 assert.equal(view.builtInGuns.flat().filter(m=>m.flash.visible).length,0);
});

test('deployed blasters follow vertical aim while the feet stay planted',{skip:!available},()=>{
 const {d,actor}=fixture(),axis=new T.Vector3(),foot=actor.byName.bone_l_leg2;
 actor.root.updateMatrixWorld(true);const planted=foot.getWorldPosition(new T.Vector3());
 for(const pitch of [-.45,0,.45]){
  d.aimPitch=pitch;d.animator.update(1/60,d);actor.root.updateMatrixWorld(true);
  for(const gun of d.guns){axis.set(1,0,0).transformDirection(gun.parent.matrixWorld);
   assert.ok(Math.abs(Math.atan2(axis.y,Math.hypot(axis.x,axis.z))-pitch)<.09);
  }
  assert.ok(foot.getWorldPosition(new T.Vector3()).distanceTo(planted)<1e-8);
 }
});
