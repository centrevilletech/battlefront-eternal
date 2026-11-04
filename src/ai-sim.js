// The AI uses the battle's existing simulation instance and bridge.
// Navigation's planner, path pool, hints and geometry live in Rust;
// controller facades can keep their answer slots while callers migrate.
import { bridgeFor } from './sim/runtime.js';
import { NavControl } from './ai/classic/nav/control.js';
import { Vector3 } from 'three';

const HEADER = 5;
const ZERO = { x: 0, y: 0, z: 0 };

// Static content only, once per battle. Numbers cross as f64 words so
// signed zero and non-finite settings are never changed by JSON text.
function contentWords(v, strings, out) {
  if (v == null) out.push(0);
  else if (v === false) out.push(1);
  else if (v === true) out.push(2);
  else if (typeof v === 'number') out.push(3, v);
  else if (typeof v === 'string') out.push(4, strings.id(v));
  else if (Array.isArray(v) || ArrayBuffer.isView(v)) {
    out.push(5, v.length);
    for (const item of v) contentWords(item, strings, out);
  } else {
    const keys = Object.keys(v);
    out.push(6, keys.length);
    for (const key of keys) { out.push(strings.id(key)); contentWords(v[key], strings, out); }
  }
}

export function attachAiSim(instance) {
  return new AiSim(bridgeFor(instance));
}

export class AiSim {
  constructor(bridge) {
    this.bridge = bridge;
    this.x = bridge.x;
    this.ids = new WeakMap();
    this.objects = [null];
    this.navigation = null;
  }

  id(object) {
    if (!object) return 0;
    let id = this.ids.get(object);
    if (id === undefined) { id = this.objects.length; this.objects.push(object); this.ids.set(object, id); }
    return id;
  }

  out() {
    const ptr = this.x.ai_nav_out(), length = this.x.ai_nav_out_len();
    this.bridge.views();
    return this.bridge.f.subarray(ptr / 8, ptr / 8 + length);
  }

  // Called before the AI binds, with the same authored map and binary
  // routing words ClassicNav received. Immutable view objects keep their
  // identity for existing agents; every mutable planner/path record is
  // native, and tactical reservation setters write through immediately.
  bindNavigation(nav, json, bin, options = {}) {
    if (this.navigation) throw Error('the battle already has native AI navigation');
    const words = [];
    const bytes = bin == null ? [] : bin instanceof ArrayBuffer ? new Uint8Array(bin) : new Uint8Array(bin.buffer, bin.byteOffset, bin.byteLength);
    contentWords({ nav: json, bin: bytes, pools: options.pools ?? {}, hintCapacity: options.hintCapacity,
      posts: (options.posts ?? []).map(p => [p.name, this.id(p), p.position.x, p.position.y, p.position.z]) }, this.bridge.strings, words);
    const ptr = this.x.sim_call_room(words.length, 0);
    this.bridge.views(); this.bridge.f.set(words, ptr / 8 + HEADER);
    const error = this.x.ai_nav_init();
    if (error) throw Error(this.bridge.strings.at(error - 1));
    const old = nav.barriers;
    nav.barriers = new NativeBarriers(this, old);
    const boundary = nav.boundary;
    Object.defineProperty(boundary, 'empty', { configurable: true, get: () => Boolean(this.x.ai_nav_boundary(2, 0, 0, 0, 0)) });
    boundary.contains = (index, point) => Boolean(this.x.ai_nav_boundary(1, index, point.x, point.y, point.z));
    boundary.outside = point => Boolean(this.x.ai_nav_boundary(0, 0, point.x, point.y, point.z));
    if (nav.graph) this.bindGraph(nav.graph);
    nav.planner = new NativePlanner(this, nav.barriers, options.pools ?? {});
    nav.nativeControl = controller => new NativeNavControl(nav, controller);
    nav.control = controller => controller.navControl ?? (controller.navControl = nav.nativeControl(controller));
    const hints = nav.hints;
    for (const hint of hints.list) {
      const index = hint.index;
      Object.defineProperties(hint, {
        reservedAt: { enumerable: true, configurable: true,
          get: () => { this.x.ai_nav_hint_read(index); return this.out()[0]; },
          set: value => { this.x.ai_nav_hint_read(index); const owner = this.out()[1]; this.x.ai_nav_hint_write(index, value, owner); } },
        reservedBy: { enumerable: true, configurable: true,
          get: () => { this.x.ai_nav_hint_read(index); return this.objects[this.out()[1]]; },
          set: value => { this.x.ai_nav_hint_read(index); const at = this.out()[0]; this.x.ai_nav_hint_write(index, at, this.id(value)); } },
      });
    }
    hints.query = (mask, center, half, cap = Infinity, out = [], requester = null, now = 0) => {
      this.x.ai_nav_hints(mask, center.x, center.y, center.z, half, cap, this.id(requester), now);
      const answer = this.out(); out.length = 0;
      for (const index of answer) out.push(hints.list[index]);
      return out;
    };
    this.navigation = nav;
    return nav;
  }

