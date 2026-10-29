import { fixture as flightFixture } from './flight-fixture.js';
import { initFlight } from '../src/flight/model.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld, makeHit } from '../src/collision.js';
import { Vehicles, VehicleDriver } from '../src/vehicles.js';
import { Ordnance } from '../src/ordnance.js';
import { LiveObjects } from '../src/props.js';

const box = (x, y, z, w, h, d) => {
  const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
};
const collision = () => { const b = box(0,-1,0,200,2,200);return new CollisionWorld(b.positions,b.indices,b.polygons); };
const identity = () => new THREE.Matrix4();
const shape = c => {const b=box(0,1,0,2,2,4);return c.createBodyShape(b.positions,b.indices,b.polygons)};
const controls = (held = false) => ({take: () => false, held: () => held});

function fixture() {
  const c=collision(), hull=box(0,1,0,2,2,4),scene=new THREE.Scene();
  const d={model:'tank',label:'Test tank',settings:{MaxHealth:1000,ForwardSpeed:20,ReverseSpeed:8,StrafeSpeed:4,Acceleration:10,Deceleration:10,SetAltitude:.5},seats:[{name:'BODY',settings:{},weapons:[]}],explosion:null};
  const data={models:{tank:{bones:[],parts:[]}},collision:{tank:hull},classes:{tank:d,enemy:d},spawns:[{name:'pad',matrix:identity().toArray(),team:1,post:'cp',respawnTime:2,classes:{1:'tank',2:'enemy'}}]};
  const post={desc:{name:'cp'},team:1};
  const system=new Vehicles({scene,data,buffer:new ArrayBuffer(0),indexBuffer:new ArrayBuffer(0),collision:c,texture:()=>null,camera:new THREE.PerspectiveCamera(),posts:{posts:[post]},ordnance:{explosion(){}},effects:{play(){return null}}});
  return {c,system,post,v:system.vehicles[0]};
}

test('moving body keeps one BVH while ray, overlap and soldier contacts follow rigid transforms',()=>{
  const c=collision(),h=shape(c),m=identity().makeTranslation(10,0,0),body=c.addMovingBody(h,m);
  const hit=makeHit();
  assert.equal(c.cast(new THREE.Vector3(10,1,-10),new THREE.Vector3(0,0,1),30,hit).body,body);
  assert.equal(c.cast(new THREE.Vector3(0,1,-10),new THREE.Vector3(0,0,1),30,hit),null);
  m.makeRotationY(Math.PI/2).setPosition(20,0,0);c.moveBody(body,m);
  assert.equal(body.bvh,h.bvh);
  assert.ok(c.overlapsSphere(new THREE.Vector3(21.8,1,0),.4));
  assert.equal(c.overlapsSphere(new THREE.Vector3(21.8,1,0),.4,body),false);
  const polygons=c.gather(new THREE.Vector3(21.8,1,0),.4);
  assert.ok(polygons.length>0);
  assert.ok(polygons.some(p=>p.triangles.some(t=>Math.max(t.a.x,t.b.x,t.c.x)>21)));
  assert.equal(c.cast(new THREE.Vector3(10,1,-10),new THREE.Vector3(0,0,1),30,hit),null);
  body.enabled=false;
  assert.equal(c.overlapsSphere(new THREE.Vector3(21.8,1,0),.4),false);
});

test('projectile tests moving target at arrival, not its location at fire time',()=>{
  const c=collision(),body=c.addMovingBody(shape(c),identity().makeTranslation(20,0,5));
  let hits=0;
  const ord=new Ordnance({scene:new THREE.Scene(),collision:c,effects:{play(){}},lights:{add(){}},texture:()=>null,onHit:b=>{assert.equal(b,body);hits++}});
  const def={base:'bolt',Velocity:10,LifeSpan:3,Damage:100};
  ord.fire(def,new THREE.Vector3(0,1,0),new THREE.Vector3(0,0,1),null);
  const bolt=ord.bolts[0];
  c.moveBody(body,identity().makeTranslation(0,0,5));
  ord.stepBolt(bolt,.5);
  assert.equal(hits,1);
  assert.equal(bolt.position.z,3);
  ord.fire(def,new THREE.Vector3(0,1,0),new THREE.Vector3(0,0,1),null);
  const second=ord.bolts[1];
  c.moveBody(body,identity().makeTranslation(20,0,5));
  ord.stepBolt(second,1);
  assert.equal(hits,1);
  assert.ok(second.life>0);
});

test('straight rockets also hit vehicles entering their flight segment',()=>{
  const c=collision(),body=c.addMovingBody(shape(c),identity().makeTranslation(20,0,5));
  let hits=0;
  const ord=new Ordnance({scene:new THREE.Scene(),collision:c,effects:{play(){}},lights:{add(){}},model:()=>null,onHit:()=>hits++});
  ord.fire({base:'bullet',Velocity:10,LifeSpan:3},new THREE.Vector3(0,1,0),new THREE.Vector3(0,0,1),null);
  c.moveBody(body,identity().makeTranslation(0,0,5));
  ord.stepRound(ord.rounds[0],.5);
  assert.equal(hits,1);
  assert.equal(ord.rounds[0].gone,true);
});

test('movement is display-rate independent and parked vehicles sleep',()=>{
  const runs=[30,60,144].map(rate=>{
    const {system,v}=fixture();v.driver={};
    for(let i=0;i<rate*3;i++)system.update(1/rate,v,{forward:1,right:0,turn:0});
    const z=v.position.z;
    v.driver=null;
    for(let i=0;i<rate*4;i++)system.update(1/rate);
    assert.ok(v.sleeping);
    return z;
  });
  for(const z of runs)assert.ok(Math.abs(z-runs[0])<1e-8,JSON.stringify(runs));
  assert.ok(runs[0]>35);
});

