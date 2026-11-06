// The battle-tail boundary follows items. Posts, statistics and spawn clocks
// stay native; AI sees touched mirrors before returning scalar commands.
import { isCommandClass,MobilePost } from './mobile-posts.js';
import { F4Battle } from './f4-battle.js';
import { uniformRandom } from './ai/random.js';
import * as F from './sim/gen/f4.js';
import * as S from './sim/gen/index.js';
const record = name=>({words:F[`${name}Layout`].words,put:F[`put${name}`]});
const SETUP=record('F4TailSetup'),CLASS=record('F4UnitClass'),WEAPON=record('F4WeaponClass'),HULL=record('F4HullClass');
const READY=record('F4HumanReady'),SPAWN=record('F4UnitSpawn');
const PAD=record('F4PadSetup'),POST=record('F4PostSetup'),HERO=record('F4HeroSetup'),CAPTURE=record('F4CaptureTeam');
const label=value=>{const s=String(value??'').trim().replace(/\n/g,' ');return s||null;};
const EMPTY_COMMANDS=Object.freeze([]);
export class F4Tail extends F4Battle {
  constructor(phase,battle,{posts,spawning=null,human,boundary={polygons:[]},local=null,character=null,
    random=null,gameplay=null,enhanced=false,rosters=[],classes=[],paths={},pads=null,heroNames=[],captureTeams=[],
    commandCapacity=16,pointCapacity=0,spawnIndex=0,onPost=null,onMobile=null,onOver=null,onSpawn=null,onHuman=null,
    retarget=null,rejected=null,ai=null,debug=false}={}) {
    phase.bridge.identify({classes:{Post:posts.posts[0]?.constructor,MobilePost}});
    super(phase,battle,{debug});
    this.posts=new WeakMap();this.weapons=new Set();this.commands=new S.HsCommandMirror();this.input=new F.F4TailInputMirror();
    this.ready=new F.F4HumanReadyMirror();this.spawnCommand=new F.F4UnitSpawnMirror();
    this.commandCapacity=commandCapacity;this.spawning=spawning;this.human=human;this.ai=ai;
    this.onPost=onPost;this.onMobile=onMobile;this.onOver=onOver;this.onSpawn=onSpawn;this.onHuman=onHuman;
    this.retarget=retarget;this.rejected=rejected;
    this.postEvent={post:null,kind:'lost',from:undefined,by:undefined,destroyed:undefined};
    this.mobileEvent={kind:'lost',post:null,vehicle:null,from:0,to:undefined};
    const b=this.bridge,x=this.x,live=phase.liveOwner;
    const mobile=posts.mobile??{posts:[]};
    b.write(S.KIND_POST,posts.posts,true);b.write(S.KIND_COMMAND_POSTS,[posts],true);
    b.write(S.KIND_MOBILE_POST,mobile.posts,true);b.write(S.KIND_MOBILE_POSTS,[mobile],true);
    if(spawning)b.write(S.KIND_SPAWNING,[spawning],true);
    b.write(S.KIND_HUMAN_SPAWN,[human],true);
    const fleet=phase.factory.context.vehicles;
    const count=Math.max(phase.construction?.limit??0,phase.factory.context.units.nextId+phase.factory.built.length);
    pads??=preparedPads(fleet,posts);
    for(const pad of pads){
      if(!pad)continue;
      for(const hull of pad.variants)pointCapacity=Math.max(pointCapacity,hull.record?.points.length??0);
      pointCapacity=Math.max(pointCapacity,pad.vehicle?.record?.points.length??0);
    }
    b.callArgs(SETUP,{local,character,classic:Boolean(spawning),enhanced:enhanced||Boolean(ai?.enhanced),registryCapacity:Math.max(64,count),victimCapacity:Math.max(64,count+(fleet?.vehicles.length??0)+(fleet?.f2?.prepared.length??0)),
      commandCapacity,pointCapacity,spawnIndex,fallbackPath:Object.values(paths)[0]??[[0,10,0,0]],random:phase.random.register(random?uniformRandom(random):null),gameplay:phase.random.register(gameplay),
      boundary:{polygons:boundary.polygons.map(edges=>({edges:edges.map(e=>[e.x,e.z,e.nx,e.nz])}))},rosters:rosters.map(classes=>({classes})),classes});
    x.f4_tail_init();this.commandInput=x.f4_tail_commands()/8;
    for(const def of live.classes){
      this.send(CLASS,{def,captures:(def.CapturePosts??1)!==0,vehicle:String(def.HealthType??'').toLowerCase()==='vehicle',
        healthEffect:def.HealthGainEffect||'Powerup_blue',ammoEffect:def.AmmoGainEffect||'Powerup_red',label:label(def.displayName)},'f4_tail_unit_class');
      for(const weapon of def.weapons??[])this.prepareWeapon({levels:[weapon,...(weapon.charges??[]).map(level=>({...weapon,...level}))],channel:weapon.channel,name:weapon.class});
    }
    for(const unit of phase.factory.built)for(const weapon of unit.combat.arsenal.weapons)this.prepareWeapon(weapon);
    if(local)for(const weapon of local.combat.arsenal.weapons)this.prepareWeapon(weapon);
    for(const hull of fleet?.vehicles??[])this.prepareHull(hull);
    for(const hull of fleet?.f2?.prepared??[])this.prepareHull(hull);
    for(const turret of phase.factory.context.live?.turrets??[])if(turret.weapon)this.prepareWeapon(turret.weapon);
    for(const post of posts.posts)this.send(POST,{post,rawPath:paths[post.desc.spawnPath]??null},'f4_tail_post');
    for(let index=0;index<pads.length;index++)if(pads[index])this.send(PAD,{index,pad:pads[index]},'f4_tail_pad');
    for(const entry of heroNames)this.send(HERO,entry,'f4_tail_hero');
    for(let team=0;team<captureTeams.length;team++)this.send(CAPTURE,{team,value:captureTeams[team]},'f4_tail_capture_team');
    this.handlers();x.f4_tail_ready();
    this.bindRows('post',posts.posts,S.POST_EXPORT_STRIDE,S.putPost,S.syncPost,S.checkPost);
    this.bindRows('mobilePost',mobile.posts,S.MOBILE_POST_EXPORT_STRIDE,S.putMobilePost,S.syncMobilePost,S.checkMobilePost);
    this.bindRows('human',[human],S.HUMAN_SPAWN_EXPORT_STRIDE,S.putHumanSpawn,S.syncHumanSpawn,S.checkHumanSpawn);
    this.bindRows('spawning',[spawning??new S.SpawningMirror()],S.SPAWNING_EXPORT_STRIDE,S.putSpawning,S.syncSpawning,S.checkSpawning);
    phase.sync();
  }
  // Stub/planning code can request a body outside the tail, using the same
  // native lifecycle and qualified roster/post handles as scheduled waves.
  spawnUnit(character,className,post,point,yaw=0) {
    const p=this.phase,b=this.bridge,row=this.spawnCommand,active=p.active;
    row.target=this.entries.get(character).handle;row.post=this.posts.get(post).handle;row.className=className;
    row.position.set(point[0],point[1],point[2]);row.yaw=yaw;
    p.random.push();p.active=true;let slot;
    try {b.callArgs(SPAWN,row);slot=this.x.f4_unit_spawn();}finally{p.active=active;}
    p.random.pull();p.sync();return slot<0?null:p.bindings[slot].unit;
  }
  readyHuman(character,choice={}){
    const p=this.phase,b=this.bridge,row=this.ready,c=this.counters,previous=b.phaseCounters,active=p.active;
    if(this.debug)this.mirrors.check();
    row.target=this.entries.get(character).handle;row.className=choice.className??null;row.classIndex=choice.classIndex??null;
    row.suppliedPost=choice.post!==undefined;row.post=choice.post?this.posts.get(choice.post).handle:null;
    const output=this.mirrors.words,unitWords=p.counters.outputFields;
    p.random.push();b.phaseCounters=c;p.active=true;c.wasmCalls++;c.commandCalls++;c.inputFields+=READY.words;
    let accepted;
    try{b.callArgs(READY,row);accepted=this.x.f4_human_ready();}finally{b.phaseCounters=previous;p.active=active;}
    p.random.pull();p.sync();c.outputFields+=this.mirrors.words-output+p.counters.outputFields-unitWords;
    return Boolean(accepted);
  }
  send(record,value,method){this.bridge.callArgs(record,value);return this.x[method]();}
  prepareWeapon(weapon){
    const def=weapon.levels[0];if(this.weapons.has(def))return;this.weapons.add(def);
    this.send(WEAPON,{def,ammunition:weapon.levels.map(level=>Number(level.ammo)),primary:weapon.channel===0,key:weapon.name??def.class??null,
      labels:weapon.levels.map(level=>label(level.displayName??level.label))},'f4_tail_weapon_class');
  }
  prepareHull(hull){
    this.send(HULL,{hull,side:hull.side??null,label:label(hull.label)},'f4_tail_hull_class');
    for(const seat of hull.seats??[])for(const gun of seat.weapons??[])if(gun.state)this.prepareWeapon(gun.state);
  }
  bindRows(kind,rows,words,put,sync,check){
    const id=F.F4ObjectKind_INDEX.get(kind),live=this.phase.liveOwner;
    for(let i=0;i<rows.length;i++){
      const handle=live.handle(this.x.f4_tail_handle(id,i));
      const entry=this.mirrors.bind(handle,{block:true,capacity:this.x.f4_tail_capacity(id,i),words,put,sync,check},rows[i]);
      if(kind==='post'||kind==='mobilePost')this.posts.set(rows[i],entry);
    }
  }
  handlers(){
    const p=this.phase;
    this.bridge.register({f4AIHook:args=>{
      const character=args.characterHandle?this.ensureCharacter(args.characterHandle):null;
      p.random.pull();p.sync();
      try{
        switch(args.op){
          case 'requestObjective':this.spawning.objectives?.request(character);break;
          case 'republish':this.spawning.objectives?.republish(character);break;
          case 'spawned':this.ai?.spawned?.(character);break;
          case 'died':this.ai?.classic?.died?.(character);break;
          case 'attach':this.ai.classic.attach(character);break;
          case 'detach':this.ai.classic.detach(character);break;
          case 'post':{
            const e=this.postEvent,a=args.post;e.post=a.post;e.kind=a.kind;e.from=a.from;e.by=a.by;e.destroyed=a.destroyed;
            this.onPost?.(e);break;
          }
          case 'mobile':{
            const e=this.mobileEvent,a=args.mobile;e.post=a.post;e.kind=a.kind;e.vehicle=a.vehicle;e.from=a.from;e.to=a.to;
            this.onMobile?.(e);break;
          }
          case 'over':this.onOver?.(args.number);break;
          case 'retarget':this.retarget?.(args.number);break;
          case 'rejected':this.rejected?.(args.className,args.number);break;
        }
      }finally{p.random.push();}
    }});
    const events=p.eventHandlers,spawn=events.f4Spawn,human=events.f4Human;
    events.f4Spawn=event=>{spawn?.(event);this.onSpawn?.(event);};
    events.f4Human=event=>{human?.(event);this.onHuman?.(event);};
  }
  step(dt,tick,server,commands=EMPTY_COMMANDS){
    if(this.debug)this.mirrors.check();
    const p=this.phase,b=this.bridge,live=p.liveOwner,counters=this.counters,previous=b.phaseCounters,active=p.active;
    // A paused input source can deliver a burst. Keep the admitted prefix in
    // order; excess commands cannot overwrite the next shared input record.
    const count=Math.min(commands?.length??0,this.commandCapacity);
    b.views();
    for(let i=0;i<count;i++){
      const command=commands[i],row=this.commands;row.kind=command.kind;row.team=command.team??0;row.className=command.className??null;row.post=command.post??null;
      S.putHsCommand(b,row,b.f,this.commandInput+i*S.HsCommandLayout.words,b.heap);
    }
    const input=this.input;input.dt=dt;input.tick=tick;input.server=server;input.humanCommands=this.commandInput*8;input.humanCount=count;
    F.putF4TailInput(b,input,b.f,live.layout.tailInput/8,b.heap);
    const output=this.mirrors.words,unitWords=p.counters.outputFields,randomWords=p.random.entries.length*p.layout.randomWords;
    p.random.push();p.active=true;b.phaseCounters=counters;
    counters.ticks++;counters.wasmCalls++;counters.phaseCalls++;counters.inputFields+=F.F4TailInputLayout.words+count*S.HsCommandLayout.words+randomWords;
    try{this.x.f4_tail();}finally{p.active=active;b.phaseCounters=previous;}
    p.random.pull();p.sync();live.readCounters();
    counters.outputFields+=this.mirrors.words-output+p.counters.outputFields-unitWords+randomWords;
    counters.rustAllocations+=live.native.allocations;counters.rustReallocations+=live.native.reallocations;counters.rustAllocationBytes+=live.native.allocationBytes;
    if(this.debug)this.mirrors.check();
  }
}

