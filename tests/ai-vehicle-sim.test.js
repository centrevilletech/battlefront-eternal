import test from 'node:test';
import assert from 'node:assert/strict';
import { Matrix4, Quaternion, Sphere, Vector3 } from 'three';
import { loadSim } from '../src/sim/module.js';
import { attachAiSim } from '../src/ai-sim.js';
import { ClassicNav } from '../src/ai/classic/nav/index.js';
import { ClassicRandom } from '../src/ai/random.js';
import { classicBinding, VehicleAI } from '../src/ai/classic/vehicles/index.js';
import { UnitBoardAgent, UnitFlyAgent, UnitUnBoardAgent } from '../src/ai/classic/vehicles/agents.js';
const v=(x=0,y=0,z=0)=>new Vector3(x,y,z);
async function fixture(native=false,json=null,bin=null) {
  const nav=json ? new ClassicNav(json,bin):ClassicNav.empty('synthetic');
  const world={time:30,nav,battle:{reinforcements:{1:100,2:100}},posts:{posts:[]},units:{query:()=>[]}};
  const ai={world,nav,randomA:new ClassicRandom(95),relation:(a,b)=>a===b?1:-1};
  if(native){world.aiSim=attachAiSim(await loadSim());world.aiSim.bindNavigation(nav,json,bin);}
  const service=classicBinding(ai);ai.vehicles=service;
  const events=[];
  const unit={id:1,generation:1,kind:5,alive:true,team:1,position:v(0,20),velocity:v(),bodyYaw:0,upY:1,
    classDef:{VehicleType:'fighter',MaxSpeed:30,MinSpeed:10,LandingTime:5,LandingSpeed:3,LandedHeight:2,ForwardSpeed:20,StrafeSpeed:4},
    sphere:new Sphere(v(),2),origin:out=>out.copy(unit.position),forward:out=>out.set(0,0,1),intent:{},human:false};
  const hull={name:'synthetic',alive:true,aiTarget:unit,flying:true,walking:false,grounded:false,flightState:'Flying',gear:0,speed:30,
    flightConfig:{minSpeed:10,maxSpeed:30,cruiseSpeed:20,turnRate:2,trickCost:20},airspeed:30,flightClock:0,power:100,
    def:{label:'synthetic',settings:{MaxSpeed:'30',ForwardSpeed:'20'}},position:unit.position,velocity:unit.velocity,yaw:0,quaternion:new Quaternion(),matrix:new Matrix4(),
    bounds:{min:v(-1,-1,-2),max:v(1,1,2)},boundRadius:3,size:v(2,2,4),seats:[],occupants:1,freeSeats:2,occupied:true,controllingTeam:1,towTarget:-1,maxHealth:100};
  const character={unit};unit.character=character;
  const lease={released:false,human:false,live:true,unit,controllable:unit,vehicle:hull,index:0,team:1,origin:out=>out.copy(unit.position)};
  unit.vehicle=hull;unit.lease=lease;unit.seat=lease;hull.seats=[{lease,weapons:[],settings:{}}];
  const c={unit,generation:1,executorStatus:0,request:{valid:true},requestState:s=>events.push(['controllerState',s]),
    requestExit:()=>events.push(['exit']),requestObjective:e=>events.push(['objective',e]),isUnitController:true};
  unit.controller=c;character.controller=c;
  const system=world.vehicleSystem={vehicles:[hull],requestExit:(u,o)=>{events.push(['requestExit',o]);return true;},
    unboard:(u,o)=>{events.push(['unboard',u.id,o]);return true;},humanTarget:()=>null};hull.system=system;
  const h={position:v(10,0,20),forward:v(0,0,1),up:v(0,1,0),commandPost:{position:v(10,0,20),team:2}};h.link=h;
  world.posts.posts=[h.commandPost];
  const hintEvents=[];ai.nav.hints.reserve=(hint,owner,now)=>{hintEvents.push([hint===h,owner===c,now]);};
  const helper={arrived:false,failed:false,state:6,destination:v(),hasPath:false,
    goto:(p,o)=>events.push(['goto',p.toArray(),o]),stop:()=>events.push(['stop']),
    followPath:(points,over,alt)=>events.push(['path',points.map(p=>[...p]),over,alt]),land:on=>events.push(['land',on])};
  function agent(kind) {
    const Type=[UnitBoardAgent,UnitUnBoardAgent,UnitFlyAgent][kind],a=Object.create(Type.prototype);
    Object.assign(a,{ai,world,controller:c,unit,helper,state:1,pending:0,vehicle:null,hint:null,searchAt:-Infinity,flag:false,deadline:0,post:null,
      interrupts:{tick:()=>{events.push(['interrupt']);return false;}},updateCombatTarget:()=>events.push(['target']),aim:s=>events.push(['aim',s]),
      tickHelper:dt=>events.push(['tickHelper',dt]),canEngage:()=>true,now:()=>world.time,requestState(s){this.pending=s;}});
    c.agent=a;if(native)service.nativeVehicle.bindAgent(a,kind);return a;
  }
  return {ai,world,service,events,unit,hull,c,lease,h,helper,agent,hintEvents};
}

