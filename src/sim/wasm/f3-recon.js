import * as THREE from 'three';
import { Arsenal } from '../../weapons.js';
import { valueWords } from './weapon-content-sim.js';
import { reconRandom } from '../../runtime/recon-random.js';
import { KIND_RECON_CONTEXT, KIND_RECON_DROID, F3ReconClassMirror, F3ReconLevelMirror,
  F3ReconStrikeMirror, F3ReconBindingMirror, F3ReconWeaponMirror, F3WeaponViewMirror,
  putF3ReconClass, putF3ReconLevel, putF3ReconStrike, putF3ReconBinding, putF3ReconWeapon } from '../gen/index.js';

const WEAPON_FIELDS = Object.keys(new F3WeaponViewMirror());
const DROID_FIELDS = ['yaw','pitch','health','dead','countdown'];

// The network recon read facade. Slot admission, launch order,
// controls, flight, weapons and detached strikes live in the native authority.
export class F3Recon {
  constructor(phase) {
    this.phase = phase;
    // The recon Map is a read view of native insertion order.
    // It also keeps retired owners visible while their strikes drain; it is
    // not an admission table or a second authority for droid lifetimes.
    this.contexts = new Map();
    // Network connection ids are opaque strings; native authority uses a
    // retained numeric cookie. Keep the original key for capture and seeds.
    this.keys = new Map(); this.names = [];
    this.slots = [];
    this.identities = new WeakMap();
    this.definitions = new Map();
    this.classes = [];
    this.classStrikes = [];
    this.strikes = new Map();
    this.strikeDefs = [];
    // Each construction record and its descriptor is retained. Content
    // preparation can grow these lists, while flight publications never
    // create a new argument object or rebuild a class table.
    this.classRow = new F3ReconClassMirror();
    this.levelRow = new F3ReconLevelMirror();
    this.strikeRow = new F3ReconStrikeMirror();
    this.binding = new F3ReconBindingMirror();
    this.weaponRow = new F3ReconWeaponMirror();
    const layout = phase.layout;
    this.classArgs = {words:layout.reconClassWords,put:putF3ReconClass};
    this.levelArgs = {words:layout.reconLevelWords,put:putF3ReconLevel};
    this.strikeArgs = {words:layout.reconStrikeWords,put:putF3ReconStrike};
    this.bindingArgs = {words:layout.reconBindingWords,put:putF3ReconBinding};
    this.weaponArgs = {words:layout.reconWeaponWords,put:putF3ReconWeapon};
    // This is a pure notification, sampled after F1 fires. A journal may
    // copy it, while a later droid snapshot cannot rename the shot's position.
    this.shot = {weapon:null};
  }

  // Game.createNetworkRemotes supplies its server's player bound before any
  // context attaches. Admission remains a native authority decision.
  configure(capacity) {
    this.phase.x.f3_recon_owners(capacity);
    this.phase.counters.wasmCalls++; this.phase.counters.lifecycleCalls++;
    this.phase.counters.inputFields++;
  }