test('thin walls stop a fast hover vehicle without rebuilding its collider',()=>{
  const {system,c,v}=fixture();
  const wall=box(0,3,12,50,6,.1);c.addBody(wall.positions,wall.indices,wall.polygons);
  const bvh=v.body.bvh;v.driver={};
  for(let i=0;i<300;i++)system.update(1/30,v,{forward:1,right:0,turn:0});
  assert.ok(v.position.z<10.1,`wall tunneled: ${v.position.z}`);
  assert.ok(v.position.z>8);
  assert.equal(v.body.bvh,bvh);
});

test('destruction is idempotent, respawn waits, neutral CP suppresses it, capture changes faction',()=>{
  const {system,v,post}=fixture();let destroyed=0;system.onDestroyed=()=>destroyed++;
  v.damage(2000);v.damage(2000);
  assert.equal(destroyed,1);assert.equal(v.body.enabled,false);
  system.updateSpawns(1);assert.equal(v.alive,false);
  post.team=0;system.updateSpawns(5);assert.equal(system.vehicles.length,1);
  post.team=2;system.updateSpawns(.1);
  assert.equal(system.slots[0].vehicle.name,'enemy');assert.equal(system.slots[0].vehicle.health,1000);
  assert.equal(v.alive,false);
  const n=system.collision.bodies.length;
  system.dispose();assert.ok(system.collision.bodies.length<n);
});

test('occupied pads delay respawn instead of overlapping a soldier',()=>{
  const {system,v}=fixture();v.damage(Infinity);
  system.spawnOccupant=new THREE.Vector3(0,0,0);system.updateSpawns(3);
  assert.equal(v.alive,false);
  system.spawnOccupant.set(50,0,50);system.updateSpawns(1);
  assert.equal(v.alive,true);
});

test('unlinked pads keep their explicit faction even when an unnamed CP is neutral',()=>{
  const {system,v,post}=fixture();v.slot.desc.post='';post.desc.name='';post.team=0;
  v.damage(Infinity);system.updateSpawns(3);
  assert.equal(v.alive,true);assert.equal(v.team,1);
});

test('damage scales and pilot repair apply to vehicle health',()=>{
  const {system,v,c}=fixture();system.struck(v.body,{Damage:100,VehicleScale:.5});assert.equal(v.health,950);
  const live=Object.create(LiveObjects.prototype);Object.assign(live,{playerTeam:1,vehicleTargets:system.vehicles,buildings:[],collision:c,center:new THREE.Vector3(),toward:new THREE.Vector3(),hit:makeHit()});
  const cutter={VehicleHealth:50,TargetVehicle:1,TargetEnemy:0,TargetFriendly:1,LockOnRange:10,LockOnAngle:20};
  const target=live.lockOn(new THREE.Vector3(0,1,-5),new THREE.Vector3(0,0,1),cutter,null,false);
  assert.equal(target,v);assert.equal(live.repair(cutter,v),true);assert.equal(v.health,1000);
});

test('beams maintain a continuous segment and integrate damage independent of frame rate',()=>{
 for(const rate of [30,60,144]){
  const c=collision(),body=c.addMovingBody(shape(c),identity().makeTranslation(0,0,10));let damage=0;
  const ord=new Ordnance({scene:new THREE.Scene(),collision:c,effects:{play(){}},lights:{add(){}},texture:()=>null,onHit:(b,d)=>{assert.equal(b,body);damage+=d.Damage}});
  ord.fire({base:'beam',Damage:300,LifeSpan:.6},new THREE.Vector3(0,1,0),new THREE.Vector3(0,0,1),null);
  while(ord.beams[0].life>0)ord.stepBeam(ord.beams[0],1/rate);
  assert.ok(Math.abs(damage-300)<1e-8,`${rate}: ${damage}`);
  assert.equal(ord.beams[0].end.z,8);
 }
});

test('aircraft ceiling recovery changes attitude smoothly rather than clamping position',()=>{
 const {system,v}=flightFixture();v.driver={};v.flightState='Flying';v.position.y=40;v.airspeed=v.midSpeed;
 system.flightEnvironment={ceiling:30};
 system.update(1/60,v,{});assert.ok(v.position.y>30);assert.equal(v.flightWarning,'Flight ceiling');
 for(let i=0;i<60;i++)system.update(1/60,v,{});
 assert.ok(new THREE.Vector3(0,0,1).applyQuaternion(v.quaternion).y<0);
});

test('vehicle-only architectural barriers are absent from infantry and projectile queries',()=>{
 const c=collision();const b=box(0,2,10,4,4,.1);c.vehicleBlockers=c.createBodyShape(b.positions,b.indices,b.polygons);
 const at=new THREE.Vector3(0,2,10);
 assert.equal(c.gather(at,.5).length,0);
 assert.ok(c.gather(at,.5,true).length>0);
 assert.equal(c.cast(new THREE.Vector3(0,2,0),new THREE.Vector3(0,0,1),20,makeHit()),null);
});

