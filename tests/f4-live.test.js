import test from 'node:test';
import assert from 'node:assert/strict';
import { Matrix4, Object3D } from 'three';
import { AnimatedProp, PowerupStation } from '../src/props.js';
import { F4Live } from '../src/f4-live.js';
import { loadClips } from '../src/anim.js';
import { f3World } from './fixtures/f3-world.js';
import { building } from './fixtures/f3-building.js';

const soldier = {class:'soldier',base:'soldier',MaxHealth:100,weapons:[]};
function station() {
  // Bundled class names differ from source names. Native policy and its
  // facade publication must follow constructor identity, including subtypes.
  class ServiceDroid extends PowerupStation {}
  const model = {sphere:[0,1,0,.5],box:[[-.25,0,-.25],[.25,2,.25]]};
  const looks = {built:{root:new Object3D(),model,byName:{}},destroyed:{root:new Object3D(),model}};
  return new ServiceDroid({class:'station',base:'powerupstation',settings:{MaxHealth:100,SoldierHealth:5,PowerupDelay:.5},matrix:new Matrix4().toArray()},looks,{built:null,destroyed:null,unbuilt:null},null);
}

test('one native live call supplies canonical units without feeding foreign rows', async()=>{
  const pump = station();
  const world = await f3World({soldier},{prepareLive:()=>({buildings:[pump],animated:[],traps:[]}),prepareUnit:u=>{u.supply={position:u.position};}});
  const unit = world.spawn('soldier',0,0,true);
  unit.combat.damage(20,null);
  const sounds = [], effects = [];
  const live = new F4Live(world.phase,{sound:(...event)=>sounds.push(event),effect:(name)=>effects.push(name)});
  pump.update = ()=>{ throw Error('legacy station tick'); };
  world.phase.props.foreign = ()=>{ throw Error('foreign rows fed'); };
  const output = live.native.outputWords, mirrored = live.mirrors.words;
  live.counters.reset(); live.step(.5,1);
  assert.equal(live.native.outputWords - output,live.mirrors.words - mirrored);
  assert.equal(unit.combat.health,85);
  assert.equal(pump.mode,'active'); assert.equal(pump.servedHealth,true);
  assert.equal(sounds.length,1); assert.equal(sounds[0][2],unit.supply); assert.deepEqual(effects,['Powerup_blue']);
  assert.equal(live.counters.wasmCalls,1); assert.equal(live.counters.phaseCalls,1);
  assert.equal(live.counters.rustAllocations,0); assert.equal(live.counters.rustReallocations,0);
  for (let tick=2;tick<1202;tick++) live.step(1/60,tick);
  assert.equal(live.counters.rustAllocations,0); assert.equal(live.counters.rustReallocations,0);
  assert.equal(unit.combat.health,100);
});

test('native health commands publish the touched building and reject an old life', async()=>{
  const wall = building('wall',0,2);
  const world = await f3World({soldier},{prepareLive:()=>({buildings:[wall],animated:[],traps:[]})});
  const live = new F4Live(world.phase,{sound(){}});
  wall.damage(20); assert.equal(wall.health,80);
  wall.setHealth(90); assert.equal(wall.health,90);
  const binding = live.buildings[0], life = binding.entry.handle.life;
  binding.entry.handle.life = life + 1;
  assert.equal(live.command(binding,'health',1),false);
  assert.equal(wall.health,90);
  binding.entry.handle.life = life;
});


test('live regeneration resumes after damage and leaves nonregenerating stations alone', async()=>{
  const wall=building('wall',0,2),pump=station();
  // Initial scalar facts precede the native ownership handoff.
  wall.health=pump.health=80;wall.addHealth=pump.addHealth=10;
  const world=await f3World({soldier},{prepareLive:()=>({buildings:[wall,pump],animated:[],traps:[]})});
  const live=new F4Live(world.phase,{sound(){}});
  live.step(1,1);assert.equal(wall.health,90);assert.equal(pump.health,80);
  live.step(1,2);assert.equal(wall.health,100);
  live.step(1,3);assert.equal(wall.health,100);
  wall.damage(20);live.step(1,4);assert.equal(wall.health,90);
  live.step(1,5);assert.equal(wall.health,100);assert.equal(pump.health,80);
  assert.equal(live.counters.rustAllocations,0);assert.equal(live.counters.rustReallocations,0);
});

test('native door publishes collision at the live slot and leaves page posing separate', async()=>{
  const root = new Object3D(), bone = new Object3D(); bone.name = 'bone_door'; root.add(bone);
  const rig = {root,bones:[bone],byName:{bone_door:bone},bind:new Map([[bone,{q:bone.quaternion.clone(),p:bone.position.clone()}]])};
  // The native sampler poses prepared clips only, as a loaded battle's are.
  const clips = loadClips({fps:1,bones:['bone_door'],clips:{open:{frames:2,flags:0,offset:0,tracks:[0],travel:[0,0,0]}}},
    new Float32Array([0,0,0,1,0,0,0,0,0,0,1,1,0,0]).buffer,['bone_door']);
  let door;
  const world = await f3World({soldier},{prepareLive:collision=>{
    const body = collision.addBody(new Float32Array([-2,0,2,2,0,2,0,3,2]),new Uint32Array([0,1,2]),new Uint32Array([0]),null);
    door = new AnimatedProp({base:'door',matrix:new Matrix4().toArray(),animation:{clips:['open'],trigger:{action:'open',radius:4}}},rig,body,clips);
    return {buildings:[],animated:[door],traps:[],presentAnimated(){},soldiers:()=>[],bodies:()=>[]};
  }});
  const live = new F4Live(world.phase,{sound(){}});
  door.step = ()=>{ throw Error('legacy door step'); };
  live.step(.5,0); assert.equal(door.openness,0);
  // A cold native body admitted after live initialization must become visible
  // to the door immediately; idle ticks need no scan of the unchanged slab.
  const unit = world.factory.build('soldier');
  unit.controller = {}; unit.networkHuman = unit.human = true;
  unit.spawn(0,0,0,0);
  live.step(.5,1);
  assert.equal(door.openness,.5); assert.equal(door.body.enabled,false);
  assert.equal(live.native.allocations,0); assert.equal(live.native.reallocations,0);
  live.presentAnimated(1/60,1);
  assert.equal(door.openness,.5); assert.equal(door.body.enabled,false);
  live.step(.5,2); assert.equal(door.openness,1);
});