  bindGraph(graph) {
    graph.table = mask => {
      this.x.ai_nav_graph(0, -1, 0, 0, mask, 0, 0, 0, 0); return this.out()[0];
    };
    const route = (op, previous, from, to, mask, random) => {
      if (!Number.isInteger(random?.state)) throw Error('native AI routing requires the shared ClassicRandom state');
      this.x.ai_nav_graph(op, previous?.index ?? -1, from.index, to?.index ?? 0, mask, 0, 0, 0, random.state);
      const answer = this.out().slice(); random.state = answer[1];
      for (let i = 2; i < answer.length; i += 5) {
        if (answer[i] === 0) graph.report('routeTable', { mask: answer[i + 1] });
        else graph.report('routeHang', { hub: answer[i + 1], table: answer[i + 2], destination: answer[i + 3], word: answer[i + 4] });
      }
      return answer[0] < 0 ? null : graph.arcs[answer[0]];
    };
    graph.select = (previous, from, to, mask, random) => route(1, previous, from, to, mask, random);
    graph.alternate = (from, previous, mask, random) => route(2, previous, from, null, mask, random);
    graph.attach = (point, mask, out = { type: 0, hub: null, arc: null }) => {
      this.x.ai_nav_graph(3, -1, 0, 0, mask, point.x, point.y, point.z, 0);
      const answer = this.out(); out.type = answer[0];
      out.hub = answer[1] < 0 ? null : graph.hubs[answer[1]];
      out.arc = answer[2] < 0 ? null : graph.arcs[answer[2]];
      return out;
    };
    graph.nearestArc = (point, mask) => {
      this.x.ai_nav_graph(4, -1, 0, 0, mask, point.x, point.y, point.z, 0);
      const index = this.out()[0]; return index < 0 ? null : graph.arcs[index];
    };
  }

  bindCombat(ai) {
    const create = ai.combat.createPerception;
    ai.combat.createPerception = (...args) => {
      const perception = create(...args);
      perception.aimer = this.aimer();
      return perception;
    };
  }

  aimer() { return new NativeAimer(this); }
  enhancedAimer(random,profile,skill=1) { return new NativeEnhancedAimer(this,random,profile,skill); }

  words(values) {
    const ptr=this.x.sim_call_room(values.length,0);
    this.bridge.views();this.bridge.f.set(values,ptr/8+HEADER);
  }
}

// These facades keep the vector identities used by the gun and fire cone.
// Assignments from other AI services immediately update the native filter.
class AimFields {
  constructor(sim,id,read,write) { this.sim=sim;this.id=id;this.readExport=read;this.writeExport=write;this.fields=[];this.pulling=false; }
  field(object,name,index,decode=value=>value,encode=Number) {
    let value=object[name];
    Object.defineProperty(object,name,{enumerable:true,configurable:true,get:()=>value,set:next=>{
      value=next;if(!this.pulling)this.sim.x[this.writeExport](this.id,index,encode(next));
    }});
    this.fields.push([object,name,index,decode]);
  }
  vector(object,index) { ['x','y','z'].forEach((key,axis)=>this.field(object,key,index+axis)); }
  read() {
    this.sim.x[this.readExport](this.id);const values=this.sim.out().slice();this.pulling=true;
    try {for(const [object,name,index,decode] of this.fields)object[name]=decode(values[index]);}
    finally {this.pulling=false;}
  }
}

