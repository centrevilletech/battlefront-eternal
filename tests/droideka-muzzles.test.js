import {actorPointCount,actorPoint} from '../src/actor-resources.js';
import './fixtures/native-render-descriptors.js';
import './fixtures/native-anim.js';
import { contentURL } from './helpers/content.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import * as T from 'three';
import {buildActor} from '../src/actor.js';
import {loadClips} from '../src/anim.js';
import {modelPack} from '../src/models.js';
import {Droideka} from './helpers/droideka.js';
import {FirstPersonView} from './helpers/presentation.js';
import {DROIDEKA_OFFSET} from '../src/firstperson.js';
import {PlayerCombat} from '../src/combat.js';
const root=contentURL('');
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

test('drawn world and first-person muzzles lie at their actual barrel apertures',{skip:!available},()=>{
 const {d,actor,view}=fixture(),point=new T.Vector3(),vertex=new T.Vector3(),selected=[];
 for(let shot=0;shot<4;shot++){
  const gun={index:Math.floor(shot/2),barrel:shot%2};
  d.visualGunAt(point,gun);selected.push(`${gun.index}:${gun.barrel}`);
  let nearest=Infinity;
  for(let i=0;i<actorPointCount(actor.body.userData.actorAsset);i++){actor.body.getVertexPosition(i,vertex);nearest=Math.min(nearest,vertex.distanceTo(point));}
  assert.ok(nearest<.025,`world muzzle is ${nearest}m away from the aperture rim`);
  view.muzzlePoint(gun,point);nearest=Infinity;
  for(const mesh of view.rigParts){const asset=mesh.userData.actorAsset;for(let i=0;i<actorPointCount(asset);i++){actorPoint(asset,i,vertex);mesh.localToWorld(vertex);nearest=Math.min(nearest,vertex.distanceTo(point));}}
  assert.ok(nearest<.025,`first-person muzzle is ${nearest}m away from the aperture rim`);
 }
 assert.equal(new Set(selected).size,4);
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
