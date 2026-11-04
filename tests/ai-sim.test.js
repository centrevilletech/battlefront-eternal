import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { ClassicNav, NavControl, PathExecutor, navigationInput } from '../src/ai/classic/nav/index.js';
import { ClassicRandom } from '../src/ai/random.js';
import { attachAiSim } from '../src/ai-sim.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { loadSim } from '../src/sim/module.js';
import { Aimer, dropCorrect, leadPoint, skillCurves } from '../src/ai/classic/aim.js';
import { AIM_PROFILES, EnhancedAimer } from '../src/ai/enhanced/aim.js';
import { PriorityHeap, DeferredLos, Perception, TargetTracker, recordTarget, scan, thin } from '../src/ai/classic/perception.js';
import { ClassicScheduler } from '../src/ai/classic/scheduler.js';
import { FireControl, clusterState, consumeFire, facingAndRange, firingCone, peekClusterState, prepareSpread, pressButton, protectedVeto, selectWeapon, validateFire } from '../src/ai/classic/weapons-ai.js';

const point = (x = 0, y = 0, z = 0) => ({ x, y, z });
const config = () => ({ schema: 1, map: 'synthetic', graph: null,
  barriers: [0, 1, 2].map(i => ({ name: `barrier${i}`, xfrm: [1,0,0,0,1,0,0,0,-1,i*7,0,0], size: [2,0,3], flags: i + 1 })),
  hints: ['snipe','patrol','cover'].map((kind,i) => ({ kind, name: `hint${i}`, xfrm: [1,0,0,0,1,0,0,0,-1,i*7,0,0], properties: [] })),
  paths: [{ name: 'boundary', info: [4], points: [[-50,0,-50],[-50,0,50],[50,0,50],[50,0,-50]] }],
  boundary: { references: [{ path: 0 }] }, regions: [], warnings: [] });

async function pair(options = {}) {
  const json = config(), instance = await loadSim();
  const js = new ClassicNav(json, null, options), native = new ClassicNav(json, null, options);
  const sim = attachAiSim(instance);
  assert.equal(sim.bridge, bridgeFor(instance), 'AI joins the existing bridge');
  sim.bindNavigation(native, json, null, options);
  return { js, native, sim };
}

test('native perception queues preserve ties, discarded values and bounded object retention', async () => {
  const {sim}=await pair();
  const random=new ClassicRandom(581);
  for(const capacity of [1,5,112,200]) {
    const j=new PriorityHeap(capacity,20000),n=sim.heap(capacity,20000);
    const ji=j.items,ni=n.items,jp=j.priority,np=n.priority;
    for(let i=0;i<2048;i++) {
      const kind=i%23;
      if(i%257===0) {j.clear();n.clear();}
      else if(kind<17) {
        const priority=i%47===0?NaN:i%41===0?-Infinity:i%31===0?-0:i%29===0?-(2**40)-i:random.int(-2,20)+i%3/4;
        const item=i%13===0?null:i%17===0?undefined:{i};
        assert.equal(n.insert(priority,item),j.insert(priority,item));
      } else if(kind<21) assert.equal(n.pop(),j.pop());
      else { assert.equal(n.peek(),j.peek());assert.equal(n.peekPriority(),j.peekPriority()); }
      assert.equal(n.size,j.size);assert.equal(n.count,j.count);assert.equal(n.dropped,j.dropped);
      assert.equal(n.minIndex(),j.minIndex());
      assert.deepEqual(ni,ji,`retained items after ${capacity}:${i}`);
      assert.deepEqual(np,jp,`retained priorities after ${capacity}:${i}`);
      assert.deepEqual(n.slots(),j.slots());
      assert.ok(n.values.size<=capacity,'released values do not accumulate');
    }
    if(j.count) {const item={placed:true};j.place(j.count,-3.25,item);n.place(n.count,-3.25,item);assert.deepEqual(n.items,j.items);assert.deepEqual(n.priority,j.priority);}
    j.clear();n.clear();assert.equal(n.values.size,0);assert.deepEqual(ni,ji);assert.deepEqual(np,jp);
  }
});

test('native target ranking preserves stale-reference boosts, queue overflow and shared draws', async () => {
  const {sim}=await pair();
  const j={id:1,generation:0,perception:{heap:new PriorityHeap(112,20000)}};
  const n={id:1,generation:0,perception:{heap:sim.heap(112,20000)}};
  const jq=new DeferredLos(),nq=new DeferredLos();
  nq.active=sim.heap(200,200);nq.scratch=sim.heap(200,200);nq.nativeAi=sim;
  const jr=new ClassicRandom(554),nr=new ClassicRandom(554),random=new ClassicRandom(988);
  const units=Array.from({length:64},(_,id)=>({id,alive:true,generation:0}));
  const request=r=>r&&[r.controller.id,r.generation,r.target&&[r.target.unit.id,r.target.generation]];
  for(let i=0;i<512;i++) {
    j.generation=n.generation=i%7;
    for(const u of units) {u.alive=(u.id+i)%13!==0;u.generation=i%5;}
    const count=i%113;
    for(let k=0;k<count;k++) {
      const u=units[random.int(0,63)],ref=k%17===0?null:{unit:u,generation:u.generation+(k%11===0?1:0)};
      const priority=random.int(0,19999);
      j.perception.heap.insert(priority,ref);n.perception.heap.insert(priority,ref);
    }
    const u=units[i%64],retained=i%3===0?null:{unit:u,generation:u.generation+(i%7===0?1:0)};
    assert.equal(thin(n,{randomA:nr},retained,nq),thin(j,{randomA:jr},retained,jq));
    assert.equal(nr.state,jr.state,`draws ${i}`);
    assert.deepEqual(n.perception.heap.priority,j.perception.heap.priority);
    assert.deepEqual(n.perception.heap.items,j.perception.heap.items);
    assert.equal(nq.overflow,jq.overflow);
    assert.equal(nq.active.dropped,jq.active.dropped);
    assert.deepEqual(nq.active.priority,jq.active.priority);
    assert.deepEqual(nq.active.slots().map(request),jq.active.slots().map(request));
    assert.equal(n.perception.heap.values.size,0);
    for(let k=0;k<i%9;k++)assert.deepEqual(request(nq.active.pop()),request(jq.active.pop()));
  }
});

test('native acquired targets preserve candidate identities and cluster cache lifetimes', async () => {
  const {sim}=await pair();
  function fixture(native) {
    const controller={unit:{team:1}},p=controller.perception=new Perception(controller);
    const point=p.candidatePoint;
    if(native)sim.bindPerception(p);
    assert.equal(p.candidatePoint,point);
    const owners=[{},{},{}],targets=Array.from({length:18},(_,id)=>({id,kind:id%7===0?6:0,team:2,generation:0,
      controller:id%3===0?null:owners[id%3],position:new Vector3(id,0,0)}));
    for(const target of targets) {const c=clusterState(target,true);c.scanAt=-10;c.flaggedAt=-20;}
    const world={time:0,history:[]};
    const ref=r=>r==null?r:[r.unit.id,r.generation];
    world.units={
      relation(a,b){world.history.push(['relation',a,b]);return a===b?1:a===0||b===0?0:-1;},
      query(at,radius,out,filter){
        world.history.push(['query',at.toArray(),radius,ref(p.candidate),p.candidatePoint.toArray(),{...peekClusterState(targets[at.x])}]);
        for(const u of targets)if(filter(u))out.push(u);
        // The source uses its out array; no returned array is required.
      },
    };
    return {controller,p,point,targets,world};
  }
  const j=fixture(false),n=fixture(true);
  const snapshot=x=>({candidate:x.p.candidate==null?x.p.candidate:[x.p.candidate.unit.id,x.p.candidate.generation],
    point:x.p.candidatePoint.toArray(),count:x.p.lastScanCount,
    clusters:x.targets.map(t=>{const c=peekClusterState(t);return c&&{...c};})});
  for(let i=0;i<512;i++) {
    const target=i%18;
    for(const x of [j,n]) {
      x.world.time=i*.27;x.world.history.length=0;
      x.controller.unit.team=i%19===0?2:1;
      for(const u of x.targets) {u.team=u.id%11===i%11?1:2;u.generation=Math.floor(i/37);}
      x.p.lastScanCount=i;
      const previous=x.p.candidate;
      recordTarget(x.controller,x.targets[target],new Vector3(i%7===0?-0:i/3,2,-i/9),x.world);
      assert.notEqual(x.p.candidate,previous);
      assert.equal(x.p.candidate,x.p.candidate,'candidate reference remains stable between publications');
      assert.equal(x.p.candidatePoint,x.point);
    }
    assert.deepEqual(n.world.history,j.world.history,`query ordering ${i}`);
    assert.deepEqual(snapshot(n),snapshot(j),`candidate/cache ${i}`);
    if(i%43===0) {j.p.reset();n.p.reset();assert.deepEqual(snapshot(n),snapshot(j));}
  }
  for(const generation of [undefined,null,-0,NaN,7]) {
    for(const x of [j,n]) {
      const target=x.targets[0];target.generation=generation;
      recordTarget(x.controller,target,new Vector3(1,2,3),x.world);
    }
    assert.deepEqual(snapshot(n),snapshot(j));
  }
  const ref={unit:n.targets[1],generation:null};n.p.candidate=ref;assert.equal(n.p.candidate,ref);
  n.p.candidate=undefined;assert.equal(n.p.candidate,undefined);
});

