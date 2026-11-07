import test from 'node:test';
import {FixedClock} from '../src/runtime/fixed-clock.js';
import {WorldTick} from '../src/runtime/world-tick.js';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { Battle,Character } from '../src/battle.js';
import { ClassicRandom,uniformRandom } from '../src/ai/random.js';
import { F4Live } from '../src/f4-live.js';
import { F4Roster } from '../src/f4-roster.js';
import { F4Tail } from '../src/f4-tail.js';
import { AiWorld } from '../src/ai-world.js';
import { postFacts,classFacts } from '../src/ai-world-content.js';
import { AiWorldSetupMirror,AiWorldContentMirror,AiRosterSetupMirror,AiPhaseFactsMirror } from '../src/sim/gen/ai-phase.js';
import { KIND_BATTLE,KIND_POST,KIND_COMMAND_POSTS,KIND_MOBILE_POSTS,HumanSpawnMirror,SpawningMirror } from '../src/sim/gen/index.js';
import { f3World } from './fixtures/f3-world.js';
import { ROSTER_SEED,rosterCalls,rosterPrepared,rosterNames,rosterPosts,rosterSnapshot } from './fixtures/ai-roster.js';

// Frozen before authoritative JS deletion. The reference includes the exact
// shared A state after quotas, objectives, class choices and spawn queues.
const reference=JSON.parse(fs.readFileSync(new URL('./fixtures/ai-roster-golden.json',import.meta.url),'utf8'));
for(const early of [false,true])for(const row of reference) test(`native ${early?'cold':'prepared'} initial ${row.enhanced?'Enhanced':'Classic'} roster, side ${row.team}, heroes ${row.heroes}`,async()=>{
  const posts=rosterPosts(),battle=new Battle({posts,teams:{1:{reinforcements:80},2:{reinforcements:80}},own:row.team,foe:3-row.team});
  const definitions=Object.fromEntries(rosterPrepared.map(name=>[name,{class:name,base:'soldier',MaxHealth:100,weapons:[]}]));
  const world=await f3World(definitions,{debug:false,prepareLive:()=>({buildings:[],animated:[],traps:[]})});
  const random=new ClassicRandom(ROSTER_SEED),randomB=new ClassicRandom(ROSTER_SEED^0x9e3779b9),spawning=new SpawningMirror();
  const tailOptions={posts,spawning,human:new HumanSpawnMirror(),random,gameplay:world.ordnance.random,enhanced:row.enhanced,
    heroNames:rosterPrepared.map(className=>({className,name:`Name ${className}`}))};
  if(early) {
    const b=world.phase.bridge;b.identify({classes:{Character,Post:posts.posts[0].constructor}});
    b.write(KIND_POST,posts.posts,true);b.write(KIND_COMMAND_POSTS,[posts],true);
    b.write(KIND_MOBILE_POSTS,[{posts:[]}],true);b.write(KIND_BATTLE,[battle],true);
  }else {new F4Live(world.phase,{sound(){}});new F4Tail(world.phase,battle,tailOptions);}
  const setup=new AiWorldSetupMirror(),settings=setup.settings;
  Object.assign(setup.navigation.pools,{paths:900,pathNodes:7200,requests:75});
  Object.assign(settings,{seed:ROSTER_SEED,streamA:world.phase.random.register(uniformRandom(random)),streamB:world.phase.random.register(uniformRandom(randomB)),
    enhanced:row.enhanced,tier:3,profileTier:3,playerTeam:row.team,playerSkill:4,enemySkill:14,sensorRange:75,lodDisabled:true});
  Object.assign(settings.capacity,{registry:256,characters:80,posts:4,hulls:8,controls:16,scopes:16,cameras:2,grenades:64,supportKeys:32,repairJobs:32});
  Object.assign(settings.capabilities,{navigation:true,population:true,randomB:true});
  const owner=new AiWorld(world.phase,setup),content=new AiWorldContentMirror();
  content.posts=posts.posts.map(postFacts);content.classes=Object.values(definitions).map(def=>classFacts(def));owner.content(content);
  const program=new AiRosterSetupMirror();
  Object.assign(program,{calls:rosterCalls.map(call=>({...call,faction:null,mask:null})),names:Array.from({length:5},(_,index)=>rosterNames[index]),local:rosterNames.local,
    humans:[{team:row.team,id:17,name:'Human'}],unitScale:1.5,heroes:row.heroes,demo:false,capacity:200,prepared:rosterPrepared,heroNames:tailOptions.heroNames});
  assert.equal(owner.initializeRoster(program,{battle,spawning}),true);
  assert.deepEqual(rosterSnapshot(spawning,random),row.expected);
  assert.equal(battle.characters.length,spawning.characters.length);
  assert.equal(battle.human.team,row.team);
  const before=random.state;assert.equal(owner.initializeRoster(program),false);assert.equal(random.state,before);
  if(early){new F4Live(world.phase,{sound(){}});new F4Tail(world.phase,battle,tailOptions);}
  owner.activate();
  for(const character of battle.characters)if(!character.human)assert.equal(character.controller.native,true);
  // Native network admission can append Characters without invoking the
  // former JS attach hook. Structural identities must precede roster rows.
  const roster=new F4Roster(world.phase.tailOwner,{ai:{native:true,spawning},battle},10);
  assert.ok(battle.characters.length>=20);
  for(const character of battle.characters) {
    assert.ok(world.phase.tailOwner.entries.has(character));
    if(!character.human)assert.equal(character.controller.native,true);
  }
  const human=roster.reserve(row.team,29,'New human');
  assert.equal(human.human,true);
  const facts=new AiPhaseFactsMirror();facts.dt=1/60;facts.tick=1;facts.lodDisabled=true;
  world.phase.registerViewpoint(world.spawn(rosterPrepared[0],80,80,true,null));
  const tick=new WorldTick(world.phase,new FixedClock());
  owner.prepare(facts,[],{sequence:0,records:[]},1/60,false,false,true);
  tick.run(1/60,1,true,null,true,true,0,false,0);tick.finish();tick.complete();
  roster.release(human);assert.equal(human.human,false);assert.equal(human.controller.native,true);
});