  // RemoteDroids.createDroid resolves class and weapon content only once.
  // The independent stream is never added to the world's named RNG registry.
  prepare(weapon) {
    const definition = weapon.levels[0], def = definition.droid;
    if (!def) return;
    this.phase.owners.reserve();
    let id = this.definitions.get(def);
    if (id === undefined) {
      id = this.classes.length;
      this.definitions.set(def,id);this.classes.push(def);
      const strikes = this.classStrikes[id] = [];
      const row = this.classRow, phase = this.phase, b = phase.bridge;
      row.id = id;row.value.length = 0;row.loadout.length = 0;
      for (const gun of def.weapons || []) {
        const strikeLevels = []; strikes.push(strikeLevels);
        phase.combatSim.weaponClass(gun);row.loadout.push(gun);
        // Charge levels inherit the base definition, as Weapon's constructor
        // does. Their F1 and strike handles are prepared before any launch.
        const levels = [gun];
        for (const charge of gun.charges || []) levels.push({...gun,...charge});
        for (let index = 0; index < levels.length; index++) {
          const level = levels[index], input = this.levelRow;
          input.weapon = gun;input.level = index;input.ordnance = level.ordnance ?? null;
          if (input.ordnance) phase.factory.context.ordnance.sim.defId(input.ordnance);
          // Destruct levels carry salvo geometry beside a separate strike
          // ordnance definition. Registering only the latter loses the
          // countdown, scatter and detached salvo frame.
          input.strike = level.strike ? this.prepareStrike(level) : null;
          strikeLevels.push(input.strike);
          input.value.length = 0;valueWords(level,b.strings,input.value);
          b.callArgs(this.levelArgs,input);phase.x.f3_recon_level();
        }
      }
      valueWords(def,b.strings,row.value);
      row.explosion = def.explosion ? phase.factory.context.ordnance.sim.explosionId(def.explosion) : null;
      // RemoteDroids.shoot uses this measured offset even without a model.
      // Geometry presence must not make the worker fire from a different
      // muzzle than the page or dedicated server.
      const point = this.firePoints?.[def.GeometryName];
      if (point) row.muzzle.fromArray(point);else row.muzzle.set(0,0,0);
      b.callArgs(this.classArgs,row);phase.x.f3_recon_class();
    }
    const row = this.weaponRow;
    row.weapon = definition;row.class = id;row.immediate = Boolean(definition.SwitchImmediately);
    this.phase.bridge.callArgs(this.weaponArgs,row);this.phase.x.f3_recon_weapon();
  }
  // RemoteDroids.strike reads salvo fields from the destruct weapon level.
  // Several levels may share F1 ordnance but keep different scatter or clocks.
  prepareStrike(definition) {
    let id = this.strikes.get(definition);
    if (id !== undefined) return id;
    id = this.strikeDefs.length;this.strikes.set(definition,id);this.strikeDefs.push(definition);
    const row = this.strikeRow, phase = this.phase;
    row.id = id;row.ordnance = definition.strike;row.value.length = 0;
    phase.factory.context.ordnance.sim.defId(row.ordnance);
    valueWords(definition,phase.bridge.strings,row.value);
    phase.bridge.callArgs(this.strikeArgs,row);phase.x.f3_recon_strike();
    return id;
  }

