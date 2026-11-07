import {PHASE_DELTA_WORDS, BlockHeaderLayout} from './sim/gen/boundary.js';
import {F3CombatField_VALUE as C} from './sim/gen/combat.js';
import {F3DroidekaClassMirror,F3DroidekaClassLayout,putF3DroidekaClass,F3BoundaryLayout,
  refreshF3Boundary,F3_BOUNDARY_REFRESH_WORDS} from './sim/gen/units.js';
import * as THREE from 'three';
import { bridgeFor } from './sim/runtime.js';
import { KIND_UNIT, KIND_BOLT, KIND_BEAM, KIND_GRENADE, KIND_ROUND, F3BoundaryMirror, F3ChangeMirror, F3AdoptionMirror,
  F3AcknowledgementMirror, F3AcknowledgementLayout, putF3Acknowledgement, F3PhaseInputMirror, IntentLayout, syncIntent, F3PhaseNetworkMirror, F3PublicationLayout, F3UnitKeyMirror, F3MetadataMirror,
  F3SoldierMotionLayout, F3DroidekaMotionLayout, syncF3Boundary, syncF3UnitMirrorDelta, syncF3CombatView, syncF3CombatViewDelta, syncF3WeaponView, F3CombatViewLayout, F3WeaponViewLayout, syncF3ControlViewDelta, F3ControlViewLayout, F3ForeignControlsLayout, syncF3UnitKey, F3UnitMirrorLayout, putF3Change, putF3Adoption, putF3PhaseInput, putF3PhaseNetwork, putF3WeaponService,
  F3DamageInfoMirror, F3HitPolicyMirror, putF3HitPolicy, F3BlastBindingMirror, putF3BlastBinding, F3ImpactRulesMirror, putF3ImpactRules,
  F3AimHeaderMirror, F3AimChangeMirror, putF3AimHeader, putF3AimChange,
  F3MissileClassMirror, putF3MissileClass,
  F3SeparationMirror, putF3Separation,
  F3BlastCallMirror, putF3BlastCall, F3PoseCommandMirror, putF3PoseCommand,
  EVENT_TABLE, IMPORT_TABLE, F3WeaponServiceMirror, F3BodySkeletonMirror, F3BodyNameMirror, putF3BodyName, putF3BodySkeleton, Air_INDEX } from './sim/gen/index.js';
import { F3Counters, F3UnitBinding } from './f3-contract.js';
import { giveMapRules } from './vehicle-phase-data.js';
import { waterDamageRule, droidekaWaterDepth } from './vehicle-water.js';
import { AnimKernel } from './anim-kernel.js';
import { blankIntent } from './units.js';
import { PlayerCombat } from './combat.js';
import { F3Registry } from './f3-registry.js';
import { UnitMuzzles } from './unit-muzzles-sim.js';
import { weaponAI, categoryOf } from './weapon-policy.js';
import { botLockTarget } from './weapon-targeting.js';
import { F3Random } from './f3-random.js';
import { F3Owners } from './f3-owners.js';
import { F3Items } from './f3-items.js';
import { F3Recon } from './f3-recon.js';
import { F3Props } from './f3-props.js';
import { F3Hulls } from './f3-hulls.js';
import { F3History } from './f3-history.js';
import { F3Factory } from './f3-factory.js';
import { applyDelta } from './f2-delta.js';
import { deltaChanged, markDelta } from './sim/compare.js';
import { F2DeltaLayout, F2PhaseLayout } from './sim/gen/f2.js';
import { F4PhaseLayout } from './sim/gen/f4.js';
import { AiPhaseBoundaryLayout } from './sim/gen/ai-phase.js';

const CLIPS = ['idle','turnl','turnr','death01',null,'idle_to_leftfoot',null,'walk_leftfoot_rightfoot',
  'walk_rightfoot_leftfoot','leftfoot_to_idle','rightfoot_to_idle','fold','roll','unfold'];
const BLOCK_HEADER = BlockHeaderLayout.words;

export function attachF3(instance, classes, game) {
  return new F3(bridgeFor(instance, { classes }), game);
}

// UnitFactory mirrors keep their original classes and vectors. Only explicit
// lifecycle and scalar commands mutate the generated canonical rows.
class F3 {
  // UnitFactory.build still creates the pooled objects that readers know.
  // A single bridge binds those objects to the battle's canonical slab;
  // neither a setter nor a callback may import the bodies again.
  constructor(bridge, {
    factory, combatSim, collision, water, map, presentEffect, separation = null, debug = false,
  }) {
    this.active = false;
    this.tick = 0;
    this.bridge = bridge;
    this.x = bridge.x;
    this.factory = factory;
    this.fleet = factory.context.vehicles ? factory.context.vehicles.f2 : null;
    this.combatSim = combatSim;
    this.collision = collision;
    this.water = water;
    this.presentEffect = presentEffect;
    this.debug = debug;
    // The slot table points at stable pooled objects. It does not become a
    // second collection of live simulation bodies, and pooling never changes
    // a body's slot just because its gameplay generation advances.
    this.bindings = [];
    this.one = [null];
    this.counters = new F3Counters();
    if (this.fleet) this.fleet.counters = this.counters;
    this.layout = new F3BoundaryMirror();
    this.key = new F3UnitKeyMirror();
    this.acknowledgement = new F3AcknowledgementMirror();
    this.change = new F3ChangeMirror();
    this.pose = new F3PoseCommandMirror();
    this.damageInfo = new F3DamageInfoMirror();
    this.metadata = new F3MetadataMirror();
    this.adoption = new F3AdoptionMirror();
    this.input = new F3PhaseInputMirror();
    this.networkInput = new F3PhaseNetworkMirror();
    // Game.separateSoldiers reads one camera scope after recon finishes.
    // This retained generated row contains no body or registry snapshots.
    this.separation = new F3SeparationMirror();
    this.globalSeparationCamera = new THREE.Vector3();
    this.separation.global = true;
    this.separationScope = separation ?? this.separation;
    this.aiming = false;
    this.aimHeader = new F3AimHeaderMirror();
    this.aimChange = new F3AimChangeMirror();
    this.heap = bridge.heap;
    this.lastVersion = -1;
    this.eventEpoch = -1;
    this.eventOffset = 0;
    this.delivering = false;
    this.birthEpoch = -1;
    this.birthOffset = 0;
    this.birthWords = -1;
    this.eventMirrors = [];
    this.eventHandlers = this.notifications();
    // Generated event mirrors are scratch records, reserved before any tick.
    // A phase publication can append to the same stream between aimers.
    for (let i = 0; i < EVENT_TABLE.length; i++) {
      const event = EVENT_TABLE[i]; if (event) this.eventMirrors[event.code] = new event.record.Mirror();
    }
    // Immutable launch owners can be needed by a Character mirror barrier
    // before ordinary notices drain. Bind only those structural records here;
    // sampled shot/damage/presentation observers keep their outer ordering.
    const hullBirth = EVENT_TABLE.find(event=>event?.name === 'f3HullBirth');
    const ownerBirth = EVENT_TABLE.find(event=>event?.name === 'f3OwnerCreated');
    const unitBirth = EVENT_TABLE.find(event=>event?.name === 'f3UnitBirth');
    const controllerBirth = EVENT_TABLE.find(event=>event?.name === 'aiControllerBirth');
    const controlBirth = EVENT_TABLE.find(event=>event?.name === 'aiControlBirth');
    const characterBirth = EVENT_TABLE.find(event=>event?.name === 'f4CharacterBirth');
    this.birthHandlers = {f4CharacterBirth:event=>{
      this.tailOwner.ensureCharacter(event);this.counters.outputFields += characterBirth.record.words;
    },aiControlBirth:event=>{
      this.aiOwner.controlBorn(event);this.counters.outputFields += controlBirth.record.words;
    },aiControllerBirth:event=>{
      this.aiOwner.controllerBorn(event);this.counters.outputFields += controllerBirth.record.words;
    },f3HullBirth:event=>{
      this.hulls.created(event); this.counters.outputFields += hullBirth.record.words;
    },f3OwnerCreated:event=>{
      this.owners.created(event);this.counters.outputFields += ownerBirth.record.words;
    },f3UnitBirth:event=>{
      this.construction.created(event);this.counters.outputFields += unitBirth.record.words;
    }};
    // Definitions are keyed by their immutable first level. A new Unit of
    // the same class shares those prepared services rather than sending the
    // weapon content table once again for each pooled body.
    this.services = new WeakSet();
    this.service = new F3WeaponServiceMirror();
    this.classes = new WeakMap();
    this.anim = new AnimKernel(this.x);
    this.muzzles = new UnitMuzzles(bridge);
    this.bodyName = new F3BodyNameMirror();
    this.bodyVersion = -1;
    this.flags = this.x.movement_flags_ptr();
    // Damage callbacks may recurse through a death explosion. Retain frames
    // by nesting depth so the original source/attacker objects stay attached
    // to the innermost hurt without allocating a frame for every hit.
    this.frames = [];
    this.depth = 0;
    for (let i = 0; i < 1024; i++) this.frames.push({ source: null, attacker: null, info: null, blast: false });
    giveMapRules(bridge, water, map);
    // A prepared fleet already owns this collision world and its vehicle-only
    // blockers. Infantry shares the world without replacing those hull rules.
    if (!factory.context.vehicles) this.x.veh_collision(collision.rayWorld, -1);
    bridge.strings.id('');
    if (this.fleet) this.fleet.prepareVariants();
    this.descriptor = this.x.f3_init(collision.rayWorld, water ? water.level : NaN, collision.bodySerial) / 8;
    bridge.views(); syncF3Boundary(bridge,bridge.f,this.descriptor,0,this.layout);
    this.random = new F3Random(this);
    // Fleet construction precedes unit ownership. Bind its retained stream
    // reader now, before any live seat or firing entry can run.
    if (this.fleet) {
      this.fleet.firing.random = this.random;
      this.fleet.seats.random = this.random;
    }
    this.owners = new F3Owners(this);
    this.items = new F3Items(this,factory.context.items);
    this.recon = new F3Recon(this);
    this.recon.firePoints = factory.context.reconFirePoints;
    // Flight missile classes are a fixed vocabulary. The native seeker shares
    // F2's live numeric tuning row instead of asking JS during each round tick.
    const missile = new F3MissileClassMirror();
    const missileRecord = {words:this.layout.missileClassWords,put:putF3MissileClass};
    for (const name of ['','fighter','scout','bomber','transport']) {
      missile.name = name; missile.class = name || 'fighter';
      bridge.callArgs(missileRecord,missile); this.x.f3_missile_classes(1);
    }
    this.foreignStride = this.layout.foreignWords;
    this.prepareImpacts();
    // Registry construction precedes adoption so mixed members preserve the
    // same insertion order as Units.add, including pre-existing hull targets.
    this.registry = new F3Registry(this, factory.context.units);
    this.props = new F3Props(this, factory.context.live);
    this.hulls = new F3Hulls(this, factory.context.vehicles);
    this.registry.prepareOwned();
    this.history = new F3History(this);
    this.construction = new F3Factory(this);
    const seatAction = IMPORT_TABLE.find(row=>row?.name === 'f2SeatAction').code;
    bridge.beforeImport = code => {
      if (!this.active) return;
      // A new AI seat control names its lease in the import arguments.
      // Bind that touched publication before the generated decoder sees it.
      if (code === seatAction) this.sync();
    };
    // The factory calls adoption only after constructing the complete body.
    // Content setup has its own cold facade; module exports stay unchanged.
    factory.context.unitPhase = this;
    bridge.register(this.handlers());
    // Import argument mirrors are construction storage too. The first aimer
    // or death callback must not allocate its argument record during a tick.
    for (let i = 0; i < IMPORT_TABLE.length; i++) {
      const imp = IMPORT_TABLE[i]; if (imp && !bridge.served[i]) bridge.served[i] = new imp.args.Mirror();
    }
    // A pointer into detached memory is invalid after a grow. Reserve room
    // while constructing the battle, before a phase can enter a callback.
    bridge.reserve(16 * 1024 * 1024);
    // CollisionWorld membership already exists at attachment. Name it cold,
    // so the first ordinary phase does not need a construction export.
    this.bodiesIn();
    this.counters.reset();
  }

