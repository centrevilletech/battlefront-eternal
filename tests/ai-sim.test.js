import test from 'node:test';
import assert from 'node:assert/strict';
import { ClassicNav, NavControl, PathExecutor } from '../src/ai/classic/nav/index.js';
import { ClassicRandom } from '../src/ai/random.js';
import { attachAiSim } from '../src/ai-sim.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { loadSim } from '../src/sim/module.js';

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
