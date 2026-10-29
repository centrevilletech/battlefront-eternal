import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {createEntityHost} from '../src/runtime/entity-host.js';
import {decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
import {decodeCombat} from '../src/runtime/network-entity-combat.js';
import {SimEvents} from '../src/sim-events.js';
import {InputMailbox} from '../src/runtime/transport.js';
import {InputActions} from '../src/runtime/input-actions.js';
import {FireCommands} from '../src/runtime/fire-commands.js';
import {Vector3} from 'three';
import {Soldier} from '../src/soldier.js';
import {CollisionWorld} from '../src/collision.js';
import {PlayerCombat} from '../src/combat.js';
import {Arsenal} from '../src/weapons.js';

// Empty authoritative world with real capture, mailboxes, registry, scheduler,
// and packet codecs. Spectators avoid unrelated actor/content prerequisites.
function emptyHost({stateBytesPerSecond=500000,local=false}={}) {
  const definition = {base:'bolt',Velocity:100,LifeSpan:1};
  const game = {
    data:{sides:{fixture:{units:{soldier:{weapons:[{class:'rifle',ordnance:definition}]}}}}},
    worldClock:{tick:0},simEvents:new SimEvents(),ordnance:{grenades:[],rounds:[],beams:[]},
    units:{all:()=>[]},posts:{posts:[]},
    battle:{characters:[],reinforcements:{},overTime:0,victoryClock:null,countdown:null},
  };
  const clients = ['phase0','phase1','phase2'];
  const world = {players:new Map(clients.map(id=>[id,{
    spectator:true,mailbox:new InputMailbox(),actions:new InputActions(),fire:new FireCommands(),
  }]))};
  const host = createEntityHost({game,world,stateBytesPerSecond,local});
  for (const id of clients) {host.addClient(id);host.setReady(id);host.setStarted(id,true);}
  const seen = new Map(clients.map(id=>[id,new Map()]));
  function step({emit=()=>{},drop=()=>false}={}) {
    game.worldClock.tick++; emit();
    const out = host.publish({begun:game.worldClock.tick*1000/60});
    assert.deepEqual(out.faults,[]);
    assert.deepEqual(out.diagnostics.flatMap(d=>d.actors),[]);
    for (const packet of out.packets) {
      host.admitted(packet.client,packet.packetId);
      const events = decodeEntityPacket(new Uint8Array(packet.buffer)).records.filter(r=>r.kind==='combat').map(decodeCombat);
      if (drop(packet.client,events)) continue;
      for (const event of events) seen.get(packet.client).set(`${event.id}:${event.schema}`,event);
      host.ack(packet.client,{epoch:1,packetId:packet.packetId});
    }
  }
  return {game,world,host,clients,seen,step,definition};
}

test('host refunds charged offers once, retains the ledger through resync, and never refunds admission',()=>{
 const {game,host,clients}=emptyHost();
 try {
  game.worldClock.tick=3;const packets=host.publish().packets.filter(p=>p.client===clients[0]);assert.ok(packets.length);
  const first=packets[0],before=host.inspect(clients[0]).offeredBytes;
  host.resync(clients[0]);host.dropped(clients[0],first.packetId);
  assert.equal(host.inspect(clients[0]).offeredBytes,before-first.buffer.byteLength);
  host.dropped(clients[0],first.packetId);assert.equal(host.inspect(clients[0]).offeredBytes,before-first.buffer.byteLength);
  game.worldClock.tick=6;const next=host.publish().packets.find(p=>p.client===clients[0]);assert.ok(next.packetId>first.packetId);
  host.admitted(clients[0],next.packetId);const admitted=host.inspect(clients[0]).offeredBytes;
  host.dropped(clients[0],next.packetId);assert.equal(host.inspect(clients[0]).offeredBytes,admitted);
 }finally{host.dispose();}
});

test('forced own correction does not mint refund credit when it spent no budget',()=>{
 const {game,world,host,clients}=emptyHost({stateBytesPerSecond:20000});const id=clients[0];
 const collision=new CollisionWorld(new Float32Array([-100,0,-100,100,0,-100,100,0,100,-100,0,100]),new Uint32Array([0,2,1,0,3,2]),new Uint32Array([0,1]));
 const soldier=new Soldier(collision,{});soldier.spawn(0,0,0,0);
 const combat=Object.assign(Object.create(PlayerCombat.prototype),{arsenal:new Arsenal([],{player:true}),health:100,zoomStep:0,firstPerson:true,soldier,kick:{rate:0,hold:0,offset:0},dead:false,deathTime:0});
 Object.assign(world.players.get(id),{spectator:false,unit:{id:1,generation:1,team:1,className:'soldier',soldier,combat}});
 for(const other of clients.slice(1))host.setStarted(other,false);
 const begun=performance.now();let forced=false;
 try{
  for(let i=0;i<20;i++){
   game.worldClock.tick=2+i*6;const before=host.inspect(id).offeredBytes;
   const out=host.publish({begun});assert.deepEqual(out.faults,[]);assert.equal(out.packets.length,1);
   const packet=out.packets[0],after=host.inspect(id).offeredBytes;
   if(after===before){host.dropped(id,packet.packetId);host.dropped(id,packet.packetId);assert.equal(host.inspect(id).offeredBytes,before);forced=true;break;}
   assert.equal(after-before,packet.buffer.byteLength);host.admitted(id,packet.packetId);
  }
  assert.ok(forced,'fixed publication time must exhaust budget and exercise uncharged fallback');
 }finally{host.dispose();}
});

test('content-free host retains every phase event through packet loss and resync', () => {
  const {game,host,clients,seen,step} = emptyHost();
  try {
    for (let i=0;i<6;i++) step();
    for(const id of clients)assert.equal(host.inspect(id).admissionAgeTicks.find(r=>r.kind==='summary'&&r.id===0)?.targetIntervalTicks,30,'common summaries receive finite deadlines');
    let dropped=0;
    for (let i=0;i<6;i++) step({
      emit:()=>game.simEvents.emit('presentation',{event:{kind:'effect',name:`phase-marker-${i}`,position:[0,0,0]}}),
      drop:(client,events)=>{if(client==='phase1'&&events.length){dropped++;return true;}return false;},
    });
    for (let i=0;i<12;i++) step();
    assert.ok(dropped>0,'the loss fixture must discard combat packets');
    const expected=Array.from({length:6},(_,i)=>`phase-marker-${i}`);
    const names=id=>[...seen.get(id).values()].map(e=>e.name).sort();
    for (const id of clients) assert.deepEqual(names(id),expected,`${id} receives all capture phases`);
    for (const id of clients) {host.resync(id);seen.get(id).clear();}
    for (let i=0;i<6;i++) step();
    for (const id of clients) assert.deepEqual(names(id),expected,`${id} reseeds retained events`);
  } finally {host.dispose();}
});

test('content-free host delivers bolt launches and terminal replacements across all phases', () => {
  const {game,host,clients,seen,step,definition} = emptyHost();
  const bolts=Array.from({length:3},(_,i)=>({def:definition,life:1,position:new Vector3(i,0,0),direction:new Vector3(0,0,1),speed:100,merge:8}));
  try {
    for (let i=0;i<6;i++) step();
    for (const bolt of bolts) step({emit:()=>game.ordnance.onLaunch(bolt)});
    for (let i=0;i<6;i++) step();
    for (const id of clients) assert.equal([...seen.get(id).values()].filter(e=>e.schema===1).length,3,`${id} receives each launch`);
    let dropped=0;
    for (const bolt of bolts) step({
      emit:()=>game.ordnance.onImpact(bolt,{point:new Vector3(0,0,10),normal:new Vector3(0,0,-1),surface:0}),
      drop:(client,events)=>{if(client==='phase2'&&events.some(e=>e.schema===2)){dropped++;return true;}return false;},
    });
    for (let i=0;i<12;i++) step();
    assert.ok(dropped>0,'terminal packets must be lost before repair');
    for (const id of clients) {
      const events=[...seen.get(id).values()],launches=events.filter(e=>e.schema===1),terminals=events.filter(e=>e.schema===2);
      assert.equal(terminals.length,3,`${id} receives each terminal`);
      assert.deepEqual(terminals.map(e=>e.id).sort(),launches.map(e=>e.id).sort(),'terminal replaces the same projectile identity');
      host.resync(id);seen.get(id).clear();
    }
    for (let i=0;i<6;i++) step();
    for (const id of clients) {
      const events=[...seen.get(id).values()];
      assert.equal(events.length,3);
      assert.ok(events.every(e=>e.schema===2),'resync does not resurrect a terminal projectile');
    }
  } finally {host.dispose();}
});

const content = process.env.BFE_CONTENT_DIR || new URL('../public', import.meta.url).pathname;
test('every publication phase receives intervening combat events, repairs loss, and reseeds on resync', {skip: !existsSync(`${content}/maps/nab2/map.json`)}, async () => {
  process.env.BFE_CONTENT_DIR ||= content;
  const {headlessGame} = await import('../tools/headless-world.mjs');
  const {createMultiplayerWorld} = await import('../src/runtime/multiplayer-world.js');
  const game = await headlessGame('?map=nab2&side=rep&ai=off&seed=1');
  const world = createMultiplayerWorld(game);
  const host = createEntityHost({game, world, stateBytesPerSecond:500000});
  const clients = ['phase0','phase1','phase2'];
  const seen = new Map(clients.map(id => [id,new Set()]));
  let drop = false, dropped = 0;
  try {
    for (const id of clients) {
      await world.addPlayer(id);
      host.addClient(id); host.setStarted(id,true); host.setReady(id);
    }
    function publish() {
      const out = host.publish({begun:game.worldClock.tick * 1000 / 60});
      assert.deepEqual(out.faults,[]);
      assert.deepEqual(out.diagnostics.flatMap(d => d.actors),[]);
      for (const packet of out.packets) {
        host.admitted(packet.client,packet.packetId);
        const decoded = decodeEntityPacket(new Uint8Array(packet.buffer));
        const markers = decoded.records.filter(r=>r.kind==='combat' && r.schema===8).map(decodeCombat).filter(e=>e.name?.startsWith('phase-marker-'));
        if (drop && packet.client==='phase1' && markers.length) {dropped++; continue;}
        for (const event of markers) seen.get(packet.client).add(event.name);
        host.ack(packet.client,{epoch:1,packetId:packet.packetId});
      }
    }
    // Seed all three recipients before testing events between their turns.
    for (let i=0;i<6;i++) {world.step();publish();}
    for(const id of clients){
      const summaries=host.inspect(id).admissionAgeTicks.filter(r=>r.kind==='summary');
      assert.equal(summaries.find(r=>r.id===0)?.targetIntervalTicks,30,'common summaries receive finite deadlines');
      assert.equal(summaries.find(r=>r.id===65536)?.targetIntervalTicks,6,'private spawn summaries receive finite deadlines');
    }
    drop = true;
    for (let i=0;i<6;i++) {
      world.step();
      game.simEvents.emit('presentation',{event:{kind:'effect',name:`phase-marker-${i}`,position:[0,0,0]}});
      publish();
    }
    drop = false;
    for (let i=0;i<12;i++) {world.step();publish();}
    assert.ok(dropped>0,'loss fixture must discard combat packets');
    const expected = Array.from({length:6},(_,i)=>`phase-marker-${i}`);
    for (const id of clients) assert.deepEqual([...seen.get(id)].sort(),expected,`${id} must receive events from all capture ticks`);
    host.resync('phase2');seen.get('phase2').clear();
    for (let i=0;i<6;i++) {world.step();publish();}
    assert.deepEqual([...seen.get('phase2')].sort(),expected,'resync restores retained events');
  } finally {host.dispose();}
});