test('vehicle services preserve identity and nested reservation changes',async()=>{
  const f=await fixture(true),{service,unit,c,hull}=f;
  assert.equal(classicBinding(f.ai),service);assert.equal(f.world.vehicles,service);
  const reservations=service.reservations;
  const other={...unit,id:2,generation:2,seat:null,lease:null,vehicle:null};
  const target={...unit,id:3,generation:3};
  service.reserveSeat(c,unit);
  const record=reservations.records[0];
  service.reserveSeat(c,unit);assert.equal(reservations.records[0],record);
  f.world.vehicleSystem.board=()=>{
    assert.equal(reservations.has(unit,unit),true);
    reservations.clear();reservations.reserve(target,other,2);
    // Resize the shared call block during a callback.
    f.world.aiSim.words(new Array(25000).fill(0));
    return true;
  };
  assert.equal(service.boardNow(c,unit),true);
  assert.equal(service.reservations,reservations);
  assert.equal(reservations.count(target,other.team,2),1);
  assert.equal(reservations.has(unit,unit),false);
  assert.equal(service.currentControllable(unit.character),unit);
  assert.equal(hull.aiTarget,unit);
});

test('leaving and flyer landing states match source callbacks and writes',async()=>{
  for(const kind of [1,2])for(const state of kind===1?[1]:[5,6])for(const transport of [false,true]) {
    const js=await fixture(),rs=await fixture(true);
    for(const f of [js,rs]) {
      const a=f.agent(kind);a.state=state;a.hint=f.h;a.flag=true;
      f.unit.classDef.VehicleType=transport?'transport':'fighter';
      a.onEnter();a.behave(0.25);if(kind===2)a.onExit();
    }
    // Object targets retain their identity within each world; compare the
    // foreign argument fields without serializing cyclic object graphs.
    const shape=f=>({events:f.events.map(e=>e.map(x=>x?.post ? {...x,post:x.post===f.h.commandPost}:x)),
      pending:f.c.agent.pending,flag:f.c.agent.flag,reboardAt:f.c.reboardAt,hintAt:f.h.reservedAt,hints:f.hintEvents,random:f.ai.randomA.state});
    assert.deepEqual(shape(rs),shape(js),`${kind}/${state}/${transport}`);
  }
});

test('hold and close hints, passenger exit and stuck destruction preserve reentry',async()=>{
  for(const native of [false,true]) {
    const f=await fixture(native),{service,h,c,unit,hull}=f;
    const next={...h,link:h};h.link=next;
    service.holdHint(h,c);service.closeHint(h,80);assert.equal(h.reservedAt,76);assert.equal(next.reservedAt,76);
    const passenger={id:4,character:{}};
    hull.seats.push({lease:{unit:passenger,human:false,released:false,vehicle:hull},weapons:[],settings:{}});
    const post=h.commandPost;
    f.world.vehicleSystem.unboard=(u,options)=>{assert.equal(u,passenger);assert.equal(u.character.objective,post);service.reserveTarget(unit,unit);return options.promote;};
    assert.equal(service.disembarkPassengers(c,post),1);
    hull.damage=amount=>{assert.equal(amount,Infinity);assert.equal(service.targetReserved(unit,1),true);};
    assert.equal(service.destroyStuck(c),undefined);
  }
});

test('dormant patterns preserve random draws, callback order and live state',async()=>{
  for(const pattern of [14,16]) {
    const js=await fixture(),rs=await fixture(true);
    for(const f of [js,rs]) {
      const a=f.agent(2),target={...f.unit,id:7,position:v(5,21,-20),velocity:v(1,0,1)};
      a.tracker={point:target.position,freshTarget:target,target,setDelay:(s,n)=>f.events.push(['delay',s,n]),defaultDelay:n=>f.events.push(['default',n])};
      const p={agent:a,controller:f.c,world:f.world,random:f.ai.randomA,face:s=>f.events.push(['face',s])};
      f.pattern=p;f.service.enterPattern(p,pattern);
      for(let i=0;i<100;i++){f.unit.upY=i%2?-1:1;f.service.tickPattern(p,pattern,0.1);}
      f.service.exitPattern(p,pattern);
    }
    assert.deepEqual(rs.events,js.events);assert.deepEqual(rs.unit.steer,js.unit.steer);assert.deepEqual(rs.pattern.vehicle,js.pattern.vehicle);
    assert.equal(rs.ai.randomA.state,js.ai.randomA.state);assert.equal(rs.c.fireA,js.c.fireA);
  }
});