class NativeAimer extends AimFields {
  constructor(sim) {
    super(sim,sim.x.ai_aimer_new(),'ai_aimer_read','ai_aimer_write');
    this.direction=new Vector3(0,0,1);this.velocity=new Vector3();this.started=false;this.curves={A:0,D:0,B:1,S:0};
    this.vector(this.direction,0);this.vector(this.velocity,3);this.field(this,'started',6,Boolean);
    ['A','D','B','S'].forEach((key,index)=>this.field(this.curves,key,index+7));this.read();
  }
  run(op,desired,dt=0,curves=null,weapon=null,hero=false) {
    const def=weapon?.def;
    this.sim.words([desired.x,desired.y,desired.z,dt,Number(Boolean(hero)),Number(Boolean(curves)),
      typeof curves?.A==='number'?curves.A:NaN,curves?.D,Number(curves?.B!=null),curves?.B??0,Number(curves?.S!=null),curves?.S??0,
      Number(def!=null),this.sim.bridge.strings.id(typeof def?.base==='string'?def.base:'')]);
    this.sim.x.ai_aimer_update(this.id,op);
    const answer=this.sim.out();
    if(answer[0]) {weapon.spreadScale=answer[1];weapon.sniperScale=answer[2];}
    this.read();return this.direction;
  }
  reset(direction) {this.run(0,direction);}
  update(desired,dt,curves,weapon=null,hero=false) {return this.run(1,desired,dt,curves,weapon,hero);}
}

class NativeEnhancedAimer extends AimFields {
  constructor(sim,random,profile,skill) {
    super(sim,sim.x.ai_enhanced_aimer_new(profile.reaction,profile.reactionFloor,profile.convergence,profile.sigma25,profile.slew,skill),'ai_enhanced_aimer_read','ai_enhanced_aimer_write');
    this.random=random;this.profile=profile;this.skill=skill;this.direction=new Vector3(0,0,1);this.started=false;
    this.error=[0,0];this.bias=[0,0];this.key=null;this.lastKey=null;this.lostAt=-Infinity;this.acquiredAt=-Infinity;this.readyAt=-Infinity;
    this.vector(this.direction,0);this.field(this,'started',3,Boolean);
    this.field(this.error,0,4);this.field(this.error,1,5);this.field(this.bias,0,6);this.field(this.bias,1,7);
    for(const [name,index] of [['key',8],['lastKey',9]])this.field(this,name,index,value=>value?sim.bridge.strings.at(value-1):null,value=>value==null?0:sim.bridge.strings.id(value)+1);
    for(const [name,index] of [['lostAt',10],['acquiredAt',11],['readyAt',12]])this.field(this,name,index);
    this.read();
  }
  run(op,values) {
    while(values.length<9)values.push(0);
    this.sim.words(values);
    const result=this.sim.x.ai_enhanced_aimer_command(this.id,op,this.random.state);
    this.random.state=this.sim.out()[0];this.read();return result;
  }
  reset(direction) {this.run(0,[direction.x,direction.y,direction.z]);}
  observe(target,now) {this.run(1,[target?this.sim.bridge.strings.id(`${target.id}:${target.generation}`)+1:0,now]);}
  reacting(now) {return Boolean(this.run(2,[now]));}
  sigma(distance,speedFraction,scoped=false) {return this.run(3,[distance,speedFraction,Number(Boolean(scoped))]);}
  gaussian() {return this.run(5,[]);}
  update(desired,dt,{distance=25,speedFraction=0,scoped=false,now=0,hero=false}={}) {
    this.run(4,[desired.x,desired.y,desired.z,dt,distance,speedFraction,Number(Boolean(scoped)),now,Number(Boolean(hero))]);
    return this.direction;
  }
}

class NativeBarriers {
  constructor(sim, old) {
    this.sim = sim;
    this.list = old.list;
    this.work = null;
    this.found = [];
  }

  query(op, a, b, radius, mask) {
    const work = this.work;
    this.sim.x.ai_nav_geometry(op, a.x, a.y, a.z, b.x, b.y, b.z, radius, mask, work ? work.count : NaN, work?.cap ?? 0);
    const out = this.sim.out();
    if (work) work.count = out[9];
    return out;
  }