  // UnitFactory.acquire pops the canonical class stack. Empty pools build
  // through the factory's explicit completed-body adoption interface.
  acquire(name) {
    const slot = this.x.f3_factory_acquire(this.bridge.strings.id(name));
    this.counters.wasmCalls++; this.counters.lifecycleCalls++; this.counters.inputFields++;
    if (slot < 0) throw Error(`unit class has no prepared body capacity: ${name}`);
    this.sync();
    const binding = this.bindings[slot];
    binding.view.pooled = false;
    return binding.unit;
  }
  // Warm construction and removal share one pool. A published return bit
  // makes the later removal listener idempotent without another command.
  release(unit) {
    const binding = this.bindings[unit.id];
    if (binding.view.pooled) return;
    this.begin(binding,'poolRelease'); this.commit();
  }

  // Read the stable descriptor rather than calling an export for each view.
  // Publication can move the output buffer, but the descriptor stays put.
  // Its generated widths also define the crossing census.
  readLayout() {
    this.bridge.views(); refreshF3Boundary(this.bridge, this.bridge.f, this.descriptor, 0, this.layout);
    this.counters.outputFields += F3_BOUNDARY_REFRESH_WORDS;
  }

  // UnitFactory.build: this is the only full Unit import. Construction must
  // finish before adoption, because class loading can grow module memory.
  // Bind existing vectors before publishing, preserving supply.position and
  // the item target's position/velocity aliases across every pooled life.
  adopt(unit, passive = false) {
    const b = this.bridge, refs = b.refs[KIND_UNIT], slot = refs.slotOf(unit);
    const key = {slot, generation: refs.key ? 0 : refs.gens[slot], life: unit.generation};
    this.one[0] = unit; b.writeSome(KIND_UNIT, this.one);
    this.adoption.key = key;
    // A dormant viewpoint has registry geometry and receives commands, but
    // never enters the factory tick order or its reusable corpse stack.
    this.adoption.passive = passive;
    this.adoption.category = categoryOf(unit);
    // Seats read immutable actor/class facts from the same construction
    // import. Later latches read body and combat flags directly in Rust.
    this.adoption.hasActor = Boolean(unit.actor);
    this.adoption.noEnter = Number.parseFloat(unit.classDef?.NoEnterVehicles ?? 0);
    const rule = unit.walker ? waterDamageRule(unit.soldier.def) : null;
    this.adoption.droidekaWater = rule ? {interval:rule.interval,amount:rule.amount,
      upright:droidekaWaterDepth(unit.soldier.def,false),ball:droidekaWaterDepth(unit.soldier.def,true)} : null;
    const binding = new F3UnitBinding(unit, key, this.debug, this);
    binding.adoption = Object.assign(new F3AdoptionMirror(),this.adoption);
    binding.adoption.key = binding.view.key;
    binding.life = unit.generation;
    binding.words = null; binding.wordsAt = 0;
    binding.weaponVersion = 0;
    binding.dirty = new Uint32Array(PHASE_DELTA_WORDS / 32);
    binding.arsenal = 0;
    binding.inputJetDirection = new THREE.Vector3();
    binding.intentAim = new THREE.Vector3();binding.intentJetDirection = new THREE.Vector3();
    binding.inputNetwork = false; binding.nativeIntent = false;
    binding.aimed = false;
    // AnimatedProp holds reuse one object and frame for this pooled body.
    // Clip handles resolve cold, rather than by name during every carry.
    binding.held = {prop:null,grab:null,since:0,frame:undefined,time:undefined};
    binding.heldFrame = new THREE.Matrix4();
    binding.bindHeld(binding.held,binding.heldFrame);
    binding.heldClips = new Map();
    binding.heldClip = null;
    binding.heldGrab = 0;
    this.bindings[slot] = binding; unit.f3 = this;
    this.registry.prepare(binding);
    this.wireUnit(binding);
    this.prepareSkeleton(binding);
    this.wireCombat(binding);
    binding.bindCombat(unit.combat);
    this.prepareBody(unit);
    this.prepareWeapons(unit.combat);
    this.items.prepareUnit(unit);
    // Preparing clips and F1 definitions uses scratch call storage. Write
    // adoption last so those construction calls cannot replace its row.
    this.readLayout(); this.heap.clear();
    putF3Adoption(b, this.adoption, b.f, this.layout.command / 8, this.heap);
    this.counters.wasmCalls++;
    this.counters.lifecycleCalls++;
    this.counters.inputFields += this.layout.adoptWords;
    if (!this.x.f3_adopt()) throw Error('stale F3 adoption');
    this.sync();
    this.random.refresh(binding);
    this.items.schedule(binding);this.items.flush();
    this.liveOwner?.prepareUnit(unit);
    this.fleet?.riders.ensureAll(unit);
    if (passive) this.construction.resident(binding);
  }

  // Unit.spawn/remove/letGo become explicit lifecycle commands. Ordinary
  // readers retain the same Unit object and its existing sphere getter.
  // Shape reads are sensors: their cache is observed at the caller's point,
  // never eagerly while publishing a body after movement or damage.
  wireUnit(binding) {
    const unit = binding.unit;
    unit.spawn = (x, y, z, yaw) => this.spawn(binding.unit, x, y, z, yaw);
    unit.remove = () => this.remove(binding.unit);
    unit.letGo = () => this.letGo(binding.unit);
    // Body replacement preserves the facade; resolve its current native binding.
    Object.defineProperty(unit,'sphere',{configurable:true,get(){return this.f3.registry.bounds(this);}});
    unit.updateSpheres = (boundsOnly = false) => boundsOnly ? this.registry.bounds(binding.unit) : this.registry.shapes(binding.unit);
    unit.hitShapes = () => {
      if (binding.shapesTick !== binding.unit.tickCount) this.registry.shapes(binding.unit);
      return binding.hitList;
    };
  }