  // Recon attachment and rebind are explicit owner lifecycle commands.
  // They can allocate a new facade, but never import a soldier or droid body.
  // Admission can name a connection before its first body exists. No RNG
  // stream or native context is created until canonical realization.
  admitKey(key) {
    let value=this.keys.get(key);
    if(value===undefined) {value=this.names.length;this.keys.set(key,value);this.names.push(key);}
    return value;
  }
  attach(key, owner) { return this.bind('attach',key,owner); }
  rebind(key, owner) { return this.bind('rebind',key,owner); }
  bind(action, key, owner) {
    // MultiplayerWorld assigns its Character before binding the context.
    // Publish that pending metadata before native recon can publish the body.
    this.phase.publishMetadata(owner);
    let nativeKey = this.keys.get(key);
    if (nativeKey === undefined) {
      nativeKey = this.names.length; this.keys.set(key,nativeKey); this.names.push(key);
    }
    const row = this.binding;
    const keyRow = this.phase.bindings[owner.id].view.key;
    row.action = action;row.key = nativeKey;
    row.owner.slot = keyRow.slot;row.owner.generation = keyRow.generation;row.owner.life = keyRow.life;
    row.seed = this.randomFor(key).seed;
    row.droid = null;row.dx = row.dy = 0;row.sensitivity = 1;row.flag = false;
    this.command();
    const context = this.contexts.get(key).context;
    owner.combat.remotes = context;
    return context;
  }
  exit(key) { const entry = this.contexts.get(key);if (entry) entry.context.exit(); }
  retire(key) {
    const entry = this.contexts.get(key);if (!entry) return;
    this.forContext(entry.context,'retire');this.command();
  }
  // Commands name the context's sampled owner life. Clearing optional
  // droid/vector fields here prevents a prior kill or look from leaking a
  // reference into an unrelated lifecycle operation.
  forContext(context, action) {
    const row = this.binding;
    row.action = action;row.key = context.nativeKey;
    row.owner.slot = context.owner.id;row.owner.generation = context.ownerKey.generation;row.owner.life = context.ownerKey.life;
    row.droid = null;row.dx = row.dy = 0;row.sensitivity = 1;row.flag = false;
    return row;
  }
  command() {
    const phase = this.phase;
    if (phase.active) throw Error('recon command inside the owned phase');
    // A droid death explosion can reach Unit damage and the shared gameplay
    // RNG. Exchange those states once around the outer command, while native
    // recursive services finish before any event callback runs.
    phase.random.push();phase.bodiesIn();
    phase.bridge.callArgs(this.bindingArgs,this.binding);
    phase.counters.wasmCalls++;phase.counters.commandCalls++;
    phase.counters.inputFields += this.bindingArgs.words;
    // Character leaves can publish units during recursive damage, while
    // recon/ordnance notices still wait for this outer command to return.
    const previous = phase.bridge.phaseCounters;
    phase.bridge.phaseCounters = phase.counters;phase.active = true;
    try { if (!phase.x.f3_recon_command()) throw Error('stale recon command'); }
    finally { phase.active = false;phase.bridge.phaseCounters = previous; }
    phase.random.pull();phase.sync();
  }
  // EntityRemoteCapture reads an admitted network identity. It never
  // allocates one by observing a droid, and an object displaced by a later
  // generation cannot accidentally acquire the replacement's identity.
  identityFor(droid) {
    const identity = this.identities.get(droid);
    if (!identity || this.slots[identity.id].droid !== droid) throw Error('stale recon identity');
    return identity;
  }
  // Preserve the capture reader's context/launch traversal. Gameplay
  // acquisition uses Rust's live lists directly; this iterator runs only
  // after publication for existing foreign capture consumers.
  *droids() {
    for (const entry of this.contexts.values()) for (const droid of entry.context.droids) yield {context:entry.context,droid};
  }

