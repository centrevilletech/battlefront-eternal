// LiveObjects' one native phase. Construction loads geometry and definition
// facts once; ordinary ticks write dt/tick and consume changed mirrors only.
import { Matrix4 } from 'three';
import * as F from './sim/gen/f4.js';
import { F4Mirrors } from './f4-mirrors.js';
import { PhaseCounters } from './sim/phase-contract.js';
import { emitSound } from './audio/events.js';
import { PowerupStation } from './props.js';
const record = (name) => ({words:F[`${name}Layout`].words, put:F[`put${name}`]});
const SETUP = record('F4LiveSetup'), CLASS = record('F4UnitClass'), WEAPON = record('F4WeaponClass');
const ANIMATION = record('F4AnimationSetup'), GRIP = record('F4GripClass'), HULL = record('F4HullClass'), COMMAND = record('F4Command');
const BUILDING = {words:F.F4BuildingViewLayout.words,sync:F.syncF4BuildingView,put:F.putF4BuildingView};
const TRAP = {words:F.F4TrapViewLayout.words,sync:F.syncF4TrapView,put:F.putF4TrapView};
const ANIMATED = {words:F.F4AnimatedViewLayout.words,sync:F.syncF4AnimatedView,put:F.putF4AnimatedView};
const BUILDING_FIELDS = ['state','health','addHealth'];
const STATION_FIELDS = [...BUILDING_FIELDS,'mode','active','pulse','servedPlayer','servedHealth','servedAmmo'];
const TRAP_FIELDS = ['state','health','phase','started','time','sprung','tick','crushed'];
const ANIMATED_FIELDS = ['slot','time','weight','loop','hold','started','startTime','openness','victim'];
const clip = c => c ? {frames:c.frames,fps:c.fps,rootMotion:c.rootMotion,tracks:c.tracks,data:c.data} : null;
const text = value => { const s = String(value ?? '').trim().replace(/\n/g,' '); return s || null; };