  // SoldierAnimator's bind pose is immutable in a headless world. Copy its
  // decomposed transforms once; the native sampler resolves joints and
  // restores bind lengths without posing the three.js bones each tick.
  prepareSkeleton(binding) {
    const b = this.bridge, unit = binding.unit, bones = unit.actor.bones;
    const record = new F3BodySkeletonMirror(); record.key = binding.view.key;
    record.bones = bones.map(bone => ({name:bone.name,parent:bones.indexOf(bone.parent),
      position:bone.position,quaternion:bone.quaternion,scale:bone.scale}));
    this.heap.clear(); const row = [];
    putF3BodySkeleton(b, record, row, 0, this.heap);
    const ptr = this.x.sim_call_room(row.length, this.heap.words.length); b.views();
    b.f.set(row, ptr / 8 + BLOCK_HEADER); b.f.set(this.heap.words, ptr / 8 + BLOCK_HEADER + row.length);
    if (!this.x.f3_body_skeleton()) throw Error('stale F3 skeleton');
    for (const [name,clip] of unit.animator.clips) binding.heldClips.set(name,this.anim.clip(clip));
  }

  // CombatWorld.fireRound/dispense need numeric F1 definition handles.
  // Charge levels can carry merged ordnance, so prepare every level rather
  // than assuming the first level's definition applies to all shots.
  // Shared class tables are sent once, independent of the number of bodies.
  prepareWeapons(combat) {
    const b = this.bridge, weapons = combat.arsenal.weapons;
    for (let i = 0; i < weapons.length; i++) {
      const weapon = weapons[i], def = weapon.levels[0];
      if (this.services.has(def)) continue;
      this.recon.prepare(weapon);
      const rows = new Float64Array(weapon.levels.length * this.layout.serviceWords);
      for (let level = 0; level < weapon.levels.length; level++) {
        const d = weapon.levels[level], service = this.service;
        if (d.ordnance) combat.ordnance.sim.defId(d.ordnance);
        if (d.item) combat.items.sim.defId(d.item);
        service.ordnance = d.ordnance ?? null; service.item = d.item ?? null;
        service.channel = weapon.channel;
        service.bank = weapon.bank;
        service.lightning = d.ordnance?.LightningEffect ?? '';
        service.hasLightning = Boolean(service.lightning && combat.effects?.lightning?.[service.lightning]);
        // Cold views share Rust's cooked content policy. Initial physical
        // construction can precede AI loading; its native binding completes
        // these masks later without replacing timers or random streams.
        const masks = weaponAI(d), lock = service.lock;
        lock.range = Number(d.LockOnRange) || 0;
        lock.onAngle = Number(d.LockOnAngle) || 0;
        lock.offAngle = Number(d.LockOffAngle) || lock.onAngle;
        lock.relationMask = masks.relationMask; lock.categoryMask = masks.categoryMask;
        this.heap.clear();
        putF3WeaponService(b, service, rows, level * this.layout.serviceWords, this.heap);
      }
      const ptr = this.x.sim_call_room(rows.length, 0); b.views(); b.f.set(rows, ptr / 8 + BLOCK_HEADER);
      this.x.f3_weapon_services(b.defs.weapon.id(def), weapon.levels.length); this.services.add(def);
    }
  }

  // OrdnanceSim.send loads full damage policy alongside F1's flight facts.
  // Both old and newly registered definitions use this construction interface;
  // a native shot never imports a class or allocates a damage-definition copy.
  prepareImpacts() {
    const b = this.bridge, sim = this.factory.context.ordnance.sim;
    const hit = new F3HitPolicyMirror(), blast = new F3BlastBindingMirror();
    const hitRecord = {words:this.layout.hitPolicyWords,put:putF3HitPolicy};
    const blastRecord = {words:this.layout.blastBindingWords,put:putF3BlastBinding};
    const blastCall = new F3BlastCallMirror();
    const blastArgs = {words:this.layout.blastWords,put:putF3BlastCall};
    sim.nativeServices = {
      item: (def,model) => this.items.prepare(def,model),
      owner: owner => this.owners.prepare(owner),
      explode: (def,position,owner) => {
        this.owners.prepare(owner);
        blastCall.explosion = sim.explosionId(def);blastCall.position = position;blastCall.owner = owner ?? null;
        b.callArgs(blastArgs,blastCall);
        this.counters.wasmCalls++;this.counters.commandCalls++;this.counters.inputFields += blastArgs.words;
        sim.nativeServices.beforeStep();
        try { this.x.f3_world_blast(); }
        finally { sim.nativeServices.afterStep(); }
      },
      // F1's later flight phase can hurt owned units too. Its outer return
      // publishes those touched rows and drains the same ordered event epoch.
      // No unit import or extra mirror getter is needed around ordnance.step.
      beforeStep: () => {
        // A foreign prop can explode between unit phases after changing its
        // own state. Import that touched life before the blast can revisit it;
        // collision flags likewise follow the just-shown look immediately.
        this.bodiesIn();this.registry.foreign();
        this.random.push();this.active = true;
        this.externalCounters = b.phaseCounters;b.phaseCounters = this.counters;
      },
      afterStep: () => {
        this.active = false;b.phaseCounters = this.externalCounters;
        this.random.pull();this.sync();
      },
      ordnance: (id, def) => {
        hit.ordnance = def; hit.damage = Number(def.Damage || 0);
        hit.person = Number(def.PersonScale ?? 1); hit.animal = Number(def.AnimalScale ?? 1);
        hit.droid = Number(def.DroidScale ?? 1); hit.vehicle = Number(def.VehicleScale ?? 1);
        hit.building = Number(def.BuildingScale ?? 1); hit.shield = Number(def.ShieldScale ?? 1);
        b.callArgs(hitRecord, hit); this.x.f3_hit_policy();
      },
      explosion: (id, def) => {
        blast.explosion = id; blast.combat = this.combatSim.explosion(def);
        b.callArgs(blastRecord, blast); this.x.f3_blast_binding();
      },
    };
    for (let i = 0; i < sim.sent; i++) sim.nativeServices.ordnance(i, sim.ordnanceDefs.at(i));
    for (let i = 0; i < sim.explosions.list.length; i++) sim.nativeServices.explosion(i, sim.explosions.at(i));
    const items = this.factory.context.items?.sim;
    if (items) for (let i = 0; i < items.sent; i++) {
      const def = items.itemDefs.at(i);this.items.prepare(def,items.model(def));
    }
    const rules = new F3ImpactRulesMirror();
    rules.friendlyFire = this.factory.context.settings?.get('friendlyFire') !== false;
    b.callArgs({words:this.layout.impactRulesWords,put:putF3ImpactRules}, rules); this.x.f3_impact_rules();
  }

  // Droideka.arm: retain the hidden gun rig separately from drawn bones.
  // The mode-to-clip order matches Droideka's state machine, including the
  // missing modes; folding and rolling keep their original clip clocks.
  prepareBody(unit) {
    if (!unit.walker) return;
    const body = unit.soldier;
    let cls = this.classes.get(body.def);
    if (cls === undefined) {
      const row = new F3DroidekaClassMirror();
      row.maxPitchSpeed = body.def.MaxPitchSpeed ?? null;
      row.ballTurnSpeed = body.def.BallTurnSpeed ?? null;
      row.ballMaxLean = body.def.BallMaxLean ?? null;
      row.ballAcceleration = body.def.BallAcceleration ?? null;
      row.ballRollingFriction = body.def.BallRollingFriction ?? null;
      for (let i = 0; i < CLIPS.length; i++) {
        const name = CLIPS[i], clip = name && body.clips.get(name);
        row.clips[i] = clip ? this.anim.clip(clip) : -1;
      }
      this.bridge.callArgs({words:F3DroidekaClassLayout.words,put:putF3DroidekaClass},row);
      cls = this.x.units_droideka_class();
      this.classes.set(body.def, cls);
    }
    this.x.units_droideka_attach(unit.id, cls);
    this.muzzles.attach(unit, cls);
  }

  // CombatFacade's content and presentation machinery remains useful while
  // its mutators become qualified commands. Weapon getters read published
  // rows without importing bodies or recomputing their state.
  // Unit.tick owns update and deferred muzzle flushing inside the module.
  damageFrame() {
    if (this.depth === this.frames.length) throw Error('damage nesting capacity exceeded');
    return this.frames[this.depth++];
  }