test('a human pilot shares a nonnull target with the same gunner object',async()=>{
  for(const native of [false,true]) {
    const f=await fixture(native);f.hull.flying=false;
    const human={...f.lease,human:true};f.hull.seats.unshift({lease:human,weapons:[],settings:{}});f.lease.index=1;
    const target={...f.unit,id:8,team:2,position:v(0,20,40)};
    f.world.vehicleSystem.humanTarget=()=>target;
    assert.equal(f.service.sharedTarget(f.c),target);
  }
});

test('native steering matches the source for every mounted family and preserves bags',async()=>{
  for(const kind of [2,5,6,7])for(const empty of [false,true]) {
    const js=await fixture(),rs=await fixture(true);
    for(const f of [js,rs]) {
      f.unit.kind=kind;f.unit.classDef.TurnRate=2;f.unit.classDef.PitchRate=1;
      f.unit.classDef.MaxPitchSpeed=0.5;f.unit.classDef.MaxTurnSpeed=2;f.unit.classDef.MaxYawSpeed=1;
      f.unit.classDef.MovingTurnOnly=true;
      if(empty)f.unit.steer={};
      const original=f.unit.steer;
      const r={valid:true,steeringTarget:v(30,21,100),waypoint:v(30,21,100),segmentDirection:v(0,0,1),mode:1,lower:-1,upper:1};
      const out={};f.answer=out;
      for(let i=0;i<8;i++){f.world.time+=0.1;assert.equal(f.service.analogs(f.unit,r,v(20,24,90),0.1,out),out);}
      if(original)assert.equal(f.unit.steer,original);
    }
    assert.deepEqual(rs.answer,js.answer,`${kind}/${empty} stick`);
    assert.deepEqual(rs.unit.steer,js.unit.steer,`${kind}/${empty} steer`);
    assert.deepEqual(rs.unit.intent,js.unit.intent,`${kind}/${empty} intent`);
  }
});

test('callbacks consume generator A draws between native pattern draws',async()=>{
  const js=await fixture(),rs=await fixture(true);
  for(const f of [js,rs]) {
    const a=f.agent(2),target={...f.unit,id:9,position:v(10,20,-10)};
    a.tracker={point:target.position,freshTarget:target,target,setDelay(){f.events.push(['delayDraw',f.ai.randomA.next()]);}};
    const goto=f.helper.goto;
    f.helper.goto=(p,o)=>{f.events.push(['helperDraw',f.ai.randomA.next()]);f.service.reserveTarget(target,f.unit);goto(p,o);};
    const p={agent:a,controller:f.c,world:f.world,random:f.ai.randomA,face:()=>{}};
    f.service.enterPattern(p,14);for(let i=0;i<30;i++)f.service.tickPattern(p,14,0.1);
    f.pattern=p;
  }
  assert.deepEqual(rs.events,js.events);assert.deepEqual(rs.pattern.vehicle,js.pattern.vehicle);assert.equal(rs.ai.randomA.state,js.ai.randomA.state);
});

test('notifier uses world relations and keeps nested service writes',async()=>{
  for(const native of [false,true])for(const callback of [false,true]) {
    const f=await fixture(native);
    f.hull.seats.push({lease:null,weapons:[],settings:{}});
    const infantry={...f.unit,id:10,team:2,kind:0,vehicle:null,lease:null,seat:null,soldier:{},controller:{isUnitController:true}};
    if(callback)infantry.controller.addVehicleNote=note=>{
      assert.equal(note.unit,f.unit);
      f.service.reserveTarget(f.unit,infantry);
      f.service.notifier.counter=3;
    };
    f.world.relation=()=>1;
    f.world.units.query=(_p,_r,_out,filter)=>[infantry].filter(filter);
    assert.equal(f.service.notifier.consider(f.service,f.unit),1);
    if(callback){assert.equal(f.service.notifier.counter,3);assert.equal(f.service.reservations.has(f.unit,infantry,2),true);}
  }
});