test('native deferred sight preserves lifetimes, budgets, point writes and heap swaps', async () => {
  const {sim}=await pair();
  function fixture(native) {
    const world={time:0,tick:0,history:[]},queue=new DeferredLos();
    if(native)sim.bindLos(queue);
    const targets=Array.from({length:20},(_,id)=>{
      const u={id,alive:true,generation:0,kind:id%3===0?0:6,team:2,position:new Vector3(id*.3,0,id),
        aimPoint(out){world.history.push(['aim',id]);out.copy(this.position);out.y+=.37;},
      };
      if(id%2===0)u.origin=function(out){world.history.push(['target-eye',id]);out.copy(this.position);out.y+=1.61;};
      return u;
    });
    const controllers=Array.from({length:8},(_,id)=>{
      const c={id,generation:0,unit:{id:100+id,alive:true,team:1,origin(out){world.history.push(['eye',id]);out.set(id,-0,0);}}};
      c.saved=new Perception(c);c.perception=native?sim.bindPerception(c.saved):c.saved;return c;
    });
    world.units={relation:(a,b)=>a===b?1:-1,query(at,radius,out,filter){out.length=0;for(const u of targets)if(filter(u))out.push(u);}};
    world.los=(from,to,unit,target)=>{
      world.history.push(['los',from.toArray(),to.toArray(),unit.id,target.id,queue.tested]);
      if(native) {sim.words(new Array(1000).fill(3));sim.x.ai_nav_boundary(2,0,0,0,0);}
      if(world.tick%17===0)to.x+=.03125;
      if(world.tick%19===0)target.alive=false;
      return (world.tick+target.id)%3!==0;
    };
    return {world,queue,targets,controllers,first:queue.active};
  }
  const j=fixture(false),n=fixture(true),random=new ClassicRandom(411);
  const ref=r=>r==null?r:[r.unit.id,r.generation];
  const req=r=>r&&[r.controller.id,r.generation,ref(r.target)];
  const heap=h=>({count:h.count,dropped:h.dropped,priority:Array.from(h.priority),items:h.items.map(req)});
  const snapshot=x=>({activeFirst:x.queue.active===x.first,active:heap(x.queue.active),scratch:heap(x.queue.scratch),
    overflow:x.queue.overflow,tested:x.queue.tested,
    candidates:x.controllers.map(c=>c.perception&&[ref(c.perception.candidate),c.perception.candidatePoint.toArray()])});
  for(let i=0;i<384;i++) {
    for(const x of [j,n]) {
      x.world.time=i*.41;x.world.tick=i;x.world.history.length=0;
      for(const c of x.controllers) {c.generation=Math.floor(i/29);c.unit.alive=(i+c.id)%23!==0;c.perception=(i+c.id)%31===0?null:c.saved;if(c.perception && i%7===0)c.perception.candidate=null;}
      for(const t of x.targets) {t.alive=(i+t.id)%17!==0;t.generation=Math.floor(i/37);}
    }
    for(let k=0;k<20;k++) {
      const ci=random.int(0,7),ti=random.int(0,19),priority=random.int(0,199);
      for(const x of [j,n]) {
        const target=x.targets[ti];
        const r=k%13===0?null:{unit:target,generation:target.generation+(k%7===0?1:0)};
        assert.equal(x.queue.submit(x.controllers[ci],r,priority),undefined);
      }
    }
    if(i%13===0) {
      assert.equal(j.queue.drain(j.controllers[i%8]),undefined);assert.equal(n.queue.drain(n.controllers[i%8]),undefined);
    } else {
      const budget=[0,.5,1,3,12,Infinity,NaN,-1][i%8];
      assert.equal(n.queue.service(n.world,budget),j.queue.service(j.world,budget),`service ${i}`);
    }
    assert.deepEqual(n.world.history,j.world.history,`queries ${i}`);
    assert.deepEqual(snapshot(n),snapshot(j),`sight ${i}`);
  }
});

test('native sight scans preserve eligibility, query order, geometry and ranked heap state', async () => {
  const {sim}=await pair();
  function fixture(native) {
    const world={time:0,tick:0,history:[]},post={},hull={towTarget:-1,system:{vehicles:[]}};
    const unit={alive:true,kind:0,team:1,pilot:false,vehicle:hull,classDef:{ForwardSpeed:8},
      combat:{arsenal:{weapons:[{def:{MaxRange:60,ExtremeRange:400}},{levels:[{MaxRange:150},{MaxRange:220}]}]}},
      origin(out){world.history.push(['eye']);out.set(world.tick%11,-0,-3);},
      forward(out){world.history.push(['forward']);out.set(0,0,1);},
    };
    const controller={unit,objective:null,rangeScale:1},p=controller.perception=new Perception(controller);
    if(native)sim.bindPerception(p);
    const targets=Array.from({length:220},(_,id)=>({id,alive:true,kind:id%11,team:2,generation:0,collisionCategory:0,
      classDef:{MaxHealth:100},sphere:{radius:(id%5)*.17},soldier:id%11===0?{air:false,posture:'stand'}:null,
      building:id%17===0?{post}:null,vehicle:{id},
      position:new Vector3((id%23-11)*3.91,(id%5-2)*.13,(id%41-3)*2.13),
      aimPoint(out){world.history.push(['aim',id]);out.copy(this.position);out.y+=.43;},
    }));
    hull.system.vehicles=targets.map(t=>t.vehicle);
    world.vehicles={sharedTarget(c){world.history.push(['shared',p.heap.size,p.lastScanCount]);return world.tick%5===0?targets[world.tick%targets.length]:null;}};
    world.units={
      relation(a,b){world.history.push(['relation',a,b]);return a===0||b===0?0:a===b?1:-1;},
      query(at,radius,out,filter){
        world.history.push(['query',at.toArray(),radius]);out.length=0;
        for(const t of targets)if(filter(t))out.push(t);
      },
    };
    const dispatch=(c,u,t,secondary,w)=>{
      world.history.push(['dispatch',t.id,secondary,p.heap.size,p.lastScanCount]);
      assert.equal(c,controller);assert.equal(u,unit);assert.equal(w,world);
      if(native) {sim.words(new Array(4000).fill(4));sim.x.ai_nav_boundary(2,0,0,0,0);}
      if(world.tick%7===0)t.generation++;
      return {pattern:t.id%3!==0,aux:t.id%5!==0};
    };
    return {world,unit,controller,p,targets,post,hull,dispatch};
  }
  const j=fixture(false),n=fixture(true);
  const snapshot=x=>({count:x.p.lastScanCount,heapCount:x.p.heap.count,dropped:x.p.heap.dropped,
    priority:Array.from(x.p.heap.priority),items:x.p.heap.items.map(r=>r&&[r.unit.id,r.generation])});
  for(let i=0;i<256;i++) {
    for(const x of [j,n]) {
      x.world.tick=i;x.world.history.length=0;x.unit.alive=i%29!==0;x.unit.kind=i%11;x.unit.pilot=i%7===0;
      x.hull.towTarget=i%220;x.controller.objective=i%3===0?x.post:null;x.controller.rangeScale=[.25,1,2,6][i%4];
      for(const t of x.targets) {
        t.alive=(i+t.id)%31!==0;t.team=(i+t.id)%43===0?1:2;t.targetable=(i+t.id)%37===0?false:undefined;
        t.collisionCategory=(i+t.id)%41===0?16:0;
        t.combat={maxHealth:[null,100,Infinity,NaN,-Infinity][(i+t.id)%5]};
        if(t.soldier) {t.soldier.air=i%3===0;t.soldier.posture=['stand','crouch','prone','unknown'][i%4];}
      }
    }
    assert.equal(scan(n.controller,n.world,n.dispatch),scan(j.controller,j.world,j.dispatch),`queued ${i}`);
    assert.deepEqual(n.world.history,j.world.history,`scan callbacks ${i}`);
    assert.deepEqual(snapshot(n),snapshot(j),`scan state ${i}`);
  }
});