test('material-batched vehicle skinning matches rigid bone parts after posing and translation',async()=>{
 const {vehicleBatches,attachVehicleBatches}=await import('../src/vehicle-rig.js');
 const rig={bones:[{name:'hull',parent:-1,local:identity().toArray()},{name:'barrel',parent:0,local:identity().makeTranslation(0,2,1).toArray()}]};
 const geometry=new THREE.BoxGeometry(1,1,2);
 const parts=['hull','barrel'].map(bone=>({bone,texture:'shared',flags:1,geometry:geometry.clone()}));
 const batches=vehicleBatches(rig,parts,()=>null);
 assert.equal(batches.length,1,'same material is one draw despite different bones');
 const root=new THREE.Group(),bones=rig.bones.map(d=>{const b=new THREE.Bone();new THREE.Matrix4().fromArray(d.local).decompose(b.position,b.quaternion,b.scale);return b});
 root.add(bones[0]);bones[0].add(bones[1]);
 const skeleton=attachVehicleBatches(root,bones,batches);
 root.position.set(20,5,-10);root.rotation.y=.7;bones[1].rotation.x=.4;
 root.updateMatrixWorld(true);skeleton.update();
 const mesh=root.children.find(x=>x.isSkinnedMesh);const a=new THREE.Vector3(),b=new THREE.Vector3();
 for(let joint=0;joint<2;joint++)for(let i=0;i<geometry.attributes.position.count;i++){
  a.fromBufferAttribute(geometry.attributes.position,i).applyMatrix4(bones[joint].matrixWorld);
  mesh.getVertexPosition(joint*geometry.attributes.position.count+i,b);b.applyMatrix4(mesh.matrixWorld);
  assert.ok(a.distanceTo(b)<1e-5,`${joint}/${i}: ${a.toArray()} vs ${b.toArray()}`);
 }
});

// A snowspeeder-like flyer with a rear seat on an aft-facing turret node
// (towcablegun) and no hp_fire, and an AT-AT-like command walker with four
// named legs under a pelvis at 12.6 m, as the retail models have them.
function towFixture() {
  const c=collision(),scene=new THREE.Scene();
  const t=(x,y,z,ry=0)=>new THREE.Matrix4().makeRotationY(ry).setPosition(x,y,z).toArray();
  const speederRig={bones:[{name:'dummyroot',parent:-1,local:identity().toArray()},{name:'towcablegun',parent:0,local:t(0,1.2,-1.9,Math.PI)},{name:'hp_gunner',parent:0,local:t(0,.4,-.9,Math.PI)}],parts:[]};
  const legs=[];const legBones=[];
  for(const [name,x,z] of [['fl',2,4.7],['fr',-2,4.7],['bl',2,-5.1],['br',-2,-5.1]]){
    legBones.push({name:`bone_${name}_thigh`,parent:0,local:t(x,12.3,z)},{name:`bone_${name}_calf`,parent:0,local:t(x,7,z+1.5)},{name:`bone_${name}_foot`,parent:0,local:t(x,2.1,z)},{name:`bone_${name}_pelvis`,parent:0,local:t(x*.7,12.6,z*.3)});
  }
  const walkerRig={bones:[{name:'dummyroot',parent:-1,local:identity().toArray()},...legBones],parts:[]};
  const tow={base:'towcableweapon',class:'all_weap_fly_snowspeeder_reargun',RoundsPerClip:20,ShotDelay:3,ReloadTime:3,LockOnRange:70,LockTime:.4,TargetVehicle:1,TargetEnemy:1,ordnance:{class:'all_weap_fly_snowspeeder_reargun_ord',base:'towcable',Velocity:250,LifeSpan:2.5,CableLength:60}};
  const speeder={model:'speeder',label:'Snowspeeder',base:'flyer',settings:{MaxHealth:600,MaxSpeed:72,MinSpeed:20,TurnRate:1.25,PitchRate:.8},
    seats:[{name:'BODY',settings:{PilotPosition:'hp_pilot'},weapons:[]},{name:'TURRET',settings:{TurretNodeName:'towcablegun',YawLimits:'80 280',PitchLimits:'-15 25',TurnRate:1.3,PilotPosition:'hp_gunner',EyePointOffset:'0 2 4'},weapons:[{def:tow,aimers:[]}]}],explosion:null};
  const walker={model:'walker',label:'AT-AT',base:'commandwalker',settings:{MaxHealth:30000,MaxTurnSpeed:.3,ForwardSpeed:5},seats:[{name:'BODY',settings:{},weapons:[]}],explosion:null};
  const data={models:{speeder:speederRig,walker:walkerRig},collision:{speeder:box(0,.7,0,5,1.6,5.6),walker:box(0,10,3,8,20,21)},classes:{speeder,walker},
    spawns:[{name:'pad',matrix:identity().toArray(),team:1,post:'cp1',respawnTime:2,classes:{1:'speeder'}},{name:'walkerpad',matrix:identity().makeTranslation(0,0,-30).toArray(),team:2,post:'cp2',respawnTime:2,classes:{2:'walker'}}]};
  const posts=[{desc:{name:'cp1'},team:1},{desc:{name:'cp2'},team:2}];
  const system=new Vehicles({scene,data,buffer:new ArrayBuffer(0),indexBuffer:new ArrayBuffer(0),collision:c,texture:()=>null,camera:new THREE.PerspectiveCamera(),posts:{posts},ordnance:{explosion(){},fire(){},lights:{flash(){}}},effects:{play(){return null}}});
  const [v,w]=system.vehicles;
  return {c,system,v,w,scene};
}
// Flies the speeder round the walker by hand: at angle a on a circle of
// radius r, facing along the circle, the rear gun toward the walker.
function orbit(system,v,w,a,r=25){
  v.position.set(w.position.x+Math.sin(a)*r,1.5,w.position.z+Math.cos(a)*r);
  v.yaw=a+Math.PI/2;v.quaternion.setFromAxisAngle(new THREE.Vector3(0,1,0),v.yaw);v.flightPreviousQ.copy(v.quaternion);
  v.previous.copy(v.position);v.previousYaw=v.yaw;
  v.syncBody();v.render(1,0);
  system.updateTethers();
}

