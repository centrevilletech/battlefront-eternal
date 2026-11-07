import { FixedClock } from '../src/runtime/fixed-clock.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { Bone, BoxGeometry, Matrix4, Vector3 } from 'three';
import { Battle, Character } from '../src/battle.js';
import { CommandPosts } from '../src/commandposts.js';
import { ClassicRandom, uniformRandom } from '../src/ai/random.js';
import { F4Live } from '../src/f4-live.js';
import { WorldTick } from '../src/runtime/world-tick.js';
import { F4Tail } from '../src/f4-tail.js';
import { hullFacts,propFacts } from '../src/ai-world-content.js';
import { ControlFacades } from '../src/ai-world-controls.js';
import { AiWorld, ControllerFacades } from '../src/ai-world.js';
import { AiWorldSetupMirror, AiWorldContentMirror, AiUnitClassMirror, AiPostDefinitionMirror, AiPhaseFactsMirror } from '../src/sim/gen/ai-phase.js';
import { HumanSpawnMirror, SpawningMirror, SpawnTeamMirror, SpawnClassMirror, SpawnMirror, F3PhaseInputLayout, F3UnitKeyLayout, F3PublicationLayout, F3NativeIntentLayout, BlockHeaderLayout } from '../src/sim/gen/index.js';
import { f3World } from './fixtures/f3-world.js';
import { fleetFixture } from './fixtures/f3-fleet.js';
import { weaponDefinition } from './fixtures/client-weapons.js';
import { building,trap } from './fixtures/f3-building.js';
import { turretAt } from './fixtures/f3-turret.js';
import { TurretSeats } from '../src/seats.js';

// Complete synthetic rider rig: ten capsules require both arms and legs.
// Its constant nine-pose makes boarding geometry independent of game files.
function prepareRider(unit) {
  const joints=[['DummyRoot',null,[0,0,0]],['bone_root','DummyRoot',[0,1,0]],
    ['bone_pelvis','bone_root',[0,0,0]],['bone_neck','bone_root',[0,.55,0]],['bone_head','bone_neck',[0,.12,0]]];
  for(const [side,x] of [['l',.1],['r',-.1]])joints.push(
    [`bone_${side}_thigh`,'bone_pelvis',[x,-.1,0]],
    [`bone_${side}_calf`,`bone_${side}_thigh`,[0,-.42,0]],
    [`bone_${side}_foot`,`bone_${side}_calf`,[0,-.35,0]],
    [`bone_${side}_upperarm`,'bone_neck',[x,-.05,0]],
    [`bone_${side}_forearm`,`bone_${side}_upperarm`,[.25,0,0]],
    [`bone_${side}_hand`,`bone_${side}_forearm`,[.25,0,0]]);
  const bones=joints.map(([name,,position])=>{const b=new Bone();b.name=name;b.position.fromArray(position);return b;});
  for(let i=0;i<bones.length;i++)(joints[i][1]?bones[joints.findIndex(j=>j[0]===joints[i][1])]:unit.actor.root).add(bones[i]);
  unit.actor.bones=bones;unit.actor.byName=Object.fromEntries(bones.map(b=>[b.name,b]));
  const data=new Float32Array(bones.length*9*7);
  for(let i=0;i<bones.length;i++)for(let frame=0;frame<9;frame++)data.set([0,0,0,1,...joints[i][2]],(i*9+frame)*7);
  const clip={name:'human_fixture_9pose',fps:30,frames:9,duration:8/30,rootMotion:false,tracks:bones.map((_,i)=>i),data};
  Object.assign(unit.animator,{bones,clips:new Map([[clip.name,clip]]),rootBind:new Vector3(),
    bindLengths:bones.filter(b=>!['DummyRoot','bone_root'].includes(b.name)).map(b=>[b,b.position.clone()])});
}