  wireCombat(binding) {
    const c = binding.unit.combat;
    // PlayerCombat.eye only reads published basis vectors for the viewpoint
    // camera. It shares the page's read helper and never advances combat.
    c.eye = PlayerCombat.prototype.eye;
    c.reset = () => this.scalar(binding.unit.id, 'reset', 0, 0, 0);
    c.damage = (amount, onShield = amount, source = null, attacker = null, info = null) => {
      if (c.dead || amount <= 0) return;
      const frame = this.damageFrame();
      frame.source = source;
      frame.attacker = attacker;
      frame.info = info;
      frame.blast = false;
      // CombatWorld.damaged retains its caller's source/info identity. A
      // nested death explosion needs another retained frame, not a copy of
      // this one that might later be overwritten by its parent's callback.
      c.hurts.push(frame);
      try {
        this.begin(binding, 'damage'); this.change.value = amount; this.change.onShield = onShield;
        this.change.source = source;
        if (info) {
          const saved = this.damageInfo;
          saved.explosion = info.kind === 'explosion';
          saved.amount = info.amount; saved.typed = info.typed; saved.generic = info.generic;
          saved.known = info.known;
          saved.pointState = info.point === undefined ? 'absent' : info.point === null ? 'null' : 'value';
          if (saved.pointState === 'value') saved.point.copy(info.point);
          saved.sourcePresent = Boolean(source); if (source) saved.source.copy(source);
          this.change.damageInfo = saved;
        }
        this.change.attacker = attacker;
        this.change.flag = info?.kind === 'explosion';
        this.commit();
      } finally { c.hurts.pop(); this.depth--; frame.source = frame.attacker = frame.info = null; }
    };
    c.explode = (explosion, point, owner = null, scale = 1) => {
      const frame = this.damageFrame();
      frame.source = point;
      frame.attacker = owner;
      frame.info = null;
      frame.blast = true;
      c.hurts.push(frame);
      try {
        const id = this.combatSim.explosion(explosion);
        this.begin(binding, 'explode'); this.change.index = id; this.change.vector.copy(point);
        this.change.attacker = owner; this.change.value = scale; this.commit();
      } finally { c.hurts.pop(); this.depth--; frame.source = frame.attacker = null; }
    };
    this.wireArsenal(binding);
    const body = binding.unit.soldier;
    body.look = (dx, dy, sensitivity = 1, invert = false) => this.look(binding, 'look', dx, dy, sensitivity, invert);
    if (!binding.unit.walker) {
      body.lookRaw = (dx, dy, sensitivity = 1, invert = false) => this.look(binding, 'lookRaw', dx, dy, sensitivity, invert);
      body.setAir = air => { this.begin(binding, 'bodyAir'); this.change.field = Air_INDEX.get(air); this.commit(); };
    }
  }

  // PlayerCombat.reset replaces arsenal identity. Capture that life's key
  // in each facade so an old weapon cannot mutate the replacement arsenal.
  // Selecting the current weapon succeeds without a boundary crossing, as
  // Arsenal.select does; settings still execute in source order.
  wireArsenal(binding) {
    binding.unit.combat.arsenal.bindOwner(this,binding);
  }

  // WeaponFacade.w observes just this weapon's complete retained cache row.
  // A version skips repeated scalar getters. Reading the current row rather
  // than accumulating masks also covers several unobserved native updates.
  readWeapon(weapon) {
    const arsenal = weapon.arsenal, binding = arsenal.binding;
    if (weapon.mirrorVersion === binding.weaponVersion || arsenal !== weapon.combat.arsenal) return;
    const b = this.bridge; b.views();
    const words = b.f, source = binding.wordsAt;
    const heap = (source + words[source + BlockHeaderLayout.heap]) | 0;
    const at = source + BLOCK_HEADER + F3PublicationLayout.combat + F3CombatViewLayout.weapons;
    syncF3WeaponView(b,words,heap + words[at] + weapon.index * F3WeaponViewLayout.words,
      heap,weapon.combat.state.arsenal.weapons[weapon.index]);
    weapon.mirrorVersion = binding.weaponVersion;
    binding.published();
  }

  // CombatFacade camera vectors are rarely observed by a headless world.
  // Read their complete current value from the retained cache on demand,
  // preserving vector identity after any number of unobserved publications.
  readCombatBasis(combat, vector, offset) {
    const binding = this.bindings[combat.owner.id], b = this.bridge;
    b.views();
    const at = (binding.wordsAt + BLOCK_HEADER + F3PublicationLayout.combat + offset) | 0;
    vector.set(b.f[at],b.f[at + 1],b.f[at + 2]);
    binding.published();
  }

  begin(binding, action, key = binding.view.key) {
    const c = this.change;
    c.key.slot = key.slot; c.key.generation = key.generation; c.key.life = key.life;
    c.action = action; c.field = 0; c.index = 0; c.value = 0; c.flag = false;
    c.source = null; c.damageInfo = null; c.attacker = null; c.controller = null; c.metadata = null;
    c.lease = null;
  }
  // ClassicCombatBinding changes the mounted arsenal between owning phases.
  // Publish that single lease now, so an intervening body/seat mirror cannot
  // replace the pending AI generator with the default seat stream.
  mountedRandom(lease, draw) {
    if (this.active) throw Error('mounted generator command inside the owned phase');
    const binding = this.bindings[lease.unit.id];
    this.begin(binding,'mountedRandom'); this.change.key.life = lease.generation;
    this.change.lease = lease; this.change.value = this.random.register(draw);
    this.commit();
    return this.layout.accepted;
  }
  // Commands publish the touched row before returning: supply and AI code
  // can read the result immediately. Save the counter owner because a
  // synchronous death hook can issue a nested command on the same bridge.
  commit() {
    const b = this.bridge;
    if (this.aiming) {
      const c = this.change, row = this.aimChange;
      if (c.action !== 'combatField' && c.action !== 'weaponField' && c.action !== 'randomBinding') throw Error('unsupported mutation from F3 aimer');
      if (this.aimHeader.count === this.layout.aimCapacity) throw Error('F3 aimer delta capacity exceeded');
      row.key.slot = c.key.slot; row.key.generation = c.key.generation; row.key.life = c.key.life;
      row.action = c.action; row.field = c.field; row.index = c.index; row.value = c.value;
      b.views();
      putF3AimChange(b, row, b.f, this.layout.aimInput / 8 + this.aimHeader.count * this.layout.aimWords, this.heap);
      this.aimHeader.count++;
      this.counters.inputFields += this.layout.aimWords;
      return;
    }
    this.random.push();
    this.owners.prepare(this.change.attacker);
    b.views(); this.heap.clear(); putF3Change(b, this.change, b.f, this.layout.command / 8, this.heap);
    this.counters.wasmCalls++;
    this.counters.commandCalls++;
    this.counters.inputFields += this.layout.commandWords;
    const previous = b.phaseCounters, active = this.active; b.phaseCounters = this.counters;
    this.active = true;
    try { if (!this.x.f3_command()) throw Error('native unit command rejected invalid state or input'); }
    finally { this.active = active;b.phaseCounters = previous; }
    this.random.pull();
    this.sync();
  }
  // The content facade handles its own construction settings. Every live
  // scalar here names an adopted body and carries both identity generations.
  scalar(slot, action, field, index, value) {
    const binding = this.bindings[slot];
    if (!binding) throw Error('scalar command before unit adoption');
    this.begin(binding, action === 'combatField' && field === C.team ? 'team' : action);
    if (action === 'combatField' && field === C.player) this.change.metadata = this.unitMetadata(binding.unit);
    this.change.field = field;
    this.change.index = index;
    this.change.value = value;
    this.commit();
    if (action === 'combatField' && field === C.random) this.random.combat(binding);
    return this.layout.accepted;
  }