test('tow cable: the rear seat fires the harpoon from towcablegun, aft, and it holds a walker leg (not its body), with the attach sound',async()=>{
  const {emitSound,listenSounds}=await import('../src/audio/events.js');
  const sounds=[];const stop=listenSounds((type,...args)=>sounds.push([type,...args]));
  const {system,v,w}=towFixture();
  assert.equal(w.base,'commandwalker');assert.equal(w.legs.length,4);assert.ok(Math.abs(w.legTop-12.6)<1e-6,`the pelvis height: ${w.legTop}`);
  assert.equal(v.seats[1].view.turret,v.byName.towcablegun);
  // The player in the rear seat, aimed aft at the walker (the arc's middle).
  v.driver={};v.seatIndex=1;v.restAim(1);v.flightState='Flying';v.airspeed=v.minSpeed;
  assert.ok(Math.abs(Math.atan2(Math.sin(v.seats[1].aimYaw-v.yaw),Math.cos(v.seats[1].aimYaw-v.yaw)))>3,'the rear seat rests aft');
  system.update(1/60,v,{forward:0,right:0,turn:0});
  assert.ok(v.seats[1].target&&v.seats[1].target.z<-15,`the seat aims at the walker: ${v.seats[1].target?.z}`);
  const pressed=new Set(['firePrimary']);
  const controls={take:(k)=>pressed.delete(k),held:(k)=>k==='firePrimary'};
  v.fire(1/60,controls);
  assert.equal(v.shotNode,v.byName.towcablegun,'fired from the turret node, not the hull nose');
  assert.ok(v.muzzle.z-v.position.z<-1.5&&v.muzzle.z-v.position.z>-2.5,`the muzzle at the rear gun: ${v.muzzle.z}`);
  assert.equal(v.towTarget,system.vehicles.indexOf(w),'the harpoon holds the walker');
  assert.ok(/^bone_(fl|fr|bl|br)$/.test(v.towLeg),`a leg: ${v.towLeg}`);
  assert.ok(v.towLocal.y<w.legTop,'anchored below the pelvis');
  assert.equal(system.tethers.get(v).length,60,'CableLength from the ordnance');
  assert.deepEqual(sounds.filter(s=>s[0]==='towAttach').length,1,'the attach sound');
  assert.ok(sounds.some(s=>s[0]==='turretFire'),'and the gun\'s fire sound');
  // A harpoon into the body glances off: the player's; a bot's is re-aimed at a leg.
  system.releaseTow(v,'test');
  assert.equal(v.towTarget,-1);
  const high=new THREE.Vector3(0,15,-2),aft=new THREE.Vector3(0,0,-1);
  assert.equal(system.launchTow(v,v.seats[1].weapons[0].def,high,aft,1,null),false,'the body does not hold the harpoon');
  assert.equal(system.launchTow(v,v.seats[1].weapons[0].def,high,aft,1,{unit:null}),true,'a bot\'s harpoon finds the nearest leg');
  assert.ok(v.towLeg&&v.towLocal.y<w.legTop);
  stop();
});

test('tow cable: the cable winds round the legs over the passes, trips the walker on the third circuit, snaps past CableLength, and is drawn from plain state',async()=>{
  const {listenSounds}=await import('../src/audio/events.js');
  const sounds=[];const stop=listenSounds((type,...args)=>sounds.push([type,...args]));
  const {system,v,w,scene}=towFixture();
  v.driver={};v.seatIndex=1;v.restAim(1);v.flightState='Flying';v.airspeed=v.minSpeed;
  system.update(1/60,v,{forward:0,right:0,turn:0});
  const pressed=new Set(['firePrimary']);
  v.fire(1/60,{take:(k)=>pressed.delete(k),held:()=>true});
  assert.equal(v.towTarget,1);
  const a0=Math.atan2(v.position.x-w.position.x,v.position.z-w.position.z);
  for(let i=1;i<=120;i++)orbit(system,v,w,a0+i*Math.PI/60);
  assert.ok(v.towWinding>2*Math.PI-.01&&v.towWinding<2*Math.PI+.01,`one circuit wound: ${v.towWinding}`);
  assert.equal(w.alive,true,'still up after one');
  for(const key of ['towTarget','towWinding','towAngle'])assert.equal(typeof v[key],'number',key);
  assert.equal(typeof v.towLeg,'string');
  // The page draws it: the anchor, four wrap points for the circuit, the sagging span to the gun.
  system.present(1);
  const line=system.tethers.get(v).line;
  assert.ok(line&&line.visible&&scene.children.includes(line),'a line in the scene');
  assert.equal(line.geometry.drawRange.count,1+4+16,'anchor, four quarter wraps, sixteen along the span');
  const p=line.geometry.attributes.position;
  const anchor=new THREE.Vector3(p.getX(0),p.getY(0),p.getZ(0)),gun=new THREE.Vector3(p.getX(20),p.getY(20),p.getZ(20));
  assert.ok(anchor.distanceTo(w.position)<12,`the anchor on the walker: ${anchor.distanceTo(w.position)}`);
  assert.ok(gun.distanceTo(v.position)<3,`the other end at the gun: ${gun.distanceTo(v.position)}`);
  // Unwinding the other way lets it out; three circuits bring it down.
  for(let i=1;i<=30;i++)orbit(system,v,w,a0+(120-i)*Math.PI/60);
  assert.ok(v.towWinding<2*Math.PI-1,`unwound: ${v.towWinding}`);
  for(let i=1;i<=400&&w.alive;i++)orbit(system,v,w,a0+(90+i)*Math.PI/60);
  assert.equal(w.alive,false,'the walker fell');
  assert.equal(v.towTarget,-1,'the cable let go');
  assert.ok(sounds.some(s=>s[0]==='towRelease'),'the release sound');
  system.present(1);
  assert.equal(line.visible,false,'nothing drawn');
  // A new walker, a new cable, too far: it snaps.
  w.spawn(2);
  v.position.set(0,1.5,-5);v.yaw=0;v.previous.copy(v.position);v.previousYaw=0;v.syncBody();v.render(1,0);
  v.driver={};v.seatIndex=1;v.restAim(1);v.flightState='Flying';v.airspeed=v.minSpeed;
  system.update(1/60,v,{forward:0,right:0,turn:0});
  pressed.add('firePrimary');v.seats[1].weapons[0].state.wait=0;
  v.fire(1/60,{take:(k)=>pressed.delete(k),held:()=>true});
  assert.equal(v.towTarget,1,'held again');
  orbit(system,v,w,0,80);
  assert.equal(v.towTarget,-1,'snapped past CableLength');
  system.dispose();assert.equal(system.tethers.size,0);
  stop();
});