  gather(x0, z0, x1, z1, out = this.found) {
    this.sim.x.ai_nav_geometry(5, x0, 0, z0, x1, 0, z1, 0, 0, NaN, 0);
    const answer = this.sim.out(); out.length = 0;
    for (const index of answer) out.push(this.list[index]);
    return out;
  }

  pointClear(p, r, mask, out = null) {
    const answer = this.query(0, p, ZERO, r, mask);
    if (out) out.barrier = answer[1] < 0 ? null : this.list[answer[1]];
    return Boolean(answer[0]);
  }
  segmentBlocked(a, b, r, mask) { return Boolean(this.query(1, a, b, r, mask)[0]); }
  nearest(a, b, r, mask, out) {
    const answer = this.query(2, a, b, r, mask);
    out.barrier = answer[1] < 0 ? null : this.list[answer[1]]; out.distanceSq = answer[2];
    return Boolean(answer[0]);
  }
  correct(p, r, mask) {
    const answer = this.query(3, p, ZERO, r, mask);
    p.x = answer[3]; p.z = answer[5];
    return Boolean(answer[0]);
  }
  obstruct(a, b, r, mask, normal = null) {
    const answer = this.query(4, a, b, r, mask);
    b.x = answer[3]; b.y = answer[4]; b.z = answer[5];
    if (normal && answer[0]) { normal.x = answer[6]; normal.y = answer[7]; normal.z = answer[8]; }
    return Boolean(answer[0]);
  }
}

class NativePool {
  constructor(planner, offset) { this.planner = planner; this.offset = offset; }
  get capacity() { return this.planner.info()[this.offset]; }
  get used() { return this.planner.info()[this.offset + 1]; }
  get full() { const a = this.planner.info(); return a[this.offset + 1] >= a[this.offset]; }
  get free() { const a = this.planner.info(); return a[this.offset] - a[this.offset + 1]; }
}

class NativePath {
  constructor(planner, id) {
    this.planner = planner; this.id = id; this.pools = planner.pools;
    this.nodes = []; this.released = false;
    this.read();
  }
  node(x, y, z, initialFlag) {
    const node = { x, y, z }; let flag = initialFlag;
    Object.defineProperty(node, 'flag', { enumerable: true, configurable: true,
      get: () => flag,
      set: value => {
        flag = value;
        const index = this.nodes.indexOf(node);
        if (index >= 0) this.planner.sim.x.ai_nav_path(5, this.id, index, 0, 0, value);
      } });
    return node;
  }
  read() {
    this.planner.sim.x.ai_nav_path(0, this.id, 0, 0, 0, 0);
    const words = this.planner.sim.out();
    this.released = Boolean(words[0]); this.nodes.length = 0;
    for (let i = 1; i < words.length; i += 4) this.nodes.push(this.node(words[i], words[i + 1], words[i + 2], words[i + 3]));
  }
  prepend(x, y, z) {
    if (!this.planner.sim.x.ai_nav_path(1, this.id, x, y, z, 0)) return false;
    this.planner.sim.x.ai_nav_path(0, this.id, 0, 0, 0, 0);
    const a = this.planner.sim.out(); this.nodes.unshift(this.node(a[1], a[2], a[3], a[4]));
    return true;
  }
  append(node) {
    const { x, y, z } = node, flag = node.flag ?? 0;
    if (!this.planner.sim.x.ai_nav_path(2, this.id, x, y, z, flag)) return false;
    this.nodes.push(this.node(x, y, z, flag)); return true;
  }
  dropFront(count) {
    this.planner.sim.x.ai_nav_path(3, this.id, count, 0, 0, 0);
    if (count > 0) this.nodes.splice(0, count);
  }
  get length() { return this.nodes.length; }
  release() {
    if (this.released) return;
    this.planner.sim.x.ai_nav_path(4, this.id, 0, 0, 0, 0);
    this.released = true; this.nodes.length = 0;
  }
}