  // Native attach publishes this before a state row or a droid launch can
  // refer to the context. Copy the life key, since the generated event
  // decoder reuses its argument record for the next creation.
  contextCreated(event) {
    const owner = this.phase.bindings[event.owner.slot].unit;
    const key = this.names[event.key];
    const context = new Context(this,key,event.key,owner,event.owner);
    const entry = {owner,context,lastTick:-1,retired:false};
    this.contexts.set(key,entry);
    this.phase.bridge.refs[KIND_RECON_CONTEXT].bind(event.slot,event.generation,context);
  }
  droidCreated(event) {
    const def = this.classes[event.class], owner = this.phase.bindings[event.owner.slot].unit;
    // RemoteDroids.createDroid gives every network generation a new object.
    // Retired terminal objects and launch attribution therefore stay immutable.
    const droid = {
      def,
      simSlot:event.slot,
      simGen:event.generation,
      owner:event.launchOwner,
      weapon:owner.combat.arsenal.weapons[event.weapon],
      team:event.team,
      mesh:null,
      position:new THREE.Vector3(),
      velocity:new THREE.Vector3(),
      yaw:0,
      pitch:0,
      health:0,
      dead:false,
      arsenal:new Arsenal((def.weapons || []).map(gun=>({...gun,ammo:gun.ammo}))),
      countdown:-1,
      salvo:null,
      eye:new THREE.Vector3(),
      aimDir:new THREE.Vector3(0,0,1),
      target:new THREE.Vector3(),
      context:event.context,
    };
    droid.salvoStorage = {
      def:null,
      at:new THREE.Vector3(),
      x:new THREE.Vector3(),
      y:new THREE.Vector3(),
      z:new THREE.Vector3(),
      left:0,
      wait:0,
    };
    // Remote correction identifies a salvo by its actual arsenal level.
    // Arsenal clones loadouts, so the content loader's source definition is
    // not that identity, even when every authored field is equal.
    droid.strikeLevels = new Array(this.strikeDefs.length);
    const strikes = this.classStrikes[event.class];
    for (let weapon = 0; weapon < strikes.length; weapon++) {
      const levels = strikes[weapon];
      for (let level = 0; level < levels.length; level++) {
        const strike = levels[level];
        if (strike !== null) droid.strikeLevels[strike] = droid.arsenal.weapons[weapon].levels[level];
      }
    }
    const identity = Object.freeze({id:event.slot,generation:event.generation});
    this.identities.set(droid,identity);
    this.slots[event.slot] = {droid,generation:event.generation,reservation:null};
    this.phase.bridge.refs[KIND_RECON_DROID].bind(event.slot,event.generation,droid);
  }
  // A context survives a soldier respawn and keeps its old flying droids.
  // Copy owner keys instead of borrowing the event decoder's scratch record.
  contextState(event) {
    const context = event.context, entry = this.contexts.get(context.key);
    context.owner = this.phase.bindings[event.owner.slot].unit;entry.owner = context.owner;
    context.ownerKey.slot = event.owner.slot;context.ownerKey.generation = event.owner.generation;context.ownerKey.life = event.owner.life;
    entry.lastTick = event.lastTick;entry.retired = event.retired;
    // Native player realization can rebind without a JavaScript command.
    // This is the retained Context facade, not a second owning controller.
    context.owner.combat.kept.remotes = context;
    context.random.seed = event.seed;
    context.firstPerson = event.firstPerson;context.piloted = event.piloted;
    context.authorityActive = Boolean(event.piloted);context.zoomStep = Number(event.zoom);
    context.lookX = event.lookX;context.lookY = event.lookY;
    // The simulation camera is state used by remote correction capture.
    // Publishing its native pose avoids running chase rays or three.js
    // camera interpolation again in the authoritative JavaScript world.
    context.aimCamera.position.copy(event.camera.position);context.aimCamera.quaternion.copy(event.camera.rotation);
    context.viewState.reach = event.camera.reach;context.viewState.fresh = event.camera.fresh;context.viewState.fov = event.camera.fov;
    context.simulationView = event.piloted ? context.viewState : null;
    context.droids.length = event.droids.length;
    for (let i = 0; i < event.droids.length; i++) context.droids[i] = event.droids[i];
    context.strikes.length = event.strikes.length;
    for (let i = 0; i < event.strikes.length; i++) context.strikes[i] = event.strikes[i];
    this.releaseRetained(context);
  }
  // RemoteDroids' dynamic fields keep stable vectors and weapon objects.
  // A salvo's definition is its destruct level, not its F1 strike ordnance.
  droidState(event) {
    const droid = event.droid;
    for (let i = 0; i < DROID_FIELDS.length; i++) { const name = DROID_FIELDS[i];droid[name] = event[name]; }
    droid.position.copy(event.position);droid.velocity.copy(event.velocity);
    droid.eye.copy(event.eye);droid.aimDir.copy(event.aim);droid.target.copy(event.target);
    droid.salvo = event.salvo ? droid.salvoStorage : null;
    if (event.salvo) {
      const source = event.salvo, target = droid.salvo;
      target.def = droid.strikeLevels[source.strike];target.left = source.left;target.wait = source.wait;
      target.at.copy(source.at);target.x.copy(source.x);target.y.copy(source.y);target.z.copy(source.z);
    }
    // Network weapon capture expects the ordinary Arsenal and Weapon
    // objects. Selection, charge and timers are mirrors of native results;
    // immutable levels and channel membership retain their construction data.
    droid.arsenal.selected[0] = event.selected[0];droid.arsenal.selected[1] = event.selected[1];
    for (let index = 0; index < event.weapons.length; index++) {
      const source = event.weapons[index], weapon = droid.arsenal.weapons[index];
      for (let i = 0; i < WEAPON_FIELDS.length; i++) {
        const name = WEAPON_FIELDS[i];if (name !== 'melee') weapon[name] = source[name];
      }
      weapon.def = weapon.levels[source.level];
      if (source.melee) {
        weapon.melee.time = source.melee.time;weapon.melee.index = source.melee.index;weapon.melee.active = source.melee.active;
      }
    }
  }
  // Release only the exact terminal generation. Its object remains useful
  // to delayed capture consumers while the network slot admits a fresh one.
  retired(event) {
    const refs = this.phase.bridge.refs[event.context ? KIND_RECON_CONTEXT : KIND_RECON_DROID];
    const object = refs.at(event.slot,event.generation);
    if (event.context) {
      this.contexts.delete(object.key);
      object.droids.length = object.strikes.length = 0;
      object.piloted = object.simulationView = null;object.authorityActive = false;
      this.releaseRetained(object);
      refs.release(object);
    } else {
      // An earlier owner may already have stepped before this death. Keep
      // that context's terminal reference until its next native list update.
      const context = object.context;
      if (context.droids.indexOf(object) >= 0 || context.strikes.indexOf(object) >= 0) {
        refs.retain(event.slot,event.generation);context.retained.push(object);
      } else refs.release(object);
    }
  }
  releaseRetained(context) {
    const list = context.retained, refs = this.phase.bridge.refs[KIND_RECON_DROID];
    let count = 0;
    for (let i = 0; i < list.length; i++) {
      const droid = list[i];
      if (context.droids.indexOf(droid) >= 0 || context.strikes.indexOf(droid) >= 0) list[count++] = droid;
      else refs.forget(droid.simSlot,droid.simGen);
    }
    list.length = count;
  }
  // Game's remoteShot listener observes the droid class and weapon channel.
  // Both are immutable for this generation, while its position is the
  // sampled event vector rather than the droid's final frame position.
  fired(event) {
    const droid = event.droid;
    this.shot.weapon = droid.arsenal.weapons[event.weapon];
    this.onFired?.(this.shot,event.position,droid);
  }
}