  // Soldier.look/lookRaw and Droideka.look keep mouse samples in order.
  // A sample clamps and normalizes separately, so summing look deltas at a
  // later tick boundary would change the body's turn at the limits.
  look(binding, action, dx, dy, sensitivity, invert) {
    this.begin(binding, action);
    this.change.vector.set(dx, dy, sensitivity);
    this.change.flag = invert;
    this.commit();
  }
  // Unit.spawn: character/controller/team are new-life metadata, while the
  // command key still names the old life. Native spawn advances that life.
  // Reset JS intent as the original spawn does; movement input from the
  // previous controller must not survive reuse of a pooled body.
  spawn(unit, x, y, z, yaw) {
    const binding = this.bindings[unit.id], m = this.unitMetadata(unit);
    this.begin(binding, 'spawn');
    this.change.metadata = m;
    this.change.vector.set(x, y, z);
    this.change.value = yaw;
    this.commit();
    unit.intent = blankIntent();binding.observeIntent();
    unit.presentation.reset(); unit.animator.reset(yaw); unit.actor.root.position.copy(unit.presentation.position);
  }
  remove(unit) {
    this.begin(this.bindings[unit.id], 'remove');
    this.change.metadata = this.unitMetadata(unit); this.commit();
  }
  // LocalServer's body-only reset preserves Unit life and combat clocks.
  placeBody(unit,x,y,z,yaw) {
    this.begin(this.bindings[unit.id], 'bodySpawn');
    this.change.vector.set(x,y,z); this.change.value = yaw; this.commit();
  }
  // PathExecutor.contactSidestep consumes this narrow body value after
  // reading it. Preserve the existing vector and the lazy shape stamp.
  consumeContact(unit) {
    this.begin(this.bindings[unit.id], 'consumeContact'); this.commit();
  }
  // AnimatedProp.seize freezes the canonical body before carry can read it.
  // Resolve the grab's clip once per hold; the repeated placement has only
  // numeric clip and identity handles, plus the foreign node's matrix.
  seize(unit,prop,grab,tick) {
    const binding = this.bindings[unit.id];
    binding.heldClip = binding.heldClips.get(grab.clip) ?? null;
    binding.heldGrab = prop.grabs.indexOf(grab);
    this.poseCommand(binding,'seize',prop,tick,0,null);
  }
  carryHeld(unit,node,time) {
    const binding = this.bindings[unit.id], held = unit.held;
    this.poseCommand(binding,'carry',held.prop,held.since,time,node);
  }
  // A foreign prop command runs between owning phases. Its notice publishes
  // just this held object, preserving frame and body-vector identities.
  poseCommand(binding,action,prop,since,time,node) {
    if (this.active) throw Error('prop placement inside the owned phase');
    const b = this.bridge, row = this.pose, key = binding.view.key;
    row.key.slot = key.slot; row.key.generation = key.generation; row.key.life = key.life;
    row.action = action; row.prop = prop; row.since = since; row.time = time;
    row.grab = binding.heldGrab; row.clip = binding.heldClip;
    if (node) row.frame.copy(node); else row.frame.identity();
    b.views(); this.heap.clear(); putF3PoseCommand(b,row,b.f,this.layout.command / 8,this.heap);
    this.counters.wasmCalls++; this.counters.commandCalls++;
    this.counters.inputFields += this.layout.poseWords;
    if (!this.x.f3_pose()) throw Error('stale held-body placement');
    this.sync();
  }
  deadTime(unit,value) {
    this.begin(this.bindings[unit.id], 'deadTime'); this.change.value = value; this.commit();
  }
  // Game's literal player owner keeps its historical launch/raycast rules.
  // Only Character credit and team/damage notices resolve this registered body.
  registerViewpoint(unit) {
    this.begin(this.bindings[unit.id], 'viewpoint'); this.commit();
  }
  // Unit.letGo restores the held body's presentation after native ownership
  // clears the hold. Keep the no-hold early return: it is not a mutation and
  // must not create a command or disturb the actor's current rotation.
  letGo(unit) {
    if (!unit.held) return;
    this.begin(this.bindings[unit.id], 'letGo'); this.commit();
    unit.animator?.gunner?.(null); const root = unit.actor?.root;
    if (root) root.rotation.set(0, root.rotation.y, 0);
  }
  // Game.spawnUnit's post-spawn hero flag and network lifecycle metadata
  // are explicit commands. They never write a body back into the instance.
  unitMetadata(unit) {
    const m = this.metadata;
    m.character = unit.character; m.controller = unit.controller; m.team = unit.team; m.life = unit.generation;
    m.human = unit.human; m.networkHuman = unit.networkHuman; m.hero = unit.hero;
    return m;
  }
  publishMetadata(unit) {
    this.begin(this.bindings[unit.id], 'metadata');
    this.change.metadata = this.unitMetadata(unit); this.commit();
  }
  // MultiplayerWorld.addPlayer reconnects the current body without a spawn.
  // Advance the qualified life while retaining its arsenal and RNG stream.
  advanceLife(unit) {
    this.begin(this.bindings[unit.id], 'metadata');
    this.change.metadata = this.unitMetadata(unit);
    this.change.metadata.life = unit.generation + 1;
    this.commit();
  }
  parkViewpoint(unit) {
    this.begin(this.bindings[unit.id], 'unspawn'); this.change.flag = true; this.commit();
  }
  unspawn(unit) { this.begin(this.bindings[unit.id], 'unspawn'); this.commit(); }
  controller(unit, controller) {
    if (this.tailOwner) return this.tailOwner.controller(unit,controller);
    // ClassicController.unbind runs inside the synchronous Character leaf.
    // Its pointer becomes the leaf's typed reply, with no nested setter call.
    if (unit === this.deathUnit) { unit.controller = controller;return; }
    this.begin(this.bindings[unit.id], 'controller');this.change.controller = controller;this.commit();
  }