test('native target memory preserves retention, identities and state visible to callbacks', async () => {
  const {sim}=await pair();
  const ref=r=>r==null?r:[r.unit?.id,r.generation];
  const snapshot=t=>({ref:ref(t.ref),fresh:t.fresh,point:t.point.toArray(),sampleTime:t.sampleTime,cost:t.cost,
    delay:t.delay,frozen:t.frozen,counting:t.counting,deadline:t.deadline,target:t.target?.id??null,freshTarget:t.freshTarget?.id??null});
  function fixture(native) {
    const world={time:0,tick:0,history:[]},controller={unit:null};
    let tracker;
    const unit=controller.unit={id:100,alive:true,kind:0,team:1,origin(out){
      world.history.push(['eye',snapshot(tracker)]);out.set(-0,2,world.tick%7);
      if(native) {sim.words(new Array(8000).fill(1));sim.x.ai_nav_boundary(2,0,0,0,0);}
    }};
    const targets=Array.from({length:20},(_,id)=>({id,alive:true,generation:0,kind:id%11,team:2,
      position:new Vector3(id/3,1,id+3),aimPoint(out){world.history.push(['aim',id]);out.copy(this.position);}}));
    const p=controller.perception=new Perception(controller);if(native)sim.bindPerception(p);
    tracker=new TargetTracker(controller);const point=tracker.point;if(native)sim.bindTracker(tracker);
    assert.equal(tracker.point,point);
    for(const key of ['reset','refresh','startCountdown']) {
      const method=tracker[key];tracker[key]=function(...args){world.history.push(['method',key,snapshot(this)]);return method.apply(this,args);};
    }
    world.units={relation(a,b){world.history.push(['relation',a,b,snapshot(tracker)]);return a===b?1:-1;}};
    return {world,controller,unit,targets,p,tracker,point};
  }
  const j=fixture(false),n=fixture(true);
  for(let i=0;i<1024;i++) {
    for(const x of [j,n]) {
      x.world.time=i*.37;x.world.tick=i;x.world.history.length=0;
      for(const t of x.targets) {t.alive=(i+t.id)%29!==0;t.team=(i+t.id)%11===0?1:2;t.generation=Math.floor(i/43);}
      const target=x.targets[(i*7)%20];
      x.p.candidate=i%5===0?null:{unit:target,generation:target.generation+(i%17===0?1:0)};
      x.p.candidatePoint.set(target.position.x,1+i%3,target.position.z);
      const t=x.tracker,now=x.world.time;
      let result;
      switch(i%13) {
        case 0: result=t.reset(i%26===0?null:target);break;
        case 1: result=t.assign(target);break;
        case 2: result=t.refresh(target,new Vector3(i/7,-0,-i/3),now);break;
        case 3: result=t.startCountdown(now);break;
        case 4: result=t.setDelay([NaN,.4,4,16,Infinity,-3][i%6],now);break;
        case 5: result=t.defaultDelay(now);break;
        case 6: result=t.freeze(i%2===0,now);break;
        default: result=t.select(x.world);
      }
      x.result=result;
      assert.equal(t.point,x.point);assert.equal(t.ref,t.ref);
    }
    assert.equal(n.result,j.result,`result ${i}`);
    assert.deepEqual(n.world.history,j.world.history,`tracker callbacks ${i}`);
    assert.deepEqual(snapshot(n.tracker),snapshot(j.tracker),`tracker ${i}`);
  }
  for(const generation of [undefined,null,-0,NaN,7]) {
    for(const x of [j,n]) {
      const target=x.targets[0];target.generation=generation;target.alive=true;x.p.candidate={unit:target,generation};
      x.tracker.reset(target);x.tracker.select(x.world);
    }
    assert.deepEqual(snapshot(n.tracker),snapshot(j.tracker));
  }
});

test('native tracker firing keeps remembered points, public callbacks and weapon targets', async () => {
  const {sim}=await pair();
  const ref=r=>r&&[r.unit.id,r.generation];
  function fixture(native) {
    const world={time:0,tick:0,history:[]};let tracker;
    const weapon={channel:0,def:{base:'rifle',ordnance:{base:'bolt',Velocity:500},ai:{maxRange:100,implicitRange:false,extremeRange:100}}};
    const arsenal={weapons:[weapon],current(){world.history.push(['weapon']);return world.tick%9===0?null:weapon;}};
    const unit={id:100,alive:true,generation:0,kind:0,team:1,classDef:{},soldier:{zoomed:false},combat:{arsenal},
      origin(out){world.history.push(['eye']);out.set(0,1.7,0);},forward(out){world.history.push(['forward']);out.set(0,0,1);}};
    const controller={unit,fireA:false,fireB:false,target:null};
    const p=controller.perception=new Perception(controller);p.aimer.reset(new Vector3(0,0,1));if(native)sim.bindPerception(p);
    tracker=new TargetTracker(controller);if(native)sim.bindTracker(tracker);
    const targets=Array.from({length:4},(_,id)=>({id,alive:true,generation:0,kind:0,team:2,category:0,position:new Vector3(0,0,20+id),
      origin(out){world.history.push(['target-eye',id]);out.copy(this.position);out.y+=1.7;},
      aimPoint(out){world.history.push(['target-aim',id]);out.copy(this.position);out.y+=1;},
    }));
    const snapshot=()=>({ref:ref(tracker.ref),fresh:tracker.fresh,point:tracker.point.toArray(),cost:tracker.cost,
      counting:tracker.counting,deadline:tracker.deadline,fire:[controller.fireA,controller.fireB],target:ref(controller.target),weaponTarget:ref(p.fire.weaponTarget)});
    for(const key of ['select','assignWeaponTarget','fireControls','pointNearTarget']) {
      const method=tracker[key];tracker[key]=function(...args){world.history.push(['method',key,snapshot()]);return method.apply(this,args);};
    }
    world.units={relation(a,b){world.history.push(['relation',a,b]);return a===b?1:-1;}};
    world.los=(from,to,owner,target)=>{
      assert.equal(to,tracker.point);assert.equal(owner,unit);
      world.history.push(['los',from.toArray(),to.toArray(),target.id,snapshot()]);
      if(native){sim.words(new Array(10000).fill(4));sim.x.ai_nav_boundary(2,0,0,0,0);}
      if(world.tick%11===0)to.x+=.25;
      return world.tick%7!==0;
    };
    const dispatch=(c,u,t,allowed,w)=>{
      assert.equal(c,controller);assert.equal(u,unit);assert.equal(w,world);
      world.history.push(['dispatch',t.id,allowed,snapshot()]);
      if(world.tick%13===0)tracker.fresh=false;
    };
    return {world,unit,controller,p,tracker,targets,dispatch,snapshot};
  }
  const j=fixture(false),n=fixture(true);
  for(let i=0;i<768;i++) {
    for(const x of [j,n]) {
      x.world.tick=i;x.world.time=i*.1;x.world.history.length=0;
      const target=x.targets[i%4];
      for(const t of x.targets) {t.alive=(i+t.id)%37!==0;t.generation=Math.floor(i/41);t.position.z=20+t.id+(i%7===0?4:0);}
      x.tracker.reset(target);x.tracker.fresh=i%5===0;x.tracker.point.set(0,1.35,20+target.id);
      x.p.candidate=i%3===0?{unit:target,generation:target.generation}:null;x.p.candidatePoint.copy(x.tracker.point);
      let result;
      if(i%4===0)result=x.tracker.update(x.world,x.dispatch);
      else if(i%4===1)result=x.tracker.fireControls(x.world);
      else if(i%4===2)result=x.tracker.assignWeaponTarget(x.world);
      else result=x.tracker.pointNearTarget();
      x.result=result===x.tracker.point?'point':result;
    }
    assert.equal(n.result,j.result,`fire result ${i}`);
    assert.deepEqual(n.world.history,j.world.history,`fire callbacks ${i}`);
    assert.deepEqual(n.snapshot(),j.snapshot(),`fire tracker ${i}`);
  }
});

test('native projectile leading preserves source order, aliases and exact solver outputs', async () => {
  const {sim}=await pair();
  function fixture(native) {
    const history=[],target={velocity:new Vector3(),position:new Vector3(),
      aimPoint(out){history.push('target');out.copy(this.position);}},
    shooter={velocity:new Vector3(),controller:{ai:{world:{aiSim:native?sim:null}}},
      origin(out){history.push('origin');target.velocity.x+=.03125;out.set(-.13,1.7,4.11);
        if(native){sim.words(new Array(4000).fill(2));sim.x.ai_nav_boundary(2,0,0,0,0);}}};
    return {history,target,shooter,out:new Vector3(),point:new Vector3()};
  }
  const j=fixture(false),n=fixture(true),bases=['bolt','missile','shell','grenade','sticky','laser','beam','unknown'];
  const values=[undefined,null,0,-0,.00001,35,150,NaN,Infinity,'28.7'];
  for(let i=0;i<2048;i++) {
    const def=i%19===0?{}:{ordnance:{base:bases[i%bases.length],Velocity:values[Math.floor(i/8)%values.length],Gravity:values[Math.floor(i/80)%values.length]}};
    const weapon=i%13===0?null:{def};
    for(const x of [j,n]) {
      x.history.length=0;x.target.position.set(i/7-100,i%7-2,i/5);x.target.velocity.set(i%17-8,.1,i%11-5);
      x.shooter.velocity.set(i%5-2,0,-.3);x.point.set(i/9,-0,37+i/13);
      const out=i%17===0?x.point:x.out;
      x.result=i%3===0?dropCorrect(x.point,x.shooter,weapon,out):leadPoint((i%23)/60,x.shooter,x.target,weapon,out);
      x.answer=out.toArray();
    }
    assert.equal(n.result,j.result,`projectile validity ${i}`);
    assert.deepEqual(n.answer,j.answer,`projectile point ${i}`);
    assert.deepEqual(n.history,j.history,`projectile calls ${i}`);
    assert.deepEqual(n.target.velocity.toArray(),j.target.velocity.toArray());
  }
});

