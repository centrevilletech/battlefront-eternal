import test from 'node:test';
import assert from 'node:assert/strict';
import {Matrix4} from 'three';
import {Battle} from '../src/battle.js';
import {CommandPosts} from '../src/commandposts.js';
import {F4Live} from '../src/f4-live.js';
import {F4Tail} from '../src/f4-tail.js';
import {F4Roster} from '../src/f4-roster.js';
import {F4Connections} from '../src/f4-connections.js';
import {HumanSpawnMirror} from '../src/sim/gen/index.js';
import {f3World} from './fixtures/f3-world.js';
import {nativeSpawning} from './fixtures/f4-spawning.js';
import {weaponDefinition} from './fixtures/client-weapons.js';

async function fixture({population=false}={}){
  const definitions={soldier:{class:'soldier',base:'soldier',MaxHealth:100,weapons:[]},
    hero:{class:'hero',base:'soldier',MaxHealth:1000000,weapons:[{...weaponDefinition('hero-gun',0),SpreadThreshold:0}]}};
  const posts=new CommandPosts(null,{commandPosts:[1,2].map((team,i)=>({name:`home${team}`,team,
    matrix:new Matrix4().makeTranslation(i*20,0,0).toArray(),hologram:[0,0,0]}))},{headless:true,playerTeam:1});
  for(const post of posts.posts)post.ai={spawnPath:{present:true,points:[[post.position.x,0,0,0]]},allyPath:null,allyCount:0,classSpawnPoints:0,classSpawnWorld:[],yaw:0};
  const battle=new Battle({posts,teams:{1:{reinforcements:100},2:{reinforcements:100}},own:1,foe:2});
  const w=await f3World(definitions,{prepareLive:()=>({buildings:[],animated:[],traps:[]})});
  w.factory.sides.rep.units=definitions;
  const populationState = population ? nativeSpawning(w,battle,posts,{calls:[['SetUnitCount',[1,2]],['SetUnitCount',[2,0]],['AddUnitClass',[1,'soldier',2]]]}) : null;
  new F4Live(w.phase,{sound(){}});
  const human=new HumanSpawnMirror();human.className='soldier';
  const tail=new F4Tail(w.phase,battle,{posts,human,rosters:[[],['soldier','hero'],['soldier','hero']],
    classes:['soldier','hero'],gameplay:w.ordnance.random,debug:true,...(populationState?{spawning:populationState.spawning,random:populationState.random,ai:populationState.ai}:{})});
  const roster=new F4Roster(tail,{battle,ai:null},2),connections=new F4Connections(tail);
  const player={character:roster.reserve(1,'first','First'),ordinal:1,className:'soldier',unit:null,spawn:{}};
  const draw=w.ordnance.random;
  w.phase.construction.bindPlayer(player.character,null,{combatRandom:draw,weaponRandom:draw});
  assert.equal(connections.command('admit',player,'soldier'),true);
  return {w,battle,posts,tail,roster,connections,player,draw,spawning:populationState?.spawning};
}
test('native connection births and class changes retain canonical body and stream identities',async()=>{
  const f=await fixture(),{connections,player,w,battle,draw}=f;
  assert.equal(w.units.list.length,0);
  assert.equal(connections.command('spawn',player,'soldier','home1'),true);
  const first=player.unit;
  assert.equal(first,player.character.unit);assert.equal(first.position.x,0);assert.equal(player.spawn.phase,'alive');
  first.combat.damage(Infinity);
  assert.equal(player.character.unit,null);assert.equal(player.character.deaths,1);
  connections.command('state',player);assert.equal(player.spawn.phase,'dying');
  assert.equal(connections.command('spawn',player,'hero'),false);
  w.phase.deadTime(first,4);
  assert.equal(connections.command('spawn',player,'hero','home1'),true);
  const hero=player.unit;
  assert.notEqual(hero,first);assert.equal(hero.combat.health,1000000);assert.equal(hero.combat.player,true);
  assert.equal(hero.combat.arsenal.weapons[0].random,draw);
  assert.equal(Object.is(hero.combat.arsenal.weapons[0].floor,-0),true);
  assert.equal(Object.is(hero.combat.arsenal.weapons[0].kick,-0),true);
  assert.equal(player.character.unit,hero);assert.equal(player.character.className,'hero');
  assert.deepEqual(w.units.list,[first,hero]);assert.equal(battle.reinforcements[1],99);
  const stale={...f.tail.entries.get(player.character).handle};
  f.roster.release(player.character);assert.equal(hero.removed,true);
  const replacement=f.roster.reserve(1,'second','Second');assert.equal(replacement,player.character);
  f.tail.entries.get(replacement).handle=stale;
  assert.equal(connections.command('state',player),false);
});
test('unspawn and a removed-body clock do not spend tickets or advance gameplay life',async()=>{
  const {connections,player,battle,w}=await fixture();
  connections.command('spawn',player,'soldier');const unit=player.unit,generation=unit.generation;
  connections.command('unspawn',player);
  assert.equal(unit.alive,false);assert.equal(unit.removed,true);assert.equal(unit.deadTime,4);
  assert.equal(player.spawn.phase,'unspawned');assert.equal(player.character.unit,unit);
  assert.equal(battle.reinforcements[1],100);assert.equal(unit.generation,generation);
  assert.equal(connections.command('spawn',player,'soldier'),true);assert.equal(player.unit,unit);
  unit.combat.damage(Infinity);unit.remove();w.phase.deadTime(unit,3.99);
  connections.command('state',player);assert.equal(unit.deadTime,4);assert.equal(player.spawn.phase,'awaiting');
});

