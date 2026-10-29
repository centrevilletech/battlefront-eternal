// Owned-assets, one-world regression for body replacement and normal respawns.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {resolve,sep} from 'node:path';
import {createGame} from '../src/game.js';
import {createMultiplayerWorld} from '../src/runtime/multiplayer-world.js';
const root=resolve(process.env.BFE_DATA_DIR||'public');
globalThis.fetch=async path=>{assert.match(path,/^\/(maps|sides|common|audio)\//);const file=resolve(root,`.${path}`);assert.ok(file.startsWith(root+sep));try{return new Response(await readFile(file));}catch(error){if(error.code==='ENOENT')return new Response('',{status:404});throw error;}};
const game=await createGame({headless:true,query:'?map=nab2&side=rep&ai=classic&seed=1'}),world=createMultiplayerWorld(game);
await world.addPlayer('human',{team:1});const player=world.players.get('human');
const classes=game.spawnPort.roster(1).map(v=>v.className);let sequence=0;
for(const className of [classes[1],classes[2],classes[1]]){
 const old=player.unit;old.combat.damage(Infinity);for(let i=0;i<245;i++)world.step();
 const tick=game.worldClock.tick+1;
 assert.ok(world.input('human',{epoch:1,sequence:++sequence,tick,generation:old.generation,down:[],actions:[],look:[],human:[{kind:'spawn',className,post:null}]}));world.step();
 assert.equal(player.unit.className,className);assert.ok(player.unit.alive);assert.equal(player.spawn.phase,'alive');assert.notEqual(player.unit,old);
 console.log('shared class respawn',className,player.unit.id,player.unit.generation,player.spawn.phase);
}
console.log('One-world class replacement and respawn passed');
