import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { loadSim } from '../src/sim/module.js';
import { SimBridge, Heap } from '../src/sim/runtime.js';
import { KIND_UNIT, UnitMirror, SoldierMirror, DroidekaMirror, F3BoundaryMirror, F3PhaseInputMirror, F3ChangeMirror,
  syncF3Boundary, putF3PhaseInput, putF3Change, F3AdoptionMirror, putF3Adoption, HitShapeMirror, CombatMirror } from '../src/sim/gen/index.js';
import { F3UnitBinding } from '../src/f3-contract.js';
import {applyDelta,deltaBuffer} from '../src/f2-delta.js';
import {F2DeltaLayout} from '../src/sim/gen/f2.js';

// Low-level fixtures consume the same encoded deltas and shared acknowledgement
// as the production bridge, including the publication before each aimer.
function publications(b,layout,bindings) {
  for(let i=0;i<layout.outputRows;i++) {
    const at=layout.output/8+i*layout.outputStride;
    const binding=bindings.get(b.f[at+F2DeltaLayout.slot]);
    binding.words??=deltaBuffer();
    applyDelta(b.f,at,layout.outputHeap/8,binding.words);
    binding.sync(b,binding.words,5,binding.words[2]);
  }
  b.f[layout.acknowledgement/8]=layout.version;
}

function unit(id) {
  const u=new UnitMirror();
  Object.assign(u,{id,generation:1,side:'rep',className:'fixture',classDef:{class:'fixture'},alive:true,dead:false,removed:false,
    team:1,deadTime:0,tickCount:0,soldier:new SoldierMirror()});
  u.soldier.position.set(2,3,4);u.soldier.previous.copy(u.soldier.position);u.soldier.settled.copy(u.soldier.position);
  u.soldier.velocity.set(0,0,0);u.soldier.avoidance.set(0,0,0);u.soldier.normal.set(0,1,0);
  u.soldier.mover=-1;u.soldier.maxTurn=1;u.soldier.aimYaw=.25;u.soldier.aimPitch=0;u.soldier.pitchLimits=[-1.4,1.4];u.soldier.lock=0;u.soldier.tickLength=1/60;
  u.combat=new CombatMirror();
  u.shapes=[Object.assign(new HitShapeMirror(),{half:0}),Object.assign(new HitShapeMirror(),{half:0})];u.shapeCount=2;u.rootScale=1;
  u.controls.was=[0,0];u.controls.pressed=[false,false];u.intent.forward=0;u.intent.right=0;
  u.intent.turn=0;u.intent.pitch=0;u.intent.buttons=[0,0];u.spheresTick=-1;
  return u;
}