// Exercise the actual native host, including the F3/F4 owner loans. The
// fixture has no JS controller, navigation, population or geometry services.
for (const enhanced of [false,true]) for (const lane of ['navigation','combat','vehicle','boarding','flight','mounted-combat','turret-combat',...(enhanced?[]:['mounted-sniper','mounted-secondary']),'repair','trap-repair']) test(`one native ${enhanced?'Enhanced':'Classic'} ${lane} world tick and immediate death`,async()=>{
  const turretLane=lane==='turret-combat',sniper=lane==='mounted-sniper',secondary=lane==='mounted-secondary',armed=lane==='mounted-combat'||sniper||secondary||turretLane,combat=lane==='combat'||armed,mounted=lane==='vehicle'||lane==='boarding'||lane==='flight'||armed,repair=lane==='repair'||lane==='trap-repair';
  const posts=new CommandPosts(null,{commandPosts:[1,2].map((team,index)=>({name:`post${index}`,team,
    matrix:new Matrix4().makeTranslation(index*20,0,0).toArray(),hologram:[0,0,0]}))},{headless:true,playerTeam:1});
  posts.posts[0].ai={spawnPath:{present:true,points:[[0,0,0,0]]},allyPath:null,allyCount:0,classSpawnPoints:0,classSpawnWorld:[],yaw:0};
  const battle=new Battle({posts,teams:{1:{reinforcements:100},2:{reinforcements:100}},own:1,foe:2});
  const character=new Character(0,1,false);battle.characters.push(character);
  const gun=weaponDefinition('native-rifle',0,{...(sniper?{YawSpread:0,TriggerSingle:1,ZoomMax:6}:{}),ordnance:{class:'native-bolt',base:'bolt',Velocity:100,LifeSpan:1,Damage:2}});
  const lateGun=weaponDefinition('native-late-rifle',0,{ordnance:{class:'native-late-bolt',base:'bolt',Velocity:100,LifeSpan:1,Damage:2}});
  // SeatUnit.initialize assigns missing channels by seat weapon index. The
  // primary tool cannot attack this enemy, so the actual bot must shoot its
  // secondary gun and preserve that channel in the native launch notice.
  if(secondary){gun.base='repair';gun.ordnance=null;delete lateGun.channel;}
  const tool=weaponDefinition('native-tool',0,{base:'repair',BuildingRebuild:10,BuildingBuild:10,ordnance:null});
  const soldier={class:'soldier',base:'soldier',UnitType:repair?'pilot':'trooper',MaxHealth:100,MaxSpeed:7,weapons:combat?[gun,lateGun]:repair?[tool]:[],chunks:combat&&!mounted?[{}]:[],ChunkFrequency:1};
  const wreck=repair?(lane==='trap-repair'?trap('unbuilt-trap',0,30):building('wreck',0,30)):null;
  const emplacement=turretLane?turretAt({settings:{Pilot9Pose:'fixture_9pose'}}):null;let crews;
  const targetClass={class:'target',base:'soldier',MaxHealth:100,weapons:[]};
  const world=await f3World({soldier,target:targetClass},{debug:false,
    // Avoidance is the movement solver's wall-contact normal. A real wall
    // exercises its native publication; nearby bodies use separation instead.
    prepareCollision:lane==='navigation'?collision=>{
      const box=new BoxGeometry(1,6,200).translate(4,3,0);
      const body=collision.addBody(box.attributes.position.array,box.index.array,new Uint32Array(box.index.count/3));body.enabled=true;
    }:null,prepareFleet:mounted?fleetFixture({positions:turretLane?[]:[[3,0]],base:lane==='flight'?'flyer':'hover',weapon:secondary?[gun,lateGun]:armed?gun:null,exposed:true}):null,prepareLive:(collision,units,context)=>{
    const live={buildings:emplacement?[emplacement]:wreck?[wreck]:[],turrets:emplacement?[emplacement]:[],animated:[],traps:lane==='trap-repair'?[wreck]:[]};
    if(emplacement){crews=new TurretSeats({live,collision,units,ordnance:context.ordnance});context.vehicles.f2.seats.attachTurrets(crews);}
    return live;
  },prepareUnit:unit=>{if(mounted)prepareRider(unit);unit.character=unit.className==='soldier'?character:null;unit.team=unit.className==='soldier'?1:2;}});
  const unit=world.spawn('soldier',0,0);character.unit=unit;character.className='soldier';
  const chunksRandom=new ClassicRandom(997);unit.combat.random=uniformRandom(chunksRandom);
  const target=combat?world.spawn('target',0,15,true):null;
  if(target) {target.combat.team=2;world.phase.controller(target,null);}
  let launches=0,secondaryLaunches=0;world.ordnance.onLaunch=(projectile,notice)=>{launches++;if((notice?.sample?.channel??projectile.networkFireChannel)===1)secondaryLaunches++;};
  // Supply cold Classic spawn rows, so death follows the production native
  // spawn/objective/AI hook order rather than the non-Classic scoreboard path.
  const spawning=new SpawningMirror();
  Object.assign(spawning,{A:15,B:.5,Q:30,waveCountdown:15,wave:0,stagger:0,released:0,localsCap:0,characters:battle.characters,started:true});
  for(let index=0;index<8;index++) {
    const team=new SpawnTeamMirror();
    Object.assign(team,{index,faction:index===1?3:index===2?4:0,unitCount:index===1?1:0,tickets:100,heroCount:0,bonus:0,aggressiveness:0,living:index===1?1:0,capacity:200});
    for(let other=0;other<8;other++)team.relations[other]=index===other?1:-1;
    if(index===1) {team.characters.push(character);const type=new SpawnClassMirror();type.name='soldier';type.target=1;team.classes.push(type);}
    spawning.teams.push(team);
  }
  character.spawn=new SpawnMirror();character.spawn.timer=0;character.classIndex=0;
  new F4Live(world.phase,{sound(){}});
  if(wreck&&lane!=='trap-repair')wreck.damage(Infinity);
  new F4Tail(world.phase,battle,{posts,spawning,human:new HumanSpawnMirror(),gameplay:world.ordnance.random});
  // f3World's general movement fixture starts with a placeholder controller.
  // Remove that explicit fixture command before the native AI owns its slot.
  world.phase.controller(unit,null);
  // Spawning.begin normally assigns the cold objective before ClassicAI
  // attaches. This fixture starts with a placed body and supplies that state.
  world.phase.tailOwner.objective(character,posts.posts[1]);
  const random=new ClassicRandom(81),randomB=new ClassicRandom(81^0x9e3779b9);
  const setup=new AiWorldSetupMirror(),settings=setup.settings;
  Object.assign(setup.navigation.pools,{paths:900,pathNodes:7200,requests:75});
  Object.assign(settings,{seed:81,streamA:world.phase.random.register(uniformRandom(random)),streamB:world.phase.random.register(uniformRandom(randomB)),
    enhanced,tier:3,profileTier:3,playerTeam:1,playerSkill:4,enemySkill:14,sensorRange:75,lodDisabled:true});
  Object.assign(settings.capacity,{registry:256,characters:8,posts:2,hulls:8,controls:16,scopes:16,cameras:2,grenades:64,supportKeys:8,repairJobs:16});
  Object.assign(settings.capabilities,{navigation:true,population:true,randomB:true,combat,combatAgent:combat,classes:combat||repair,vehicles:mounted,vehicleAdapter:mounted});
  const owner=new AiWorld(world.phase,setup),content=new AiWorldContentMirror(),policy=new AiUnitClassMirror();
  Object.assign(policy,{def:soldier,name:'soldier',sizeType:'SOLDIER',maxSpeed:7,forwardSpeed:7,maxHealth:100,base:'soldier'});
  policy.unitType=soldier.UnitType;
  content.classes.push(policy);
  if(combat) {
    // The second canonical weapon is deliberately absent from the initial
    // host content snapshot, like inactive hero stock prepared afterwards.
    content.weapons.push(gun);
    const targetPolicy=new AiUnitClassMirror();Object.assign(targetPolicy,{def:targetClass,name:'target',sizeType:'SOLDIER',maxSpeed:7,forwardSpeed:7,maxHealth:100,base:'soldier'});
    content.classes.push(targetPolicy);
  }
  if(emplacement){content.weapons.push(emplacement.weapon.def);content.props.push(propFacts({object:emplacement,entry:{handle:owner.propOwner(0)}}));}
  if(wreck){content.weapons.push(tool);content.props.push(propFacts({object:wreck,entry:{handle:owner.propOwner(0)}}));}
  for (const post of posts.posts) {
    const row=new AiPostDefinitionMirror();
    Object.assign(row.policy,{post,attack:[100,100,100,100,100,100],defend:[100,100,100,100,100,100],spawn:post.team===1,ally:false});
    Object.assign(row,{name:post.desc.name,valueBleed:100,spawnPresent:post.team===1?true:null,spawnPath:post.team===1,spawnPoints:post.team===1?1:0});content.posts.push(row);
  }
  let hull=null;
  if(mounted&&!turretLane) {
    hull=world.factory.context.vehicles.vehicles[0];
    content.hulls.push(hullFacts(hull));
  }
  owner.content(content);
  // The foreign body can retain an earlier rider intent. Zero words in the
  // new native record must replace it even when the numeric cache was zero.
  unit.intent.forward=.625;unit.intent.right=-.375;
  owner.activate();
  assert.equal(unit.intent.forward,0,'native binding restores an unchanged zero axis');
  assert.equal(unit.intent.right,0,'native binding replaces the foreign held row');
  assert.equal(character.controller.native,true);assert.equal(unit.controller,character.controller);
  if(mounted&&lane!=='boarding') {
    // Native AI boarding never enters F2Riders.ensure. Suppress the external
    // fixture command's convenience import so only cold stock can supply it.
    world.factory.context.vehicles.f2.riders.ensure=()=>{};
    assert.equal(turretLane?crews.man(unit,emplacement):world.factory.context.vehicles.board(unit,hull),true);
    assert.equal(unit.seat.controllable.native,true);
  }
  world.phase.registerViewpoint(unit);
  const coordinator=new WorldTick(world.phase,new FixedClock());
  const facts=new AiPhaseFactsMirror();facts.dt=1/60;facts.lodDisabled=true;
  const cameras=[],journal={sequence:0,records:[]};
  let allocations,reallocations,sawContact=false,sawIntent=false,sawMountedControl=false,sawScan=false,checkedRider=false,sawSniperSpread=false,sawDeferredIntent=false;
  const initialRandom=random.state,heldCounters=owner.counters;
  for(let tick=1;tick<=360;tick++) {
    const beforeRows=world.phase.counters.unitRows,forward=unit.intent.forward,right=unit.intent.right;
    facts.tick=tick;
    owner.prepare(facts,cameras,journal,tick/60,false,false,true);
    const aiRows=world.phase.counters.unitRows-beforeRows;
    if(!mounted&&!repair)assert.equal(aiRows,0,'ordinary bot controls, avoidance and tuning join the unit phase');
    if(owner.counters.scanned>0)sawScan=true;
    if(unit.intent.forward!==0 || unit.intent.right!==0)sawIntent=true;
    if(mounted && unit.seat?.controllable?.controller===character.controller)sawMountedControl=true;
    const writeFacts = world.phase.registry.foreign;
    if (tick===1) world.phase.registry.foreign=(count,networks)=>{
      const phase=world.phase,b=phase.bridge;b.views();let native=0,foreign=0;
      for(let i=0,at=phase.layout.input/8;i<count;i++,at+=phase.layout.inputStride) {
        const slot=b.f[at+F3PhaseInputLayout.key+F3UnitKeyLayout.slot],binding=phase.bindings[slot];
        assert.equal(b.f[at+F3PhaseInputLayout.intent]!==0,binding.nativeIntent,'only canonical AI bodies omit foreign intent');
        if(binding.nativeIntent)native++;else foreign++;
      }
      if(!mounted) {
        assert.ok(phase.bindings[unit.id].nativeIntent);
        assert.equal(native,0,'canonical bots need no foreign body row');
      }
      if(target)assert.ok(foreign>0,'unbound target still carries explicit foreign input');
      return writeFacts.call(world.phase.registry,count,networks);
    };
    coordinator.run(1/60,tick,true,null,true,true,0,false,0);coordinator.finish();coordinator.complete();
    world.phase.registry.foreign=writeFacts;
    if(lane==='navigation'&&unit.soldier.avoidance.lengthSq()>0)sawContact=true;
    if(tick===1&&!mounted) {
      const binding=world.phase.bindings[unit.id],words=binding.words;
      const row=binding.wordsAt+BlockHeaderLayout.words;
      assert.equal(words[row+F3PublicationLayout.controls],1,'native latch and movement input remain in Rust');
      const held=row+F3PublicationLayout.controls;
      assert.equal(words[held],1,'held intent remains observable after the phase');
      assert.equal(words[held+1+F3NativeIntentLayout.forward],unit.intent.forward);
      if(target) {
        const other=world.phase.bindings[target.id],at=other.wordsAt+BlockHeaderLayout.words;
        assert.equal(other.words[at+F3PublicationLayout.controls],0,'foreign player controls keep full restoration');
      }
    }
    {
      if(unit.intent.forward!==0||unit.intent.right!==0)sawIntent=true;
      if(aiRows===0&&(unit.intent.forward!==forward||unit.intent.right!==right))sawDeferredIntent=true;
    }
    if(sniper&&(hull.seats[0].weapons[0].state.yawSpreadOverride??0)>0)sawSniperSpread=true;
    if(mounted&&unit.seat&&!checkedRider) {
      assert.equal(unit.hitShapes().length,10,'prepared rider capsules follow native activation and lease life');checkedRider=true;
    }
    // A repair job at this neutral wreck wins before objective offset or
    // graph routing can draw A. Missing the cold candidate changes the stream.
    if(repair && tick===16)assert.equal(random.state,initialRandom);
    if(tick===180){allocations=owner.counters.allocations;reallocations=owner.counters.reallocations;}
  }
  if(turretLane) {
    assert.equal(owner.counters.allocations,0,'first-use turret controls retain their prepared native storage');
    assert.equal(owner.counters.reallocations,0,'mount publication marks use the active seat owner');
  }
  if(sniper)assert.ok(sawSniperSpread,'native sniper yaw spread survives mounted input before firing');
  if(secondary)assert.ok(secondaryLaunches>0,'native secondary shots retain their seat channel without mirrored input');
  assert.equal(owner.counters.phases,360);assert.equal(heldCounters.phases,360,'retained diagnostic views stay current');assert.ok(owner.counters.drawsA>0);
  assert.equal(owner.counters.stepDt,1/60);assert.equal(owner.counters.liveControllers,1);assert.ok(Number.isFinite(owner.counters.schedulerTicks));assert.equal(owner.counters.time,6);
  assert.ok(owner.counters.serviced>0,JSON.stringify(owner.counters));
  assert.equal(owner.counters.allocations,allocations);assert.equal(owner.counters.reallocations,reallocations);
  if(combat) {
    if(mounted)assert.ok(sawScan,'the AI scans through the mounted owner while Store is loaned out of Sim');
    else assert.ok(launches>0,JSON.stringify({reason:'post-movement native aim and shared A produce real native shots',team:unit.team,targetTeam:target.team,position:unit.soldier.position,targetPosition:target.soldier.position,intent:unit.intent,counters:owner.counters}));
    if(mounted&&!enhanced)assert.notEqual((turretLane?emplacement.weapon:hull.seats[0].weapons[secondary?1:0].state).spreadScale,1,
      'native Aimer spread curves survive retained mounted input before firing');
    if(!mounted)assert.ok(unit.intent.aim,'native aim publishes its retained vector');
    if(enhanced&&!mounted)assert.ok(owner.counters.drawsC>0,'Enhanced aim consumes private C');
    // Grenade warnings run after the same battle tail as production; no
    // separate host danger entry can move their observation before physics.
    world.ordnance.fire({class:'native-warning',base:'grenade',Velocity:0,LifeSpan:20},new Vector3(0,100,0),new Vector3(0,0,1),unit);
    const dangers=owner.counters.dangerPhases;
    facts.tick=361;owner.prepare(facts,cameras,journal,361/60,false,false,true);
    coordinator.run(1/60,361,true,null,true,true,0,false,0);coordinator.finish();coordinator.complete();
    assert.equal(owner.counters.dangerPhases,dangers+1);
  }else if(repair) {
    assert.ok(sawIntent,'the repair job moves toward its native wreck');
  }else if(mounted) {
    assert.ok(hull.position.x!==3 || hull.position.y!==0 || hull.position.z!==0,'native seat steering moves the canonical hull');
    assert.ok(sawMountedControl,'the mounted control publishes its current qualified controller');
    assert.ok(checkedRider,'native boarding activates the prepared rider sampler');
  }else {
    assert.ok(sawIntent,'native held intent reaches the unit owner');
    assert.ok(sawContact,'native movement supplies actual wall contact');
    assert.ok(sawDeferredIntent,'the following unit phase consumes changed intent without an intermediate body publication');
    assert.ok(unit.soldier.position.lengthSq()>0,'native intent survives the foreign input boundary and moves the body');
    assert.equal(owner.playerCommand(character,2,6),true);
    const stale={},handle=world.phase.tailOwner.entries.get(character).handle;
    world.phase.tailOwner.entries.set(stale,{handle:{...handle,life:handle.life+1}});
    const before=random.state;assert.equal(owner.playerCommand(stale,2,6),false);
    assert.equal(random.state,before,'a stale Character command consumes no draw');
    assert.equal(owner.counters.commands,2);assert.equal(owner.counters.staleCommands,1);
  }
  const seated=mounted&&Boolean(unit.seat);
  const drop=seated?world.factory.context.vehicles.f2.riders.drop(unit,new Vector3())?.clone():null;
  const controller=character.controller;
  const chunkReference=new ClassicRandom(chunksRandom.state);
  if(combat&&!mounted)chunkReference.uniform();
  unit.combat.damage(Infinity);
  assert.equal(chunksRandom.state,chunkReference.state,'bot weapon binding preserves the separate combat chunks generator');
  assert.equal(unit.controller,null);assert.equal(character.controller,controller);
  assert.equal(unit.combat.dead,true);
  if(seated) {
    assert.ok(drop,'the native seat sample supplies a death drop point');
    assert.ok(unit.soldier.position.distanceTo(drop)<1e-9,'death starts at the completed rider sample');
  }
});


