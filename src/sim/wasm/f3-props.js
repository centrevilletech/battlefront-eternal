import {sceneProperty} from '../../rust-render/scene-changes.js';
// LiveObjects' foreign health/life facts and touched prop publications. Native
// queries and Building's native kernel own combat traversal and recursive damage;
// this bridge retains ordinary prop objects for the unported world and page.
import { KIND_PROP, F3BuildingSetupMirror, F3BuildingStateMirror, putF3BuildingSetup, putF3BuildingState, sameTrState } from '../gen/index.js';
import { ObjectivePost, PowerupStation, Turret } from '../../props.js';

const LOOKS = ['built', 'destroyed', 'unbuilt'];

export class F3Props {
  constructor(phase, live) {
    this.phase = phase;
    this.buildings = live?.buildings ?? [];
    this.prior = [];
    this.looks = [];
    this.bodies = [];
    this.objectives = [];
    this.members = [];
    this.traps = [];this.shown=[];
    this.effectEpoch=-1;this.effectOffset=0;this.effectCount=0;
    this.effectMarked=new Uint8Array(this.buildings.length);
    this.effectTouched=new Uint32Array(this.buildings.length);
    this.effectHandlers={f3PropEffect:event=>this.effect(event)};
    this.row = new F3BuildingStateMirror();
    this.damageInfo = {direction:null, amount:0, typed:1, generic:1};
    this.setup = new F3BuildingSetupMirror();
    this.record = {words:phase.layout.buildingSetupWords, put:putF3BuildingSetup};
    // AnimatedProp commands retain stable foreign identities. Number them
    // before any world tick so a later grab does not create a registry row.
    const animated = live?.animated;
    if (animated) for (let i = 0; i < animated.length; i++) phase.bridge.refs[KIND_PROP].slotOf(animated[i]);
    // The three static model looks use their existing boxes and spheres.
    // Rust applies the matrix; no per-tick JS bound or rig sampling occurs here.
    for (let i = 0; i < this.buildings.length; i++) this.prepare(i);
  }

  // LiveObjects.buildings defines both damage visitation and the l-index used
  // by repairLockKey. Resolve handles and geometry once in that same order.
  prepare(index) {
    const p = this.phase, b = p.bridge, building = this.buildings[index];
    const row = this.setup, settings = row.settings;
    row.index = index; row.target = building; row.member = building.aiTarget ?? null;
    this.traps[index] = building.desc.base === 'trap';
    row.trapSphere = this.traps[index] ? building.padSphere : null;
    row.key = `l${index}`; row.damaged = typeof building.onDamaged === 'function';
    row.explosion = building.explosion ? p.factory.context.ordnance.sim.explosionId(building.explosion) : null;
    if (row.member) {
      this.members[row.member.id] = true;
      row.category = p.registry.facts[row.member.id].category; row.kind = row.member.kind;
    }
    // Constructors retain identity when the desktop bundler renames classes.
    settings.kind = building instanceof Turret ? 'turret' : building instanceof ObjectivePost ? 'objective' : 'building';
    this.objectives[index] = settings.kind === 'objective';
    settings.maxHealth = building.maxHealth; settings.respawnTime = building.respawnTime;
    settings.builtAddHealth = Number.parseFloat(building.desc.settings.AddHealth) || 0;
    settings.regenerates = building.regenerates;
    settings.hasDestroyed = Boolean(building.looks.destroyed);
    settings.hasUnbuilt = Boolean(building.looks.unbuilt);
    settings.hasExplosion = Boolean(building.explosion); settings.matrix = building.matrix;
    const looks = this.looks[index] = [], bodies = this.bodies[index] = [];
    settings.models.length = 0; row.bodies.length = 0;
    for (let k = 0; k < LOOKS.length; k++) {
      const look = building.looks[LOOKS[k]], model = look?.model;
      looks.push(look ?? null); bodies.push(building.bodies[LOOKS[k]] ?? null);
      row.bodies.push(bodies[k]);
      settings.models.push(model ? {sphere:model.sphere ?? null,
        low:model.box ? {x:model.box[0][0],y:model.box[0][1],z:model.box[0][2]} : null,
        high:model.box ? {x:model.box[1][0],y:model.box[1][1],z:model.box[1][2]} : null} : null);
    }
    this.shown[index]=building.state;
    this.fill(index, row.state);
    // Construction seeds the shared F4 row once, including station clocks and
    // the trap's prepared settings. Combat retains no second health frame.
    const live = row.live, station = live.state, health = station.building, source = row.state;
    live.settings = settings; live.attacker = source.attacker; live.crew = Boolean(building.crew); live.damaged = row.damaged;
    health.state = source.state; health.health = source.health; health.addHealth = source.addHealth;
    health.lastAttacker.kind = source.attacker === undefined ? 0 : source.attacker === null ? 1 : 2;
    health.lastAttacker.id = 0; health.targetGeneration = source.generation;
    station.mode = building.mode ?? 'waiting'; station.active = building.active ?? 0; station.pulse = building.pulse ?? 0;
    station.servedPlayer = Boolean(building.servedPlayer); station.servedHealth = Boolean(building.servedHealth); station.servedAmmo = Boolean(building.servedAmmo);
    live.supply = building instanceof PowerupStation ? {position:building.position, radius:building.radius, delay:building.delay,
      health:building.giveHealth, ammo:building.giveAmmo, vehicleHealth:building.vehicleHealth, vehicleAmmo:building.vehicleAmmo} : null;
    row.trap = this.traps[index] ? {state:source.trapState, attacker:source.attacker, settings:{maxHealth:building.maxHealth,
      reach:building.reach, minRadius:building.minRadius, deathTime:building.deathTime, buildFrames:building.buildClip?.frames ?? null,
      triggerFrames:building.triggerClip?.frames ?? null, triggerTeam:building.settings.triggerTeam ?? null,
      sound:building.settings.sound ?? null, deathEffect:building.settings.deathEffect ?? null, point:building.point, effectAt:building.effectAt}} : null;
    b.callArgs(this.record, row); p.x.f3_building();
    // The first phase still sends mutable state: callback installation can
    // finish after construction without requiring a full geometry reimport.
    const previous = this.prior[index] = new F3BuildingStateMirror();
    previous.index = -1;
    // Only scalar facts use this comparison row. Nested model data never needs
    // another import when health, team or target generation changes later.
  }

