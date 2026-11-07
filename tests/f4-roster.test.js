import test from 'node:test';
import assert from 'node:assert/strict';
import { Matrix4 } from 'three';
import { Battle,Character } from '../src/battle.js';
import { CommandPosts } from '../src/commandposts.js';
import { nativeSpawning } from './fixtures/f4-spawning.js';
import { F4Live } from '../src/sim/wasm/f4-live.js';
import { F4Tail } from '../src/sim/wasm/f4-tail.js';
import { F4Roster } from '../src/sim/wasm/f4-roster.js';
import { HumanSpawnMirror } from '../src/sim/gen/index.js';
import { f3World } from './fixtures/f3-world.js';

for(const enhanced of [false,true])test(`native ${enhanced?'Enhanced':'Classic'} roster fills, replaces and releases slots with retained names`,async()=>{
  const posts=new CommandPosts(null,{commandPosts:[1,2].map((team,i)=>({name:`post${team}`,team,
    matrix:new Matrix4().makeTranslation(i*20,0,0).toArray(),hologram:[0,0,0]}))},{headless:true,playerTeam:1});
  for(const post of posts.posts)post.ai={spawnPath:{present:true,points:[[post.position.x,0,0,0]]},allyPath:null,allyCount:0,classSpawnPoints:0,classSpawnWorld:[],yaw:0};
  const battle=new Battle({posts,teams:{1:{units:2,reinforcements:100},2:{units:2,reinforcements:100}},own:1,foe:2});
  const world=await f3World({},{prepareLive:()=>({buildings:[],animated:[],traps:[]})});
  const {spawning,random,owner,ai}=nativeSpawning(world,battle,posts,{enhanced,
    calls:[['SetUnitCount',[1,2]],['SetUnitCount',[2,2]],['AddUnitClass',[1,'soldier',2]],['AddUnitClass',[2,'soldier',2]]],
    names:{0:{first:['Alpha','Beta'],last:['One','Two']}}});
  new F4Live(world.phase,{sound(){}});
  const human=new HumanSpawnMirror();human.className='soldier';
  const tail=new F4Tail(world.phase,battle,{posts,spawning,human,random,gameplay:world.ordnance.random,ai,debug:true});
  owner.activate();
  const roster=new F4Roster(tail,{battle,ai},3);
  assert.deepEqual(roster.counts(),[{team:1,humans:0,total:3},{team:2,humans:0,total:3}]);
  assert.equal(battle.characters.length,7);
  const nativeBorn=battle.characters.slice(4);
  assert.equal(nativeBorn.length,3);
  for(const c of nativeBorn){assert.equal(c instanceof Character,true);assert.ok(posts.posts.includes(c.objective));}
  const indexOrder=battle.characters.map(c=>c.index);
  const player=roster.reserve(1,'connection','player');assert.equal(player.human,true);assert.equal(player.name,'player');
  for(const invalid of [-1,0,3,4096,Infinity,NaN])assert.equal(roster.switchTeam(player,invalid),false);
  assert.equal(player.team,1);
  assert.deepEqual(roster.counts(),[{team:1,humans:1,total:3},{team:2,humans:0,total:3}]);
  roster.release(player);assert.equal(player.human,false);assert.equal(player.playerId,-1);assert.equal(player.name,'Beta One');assert.equal(player.controller.native,true);
  assert.ok(posts.posts.includes(player.objective));
  const next=roster.reserve(1,'next','next');assert.equal(roster.switchTeam(next,2),true);assert.equal(next.team,2);
  assert.deepEqual(roster.counts(),[{team:1,humans:0,total:3},{team:2,humans:1,total:3}]);
  assert.deepEqual(battle.characters.map(c=>c.index),indexOrder);
  assert.equal(battle.reinforcements[1],100);assert.equal(battle.reinforcements[2],100);
  assert.equal(next.controller,null,'a human reservation releases its native AI controller');
});