test('owned phase retains canonical gated bodies without body write-in and commands publish one row',async()=>{
  const instance=await loadSim(), b=new SimBridge(instance,{classes:{Soldier:SoldierMirror,Droideka:DroidekaMirror}}), x=b.x;
  const ptr=x.f3_init(0), layout=new F3BoundaryMirror();
  const seated=unit(1),held=unit(2),removed=unit(3);
  seated.seat={};held.held={prop:{},since:0,frame:null,time:undefined};removed.removed=true;
  b.writeSome(KIND_UNIT,[seated,held,removed]);
  b.views();syncF3Boundary(b,b.f,ptr/8,0,layout);
  const heap=new Heap();
  const bindings=new Map([seated,held,removed].map(u=>[u.id,new F3UnitBinding(u,{slot:u.id,generation:0,life:1})]));
  for(const u of [seated,held,removed]) {
    const adoption=new F3AdoptionMirror();Object.assign(adoption.key,{slot:u.id,generation:0,life:1});
    // Adoption reserves each body's history frames and may grow memory.
    // Construction writes reacquire the view, as the production bridge does.
    b.views();putF3Adoption(b,adoption,b.f,layout.command/8,heap);assert.equal(x.f3_adopt(),1);
    b.views();syncF3Boundary(b,b.f,ptr/8,0,layout);publications(b,layout,bindings);
  }
  b.views();syncF3Boundary(b,b.f,ptr/8,0,layout);
  let at=layout.input/8;
  for(const u of [seated,held,removed]){
    const input=new F3PhaseInputMirror();input.key.slot=u.id;input.key.generation=0;input.key.life=1;
    input.intent=u.intent;input.remoteFirstPerson=false;
    putF3PhaseInput(b,input,b.f,at,heap);at+=layout.inputStride;
  }
  // A mirror write cannot become an implicit body input.
  seated.soldier.position.x=99;
  assert.equal(x.f3_step(3,1/60,NaN,0),1);
  b.views();syncF3Boundary(b,b.f,ptr/8,0,layout);
  assert.equal(layout.outputRows,2);
  publications(b,layout,bindings);
  assert.equal(seated.soldier.position.x,2);assert.equal(seated.tickCount,1);assert.equal(held.tickCount,1);assert.equal(removed.tickCount,0);
  const command=new F3ChangeMirror();Object.assign(command.key,{slot:1,generation:0,life:1});command.action='team';command.value=2;
  putF3Change(b,command,b.f,layout.command/8,heap);
  assert.equal(x.f3_command(),1);b.views();syncF3Boundary(b,b.f,ptr/8,0,layout);
  assert.equal(layout.outputRows,1);publications(b,layout,bindings);assert.equal(seated.team,2);assert.equal(held.team,1);
  command.key.life=0;putF3Change(b,command,b.f,layout.command/8,heap);assert.equal(x.f3_command(),0);
  command.key.life=1;command.key.generation=1;putF3Change(b,command,b.f,layout.command/8,heap);assert.equal(x.f3_command(),0);
});

test('native shape sensors retain stale death poses and strict ordered ray ties',async()=>{
  const instance=await loadSim(), b=new SimBridge(instance,{classes:{Soldier:SoldierMirror,Droideka:DroidekaMirror}}), x=b.x;
  const generated=await import('../src/sim/gen/index.js');
  const ptr=x.f3_init(0), layout=new F3BoundaryMirror(), heap=new Heap();
  const first=unit(1),second=unit(2);
  first.soldier.position.set(0,0,5);second.soldier.position.set(0,0,5);
  first.rootScale=second.rootScale=1;
  b.writeSome(KIND_UNIT,[first,second]);
  const registry=new generated.UnitsMirror();registry.list=[first,second];registry.nextId=3;
  b.writeSome(generated.KIND_UNITS,[registry]);
  b.views();syncF3Boundary(b,b.f,ptr/8,0,layout);
  for(const u of [first,second]){
    const a=new F3AdoptionMirror();Object.assign(a.key,{slot:u.id,generation:0,life:1});
    b.views();putF3Adoption(b,a,b.f,layout.command/8,heap);assert.equal(x.f3_adopt(),1);
  }
  const request=new generated.F3SensorMirror(), view=new generated.F3ShapeViewMirror(), result=new generated.F3SensorResultMirror();
  request.mode='shapes';Object.assign(request.key,{slot:1,generation:0,life:1});
  function sensor(){
    b.views();generated.putF3Sensor(b,request,b.f,layout.command/8,heap);const at=x.f3_sensor()/8;b.views();return at;
  }
  let at=sensor();generated.syncF3ShapeView(b,b.f,at+5,at+b.f[at+2],view);
  const standing=view.shapes.map(s=>s.center.toArray());
  const dead=new F3ChangeMirror();Object.assign(dead.key,request.key);dead.action='combatField';dead.field=2;dead.value=1;
  putF3Change(b,dead,b.f,layout.command/8,heap);assert.equal(x.f3_command(),1);
  at=sensor();generated.syncF3ShapeView(b,b.f,at+5,at+b.f[at+2],view);
  assert.deepEqual(view.shapes.map(s=>s.center.toArray()),standing,'damage does not force the lazy body cache');
  request.mode='raycast';request.origin.set(0,1,0);request.direction.set(0,0,1);request.far=10;request.riders=true;
  at=sensor();generated.syncF3SensorResult(b,b.f,at+5,at+b.f[at+2],result);
  assert.equal(result.hit.found,true);assert.equal(result.hit.unit,first,'the first equal-distance registry entry wins');
  request.masked=true;b.f[layout.input/8+1]=0;b.f[layout.input/8+2]=1;
  at=sensor();generated.syncF3SensorResult(b,b.f,at+5,at+b.f[at+2],result);assert.equal(result.hit.unit,second);
});