test('tow cable: the gun takes no trigger while its cable holds, so a second press neither fires nor starts the circuits over; once it lets go the gun fires again',()=>{
  const {system,v,w}=towFixture();
  v.driver={};v.seatIndex=1;v.restAim(1);v.flightState='Flying';v.airspeed=v.minSpeed;
  system.update(1/60,v,{forward:0,right:0,turn:0});
  const gun=v.seats[1].weapons[0];
  const pressed=new Set(['firePrimary']);
  const controls={take:(k)=>pressed.delete(k),held:()=>true};
  let launched=0;const launch=system.launchTow.bind(system);system.launchTow=(...args)=>{launched++;return launch(...args);};
  v.fire(1/60,controls);
  assert.equal(launched,1);assert.equal(v.towTarget,1,'the first harpoon holds');
  assert.equal(v.towHeld(gun),true,'the gun is held');
  const a0=Math.atan2(v.position.x-w.position.x,v.position.z-w.position.z);
  for(let i=1;i<=60;i++)orbit(system,v,w,a0+i*Math.PI/60);
  const wound=v.towWinding;
  assert.ok(wound>Math.PI-.01,`half a circuit: ${wound}`);
  // The shot delay long past, the trigger pressed and held again.
  gun.state.wait=0;pressed.add('firePrimary');
  for(let i=0;i<30;i++)v.fire(1/60,controls);
  assert.equal(launched,1,'no second harpoon');
  assert.equal(v.towTarget,1,'the cable still holds');
  assert.equal(v.towWinding,wound,'the circuits wound count on');
  system.releaseTow(v,'test');
  assert.equal(v.towHeld(gun),false);
  gun.state.wait=0;pressed.add('firePrimary');v.fire(1/60,controls);
  assert.equal(launched,2,'free again, the gun fires');
});

test('tow cable: something between the gun and the walker (another walker, a wall, the ground) does not cut the cable; stretched past 1.2 x CableLength it snaps',()=>{
  const {c,system,v,w}=towFixture();
  v.driver={};v.seatIndex=1;v.restAim(1);v.flightState='Flying';v.airspeed=v.minSpeed;
  system.update(1/60,v,{forward:0,right:0,turn:0});
  const pressed=new Set(['firePrimary']);
  v.fire(1/60,{take:(k)=>pressed.delete(k),held:()=>true});
  assert.equal(v.towTarget,1,'held');
  // A wall right across the cable, between the speeder and the walker.
  const wall=box(0,5,-15,30,10,.5);c.addBody(wall.positions,wall.indices,wall.polygons);
  const a0=Math.atan2(v.position.x-w.position.x,v.position.z-w.position.z);
  for(let i=1;i<=10;i++)orbit(system,v,w,a0+i*Math.PI/60);
  assert.equal(v.towTarget,1,'the wall between does not cut it');
  assert.ok(v.towWinding>0,'and it winds on');
  orbit(system,v,w,a0+11*Math.PI/60,90);
  assert.equal(v.towTarget,-1,'stretched past 72 m it snaps');
});

test('tow cable: a bot takes the rear seat looking aft into its arc (YawLimits 80 280), not along the hull past its limits',()=>{
  const {system,v}=towFixture();
  v.flightState='Flying';
  const soldier={position:new THREE.Vector3(),previous:new THREE.Vector3(),velocity:new THREE.Vector3(),aimYaw:0,aimPitch:0,air:null,spawn(){}};
  const unit={id:7,generation:1,team:1,alive:true,dead:false,human:false,walker:false,seat:null,classDef:{},intent:{},soldier,
    actor:{root:new THREE.Group(),byName:{}},animator:{clips:new Map(),gunner(){},reset(){}},presentation:{reset(){}},combat:{mounted:false,dead:false,team:1}};
  assert.ok(system.board(unit,v,1),'boarded the rear seat');
  const off=Math.atan2(Math.sin(v.seats[1].aimYaw-v.yaw),Math.cos(v.seats[1].aimYaw-v.yaw));
  assert.ok(Math.abs(Math.abs(off)-Math.PI)<1e-9,`aft, the arc's middle: ${off}`);
  assert.equal(v.restYaw(0),v.yaw,'the pilot\'s seat rests along the hull');
});

