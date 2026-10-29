import test from 'node:test';import assert from 'node:assert/strict';
import {Soldier} from '../src/soldier.js';import {PlayerCombat} from '../src/combat.js';import {Arsenal,Weapon} from '../src/weapons.js';import {CollisionWorld} from '../src/collision.js';
import {captureOwnPlayer,decodeOwnPlayer,applyOwnPlayer,OwnPlayerPrerequisiteError} from '../src/runtime/network-entity-player.js';
import {capturePlayerCorrection} from '../src/runtime/network-entity-capture.js';
import {captureCompactSoldier as captureSoldierCorrection} from '../src/runtime/network-entity-soldier-compact.js';
const SOLDIER_REPLAY_SCHEMA=6,soldierReplayBytes=124;
import {encodeEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
import {fixture} from './flight-fixture.js';import {predictionVehicle} from '../src/runtime/vehicle-prediction.js';
import {captureCompactFlight as captureFlightCorrection} from '../src/runtime/network-entity-flight-compact.js';
import {mountFixture} from './mount-fixture.js';
import {captureMountedControl} from '../src/runtime/network-mounted-control.js';
import {captureWeaponState} from '../src/runtime/network-weapon-state.js';
const defs=[{class:'rifle',channel:0,RoundsPerClip:20,ammo:3,ZoomMin:2,ZoomMax:4,ShotDelay:.2,base:'cannon'},{class:'grenade',channel:1,RoundsPerClip:1,ammo:4,base:'grenade'}];
const registry={contentEpoch:42,classes:new Map([[1,'trooper']]),weapons:new Map([[1,'rifle'],[2,'grenade']]),inputCodes:['KeyW','KeyS','Mouse0','ShiftLeft'],bindingProfiles:new Map([[0,{}]])};
const plane=()=>new CollisionWorld(new Float32Array([-100,0,-100,100,0,-100,100,0,100,-100,0,100]),new Uint32Array([0,2,1,0,3,2]),new Uint32Array([0,1]));
function unit(world=plane()){
 const soldier=new Soldier(world,{});soldier.spawn(0,0,0,0);
 const combat=Object.create(PlayerCombat.prototype);Object.assign(combat,{arsenal:new Arsenal(defs,{player:true}),health:97,zoomStep:1,firstPerson:true,soldier,kick:{rate:.4,hold:.8,offset:.1},dead:false,deathTime:0});
 return {id:1,generation:7,team:1,className:'trooper',soldier,combat};
}
const view={mouseScale:1.25,aspect:16/9,invertMouse:true,firstPerson:true,viewTick:90,flightSettings:{sensitivity:1.2,invert:false,curve:1.35,bindings:{}},gamepad:{axes:[.2,-.3,.4,-.5],buttons:Array.from({length:16},(_,i)=>({pressed:i===11}))}};
const capture=(u,extra={})=>captureOwnPlayer({unit:u,tick:100,epoch:3,registry,classId:1,held:['KeyW','Mouse0'],view,...extra});

test('passenger own correction carries exact aim and acknowledged controls without hull prediction',()=>{
 const source=unit(),target=unit(),a=mountFixture().v,b=mountFixture().v;
 a.aiTarget={id:20,generation:2};b.aiTarget={id:20,generation:2};
 source.seat={vehicle:a,index:1};target.seat={vehicle:b,index:1};
 const mountedWeapons=()=>[0,1].map(channel=>({state:new Weapon({...defs[0],channel}),barrel:17}));
 a.seats[1].weapons=mountedWeapons();b.seats[1].weapons=mountedWeapons();
 a.seats[1].weapons[0].state.heat=.123456789;a.seats[1].weapons[1].state.wait=Number.EPSILON;
 Object.assign(a.seats[1],{aimYaw:.27,pitch:.12,lookYaw:.29,lookPitch:.15});
 const world={players:new Map([[1,{unit:source,mailbox:{down:['KeyW','Mouse0'],view}}]])};
 const record=capturePlayerCorrection({},world,1,{tick:100,epoch:3,registry});
 assert.deepEqual(record,capture(source));
 const data=decodeOwnPlayer(record);assert.equal(data.vehicleId,0);assert.equal(data.controlMode,2);
 b.position.set(99,17,-45);const hull=b.position.clone();
 const context=applyOwnPlayer({unit:target,epoch:3,registry,record});
 assert.deepEqual(captureMountedControl(target),captureMountedControl(source));assert.deepEqual(b.position,hull);
 assert.equal(b.seats[1].weapons[0].state.heat,.123456789);assert.equal(b.seats[1].weapons[1].state.wait,Number.EPSILON);
 assert.equal(context.mountId,20);assert.equal(context.mountSeat,1);assert.equal(context.reset,true);
 assert.deepEqual(context.held,['KeyW','Mouse0']);
 assert.equal(applyOwnPlayer({unit:target,epoch:3,registry,record,previous:context}).reset,false);
 const packet=encodeEntityPacket({epoch:3,packetId:1,tick:100,records:[record]});assert.ok(packet.length<=1100);
 console.log(`Own passenger with infantry/gamepad and exact aim: ${packet.length} bytes`);
 b.aiTarget.generation=3;target.soldier.position.x=123;const before=captureSoldierCorrection(target.soldier);
 assert.throws(()=>applyOwnPlayer({unit:target,epoch:3,registry,record}),/lease mismatch/);assert.deepEqual(captureSoldierCorrection(target.soldier),before);
 b.aiTarget.generation=2;target.seat=null;source.seat=null;
 assert.equal(applyOwnPlayer({unit:target,epoch:3,registry,record:capture(source),previous:context}).reset,true,'exit resets mounted prediction context');
});

test('stationary turret own correction restores aim velocity and rejects invalid control before body mutation',()=>{
 const source=unit(),target=unit();
 const make=()=>({aiTarget:{id:30,generation:4},yaw:.1,pitch:.2,yawVel:.3,pitchVel:-.4,gunYaw:.05,gunPitch:.12,weapon:new Weapon(defs[0]),barrel:23});
 source.seat={turret:make(),index:0};target.seat={turret:make(),index:0};target.seat.turret.yawVel=0;source.seat.turret.weapon.wait=.1;
 const record=capture(source),context=applyOwnPlayer({unit:target,epoch:3,registry,record});
 assert.equal(context.controlMode,3);assert.equal(context.mountId,30);assert.equal(target.seat.turret.yawVel,.3);
 const packet=encodeEntityPacket({epoch:3,packetId:1,tick:100,records:[record]});assert.ok(packet.length<=1100);
 console.log(`Own stationary turret with infantry/gamepad and exact aim: ${packet.length} bytes`);
 target.soldier.position.x=123;const before=captureSoldierCorrection(target.soldier),bad={...record,payload:record.payload.slice()};
 new DataView(bad.payload.buffer).setFloat64(bad.payload.length-8,NaN,true);
 assert.throws(()=>applyOwnPlayer({unit:target,epoch:3,registry,record:bad}),/invalid weapon state number/);
 assert.deepEqual(captureSoldierCorrection(target.soldier),before);
 source.combat.remotes={piloted:{}};assert.throws(()=>capture(source),/conflicting own control/);
});

test('indexed content registries preserve exact own correction bytes and reject stale indexes',()=>{
 const source=unit(),indexed={...registry,classIds:new Map([['trooper',1]]),weaponIds:new Map([['rifle',1],['grenade',2]])};
 const player={unit:source,mailbox:{down:['KeyW','Mouse0'],view,discarded:0}},world={players:new Map([[1,player]])};
 const args={tick:100,epoch:3,registry:indexed};
 assert.deepEqual(capturePlayerCorrection({},world,1,args),capture(source));
 indexed.classIds.set('trooper',2);
 assert.throws(()=>capturePlayerCorrection({},world,1,args),/class prerequisite/);
 indexed.classIds.set('trooper',1);indexed.weaponIds.set('rifle',2);
 assert.throws(()=>capturePlayerCorrection({},world,1,args),/weapon definition prerequisite/);
 indexed.weaponIds.delete('rifle');
 assert.throws(()=>capturePlayerCorrection({},world,1,args),/weapon definition prerequisite/);
});

test('bounded own envelope restores real weapon selection/zoom, ammo and exact replay context',()=>{
 const source=unit(),target=unit();source.combat.arsenal.current(0).clip=.75;source.combat.arsenal.current(0).wait=.12;source.soldier.posture='crouch';
 const record=capture(source),wire=encodeEntityPacket({epoch:3,packetId:1,tick:100,records:[record]}),decoded=decodeEntityPacket(wire).records[0];
 target.combat.zoomStep=0;target.combat.health=1;target.soldier.position.set(5,6,7);
 const context=applyOwnPlayer({unit:target,epoch:3,registry,record:decoded});
 assert.deepEqual(captureSoldierCorrection(target.soldier,SOLDIER_REPLAY_SCHEMA),captureSoldierCorrection(source.soldier,SOLDIER_REPLAY_SCHEMA));
 assert.equal(target.combat.zoom,2);assert.equal(target.combat.health,97);assert.equal(target.combat.arsenal.current(0).clip,.75);assert.equal(target.combat.arsenal.current(0).wait,.12);
 assert.deepEqual(context.held,['KeyW','Mouse0']);assert.deepEqual(context.view,view);assert.equal(context.reset,true);
 assert.equal(applyOwnPlayer({unit:target,epoch:3,registry,record,previous:context}).reset,false);
 assert.ok(wire.length<=1100);console.log(`Own infantry with two weapons + gamepad: ${wire.length} bytes; exact motion core ${soldierReplayBytes} bytes`);
});

test('own correction repairs complete inactive inventory and both selected channels atomically',()=>{
 const loadout=[...defs,{...defs[0],class:'spare',HeatRecoverRate:.1}],content={...registry,weapons:new Map([...registry.weapons,[3,'spare']])};
 const source=unit(),target=unit();source.combat.arsenal=new Arsenal(loadout);target.combat.arsenal=new Arsenal(loadout);
 source.combat.arsenal.selected[0]=1;source.combat.zoomStep=0;
 Object.assign(source.combat.arsenal.weapons[0],{clip:.45,heat:.82,wait:.017,reloadLeft:.3,limit:.3});
 Object.assign(source.combat.arsenal.weapons[1],{throwTime:.03,released:true,hidden:true,charge:.7,held:.4});
 Object.assign(source.combat.arsenal.weapons[2],{salvosLeft:2,salvoWait:.023,charging:true,held:.51,overheated:true});
 const record=capture(source,{registry:content,view:{...view,viewTick:90.625}});
 applyOwnPlayer({unit:target,epoch:3,registry:content,record});
 assert.deepEqual(target.combat.arsenal.selected,source.combat.arsenal.selected);
 assert.deepEqual(target.combat.arsenal.weapons.map(captureWeaponState),source.combat.arsenal.weapons.map(captureWeaponState));
 assert.equal(decodeOwnPlayer(record).view.viewTick,90.625);
 const bad={...record,payload:record.payload.slice()},decoded=decodeOwnPlayer(bad),last=decoded.weapons.at(-1);
 const needle=last.payload,at=bad.payload.findIndex((_,i)=>needle.every((v,n)=>bad.payload[i+n]===v));
 assert.ok(at>=0);bad.payload[at]=255;
 const before=capture(target,{registry:content});
 assert.throws(()=>applyOwnPlayer({unit:target,epoch:3,registry:content,record:bad}),/definition mismatch/);
 assert.deepEqual(capture(target,{registry:content}),before,'invalid inactive state cannot partly apply body or selection');
});

test('class, content, life, epoch and binding prerequisites fail without partial mutation',()=>{
 const source=unit(),target=unit(),record=capture(source),before=capture(target).payload;
 for(const args of [{epoch:4},{unit:{...target,generation:8}},{unit:{...target,className:'other'}},{registry:{...registry,contentEpoch:43}},{registry:{...registry,weapons:new Map()}},{registry:{...registry,bindingProfiles:new Map()}}]){
  assert.throws(()=>applyOwnPlayer({unit:target,epoch:3,registry,record,...args}),OwnPlayerPrerequisiteError);assert.deepEqual(capture(target).payload,before);
 }
 source.generation=8;const spawned=capture(source);target.generation=8;
 assert.equal(applyOwnPlayer({unit:target,epoch:3,registry,record:spawned,previous:{epoch:3,generation:7,classId:1}}).reset,true);
});

test('omitting overwritten Soldier scratch preserves continuation including zoom-scaled look and jump',()=>{
 const world=plane(),a=unit(world),b=unit(world);
 for(let tick=1;tick<=210;tick++){
  a.soldier.lookRaw(2/a.combat.zoom,1/a.combat.zoom,view.mouseScale,view.invertMouse);
  const input={forward:1,right:.2,turn:0,jump:tick===70,crouch:tick===170,prone:false};a.soldier.tick(input,1/60);a.combat.updateKick(1/60);
  if(tick===90){
   // Dirty every scratch vector before restore, proving replay does not rely on
   // convenient zero initialization for the fields omitted from schema4.
   for(const value of Object.values(b.soldier))if(value?.isVector3)value.set(55,-33,22);
   applyOwnPlayer({unit:b,epoch:3,registry,record:capture(a,{tick})});
  }else if(tick>90){b.soldier.lookRaw(2/b.combat.zoom,1/b.combat.zoom,view.mouseScale,view.invertMouse);b.soldier.tick(input,1/60);b.combat.updateKick(1/60);}
  if(tick>=90)assert.deepEqual(captureSoldierCorrection(b.soldier,SOLDIER_REPLAY_SCHEMA),captureSoldierCorrection(a.soldier,SOLDIER_REPLAY_SCHEMA),`tick${tick}`);
 }
});

test('flyer own envelope fits with controls and two weapon channels and restores correct lease',()=>{
 const a=fixture().v;a.captureTick();a.tick(1/60,{takeoff:true,forward:1});a.captureTick();a.aiTarget={id:20,generation:2};const b=predictionVehicle(a),u=unit(),v=unit();
 u.seat={vehicle:a,index:0};v.seat={vehicle:b,index:0};
 const record=capture(u,{vehicle:a,vehicleId:20,vehicleGeneration:2}),packet=encodeEntityPacket({epoch:3,packetId:1,tick:100,records:[record]});
 b.position.addScalar(10);assert.throws(()=>applyOwnPlayer({unit:v,vehicle:b,vehicleId:21,vehicleGeneration:2,epoch:3,registry,record}),/lease/);
 applyOwnPlayer({unit:v,vehicle:b,vehicleId:20,vehicleGeneration:2,epoch:3,registry,record});assert.deepEqual(captureFlightCorrection(b),captureFlightCorrection(a));assert.ok(packet.length<=1100);
 console.log(`Own flyer with two infantry weapons + gamepad: ${packet.length} bytes`);
 const d=decodeOwnPlayer(record);assert.equal(d.bodySchema,7);
});

test('own correction carries explicit discard retirement with authoritative held and rejects previous envelope schema',()=>{const source=unit(),target=unit(),record=capture(source,{discardedInput:42,held:[]});assert.equal(record.schema,11);const context=applyOwnPlayer({unit:target,epoch:3,registry,record});assert.equal(context.discardedInput,42);assert.deepEqual(context.held,[]);assert.throws(()=>decodeOwnPlayer({...record,schema:5}),/unsupported own-player/);assert.throws(()=>capture(source,{discardedInput:-1}),/invalid own-player integer/);});
test('compact bodies leave weapon eligibility boundary timers and recoil bit-exact',()=>{
 const a=unit(),b=unit();
 for(const timer of [0,Number.EPSILON,1/60-Number.EPSILON,1/60,1/60+Number.EPSILON]){
  const weapon=a.combat.arsenal.current(0);weapon.wait=timer;weapon.reloadLeft=timer;a.combat.kick.hold=timer;
  applyOwnPlayer({unit:b,epoch:3,registry,record:capture(a)});
  assert.ok(Object.is(b.combat.arsenal.current(0).wait,timer));assert.ok(Object.is(b.combat.arsenal.current(0).reloadLeft,timer));assert.ok(Object.is(b.combat.kick.hold,timer));
 }
});
test('ground pilot envelope restores body and rejects wrong lease without mutation',()=>{
 const a=fixture().v;a.flying=false;a.captureTick();a.aiTarget={id:20,generation:2};const b=predictionVehicle(a),u=unit(),v=unit();u.seat={vehicle:a,index:0};v.seat={vehicle:b,index:0};
 const record=capture(u,{vehicle:a,vehicleId:20,vehicleGeneration:2});assert.equal(decodeOwnPlayer(record).bodySchema,10);
 b.position.addScalar(10);assert.throws(()=>applyOwnPlayer({unit:v,vehicle:b,vehicleId:21,vehicleGeneration:2,epoch:3,registry,record}),/lease/);
 applyOwnPlayer({unit:v,vehicle:b,vehicleId:20,vehicleGeneration:2,epoch:3,registry,record});assert.deepEqual(b.position,a.position);assert.equal(b.yaw,a.yaw);
 assert.ok(encodeEntityPacket({epoch:3,packetId:1,tick:100,records:[record]}).length<=1100);
});

test('worst recon own envelope includes full infantry/gamepad/strike replay in one datagram',async()=>{
 const {RemoteDroids}=await import('../src/remote.js'),{NetworkRemoteWorld,reconRandom}=await import('../src/runtime/network-remote-world.js'),{Scene,Vector3}=await import('three');
 const create=()=>new RemoteDroids({scene:new Scene(),camera:null,collision:{cast:()=>null,gather:()=>[]},lights:{flash(){}},model:()=>null,fire(){},explode(){}});
 const source=unit(),target=unit();source.alive=true;source.team=target.team=1;
 const loadout=[...defs,{...defs[0],class:'spare-rifle'},{...defs[1],class:'spare-grenade'}].map(def=>({ShotDelay:.2,InitialSalvoDelay:0,ReloadTime:1,HeatPerShot:.2,HeatRecoverRate:.1,...def}));
 source.combat.arsenal=new Arsenal(loadout,{random:()=>.5});target.combat.arsenal=new Arsenal(loadout,{random:()=>.5});
 for(const weapon of source.combat.arsenal.weapons)weapon.update(1/60,true,{held:true,pressed:true},false,{stick:0,posture:'stand',zoomed:false});
 const world=new NetworkRemoteWorld(create(),{randomFor:key=>reconRandom(37,key)});source.combat.remotes=world.attach(1,source);target.combat.remotes=create().createContext(target,{random:reconRandom(1,2)});
 const def={base:'droid',weapons:[{base:'cannon',class:'gun',channel:0,RoundsPerClip:0,ShotDelay:.2,ReloadTime:1,ordnance:{}},{base:'destruct',class:'strike',channel:1,RoundsPerClip:0,ShotDelay:.2,ReloadTime:1,strike:{},SalvoCount:4,SalvoDelay:.5,AimDistance:10,ScatterDistance:4}],AddHealth:-1};
 const droid=source.combat.remotes.launch({def:{SwitchImmediately:1,droid:def}},new Vector3(1,2,3),new Vector3(4,5,6),.3,1);source.combat.remotes.firstPerson=true;source.combat.remotes.updateCamera(1/60,true,true);source.combat.remotes.strike(droid,droid.arsenal.current(1).def);
 for(const weapon of droid.arsenal.weapons)weapon.update(1/60,true,{held:true,pressed:true},false,{stick:0,posture:'stand',zoomed:false});
 const content={...registry,weapons:new Map([...registry.weapons,[3,'spare-rifle'],[4,'spare-grenade']]),combat:{definitions:new Map([[1,def]]),definitionId:()=>1}};const record=capture(source,{registry:content});const packet=encodeEntityPacket({epoch:3,packetId:1,tick:100,records:[record]});assert.ok(packet.length<=1100,`own recon packet ${packet.length}`);console.log(`Dirty four-slot infantry, gamepad, recon and active strike: ${packet.length} bytes`);
 const context=applyOwnPlayer({unit:target,epoch:3,registry:content,record});assert.equal(context.remoteId,0);assert.equal(context.remoteGeneration,1);assert.equal(target.combat.remotes.active,true);assert.deepEqual(target.combat.remotes.piloted.position.toArray(),droid.position.toArray());assert.equal(target.combat.remotes.piloted.salvo.left,4);assert.equal(target.combat.remotes.random.seed,source.combat.remotes.random.seed);
 assert.deepEqual(target.combat.remotes.piloted.arsenal.weapons.map(captureWeaponState),droid.arsenal.weapons.map(captureWeaponState));
 const shown=target.combat.remotes.piloted;shown.def={...def};target.soldier.position.x=123;const beforeBody=captureSoldierCorrection(target.soldier);assert.throws(()=>applyOwnPlayer({unit:target,epoch:3,registry:content,record}),/definition changed/);assert.deepEqual(captureSoldierCorrection(target.soldier),beforeBody);shown.def=def;
 const applyLifecycle=lifecycle=>applyOwnPlayer({unit:target,epoch:3,registry:content,record,previous:context,remoteLifecycle:lifecycle});
 const terminal={id:0,generation:1,tick:101,active:false};
 const retired=applyLifecycle(terminal);assert.equal(retired.remoteId,null);assert.equal(retired.controlMode,0);assert.equal(retired.reset,true);assert.equal(retired.tick,record.tick);assert.equal(target.combat.remotes.droids.length,0);assert.equal(target.combat.remotes.strikes.length,0);
 applyLifecycle(terminal);assert.equal(target.combat.remotes.piloted,null,'older own checkpoint cannot resurrect retired control');
 applyLifecycle({...terminal,id:9});assert.equal(target.combat.remotes.active,true,'unrelated slot cannot suppress own control');
 target.combat.remotes.exit();target.combat.remotes.droids=[];target.combat.remotes.strikes=[];
 applyLifecycle(terminal);assert.equal(target.combat.remotes.droids.length,0,'terminal before first own checkpoint does not create a mesh');
 applyLifecycle({...terminal,tick:99});assert.equal(target.combat.remotes.active,true,'older lifecycle tick cannot override a newer own checkpoint');
 const next=target.combat.remotes.createDroid(def,null,new Vector3(80,2,3),new Vector3(),0,1);next.networkSlot=0;next.networkGeneration=2;target.combat.remotes.droids.push(next);target.combat.remotes.enter(next);
 applyLifecycle({id:0,generation:2,tick:101,active:true});assert.equal(target.combat.remotes.piloted,next);assert.deepEqual(target.combat.remotes.droids,[next]);assert.equal(next.position.x,80,'old own generation cannot move or retire newer presentation');
 source.combat.remotes.exit();const exited=capture(source,{registry:content});const after=applyOwnPlayer({unit:target,epoch:3,registry:content,record:exited,previous:context});assert.equal(target.combat.remotes.active,false);assert.equal(after.reset,true);
});


test('own team validates before restoring body or combat state',()=>{
 const source=unit(),target=unit();source.team=2;source.soldier.position.x=11;
 const record=capture(source);target.team=1;const context=applyOwnPlayer({unit:target,epoch:3,registry,record,previous:{epoch:3,id:1,generation:7,tick:99,team:1}});assert.equal(context.reset,true);
 assert.equal(target.team,2);assert.equal(target.combat.team,2);
 const bad={...record,payload:record.payload.slice()};bad.payload[10]=8;target.soldier.position.x=77;
 assert.throws(()=>applyOwnPlayer({unit:target,epoch:3,registry,record:bad}),/invalid own team/);
 assert.equal(target.soldier.position.x,77);assert.equal(target.team,2);
 source.team=0;assert.throws(()=>capture(source),/invalid own team/);
});

test('pilot full gun heat, salvo and barrel state fits with worst flyer and gamepad context',()=>{
 const a=fixture().v;a.aiTarget={id:20,generation:2};a.captureTick();a.tick(1/60,{takeoff:true,forward:1});a.captureTick();
 const b=predictionVehicle(a),u=unit(),v=unit();u.seat={vehicle:a,index:0};v.seat={vehicle:b,index:0};
 const definition={class:'pilot-gun',base:'cannon',channel:0,RoundsPerClip:25,ammo:2,ShotDelay:.13,SalvoCount:3,ShotsPerSalvo:1,SalvoDelay:.07,HeatPerShot:.2,HeatRecoverRate:.1};
 const weapons=()=>[0,1].map(channel=>({state:new Weapon({...definition,channel},{random:()=>.5}),barrel:0}));
 a.seats[0].weapons=weapons();b.seats[0].weapons=weapons();
 for(let tick=0;tick<13;tick++)for(const entry of a.seats[0].weapons)entry.barrel+=entry.state.update(1/60,true,{held:true,pressed:tick===0},false,{stick:0,posture:'stand',zoomed:false}).length;
 const record=capture(u,{vehicle:a,vehicleId:20,vehicleGeneration:2});
 const packet=encodeEntityPacket({epoch:3,packetId:1,tick:100,records:[record]});assert.ok(packet.length<=1100);assert.equal(decodeOwnPlayer(record).controlMode,4);
 console.log(`Own flyer with two full mounted weapons, two infantry weapons and gamepad: ${packet.length} bytes`);
 b.position.addScalar(17);b.seats[0].weapons[0].state.heat=.9;
 const before=captureFlightCorrection(b),invalid={...record,payload:record.payload.slice()};new DataView(invalid.payload.buffer).setFloat64(invalid.payload.length-8,NaN,true);
 assert.throws(()=>applyOwnPlayer({unit:v,vehicle:b,vehicleId:20,vehicleGeneration:2,epoch:3,registry,record:invalid}));assert.deepEqual(captureFlightCorrection(b),before);assert.equal(b.seats[0].weapons[0].state.heat,.9);
 applyOwnPlayer({unit:v,vehicle:b,vehicleId:20,vehicleGeneration:2,epoch:3,registry,record});
 for(let tick=0;tick<40;tick++)for(let channel=0;channel<2;channel++){
  const x=a.seats[0].weapons[channel],y=b.seats[0].weapons[channel],input={held:tick<25,pressed:false};
  const shots=x.state.update(1/60,true,input,false,{stick:0,posture:'stand',zoomed:false}),replayed=y.state.update(1/60,true,input,false,{stick:0,posture:'stand',zoomed:false});
  const scalar=list=>list.map(({weapon,...shot})=>shot);assert.deepEqual(scalar(replayed),scalar(shots));x.barrel+=shots.length;y.barrel+=replayed.length;
  assert.deepEqual(captureMountedControl(v),captureMountedControl(u));
 }
});

test('four dirty infantry slots and two actively firing pilot guns fit one correction datagram',()=>{
 const vehicle=fixture().v;vehicle.aiTarget={id:20,generation:2};vehicle.captureTick();vehicle.tick(1/60,{takeoff:true,forward:1});vehicle.captureTick();
 const source=unit(),target=unit(),loadout=Array.from({length:4},(_,i)=>({...defs[0],class:`slot-${i}`,channel:i%2,ReloadTime:.1,HeatPerShot:.02,HeatRecoverRate:.001,SpreadPerShot:.1,SpreadRecoverRate:.001}));
 source.combat.arsenal=new Arsenal(loadout);target.combat.arsenal=new Arsenal(loadout);source.combat.zoomStep=0;
 const content={...registry,weapons:new Map(loadout.map((d,i)=>[i+1,d.class]))};
 // Generate every dirty field through the shared rules: shoot, reload to
 // spend reserve, then fire again. No impossible combinations of timers.
 for(const weapon of source.combat.arsenal.weapons){
  const aim={stick:0,posture:'stand',zoomed:false};
  weapon.update(1/60,true,{held:true,pressed:true},false,aim);
  weapon.startReload();
  for(let tick=0;tick<20;tick++)weapon.update(1/60,true,{held:false,pressed:false},false,aim);
  weapon.update(1/60,true,{held:true,pressed:true},false,aim);
 }
 const definition={...defs[0],ShotDelay:.13,SalvoCount:3,SalvoDelay:.07,HeatPerShot:.2,HeatRecoverRate:.1};
 vehicle.seats[0].weapons=[0,1].map(channel=>({state:new Weapon({...definition,channel},{random:()=>.5}),barrel:0}));
 for(let tick=0;tick<13;tick++)for(const entry of vehicle.seats[0].weapons)entry.barrel+=entry.state.update(1/60,true,{held:true,pressed:tick===0},false,{stick:0,posture:'stand',zoomed:false}).length;
 source.seat={vehicle,index:0};
 const record=capture(source,{registry:content,vehicle,vehicleId:20,vehicleGeneration:2});
 const packet=encodeEntityPacket({epoch:3,packetId:1,tick:100,records:[record]});
 assert.ok(packet.length<=1100,`dirty four-slot pilot checkpoint ${packet.length} bytes`);
 console.log(`Dirty four-slot inventory, active dual pilot guns and gamepad: ${packet.length} bytes`);
 target.seat={vehicle,index:0};applyOwnPlayer({unit:target,vehicle,vehicleId:20,vehicleGeneration:2,epoch:3,registry:content,record});
 assert.deepEqual(target.combat.arsenal.weapons.map(captureWeaponState),source.combat.arsenal.weapons.map(captureWeaponState));
});

test('authoritative underwater firing gate restores without increasing own packet size',()=>{
 const source=unit(),target=unit();source.combat.gunUnderwater=false;
 const clear=capture(source);source.combat.gunUnderwater=true;const wet=capture(source);
 assert.equal(wet.payload.length,clear.payload.length);assert.equal(decodeOwnPlayer(wet).gunUnderwater,true);
 applyOwnPlayer({unit:target,epoch:3,registry,record:wet});assert.equal(target.combat.gunUnderwater,true);assert.equal(target.combat.firstPerson,true);
 source.combat.firstPerson=false;const thirdPersonWet=capture(source);
 applyOwnPlayer({unit:target,epoch:3,registry,record:thirdPersonWet});assert.equal(target.combat.gunUnderwater,true);assert.equal(target.combat.firstPerson,false);
 applyOwnPlayer({unit:target,epoch:3,registry,record:clear});assert.equal(target.combat.gunUnderwater,false);assert.equal(target.combat.firstPerson,true);
});

test('own corrections play his hurt and death once per newer tick within a life',async()=>{
 const {listenSounds}=await import('../src/audio/events.js');
 const heard=[],stop=listenSounds((type,_,amount)=>{if(type==='hurt'||type==='death')heard.push([type,amount]);});
 try{
  const source=unit(),target=unit();
  const at=(tick,health,dead=false)=>{source.combat.health=health;source.combat.dead=dead;return capture(source,{tick});};
  let previous=applyOwnPlayer({unit:target,epoch:3,registry,record:at(100,97)});
  previous=applyOwnPlayer({unit:target,epoch:3,registry,record:at(101,60),previous});
  previous=applyOwnPlayer({unit:target,epoch:3,registry,record:at(101,60),previous});
  previous=applyOwnPlayer({unit:target,epoch:3,registry,record:at(102,80),previous});
  previous=applyOwnPlayer({unit:target,epoch:3,registry,record:at(103,0,true),previous});
  assert.deepEqual(heard,[['hurt',37],['death',80]]);
 }finally{stop();}
});