test('owned movement and combat phase matches frozen movement results over retained input rows',async()=>{
  const [{Soldier},{CollisionWorld},{loadRayKernel},{F3CombatStore}]=await Promise.all([
    import('../src/soldier.js'),import('../src/collision.js'),import('../src/ray-kernel.js'),import('../src/f3-combat.js')]);
  const instance=await loadSim(),b=new SimBridge(instance,{classes:{Soldier:SoldierMirror,Droideka:DroidekaMirror}}),x=b.x;
  const rays=await loadRayKernel(instance);
  const collision=new CollisionWorld(new Float32Array([-100,0,-100,-100,0,100,100,0,100,100,0,-100]),
    new Uint32Array([0,1,2,0,2,3]),new Uint32Array([0,1]),undefined,undefined,rays);
  const cls={class:'fixture',base:'soldier',AddHealth:0,weapons:[]}, combat=new F3CombatStore(b);combat.unitClass(cls);
  const reference=new Soldier(collision,cls);reference.spawn(0,0,0,0);
  const golden=JSON.parse(readFileSync(new URL('./fixtures/f3-movement-golden.json',import.meta.url),'utf8'));
  const bits=new DataView(new ArrayBuffer(8));
  const owned=unit(1);owned.classDef=cls;owned.soldier=new SoldierMirror();Object.assign(owned.soldier,reference);
  owned.soldier.position=reference.position.clone();owned.soldier.previous=reference.previous.clone();owned.soldier.settled=reference.settled.clone();
  owned.soldier.velocity=reference.velocity.clone();owned.soldier.normal=reference.normal.clone();owned.soldier.avoidance=reference.avoidance.clone();
  owned.combat.health=owned.combat.maxHealth=100;owned.combat.dead=false;owned.combat.team=1;
  Object.assign(owned.combat.kick,{rate:0,hold:0,offset:0});
  const descriptor=x.f3_init(collision.rayWorld),layout=new F3BoundaryMirror(),heap=new Heap();
  b.writeSome(KIND_UNIT,[owned]);b.views();syncF3Boundary(b,b.f,descriptor/8,0,layout);
  const adoption=new F3AdoptionMirror();Object.assign(adoption.key,{slot:1,generation:0,life:1});putF3Adoption(b,adoption,b.f,layout.command/8,heap);
  assert.equal(x.f3_adopt(),1);
  const binding=new F3UnitBinding(owned,adoption.key), bindings=new Map([[1,binding]]);
  b.views();syncF3Boundary(b,b.f,descriptor/8,0,layout);publications(b,layout,bindings);
  b.register({unitAim:()=>{b.views();syncF3Boundary(b,b.f,descriptor/8,0,layout);publications(b,layout,bindings);return {aim:null};}});
  const input=new F3PhaseInputMirror();Object.assign(input.key,adoption.key);input.intent=owned.intent;input.remoteFirstPerson=false;
  for(let tick=0;tick<120;tick++){
    input.intent.forward=tick<80?1:0;input.intent.right=tick>=40&&tick<90?.25:0;
    b.views();putF3PhaseInput(b,input,b.f,layout.input/8,heap);assert.equal(x.f3_step(1,1/60,NaN),1);
    b.views();syncF3Boundary(b,b.f,descriptor/8,0,layout);publications(b,layout,bindings);
    const expected=golden.frames[tick].split(' ');let field=0;
    for(const name of golden.fields)for(const axis of golden.axes){
      bits.setFloat64(0,owned.soldier[name][axis],true);
      assert.equal(bits.getBigUint64(0,true).toString(16).padStart(16,'0'),expected[field++],`${tick} ${name}.${axis}`);
    }
    assert.equal(owned.tickCount,tick+1);
  }
});