test('fatray: the LR1K sonic round strikes everyone within its width along its flight, once each, and stops at a wall; a flak bolt bursts where its flight ends',()=>{
  const c=collision();
  const wall=box(0,3,30,20,6,.2);c.addBody(wall.positions,wall.indices,wall.polygons);
  const unit=(x,z)=>({alive:true,team:2,sphere:{center:new THREE.Vector3(x,1,z),radius:.6},seat:null,hitShapes:[]});
  const near=unit(.8,10),wide=unit(3.5,12),behind=unit(0,40);
  const units={list:[near,wide,behind],query(center,radius,out=[],filter){out.length=0;for(const u of this.list){const reach=radius+u.sphere.radius;if(u.sphere.center.distanceToSquared(center)>reach*reach)continue;if(filter&&!filter(u))continue;out.push(u);}return out;},raycast(){return null;}};
  const struck=[],explosions=[];
  const ord=new Ordnance({scene:new THREE.Scene(),collision:c,effects:{play(){return null}},lights:{add(){}},texture:()=>null,onHit(){}});
  ord.units=units;ord.onHitUnit=(u,def)=>struck.push([u,def.Damage]);
  ord.explosion=(def,at)=>explosions.push([def,at.clone()]);
  const fat={base:'fatray',class:'geo_weap_bldg_geoturret_ord',Velocity:100,LifeSpan:.45,Damage:400,TrailEffect:'geo_blunderbuss_blast'};
  ord.fire(fat,new THREE.Vector3(0,1,0),new THREE.Vector3(0,0,1),null);
  assert.equal(ord.rounds.length,1);assert.equal(ord.rounds[0].kind,'fatray');
  for(let i=0;i<60;i++)ord.update(1/60,false);
  assert.deepEqual(struck.map(([u])=>u),[near],'the unit inside the ray\'s width is struck once; the one 3.5 m off and the one behind the wall are not');
  assert.equal(struck[0][1],400);
  assert.equal(ord.rounds.length,0,'stopped at the wall');
  // The AAA turret's flak: a bolt with ExplosionExpire bursts at the end of its flight in the open.
  const flak={base:'bolt',class:'kam_weap_bldg_AAA_turret_cannon_ord',Velocity:450,LifeSpan:.05,ExplosionExpire:{class:'kam_weap_bldg_AAA_turret_cannon_exp',Damage:300,DamageRadiusOuter:4}};
  ord.fire(flak,new THREE.Vector3(0,20,0),new THREE.Vector3(0,0,1),null);
  for(let i=0;i<10;i++)ord.update(1/60,false);
  assert.equal(explosions.length,1,'burst once');
  assert.ok(Math.abs(explosions[0][1].z-22.5)<1,`where the flight ended: ${explosions[0][1].z}`);
});

test('spawn clearance permits narrow side-by-side fighters while rejecting overlapping hulls',async()=>{
 const {vehicleFootprintsOverlap}=await import('../src/vehicles.js');
 const hull={center:new THREE.Vector3(0,1,0),size:new THREE.Vector3(2,2,12)};
 assert.equal(vehicleFootprintsOverlap(hull,identity(),hull,identity().makeTranslation(3,0,0)),false);
 assert.equal(vehicleFootprintsOverlap(hull,identity(),hull,identity().makeTranslation(1,0,0)),true);
 assert.equal(vehicleFootprintsOverlap(hull,identity(),hull,identity().makeTranslation(0,10,0)),false);
});

test('blocked exits retain control, valid exits restore infantry without changing health',()=>{
 const {system,v,c}=fixture();
 const soldier={position:new THREE.Vector3(),previous:new THREE.Vector3(),settled:new THREE.Vector3(),velocity:new THREE.Vector3(),spawn(x,y,z,yaw){this.position.set(x,y,z);this.aimYaw=yaw}};
 const combat={dead:false,health:75,mounted:false};
 const driver=new VehicleDriver({vehicles:system,soldier,combat,actor:{root:new THREE.Group()},animator:{},clips:new Map(),camera:system.camera,collision:c,hud:{message(){}}});
 assert.equal(driver.enter(v),true);assert.equal(combat.mounted,true);
 const cast=c.cast;c.cast=()=>null;
 assert.equal(driver.exit(),false);assert.equal(driver.active,true);
 c.cast=cast;
 assert.equal(driver.exit(),true);assert.equal(driver.active,false);assert.equal(combat.mounted,false);assert.equal(combat.health,75);
});

test('vehicle right strafe and right steering agree with the camera, with bounded yaw acceleration',()=>{
 const {system,v}=fixture();v.driver={};
 for(let i=0;i<120;i++)system.update(1/120,v,{forward:0,right:1,turn:0});
 assert.ok(v.position.x < -2,'D should move screen-right when looking along +Z');
 v.spawn(1);v.driver={};v.strafeSpeed=0;
 let previousRate=0;
 for(let i=0;i<120;i++) {
  system.update(1/120,v,{forward:1,right:1,turn:0});
  assert.ok(Math.abs(v.yawRate-previousRate) <= v.turnRate*9/120 + 1e-8);
  previousRate=v.yawRate;
 }
 assert.ok(v.yaw < -.5,'D should turn right, the same direction as mouse-right');
 // On level ground a hover turns flat: the original game leans it only by its
 // tiny BankAngle, which the port leaves out (src/vehicle-hover.js).
 assert.ok(v.position.x < 0);assert.ok(Math.abs(v.roll)<.01,String(v.roll));
});

test('a hover vehicle gently bobs while its sleeping collider remains fixed',()=>{
 const {system,v}=fixture();for(let i=0;i<120;i++)system.update(1/60);
 assert.equal(v.sleeping,true);
 const physics=v.matrix.clone(),heights=[];
 for(let i=0;i<180;i++){system.update(1/60);heights.push(v.root.position.y);}
 assert.deepEqual(v.matrix.elements,physics.elements);
 assert.ok(Math.max(...heights)-Math.min(...heights)>.05);
 assert.ok(Math.max(...heights)-Math.min(...heights)<.2);
});

