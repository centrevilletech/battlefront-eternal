import test from 'node:test';
import assert from 'node:assert/strict';
import { CommandPosts } from '../src/commandposts.js';
import { MobilePosts } from '../src/mobile-posts.js';
import { F4Live } from '../src/sim/wasm/f4-live.js';
import { F4Tail } from '../src/sim/wasm/f4-tail.js';
import { HumanSpawnMirror } from '../src/sim/gen/index.js';
import { f3World } from './fixtures/f3-world.js';
import { fleetFixture } from './fixtures/f3-fleet.js';
import { battleFixture } from './fixtures/f4-battle.js';

test('the native tail refreshes command pads and reports occupation and destruction in order',async()=>{
  const posts=new CommandPosts(null,{},{headless:true,playerTeam:1}),battle=battleFixture(posts),character=battle.characters[1];
  const world=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:100,weapons:[]}},{
    prepareFleet:fleetFixture({positions:[0],base:'commandhover'}),prepareLive:()=>({buildings:[],animated:[],traps:[]}),
    prepareUnit:unit=>{unit.character=character;unit.team=2;unit.combat.team=2;}});
  const unit=world.spawn('soldier',0,0,true);character.unit=unit;character.className='soldier';
  world.phase.registerViewpoint(unit);
  const fleet=world.factory.context.vehicles,hull=fleet.vehicles[0];
  posts.mobile=new MobilePosts(fleet,{tank:{values:{atk:[1,2,3,4,5,6],def:[6,5,4,3,2,1]},valueBleed:37,spawnPoints:[[3,0,0,90],[0,0,3,0]]}});
  const live=new F4Live(world.phase,{sound(){}}),human=new HumanSpawnMirror();human.className='soldier';
  const notices=[],tail=new F4Tail(world.phase,battle,{posts,human,gameplay:world.ordnance.random,
    onMobile:event=>notices.push([event.kind,event.from,event.to]),debug:true});
  tail.counters.reset();tail.step(1/60,1,true);
  const post=posts.mobile.posts[0];assert.equal(post.active,true);assert.equal(post.team,1);assert.equal(post.desc.valueBleed,37);
  assert.equal(post.ai.points.length,2);assert.equal(tail.counters.rustAllocations,0);assert.equal(tail.counters.rustReallocations,0);
  assert.equal(fleet.board(unit,hull,0),true);
  // AI reads current ownership before the tail; refreshing its view must
  // leave the ordered mobile notices for their original tail slot.
  world.phase.x.f4_viewpoint_clock();world.phase.sync();
  assert.equal(post.team,2);assert.deepEqual(notices,[]);
  tail.step(1/60,2,true);
  assert.equal(post.team,2);assert.deepEqual(notices,[['owner',1,2]]);
  hull.setHealth(0);
  world.phase.x.f4_viewpoint_clock();world.phase.sync();
  assert.equal(post.active,false);assert.equal(notices.length,1);
  tail.step(1/60,3,true);
  assert.equal(post.active,false);assert.deepEqual(notices,[['owner',1,2],['lost',2,undefined]]);
  assert.equal(tail.counters.rustAllocations,0);assert.equal(tail.counters.rustReallocations,0);
  assert.equal(live.phase.tailOwner,tail);
});