// Command pad facts include prepared variants before their first activation.
// Only their native fleet membership and life determine which can be read.
function preparedPads(fleet,posts){
  const rows=[];
  for(const post of posts.mobile?.posts??[]){
    const variants=[],known=new Set();
    for(const hull of post.slot.variants?.values()??[]){variants.push(padHull(hull,post.classes));known.add(hull);}
    for(const hull of fleet?.f2?.prepared??[])if(hull.slot===post.slot&&!known.has(hull)){variants.push(padHull(hull,post.classes));known.add(hull);}
    if(post.slot.vehicle&&!known.has(post.slot.vehicle))variants.push(padHull(post.slot.vehicle,post.classes));
    rows[post.index]={variants,vehicle:post.slot.vehicle?padHull(post.slot.vehicle,post.classes):null};
  }
  return rows;
}
function padHull(v,classes){
  const r=classes[v.name];
  return {vehicle:v,alive:Boolean(v.alive),command:isCommandClass(v.def),generation:v.aiTarget?.generation??0,team:v.team|0,
    controllingTeam:v.controllingTeam|0,seated:Boolean(v.seats?.some(seat=>seat.lease&&!seat.lease.released)),
    yaw:v.yaw,name:v.name,label:v.label||v.name,radius:v.size.length()/2,speed:v.speed??0,valueBleed:Number.parseInt(v.def.settings?.ValueBleed,10),
    record:r?{atk:r.values?.atk??null,def:r.values?.def??null,valueBleed:r.valueBleed??null,
      points:(r.spawnPoints??[]).map(p=>[p[0]??0,p[1]??0,p[2]??0,p[3]??0])}:null};
}