  // Apply only the rows the module published. A callback can read the same
  // descriptor repeatedly; its version prevents copying those rows twice.
  // Reset replaces arsenal facades; a reconnect only advances their key.
  // Body vectors and aliases are updated in place in either case.
  sync() {
    const b = this.bridge; b.views();
    // Unit.onMoved often has no new publication at all. During the native
    // span no outer notices or construction work can drain here. Check only
    // the owning versions and structural event cursor before the full reader.
    // AI control and F4 versions can advance during their own native phases
    // without a Unit publication; those owners must finish their barrier too.
    const d = this.descriptor, f = b.f, fleet = this.fleet, live = this.liveOwner, tail = this.tailOwner, ai = this.aiOwner;
    if (this.active && this.lastVersion === f[d + F3BoundaryLayout.version] &&
        this.birthEpoch === f[d + F3BoundaryLayout.eventEpoch] &&
        this.birthWords === f[d + F3BoundaryLayout.eventWords] &&
        (!fleet || fleet.lastVersion === f[fleet.descriptor + F2PhaseLayout.version]) &&
        (!live || live.lastVersion === f[live.descriptor + F4PhaseLayout.version]) &&
        (!tail || !live || tail.lastVersion === f[live.descriptor + F4PhaseLayout.version]) &&
        (!ai || ai.controlRefs.version === f[ai.descriptor + AiPhaseBoundaryLayout.controlVersion])) {
      this.counters.outputFields += (fleet ? 4 : 3) + (live ? 1 : 0) + (tail && live ? 1 : 0) + (ai ? 1 : 0);
      return;
    }
    this.readLayout();
    this.bindBirths();
    // Hull damage and lease changes share this mirror barrier. Resolve new
    // leases before a Unit publication names them, without another export.
    if (this.fleet) this.fleet.sync();
    this.aiOwner?.syncControls();
    if (this.lastVersion === this.layout.version) { this.liveOwner?.sync();this.tailOwner?.sync(); this.drainEvents(); this.items.flush(); this.construction.finish(); return; }
    this.lastVersion = this.layout.version;
    this.counters.unitRows += this.layout.outputRows;
    this.counters.unitWords += this.layout.outputWords;
    let at = (this.layout.output / 8) | 0;
    for (let i = 0; i < this.layout.outputRows; i++, at += this.layout.outputStride) {
      const slot = b.f[at + F2DeltaLayout.slot];
      const binding = this.bindings[slot];
      if (!binding) throw Error('unknown F3 publication');
      // The borrowed cache already contains every unchanged word. Decode it
      // in place, copying only the mask that an aimer may need to extend.
      // Neither a typed-array slice nor another per-unit row is constructed.
      const source = applyDelta(b.f,at,this.layout.outputHeap/8,null,binding.dirty);
      if (!source) throw Error('unit publication requires a borrowed native row');
      const words = binding.words = b.f;
      binding.wordsAt = source;
      if (binding.aimed) {
        // Movement supplied to an aimer can be undone by separation. Restore
        // borrowed fields from retained canonical words even if their final
        // bits equal the old baseline and therefore have no native delta.
        const base = source + BLOCK_HEADER + F3PublicationLayout.body, u = F3UnitMirrorLayout;
        markDelta(binding.dirty, base + u.position, 3, source);
        markDelta(binding.dirty, base + u.velocity, 3, source);
        markDelta(binding.dirty, base + u.tickCount, 1, source);
        const walker = binding.unit.walker;
        const motion = base + (walker ? u.droideka : u.soldier) + 1;
        const fields = walker ? F3DroidekaMotionLayout : F3SoldierMotionLayout;
        // The aimer borrows facing and posture, not normals, fuel or clocks.
        // Restore exactly its declared fields; leave other motion masks sparse.
        markDelta(binding.dirty, motion + fields.aimYaw, 2, source);
        if (!walker) markDelta(binding.dirty, motion + fields.posture, 1, source);
        binding.aimed = false;
      }
      const rowAt = source + BLOCK_HEADER, heapAt = (source + words[source + BlockHeaderLayout.heap]) | 0;
      syncF3UnitKey(b,words,rowAt,0,this.key);
      if (!binding || binding.view.key.generation !== this.key.generation) throw Error('stale F3 publication');
      const c = binding.unit.combat;
      c.phase = this;
      // Only lifecycle changes create facades here. An ordinary publication
      // mutates the existing weapon rows read by AI and network capture.
      const lifeChanged = binding.life !== this.key.life;
      if (lifeChanged) {
        binding.life = this.key.life;
        // Native waves reset the same canonical intent as Unit.spawn. This
        // scalar mirror reset precedes the spawned AI hook, including reuse
        // of a corpse whose old axes/aim are still visible in JavaScript.
        const intent=binding.unit.intent;
        intent.forward=0;intent.right=0;intent.turn=0;intent.pitch=0;
        intent.jump=false;intent.crouch=false;intent.prone=false;intent.use=false;
        intent.buttons[0]=0;intent.buttons[1]=0;intent.reload=false;intent.nextPrimary=false;
        intent.previousPrimary=false;intent.nextSecondary=false;intent.zoom=false;
        intent.aim=null;intent.jetDirection=null;intent.exit=false;
        delete intent.flight;delete intent.brake;delete intent.up;delete intent.down;
        binding.arsenalKey.life = this.key.life;
        this.items.schedule(binding);
      }
      const arsenal = words[rowAt + F3PublicationLayout.arsenal];
      const resetArsenal = binding.arsenal !== arsenal;
      if (resetArsenal || deltaChanged(binding.dirty,heapAt,
          words[source + BlockHeaderLayout.heapWords],source)) binding.weaponVersion++;

      if (resetArsenal) {
        binding.arsenal = arsenal;
        // A body may return to its pool and respawn before this publication.
        // Clear the old life's drawing resources before replacing its arsenal.
        c.clearCharge();
        // Reset and class realization prepare the complete new list once.
        // Ordinary ticks keep its row objects and decode only observed weapons.
        syncF3CombatView(b,words,rowAt + F3PublicationLayout.combat,heapAt,c.state);
        c.arsenal.nativeWeaponsAt = -1;
        c.arsenal = new c.arsenal.constructor(c, c.def.weapons || []);
        this.wireArsenal(binding);
        this.items.schedule(binding);
      }
      // Weapon getters share one current heap base per consumed publication.
      // Acknowledgement can swap cache buffers even when weapon bits stay
      // equal, so rebind this address with the touched Unit's row each time.
      c.arsenal.nativeWeaponsAt = (heapAt + words[rowAt + F3PublicationLayout.combat +
        F3CombatViewLayout.weapons]) | 0;
      // An unchanged body skips decoding. Rebind the reserved optional water
      // record only when a body delta can expose it again.
      const bodyChanged = resetArsenal || deltaChanged(binding.dirty,rowAt,F3PublicationLayout.arsenal,source);
      const waterAt = rowAt + F3PublicationLayout.body + F3UnitMirrorLayout.waterContact;
      if (resetArsenal || deltaChanged(binding.dirty, waterAt,
          F3UnitMirrorLayout.words - F3UnitMirrorLayout.waterContact,source)) {
        binding.view.waterContact = binding.waterContact;
      }
      // Unit.tick's optional jet direction reuses cold storage when present.
      // A null interval must not cause the next jump to allocate a vector.
      const nativeIntent = words[rowAt + F3PublicationLayout.controls] === 1;
      const ownershipChanged = binding.nativeIntent !== nativeIntent;
      binding.nativeIntent = nativeIntent;
      // Foreign input remains a plain writable object. Only native ownership
      // selects the cold observer facade, keeping scalar decoding allocation-free.
      if (ownershipChanged) binding.unit.intent=nativeIntent ? binding.intentObserver.intent : binding.intentObserver.values;
      const intents = this.layout.controlsConsumed || nativeIntent || ownershipChanged;
      // Unit.tick consumes controls even when their native bits equal the
      // preceding baseline. Commands leave pending JS intents alone. Select
      // that generated reader here without writing into the native cache.
      if (bodyChanged) syncF3UnitMirrorDelta(b,words,rowAt + F3PublicationLayout.body,
        heapAt,binding.view,binding.dirty,resetArsenal,source);
      const combatAt = rowAt + F3PublicationLayout.combat;
      if (resetArsenal || deltaChanged(binding.dirty,combatAt,
          F3PublicationLayout.controls - F3PublicationLayout.combat,source) ||
          deltaChanged(binding.dirty,heapAt,words[source + BlockHeaderLayout.heapWords],source)) {
        syncF3CombatViewDelta(b,words,combatAt,heapAt,c.state,binding.dirty,resetArsenal,source,true);
      }
      // Changing control owners changes the row representation. Equal zero
      // words still need restoration because the old foreign mirror can hold
      // a different axis (for example, a rider returning to its own body).
      // Bot input and latch state have no host consumer. The narrow held
      // record retains the existing diagnostic/aimer reads without decoding
      // the movement input a second time after native F3 has consumed it.
      // Native held inputs are observed directly from this retained row.
      // No eager field decode is needed for canonically controlled bots.
      const controlsAt = rowAt + F3PublicationLayout.controls;
      if (intents && words[controlsAt] === 0 && (!binding.nativeIntent || resetArsenal ||
          deltaChanged(binding.dirty,controlsAt + 1,F3ControlViewLayout.words,source))) {
        // Native input is never written by the page. An unchanged control
        // group can retain its facade, including null aim/vector intervals.
        // Human input still needs the full one-shot restoration each tick.
        const controlRow=heapAt+words[controlsAt+1+F3ForeignControlsLayout.values];
        const intentAt = controlRow + F3ControlViewLayout.intent;
        const restoreIntent = !binding.nativeIntent || resetArsenal || lifeChanged;
        if (restoreIntent || deltaChanged(binding.dirty,intentAt + IntentLayout.aim,
            IntentLayout.jetDirection - IntentLayout.aim,source)) binding.unit.intent.aim = binding.intentAim;
        if (restoreIntent || deltaChanged(binding.dirty,intentAt + IntentLayout.jetDirection,
            IntentLayout.exit - IntentLayout.jetDirection,source)) binding.unit.intent.jetDirection = binding.intentJetDirection;
        // Page-owned one-shots can differ from unchanged canonical bits.
        // Restore those eagerly, including deleted fields on a new life.
        // Ordinary native intents have no page writer.
        if (!binding.nativeIntent || lifeChanged) syncIntent(b,words,intentAt,heapAt,binding.unit.intent);
        // One-shot intent buttons are consumed every tick, including equal
        // native bits. Held input and latch state only need changed stores.
        const inputAt = controlRow + F3ControlViewLayout.input;
        if (resetArsenal || deltaChanged(binding.dirty,inputAt,F3ControlViewLayout.words - F3ControlViewLayout.input,source)) {
          binding.unit.input.jetDirection = binding.inputJetDirection;
        }
        syncF3ControlViewDelta(b,words,controlRow,heapAt,binding.unit,binding.dirty,resetArsenal || ownershipChanged,source);
      }
      // A weapon clock does not require reassigning every Unit flag and
      // actor visibility. Debug stamps still consume the changed combat row.
      const bodyAt = rowAt + F3PublicationLayout.body;
      const metadataChanged = resetArsenal || deltaChanged(binding.dirty,bodyAt,
        F3UnitMirrorLayout.tickCount,source) || deltaChanged(binding.dirty,
        bodyAt + F3UnitMirrorLayout.remoteActive,
        F3UnitMirrorLayout.position - F3UnitMirrorLayout.remoteActive,source) ||
        deltaChanged(binding.dirty,bodyAt + F3UnitMirrorLayout.waterContact,1,source);
      // Ordinary Unit.tick changes its clock and borrowed vectors without
      // changing membership, control or actor visibility. Those rare edges
      // still apply together; movement needs just its final tick stamp.
      if (metadataChanged) binding.apply();
      else {
        if (bodyChanged) binding.unit.tickCount = binding.view.tickCount;
        binding.published();
      }
      c.kept.mounted = c.state.mounted; c.kept.gunUnderwater = c.state.gunUnderwater; c.kept.team = binding.unit.team;
    }
    this.counters.outputFields += this.layout.outputWords;
    this.acknowledgement.version = this.layout.version;
    putF3Acknowledgement(b,this.acknowledgement,b.f,this.layout.acknowledgement/8,this.heap);
    this.counters.inputFields += F3AcknowledgementLayout.words;
    this.liveOwner?.sync();this.tailOwner?.sync();
    this.drainEvents();
    this.items.flush();
    this.construction.finish();
  }

  // F3OwnerCreated contains only metadata and already-bound source handles.
  // It can bind a reserved frozen facade without gameplay callbacks or a
  // nested module call, before a Unit's lastAttacker reference is decoded.
  bindBirths() {
    if (this.birthEpoch !== this.layout.eventEpoch) {
      this.birthEpoch = this.layout.eventEpoch;this.birthOffset = 0;
    }
    this.birthOffset = this.bridge.drainFrom(this.layout.events,this.birthOffset,
      this.eventMirrors,this.birthHandlers,true);
    this.birthWords = this.layout.eventWords;
  }

