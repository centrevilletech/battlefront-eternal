import { Character } from '../../src/battle.js';
import { ClassicRandom,uniformRandom } from '../../src/ai/random.js';
import { AiWorld } from '../../src/ai-world.js';
import { classFacts,postFacts } from '../../src/ai-world-content.js';
import { AiWorldSetupMirror,AiWorldContentMirror,AiRosterSetupMirror } from '../../src/sim/gen/ai-phase.js';
import { KIND_BATTLE,KIND_POST,KIND_COMMAND_POSTS,KIND_MOBILE_POSTS,SpawningMirror } from '../../src/sim/gen/index.js';

// F4 fixtures use the same cold native roster entry as Game. No JavaScript
// spawning, objective selection or private naming implementation is retained.
export function nativeSpawning(world,battle,posts,{calls,enhanced=false,seed=739,names={},prepared=null}={}) {
  const phase=world.phase,b=phase.bridge,random=new ClassicRandom(seed),randomB=new ClassicRandom(seed^0x9e3779b9);
  const spawning=new SpawningMirror();
  b.identify({classes:{Character,Post:posts.posts[0].constructor}});
  for(const post of posts.posts)post.ai={order:0,atk:[100,100,100,100,100,100],def:[100,100,100,100,100,100],
    bans:0,valueBleed:100,capture:null,radius:0,yaw:0,spawnPath:null,allyPath:null,allyCount:0,classSpawnPoints:0,classSpawnWorld:[],...post.ai};
  b.write(KIND_POST,posts.posts,true);b.write(KIND_COMMAND_POSTS,[posts],true);
  b.write(KIND_MOBILE_POSTS,[{posts:[]}],true);b.write(KIND_BATTLE,[battle],true);
  const setup=new AiWorldSetupMirror(),settings=setup.settings;
  Object.assign(setup.navigation.pools,{paths:900,pathNodes:7200,requests:75});
  Object.assign(settings,{seed,streamA:phase.random.register(uniformRandom(random)),streamB:phase.random.register(uniformRandom(randomB)),
    enhanced,tier:3,profileTier:3,playerTeam:1,playerSkill:4,enemySkill:14,sensorRange:75,lodDisabled:true});
  Object.assign(settings.capacity,{registry:256,characters:80,posts:posts.posts.length,hulls:8,controls:16,scopes:16,cameras:2,grenades:64,supportKeys:32,repairJobs:32});
  Object.assign(settings.capabilities,{population:true,randomB:true});
  const owner=new AiWorld(phase,setup),content=new AiWorldContentMirror();
  content.posts=posts.posts.map(postFacts);content.classes=b.defs.unitClass.list.map(def=>classFacts(def));owner.content(content);
  const program=new AiRosterSetupMirror();
  // Game's translated mission includes the battle's tickets before begin.
  // A zero-ticket cold bot is deactivated before any fixture body is placed.
  const instructions=[1,2].map(team=>({name:'SetReinforcementCount',args:[team,battle.reinforcements[team]??0]}));
  instructions.push(...calls.map(call=>Array.isArray(call)?{name:call[0],args:call[1]}:call));
  Object.assign(program,{calls:instructions.map(call=>({...call,faction:call.faction??null,mask:call.mask??null})),
    names:Array.from({length:5},(_,index)=>({first:names[index]?.first??[],last:names[index]?.last??[]})),local:names.local??null,
    humans:[{team:1,id:null,name:null}],unitScale:1,heroes:false,demo:false,capacity:200,
    prepared:prepared??instructions.filter(call=>call.name==='AddUnitClass').map(call=>call.args[1])});
  if(!owner.initializeRoster(program,{battle,spawning}))throw Error('invalid native fixture roster');
  return {spawning,random,owner,ai:{native:true,enhanced,spawning,owner}};
}