  // F4 may regenerate a prop or change its gunner before the unit span. These
  // scalars are its declared foreign inputs; model and body data stay prepared.
  fill(index, row) {
    const b = this.buildings[index];
    row.index = index; row.state = b.state; row.health = b.health;
    row.addHealth = b.addHealth; row.attacker = b.lastAttacker;
    row.generation = b.aiTarget?.generation ?? null;
    row.team = b.team; row.targetTeam = b.aiTarget?.team ?? b.team;
    // Turret.gunner is its Soldier presentation body. The qualified Unit
    // belongs to the lease; a body object cannot name a registry member.
    row.gunner = b.lease?.unit ?? null;
    row.trapMode = this.traps[index];
    if (row.trapMode) {
      // Trap's next world step observes sprung and its reset clock. Keep the
      // whole fixed clock frame, rather than treating zero health as death.
      const state = row.trapState;
      state.built = b.state === 'built'; state.health = b.health; state.phase = b.phase;
      state.started = b.started; state.time = b.time; state.sprung = b.sprung;
      state.tick = b.tick; state.crushed = b.crushed;
    }
  }

  // Keep comparison storage separate from the live JS object. Saving a view
  // of mutable fields would hide the next outside-phase change from Rust.
  remember(index, row) {
    const saved = this.prior[index];
    saved.index = index; saved.state = row.state; saved.health = row.health;
    saved.addHealth = row.addHealth; saved.attacker = row.attacker;
    saved.generation = row.generation; saved.team = row.team; saved.targetTeam = row.targetTeam; saved.gunner = row.gunner;
    saved.trapMode = row.trapMode;
    if (row.trapMode) {
      const source = row.trapState, state = saved.trapState;
      state.built = source.built; state.health = source.health; state.phase = source.phase;
      state.started = source.started; state.time = source.time; state.sprung = source.sprung;
      state.tick = source.tick; state.crushed = source.crushed;
    }
  }

  // LiveObjects.update remains foreign. Append just changed building facts
  // after registry facts in the unit phase's single retained input reservation.
  // No callback creates a list, record or temporary vector while writing it.
  foreign(at) {
    const p = this.phase, bridge = p.bridge, row = this.row;
    let count = 0;
    for (let i = 0; i < this.buildings.length; i++) {
      this.fill(i, row);
      const saved = this.prior[i];
      if (saved.index === i && saved.state === row.state && Object.is(saved.health,row.health) &&
          Object.is(saved.addHealth,row.addHealth) && saved.attacker === row.attacker &&
          Object.is(saved.generation,row.generation) && Object.is(saved.team,row.team) && Object.is(saved.targetTeam,row.targetTeam) && saved.gunner === row.gunner && (!row.trapMode || !sameTrState(saved.trapState,row.trapState))) continue;
      putF3BuildingState(bridge, row, bridge.f, at, p.heap);
      at += p.layout.buildingStateWords; count++;
      // A later native leaf updates this same comparison row immediately;
      // unchanged health then produces no redundant input on the next tick.
      this.remember(i, row);
    }
    return count;
  }