class NativePlanner {
  constructor(sim, barriers, options) {
    this.sim = sim; this.barriers = barriers; this.capacity = options.requests ?? 75;
    this.owners = []; this.ownerIds = new WeakMap(); this.paths = [];
    this.pools = { paths: new NativePool(this, 4), nodes: new NativePool(this, 6) };
  }
  owner(object) {
    let id = this.ownerIds.get(object);
    if (id === undefined) { id = this.sim.x.ai_nav_owner(); this.ownerIds.set(object, id); this.owners[id] = object; }
    return id;
  }
  feed() {
    this.owners.forEach((owner, id) => {
      if (!owner.nativeControl) this.sim.x.ai_nav_owner_feed(id, owner.serial, Boolean(owner.disposed), owner.segments,
        owner.published?.id ?? -1, Boolean(owner.ready));
    });
  }
  pathOf(id) {
    if (id < 0) return null;
    return this.paths[id] ?? (this.paths[id] = new NativePath(this, id));
  }
  run(op, owner = null, a = ZERO, b = ZERO, radius = 0, mask = 0, priority = 0) {
    const id = owner ? this.owner(owner) : 0;
    this.feed();
    this.sim.x.ai_nav_planner(op, id, a.x, a.y, a.z, b.x, b.y, b.z, radius, mask, priority);
    // Publishing a path can release its predecessor through another
    // export; copy these few words before that reentry uses the answer.
    const answer = this.sim.out().slice();
    this.publications(answer, 1);
    return answer[0];
  }
  publications(answer, start) {
    for (let i = start; i < answer.length; i += 3) {
      const owner = this.owners[answer[i]];
      if (owner.nativeControl) owner.read();
      else owner.publish(this.pathOf(answer[i + 1]), Boolean(answer[i + 2]));
    }
  }
  path() { return this.pathOf(this.run(4)); }
  segment(a, b) { return this.pathOf(this.run(5, null, a, b)); }
  request(owner, a, b, radius, mask, priority) { return Boolean(this.run(0, owner, a, b, radius, mask, priority)); }
  cancel(owner) { this.run(1, owner); }
  promote(owner) { this.run(2, owner); }
  service(budget = 1) { this.run(3, null, ZERO, ZERO, 0, 0, budget); }
  info() { this.sim.x.ai_nav_planner_info(); return this.sim.out(); }
  get idle() { return Boolean(this.info()[0]); }
  get served() { return this.info()[1]; }
  get current() { const id = this.info()[9]; return id < 0 ? null : { owner: this.owners[id] }; }
  get work() { const a = this.info(); return { count: a[2], cap: a[3] }; }
  get queue() {
    const a = this.info(), result = [];
    for (let i = 10; i < a.length; i += 12) result.push({ owner: this.owners[a[i]], serial: a[i + 1],
      start: { x: a[i + 2], y: a[i + 3], z: a[i + 4] }, end: { x: a[i + 5], y: a[i + 6], z: a[i + 7] },
      radius: a[i + 8], mask: a[i + 9], priority: a[i + 10], served: a[i + 11] });
    return result;
  }
}

// Unit reads used by the path executor. Optional numeric fields carry a
// presence bit so a NaN is distinct from an absent coefficient.
function navigationUnit(sim, words, unit, forward, origin = null) {
  if (!unit) { words.push(0); return; }
  const vec = p => words.push(p.x, p.y, p.z);
  const optional = value => words.push(value != null ? 1 : 0, value ?? 0);
  words.push(1, sim.id(unit), unit.kind); vec(unit.position); vec(unit.velocity ?? ZERO);
  optional(unit.generation);
  const def = unit.classDef; words.push(def ? 1 : 0);
  if (def) {
    for (const key of ['MaxSpeed','ForwardSpeed','TurnRate','MaxStrafeSpeed','MaxTurnSpeed','MaxPitchSpeed']) optional(def[key]);
  }
  const soldier = unit.soldier; words.push(soldier ? 1 : 0);
  if (soldier) {
    for (const key of ['aimYaw','aimPitch','maxSpeed','maxStrafe','maxTurn','maxPitch']) optional(soldier[key]);
    words.push(soldier.avoidance ? 1 : 0);
    if (soldier.avoidance) vec(soldier.avoidance);
    words.push(!soldier.air?0:soldier.air==='jump'?1:soldier.air==='jet'?2:3,
      soldier.posture==='stand'?0:soldier.posture==='crouch'?1:soldier.posture==='prone'?2:3,soldier.lock,Number(Boolean(soldier.dive)));
  }
  words.push(forward ? 1 : 0); if (forward) vec(forward);
  words.push(origin ? 1 : 0); if (origin) vec(origin);
}

