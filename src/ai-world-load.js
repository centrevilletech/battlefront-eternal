// Cold content joins the canonical native owners once. Per-tick input below
// contains camera and journal facts; it never builds controller snapshots.
import { MAX_TEAM_SIZE } from './battle-limits.js';
import { Character } from './battle.js';
import { readyCharacter } from './f4-characters.js';
import { preparedPads } from './f4-tail.js';
import { boundaryFromNav } from './battlefield.js';
import { selectAIMission } from './mission.js';
import { attachPostAI } from './post-data.js';
import { loadAIData } from './ai-data.js';
import { ClassicRandom,uniformRandom } from './ai/random.js';
import { callsFromMapMission } from './mission-ai-calls.js';
import { AiWorld } from './ai-world.js';
import { AiJournalFacts } from './ai-world-input.js';
import { classFacts,hullFacts,postFacts,propFacts,itemFacts } from './ai-world-content.js';
import { SpawningMirror,KIND_POST,KIND_COMMAND_POSTS,KIND_MOBILE_POST,KIND_MOBILE_POSTS,KIND_BATTLE } from './sim/gen/index.js';
import { AiWorldSetupMirror,AiWorldContentMirror,AiRosterSetupMirror,AiPhaseFactsMirror,AiPhaseCameraMirror } from './sim/gen/ai-phase.js';