test('owned bridge pools ordinary Unit objects and submits no body rows after construction',async(t)=>{
  const [{Soldier},{Droideka},{CollisionWorld},{loadRayKernel},{attachF3Combat},{attachF3},{Unit},{Units},THREE]=await Promise.all([
    import('../src/soldier.js'),import('../src/droideka.js'),import('../src/collision.js'),import('../src/ray-kernel.js'),
    import('../src/f3-combat.js'),import('../src/f3-sim.js'),import('../src/units.js'),import('../src/registry.js'),import('three')]);
  const instance=await loadSim(),rays=await loadRayKernel(instance);
  const collision=new CollisionWorld(new Float32Array([-100,0,-100,-100,0,100,100,0,100,100,0,-100]),new Uint32Array([0,1,2,0,2,3]),new Uint32Array([0,1]),undefined,undefined,rays);
  const units=new Units(),cls={class:'fixture',base:'soldier',MaxHealth:100,AddHealth:0,weapons:[],chunks:[{}],ChunkFrequency:.01};
  const combatSim=attachF3Combat(instance,{Unit,Soldier,Droideka});
  const { Ordnance } = await import('../src/ordnance.js');
  const { attachOrdnanceSim } = await import('../src/ordnance-sim.js');
  const ordnance = new Ordnance({scene:new THREE.Scene(),collision});
  attachOrdnanceSim(ordnance, combatSim.bridge);
  const factory={built:[],context:{units,ordnance},
    acquire(name){return this.context.unitPhase.acquire(name);},
    release(unit){return this.context.unitPhase.release(unit);},
    build(name){
    const soldier=new Soldier(collision,cls),actor={root:new THREE.Object3D(),bones:[]},animator={reset(){},clips:new Map()},presentation={reset(){},position:new THREE.Vector3()};
    const u=new Unit({side:'rep',className:name,def:cls,actor,soldier,animator,presentation});
    u.combat=combatSim.make({soldier,def:cls,actor,animator,collision,owner:u});
    units.add(u);u.combat.attach();this.context.unitPhase.adopt(u);this.built.push(u);return u;
  }};
  const phase=attachF3(instance,{Unit,Soldier,Droideka},{factory,combatSim,collision,water:null,map:{killY:-100,deathRegions:[],scripted:[]},presentEffect(){},debug:true});
  const {RandomStreams}=await import('../src/ai/random.js');
  const streams=new RandomStreams(7);
  phase.random.configure(streams.draw('gameplay'),streams.draw('grenade-spin'),streams.draw('item-drop'));
  factory.release(factory.build('fixture'));
  const u=factory.acquire('fixture');u.controller={};u.spawn(0,0,0,0);
  // Enhanced's post-movement settings return as deltas in the aimer reply.
  // No setter may reenter a mutating export while this phase is on the stack.
  u.onMoved=()=>{u.combat.gunUnderwater=false;u.combat.deferMuzzles=true;};
  const position=u.position,velocity=u.velocity,firstArsenal=u.combat.arsenal;
  phase.bodiesIn();phase.counters.reset();u.intent.forward=1;
  for(let tick=0;tick<120;tick++) phase.phase(1/60);
  assert.equal(u.position,position);assert.equal(u.velocity,velocity);assert.equal(u.tickCount,121);assert.ok(u.position.z>0);
  const measured=phase.counters.snapshot();assert.equal(measured.phaseCalls,120);assert.equal(measured.commandCalls,0);assert.equal(measured.importCalls,120);
  assert.equal(measured.rustAllocations,0); assert.equal(measured.rustReallocations,0);
  t.diagnostic(`synthetic phase: ${measured.wasmCalls/120} export calls, ${measured.importCalls/120} imports, ${measured.inputFields/120} input words, ${measured.outputFields/120} output words per tick`);
  let damageNotice, firstPoint, firstSource;
  u.combat.onDamaged=(amount,attacker,info,sample)=>{
    firstPoint = info.point; firstSource = info.source;
    damageNotice={amount,life:sample.key.life,known:info.known,typed:info.typed,generic:info.generic,
      authored:info.amount,point:info.point.toArray(),source:info.source.toArray()};
  };
  u.combat.damage(5,5,new THREE.Vector3(7,8,9),null,{amount:11,typed:.5,generic:.75,known:true,point:new THREE.Vector3(1,2,3)});
  assert.deepEqual(damageNotice,{amount:5,life:1,known:true,typed:.5,generic:.75,authored:11,point:[1,2,3],source:[7,8,9]});
  assert.equal(u.combat.health,95);
  u.combat.onDamaged=(amount,attacker,info)=>{assert.equal(info.point,undefined);assert.equal(info.source,null);};
  u.combat.damage(1);
  u.combat.onDamaged=(amount,attacker,info)=>{assert.equal(info.point,firstPoint);assert.equal(info.source,firstSource);};
  u.combat.damage(1,1,new THREE.Vector3(4,5,6),null,{point:new THREE.Vector3(3,2,1)});
  u.remove();assert.equal(factory.acquire('fixture'),u);u.spawn(2,0,3,0);
  assert.equal(u.position,position);assert.equal(u.generation,2);assert.equal(u.intent.forward,0);assert.notEqual(u.combat.arsenal,firstArsenal);
  const {ClassicRandom,uniformRandom}=await import('../src/ai/random.js');
  const shared=new ClassicRandom(17),expected=new ClassicRandom(17);
  assert.ok(expected.uniform()>.01);
  u.combat.random=uniformRandom(shared);u.combat.onDamaged=()=>{};
  u.combat.damage(200);
  assert.equal(shared.state,expected.state,'native death consumes the shared chunks draw exactly once');
  assert.equal(u.dead,true);
  u.position.x++;assert.throws(()=>phase.check(),/F3 mirror write/);
});