class NativeNavControl extends NavControl {
  constructor(nav, controller) {
    super(nav, controller);
    this.sim = nav.planner.sim;
    this.id = this.planner.owner(this);
    this.nativeControl = true;
    this.pulling = false;
    this.fields = [];
    this.executors = new Map();
    const field = (object, name, index, decode = value => value, encode = Number) => {
      let value = object[name];
      Object.defineProperty(object, name, { enumerable: true, configurable: true,
        get: () => value,
        set: next => {
          value = next;
          if (!this.pulling) this.sim.x.ai_nav_control_write(this.id, index, encode(next));
        } });
      this.fields.push([object, name, index, decode, encode]);
    };
    field(this, 'unit', 0, value => this.sim.objects[value], value => this.sim.id(value));
    for (const [name,index] of [['generation',1],['serial',2],['segments',6],['stance',10],['avoidSide',13],['strandedFor',15],['trailDelay',20]]) field(this,name,index);
    for (const [name,index] of [['disposed',3],['ready',5],['landing',9],['jetting',12],['stranded',14]]) field(this,name,index,Boolean);
    for (const [name,index] of [['published',4],['trail',19]]) field(this,name,index,value=>this.planner.pathOf(value),value=>value?.id??-1);
    field(this.installFlags,0,7,Boolean); field(this.installFlags,1,8,Boolean);
    field(this.jet,'pending',11,Boolean);
    field(this.request,'valid',21,Boolean);
    for (const [name,index] of [['mode',22],['lower',23],['upper',24]]) field(this.request,name,index);
    for (const [object,index] of [[this.jet.vector,25],[this.strandedAt,28],[this.request.steeringTarget,31],[this.request.waypoint,34],[this.request.segmentDirection,37]]) {
      ['x','y','z'].forEach((key,axis)=>field(object,key,index+axis));
    }
    for (const [object,name,index,,encode] of this.fields) this.sim.x.ai_nav_control_write(this.id,index,encode(object[name]));
    this.read();
  }

  read() {
    const previous = [this.published,this.trail,this.executor?.path];
    this.sim.x.ai_nav_control_read(this.id);
    const words = this.sim.out().slice();
    this.pulling = true;
    try {
      for (const [object,name,index,decode] of this.fields) object[name] = decode(words[index]);
      this.leader = words[16] ? { unit:this.sim.objects[words[16]], generation:words[17] ? words[18] : undefined } : null;
      if (words[40] < 0) this.executor = null;
      else {
        let executor = this.executors.get(words[40]);
        if (!executor) { executor = new NativeExecutor(this,words[40]); this.executors.set(words[40],executor); }
        executor.read(words); this.executor = executor;
      }
    } finally { this.pulling = false; }
    for (const path of new Set([...previous,this.published,this.trail,this.executor?.path])) path?.read();
  }