// A Context contains ordinary mirrors and explicit commands only. Camera and
// arsenal readers keep their original shapes for network corrections and HUD.
class Context {
  constructor(world, key, nativeKey, owner, ownerKey) {
    this.world = world;this.key = key;this.nativeKey = nativeKey;this.owner = owner;
    this.ownerKey = {slot:ownerKey.slot,generation:ownerKey.generation,life:ownerKey.life};
    // This function exposes the same seed property as reconRandom. Native
    // publications set its state; authoritative flight never calls it from
    // JavaScript or inserts it into the battle's shared stream registry.
    this.random = reconRandom(0,0);
    this.droids = [];this.strikes = [];this.retained = [];this.piloted = null;this.view = null;
    this.lookX = this.lookY = this.zoomStep = 0;this.firstPerson = false;this.authorityActive = false;
    this.aimCamera = new THREE.PerspectiveCamera();
    this.viewState = {reach:1,fresh:true,fov:0};this.simulationView = null;
  }
  get active() { return this.authorityActive; }
  get zoom() { return this.zoomStep && this.piloted ? this.piloted.arsenal.current(0)?.def.ZoomMin || 1 : 1; }
  // RemoteDroids.of compares weapon identity within an arsenal life. A
  // respawn replaces that arsenal, so an old flying droid is not silently
  // adopted by the new life's remote weapon on the same pooled Unit.
  of(weapon) {
    for (let i = 0; i < this.droids.length; i++) {
      const droid = this.droids[i];if (droid.weapon === weapon && !droid.dead) return droid;
    }
    return null;
  }
  identityFor(droid) { return this.world.identityFor(droid); }
  look(dx, dy, sensitivity, invert) {
    const row = this.world.forContext(this,'look');row.dx = dx;row.dy = dy;row.sensitivity = sensitivity;row.flag = invert;
    this.world.command();
  }
  exit() { this.world.forContext(this,'exit');this.world.command(); }
  enter(droid) { if (!droid || droid.dead) return;this.world.forContext(this,'enter').droid = droid;this.world.command(); }
  kill(droid) { if (droid.dead) return;this.world.forContext(this,'kill').droid = droid;this.world.command(); }
}
