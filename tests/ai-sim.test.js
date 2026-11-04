import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { ClassicNav, NavControl, PathExecutor, navigationInput } from '../src/ai/classic/nav/index.js';
import { ClassicRandom } from '../src/ai/random.js';
import { attachAiSim } from '../src/ai-sim.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { loadSim } from '../src/sim/module.js';
import { Aimer, skillCurves } from '../src/ai/classic/aim.js';
import { AIM_PROFILES, EnhancedAimer } from '../src/ai/enhanced/aim.js';

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