test('a native hero birth publishes canonical flags and binds its native AI controller',async()=>{
  const definitions={hero:{class:'hero',base:'soldier',MaxHealth:1000000,weapons:[]}};
  const posts=new CommandPosts(null,{commandPosts:[{name:'home',team:1,matrix:new Matrix4().toArray(),hologram:[0,0,0]}]},{headless:true,playerTeam:1});
  const battle=new Battle({posts,teams:{1:{units:2,reinforcements:100},2:{units:0,reinforcements:100}},own:1,foe:2});
  const w=await f3World(definitions,{prepareLive:()=>({buildings:[],animated:[],traps:[]})});
  w.factory.sides.rep.units=definitions;
  const {spawning,random,owner,ai}=nativeSpawning(w,battle,posts,{
    calls:[['SetUnitCount',[1,2]],['SetUnitCount',[2,0]],['AddUnitClass',[1,'hero',2]]]});
  const character=spawning.characters.find(c=>!c.human);character.hero=true;character.className='hero';
  new F4Live(w.phase,{sound(){}});
  const human=new HumanSpawnMirror();human.className='hero';
  const tail=new F4Tail(w.phase,battle,{posts,spawning,human,random,gameplay:w.ordnance.random,ai});
  owner.activate();
  const unit=tail.spawnUnit(character,'hero',posts.posts[0],[0,0,0],0);
  assert.equal(unit.hero,true);assert.equal(unit.human,false);assert.equal(unit.networkHuman,undefined);
  assert.equal(unit.controller,character.controller);assert.equal(unit.controller.native,true);
});

test('a connection outside prepared capacity rejects without stopping the next tail',async()=>{
  const {w,tail,connections,player}=await fixture();
  w.phase.x.f4_connections_init(0);
  assert.equal(connections.command('admit',player,'soldier'),false);
  assert.equal(connections.command('state',player),false);
  tail.step(1/60,1,true);assert.equal(tail.counters.phaseCalls,1);
  const admitted=new F4Connections(tail);
  assert.equal(admitted.command('admit',player,'soldier'),true);
  assert.equal(admitted.command('spawn',player,'soldier'),true);
});
test('exhausted player class stock leaves the current body alive and rejects the spawn',async()=>{
  const {w,connections,player}=await fixture();
  assert.equal(connections.command('spawn',player,'soldier'),true);
  const old=player.unit;
  for(let i=0;i<4;i++)w.spawn('hero',i*3,0);
  assert.equal(connections.command('spawn',player,'hero'),false);
  assert.equal(player.unit,old);assert.equal(old.alive,true);assert.equal(old.className,'soldier');
  assert.equal(connections.command('state',player),true);
});

// Network births update the canonical population used by bleed and waves.
// A subsequent native sync must retain those counts rather than overwrite
// a host-only mirror edit. Neither operation spends reinforcement tickets.
test('network living counts survive native publication after spawn and unspawn', async () => {
  const {connections,player,spawning,tail,battle}=await fixture({population:true});
  const team=spawning.teams.find(team=>team.index===1), before=team.living;
  assert.equal(connections.command('spawn',player,'soldier','home1'),true);
  tail.phase.sync();
  assert.equal(team.living,before+1);
  assert.equal(connections.command('unspawn',player),true);
  tail.phase.sync();
  assert.equal(team.living,before);
  assert.equal(connections.command('spawn',player,'soldier','home1'),true);
  tail.phase.sync();
  assert.equal(team.living,before+1);
  assert.equal(battle.reinforcements[1],100);
});