export class F4Live {
  constructor(phase, {local = null, unitCapacity = 64, debug = false, effect = null, sound = emitSound, authority = () => true} = {}) {
    this.counters = new PhaseCounters();
    this.phase = phase; this.bridge = phase.bridge; this.x = phase.x;
    this.live = phase.factory.context.live; this.local = local; this.debug = debug;
    this.effect = effect; this.sound = sound; this.authority = authority;
    this.layout = new F.F4PhaseMirror(); this.native = new F.F4CountersMirror();
    this.input = new F.F4LiveInputMirror(); this.change = new F.F4CommandMirror();
    this.damageInfo = new F.F4HitInfoMirror();
    this.mirrors = new F4Mirrors(this.bridge); this.buildings = []; this.animations = []; this.units = [];
    this.classes = new Set(); this.weapons = new Set(); this.gripped = new WeakSet(); this.buildingsByKind = {building:[],trap:[]}; this.lastVersion = 0;
    const definitions = new Set();
    for (const side of Object.values(phase.factory.sides)) for (const def of Object.values(side.units ?? {})) definitions.add(def);
    for (const unit of phase.factory.built) definitions.add(unit.classDef);
    if (local) definitions.add(local.classDef);
    const fleet = phase.factory.context.vehicles?.vehicles ?? [];
    const prepared = phase.factory.context.vehicles?.f2?.prepared ?? [];
    const limit=phase.construction?.limit??phase.factory.context.units.nextId+unitCapacity;
    this.bridge.callArgs(SETUP, {local,unitCapacity:Math.max(unitCapacity,limit),
      registryCapacity:limit, hullCapacity:fleet.length + prepared.length});
    // Prepared future births need their held-frame mirrors before ticking.
    for(let id=0;id<limit;id++)this.units[id]={held:{prop:null,grab:null,since:0,frame:undefined,time:undefined},frame:new Matrix4()};
    this.descriptor = this.x.f4_live_init() / 8; this.readLayout();
    phase.liveOwner = this;
    for (let i = 0; i < phase.props.buildings.length; i++) this.bindBuilding(i);
    for (const def of definitions) this.prepareClass(def);
    for (const vehicle of fleet) this.prepareHull(vehicle);
    for (const vehicle of prepared) this.prepareHull(vehicle);
    for (const prop of this.live?.animated ?? []) this.bindAnimated(prop);
    // Factory preparation already loaded these models. Their clip mapping
    // depends on bone names only, so no actor or posed rig is needed here.
    if (phase.factory.bodyData && phase.factory.clipsFor) {
      for (const side of Object.values(phase.factory.sides)) for (const [name,def] of Object.entries(side.units ?? {})) {
        if (def.base !== 'soldier') continue;
        const data = phase.factory.bodyData(name);
        if (data) this.prepareGrips(def,phase.factory.clipsFor(data.def.bones));
      }
    }
    for (const unit of phase.factory.built) this.prepareUnit(unit);
    if (local) this.prepareUnit(local);
    this.installEvents();
    this.x.f4_live_publish(); this.sync();
  }
  readLayout() {
    const b = this.bridge; b.views(); F.syncF4Phase(b,b.f,this.descriptor,0,this.layout);
    this.readCounters();
  }
  // Phase diagnostics advance even when no gameplay row needs publication.
  // Read them once at the owning return, rather than at every idle observer.
  readCounters() {
    const b = this.bridge; b.views(); F.syncF4Counters(b,b.f,this.layout.counters / 8,0,this.native);
  }
  send(record, value, method) { this.bridge.callArgs(record,value); return this.x[method](); }
  handle(pointer) { const h = new F.F4HandleMirror(); this.bridge.views(); return F.syncF4Handle(this.bridge,this.bridge.f,pointer / 8,0,h); }
  prepareClass(def) {
    if (this.classes.has(def)) return;
    this.classes.add(def);
    this.send(CLASS,{def,captures:(def.CapturePosts ?? 1) !== 0,vehicle:String(def.HealthType ?? '').toLowerCase() === 'vehicle',
      healthEffect:def.HealthGainEffect || 'Powerup_blue',ammoEffect:def.AmmoGainEffect || 'Powerup_red',label:text(def.displayName)},'f4_unit_class');
    for (const weapon of def.weapons ?? []) this.prepareWeapon({levels:[weapon,...(weapon.charges ?? []).map(level=>({...weapon,...level}))],channel:weapon.channel,name:weapon.class});
  }
  prepareWeapon(weapon) {
    const root = weapon.levels[0]; if (this.weapons.has(root)) return;
    this.weapons.add(root);
    this.send(WEAPON,{def:root,ammunition:weapon.levels.map(def=>Number(def.ammo)),primary:weapon.channel === 0,
      key:weapon.name ?? root.class ?? null,labels:weapon.levels.map(def=>text(def.displayName ?? def.label))},'f4_weapon_class');
  }
  prepareHull(vehicle) { this.send(HULL,{hull:vehicle,vehicle:vehicle.healthType==='vehicle',side:vehicle.side ?? null,label:text(vehicle.label)},'f4_hull_class'); }
  prepareUnit(unit) {
    this.prepareClass(unit.classDef);
    for (const weapon of unit.combat.arsenal.weapons) this.prepareWeapon(weapon);
    if (!this.units[unit.id]) this.units[unit.id] = {held:{prop:null,grab:null,since:0,frame:undefined,time:undefined},frame:new Matrix4()};
    this.prepareGrips(unit.classDef,unit.animator?.clips);
  }
  prepareGrips(def, source) {
    if (this.gripped.has(def)) return;
    this.gripped.add(def);
    for (const binding of this.animations) {
      if (!binding) continue;
      const clips = binding.object.grabs.map(grab=>source?.get(grab.clip));
      this.send(GRIP,{def,animated:binding.entry.handle,
        clips:clips.map(clip=>clip ? this.phase.anim.clip(clip) : null)},'f4_grip_class');
    }
  }
  bindBuilding(index) {
    const object = this.phase.props.buildings[index], handle = this.handle(this.x.f4_building_handle(index));
    const trap = handle.kind === 'trap', view = trap ? new F.F4TrapViewMirror() : new F.F4BuildingViewMirror();
    const binding = {object,view,trap,station:object instanceof PowerupStation,index,hasAttacker:Object.hasOwn(object,'lastAttacker'),entry:null};
    binding.entry = this.mirrors.bind(handle,trap ? TRAP : BUILDING,view,()=>this.publishBuilding(binding));
    this.watch(binding,trap ? TRAP_FIELDS : binding.station ? STATION_FIELDS : BUILDING_FIELDS);
    this.buildings.push(binding); this.buildingsByKind[handle.kind][handle.slot] = binding;
    object.setHealth = value=>this.command(binding,'health',value);
    object.damage = (amount,attacker = null,hit = null)=>this.command(binding,'damage',amount,attacker,false,0,hit);
  }
  publishBuilding(binding) {
    const b = binding.object, view = binding.view, s = view.state;
    if (binding.trap) {
      b.state = s.built ? 'built' : 'unbuilt'; b.health = s.health; b.phase = s.phase; b.started = s.started;
      b.time = s.time; b.sprung = s.sprung; b.tick = s.tick; b.crushed = s.crushed;
    } else {
      const h = s.building; b.state = h.state; b.health = h.health; b.addHealth = h.addHealth;
      if (b.aiTarget && h.targetGeneration !== null) b.aiTarget.generation = h.targetGeneration;
      if (binding.station) {
        b.mode = s.mode; b.active = s.active; b.pulse = s.pulse;
        b.servedPlayer = s.servedPlayer; b.servedHealth = s.servedHealth; b.servedAmmo = s.servedAmmo;
      }
    }
    if (view.attacker !== undefined || binding.hasAttacker) { b.lastAttacker = view.attacker; binding.hasAttacker = true; }
    this.remember(binding);
  }
  bindAnimated(p) {
    const index = this.animations.length;
    // Pose accumulators are views of the sampler's memory; growth detaches them.
    p.pose?.refresh();
    const settings = {door:p.door,clips:p.slots.map(clip),doorClip:clip(p.doorClip),doorDuration:p.doorClip?.duration ?? 0,
      idleDelay:p.idleDelay,openSlots:p.names.map(name=>name === 'open'),position:p.position,killSound:p.killSound,
      triggers:p.triggers.map(t=>({slot:t.slot,point:t.point,radius:t.radius,sound:t.sound || null,opens:t.action === 'open'})),
      grabs:p.grabs.map(g=>({slot:g.slot,window:g.window,radius:g.radius,node:g.node ? p.rig.bones.indexOf(g.node) : null,clip:g.clip})),
      root:p.rig.root.matrix,parents:p.rig.bones.map(b=>p.rig.bones.indexOf(b.parent)),bind:p.rig.bones.map(b=>p.rig.bind.get(b))};
    const state = {index,slot:p.slot,time:p.time,weight:p.weight,loop:p.loop,hold:p.hold,started:p.started,startTime:p.startTime,
      victim:p.victim ? {unit:p.victim.unit.id,generation:p.victim.generation,grab:p.grabs.indexOf(p.victim.grab),since:p.victim.since} : null,
      openness:p.openness,bodyEnabled:p.body?.enabled ?? null,standQ:p.standQ,standT:p.standT,
      pose:p.pose ? {root:p.pose.root,q:p.pose.q,t:p.pose.t,w:p.pose.w} : null,rootWorld:p.rig.root.matrixWorld,
      bones:p.rig.bones.map(b=>({p:b.position,q:b.quaternion,s:b.scale,world:b.matrixWorld}))};
    const handle = this.handle(this.send(ANIMATION,{prop:p,body:p.body ?? null,birth:{settings,state}},'f4_animated_birth'));
    const binding = {object:p,index,view:new F.F4AnimatedViewMirror(),victim:{unit:null,generation:0,grab:null,since:0},entry:null};
    binding.entry = this.mirrors.bind(handle,ANIMATED,binding.view,()=>this.publishAnimated(binding));
    this.watch(binding,ANIMATED_FIELDS);
    this.animations[handle.slot] = binding;
    p.play = (slot,looping = false,tick = 0)=>this.command(binding,'play',slot,null,looping,tick);
  }
  publishAnimated(binding) {
    const p = binding.object, v = binding.view;
    p.slot = v.slot; p.time = v.time; p.weight = v.weight; p.loop = v.looping; p.hold = v.hold;
    p.started = v.started; p.startTime = v.startTime; p.openness = v.openness;
    if (p.body && v.bodyEnabled !== null) p.body.enabled = v.bodyEnabled;
    if (v.hasVictim) {
      const a = binding.victim, b = v.victim; a.unit = this.phase.bindings[b.unit].unit;
      a.generation = b.generation; a.grab = p.grabs[b.grab]; a.since = b.since; p.victim = a;
    } else p.victim = null;
    this.remember(binding);
  }
  installEvents() {
    const handlers = this.phase.eventHandlers;
    handlers.f4Effect = e=>{ if (this.effect) this.effect(e.name,e.position); };
    handlers.f4Sound = e=>this.sound('sound',e.name,e.position);
    handlers.f4Serve = e=>this.sound('serve',this.buildingsByKind[e.station.kind][e.station.slot].object,e.unit?.supply??null);
    handlers.f4Held = e=>{
      const binding = this.phase.bindings[e.key.slot];
      if (!binding || binding.view.key.generation !== e.key.generation || binding.view.key.life !== e.key.life) return;
      const unit = binding.unit, cache = this.units[e.key.slot];
      if (!e.prop) {
        unit.held = null; unit.animator?.gunner?.(null); const root = unit.actor?.root;
        if (root) root.rotation.set(0,root.rotation.y,0); return;
      }
      const prop = this.animations[e.prop.slot];
      if (!prop || prop.entry.handle.generation !== e.prop.generation) return;
      const h = cache.held; h.prop = prop.object; h.grab = prop.object.grabs[e.grab]; h.since = e.since;
      if (e.frame) { cache.frame.copy(e.frame); h.frame = cache.frame; } else h.frame = undefined;
      h.time = e.time === null ? undefined : e.time; unit.held = h;
    };
  }
  watch(binding, fields) {
    if (!this.debug) return;
    binding.fields = fields; binding.expected = new Array(fields.length);
  }
  remember(binding) {
    if (!this.debug) return;
    for (let i=0;i<binding.fields.length;i++) binding.expected[i] = binding.object[binding.fields[i]];
  }
  check() {
    if (!this.debug) return;
    this.checkBindings(this.buildings); this.checkBindings(this.animations);
  }
  checkBindings(bindings) {
    for (const binding of bindings) {
      if (!binding) continue;
      for (let i=0;i<binding.fields.length;i++) if (!Object.is(binding.expected[i],binding.object[binding.fields[i]])) {
        throw Error(`live-object mirror write: ${binding.entry.handle.kind}/${binding.entry.handle.slot}/${binding.fields[i]}`);
      }
    }
  }
  sync() {
    const b = this.bridge; b.views();
    if (this.lastVersion === b.f[this.descriptor + F.F4PhaseLayout.version]) return;
    this.readLayout();
    this.lastVersion = this.layout.version;
    this.mirrors.apply(b.f,this.layout.buildings / 8); this.mirrors.apply(b.f,this.layout.animated / 8); this.mirrors.apply(b.f,this.layout.traps / 8);
  }
  step(dt, tick) {
    this.check();
    const b = this.bridge; b.views(); this.input.dt = dt; this.input.tick = tick;
    F.putF4LiveInput(b,this.input,b.f,this.layout.liveInput / 8,b.heap);
    const p = this.phase, counters = this.counters, previous = b.phaseCounters, active = p.active;
    const randomWords = p.random.entries.length * p.layout.randomWords, output = p.counters.outputFields, mirrorWords = this.mirrors.words;
    p.random.push(); p.active = true; b.phaseCounters = counters;
    counters.ticks++; counters.wasmCalls++; counters.phaseCalls++; counters.inputFields += 2 + randomWords;
    try { this.x.f4_live(); }
    finally { p.active = active; b.phaseCounters = previous; }
    p.random.pull(); p.sync(); this.readCounters();
    counters.outputFields += this.mirrors.words - mirrorWords + p.counters.outputFields - output + randomWords;
    counters.rustAllocations += this.native.allocations;
    counters.rustReallocations += this.native.reallocations;
    counters.rustAllocationBytes += this.native.allocationBytes;
    if (this.debug) this.mirrors.check();
  }
  command(binding, op, value, attacker = null, flag = false, tick = 0, hit = null) {
    const c = this.change; c.target = binding.entry.handle; c.op = op; c.value = value; c.attacker = attacker; c.flag = flag; c.tick = tick;
    const info=this.damageInfo;
    if(hit){info.direction=hit.direction??null;info.amount=hit.amount??value;info.typed=hit.typed??1;info.generic=hit.generic??1;}
    c.damageInfo=hit?info:null;
    const p = this.phase, b = this.bridge, previous = b.phaseCounters, active = p.active;
    if (active) throw Error('live command reentered the owned phase');
    p.owners.prepare(attacker); p.random.push();
    b.callArgs(COMMAND,c); p.active = true; b.phaseCounters = this.counters;
    this.counters.wasmCalls++; this.counters.commandCalls++; this.counters.inputFields += COMMAND.words;
    let accepted;
    try { accepted = this.x.f4_live_command(); }
    finally { p.active = active; b.phaseCounters = previous; }
    p.random.pull(); p.sync(); this.readCounters(); return Boolean(accepted);
  }
}