  // instrument's shot/damage hooks receive values captured at emission time.
  // One scratch shot object serves the callbacks; the journal copies fields
  // it retains. A notification never calls a per-method simulation adapter.
  notifications() {
    const shot = {weapon:null,repair:false,meleeStart:false,swing:0,yaw:0,pitch:0,charge:0,targetId:0,targetGeneration:0,admission:null};
    const info = {kind:undefined,amount:undefined,typed:undefined,generic:undefined,known:undefined,point:undefined,source:null};
    const ordnance = this.factory.context.ordnance;
    const launch = {tick:null,sample:null};
    const impact = {point:null,normal:null,surface:0,terrain:undefined,body:undefined,unit:undefined,part:undefined};
    const explosionContext = {wet:false};
    return {
      // Fleet presentation shares this cursor, keeping native shot/death
      // order without a second drain or a callback inside the owning phase.
      f2Shot: event => {
        const fleet = this.factory.context.vehicles;
        fleet.f2.firing.presentShot(event);
        fleet.world.fired(event.hull,event.seat,fleet.f2.firing.shot(event),event.owner,event.origin,event.direction,event.sample);
      },
      f2Tow: event => this.factory.context.vehicles.f2.firing.presentTow(event),
      f3Seat: event => this.factory.context.vehicles.f2.seats.notice(event),
      f3Turret: event => this.factory.context.vehicles.f2.seats.turretNotice(event),
      f3ReconContextCreated: event => this.recon.contextCreated(event),
      f3ReconDroidCreated: event => this.recon.droidCreated(event),
      f3ReconContext: event => this.recon.contextState(event),
      f3ReconDroid: event => this.recon.droidState(event),
      f3ReconRetired: event => this.recon.retired(event),
      f3ReconShot: event => this.recon.fired(event),
      f3OwnerCreated: event => this.owners.created(event),
      f3UnitBirth: event => this.construction.created(event),
      f4CharacterBirth: event => this.tailOwner.ensureCharacter(event),
      aiControlBirth: event => this.aiOwner.controlBorn(event),
      aiControllerBirth: event => this.aiOwner.controllerBorn(event),
      aiVoice: event => this.aiOwner.notices.voice(event),
      aiItemOwner: event => this.aiOwner.notices.itemOwner(event),
      f3LifeStreams: event => this.construction.streams(event),
      f3HullBirth: event => this.hulls.created(event),
      f3Item: event => this.items.snapshot(event),
      f3PowerupEffect: event => this.presentEffect(event.name,event.position),
      f3Pickup: event => this.items.items.world.pickup(event.def,event.position),
      f3Held: event => {
        const binding = this.bindings[event.key.slot], unit = binding.unit;
        if (unit.generation !== event.key.life || binding.view.held !== true) return;
        const held = binding.held;
        held.prop = event.prop; held.grab = event.prop.grabs[event.grab]; held.since = event.since;
        held.frame = event.placed ? binding.heldFrame.copy(event.frame) : undefined;
        held.time = event.time; unit.held = held;
        binding.published();
      },
      f3ItemRetired: event => this.items.retired(event),
      f3Dispenser: event => this.items.owned(event),
      f3Death: event => this.factory.context.deathNotice?.(this.bindings[event.key.slot].unit,event),
      f3Body: event => {
        const unit = this.bindings[event.key.slot].unit;
        if (event.event === 'deathBlast') this.factory.context.deathBlast?.(unit,event.key);
        else if (event.event === 'respawn') unit.combat.onRespawn();
        else this.factory.context.bodyNotice?.(unit,event.health,event.key);
      },
      f3Splash: event => this.presentEffect('watersplash_md',event.at),
      f3Blast: event => {
        const def = ordnance.sim.explosions.at(event.explosion);
        if (event.stage === 'present') {
          ordnance.presentExplosion(def,event.position,event.wet);
          this.factory.context.blastPresent?.(def,event);
        }
        else if (event.stage === 'event') this.factory.context.blastNotice?.(def,event);
        else {
          explosionContext.wet = event.wet;
          ordnance.onExplosion(def,event.position,event.owner,explosionContext);
        }
      },
      f3PropDamage: event => this.props.damaged(event),
      f3Hull: event => this.hulls.notice(event),
      f3Removed: event => {
        const binding = this.bindings[event.key.slot];
        // F2 can acquire this pooled body later in the same native span. An
        // old removal notice must not hide that body's newly published life.
        if (binding.life !== event.key.life) return;
        binding.unit.undraw(); binding.unit.onRemoved(binding.unit);
      },
      f3Shot: event => {
        const c = this.bindings[event.key.slot].unit.combat, row = event.shot;
        shot.weapon = c.arsenal.weapons[row.weapon]; shot.repair = row.kind === 'repair';
        shot.meleeStart = row.kind === 'meleeStart'; shot.swing = row.swing;
        shot.yaw = row.yaw; shot.pitch = row.pitch; shot.charge = row.charge;
        shot.targetId = row.targetId; shot.targetGeneration = row.targetGeneration; shot.admission = row.admission;
        c.onFired?.(shot, event);
      },
      f3Damage: event => {
        const c = this.bindings[event.key.slot].unit.combat, row = event.info;
        // Nullable vectors use retained fixed fields in the boundary. Switching
        // from a fall's null source to a round's point creates no new vector.
        info.kind = row.explosion ? 'explosion' : undefined;
        info.amount = row.amount; info.typed = row.typed; info.generic = row.generic; info.known = row.known;
        info.point = row.pointState === 'absent' ? undefined : row.pointState === 'null' ? null : row.point;
        info.source = row.sourcePresent ? row.source : null;
        c.onDamaged?.(event.amount, event.attacker, info, event);
      },
      f3Presentation: event => {
        // The journal consumer snapshots this notice. Keeping generated vectors
        // here avoids arrays or action objects in the mirror delivery path.
        this.bindings[event.key.slot].unit.combat.onPresentation?.(event, event);
      },
      f3Shield: event => {
        const unit = this.bindings[event.key.slot].unit;
        this.factory.context.shieldHit?.(unit, event.taken, event.sourcePresent ? event.source : null,
          event.explosion ? 'explosion' : undefined, event.key);
      },
      f3Effect: event => {
        const unit = this.bindings[event.key.slot].unit;
        if (event.effect === 'headshot') this.presentEffect('headshot', event.position);
        else {
          this.factory.context.effects?.play(event.effect === 'meleeHit' ? 'lightsabre_hit' : 'lightsabre_deflect', event.position);
          if (event.effect === 'meleeDeflect') unit.combat.onPresentation?.(event, event);
        }
      },
      f3Ordnance: event => {
        if (event.event === 'bounce') {
          ordnance.world.bounce(event.def, event.position, event.speed); return;
        }
        const type = event.key.kind;
        const kind = type === 'bolt' ? KIND_BOLT : type === 'beam' ? KIND_BEAM : type === 'grenade' ? KIND_GRENADE : KIND_ROUND;
        const projectile = this.bridge.refs[kind].at(event.key.slot, event.key.generation);
        // A native launch may be the mirror's first observation. Its immutable
        // metadata is construction state; sampled poses stay on the notice.
        if (!projectile.def) {
          projectile.def = event.def; projectile.owner = event.owner; projectile.networkFireChannel = event.channel;
        }
        if (event.event === 'projectile') ordnance.onProjectile(projectile, type, event);
        else if (event.event === 'launch') {
          launch.tick = event.tick; launch.sample = event;
          ordnance.onLaunch(projectile, launch);
        } else if (event.event === 'grenadeContact') ordnance.onGrenadeContact(projectile, event);
        else {
          impact.point = event.point; impact.normal = event.normal; impact.surface = event.surface;
          const hit = event.hasStruck ? event.struck : null;
          impact.terrain = hit?.terrain; impact.body = hit?.body; impact.unit = hit?.unit; impact.part = hit?.part;
          ordnance.onImpact(projectile, impact);
        }
      },
    };
  }

  // Publications expose the shared event buffer directly. Offset/epoch keep
  // repeated mirror reads from delivering an already observed notification.
  // Acknowledge the current notification before an observer can issue a
  // command. Nested syncs publish mirrors, while this outer pump keeps sole
  // ownership of the reusable event mirrors and the unread journal suffix.
  consumeEvent(offset) {
    this.eventOffset = offset;
    const b=this.bridge,a=this.acknowledgement;
    a.eventEpoch=this.eventEpoch;a.eventOffset=offset;
    putF3Acknowledgement(b,a,b.f,this.layout.acknowledgement/8,this.heap);
    this.counters.inputFields += F3AcknowledgementLayout.words;
  }
  drainEvents() {
    if (this.active || this.delivering) return;
    this.delivering=true;
    try {
      for (;;) {
        if (this.eventEpoch !== this.layout.eventEpoch) {
          this.eventEpoch=this.layout.eventEpoch;this.eventOffset=0;
        }
        const b=this.bridge,base=this.layout.events/8;
        b.views();
        this.props.drainEffects();
        if (this.eventOffset>=b.f[base]) break;
        const before=this.eventOffset;
        const next=b.drainFrom(this.layout.events,before,this.eventMirrors,this.eventHandlers,false,this);
        this.counters.outputFields += next-before;
        // Only read the descriptor here. An observer's completed command may
        // have begun a new epoch after consuming the old journal's last event.
        this.readLayout();
      }
    } finally {this.delivering=false;}
    this.owners.reserveFuture();
    this.aiOwner?.finishPublication();
  }