test('native trigger state preserves button edges, reference identities and partial resets', async () => {
  const {sim}=await pair(),j=new FireControl(),n=sim.bindFire(new FireControl()),random=new ClassicRandom(614);
  const buttons=n.buttons,burst=n.burst,protectedRefs=n.protected,units=Array.from({length:8},(_,id)=>({id,generation:id}));
  const snapshot=fire=>({...fire,buttons:fire.buttons.map(b=>({...b})),burst:{...fire.burst}});
  for(let i=0;i<4096;i++) {
    if(i%131===0) {assert.equal(j.reset(),undefined);assert.equal(n.reset(),undefined);}
    const value=i%59===0?NaN:i%47===0?-0:random.int(-150,150)/3;
    for(const fire of [j,n]) {
      fire.channel=i%2;fire.lastRelease=value;fire.charging=i%7===0;fire.hold=i/7;fire.sniperActive=i%11===0;fire.sniperDeadline=i/5;
      const key=['state','heatLimit','lastEnd','savedAmmo','count'][i%5];fire.burst[key]=value;
      if(i%17===0)fire.buttons[i%2].count=value;
      if(i%13===0)fire.buttons[i%2].code=i%4;
      const target={unit:units[i%8],generation:i%37===0?undefined:units[i%8].generation};fire.weaponTarget=target;
      assert.equal(fire.weaponTarget,target);assert.equal(fire.weaponTarget,fire.weaponTarget);
      if(fire.protected.length<8)fire.protected.push(target);
    }
    const down=random.int(0,1)!==0,channel=i%2;
    assert.equal(pressButton(j.buttons[channel],down),j.buttons[channel]);
    assert.equal(pressButton(n.buttons[channel],down),n.buttons[channel]);
    assert.deepEqual(snapshot(n),snapshot(j),`trigger state ${i}`);
    assert.equal(n.buttons,buttons);assert.equal(n.burst,burst);assert.equal(n.protected,protectedRefs);
  }
  j.reset();n.reset();assert.deepEqual(snapshot(n),snapshot(j));
});

test('native fire consumption preserves charge, sniper and burst decisions at shared draws', async () => {
  const {sim}=await pair();
  function fixture(native) {
    const world={time:0,tick:0,history:[]},random=world.randomA=new ClassicRandom(786);
    const bases=['cannon','cannon','launcher','rifle','rifle','melee','dispenser','shield','grenade','remote','repair','detonator'];
    const weapons=bases.map((base,id)=>({channel:id%2,rounds:1,clip:40,reserve:80,heat:0,limit:.35,held:0,wait:0,
      def:{base,MaxPressedTime:id===1||id===8?.6:0,ShotsPerSalvo:id%3+1,HeatPerShot:.05,
        ordnance:{base:id===1||id===8?'grenade':'bolt',Velocity:100,Gravity:1},
        ai:{maxRange:100,extremeRange:100,implicitRange:false,triggerSingle:id>=3,zoomMax:id===4?8:1,barrageMin:2,barrageMax:5,barrageDelay:.2}}}));
    const unit={id:100,alive:true,generation:0,kind:0,team:1,position:new Vector3(),classDef:{},soldier:{zoomed:false},intent:{buttons:[0,0]},
      origin(out){world.history.push(['eye']);out.set(0,1.7,0);},forward(out){world.history.push(['forward']);out.set(0,0,1);}};
    unit.combat={arsenal:{weapons,current(channel){world.history.push(['weapon',channel]);return world.tick%41===0?null:weapons[Math.floor(world.tick/19)%weapons.length];}}};
    const c=unit.controller={unit,ai:{world},fireA:false,fireB:false,target:null};
    const p=c.perception=new Perception(c);p.aimer.reset(new Vector3(0,0,1));
    if(native){world.aiSim=sim;sim.bindPerception(p);}
    const target={id:1,alive:true,generation:0,kind:0,team:2,category:0,position:new Vector3(0,0,20),velocity:new Vector3(),
      aimPoint(out){world.history.push(['target']);out.set(0,1.35,20);}};
    const human={id:2,alive:true,generation:0,sphere:{center:new Vector3(0,1.7,10),radius:1}};
    const snapshot=()=>({fire:{...p.fire,buttons:p.fire.buttons.map(b=>({...b})),burst:{...p.fire.burst},protected:p.fire.protected.map(r=>r.unit.id)},
      requests:[c.fireA,c.fireB],buttons:[...unit.intent.buttons],spread:weapons.map(w=>w.yawSpreadOverride),random:random.state});
    for(const name of ['range','int']) {
      const method=random[name];random[name]=function(lo,hi){
        world.history.push(['draw',name,lo,hi,snapshot()]);
        if(native){sim.words(new Array(11000).fill(2));sim.x.ai_nav_boundary(2,0,0,0,0);}
        if(world.tick%17===0)weapons[Math.floor(world.tick/19)%weapons.length].held=2;
        return method.call(this,lo,hi);
      };
    }
    world.units={relation:(a,b)=>a===b?1:-1};
    return {world,unit,c,p,target,human,weapons,snapshot};
  }
  const j=fixture(false),n=fixture(true);
  for(let i=0;i<4096;i++) {
    for(const x of [j,n]) {
      x.world.tick=i;x.world.time=i/20;x.world.history.length=0;
      x.c.fireA=i%7!==0;x.c.fireB=i%5===0;x.c.target=i%13===0?null:{unit:x.target,generation:i%29===0?1:0};
      x.p.fire.channel=Math.floor(i/19)%2;
      if(i%227===0)x.p.fire.reset();
      x.p.fire.protected.length=0;if(i%37===0)x.p.fire.protected.push({unit:x.human,generation:0});
      const w=x.weapons[Math.floor(i/19)%x.weapons.length];
      w.charging=i%11<4;w.held=(i%13)/10;w.wait=i%17===0?.01:0;w.reloading=i%31===0;w.busy=i%23===0;w.overheated=i%43===0;
      w.heat=(i%16)/20;w.clip=40-i%30;w.reserve=i%47===0?Infinity:80;w.bottomless=i%53===0;
      x.result=consumeFire(x.c,x.unit,x.world);
    }
    assert.equal(n.result,j.result,`consume result ${i}`);
    assert.deepEqual(n.world.history,j.world.history,`consume callbacks ${i}`);
    assert.deepEqual(n.snapshot(),j.snapshot(),`consume state ${i}`);
  }
});

