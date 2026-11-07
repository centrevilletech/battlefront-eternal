import test from 'node:test';
import assert from 'node:assert/strict';
import {Matrix4} from 'three';
import {Battle} from '../src/battle.js';
import {CommandPosts} from '../src/commandposts.js';
import {nativeSpawning} from './fixtures/f4-spawning.js';
import {F4Live} from '../src/sim/wasm/f4-live.js';
import {F4Tail} from '../src/sim/wasm/f4-tail.js';
import {HumanSpawnMirror} from '../src/sim/gen/index.js';
import {f3World} from './fixtures/f3-world.js';

test('dead player combat confirms the native Classic queue before the battle tail',async()=>{
  const posts=new CommandPosts(null,{commandPosts:[{name:'home',team:1,spawnPath:'raw',
    matrix:new Matrix4().toArray(),hologram:[0,0,0]}]},{headless:true,playerTeam:1});
  posts.posts[0].ai={spawnPath:{present:true,points:[[9,0,0,0]]},allyPath:null,allyCount:0,classSpawnPoints:0,classSpawnWorld:[],yaw:0};
  let character;
  const battle=new Battle({posts,teams:{1:{units:1,reinforcements:100},2:{units:0,reinforcements:100}},own:1,foe:2});
  const world=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:100,weapons:[]}},{
    prepareLive:()=>({buildings:[],animated:[],traps:[]}),
    prepareUnit:unit=>{unit.character=character;unit.team=1;unit.combat.team=1;},
    deathCredit:()=>{throw Error('foreign death');},
  });
  const {spawning,random,owner}=nativeSpawning(world,battle,posts,{
    calls:[['SetUnitCount',[1,1]],['SetUnitCount',[2,0]],['AddUnitClass',[1,'soldier',1]]]});
  character=spawning.characters.find(c=>c.human);
  const unit=world.spawn('soldier',0,0,true,null);unit.character=character;world.phase.publishMetadata(unit);character.unit=unit;character.className='soldier';
  new F4Live(world.phase,{local:unit,sound(){}});
  const human=new HumanSpawnMirror();human.className='soldier';human.phase='alive';human.spawnedOnce=true;
  human.auto={team:1,className:'soldier',post:null};
  const tail=new F4Tail(world.phase,battle,{posts,spawning,human,local:unit,character,random,
    gameplay:world.ordnance.random,paths:{raw:[[71,0,0,0]]},debug:true});
  owner.activate();
  unit.combat.onRespawn=()=>{throw Error('foreign respawn');};
  unit.combat.damage(Infinity);
  tail.step(4,1,true);
  assert.equal(character.unit,null);assert.equal(character.spawn.ready,false);
  world.phase.phase(4.1,2);
  assert.equal(character.spawn.ready,true);
  assert.equal(character.spawn.post,posts.posts[0]);
  assert.equal(character.unit,null,'readiness does not place early');
  assert.equal(unit.combat.dead,true);
});