  run(op, a = 0, b = 0, c = 0, unit = this.unit, explicitLeader = null, aim = null, facing = false, intent = null) {
    const words = [], world = this.controller?.world;
    const rng = world?.randomB ?? world?.gameRandom ?? world?.random;
    const ref = this.leader;
    const leader = explicitLeader ?? (ref?.unit?.alive && ref.unit.generation === ref.generation ? ref.unit : null);
    // These methods are pure unit reads. Only operations that can steer
    // request the flyer direction, and only a due trail sample asks for
    // its leader's direction.
    const steer = [4,5,6,8,11,12,16].includes(op);
    const forward = steer && unit?.kind === 5 && unit.forward ? unit.forward(this.strandedAt.clone()) : null;
    const sample = (op === 8 || op === 11 || op === 16) && leader && !this.executor?.hold && Math.fround(this.trailDelay - a) < 0;
    const leaderForward = sample && leader.forward ? leader.forward(this.strandedAt.clone()) : null;
    const origin = op === 16 && unit?.origin ? unit.origin(this.strandedAt.clone()) : null;
    navigationUnit(this.sim, words, unit, forward, origin);
    navigationUnit(this.sim, words, leader, leaderForward);
    words.push(rng?.state ?? 0, this.controller?.ai?.followRadii?.far ?? world?.missionControls?.formation?.far ?? 3);
    words.push(Number(Boolean(aim)),aim?.x??0,aim?.y??0,aim?.z??0,Number(Boolean(facing)));
    const ptr = this.sim.x.sim_call_room(words.length,0);
    this.sim.bridge.views(); this.sim.bridge.f.set(words,ptr/8+HEADER);
    this.planner.feed();
    this.sim.x.ai_nav_control_command(op,this.id,a,b,c);
    const answer = this.sim.out().slice();
    if (rng) rng.state = answer[1];
    if (answer[2] && unit?.soldier?.avoidance) {
      const v=unit.soldier.avoidance; v.x=answer[3];v.y=answer[4];v.z=answer[5];
    }
    const start=answer[6] ? 15 : 7;
    this.planner.publications(answer,start);
    this.read();
    if (answer[6]) {
      intent.forward=answer[7];intent.right=answer[8];intent.turn=answer[9];intent.pitch=answer[10];
      if(answer[11])intent.jump=true;if(answer[12])intent.crouch=true;if(answer[13])intent.prone=true;
      if(answer[14])intent.jetDirection=answer[14]===1?null:this.jet.vector;
    }
    return answer[0];
  }

  attach(unit) {
    if (unit === this.unit && unit?.generation === this.generation) return this;
    this.run(0,0,0,0,unit); return this;
  }
  reset() { this.run(1); }
  strand(on = true) { this.run(2,Number(on)); }
  publish(path,ready) { this.run(3,path?.id??-1,Number(ready)); }
  consume(mode) { const answer=this.run(4,mode); return { ready:Boolean(answer&1),success:Boolean(answer&2) }; }
  install(path,a=true,b=false) { this.run(5,path.id,Number(a),Number(b)); }
  append(path) { this.run(6,path.id); path.read(); }
  stop() { this.run(7); }
  update(dt) { this.run(8,dt); }
  dropExecutor() { this.run(9); }
  setLeader(ref) { this.run(10,this.sim.id(ref?.unit),Number(ref?.generation!=null),ref?.generation??0); this.leader=ref; }
  sampleTrail(dt,leader) { this.run(11,dt,0,0,this.unit,leader); }
  recordTrail(on) { this.run(12,Number(on)); }
  consumeSegment() { this.run(13); }
  dispose() { this.run(14); }
  get status() { return this.run(15); }
  nativeInput(unit,intent,dt,aim,options) { this.run(16,dt,0,0,unit,null,aim,options.facing,intent);return intent; }
}

class NativeExecutor {
  constructor(control,path) {
    this.control=control; this.path=control.planner.pathOf(path);
    this.values=[];
    for (const [name,index] of [['index',41],['mode',42],['side',43],['complete',44],['status',45],['stationary',46],['best',47],['noProgress',48],['hold',49]]) {
      Object.defineProperty(this,name,{enumerable:true,get:()=>index===44||index===49 ? Boolean(this.values[index]) : this.values[index],set:value=>{
        if (name!=='hold' && name!=='complete') throw Error(`navigation executor ${name} is native`);
        this.values[index]=Boolean(value);
        control.sim.x.ai_nav_control_write(control.id,index,Number(value));
      }});
    }
    for (const name of ['prev','waypoint','next','final','direction','anchor','target']) this[name]={x:0,y:0,z:0};
  }
  read(words) {
    this.values=words;
    for (const [i,name] of ['prev','waypoint','next','final','direction','anchor','target'].entries()) {
      const at=50+i*3,point=this[name]; point.x=words[at];point.y=words[at+1];point.z=words[at+2];
    }
  }
  get node() { return this.path.nodes[this.index]; }
  get last() { return this.index >= this.path.nodes.length-1; }
}