test('ships repeatedly launch with Space and settle with an explicit landing request',()=>{
 const {system,v}=flightFixture();v.driver={};system.flightEnvironment={ceiling:500};
 for(let cycle=0;cycle<3;cycle++) {
  system.update(1/60,v,{takeoff:true});for(let i=0;i<200;i++)system.update(1/60,v,{});
  assert.equal(v.flightState,'Flying');
  for(let i=0;i<180;i++)system.update(1/60,v,{forward:-1});
  system.update(1/60,v,{takeoff:true});for(let i=0;i<1200&&v.flightState==='Landing';i++)system.update(1/60,v,{});
  assert.equal(v.flightState,'Landed');assert.equal(v.health,v.maxHealth);
  const y=v.position.y;for(let i=0;i<60;i++)system.update(1/60,v,{});assert.equal(v.position.y,y);
 }
 for(let i=0;i<60;i++)system.update(1/60,v,{forward:1});assert.equal(v.flightState,'Landed');
});

test('vehicle camera eases turns and follows motion without placing its eye behind walls',()=>{
 const {system,v,c}=fixture();v.walking=true; // Exercise the shared turret/seat camera.
 const driver=new VehicleDriver({vehicles:system,camera:system.camera,collision:c});driver.vehicle=v;
 driver.updateCamera(1/60,false);const original=system.camera.quaternion.clone();
 v.aimYaw=.7;driver.updateCamera(1/60,false);
 const step=original.angleTo(system.camera.quaternion);
 assert.ok(step>0&&step<.4,'one frame must not snap to the new heading');
 for(let i=0;i<120;i++)driver.updateCamera(1/60,false);
 const look=system.camera.getWorldDirection(new THREE.Vector3());
 assert.ok(look.dot(v.viewDirection(new THREE.Vector3()))>.999);
 const wall=box(0,6,-4,100,12,.1);c.addBody(wall.positions,wall.indices,wall.polygons);
 v.aimYaw=0;
 for(let i=0;i<60;i++)driver.updateCamera(1/60,false);
 assert.ok(system.camera.position.z>-4);
});

test('armed vehicle seats without retail reticle art still draw a readable aiming mark',async()=>{
 const {BattleHud}=await import('../src/hud.js');const hud=Object.defineProperty(Object.create(BattleHud.prototype),'height',{value:768});
 const quads=[];hud.vehicle={};hud.texture=()=>null;hud.solid={};hud.anchor=()=>identity();
 hud.batch={quad:(...args)=>quads.push(args)};
 hud.setReticle(.5,{def:{}},.6);hud.drawReticle();assert.equal(quads.length,4);
 hud.vehicle=null;quads.length=0;hud.drawReticle();assert.equal(quads.length,0);
});

test('a landing over ground steeper than the 2004 game settles on stays on its way down until he lifts off',()=>{
 const {system,v,c}=fixture();
 const pad=new THREE.BoxGeometry(30,2,30).rotateX(.8).translate(0,4,0);
 c.addBody(pad.attributes.position.array,pad.index.array,new Uint32Array(pad.index.count/3));
 v.flying=true;initFlight(v);v.driver={};v.flightState='Flying';v.position.y=12;v.airspeed=0;
 system.update(1/60,v,{takeoff:true});assert.equal(v.landingReason,'No landing zone','no TO LAND over it');assert.equal(v.flightState,'Landing','the key still lands');
 for(let i=0;i<600;i++)system.update(1/60,v,{});
 assert.equal(v.flightState,'Landing','not down on a 46 degree slope');
 system.update(1/60,v,{takeoff:true});assert.equal(v.flightState,'TakingOff','late in a landing the key lifts it off again');
});

test('skinned vehicle triangles retain different joints across a shared triangle',async()=>{
 const {vehicleBatches,attachVehicleBatches}=await import('../src/vehicle-rig.js');
 const rig={bones:[{name:'hip',parent:-1,local:identity().toArray()},{name:'leg',parent:0,local:identity().makeTranslation(1,0,0).toArray()}]};
 const geometry=new THREE.BufferGeometry();
 geometry.setAttribute('position',new THREE.Float32BufferAttribute([0,0,0,1,0,0,1,1,0],3));
 geometry.setAttribute('normal',new THREE.Float32BufferAttribute([0,0,1,0,0,1,0,0,1],3));
 geometry.setAttribute('uv',new THREE.Float32BufferAttribute([0,0,1,0,1,1],2));geometry.setIndex([0,1,2]);
 const batches=vehicleBatches(rig,[{geometry,joints:[0,1,1],texture:'skin',flags:1}],()=>null);
 const root=new THREE.Group(),hip=new THREE.Bone(),leg=new THREE.Bone();leg.position.x=1;hip.add(leg);root.add(hip);
 const skeleton=attachVehicleBatches(root,[hip,leg],batches);
 leg.rotation.z=Math.PI/4;root.updateMatrixWorld(true);skeleton.update();
 const mesh=root.children.find(o=>o.isSkinnedMesh),point=new THREE.Vector3();
 mesh.getVertexPosition(0,point);assert.ok(point.length()<1e-8);
 mesh.getVertexPosition(2,point);assert.ok(point.distanceTo(new THREE.Vector3(1-Math.SQRT1_2,Math.SQRT1_2,0))<1e-7);
 assert.equal(batches[0].geometry.index.count,3);
});

