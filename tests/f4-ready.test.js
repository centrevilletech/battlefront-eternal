import test from 'node:test';
import assert from 'node:assert/strict';
import {Matrix4} from 'three';
import {Battle} from '../src/battle.js';
import {CommandPosts} from '../src/commandposts.js';
import {nativeSpawning} from './fixtures/f4-spawning.js';
import {F4Live} from '../src/sim/wasm/f4-live.js';
import {F4Tail} from '../src/sim/wasm/f4-tail.js';
import {readyCharacter} from '../src/sim/wasm/f4-characters.js';
import {HumanSpawnMirror} from '../src/sim/gen/index.js';
import * as F from '../src/sim/gen/f4.js';
import {f3World} from './fixtures/f3-world.js';
const READY={words:F.F4HumanReadyLayout.words,put:F.putF4HumanReady};
async function fixture(onPost=null){
  const posts=new CommandPosts(null,{commandPost:{neutralizeTime:1,captureTime:1},commandPosts:[{name:'center',team:0,
    matrix:new Matrix4().toArray(),hologram:[0,0,0],capture:{type:'sphere',size:[10,10,10]}}]},{headless:true,playerTeam:1});
  posts.posts[0].ai={spawnPath:{present:true,points:[[0,0,0,0]]},allyPath:null,allyCount:0,classSpawnPoints:0,classSpawnWorld:[],yaw:0};
  let human,bot;
  const battle=new Battle({posts,teams:{1:{units:2,reinforcements:100},2:{units:0,reinforcements:100}},own:1,foe:2});
  const world=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:100,weapons:[]}}, {
    prepareLive:()=>({buildings:[],animated:[],traps:[]}),prepareUnit:u=>{u.character=bot;u.team=u.combat.team=1;},
  });
  const {spawning,random,owner}=nativeSpawning(world,battle,posts,{
    calls:[['SetUnitCount',[1,2]],['SetUnitCount',[2,0]],['AddUnitClass',[1,'soldier',1]],['AddUnitClass',[1,'hero',1]]]});
  spawning.teams[1].classes[1].hero=true;
  human=spawning.characters.find(c=>c.human);bot=spawning.characters.find(c=>!c.human);
  human.spawn.timer=0;human.spawn.ready=false;
  bot.unit=world.spawn('soldier',0,0,false,null);bot.unit.character=bot;world.phase.publishMetadata(bot.unit);bot.className='soldier';
  new F4Live(world.phase,{sound(){}});
  const screen=new HumanSpawnMirror();screen.className='soldier';
  const tail=new F4Tail(world.phase,battle,{posts,spawning,human:screen,random,gameplay:world.ordnance.random,
    heroNames:[{className:'hero',name:'Native hero'}],onPost:e=>onPost?.(e,tail,human),debug:true});
  owner.activate();
  return {world,posts,spawning,human,tail};
}
test('qualified human readiness publishes hero naming and queues and rejects stale posts',async()=>{
  const {world,posts,human,tail}=await fixture(),post=posts.posts[0];
  const target=tail.entries.get(human).handle,handle=tail.posts.get(post).handle;
  tail.bridge.callArgs(READY,{target:{...target,life:target.life+1},className:'hero',classIndex:null,post:handle,suppliedPost:true});
  assert.equal(tail.x.f4_human_ready(),0);world.phase.sync();assert.equal(human.spawn.ready,false);
  tail.bridge.callArgs(READY,{target,className:'hero',classIndex:null,post:{...handle,generation:handle.generation+1},suppliedPost:true});
  assert.equal(tail.x.f4_human_ready(),0);world.phase.sync();assert.equal(human.spawn.ready,false);
  assert.equal(readyCharacter(human,{className:'hero',post}),true);
  assert.equal(human.spawn.ready,true);assert.equal(human.spawn.post,post);assert.equal(human.hero,true);assert.equal(human.name,'Native hero');
  assert.equal(tail.readyHuman(human,{className:'soldier',post:null}),false);assert.equal(human.className,'hero');
  assert.equal(tail.command('cancel',human),true);assert.equal(human.spawn.ready,false);
  assert.equal(tail.readyHuman(human,{classIndex:0}),true);assert.equal(human.className,'soldier');assert.equal(human.spawn.post,post);
  tail.command('cancel',human);
  assert.equal(tail.readyHuman(human,{post:null}),true);assert.equal(human.spawn.post,null);
});
test('a native post notice can confirm and cancel a human choice after publication',async()=>{
  let notices=0;
  const {tail,human}=await fixture((event,owner,character)=>{
    notices++;assert.equal(event.kind,'captured');
    assert.equal(owner.readyHuman(character,{className:'hero',post:event.post}),true);
    assert.equal(character.spawn.ready,true);assert.equal(character.name,'Native hero');
    assert.equal(owner.command('cancel',character),true);assert.equal(character.spawn.ready,false);
  });
  tail.counters.reset();tail.step(.5,1,true);tail.step(.5,2,true);
  assert.equal(notices,1);assert.equal(human.spawn.ready,false);
  assert.equal(tail.counters.phaseCalls,2);assert.equal(tail.counters.commandCalls,2);
  assert.equal(tail.counters.rustAllocations,0);assert.equal(tail.counters.rustReallocations,0);
});