test('native weapon guards preserve firing geometry, range bounds and changing protected lists', async () => {
  const {sim}=await pair();
  function fixture(native) {
    const history=[],world={aiSim:native?sim:null},c={ai:{world},unit:null,rangeScale:1},point=new Vector3(),lead=new Vector3();let tick=0;
    const weapons=['rifle','melee','rifle','repair','cannon'].map((base,id)=>({def:{base,LockOnRange:40,
      ordnance:{base:id===4?'towcable':'bolt'},ai:{maxRange:60+id*30,extremeRange:200,implicitRange:id%2===0,triggerSingle:id===2,zoomMax:id===2?8:1}}}));
    const unit=c.unit={kind:0,position:new Vector3(),combat:{arsenal:{current(){history.push(['weapon']);return tick%23===0?null:weapons[tick%weapons.length];}}},
      origin(out){history.push(['origin']);out.set(.3,1.7,-0);if(tick%17===0)point.y+=.125;},
      forward(out){history.push(['forward']);out.set(tick%19===0?.31:0,0,1);if(tick%29===0)c.perception.aimer.direction.x=.27;}};
    const p=c.perception=new Perception(c);if(native)sim.bindPerception(p);
    unit.controller=c;
    const target={alive:true,generation:0,position:new Vector3(0,0,20),velocity:new Vector3(.1,0,-.1),
      aimPoint(out){history.push(['target']);out.copy(this.position);out.y+=1.35;}};
    const humans=Array.from({length:8},(_,id)=>({id,alive:true,generation:0,get sphere(){
      history.push(['sphere',id]);
      if(id===0 && tick%11===0)p.fire.protected.push({unit:humans[7],generation:humans[7].generation});
      if(native){sim.words(new Array(9000).fill(5));sim.x.ai_nav_boundary(2,0,0,0,0);}
      return {center:new Vector3((id-2)*.2,1.7,id*4+5),radius:tick%13===0?0:1};
    }}));
    return {history,world,c,unit,p,point,lead,weapons,humans,target,setTick:i=>{tick=i;}};
  }
  const j=fixture(false),n=fixture(true);
  for(let i=0;i<2048;i++) {
    for(const x of [j,n]) {
      x.setTick(i);x.history.length=0;x.unit.kind=i%7;x.c.rangeScale=[0,.5,1,2,8,NaN][i%6];
      x.point.set((i%19-9)*.17,i%5,10+i%200);x.lead.copy(x.point);x.lead.y+=.03;
      x.p.aimer.direction.set(i%29===0?.12:0,0,i%31===0?-1:1);
      x.target.alive=i%31!==0;x.target.generation=Math.floor(i/73);x.target.position.z=10+i%180;
      x.c.target=i%17===0?null:{unit:x.target,generation:x.target.generation+(i%41===0?1:0)};
      x.unit.human=i%43===0;
      for(const w of x.weapons){w.sniperScale=i%37===0?-0:i%53===0?NaN:i/17;w.def.YawSpread=[undefined,0,'0',NaN,.01][i%5];}
      for(const h of x.humans){h.alive=(h.id+i)%17!==0;h.generation=Math.floor(i/83);}
      x.p.fire.protected.length=0;
      for(let k=0;k<i%8;k++)x.p.fire.protected.push({unit:x.humans[k],generation:x.humans[k].generation+(i%41===0?1:0)});
      x.result=[firingCone(x.c,x.unit,x.lead),facingAndRange(x.c,x.unit,x.lead,x.point),protectedVeto(x.c,x.unit,x.weapons[i%x.weapons.length]),
        validateFire(x.c,x.unit,i%7===0?x.humans[3]:x.target,x.world)];
      for(const w of x.weapons)assert.equal(prepareSpread(x.c,x.unit,w),undefined);
    }
    assert.deepEqual(n.result,j.result,`weapon geometry ${i}`);
    assert.deepEqual(n.history,j.history,`weapon queries ${i}`);
    assert.deepEqual(n.point.toArray(),j.point.toArray());
    assert.deepEqual(n.p.fire.protected.map(r=>r.unit.id),j.p.fire.protected.map(r=>r.unit.id));
    assert.deepEqual(n.weapons.map(w=>w.yawSpreadOverride),j.weapons.map(w=>w.yawSpreadOverride));
  }
});

test('native weapon selection preserves inventory changes, grenade priority and switch observations', async () => {
  const {sim}=await pair();
  function fixture(native) {
    const history=[],world={aiSim:native?sim:null,time:0},out={retained:9},c={ai:{world},rangeScale:1,roll:0,id:7};let tick=0,changed=false;
    const snapshot=()=>({...out,weapon:out.weapon?.id??null});
    const fireSnapshot=()=>[c.perception.fire.channel,c.perception.fire.lastRelease,c.perception.fire.charging,c.perception.fire.sniperActive,c.perception.fire.burst.state];
    const log=(name,...args)=>{history.push([name,...args,snapshot(),fireSnapshot()]);if(native){sim.words(new Array(10000).fill(3));sim.x.ai_nav_boundary(2,0,0,0,0);}};
    const pool=Array.from({length:8},(_,id)=>({id,channel:id%2,clip:15,reserve:30,rounds:1,bottomless:false,
      def:{base:['rifle','grenade','cannon','melee'][id%4],ammo:id%3?30:6,
        ordnance:id===3?null:{base:['bolt','grenade','shell','sticky'][id%4],Damage:5+id*7,VehicleScale:.5,
          ExplosionImpact:{Damage:10,DamageRadiusInner:1,DamageRadiusOuter:4}},
        ai:{minRange:id%3,maxRange:20+id*15,extremeRange:180,implicitRange:id%2===0,categoryMask:0xffffffff,action:id%2?3:6}}}));
    const arsenal={weapons:pool.slice(0,7),selected:0,
      current(channel){log('current',channel);return tick%37===0?null:this.weapons[this.selected];},
      select(weapon){log('select',weapon.id);if(tick%29===0)return false;this.selected=this.weapons.indexOf(weapon);return true;}};
    const unit=c.unit={kind:0,hero:false,combat:{arsenal},classDef:{MaxSpeed:4},
      origin(p){log('origin');p.set(.25,1.5,-0);if(tick%31===0)nested();}};
    c.perception=new Perception(c);if(native)sim.bindPerception(c.perception);
    const target={alive:true,generation:0,kind:0,category:0,maxHealth:100,velocity:new Vector3(),position:new Vector3(),
      aimPoint(p){log('aim');p.copy(this.position);if(tick%47===0)nested();},
      get sphere(){log('sphere');if(!changed){changed=true;if(tick%17===0)arsenal.weapons.push(pool[7]);else if(tick%19===0)arsenal.weapons.splice(1,1);}return {radius:1};}};
    const innerUnit={kind:0,combat:{arsenal:{weapons:[pool[0]]}},classDef:{MaxSpeed:3},origin:p=>{log('nestedOrigin');p.set(1,2,3);}};
    const innerTarget={kind:0,category:0,maxHealth:5,sphere:{radius:0},aimPoint:p=>{log('nestedAim');p.set(9,8,7);}};
    function nested(){const value=selectWeapon(c,innerUnit,innerTarget,false,world,{});log('nestedChoice',value.index);}
    if(native)sim.cluster(target,true);else clusterState(target,true);
    return {history,world,out,c,unit,target,pool,arsenal,snapshot,fireSnapshot,setTick:i=>{tick=i;changed=false;}};
  }
  const j=fixture(false),n=fixture(true);
  for(let i=0;i<3072;i++) {
    for(const x of [j,n]) {
      x.setTick(i);x.history.length=0;x.world.time=i%43===0?NaN:i%41===0?-0:i/7;
      x.unit.kind=[0,2,3,6,8][Math.floor(i/11)%5];x.unit.hero=i%23===0;x.unit.classDef.MaxSpeed=i%71===0?NaN:(i%13)/2;
      x.c.rangeScale=[0,.5,1,2,8,NaN][i%6];x.c.roll=i%53===0?undefined:(i%21)-10;
      x.target.generation=Math.floor(i/97);x.target.kind=[0,2,3,6,8,9][i%6];x.target.category=i%5;x.target.trap=i%59===0;
      x.target.maxHealth=[5,100,1000,Infinity][i%4];x.target.velocity.set(i%2?20:0,0,0);x.target.position.set((i%17)/3,i%5,2+i%210);
      const cluster=clusterState(x.target,true);cluster.flaggedAt=i%11<3?x.world.time-2:-Infinity;
      x.arsenal.weapons=x.pool.slice(0,i%79===0?0:7);x.arsenal.selected=i%7;
      const fire=x.c.perception.fire;fire.channel=i%2;fire.lastRelease=9;fire.charging=true;fire.sniperActive=true;fire.burst.state=2;
      for(const w of x.pool){w.clip=(i+w.id)%13===0?0:12;w.reserve=(i+w.id)%13===0?0:i%61===0?Infinity:24;w.rounds=i%67===0?0:1+w.id%3;w.bottomless=(i+w.id)%101===0;}
      const result=selectWeapon(x.c,x.unit,i%73===0?null:x.target,i%3!==0,x.world,x.out);assert.equal(result,x.out);
    }
    assert.deepEqual(n.snapshot(),j.snapshot(),`weapon choice ${i}`);
    assert.deepEqual(n.history,j.history,`weapon callbacks ${i}`);
    assert.deepEqual(n.fireSnapshot(),j.fireSnapshot(),`switch state ${i}`);
    assert.deepEqual(n.arsenal.weapons.map(w=>w.id),j.arsenal.weapons.map(w=>w.id));
    assert.equal(n.arsenal.selected,j.arsenal.selected);
  }
});