test('native controller facades preserve retired storage generations',()=>{
  const refs=new ControllerFacades(4),row=new Float64Array(2);
  refs.born({slot:0,generation:1,index:0});const first=refs.at(0,1);
  refs.born({slot:0,generation:2,index:1});const replacement=refs.at(0,2);
  assert.notEqual(first,replacement);assert.equal(refs.at(0,1),first);
  refs.put(first,row,0);assert.deepEqual([...row],[0,1]);
  assert.throws(()=>refs.at(1,1),/precedes its native birth/);
  refs.born({slot:0,generation:3,index:4});assert.equal(refs.at(0,3).index,4);
  assert.equal(refs.at(0,1),first);
});


test('a native seat reader retains its lease and captured occupant across storage reuse',()=>{
  const refs=new ControlFacades(),first=refs.born({slot:0,generation:1});
  assert.ok(first.intent);
  const vehicle={def:{settings:{ForwardSpeed:12}},base:'hover',name:'tank',alive:true,maxHealth:100,speed:12,turnRate:1,
    bounds:{min:new Vector3(-1,0,-1),max:new Vector3(1,2,1)},size:new Vector3(2,2,2),seats:[{weapons:[]}]};
  const occupant={id:7,hero:true},lease={vehicle,index:0,unit:occupant,team:1,live:true};
  first.initialize(lease);first.nativeInitialized=true;
  assert.equal(first.alive,true);assert.equal(first.hero,true);assert.equal(first.id,7);
  first.controller={};const next=refs.born({slot:0,generation:2});
  assert.notEqual(next,first);assert.equal(first.controller,null);assert.equal(first.lease,lease);
  occupant.hero=false;lease.live=false;assert.equal(first.hero,true);assert.equal(first.alive,false);
  assert.equal(refs.at(0,1),first);assert.equal(refs.at(0,2),next);
  assert.equal(refs.born({slot:0,generation:2}),next);
  const row=new Float64Array(2);refs.put(first,row,0);assert.deepEqual([...row],[0,1]);
});

// Repeated Core indices must not become an ever-growing JS frame array.
// Retired generations are readable only while their publication is draining.
test('native facade storage stays bounded across thousands of reused generations',()=>{
  for(const refs of [new ControllerFacades(2),new ControlFacades()]) {
    let held;
    for(let generation=1;generation<=5000;generation++) {
      const current=refs.born({slot:0,generation,index:generation*17});
      assert.equal(refs.at(0,generation),current);
      if(held){assert.equal(refs.at(0,generation-1),held);assert.equal(refs.born({slot:0,generation:generation-1,index:0}),held);}
      refs.finishPublication();
      assert.equal(refs.objects.length<=2,true);assert.equal(refs.displaced.size,0);
      if(held){assert.throws(()=>refs.at(0,generation-1),/retired generation/);assert.equal(held.simGen,generation-1);assert.equal(refs.born({slot:0,generation:generation-1,index:0}),null);}
      held=current;
    }
    assert.equal(refs.frames,undefined);assert.equal(refs.lives,undefined);
  }
});