  // CollisionWorld.rayBodies: membership names bodies only when it changes.
  // Enabled flags are foreign input, in collision-world order, rather than
  // a reason to copy every collision body or every Unit before each query.
  bodiesIn() {
    const c = this.collision, b = this.bridge;
    c.rayBodies();
    if (this.bodyVersion !== c.bodiesVersion) {
      const count = c.bodies.length;
      b.views(); let at = this.layout.input / 8;
      for (let i = 0; i < count; i++) {
        this.bodyName.body = c.bodies[i]; this.bodyName.kernel = c.bodies[i].raySlot;
        putF3BodyName(b, this.bodyName, b.f, at, this.heap); at += this.layout.bodyNameWords;
      }
      this.x.f3_body_names(count); this.bodyVersion = c.bodiesVersion;
      this.counters.wasmCalls++;
      this.counters.lifecycleCalls++;
      this.counters.inputFields += count * this.layout.bodyNameWords + 1;
    }
    b.views();
    for (let i = 0; i < c.bodies.length; i++) b.u8[this.flags + i] = c.bodies[i].enabled ? 1 : 0;
    this.counters.inputFields += c.bodies.length;
  }

  // The Unit.tick loop consumes intents and trigger facts, never body rows.
  // Keep the first edge for each button. An empty networkShots array is
  // still present and suppresses networkShot, matching networkShotFor.
  // The native loop keeps construction order, including pooled corpses.
  preparePhase(dt, tick = this.tick) {
    // Instrumentation resets cannot rewind a recon owner's once-per-tick
    // clock. Production supplies the world tick; standalone battles keep it.
    this.tick = tick + 1;
    const b = this.bridge, built = this.factory.built;
    if (this.debug) this.check();
    // Structural body membership prepares query capacity before fleet timing
    // begins. Its first buffer growth belongs to construction, not physics.
    this.bodiesIn();
    if (this.fleet) this.fleet.prepareStep();
    this.random.push();
    let count = 0, networks = 0, nativeIntents = 0, at = this.layout.input / 8;
    for (let i = 0; i < built.length; i++) {
      const u = built[i]; if (u.removed) continue;
      const binding = this.bindings[u.id];
      // Preserve explicit remote/network facts, including an empty shot list.
      // Native bots otherwise need no foreign row: Rust resolves their input
      // directly from the same canonical controller and unit generation.
      binding.inputNetwork = Boolean(u.networkShots || u.networkShot ||
        u.networkViewTick !== undefined || u.networkInputLeadTicks !== undefined || u.networkPlayoutMaxTicks !== undefined);
      this.input.remoteFirstPerson = Boolean(u.combat.remotes?.firstPerson);
      if (binding.nativeIntent && !binding.inputNetwork && !this.input.remoteFirstPerson) continue;
      this.input.key = binding.view.key;
      if (binding.nativeIntent) {
        this.input.intent = null;this.input.botTargetSet = false;this.input.botTarget = null;nativeIntents++;
      } else {
        this.input.intent = u.intent;
        const target = botLockTarget(u);
        this.input.botTargetSet = target !== undefined;this.input.botTarget = target ?? null;
      }
      this.input.hasNetwork = binding.inputNetwork;
      this.input.networkIndex = binding.inputNetwork ? networks++ : 0;
      putF3PhaseInput(b, this.input, b.f, at, this.heap); at += this.layout.inputStride; count++;
    }
    // networkShotFor consumes this sparse tail after all intent rows. Keep
    // first-button order and leave retirement to the network input owner.
    const input = this.networkInput;
    for (let i = 0; i < built.length; i++) {
      const u = built[i]; if (u.removed || !this.bindings[u.id].inputNetwork) continue;
      input.edgeList = Boolean(u.networkShots); input.edge = u.networkShot ?? null;
      input.edge0 = input.edge1 = null;
      if (u.networkShots) for (let k = 0; k < u.networkShots.length; k++) {
        const edge = u.networkShots[k];
        if (edge.button === 0 && !input.edge0) input.edge0 = edge;
        if (edge.button === 1 && !input.edge1) input.edge1 = edge;
      }
      const net = input.network;
      net.view = u.networkViewTick; net.lead = u.networkInputLeadTicks; net.playout = u.networkPlayoutMaxTicks;
      const edge0 = input.edgeList ? input.edge0 : input.edge;
      const edge1 = input.edgeList ? input.edge1 : input.edge;
      net.fire0 = edge0?.id ?? 0; net.fire1 = edge1?.id ?? 0;
      net.view0 = edge0?.viewTick; net.view1 = edge1?.viewTick;
      putF3PhaseNetwork(b, input, b.f, at, this.heap); at += this.layout.networkWords;
    }
    this.registry.foreign(count, networks);
    const scope = this.separationScope;
    this.separation.global = scope.global;
    this.separation.camera = scope.global ? this.globalSeparationCamera : scope.camera;
    putF3Separation(b, this.separation, b.f, this.layout.separation / 8, this.heap);
    this.counters.ticks++;
    this.counters.phaseCalls++;
    this.counters.wasmCalls++;
    this.counters.inputFields += count * this.layout.inputStride - nativeIntents * IntentLayout.words + networks * this.layout.networkWords + this.layout.separationWords + 4;
    if (this.fleet) this.counters.inputFields += this.fleet.preparedWords;
    return count;
  }

  // The world entry owns the whole interval; standalone fixtures use this
  // same input writer and publication reader around the native unit entry.
  phase(dt, tick = this.tick) {
    const count = this.preparePhase(dt,tick), b = this.bridge;
    this.active = true;
    const previous = b.phaseCounters; b.phaseCounters = this.counters;
    try { if (!this.x.f3_step(count, dt, this.water ? this.water.level : NaN, tick)) throw Error('native unit phase rejected invalid state or input'); }
    finally { this.active = false; b.phaseCounters = previous; }
    this.finishPhase();
  }
  finishPhase() {
    if (this.fleet) this.fleet.commitForeign();
    this.random.pull();
    this.sync();
    if (this.fleet) this.fleet.finishStep();
    this.counters.rustAllocations += this.layout.allocations;
    this.counters.rustReallocations += this.layout.reallocations;
    this.counters.rustAllocationBytes += this.layout.allocationBytes;
    if (this.debug) this.check();
  }

  // Check ordinary published values without trapping game objects.
  check() { for (let i = 0; i < this.bindings.length; i++) this.bindings[i]?.check(); }

  // Unit.onMoved runs after movement and before weapon update. Publish its
  // new body first, and return only the aim delta the aimer owns. These
  // retained reply objects avoid creating a result object every unit tick.
  // Removal notifications keep Character and presentation bookkeeping in
  // the same order as Unit.remove until those callers move into the store.
  handlers() {
    const aim = {aim:null};
    const death = {controller:null};
    return {
      unitAim: a => {
        const binding = this.bindings[a.unit], unit = binding.unit, dt = a.dt;
        this.random.pull(); this.sync();
        // Classic updateAim reads forward; Enhanced also reads its origin,
        // velocity and target's lazy sphere. Refresh exactly those facts here.
        // Other motion fields arrive once through the completed publication.
        const body = unit.soldier;
        body.aimYaw = a.aimYaw; body.aimPitch = a.aimPitch;
        body.position.copy(a.position); body.velocity.copy(a.velocity);
        if (!unit.walker) body.posture = a.posture;
        unit.tickCount = a.tickCount;
        binding.aimed = true;
        binding.published();
        this.aimHeader.count = 0; this.aiming = true;
        try { unit.onMoved(dt); }
        finally { this.aiming = false; }
        this.random.push();
        const b = this.bridge; b.views();
        putF3AimHeader(b, this.aimHeader, b.f, this.layout.aimHeader / 8, this.heap);
        this.counters.inputFields += this.layout.aimHeaderWords;
        aim.aim = unit.intent.aim; return aim;
      },
      // Game.onHitUnit updates Character tallies synchronously. This leaf
      // receives the sampled Character and cannot run a damage or fire adapter.
      f3HeadCredit: a => this.factory.context.headshotCredit?.(a.character),
      f3HullCredit: a => {
        this.sync();
        this.factory.context.hullDestroyedCredit?.(a.character);
      },
      f3DeathCredit: a => {
        this.random.pull();this.sync();this.deathUnit = a.unit;
        try { this.factory.context.deathCredit?.(a.unit,a.killer,a.relation); }
        finally { this.deathUnit = null;this.random.push(); }
        death.controller = a.unit.controller;return death;
      },
      f3CharacterRelease: a => {
        if (a.character.unit === a.unit) a.character.unit = null;
      },
    };
  }
}