test('native scheduler preserves callback order, reentry and changing controller collections', async () => {
  const {sim}=await pair();
  function fixture(native) {
    const ai={tick:0,world:{networkViews:false},controllers:[],history:[]};
    const state=()=>[ai.scheduler.counter,ai.scheduler.parity,ai.scheduler.inputParity,ai.scheduler.skips,...Object.values(ai.scheduler.stats)];
    const log=(...args)=>ai.history.push([...args,state()]);
    const camera=(x,z)=>({matrixWorld:{elements:[1,0,0,0,0,1,0,0,0,0,1,0,x,0,z,1]},zoom:1,near:.1,far:200,fov:60,aspect:1.6});
    ai.cameras=()=>{log('cameras');return ai.tick%29===0?[]:[camera(0,0),camera(100,50)];};
    ai.lodDisabled=()=>{log('disabled');return ai.tick%37===0;};
    ai.identity=u=>{log('identity',u.id);return u.id;};
    ai.schedulerNumbers=()=>{log('config');return {cycle:ai.tick%71<35?3:15,losBudget:ai.tick%71<35?12:6};};
    ai.humanUnits=()=>{log('humans');return [{id:501},{id:502}];};
    ai.vehicles={notify(){log('notify');if(ai.tick%13===0)ai.controllers.reverse();}};
    ai.nav={plannerIdle(){log('idle');return ai.tick%3===0;},service(budget){
      log('nav',budget);
      if(native) {sim.words(new Array(21000).fill(7));sim.x.ai_scheduler_read(0);}
    }};
    ai.combat={
      broadcastAcoustic:u=>log('broadcast',u.id),rebuildProtected:()=>log('protected'),
      dropLosRequests:c=>log('drop',c.id),scan:c=>log('scan',c.id),clearCandidate:c=>log('clear',c.id),
      serviceLos:(_world,budget)=>{log('los',budget);return ai.tick%4===0;},
    };
    const make=id=>({id,processed:false,lodSkipped:false,live:true,
      unit:{id,position:new Vector3(id%3?60+id:0,0,id%2?-70:70),velocity:new Vector3(id%5===0?10:0,0,0)},
      update(dt,phase){
        log('phase',id,dt,phase);
        if(ai.tick===5 && phase===0 && id===0) {const gone=ai.controllers.splice(1,1)[0];gone.live=false;}
        if(ai.tick===14 && phase===0 && id===4)ai.controllers.push(make(30));
        if(ai.tick===18 && phase===0 && id===6)this.live=false;
      },
    });
    ai.controllers=Array.from({length:18},(_,id)=>make(id));
    ai.scheduler=native?sim.scheduler(ai):new ClassicScheduler(ai);
    ai.scheduler.executeInput=(c,dt)=>{
      log('input',c.id,dt);
      if(ai.tick===9 && c.id===2)ai.controllers.splice(3,1);
      if(ai.tick===11 && c.id===3)ai.controllers.push(make(31));
    };
    return ai;
  }
  const j=fixture(false),n=fixture(true);
  const snapshot=ai=>({counter:ai.scheduler.counter,parity:ai.scheduler.parity,inputParity:ai.scheduler.inputParity,
    skips:ai.scheduler.skips,stats:{...ai.scheduler.stats},controllers:ai.controllers.map(c=>[c.id,c.processed,c.lodSkipped,c.live])});
  for(let tick=0;tick<256;tick++) {
    j.tick=n.tick=tick;j.world.networkViews=n.world.networkViews=tick%2===0;
    j.scheduler.update(tick%17===0?0:1/60);n.scheduler.update(tick%17===0?0:1/60);
    j.scheduler.executeInputs(1/60);n.scheduler.executeInputs(1/60);
    assert.deepEqual(n.history,j.history,`callbacks at ${tick}`);
    assert.deepEqual(snapshot(n),snapshot(j),`state at ${tick}`);
    j.history.length=n.history.length=0;
  }
});

