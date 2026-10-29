import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {createEntityHost} from '../src/runtime/entity-host.js';
import {decodeEntityPacket} from '../src/runtime/network-entity-codec.js';

const content=process.env.BFE_CONTENT_DIR||new URL('../public',import.meta.url).pathname;
const isPose=r=>['soldier','vehicle','droideka'].includes(r.kind);
const version=r=>`${r.kind}:${r.id}:${r.generation}:${r.tick}:${r.schema??''}:${r.active??''}`;
const percentile=(values,p)=>values.length?[...values].sort((a,b)=>a-b)[Math.min(values.length-1,Math.floor(values.length*p))]:null;

test('real host bounds pending versions and preserves near/far service with delayed admission and loss', {skip:!existsSync(`${content}/maps/nab2/map.json`)}, async t=>{
 process.env.BFE_CONTENT_DIR||=content;
 const {headlessGame}=await import('../tools/headless-world.mjs');
 const {createMultiplayerWorld}=await import('../src/runtime/multiplayer-world.js');
 const game=await headlessGame('?map=nab2&side=rep&ai=classic&seed=1');
 const world=createMultiplayerWorld(game,{armySize:32});
 await world.addPlayer('freshness',{reliableFire:false});
 const actor=world.players.get('freshness');actor.autoRespawn=true;
 // Host creation, publication, stale-input checks and refunds share one clock.
 // Advancing only publish({begun}) would leave refund() on real wall time.
 let now=performance.now();t.mock.method(performance,'now',()=>now);
 const host=createEntityHost({game,world,stateBytesPerSecond:100000});
 host.addClient(actor.id);host.setStarted(actor.id,true);host.setReady(actor.id);
 const pending=new Map(),queue=[],latest=new Map(),nearGaps=[],farGaps=[],farSeen=new Set(),seen=new Set();
 // Allow the map's opening spawn wave to fill the roster before measuring.
 // Death/respawn remains normal; observed live counts are reported, not fixed.
 const warmup=900,duration=720,stats={offered:0,admitted:0,dropped:0,ackLost:0,ownOffered:0,bytes:0,combat:0,nearSamples:0,maxPending:0,aliveMin:Infinity,aliveMax:0};
 function retire(entry){for(const r of entry.records)if(r.kind!=='correction')assert.equal(pending.delete(version(r)),true);}
 try{
  for(let frame=1;frame<=warmup+duration+3;frame++){
   now+=1000/60;
   // Deterministic 50ms offer-to-admission delay, with explicit native drops.
   // This models the host callback contract, not SCTP or a capacity benchmark.
   while(queue.length&&queue[0].due<=frame){
    const entry=queue.shift();retire(entry);
    if(entry.packet.packetId%17===0){host.dropped(actor.id,entry.packet.packetId);if(frame>warmup)stats.dropped++;continue;}
    host.admitted(actor.id,entry.packet.packetId,{ownCorrection:entry.packet.ownCorrection,tick:entry.tick});
    if(frame>warmup&&frame<=warmup+duration){
     stats.admitted++;stats.bytes+=entry.packet.buffer.byteLength;
     for(const r of entry.records){
      if(r.kind==='combat')stats.combat++;
      if(!isPose(r))continue;
      const key=`${r.kind}:${r.id}:${r.generation}`,old=latest.get(key);
      if(old!==undefined&&r.tick<=old)continue;
      latest.set(key,r.tick);seen.add(key);
      const distance=actor.unit.position.distanceTo({x:r.position[0],y:r.position[1],z:r.position[2]});
      if(r.health>0&&Math.hypot(...r.velocity)>.5&&distance<35){stats.nearSamples++;if(old!==undefined)nearGaps.push(r.tick-old);}
      if(distance>100){farSeen.add(key);if(old!==undefined)farGaps.push(r.tick-old);}
     }
    }
    if(entry.packet.packetId%19)host.ack(actor.id,{epoch:1,packetId:entry.packet.packetId});else if(frame>warmup)stats.ackLost++;
   }
   if(frame>warmup+duration)continue;
   const tick=game.worldClock.tick+1;
   world.input(actor.id,{epoch:1,sequence:tick,tick,generation:actor.unit.generation,down:['Mouse0','KeyW'],actions:[],look:[]});
   assert.deepEqual(host.advanceEdges(),[]);world.step();assert.deepEqual(host.consumedEdges(game.worldClock.tick),[]);
   const out=host.publish({begun:now});assert.deepEqual(out.faults,[]);assert.deepEqual(out.diagnostics.flatMap(d=>d.actors),[]);
   for(const packet of out.packets){
    const decoded=decodeEntityPacket(new Uint8Array(packet.buffer));
    if(packet.ownCorrection)assert.ok(decoded.records.some(r=>r.kind==='correction'),'40Hz own traffic includes its actual replay envelope');
    for(const r of decoded.records)if(r.kind!=='correction'){
     const key=version(r);assert.equal(pending.has(key),false,`duplicate outstanding ${key}`);pending.set(key,packet.packetId);
    }
    queue.push({packet,records:decoded.records,tick:decoded.tick,due:frame+3});
    if(frame>warmup){stats.offered++;if(packet.ownCorrection)stats.ownOffered++;}
   }
   stats.maxPending=Math.max(stats.maxPending,pending.size);
   if(frame>warmup){const alive=game.battle.characters.filter(c=>c.unit?.alive&&!c.unit.removed).length;stats.aliveMin=Math.min(stats.aliveMin,alive);stats.aliveMax=Math.max(stats.aliveMax,alive);}
   // Real workers yield between steps. AI class preparation/spawn promises
   // must settle here too, rather than silently leaving roster slots empty.
   await new Promise(resolve=>setImmediate(resolve));
  }
  const result={roster:world.roster.counts(),...stats,near:{count:nearGaps.length,p50:percentile(nearGaps,.5),p95:percentile(nearGaps,.95),p99:percentile(nearGaps,.99),max:Math.max(...nearGaps)},far:{actors:farSeen.size,count:farGaps.length,p95:percentile(farGaps,.95),max:Math.max(...farGaps)},actorsSeen:seen.size,ownHz:stats.ownOffered/(duration/60),admittedBytesPerSecond:stats.bytes/(duration/60)};
  console.log('real-host-freshness',JSON.stringify(result));
  assert.deepEqual(result.roster.map(team=>team.total),[32,32]);
  assert.equal(result.ownHz,40);assert.ok(stats.combat>0);assert.ok(stats.dropped>0&&stats.ackLost>0);
  assert.ok(nearGaps.length>1000,'exercise moving actors near the actual human viewpoint');
  assert.ok(result.near.p95<=6,`near moving admission gaps p95 ${result.near.p95} exceeds6ticks`);
  assert.ok(farSeen.size>=32,'near urgency must retain broad distant coverage');
  assert.ok(result.far.p95<=18&&result.far.max<=36,`far service regressed: ${JSON.stringify(result.far)}`);
  assert.equal(pending.size,0);
 }finally{host.dispose();}
});
