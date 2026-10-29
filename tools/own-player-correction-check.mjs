// Owned-content functional correction/replay proof. Does not measure transport,
// rendering performance or player capacity; no retail data is emitted.
import assert from 'node:assert/strict';import {readFile} from 'node:fs/promises';import {resolve,sep} from 'node:path';import {execFileSync} from 'node:child_process';
import {createGame} from '../src/game.js';import {createMultiplayerWorld} from '../src/runtime/multiplayer-world.js';
import {captureOwnPlayer,applyOwnPlayer} from '../src/runtime/network-entity-player.js';
import {captureCompactSoldier as captureSoldierCorrection} from '../src/runtime/network-entity-soldier-compact.js';
const SOLDIER_REPLAY_SCHEMA=6;
import {captureCompactFlight as captureFlightCorrection} from '../src/runtime/network-entity-flight-compact.js';import {predictionVehicle} from '../src/runtime/vehicle-prediction.js';
import {captureCompactSoldier,restoreCompactSoldier} from '../src/runtime/network-entity-soldier-compact.js';import {encodeEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
const root=resolve(process.env.BFE_DATA_DIR||'public');globalThis.fetch=async path=>{const file=resolve(root,`.${path}`);assert.ok(file.startsWith(root+sep));try{return new Response(await readFile(file));}catch(e){if(e.code==='ENOENT')return new Response('',{status:404});throw e;}};
const game=await createGame({headless:true,query:'?map=bes1&side=all&ai=classic&seed=1'}),world=createMultiplayerWorld(game,{armySize:1});
const className=game.spawnPort.roster(1).find(c=>/sniper|marksman|scout/i.test(c.className))?.className;assert.ok(className);
await world.addPlayer('probe',{team:1,className});const u=world.players.get('probe').unit,local=game.factory.build(className),reference=game.factory.build(className);
const view={mouseScale:1.25,aspect:16/9,invertMouse:false,firstPerson:false};let fired=false,zoomed=false,crouched=false;
const beforeClip=u.combat.arsenal.current(0).clip;
for(let tick=1;tick<=180;tick++){
 const shoot=!fired&&tick>30&&u.soldier.grounded&&u.soldier.lock<=0;
 const actions=[];if(fired&&!zoomed&&u.soldier.grounded){actions.push('zoom');zoomed=true;}if(zoomed&&!crouched&&tick>80&&u.soldier.grounded){actions.push('crouch');crouched=true;}
 world.input('probe',{epoch:1,sequence:tick,tick,generation:u.generation,down:shoot?['Mouse0']:[],actions,look:[],...view});world.step();
 if(u.combat.arsenal.current(0).clip<beforeClip)fired=true;
}
assert.ok(fired,'actual grounded authoritative shot consumes ammunition');assert.ok(u.combat.zoom>1,'actual scoped weapon zoom applied');assert.equal(u.soldier.posture,'crouch');const capturedZoom=u.combat.zoom;
const registry={contentEpoch:1,classes:new Map([[1,className]]),weapons:new Map(u.combat.arsenal.weapons.map((w,i)=>[i+1,w.name])),inputCodes:['KeyW','Mouse0','KeyS'],bindingProfiles:new Map([[0,{}]])};
const target={id:u.id,generation:u.generation,className,soldier:local.soldier,combat:local.combat};
const copyCombat=()=>{reference.combat.arsenal.selected=[...u.combat.arsenal.selected];reference.combat.zoomStep=u.combat.zoomStep;reference.combat.kick={...u.combat.kick};};
const full=captureCompactSoldier(u.soldier);restoreCompactSoldier(reference.soldier,full);copyCombat();
for(const value of Object.values(target.soldier))if(value?.isVector3)value.set(73,-41,27);
const record=captureOwnPlayer({unit:u,tick:180,epoch:1,registry,classId:1,held:['KeyW'],view});
const wire=encodeEntityPacket({epoch:1,packetId:1,tick:180,records:[record]}),context=applyOwnPlayer({unit:target,epoch:1,registry,record:decodeEntityPacket(wire).records[0]});
for(let tick=1;tick<=180;tick++){
 const input={forward:1,right:.1,turn:0,jump:tick===30,crouch:false,prone:tick===120};
 for(const unit of [reference,target]){unit.soldier.lookRaw(2/unit.combat.zoom,1/unit.combat.zoom,context.view.mouseScale,context.view.invertMouse);unit.soldier.tick(input,1/60);unit.combat.updateKick(1/60);}
 assert.deepEqual(captureSoldierCorrection(target.soldier,SOLDIER_REPLAY_SCHEMA),captureSoldierCorrection(reference.soldier,SOLDIER_REPLAY_SCHEMA),`owned infantry replay${tick}`);
}
const a=game.vehicles.vehicles.find(v=>v.flying&&/tie/i.test(v.name));assert.ok(a);for(const occupant of game.units.all())if(occupant.seat?.vehicle===a)game.vehicles.unboard(occupant,{force:true,placement:false});assert.ok(game.vehicles.board(u,a));a.driver={};a.seatIndex=0;a.captureTick();const initial=predictionVehicle(a);let b,flightBytes=0;
for(let tick=1;tick<=300;tick++){
 const input={takeoff:tick===1,forward:tick<220?1:-1,stickX:tick>160?.1:0};u.intent.flight=input;a.captureTick();a.tick(1/60,input);
 if(tick===90){b=predictionVehicle(initial);target.seat={vehicle:b,index:0};const r=captureOwnPlayer({unit:u,vehicle:a,vehicleId:99,vehicleGeneration:1,tick:tick+180,epoch:1,registry,classId:1,held:['KeyW'],view});const p=encodeEntityPacket({epoch:1,packetId:2,tick:tick+180,records:[r]});flightBytes=p.length;applyOwnPlayer({unit:target,vehicle:b,vehicleId:99,vehicleGeneration:1,epoch:1,registry,record:decodeEntityPacket(p).records[0],previous:context});}
 else if(b){b.captureTick();b.tick(1/60,input);}
 if(b)assert.deepEqual(captureFlightCorrection(b),captureFlightCorrection(a),`owned TIE envelope replay${tick}`);
}
const generation=u.generation;game.vehicles.unboard(u,{force:true,placement:false});target.seat=null;u.spawn(...u.soldier.position.toArray(),u.soldier.aimYaw);target.generation=u.generation;
const spawned=captureOwnPlayer({unit:u,tick:500,epoch:1,registry,classId:1,held:[],view});assert.ok(applyOwnPlayer({unit:target,epoch:1,registry,record:spawned,previous:context}).reset);assert.ok(u.generation>generation);
console.log(JSON.stringify({source:{revision:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),dirty:!!execFileSync('git',['status','--porcelain'],{encoding:'utf8'}).trim()},map:'bes1',className,vehicle:a.name,authoritativeShot:fired,capturedZoom,infantryPacketBytes:wire.length,flightPacketBytes:flightBytes,infantryReplayTicks:180,flightReplayTicks:210,generationReset:true,scope:'owned-content correction and physics replay, not transport/render/capacity'},null,2));