test('one live call advances trap reset, spring and return without the old scheduler', async()=>{
  const { Trap } = await import('../src/traps.js');
  const trap = new Trap({class:'trap',base:'trap',settings:{MaxHealth:5},trap:{trigger:'swing'},matrix:new Matrix4().toArray()},
    {built:{root:new Object3D(),bones:[],byName:{}},pad:null},{},null,new Map([['swing',{frames:3}]]));
  const world = await f3World({soldier},{prepareLive:()=>({buildings:[trap],animated:[],traps:[trap]})});
  const live = new F4Live(world.phase,{sound(){}});
  trap.step = ()=>{ throw Error('legacy trap tick'); };
  trap.setHealth(5); assert.equal(trap.state,'built'); assert.equal(trap.started,null);
  live.step(1/60,1); assert.equal(trap.phase,'armed'); assert.equal(trap.started,1);
  assert.equal(trap.damage(5),true); assert.equal(trap.sprung,true);
  live.step(1/60,2); assert.equal(trap.phase,'swing');
  live.step(.2,3); assert.equal(trap.phase,'down'); assert.equal(trap.state,'unbuilt'); assert.equal(trap.health,0);
  assert.equal(live.native.allocations,0); assert.equal(live.native.reallocations,0);
});

test('live mirror detector checks the ordinary facade before another tick', async()=>{
  const wall = building('wall',0,2);
  const world = await f3World({soldier},{prepareLive:()=>({buildings:[wall],animated:[],traps:[]})});
  const live = new F4Live(world.phase,{sound(){},debug:true});
  wall.health = 3;
  assert.throws(()=>live.step(1/60,1),/mirror write: building\/0\/health/);
});

test('native carry reaches a later door and swallow completes death and removal in the same call', async()=>{
  const root = new Object3D(), hand = new Object3D(); hand.name = 'hand'; hand.position.x = 30; root.add(hand);
  const rig = {root,bones:[hand],byName:{hand},bind:new Map([[hand,{q:hand.quaternion.clone(),p:hand.position.clone()}]])};
  const idle = {frames:2,fps:1,rootMotion:false,tracks:[],data:new Float32Array(),duration:1};
  const bodyClip = {...idle,tracks:[1],data:new Float32Array([0,0,0,1,0,0,0,0,0,0,1,0,0,0])};
  const creature = new AnimatedProp({base:'animatedprop',matrix:new Matrix4().toArray(),animation:{clips:['idle','grab'],attach:[{action:'grab',node:'hand',window:10000,radius:100,clip:'held'}]}},rig,null,new Map([['idle',idle],['grab',idle]]));
  creature.slot = 1;
  const doorRoot = new Object3D(), doorBone = new Object3D(); doorRoot.add(doorBone);
  const doorRig = {root:doorRoot,bones:[doorBone],byName:{},bind:new Map([[doorBone,{q:doorBone.quaternion.clone(),p:doorBone.position.clone()}]])};
  const door = new AnimatedProp({base:'door',matrix:new Matrix4().toArray(),animation:{clips:['open'],trigger:{action:'open',radius:5}}},doorRig,null,new Map([['open',idle]]));
  const names = ['DummyRoot','bone_root','bone_pelvis','bone_neck','bone_head','bone_l_thigh','bone_l_calf','bone_l_foot','bone_r_thigh','bone_r_calf','bone_r_foot'];
  const world = await f3World({soldier},{
    prepareLive:()=>({buildings:[],animated:[creature,door],traps:[]}),
    prepareUnit:unit=>{
      for (const name of names) { const bone = new Object3D(); bone.name = name; (unit.actor.bones[0] ?? unit.actor.root).add(bone); unit.actor.bones.push(bone); }
      unit.animator.clips.set('held',bodyClip);
    },
  });
  const unit = world.spawn('soldier',0,0,true);
  const live = new F4Live(world.phase,{sound(){}});
  live.step(0,1);
  assert.equal(unit.held.prop,creature); assert.equal(unit.position.x,30);
  assert.equal(creature.victim.unit,unit); assert.equal(door.openness,0);
  const head = unit.hitShapes().find(shape=>shape.part === 'head');
  assert.ok(head); assert.equal(head.center.x,30.05); assert.equal(head.center.y,.04);
  assert.equal(live.native.allocations,0); assert.equal(live.native.reallocations,0);
  for (let tick=2;tick<=1200;tick++) live.step(0,tick);
  assert.equal(live.counters.rustAllocations,0); assert.equal(live.counters.rustReallocations,0);
  creature.play(0,true,1201); live.step(1/60,1201);
  assert.equal(unit.alive,false); assert.equal(unit.removed,true); assert.equal(unit.held,null); assert.equal(creature.victim,null);
});
