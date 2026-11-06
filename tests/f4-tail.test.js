import test from 'node:test';
import assert from 'node:assert/strict';
import { Matrix4 } from 'three';
import { CommandPosts } from '../src/commandposts.js';
import { Boundary } from '../src/ai/classic/nav/boundary.js';
import { F4Live } from '../src/f4-live.js';
import { F4Roster } from '../src/f4-roster.js';
import { F4Tail } from '../src/f4-tail.js';
import { HumanSpawnMirror } from '../src/sim/gen/index.js';
import { f3World } from './fixtures/f3-world.js';
import { battleFixture } from './fixtures/f4-battle.js';
const soldier={class:'soldier',base:'soldier',MaxHealth:100,weapons:[]};
async function fixture(boundary,postHook=null){
  const posts=new CommandPosts(null,{commandPost:{neutralizeTime:1,captureTime:1},commandPosts:[{
    name:'center',team:0,matrix:new Matrix4().toArray(),hologram:[0,0,0],capture:{type:'sphere',size:[10,10,10]},
  }]},{headless:true,playerTeam:1});
  const battle=battleFixture(posts),[a,b]=battle.characters;b.team=1;
  const world=await f3World({capturer:soldier,hero:{...soldier,CapturePosts:0}},{
    prepareLive:()=>({buildings:[],animated:[],traps:[]}),
    prepareUnit:unit=>{const c=unit.className==='capturer'?a:b;unit.character=c;unit.team=c.team;unit.combat.team=c.team;},
    deathCredit:()=>{throw Error('foreign Character death');},
  });
  const first=world.spawn('capturer',0,0),hero=world.spawn('hero',0,0);
  a.unit=first;a.className='capturer';b.unit=hero;b.className='hero';
  const live=new F4Live(world.phase,{sound(){}}),human=new HumanSpawnMirror();human.className='capturer';
  const observed=[];
  const tail=new F4Tail(world.phase,battle,{posts,human,boundary,gameplay:world.ordnance.random,
    onPost:event=>{observed.push(event.kind);assert.equal(a.captures,1);assert.equal(b.captures,1);postHook?.(event,world.phase.tailOwner,first,a);},debug:true});
  return {world,posts,battle,a,b,first,hero,live,tail,observed};
}
test('one native tail captures and awards the hero before its foreign post notice',async()=>{
  const f=await fixture();f.posts.update=()=>{throw Error('legacy post phase');};f.battle.update=()=>{throw Error('legacy rule phase');};
  f.tail.counters.reset();f.tail.step(.5,1,true);
  assert.equal(f.posts.posts[0].progress,.5);assert.deepEqual(f.observed,[]);
  assert.equal(f.tail.counters.wasmCalls,1);assert.equal(f.tail.counters.phaseCalls,1);
  f.tail.step(.5,2,true);
  assert.equal(f.posts.posts[0].team,1);assert.deepEqual(f.observed,['captured']);
  assert.equal(f.a.captures,1);assert.equal(f.b.captures,1);assert.equal(f.battle.stats.time,1);
  assert.equal(f.tail.counters.wasmCalls,2);assert.equal(f.tail.counters.phaseCalls,2);
  assert.equal(f.tail.counters.rustAllocations,0);assert.equal(f.tail.counters.rustReallocations,0);
  for(let tick=3;tick<1203;tick++)f.tail.step(1/60,tick,true);
  assert.equal(f.tail.counters.rustAllocations,0);assert.equal(f.tail.counters.rustReallocations,0);
});
test('native battlefield death completes character credit before returning the tail',async()=>{
  const boundary=new Boundary([[[30,0,30],[30,0,40],[40,0,40],[40,0,30]]]);
  const f=await fixture(boundary);f.tail.step(10.1,1,true);
  assert.equal(f.first.combat.dead,true);assert.equal(f.hero.combat.dead,true);
  assert.equal(f.a.unit,null);assert.equal(f.b.unit,null);assert.equal(f.a.deaths,1);assert.equal(f.b.deaths,1);
  assert.equal(f.battle.reinforcements[1],98);assert.deepEqual(f.observed,[]);
  assert.equal(f.tail.counters.rustAllocations,0);assert.equal(f.tail.counters.rustReallocations,0);
});