test('native scheduler inputs preserve carry, callback writes and both navigation fallbacks', async () => {
  const {sim}=await pair();
  function fixture(native) {
    const ai={world:{nav:{}},controllers:[],history:[],kind:0};
    ai.identity=u=>{if(ai.kind%41===0)ai.controllers[0].inputCarry=.2;return u.id;};
    ai.lodDisabled=()=>ai.kind%11!==0;
    ai.cameras=()=>[{matrixWorld:{elements:[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}}];
    ai.combat={
      markSkip(c,on){ai.history.push(['skip',on,c.inputCarry]);if(ai.kind%43===0)c.inputCarry=.125;},
      consumeFire(c,u,dt,intent){
        ai.history.push(['fire',dt]);intent.buttons=[dt,-dt];
        if(ai.kind%7===0)c.exitRequest=true;
        if(ai.kind%23===0)u.intent={replaced:true};
      },
      aimPoint(c,_u,dt,out){ai.history.push(['aim',dt]);if(ai.kind%13===0)c.dodge=-1;return ai.kind%4===0?null:out.copy(c.aimPoint);},
    };
    ai.world.nav.control=c=>{ai.history.push(['control']);return ai.kind%3===0?{stance:c.stance}:null;};
    ai.scheduler=native?sim.scheduler(ai):new ClassicScheduler(ai);
    return ai;
  }
  const j=fixture(false),n=fixture(true);
  function setup(ai,i) {
    ai.kind=i;ai.history.length=0;
    const soldier=i%9===0?null:{aimYaw:i*.07,aimPitch:i*.001,maxSpeed:i%7===0?0:3.3,maxStrafe:i%4===0?0:2.5,
      maxTurn:i%13===0?0:2.7,posture:['stand','crouch','prone',''][i%4],mode:i%2?12:7,rolled:i%3?false:undefined};
    const intent={buttons:[1,-1],forward:.7,right:-0,turn:.5,pitch:.25,jump:i%31===0,crouch:i%37===0,prone:false,use:false,exit:false};
    const unit={id:i%19,kind:[0,1,3,5,6,7][i%6],position:new Vector3(i%13,0,i%2?70:-70),velocity:new Vector3(i%17===0?10:0,0,0),
      soldier,classDef:{MaxPitchSpeed:i%5===0?0:.7},intent};
    const c={unit,live:true,inputSkip:i%7===0,inputCarry:i%5/60,exitRequest:i%19===0,facing:i%2===0,
      request:{valid:i%4!==0,steeringTarget:new Vector3(i%3?13:unit.position.x,0,14)},aimPoint:new Vector3(19,i%11,25),
      stance:i%3,wantedMode:i%3,dodge:[-1,0,1,undefined,NaN][i%5],jet:{pending:i%6===0,vector:new Vector3(i%12?2:0,0,0)}};
    ai.controllers=[c];ai.scheduler.inputParity=i%2;
    ai.navigationInput=i%3===0?(control,u,target,dt,aim,options)=>{
      assert.equal(u,unit);assert.equal(target,intent);ai.history.push(['nav',dt,aim?.toArray()??null,options.facing]);
      target.forward=.3;target.right=-.2;target.turn=dt;target.pitch=-dt;c.jet.pending=false;
    }:null;
    ai.analogs=i%3===1?(u,request,aim,dt,out,options)=>{
      assert.equal(u,unit);assert.equal(request,c.request);ai.history.push(['analogs',dt,aim?.toArray()??null,options.facing]);
      out.forward=dt;out.right=-dt;out.turn=.4;out.pitch=-.3;
    }:null;
    return {c,unit,intent};
  }
  const snapshot=({c,unit,intent},s)=>({inputSkip:c.inputSkip,inputCarry:c.inputCarry,exitRequest:c.exitRequest,dodge:c.dodge,
    jetPending:c.jet.pending,jet:c.jet.vector.toArray(),jetAlias:intent.jetDirection===c.jet.vector,intent,unitIntent:unit.intent,analogs:{...s.analogOut}});
  for(let i=0;i<1536;i++) {
    const a=setup(j,i),b=setup(n,i),dt=i%17===0?0:1/60;
    assert.equal(n.scheduler.executeInput(b.c,dt),j.scheduler.executeInput(a.c,dt),`return ${i}`);
    assert.deepEqual(n.history,j.history,`callbacks ${i}`);
    assert.deepEqual(snapshot(b,n.scheduler),snapshot(a,j.scheduler),`input ${i}`);
  }
});

test('native AI geometry and hints match the source with exact query outputs', async () => {
  const { js, native } = await pair();
  const random = new ClassicRandom(761);
  const sample = () => point(random.range(-24,24),random.range(-2,2),random.range(-16,16));
  const own = [{},{}];
  for (let i = 0; i < 512; i++) {
    const a = sample(), b = sample(), r = random.range(0,3), mask = i % 4;
    const j = js.barriers, n = native.barriers;
    j.work = { count: i, cap: 17 }; n.work = { count: i, cap: 17 };
    const jo = {}, no = {};
    assert.equal(n.pointClear(a,r,mask,no),j.pointClear(a,r,mask,jo));
    assert.equal(no.barrier?.index,jo.barrier?.index);
    assert.equal(n.segmentBlocked(a,b,r,mask),j.segmentBlocked(a,b,r,mask));
    assert.deepEqual(n.work,j.work);
    assert.equal(n.nearest(a,b,r,mask,no),j.nearest(a,b,r,mask,jo));
    assert.equal(no.barrier?.index,jo.barrier?.index); assert.equal(no.distanceSq,jo.distanceSq);
    assert.deepEqual(n.work,j.work);
    const jp={...a}, np={...a};
    assert.equal(n.correct(np,r,mask),j.correct(jp,r,mask)); assert.deepEqual(np,jp);
    const je={...b}, ne={...b}, jn=point(-0,4,7), nn={...jn};
    assert.equal(n.obstruct(a,ne,r,mask,nn),j.obstruct(a,je,r,mask,jn));
    assert.deepEqual(ne,je); assert.deepEqual(nn,jn); assert.deepEqual(n.work,j.work);
    assert.equal(native.outside(a),js.outside(a)); assert.equal(native.boundary.empty,js.boundary.empty);
    if (i % 7 === 0) {
      const index = i % js.hints.list.length;
      js.hints.reserve(js.hints.list[index], own[0],i/16);
      native.hints.reserve(native.hints.list[index],own[1],i/16);
      assert.equal(native.hints.list[index].reservedBy,own[1]);
    }
    const jq=js.queryHints({mask:7,center:a,halfExtent:8,cap:i%9,requester:own[0],now:i/16});
    const nq=native.queryHints({mask:7,center:a,halfExtent:8,cap:i%9,requester:own[1],now:i/16});
    assert.deepEqual(nq.map(h=>h.index),jq.map(h=>h.index));
    assert.deepEqual(native.hints.list.map(h=>h.reservedAt),js.hints.list.map(h=>h.reservedAt));
  }
});

const nodes = p => p ? p.nodes.map(n=>[n.x,n.y,n.z,n.flag]) : null;
function owner(tag, history) {
  return { tag, serial: 1, disposed: false, segments: 0, published: null, ready: false,
    publish(path, ready) {
      history.push([tag,nodes(path),ready]);
      if (this.published && this.published !== path) this.published.release();
      this.published=path; this.ready=ready;
    } };
}
function plannerView(nav, owners) {
  const p=nav.planner;
  return { idle:p.idle,served:p.served,work:{...p.work},current:p.current?.owner.tag ?? null,
    pools:[p.pools.paths.capacity,p.pools.paths.used,p.pools.nodes.capacity,p.pools.nodes.used],
    queue:p.queue.map(r=>({...r,owner:r.owner.tag})),
    owners:owners.map(o=>[o.serial,o.disposed,o.segments,o.ready,nodes(o.published)]) };
}

test('native path planner preserves queue order, cancellation and pool accounting', async () => {
  const {js,native}=await pair({pools:{paths:10,pathNodes:31,requests:4,searchNodes:48,heap:24}});
  const jh=[],nh=[],jo=Array.from({length:7},(_,i)=>owner(i,jh)),no=Array.from({length:7},(_,i)=>owner(i,nh));
  const random=new ClassicRandom(912);
  for(let i=0;i<320;i++) {
    const index=i%7,kind=i%8;
    if(kind<4) {
      const a=point(random.range(-15,20),0,random.range(-9,9)),b=point(random.range(-15,20),0,random.range(-9,9));
      const priority=random.int(0,14); jo[index].segments=no[index].segments=i%13;
      assert.equal(native.planner.request(no[index],a,b,.4,3,priority),js.planner.request(jo[index],a,b,.4,3,priority));
    } else if(kind===4) {
      jo[index].serial++;no[index].serial++;
      native.planner.cancel(no[index]);js.planner.cancel(jo[index]);
    } else if(kind===5) { native.planner.promote(no[index]);js.planner.promote(jo[index]); }
    else {
      const budget=kind===6 ? .2 : 1.3;
      native.service(budget);js.service(budget);
    }
    assert.deepEqual(nh,jh,`publications after ${i}`);
    assert.deepEqual(plannerView(native,no),plannerView(js,jo),`planner after ${i}`);
  }
  for(let i=0;i<7;i++) { native.planner.cancel(no[i]);js.planner.cancel(jo[i]); }
  assert.deepEqual(plannerView(native,no),plannerView(js,jo));
});

test('native path mirrors keep executor flag writes and live node arrays', async () => {
  const {js,native}=await pair();
  const jp=js.planner.segment(point(-12,0,0),point(-12,0,8)), np=native.planner.segment(point(-12,0,0),point(-12,0,8));
  const jc=new NavControl(js),nc=new NavControl(native);
  const unit={kind:0,position:point(-12,0,0),velocity:point()};
  const je=new PathExecutor(jp,jc,unit),ne=new PathExecutor(np,nc,unit);
  assert.deepEqual(nodes(np),nodes(jp));
  const ja=jp.nodes,na=np.nodes;
  for(let i=0;i<40;i++) {
    const jnext=js.planner.segment(point(-12,0,i+8),point(-12,0,i+9));
    const nnext=native.planner.segment(point(-12,0,i+8),point(-12,0,i+9));
    je.append(jnext,unit);ne.append(nnext,unit);
    assert.equal(np.nodes,na);assert.equal(jp.nodes,ja);
    assert.deepEqual(nodes(np),nodes(jp));
    assert.equal(native.planner.pools.nodes.used,js.planner.pools.nodes.used);
  }
  // Force a reread of the native path to prove facade flag writes reached
  // its authoritative nodes, rather than merely matching two JS mirrors.
  np.read();assert.deepEqual(nodes(np),nodes(jp));
  np.release();jp.release();
  assert.equal(native.planner.pools.nodes.used,js.planner.pools.nodes.used);
  assert.equal(native.planner.pools.paths.used,js.planner.pools.paths.used);
});

test('native graph routing retains shared RNG draws, identities and diagnostics', async () => {
  const json=config();
  json.graph={info:[3,3,5],hubs:[
    {name:'a',position:[0,0,0],radius:3,arcs:[0,1,255,255,255,255,255,255]},
    {name:'b',position:[12,0,0],radius:2,arcs:[0,2,255,255,255,255,255,255]},
    {name:'c',position:[6,0,12],radius:4,arcs:[1,2,255,255,255,255,255,255]},
  ],arcs:[{name:'ab',hubs:[0,1],mask:31},{name:'ac',hubs:[0,2],mask:31},{name:'bc',hubs:[1,2],mask:31}],
  routes:{tables:5,count:45,byteOffset:0}};
  const words=new Uint32Array(45);words.fill((15<<3) | (((16<<3)|1)<<8));
  const js=new ClassicNav(json,words.buffer),native=new ClassicNav(json,words.buffer);
  attachAiSim(await loadSim()).bindNavigation(native,json,words.buffer);
  const jr=new ClassicRandom(367),nr=new ClassicRandom(367),sample=new ClassicRandom(611);
  const j=js.graph,n=native.graph;
  for(let i=0;i<512;i++) {
    const from=i%3,to=(i+1)%3,previous=i%4 ? i%3 : -1,mask=[1,2,4,8,16,0,32][i%7];
    const ja=j.select(j.arcs[previous]??null,j.hubs[from],j.hubs[to],mask,jr);
    const na=n.select(n.arcs[previous]??null,n.hubs[from],n.hubs[to],mask,nr);
    assert.equal(na?.index,ja?.index); assert.equal(nr.state,jr.state);
    assert.equal(n.alternate(n.hubs[from],n.arcs[previous]??null,mask,nr)?.index,j.alternate(j.hubs[from],j.arcs[previous]??null,mask,jr)?.index);
    assert.equal(nr.state,jr.state);
    const p=point(sample.range(-6,18),sample.range(-3,3),sample.range(-6,18));
    const a=j.attach(p,mask),b=n.attach(p,mask);
    assert.deepEqual([b.type,b.hub?.index,b.arc?.index],[a.type,a.hub?.index,a.arc?.index]);
    assert.equal(n.nearestArc(p,mask)?.index,j.nearestArc(p,mask)?.index);
    assert.deepEqual(native.issues,js.issues);
  }
});

function controlView(c) {
  const p = v => v ? [v.x,v.y,v.z] : null;
  const e = c.executor;
  return { generation:c.generation,serial:c.serial,disposed:c.disposed,ready:c.ready,segments:c.segments,
    flags:[...c.installFlags],landing:c.landing,stance:c.stance,jet:[c.jet.pending,p(c.jet.vector),c.jetting],
    avoid:c.avoidSide,stranded:[c.stranded,c.strandedFor,p(c.strandedAt)],leader:c.leader?.unit.tag??null,
    trail:nodes(c.trail),trailDelay:c.trailDelay,published:nodes(c.published),
    request:[c.request.valid,p(c.request.steeringTarget),p(c.request.waypoint),p(c.request.segmentDirection),c.request.mode,c.request.lower,c.request.upper],
    executor:e ? { nodes:nodes(e.path),index:e.index,prev:p(e.prev),waypoint:p(e.waypoint),next:p(e.next),final:p(e.final),direction:p(e.direction),
      mode:e.mode,side:e.side,complete:e.complete,status:e.status,anchor:p(e.anchor),stationary:e.stationary,best:e.best,noProgress:e.noProgress,hold:e.hold,target:p(e.target) } : null,
    following:c.following,status:c.status };
}

test('native navigation controls retain executors, trails, contacts and life changes', async () => {
  const {js,native}=await pair();
  const unit = tag => ({tag,kind:0,generation:1,alive:true,position:new Vector3(-12,0,0),velocity:new Vector3(0,0,3),
    soldier:{aimYaw:-0,avoidance:new Vector3()},forward:out=>out.set(.6,0,.8)});
  const ju=unit('unit'),nu=unit('unit'),jl=unit('leader'),nl=unit('leader');
  const jw={randomB:new ClassicRandom(367)},nw={randomB:new ClassicRandom(367)};
  const jc=js.control({unit:ju,world:jw}),nc=native.control({unit:nu,world:nw});
  assert.equal(nc.nativeControl,true);
  const compare = label => {
    assert.deepEqual(controlView(nc),controlView(jc),label);
    assert.equal(nw.randomB.state,jw.randomB.state,label+' stream B');
    assert.deepEqual(nu.soldier.avoidance,ju.soldier.avoidance,label+' contact');
    assert.deepEqual([native.planner.pools.paths.used,native.planner.pools.nodes.used],[js.planner.pools.paths.used,js.planner.pools.nodes.used],label+' pools');
  };
  jc.attach(ju);nc.attach(nu); compare('attach');
  for(let i=0;i<240;i++) {
    const z=i/12;
    ju.position.set(-12,0,z);nu.position.copy(ju.position);
    jl.position.set(-10+Math.sin(i/9),0,z+4);nl.position.copy(jl.position);
    if(i%30===0) {
      const a=point(-12,0,z),b=point(-12,0,z+3);
      jc.install(js.planner.segment(a,b));nc.install(native.planner.segment(a,b));
    } else if(i%30===4) {
      const a=point(-12,0,z+3),b=point(-11,0,z+5);
      jc.append(js.planner.segment(a,b));nc.append(native.planner.segment(a,b));
    } else if(i%30===6) { jc.executor.hold=true;nc.executor.hold=true; }
    else if(i%30===8) { jc.executor.hold=false;nc.executor.hold=false; }
    else if(i%30===10) {
      jc.setLeader({unit:jl,generation:1});nc.setLeader({unit:nl,generation:1});
      jc.recordTrail(true);nc.recordTrail(true);
    } else if(i%30===15) { jc.recordTrail(false);nc.recordTrail(false); }
    else if(i%30===20) { jc.setLeader(null);nc.setLeader(null);jc.strand();nc.strand(); }
    else if(i%30===25) { jc.stop();nc.stop(); }
    if(i%17===0) { ju.soldier.avoidance.set(1,0,-.25);nu.soldier.avoidance.copy(ju.soldier.avoidance); }
    jc.update(1/60);nc.update(1/60); compare(`step ${i}`);
    if(i===119) { ju.generation++;nu.generation++;jc.attach(ju);nc.attach(nu);compare('new life'); }
  }
  jc.reset();nc.reset(); compare('reset');
  jc.request.write(point(-0,0,1),point(1,2,3),point(0,0,1),2,-1,1);
  nc.request.write(point(-0,0,1),point(1,2,3),point(0,0,1),2,-1,1);
  nc.read();compare('shared request writes');
  jc.dispose();nc.dispose();compare('dispose');
});

test('native aim filters keep exact curves, write-through vectors and shared stream C', async () => {
  const {sim}=await pair();
  const j=new Aimer(),n=sim.aimer(),sample=new ClassicRandom(912);
  const classicView=a=>({direction:a.direction.toArray(),velocity:a.velocity.toArray(),started:a.started,curves:{...a.curves}});
  const desired=new Vector3();
  for(let i=0;i<1024;i++) {
    desired.set(sample.range(-1,1),sample.range(-1,1),sample.range(-1,1)).normalize();
    const curves=i%7===0 ? null : i%19===0 ? {A:NaN,D:0} : skillCurves(1+i%20);
    const base=['cannon','launcher','melee','repair',undefined][i%5];
    const jw=i%11===0 ? null : {def:{base},spreadScale:31,sniperScale:-0};
    const nw=jw?{def:{base},spreadScale:31,sniperScale:-0}:null;
    const dt=i%17===0 ? 0 : 1/60,hero=i%23===0;
    assert.equal(n.update(desired,dt,curves,nw,hero),n.direction);
    j.update(desired,dt,curves,jw,hero);
    assert.deepEqual(classicView(n),classicView(j),`Classic ${i}`);assert.deepEqual(nw,jw);
    if(i%37===0) {
      n.direction.set(-0,.5,1);j.direction.set(-0,.5,1);
      n.velocity.set(1,-0,-1);j.velocity.set(1,-0,-1);
      n.started=false;j.started=false;n.read();assert.deepEqual(classicView(n),classicView(j));
    }
  }
  const enhancedView=a=>({direction:a.direction.toArray(),started:a.started,error:[...a.error],bias:[...a.bias],
    key:a.key,lastKey:a.lastKey,lostAt:a.lostAt,acquiredAt:a.acquiredAt,readyAt:a.readyAt});
  for(const tier of [1,2,3]) {
    const jr=new ClassicRandom(675),nr=new ClassicRandom(675),profile=AIM_PROFILES[tier];
    const je=new EnhancedAimer(jr,profile,1.07),ne=sim.enhancedAimer(nr,profile,1.07);
    for(let i=0;i<768;i++) {
      const now=i/60,target=i%71<12 ? null : {id:i%47<3?undefined:Math.floor(i/101),generation:1+i%2};
      ne.observe(target,now);je.observe(target,now);
      assert.equal(ne.reacting(now),je.reacting(now));
      desired.set(sample.range(-1,1),sample.range(-1,1),sample.range(-1,1)).normalize();
      const options={distance:(i%240)/2,speedFraction:(i%19)/18,scoped:i%3===0,now,hero:i%41===0};
      assert.equal(ne.sigma(options.distance,options.speedFraction,options.scoped),je.sigma(options.distance,options.speedFraction,options.scoped));
      const dt=i%31===0?0:1/60;
      assert.equal(ne.update(desired,dt,options),ne.direction);je.update(desired,dt,options);
      if(i%17===0)assert.equal(ne.gaussian(),je.gaussian());
      assert.deepEqual(enhancedView(ne),enhancedView(je),`Enhanced ${tier}:${i}`);
      assert.equal(nr.state,jr.state,`stream C ${tier}:${i}`);
    }
  }
});

test('native navigation input preserves analogs, jet aliases and existing intent edges', async () => {
  const {js,native}=await pair();
  const random=new ClassicRandom(618);
  for(const kind of [0,1,2,3,5,6]) {
    const unit=()=>({kind,generation:1,position:new Vector3(),velocity:new Vector3(),
      classDef:{MaxSpeed:9,MaxStrafeSpeed:7,MaxTurnSpeed:2,MaxPitchSpeed:.5,ForwardSpeed:20,TurnRate:2},
      soldier:kind===0||kind===3 ? {aimYaw:0,aimPitch:0,maxSpeed:9,maxStrafe:7,maxTurn:2,maxPitch:.5,air:null,posture:'stand',lock:0,dive:false,avoidance:new Vector3()} : null,
      forward:out=>out.set(.6,0,.8),origin(out){return out.copy(this.position).add(new Vector3(0,1.5,0));}});
    const ju=unit(),nu=unit();
    const jc=js.control({unit:ju,world:{randomB:new ClassicRandom(71)}}),nc=native.control({unit:nu,world:{randomB:new ClassicRandom(71)}});
    assert.equal(typeof nc.nativeInput,'function');jc.attach(ju);nc.attach(nu);
    for(let i=0;i<192;i++) {
      ju.position.set(random.range(-5,5),random.range(-1,1),random.range(-5,5));nu.position.copy(ju.position);
      ju.velocity.set(random.range(-4,4),0,random.range(-4,4));nu.velocity.copy(ju.velocity);
      if(ju.soldier) {
        Object.assign(ju.soldier,{aimYaw:random.range(-3,3),aimPitch:random.range(-1,1),air:[null,'jump','jet','fall'][i%4],posture:['stand','crouch','prone'][i%3],lock:i%11===0?1:0,dive:i%17===0});
        Object.assign(nu.soldier,{...ju.soldier,avoidance:nu.soldier.avoidance});
      }
      if(i%13===0) {
        const end=ju.position.clone().add(new Vector3(0,0,10));
        jc.install(js.planner.segment(ju.position,end));nc.install(native.planner.segment(nu.position,end));
      } else if(i%13===7) {jc.stop();nc.stop();}
      jc.stance=nc.stance=i%3;
      jc.jet.pending=nc.jet.pending=i%19===0;
      const jet=i%2===0 ? new Vector3() : new Vector3(0,1,-0);
      jc.jet.vector.copy(jet);nc.jet.vector.copy(jet);
      const aim=i%5===0?null:new Vector3(random.range(-8,8),random.range(-2,8),random.range(-8,8));
      const ji={jump:i%7===0,crouch:false,prone:false,jetDirection:'previous'},ni={...ji};
      const dt=i%31===0?0:1/60,options={facing:i%3===0};
      navigationInput(jc,ju,ji,dt,aim,options);navigationInput(nc,nu,ni,dt,aim,options);
      assert.deepEqual(ni,ji,`intent ${kind}:${i}`);
      if(ji.jetDirection===jc.jet.vector)assert.equal(ni.jetDirection,nc.jet.vector);
      assert.deepEqual(controlView(nc),controlView(jc),`control ${kind}:${i}`);
    }
  }
});