test('rider pelvis stays attached to the animated saddle without double root height',()=>{
 const {system,v,c}=fixture();v.healthType='animal';
 const saddle=new THREE.Object3D();saddle.position.set(0,2.4,0);v.root.add(saddle);v.byName.hp_active=saddle;
 v.seats[0].view.rider={anim:'ride',clip:'human_ride',ninePose:false,node:saddle};
 const root=new THREE.Group(),dummy=new THREE.Bone(),pelvis=new THREE.Bone();root.add(dummy);dummy.add(pelvis);
 dummy.position.y=2.1;pelvis.position.y=.5;root.position.y=50;
 const driver=new VehicleDriver({vehicles:system,camera:system.camera,collision:c,actor:{root,byName:{bone_pelvis:pelvis}}});driver.vehicle=v;
 for(let i=0;i<60;i++){
  saddle.position.y=2.4+Math.sin(i*.1)*.04;v.root.updateMatrixWorld(true);
  driver.presentRider(1/60);
  const a=saddle.getWorldPosition(new THREE.Vector3()),b=pelvis.getWorldPosition(new THREE.Vector3());
  assert.ok(a.distanceTo(b)<1e-8);
 }
 assert.ok(root.position.y<1,'authored two-metre root offset must not be added on top of the saddle');
});

test('the player\'s body stays hidden on an enclosed seat after a correction shows it again',()=>{
 const {system,v,c}=fixture();
 assert.equal(v.seats[0].view.rider,null,'no pilot animation: an enclosed seat');
 const root=new THREE.Group();
 const driver=new VehicleDriver({vehicles:system,camera:system.camera,collision:c,actor:{root,byName:{}}});driver.vehicle=v;
 // Each own correction (EntityGameState.correct) shows the body; the frame's
 // presentation hides it again before it is drawn on top of the hull.
 for(let i=0;i<3;i++){root.visible=true;driver.presentRider(1/60);assert.equal(root.visible,false);}
 // An exposed seat keeps him shown, at its rider node.
 const node=new THREE.Object3D();node.position.set(0,2.4,.5);v.root.add(node);v.root.updateMatrixWorld(true);
 v.seats[0].view.rider={anim:'drive',clip:'human_drive',ninePose:false,node};
 root.visible=true;driver.presentRider(1/60);
 assert.equal(root.visible,true);
 assert.ok(root.position.distanceTo(node.getWorldPosition(new THREE.Vector3()))<1e-9);
});

test('ground vehicle mouse aim responds on the next tick at any display rate while hull turn stays bounded', () => {
 for (const hz of [20, 30, 60, 144]) {
  const {v}=fixture(); v.driver={};
  const driver=new VehicleDriver({}); driver.vehicle=v;
  const yaw=v.aimYaw, hull=v.yaw;
  driver.look(64, -32, 8, false);
  v.steer(1/hz, {forward:0,right:0,turn:0});
  assert.ok(Math.abs(v.aimYaw-(yaw-.25))<1e-10);
  assert.equal(v.pitch, .125);
  assert.ok(Math.abs(v.yaw-hull)<=v.turnRate/hz);
 }
});

test('vehicle reticle previews queued input once, preserves per-command sensitivity, and never mutates the seat', () => {
 const {v}=fixture(), driver=new VehicleDriver({}); driver.vehicle=v;
 const before={...v.seat};
 const pending=[{look:[[64,0]],mouseScale:8,invertMouse:false}];
 const live=[[0,32]], out=new THREE.Vector3();
 driver.preview(pending,live,4,true);
 const expected=new THREE.Vector3(Math.sin(-.25)*Math.cos(.0625),Math.sin(.0625),Math.cos(-.25)*Math.cos(.0625));
 assert.ok(driver.viewDirection(out).distanceTo(expected)<1e-10);
 assert.deepEqual(v.seat,before);
 driver.look(64,0,8,false); v.aimPlayerSeat(1/60);
 driver.preview([],live,4,true);
 assert.ok(driver.viewDirection(out).distanceTo(expected)<1e-10,'acknowledgement must not double apply look');
 driver.preview([],[],4,true);
 assert.ok(driver.viewDirection(out).distanceTo(v.viewDirection(new THREE.Vector3()))<1e-10,'clearing input drops its preview');
});

test('vehicle preview clamps each mouse event before a reversal at pitch and yaw limits', () => {
 const {v}=fixture(), driver=new VehicleDriver({}); driver.vehicle=v;
 v.seat.view.pitchLimits=[-.2,.2]; v.seat.view.yawLimits=[-.3,.3];
 const counts=[[2048,-2048],[-100,100]];
 driver.preview([],counts,1,false);
 const shown=driver.viewDirection(new THREE.Vector3()).clone();
 for (const [dx,dy] of counts) driver.look(dx,dy,1,false);
 v.aimPlayerSeat(1/60);
 assert.ok(shown.distanceTo(v.viewDirection(new THREE.Vector3()))<1e-10);
});

test('occupied seat and cockpit cameras show pending mouse aim without waiting for a simulation tick', () => {
 for (const first of [false,true]) {
  const {system,v,c}=fixture(); v.walking=true; v.driver={};
  const driver=new VehicleDriver({camera:system.camera,collision:c}); driver.vehicle=v;
  driver.updateCamera(1/60,first);
  driver.preview([],[[64,-16]],8,false);
  driver.updateCamera(1/144,first);
  const shown=driver.viewDirection(new THREE.Vector3());
  assert.ok(system.camera.getWorldDirection(new THREE.Vector3()).dot(shown)>.999999);
  assert.equal(v.aimYaw,0,'rendered look must not change simulation aim');
 }
});