test('native human input kills and switches team before publishing ordered spawn notices',async()=>{
  const posts=new CommandPosts(null,{},{headless:true,playerTeam:1}),battle=battleFixture(posts),[character]=battle.characters;
  character.human=true;
  const world=await f3World({capturer:soldier},{prepareLive:()=>({buildings:[],animated:[],traps:[]}),
    prepareUnit:unit=>{unit.character=character;unit.team=1;unit.combat.team=1;},
    deathCredit:()=>{throw Error('foreign human death');}});
  const unit=world.spawn('capturer',0,0,true);character.unit=unit;character.className='capturer';
  new F4Live(world.phase,{local:unit,sound(){}});
  const human=new HumanSpawnMirror();human.className='capturer';human.phase='alive';human.spawnedOnce=true;
  const events=[],teams=[];
  const tail=new F4Tail(world.phase,battle,{posts,human,local:unit,character,gameplay:world.ordnance.random,
    rosters:[[],['capturer'],['capturer']],classes:['capturer'],onHuman:event=>events.push(event.kind),retarget:team=>teams.push(team),debug:true});
  tail.counters.reset();tail.step(.1,1,false,[{kind:'suicide'}]);
  assert.equal(unit.combat.dead,true);assert.equal(character.unit,null);assert.equal(character.deaths,1);
  assert.equal(human.phase,'dying');assert.equal(human.deadTime,0);assert.equal(battle.reinforcements[1],99);
  assert.deepEqual(events,['playerDied']);
  tail.step(1,61,false,[{kind:'team',team:2},{kind:'cancel'}]);
  assert.equal(battle.own,2);assert.equal(character.team,2);assert.equal(unit.team,2);assert.deepEqual(teams,[2]);
  assert.deepEqual(events,['playerDied','teamChanged']);
  tail.step(3,241,false);assert.equal(human.phase,'awaiting');
  assert.deepEqual(events,['playerDied','teamChanged','awaitingSpawn']);
  assert.equal(tail.counters.rustAllocations,0);assert.equal(tail.counters.rustReallocations,0);
});

test('an AI post hook publishes qualified objectives and controllers before the tail resumes',async()=>{
  const controller={};
  const f=await fixture(undefined,(event,owner,unit,character)=>{
    assert.equal(owner.objective(character,event.post),true);
    assert.equal(owner.controller(unit,controller),true);
    assert.equal(character.objective,event.post);assert.equal(unit.controller,controller);
  });
  f.tail.counters.reset();f.tail.step(1,1,true);
  assert.equal(f.a.objective,f.posts.posts[0]);assert.equal(f.first.controller,controller);
  assert.equal(f.tail.counters.phaseCalls,1);assert.equal(f.tail.counters.commandCalls,2);
  assert.equal(f.tail.counters.importCalls,1);assert.equal(f.tail.counters.rustAllocations,0);assert.equal(f.tail.counters.rustReallocations,0);
});

test('native removal clears the canonical Character without the former release import',async()=>{
  const f=await fixture();
  f.world.phase.bridge.handlers.f3CharacterRelease=()=>{throw Error('foreign Character release');};
  f.first.remove();
  assert.equal(f.a.unit,null);assert.equal(f.first.character,null);assert.equal(f.first.removed,true);
  f.tail.step(1,1,true);assert.equal(f.a.unit,null);
});

test('native player roster retains slot and wire order across capacity, team switch and reuse',async()=>{
  const f=await fixture(),roster=new F4Roster(f.tail,{battle:f.battle,ai:null},1);
  assert.equal(f.battle.characters.length,0);
  assert.deepEqual(roster.counts(),[{team:1,humans:0},{team:2,humans:0}]);
  const name='\u{1f642}'.repeat(17),a=roster.reserve(1,'first',name),b=roster.reserve(2,7,'second');
  assert.equal(a.name,name.slice(0,32));assert.equal(a.name.length,32);assert.equal(a.index,0);assert.equal(b.index,1);
  assert.deepEqual(f.battle.characters.map(c=>c.rosterSlot),[0,1]);
  assert.equal(a.playerId,'first');assert.equal(b.playerId,7);assert.equal(a.spawn.timer,Infinity);
  assert.throws(()=>roster.reserve(1,'extra','full'),/team full/);
  assert.equal(roster.switchTeam(a,2),false);
  const stale={...f.tail.entries.get(a).handle};
  roster.release(a);assert.equal(a.team,0);assert.equal(a.unit,null);
  const edge='a'.repeat(31)+'\u{1f642}',temporary=roster.reserve(1,'temporary',edge);
  assert.equal(temporary.name,edge.slice(0,32));roster.release(temporary);
  const reused=roster.reserve(1,'next','replacement');assert.equal(reused,a);
  assert.equal(f.battle.characters.length,2);assert.deepEqual(f.battle.characters,[a,b]);
  assert.equal(roster.command('release',a,0,undefined,'',stale).flag,false);assert.equal(a.team,1);
  assert.deepEqual(roster.counts(),[{team:1,humans:1},{team:2,humans:1}]);
  const wire=JSON.stringify(f.battle.characters.map(c=>({index:c.index,team:c.team,human:c.human,name:c.name,id:c.playerId,kills:c.kills,deaths:c.deaths,captures:c.captures})));
  assert.equal(wire,'[{"index":0,"team":1,"human":true,"name":"replacement","id":"next","kills":0,"deaths":0,"captures":0},{"index":1,"team":2,"human":true,"name":"second","id":7,"kills":0,"deaths":0,"captures":0}]');
});

test('oversized input counts stay within prepared native human command storage',async()=>{
  const f=await fixture(),p=f.world.phase;
  f.tail.step(1/60,1,true,Array(16384).fill({kind:'cancel'}));
  assert.equal(f.tail.counters.phaseCalls,1);
  const {F4TailInputLayout}=await import('../src/sim/gen/f4.js');
  p.bridge.views();p.bridge.f[f.live.layout.tailInput/8+F4TailInputLayout.humanCount]=0xffffffff;
  f.tail.x.f4_tail();p.sync();
  assert.equal(f.battle.stats.time,2/60);
  assert.equal(f.tail.counters.rustAllocations,0);assert.equal(f.tail.counters.rustReallocations,0);
});