// Notifications may be read before an aimer, then appended before phase end.
// The ordinary generated event stream keeps one prefix and one scratch mirror.
test('owned event publications drain appended prefixes once and reset at the next phase',async()=>{
  const instance=await loadSim(),b=new SimBridge(instance),x=b.x;
  const generated=await import('../src/sim/gen/index.js');
  const ptr=x.f3_init(0),layout=new F3BoundaryMirror();
  b.views();syncF3Boundary(b,b.f,ptr/8,0,layout);
  const event=generated.EVENT_TABLE.find(e=>e?.name==='echoed');
  const mirrors=[],record=new event.record.Mirror(),values=[];
  mirrors[event.code]=record;
  const handlers={echoed:e=>{assert.equal(e,record);values.push(e.value);}};
  const args=new generated.EchoMirror();
  args.value=0;args.at.set(1,2,3);args.count=0;args.note=null;args.events=2;
  const answer=new generated.EchoMirror();
  b.command('echo',args,answer);
  let offset=b.drainFrom(layout.events,0,mirrors,handlers);
  assert.deepEqual(values,[0,1]);
  assert.equal(b.drainFrom(layout.events,offset,mirrors,handlers),offset);
  assert.deepEqual(values,[0,1]);
  args.events=1;b.command('echo',args,answer);
  offset=b.drainFrom(layout.events,offset,mirrors,handlers);
  assert.deepEqual(values,[0,1,0]);
  const epoch=layout.eventEpoch;
  const acknowledgement=new generated.F3AcknowledgementMirror();
  acknowledgement.version=layout.version;
  acknowledgement.eventEpoch=epoch;acknowledgement.eventOffset=offset;
  generated.putF3Acknowledgement(b,acknowledgement,b.f,layout.acknowledgement/8,b.heap);
  assert.equal(x.f3_step(0,1/60,NaN),1);
  b.views();syncF3Boundary(b,b.f,ptr/8,0,layout);
  assert.equal(layout.eventEpoch,epoch+1);
  assert.equal(layout.eventWords,0);
  assert.equal(b.drainFrom(layout.events,0,mirrors,handlers),0);
});


