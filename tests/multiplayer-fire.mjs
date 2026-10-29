// Real authoritative weapons acceptance, using operator-owned imported content.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
import { createGame } from '../src/game.js';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
const root=resolve(process.env.BFE_DATA_DIR??'public');
globalThis.fetch=async path=>{const filename=resolve(root,'.'+path);assert(filename.startsWith(root+sep));try{return new Response(await readFile(filename));}catch(e){if(e.code==='ENOENT')return new Response('',{status:404});throw e;}};
const game=await createGame({headless:true,query:'?map=nab2&side=rep&ai=classic&seed=1'});
const world=createMultiplayerWorld(game,{maxPlayers:2,armySize:1});
const player=await world.addPlayer('fire-test',{team:1,reliableFire:true});
const unit=game.units.byId(player.unitId);
for(let n=0;n<60;n++)world.step();
let shots=0;const original=unit.combat.fire.bind(unit.combat);unit.combat.fire=shot=>{shots++;return original(shot);};
const stamp=(id,pressed,tick=game.worldClock.tick+1)=>({epoch:1,id,pressed,button:0,tick,generation:unit.generation});
const press=stamp(1,true),release=stamp(2,false);
assert(world.fire('fire-test',press));assert(world.fire('fire-test',release));assert(!world.fire('fire-test',press));
world.step();assert.equal(shots,1,'a same-delivery-batch click fires exactly once');world.step();
assert(world.fire('fire-test',stamp(3,true)));assert(world.fire('fire-test',stamp(4,false)));world.step();world.step();
assert.equal(shots,1,'weapon cooldown rejects a fresh edge arriving too soon');
assert(!world.fire('fire-test',stamp(5,true,game.worldClock.tick-31)),'stale shot cannot replay');
const input=sequence=>({epoch:1,sequence,tick:game.worldClock.tick+1,generation:unit.generation,down:['KeyW'],actions:[],look:[]});
assert(world.input('fire-test',input(1)));world.step();assert(unit.intent.forward>0);
assert(world.fire('fire-test',stamp(6,true)));world.step();
await new Promise(r=>setTimeout(r,550));world.step();assert.equal(unit.intent.forward,0);assert.deepEqual(unit.intent.buttons.slice(0,2),[0,0]);
assert(world.input('fire-test',input(2)));world.step();assert(unit.intent.forward>0,'fresh input resumes after silence');
assert(world.fire('fire-test',stamp(7,true)));world.step();assert.equal(unit.intent.buttons[0],1);
assert(world.input('fire-test',input(3)));world.step();assert.equal(unit.intent.buttons[0],0,'fresh released state stops fire while the reliable release is stalled');
// A short click may arrive ahead of the simulation while fresh movement
// snapshots already report its released button. Preserve one eligible shot.
for(let n=0;n<90;n++)world.step();
assert(!unit.soldier.air && unit.soldier.lock<=0,'future-click fixture is grounded and ready');
const beforeShots=shots, beforeClip=unit.combat.arsenal.current(0).clip;
const futureTick=game.worldClock.tick+5;
assert(world.fire('fire-test',stamp(8,true,futureTick)));
assert(world.fire('fire-test',stamp(9,false,futureTick)));
for(let n=0;n<8;n++){
  assert(world.input('fire-test',{...input(4+n),down:[]}));
  world.step();
}
assert.equal(shots,beforeShots+1,'future same-batch press/release fires exactly once');
assert(unit.combat.arsenal.current(0).clip<beforeClip,'grounded future click consumes authoritative ammunition');
world.removePlayer('fire-test');assert.equal(unit.intent.forward,0);assert.deepEqual(unit.intent.buttons.slice(0,2),[0,0]);
console.log(JSON.stringify({ok:true,checks:['short-click','duplicate','cooldown','stale-shot','silent-controls','stalled-release','future-short-click','disconnect'],shots}));
