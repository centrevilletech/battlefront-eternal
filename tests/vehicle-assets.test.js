// Optional intake/runtime contract test; retail assets remain local and ignored.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles } from '../src/vehicles.js';
import { Ordnance } from '../src/ordnance.js';
import { Weapon } from '../src/weapons.js';
import { CommandControls } from '../src/runtime/commands.js';
import { classSetting } from '../src/vehicle-motion.js';
import { vehiclePacks } from '../tools/vehicle-packs.mjs';
const maps = new URL('../public/maps/', import.meta.url);
const binary = file => { const b=fs.readFileSync(file);return b.buffer.slice(b.byteOffset,b.byteOffset+b.byteLength); };
const manifests = vehiclePacks(maps);

test('extracted vehicle rigs, collision, controllers and every weapon seat initialize together', {skip:!manifests.length}, () => {
 const tested = new Set();
 for(const file of manifests) {
  const data=JSON.parse(fs.readFileSync(file,'utf8'));
  const names=Object.keys(data.classes).filter(name=>!tested.has(name));
  if(!names.length)continue;
  const scene=new THREE.Scene(),camera=new THREE.PerspectiveCamera();
  const g=new THREE.BoxGeometry(20000,2,20000).translate(0,-1,0);
  const collision=new CollisionWorld(g.attributes.position.array,g.index.array,new Uint32Array(g.index.count/3));
  const effects={play:()=>null},lights={add(){},flash(){}};
  const ordnance=new Ordnance({scene,collision,effects,lights,texture:()=>null,model:()=>null});
 const shots=[];const fire=ordnance.fire.bind(ordnance);ordnance.fire=(...args)=>{shots.push({def:args[0],origin:args[1].clone(),direction:args[2].clone(),ignore:args[5]});return fire(...args)};
  // A neutral test floor isolates asset/controller contracts from map layout.
  const spawns=names.map((name,i)=>({name,matrix:new THREE.Matrix4().makeTranslation(i*100,0,0).toArray(),team:1,post:'',respawnTime:45,classes:{1:name}}));
  const dir=new URL('./',file);
  const vehicles=new Vehicles({scene,camera,collision,data:{...data,spawns,blockers:null},buffer:binary(new URL('models.bin',dir)),indexBuffer:binary(new URL('models_index.bin',dir)),animationBuffer:binary(new URL('animations.bin',dir)),texture:()=>null,ordnance,effects});
  for(const name of names) {
   const v=vehicles.vehicles.find(v=>v.name===name);
   assert.ok(v?.alive,`${name}: spawn`);
   if (v.walking) {
    const left=v.clips?.get('walk_leftfoot_rightfoot'),right=v.clips?.get('walk_rightfoot_leftfoot');
    if(left && right) {
     // Walkers and animals walk by DummyRoot's travel, each clip lasting its
     // frames at 30 fps over AnimationSpeed, backward too, and never strafe.
     const pace=(left.stride+right.stride)*30/(left.frames+right.frames)*Number(classSetting(v.def,'AnimationSpeed') ?? 1);
     assert.ok(Math.abs(v.speed-pace)<.001&&v.reverseSpeed===v.speed&&v.strafeSpeed===0,`${name}: use authored travel, not chassis speed`);
    }
   }
   const wheelAxes = v.wheels.map(w=>new THREE.Vector3(0,1,0).applyQuaternion(w.bone.quaternion));
   v.driver={};const start=v.position.clone();
   // A flyer's lift runs at its own TakeoffSpeed to its TakeoffHeight (3 to
   // 5.5 s for the gunship, MAF and cloud car): fly on until it is airborne.
   for(let i=0;i<(v.flying?200:120)||(v.flying&&v.flightState!=='Flying'&&i<1200);i++)vehicles.update(1/120,v,{forward:1,right:0,turn:0,takeoff:v.flying&&i===0});
   assert.ok(v.position.distanceTo(start)>.05,`${name}: motion`);
   assert.ok(v.position.toArray().every(Number.isFinite),`${name}: finite physics`);
   v.wheels.forEach((w,i)=>{
    const axle=new THREE.Vector3(0,1,0).applyQuaternion(w.bone.quaternion);
    assert.ok(axle.dot(wheelAxes[i])>.999999,`${name}: rotating a rim must not tilt its axle`);
    assert.ok(w.phase>0,`${name}: rolling forward rotates both mirrored rims forward`);
   });
   camera.position.copy(v.position).add(new THREE.Vector3(0,10,-20));
   for(let seat=0;seat<v.seats.length;seat++) {
    v.seatIndex=seat;v.render(1,0);
    for (const [channel, weapon] of v.seat.weapons.entries()) {
     const label=`${name}: seat ${seat}, ${weapon.def.class}`;
     for (const w of v.weapons) Object.assign(w,new Weapon(w.levels[0]));
     const before=shots.length, input=new CommandControls();
     const action=channel===0?'firePrimary':'fireSecondary', key=channel===0?'Mouse0':'Mouse2';
     // Each channel alone, including release-to-fire weapons. Real command
     // edges must survive independently of whether the button is still held.
     const hold=Math.max(1,weapon.def.MaxPressedTime || 0);
     for(let i=0;i<Math.ceil((hold+1)*60);i++) {
      input.accept({epoch:1,generation:1,sequence:i+1,down:i<hold*60?[key]:[],actions:i===0?[action]:[]});
      v.aimTick(1,1/60);v.fire(1/60,input);
     }
     if(weapon.def.ordnance.base==='towcable')continue; // dedicated tether tests
     const emitted=shots.slice(before);
     assert.ok(emitted.length>0,`${label}: no projectile`);
     for(const shot of emitted) {
      assert.equal(shot.def.class,weapon.state.def.ordnance.class,`${label}: wrong channel fired`);
      assert.equal(shot.ignore,v.body,`${label}: must exclude its own hull`);
      assert.ok(shot.origin.toArray().every(Number.isFinite),`${label}: finite muzzle`);
      assert.ok(Math.abs(shot.direction.length()-1)<1e-6,`${label}: usable projectile direction`);
     }
    }
   }
   v.driver=null;tested.add(name);
  }
  vehicles.dispose();
 }
 assert.ok(tested.size>0);
 console.log(`Checked every weapon channel on ${tested.size} extracted vehicle classes`);
});
