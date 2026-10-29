// Local acceptance test; requires the operator's imported Theed assets.
// Run: node tests/multiplayer-world.mjs
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
import { headlessGame } from '../tools/headless-world.mjs';
import {createEntityHost} from '../src/runtime/entity-host.js';
import {EntityGameState} from '../src/runtime/network-entity-game.js';
import {EntitySeats} from '../src/runtime/entity-seats.js';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
import { Character } from '../src/battle.js';
const root=resolve(process.env.BFE_DATA_DIR || 'public');
globalThis.fetch=async path=>{
  assert.match(path,/^\/(maps|sides|common|audio)\//);
  const filename=resolve(root,`.${path}`); assert.ok(filename.startsWith(root+sep));
  try{return new Response(await readFile(filename));}catch(error){if(error.code==='ENOENT')return new Response('',{status:404});throw error;}
};
const query='?map=nab2&side=rep&ai=classic&seed=1';
const game=await headlessGame(query);
const world=createMultiplayerWorld(game);
const a=await world.addPlayer('alice',{team:1}),b=await world.addPlayer('bob',{team:2});
assert.notEqual(a.unitId,b.unitId);
const au=game.units.byId(a.unitId),bu=game.units.byId(b.unitId);
const start=au.position.clone(),other=bu.position.clone();
let bytes=0;
for(let tick=1;tick<=120;tick++){
  world.input('alice',{epoch:1,sequence:tick,tick,generation:a.generation,down:['KeyW'],actions:[],look:[]});
  world.step();
}
assert.ok(au.position.distanceTo(start)>1,'first human follows its own input');
assert.ok(bu.position.distanceTo(other)<1,'second human does not inherit first input');
assert.equal(world.input('alice',{epoch:1,sequence:1,tick:120,generation:1,down:[],actions:[],look:[]}),false,'replayed sequence rejected');

// Decode a real per-client packet onto a separately loaded client world.
const slot=game.vehicles.slots[0];
const variantName=[...game.vehicles.templates.keys()].find(name=>!slot.variants.has(name));
assert.ok(variantName);
const lateVehicle=game.vehicles.variant(slot,variantName);
const replica=await headlessGame(query);
replica.validateAuthoritativeHumanTeam=()=>{};
replica.applyAuthoritativeHumanTeam=team=>replica.setHumanTeam(team,false);
Object.defineProperty(replica,'client',{value:{seats:new EntitySeats(replica)}});
const host=createEntityHost({game,world,local:true});host.addClient('bob');host.setReady('bob');host.setStarted('bob',true);
const adapter=new EntityGameState(replica,host.registry,{onApplied:p=>host.ack('bob',p.ack)});adapter.initialize();
function publish(){
 const out=host.publish();assert.deepEqual(out.faults,[]);
 for(const packet of out.packets){host.admitted('bob',packet.packetId);adapter.receive(new Uint8Array(packet.buffer));bytes+=packet.buffer.byteLength;}
 adapter.present(game.worldClock.tick);
}
lateVehicle.spawn(2);publish();
const local=replica.playerUnit;
assert.equal(replica.units.byId(lateVehicle.aiTarget.id).vehicle.name,lateVehicle.name,'late vehicle IDs remain distinct from joining infantry');
assert.equal(local.combat.health,bu.combat.health);
assert.ok(local.position.distanceTo(bu.position)<1e-9);
assert.equal(local.character.name,'Player');
assert.deepEqual(local.combat.arsenal.weapons.map(w=>[w.clip,w.reserve]),bu.combat.arsenal.weapons.map(w=>[w.clip,w.reserve]));

const tickets=game.battle.reinforcements[1],kills=bu.character.kills;
au.combat.damage(100000,100000,bu.position,bu);
assert.equal(au.alive,false);assert.equal(bu.character.kills,kills+1);assert.equal(game.battle.reinforcements[1],tickets-1);
for(let n=0;n<241;n++){world.step();publish();}
const nextClass=game.spawnPort.roster(1)[1].className,tick=game.worldClock.tick+1;
world.input('alice',{epoch:1,sequence:tick,tick,generation:au.generation,down:[],actions:[],look:[],human:[{kind:'spawn',className:nextClass,post:null}]});world.step();
assert.equal(world.player('alice').className,nextClass);assert.ok(world.players.get('alice').unit.alive);
assert.ok(world.player('alice').generation>a.generation,'new class rejects previous-life commands');
assert.equal(world.players.get('alice').spawn.phase,'alive','new class must not inherit the removed corpse phase');
const retained=world.player('bob').unitId;
world.removePlayer('bob');world.step();await world.addPlayer('bob');
assert.equal(world.player('bob').unitId,retained,'reconnect retains the same body');
assert.ok(world.player('bob').generation>b.generation,'reconnect rejects prior connection commands');
// A network human drives the regular crew simulation and exits via its own input.
// The late-join fixture spawned two variants on one pad. Use another pad
// for the exit test so that overlapping hulls do not block the safe exit.
const vehicle=game.vehicles.vehicles.find(v=>v.alive&&v.seats.length>1&&v.slot!==slot);
assert.ok(vehicle,'Theed provides an available vehicle');
for (const unit of game.units.all()) if (unit.seat?.vehicle === vehicle) game.vehicles.unboard(unit,{force:true,placement:false});
assert.ok(game.vehicles.board(bu,vehicle),'network human can board a live vehicle');
assert.equal(bu.seat.controllable,null,'AI cannot take over the network seat');
publish();
assert.equal(local.seat.vehicle.aiTarget.id,vehicle.aiTarget.id);
assert.equal(local.seat.human,true);
const driveTick=game.worldClock.tick+1;
world.input('bob',{epoch:1,sequence:driveTick,tick:driveTick,generation:bu.generation,down:['KeyW'],actions:[],look:[]});world.step();
assert.equal(bu.seat.input.forward,1,'network pilot input reaches vehicle physics');
const originalSeat=bu.seat.index,cycleTick=game.worldClock.tick+1;
world.input('bob',{epoch:1,sequence:cycleTick,tick:cycleTick,generation:bu.generation,down:[],actions:['nextPrimary'],look:[]});world.step();
assert.notEqual(bu.seat.index,originalSeat,'network weapon-cycle edge changes vehicle seats');
const reverseTick=game.worldClock.tick+1;
world.input('bob',{epoch:1,sequence:reverseTick,tick:reverseTick,generation:bu.generation,down:[],actions:['previousPrimary'],look:[]});world.step();
assert.equal(bu.seat.index,originalSeat,'reverse seat cycling returns to the pilot');
const exitTick=game.worldClock.tick+1;
world.input('bob',{epoch:1,sequence:exitTick,tick:exitTick,generation:bu.generation,down:[],actions:['use'],look:[]});
world.step();
assert.ok(bu.seat===null,'network use edge exits the seat');
const turret=game.turretSeats.turrets.find(t=>game.turretSeats.canMan(bu,t));
if(turret){
  assert.ok(game.turretSeats.man(bu,turret));
  assert.equal(bu.seat.controllable,null);
  const turretTick=game.worldClock.tick+1;
  world.input('bob',{epoch:1,sequence:turretTick,tick:turretTick,generation:bu.generation,down:[],actions:['use'],look:[]});world.step();
  assert.ok(bu.seat===null,'network use edge exits the turret');
}
const suicideTick=game.worldClock.tick+1;
world.input('bob',{epoch:1,sequence:suicideTick,tick:suicideTick,generation:world.player('bob').generation,down:[],actions:[],look:[],human:[{kind:'suicide'}]});world.step();
assert.equal(world.players.get('bob').unit.alive,false,'respawn menu commits authoritative suicide');
for(let n=0;n<241;n++){world.step();if(n%3===0){publish();}}
const changeTick=game.worldClock.tick+1;
const teamClass=game.spawnPort.roster(1)[0].className;
world.input('bob',{epoch:1,sequence:changeTick,tick:changeTick,generation:world.player('bob').generation,down:[],actions:[],look:[],human:[{kind:'team',team:1},{kind:'spawn',className:teamClass,post:null}]});world.step();
assert.equal(world.player('bob').team,1);
assert.equal(world.players.get('bob').unit.team,1);
assert.ok(world.players.get('bob').unit.alive,'team and class menu choices spawn the selected body');
// A late reliable menu edge remains valid after newer unreliable movement.
const split=await world.addPlayer('split-lanes',{team:2,separateActions:true});
const splitUnit=world.players.get('split-lanes').unit,splitTick=game.worldClock.tick+1;
world.input('split-lanes',{epoch:1,sequence:20,tick:splitTick,generation:split.generation,down:[],actions:[],look:[],lookTotal:[0,0],human:[{kind:'suicide'}]});
world.step();
assert.ok(splitUnit.alive,'replaceable state cannot execute a duplicated menu action');
assert.equal(world.actions('split-lanes',{epoch:1,id:5,tick:splitTick,generation:split.generation,actions:[],human:[{kind:'suicide'}]}),true);
world.step();
assert.equal(splitUnit.alive,false,'late reliable action survives the newer movement watermark');
assert.equal(world.players.get('split-lanes').mailbox.ack,20);
assert.equal(world.players.get('split-lanes').actions.ack,5);
assert.equal(world.actions('split-lanes',{epoch:1,id:5,tick:splitTick,generation:split.generation,actions:[],human:[{kind:'suicide'}]}),false);
world.removePlayer('split-lanes');
if(process.env.BFE_FULL_BATTLE==='1'){
  // The regular bots, projectiles and post rules must end the match.
  game.battle.reinforcements[1]=1;
  game.battle.reinforcements[2]=30;
  const deadline=game.worldClock.tick+7200;
  while(!game.battle.finished && game.worldClock.tick<deadline){
    world.step();
    if(game.worldClock.tick%3===0){publish();}
  }
  assert.ok(game.battle.finished,'normal combat completes a low-ticket battle');
  assert.ok(game.battle.gameOver);
  assert.ok([1,2].includes(game.battle.loser));
  publish();
  assert.equal(replica.battle.gameOver,true,'client receives end-of-match state');
}
console.log(JSON.stringify({status:'passed',ticks:game.worldClock.tick,players:[world.player('alice'),world.player('bob')],steadyBytesPerSecond:Math.round(bytes/117*60)}));

host.dispose();