test('seat input keeps service fields and callback context on the same objects',async()=>{
  const js=await fixture(),rs=await fixture(true);
  for(const f of [js,rs]) {
    const {service,unit,c}=f;
    const seatOut=service.seatOut,pad=service.padLeft,decay=f.hull.decay={idle:0,sample:0};
    pad.set(f.hull,20);assert.equal(pad.get(f.hull),20);
    const r={valid:true,steeringTarget:v(30,21,100),waypoint:v(30,21,100),segmentDirection:v(0,0,1),mode:1,lower:-1,upper:1};
    const inner={request:r,attach:u=>assert.equal(u,unit),update:()=>f.events.push(['inner'])};
    c.navControl=inner;
    const outer={request:r,attach:u=>assert.equal(u,unit),update:()=>{
      f.events.push(['outer']);
      service.analogs(unit,r,null,0.1,{});
      service.reserveTarget(unit,unit);
    }};
    const intent={};assert.equal(service.seatInput(outer,unit,intent,0.1),intent);
    service.updateDecay(0.1);
    assert.equal(service.seatOut,seatOut);assert.equal(service.padLeft,pad);assert.equal(f.hull.decay,decay);
    assert.deepEqual(seatOut,intent);assert.equal(service.reservations.has(unit,unit,2),true);
    f.answer=intent;
  }
  assert.deepEqual(rs.events,js.events);assert.deepEqual(rs.answer,js.answer);assert.deepEqual(rs.hull.decay,js.hull.decay);
});

test('support cache is visible during LOS reentry on the shared service',async()=>{
  const json={schema:1,map:'synthetic',hints:[],paths:[],regions:[],warnings:[],boundary:{references:[]},
      barriers:[{name:'post',xfrm:[1,0,0,0,1,0,0,0,-1,0,0,0],size:[20,0,20],flags:8}],
      graph:{info:[3,3,5],hubs:[30,40,60].map((x,i)=>({name:`hub${i}`,position:[x,0,0],radius:4,arcs:[0,1,2,255,255,255,255,255]})),
        arcs:[{name:'ab',hubs:[0,1],mask:31},{name:'ac',hubs:[0,2],mask:31},{name:'bc',hubs:[1,2],mask:31}],routes:{tables:5,count:45,byteOffset:0}}};
  const words=new Uint32Array(45);words.fill((15<<3)|(((16<<3)|1)<<8));
  const js=await fixture(false,json,words.buffer),rs=await fixture(true,json,words.buffer);
  for(const f of [js,rs]) {
    f.unit.kind=2;f.unit.position.set(30,0,0);
    const post={position:v(),team:1},helper={mask:8,radius:2},cache=f.service.supportCache;
    let nested=false,calls=0;
    f.world.los=()=>{
      calls++;assert.equal(cache.has(post),true);
      if(!nested){nested=true;f.service.reserveTarget(f.unit,f.unit);f.service.supportPoint(f.unit,post,{mask:8,radius:1});}
      return true;
    };
    f.answer=f.service.supportPoint(f.unit,post,helper);
    assert.ok(f.answer);assert.equal(calls,6);assert.equal(f.service.supportCache,cache);
    assert.deepEqual([...cache.get(post).keys()],['8/1.0','8/2.0']);
    assert.equal(f.service.reservations.has(f.unit,f.unit,2),true);
    const byHull=cache.get(post);f.service.supportPoint(f.unit,post,helper);assert.equal(cache.get(post),byHull);assert.equal(calls,6);
  }
  assert.deepEqual(rs.answer,js.answer);assert.equal(rs.ai.randomA.state,js.ai.randomA.state);
});

test('controller binding keeps its object, policy identity and one request adapter',async()=>{
  for(const kind of [0,2,5,6,7]) {
    const js=await fixture(),rs=await fixture(true);
    for(const f of [js,rs]) {
      f.unit.kind=kind;if(kind===0)f.unit.seat=null;
      if(kind===7)f.unit.turret={};
      const c=f.c;f.service.adaptController(c,f.unit);const request=c.requestState;
      f.service.adaptController(c,f.unit);assert.equal(c.requestState,request);assert.equal(f.unit.controller,c);
      assert.equal(c.policy,f.service.controllerPolicy(f.unit));
      for(const state of [0,1,2,3,4,5,6,7,8,9,10,11,100])c.requestState(state);
      const bag=f.service.steerState(f.unit);assert.equal(f.service.steerState(f.unit),bag);
    }
    assert.deepEqual(rs.events,js.events);assert.equal(rs.c.homeFacing,js.c.homeFacing);assert.deepEqual(rs.unit.steer,js.unit.steer);
  }
});