class SpawningView extends SpawningMirror {
  relation(a,b) {return this.teams[a]?.relations[b]??0;}
  tickets(team) {return this.teams[team]?.tickets??0;}
  awaitingHuman(character) {return Boolean(character?.human&&!character.unit&&character.spawn&&!character.spawn.ready&&character.spawn.timer<=0);}
}
function setupRows(world,options,data,calls,posts,hulls,classes) {
  const setup=new AiWorldSetupMirror(),settings=setup.settings,phase=world.phase,seed=options.seed>>>0;
  Object.assign(settings,{seed,enhanced:options.enhanced,tier:options.tier,profileTier:options.tier,playerTeam:options.playerTeam,
    streamA:phase.random.register(uniformRandom(world.random)),streamB:phase.random.register(uniformRandom(world.randomB)),
    streams:{a:world.random.state,b:world.randomB.state,c:(seed^0x51ed270b)>>>0},playerSkill:4,enemySkill:14,sensorRange:75,
    mission:calls.map(call=>({name:call.name,args:call.args.map(value=>Number(value)||0),strings:call.args.filter(value=>typeof value==='string')}))});
  Object.assign(settings.capabilities,{navigation:true,combat:true,combatAgent:true,population:true,vehicles:true,classes:true,randomB:true,mineGround:true,vehicleAdapter:true});
  // Future roster admissions reuse prepared slots. Geometry and recursive
  // scratch bounds cover all cold bodies, props, controls and network views.
  const counts=calls.filter(call=>call.name==='SetUnitCount');
  const scaled=call=>Math.max(0,Math.ceil(Number(call.args[1])*options.unitScale));
  const locals=counts.filter(call=>Number(call.args[0])>2).reduce((sum,call)=>sum+scaled(call),0);
  // Dedicated play can pad both armies while retaining the dormant local
  // Character outside those teams. Locals keep their authored roster too.
  const characters=Math.max(2*MAX_TEAM_SIZE+1+locals,counts.reduce((sum,call)=>sum+scaled(call),0)+32);
  const registry=Math.max(256,world.units.nextId+characters*2+hulls.length+phase.props.buildings.length+64);
  Object.assign(settings.capacity,{registry,characters,posts:posts.length,hulls:Math.max(8,hulls.length),buildings:phase.props.buildings.length,
    controls:Math.max(characters*2,hulls.reduce((sum,hull)=>sum+hull.seats.length,0)*2+phase.props.buildings.length),
    grenades:4096,scopes:16,cameras:64,supportKeys:Math.max(16,classes.length+hulls.length+phase.props.buildings.length),repairJobs:registry});
  Object.assign(setup.navigation,{json:data.navBytes??[],bin:data.navBin?new Uint8Array(data.navBin):[],
    posts:world.posts.posts.map(post=>({post,name:post.name,position:post.position}))});
  Object.assign(setup.navigation.pools,{paths:900,pathNodes:7200,requests:75});
  return setup;
}
export async function createNativeAI(mode,world,options) {
  if(mode==='off'||!world.phase)return null;
  if(mode!=='classic'&&mode!=='enhanced')throw Error(`unknown AI mode ${mode}`);
  const enhanced=mode==='enhanced',data=await loadAIData(options.map,options.sides);
  const mission=selectAIMission(data.mission,world.mission);
  const calls=mission?.scripts?.[mission.mission]?.calls??callsFromMapMission(world.mission);
  attachPostAI(world.posts.posts,mission,world.spawns);
  world.posts.attachVehicles?.(world.vehicleSystem,mission?.commandClasses);
  const posts=world.posts.all?.()??world.posts.posts,fleet=world.vehicleSystem;
  const hulls=[...new Set([...(fleet?.vehicles??[]),...(fleet?.f2?.prepared??[])])];
  const definitions=[],prepared=[];
  for(const side of Object.values(world.factory.sides))for(const [name,def] of Object.entries(side.units??{})) {
    if(world.factory.prepared(name)){definitions.push(def);prepared.push(name);}
  }
  world.randomB=new ClassicRandom((options.seed^0x9e3779b9)>>>0);
  const phase=world.phase,b=phase.bridge;
  b.identify({classes:{Character,Post:world.posts.posts[0]?.constructor}});
  // The placeholder Battle.populate roster was never the Classic roster.
  // Native Spawning.begin replaces it at the same cold construction point.
  world.battle.characters=[];
  b.write(KIND_POST,world.posts.posts,true);b.write(KIND_COMMAND_POSTS,[world.posts],true);
  b.write(KIND_MOBILE_POST,world.posts.mobile?.posts??[],true);b.write(KIND_MOBILE_POSTS,[world.posts.mobile??{posts:[]}],true);
  b.write(KIND_BATTLE,[world.battle],true);
  const setup=setupRows(world,{...options,enhanced,unitScale:options.unitScale??1},data,calls,posts,hulls,definitions);
  const owner=new AiWorld(phase,setup,{events:world.events}),content=new AiWorldContentMirror();
  content.classes=definitions.map(def=>classFacts(def));content.hulls=hulls.map(hullFacts);content.posts=posts.map(postFacts);
  content.weapons=b.defs.weapon.list.slice();content.weaponData=data.classBytes.filter(Boolean).map(json=>({json}));
  // F3 prepares item definitions embedded in drop/weapon records as well as
  // the side's named classes. Preserve every canonical identity, including
  // distinct objects that happen to carry the same item class name.
  for(const side of Object.values(world.factory.sides))for(const def of Object.values(side.items??{}))b.defs.item.id(def);
  for(const def of b.defs.item.list)content.items.push(itemFacts(def));
  for(let index=0;index<phase.props.buildings.length;index++) {
    const object=phase.props.buildings[index];
    const post=world.posts.posts.find(post=>post.building===object)??null;
    content.props.push(propFacts({object,entry:{handle:owner.propOwner(index)}},post));
  }
  owner.content(content);
  const preparedClasses=new Set(definitions),itemCount=content.items.length;
  const program=new AiRosterSetupMirror();
  Object.assign(program,{calls:calls.map(call=>({name:call.name,args:call.args,faction:call.faction??null,mask:call.mask??null})),
    names:Array.from({length:5},(_,index)=>({first:mission?.names?.[index]?.first??[],last:mission?.names?.[index]?.last??[]})),local:mission?.names?.local??null,
    humans:[{team:options.playerTeam,id:null,name:null}],unitScale:options.unitScale??1,heroes:options.heroes,demo:false,capacity:200,prepared,
    pads:preparedPads(fleet,world.posts).flatMap((pad,index)=>pad?[{index,pad}]:[]),heroNames:definitions.map(def=>({className:def.class,name:def.displayName??null}))});
  const spawning=new SpawningView();
  if(!owner.initializeRoster(program,{battle:world.battle,spawning}))throw Error('native initial AI roster is invalid');
  const journal=new AiJournalFacts(),facts=new AiPhaseFactsMirror(),cameras=[];
  journal.cursor=world.events?.seq??0;
  const cameraRows=Array.from({length:setup.settings.capacity.cameras},()=>new AiPhaseCameraMirror());
  const offline=world.camera?[world.camera]:[];
  const api={native:true,mode,enhanced,owner,worldData:world,spawning,nav:{boundary:boundaryFromNav(data.nav)},
    activate(){
      // UnitFactory's inactive hero/player stock finishes after the initial
      // roster. Bind its newly admitted definitions before any native bot
      // can inspect that arsenal; the running phase never loads a class.
      const late=new AiWorldContentMirror();
      for(const side of Object.values(world.factory.sides))for(const [name,def] of Object.entries(side.units??{})) {
        if(world.factory.prepared(name)&&!preparedClasses.has(def))late.classes.push(classFacts(def));
      }
      late.items=b.defs.item.list.slice(itemCount).map(itemFacts);
      if(late.classes.length||late.items.length)owner.content(late);
      owner.activate();
    },
    prepare(dt,tick,unitPhaseFollows=false){
      facts.dt=dt;facts.tick=tick;facts.networkViews=Boolean(world.networkViews);facts.lodDisabled=Boolean(world.lodDisabled||world.network);
      const views=world.cameras??offline;const count=Math.min(views.length,cameraRows.length);cameras.length=count;
      for(let index=0;index<count;index++) {
        const source=views[index],row=cameraRows[index];row.elements=source.matrixWorld;row.fov=source.fov;row.aspect=source.aspect;
        row.near=source.near;row.far=source.far;row.zoom=source.zoom;cameras[index]=row;
      }
      const reticle=facts.reticle,pilot=world.reticlePilot?.()??null,target=world.reticleTarget?.()??null;
      reticle.pilot=pilot;reticle.pilotLife=pilot?.aiTarget?.generation??0;reticle.target=target;reticle.targetLife=target?.generation??0;
      owner.prepare(facts,cameras,world.events?journal.capture(world.events):journal,world.time??0,false,false,unitPhaseFollows);
    },
    update(dt,tick,unitPhaseFollows=false){
      this.prepare(dt,tick,unitPhaseFollows);
      if(owner.x.ai_world_phase())owner.complete();
    },
    dangers(){owner.dangers();},
    playerCommand(kind,character=world.battle.human){return owner.playerCommand(character,kind,world.time??0);},
    humanReady(character,choice={}){return readyCharacter(character,choice);},
    awaitingHuman(character){return spawning.awaitingHuman(character);},
    // Native F4's retarget hook already refreshes living curves before its
    // presentation notice. This reader records the page's current side only.
    presentHumanTeam(team){world.playerTeam=team;},
  };
  return api;
}