  // Building.show is a foreign visibility operation. Use the arrays prepared
  // at construction, rather than Object.entries allocating on every mutation.
  // ObjectivePost's old-state test must happen before publishing the new state.
  show(index, shown, old = this.buildings[index].state) {
    const b = this.buildings[index];
    const looks = this.looks[index], bodies = this.bodies[index];
    b.state = shown;this.shown[index]=shown;
    for (let k = 0; k < LOOKS.length; k++) {
      const on = LOOKS[k] === shown;
      if (looks[k]) sceneProperty(looks[k].root,"visible",on);
      if (bodies[k]) bodies[k].enabled = on;
    }
    if (b.hologram) {
      const on = shown === 'unbuilt';
      if (on && !b.hologram.on) b.hologram.fade = 0;
      b.hologram.on = on;
    }
    if (this.traps[index]) {
      // Trap.show keeps the logs visible and shows its separate pad only when
      // unbuilt. Its pad can have another placement from the log rig itself.
      if (looks[0]) sceneProperty(looks[0].root,"visible",true);
      if (b.pad) sceneProperty(b.pad.root,"visible",shown === 'unbuilt');
    }
    if (this.objectives[index]) {
      b.dying = old === 'built' && shown === 'destroyed' && b.span > 0 && looks[0] ? b.span : 0;
      if (b.dying) { sceneProperty(looks[0].root,"visible",true); b.time = 0; }
    }
  }

  effect(effect) {
    const row = effect.state, index = row.index;
    // Structural births and completed victims already precede notification
    // delivery. Intermediate show states still follow their recorded order.
    const old = this.shown[index];
    if(!this.effectMarked[index]){this.effectMarked[index]=1;this.effectTouched[this.effectCount++]=index;}
    const b = this.buildings[index];
    b.health = row.health; b.addHealth = row.addHealth;
    if (row.attacker !== undefined || Object.hasOwn(b,'lastAttacker')) b.lastAttacker = row.attacker;
    // Turret.built increments its target life after show, while death does
    // not. Publish the final native value before the next JS reader runs.
    if (b.aiTarget && row.generation !== null) b.aiTarget.generation = row.generation;
    if (row.trapMode) {
      const state = row.trapState;
      b.phase = state.phase; b.started = state.started; b.time = state.time;
      b.sprung = state.sprung; b.tick = state.tick; b.crushed = state.crushed;
    }
    if (effect.kind === 'show') this.show(index, row.state, old);
    else b.state = row.state;
    this.remember(index, row);
    // Game's headless callback changes foreign bookkeeping only. Native crew
    // death and explosions already occurred before this callback is reached.
    if (effect.kind === 'destroyed') b.onDestroyed(b);
  }

  // Building.show/onDestroyed completed before the queued combat observers
  // in the original owning call. Deliver those source-point records first,
  // then restore completed F4 clocks that an intermediate show could mask.
  drainEffects(){
    const p=this.phase,b=p.bridge;
    if(this.effectEpoch!==p.layout.eventEpoch){this.effectEpoch=p.layout.eventEpoch;this.effectOffset=0;}
    const end=b.f[p.layout.events/8];
    if(this.effectOffset>=end)return;
    this.effectOffset=b.drainFrom(p.layout.events,this.effectOffset,p.eventMirrors,this.effectHandlers,true);
    const live=p.liveOwner;
    for(let i=0;i<this.effectCount;i++){
      const index=this.effectTouched[i];this.effectMarked[index]=0;
      if(live){live.publishBuilding(live.buildings[index]);this.fill(index,this.row);this.remember(index,this.row);}
    }
    this.effectCount=0;
  }

  // Turret.onDamaged is pure journal delivery after the outer native return.
  // Its generated sample retains the health event's position and target life
  // even if another repair already rebuilt the same prop before publication.
  damaged(event) {
    const b = this.buildings[event.index], info = this.damageInfo;
    info.direction = event.hasDirection ? event.direction : null; info.amount = event.untyped;
    info.typed = event.typed; info.generic = event.generic;
    b.onDamaged(b,event.amount,event.attacker,info,event);
  }
}