test('AI contact consumption clears canonical movement input without invalidating shapes', async() => {
  const {f3World} = await import('./fixtures/f3-world.js');
  const world = await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[]}},
    {prepareUnit:unit=>unit.soldier.avoidance.set(3,0,4)});
  const unit = world.spawn('soldier',0,0,true), contact = unit.soldier.avoidance;
  const stamp = unit.tickCount;
  unit.hitShapes();
  const observed = unit.spheresTick;
  assert.equal(observed,stamp);
  world.phase.consumeContact(unit);
  assert.ok(unit.soldier.avoidance === contact); assert.deepEqual(contact.toArray(),[0,0,0]);
  assert.equal(unit.tickCount,stamp); assert.equal(unit.spheresTick,observed);
  world.phase.counters.reset(); world.phase.phase(1/60);
  assert.deepEqual(contact.toArray(),[0,0,0]);
  assert.equal(world.phase.counters.wasmCalls,1); assert.equal(world.phase.counters.rustAllocations,0);
});

test('scalar publications preserve controller actions until the owning phase consumes them', async() => {
  const {f3World} = await import('./fixtures/f3-world.js');
  const world = await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[]}});
  const unit = world.spawn('soldier',0,0,true), intent = unit.intent, input = unit.input;
  const actions = ['jump','crouch','prone','use','exit','reload','nextPrimary','previousPrimary','nextSecondary','zoom'];
  for (const name of actions) intent[name] = true;
  input.forward = .75;
  world.phase.consumeContact(unit);
  unit.soldier.look(1,2,1,false);
  unit.combat.health = 250;
  for (const name of actions) assert.equal(intent[name],true,`${name} survives a scalar publication`);
  assert.equal(unit.intent,intent); assert.equal(unit.input,input); assert.equal(input.forward,.75);
  world.phase.counters.reset(); world.phase.phase(1/60);
  assert.equal(intent.jump,false); assert.equal(intent.crouch,false); assert.equal(intent.prone,false);
  assert.equal(world.phase.counters.wasmCalls,1); assert.equal(world.phase.counters.rustAllocations,0);
  const {Vector3} = await import('three');
  intent.jetDirection = new Vector3(1,0,0);
  world.phase.phase(1/60);
  const retained = input.jetDirection;
  assert.deepEqual(retained.toArray(),[1,0,0]);
  intent.jetDirection = null; world.phase.phase(1/60);
  assert.equal(input.jetDirection,null);
  intent.jetDirection = new Vector3(0,0,1); world.phase.phase(1/60);
  assert.equal(input.jetDirection,retained); assert.deepEqual(retained.toArray(),[0,0,1]);
});

// Soldier.touchPolygons can first meet a dense body long after construction.
// Prepare its real polygon capacity without warming contact or movement state.
test('first soldier contact retains a dense polygon triangle buffer',async()=>{
  const {f3World}=await import('./fixtures/f3-world.js');
  const world=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[]}},
    {prepareCollision(collision){
      const positions=new Float32Array([-4,.1,-4,-4,.1,4,4,.1,4]);
      const indices=new Uint32Array(96), polygons=new Uint32Array(32);
      for(let i=0;i<32;i++){indices[i*3]=0;indices[i*3+1]=1;indices[i*3+2]=2;}
      collision.addBody(positions,indices,polygons,null);
    }});
  const unit=world.spawn('soldier',-1,1);
  world.phase.counters.reset();
  world.phase.phase(1/60);
  assert.ok(Number.isFinite(unit.position.y));
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});
