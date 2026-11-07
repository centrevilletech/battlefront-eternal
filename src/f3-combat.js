import {F3CombatField_VALUE as C,F3WeaponField_VALUE as W,F3CombatViewLayout as V,F3WeaponViewLayout as WV} from './sim/gen/combat.js';
import {BlockHeaderLayout} from './sim/gen/boundary.js';
// Immutable combat content and ordinary mirrors for the native unit owner.
// Construction prepares classes and initial combat once; only the owning
// command boundary changes an adopted body's state.
import { valueWords, weaponClasses, registerWeaponClass } from './weapon-content-sim.js';
import { bridgeFor } from './sim/runtime.js';
import { CombatMirror, KIND_UNIT, syncCombat } from './sim/gen/index.js';
import { PlayerCombat } from './combat.js';
import { HEALTH_SCALES } from './ordnance.js';
import { ordnanceTests } from './ordnance-sim.js';

const HEADER = BlockHeaderLayout.words;
export function attachF3Combat(instance, classes) {
  return new F3CombatStore(bridgeFor(instance,{classes,tests:ordnanceTests}));
}
export class F3CombatStore {
  constructor(bridge) {
    this.bridge = bridge;
    this.x = bridge.x;
    this.facades = [];
    this.weaponsSent = weaponClasses(bridge);
    this.classesSent = new Set();
    this.explosions = new Map();
  }
  make(options) { return new CombatFacade(this,options); }

  // PlayerCombat's pre-adoption settings are construction data. Afterwards
  // every scalar passes through its body's qualified command key.
  setCombat(combat,field,value) {
    const phase = combat.owner.f3;
    if (phase) return phase.scalar(combat.owner.id,'combatField',field,0,value);
    const accepted = this.x.cmb_set(combat.owner.id,field,value);
    if (!accepted) throw Error('invalid native combat setting');
    return accepted;
  }
  // The content a combat reads, given to the module the first time.
  send(words, call) {
    const b = this.bridge, ptr = this.x.sim_call_room(words.length, 0);
    b.views();
    b.f.set(words, ptr / 8 + HEADER);
    call();
  }

  // Weapon construction interns the original immutable class object.
  // Charge levels share the same content table as mounted and recon weapons.
  // Registration can happen while preparing a future inactive body.
  // No live weapon getter sends that content again.
  weaponClass(def) {
    return registerWeaponClass(this.bridge, def);
  }

  // PlayerCombat.reset needs the class regeneration rate and loadout.
  // Register every weapon before the class record can refer to its handle.
  // The weak definition identity is construction-time bookkeeping only.
  // Ordinary updates use the native prepared class and level records.
  unitClass(def) {
    const id = this.bridge.defs.unitClass.id(def);
    if (this.classesSent.has(def)) return id;
    for (const weapon of def.weapons || []) this.weaponClass(weapon);
    this.classesSent.add(def);
    const view = { AddHealth: def.AddHealth, base: def.base, weapons: def.weapons || [] };
    this.send(valueWords(view, this.bridge.strings, [], this.weaponsSent), () => this.x.cmb_class(id));
    return id;
  }

  // PlayerCombat.explode prepares each shared explosion only once.
  // This table translates class identity into the native blast-policy handle.
  // F1's later definition binding joins that handle to the same damage solver.
  // A live blast does not inspect the original class object per victim.
  explosion(def) {
    let id = this.explosions.get(def);
    if (id !== undefined) return id;
    id = this.explosions.size;
    this.explosions.set(def, id);
    this.send(valueWords(def, this.bridge.strings, []), () => this.x.cmb_explosion(id));
    return id;
  }

}

// Weapon mirrors retain scalar readiness and ammunition for foreign AI.
// Completed-body adoption binds their qualified command methods. These
// getters never enter the module or recompute a weapon's live state.
class WeaponFacade {
  constructor(arsenal, index, def) {
    this.arsenal = arsenal;
    this.combat = arsenal.combat;
    this.bridge = arsenal.combat.sim.bridge;
    this.index = index;
    // The class at each charge level, as Weapon keeps them.
    this.levels = [def, ...(def.charges || []).map((level) => ({ ...def, ...level }))];
    // Weapon.def is immutable for a single-level class. Stats and AI read
    // it often; only a charged class needs the canonical native level lookup.
    this.fixedDef = this.levels.length === 1 ? this.levels[0] : null;
    this.name = def.class;
    this.channel = def.channel;
    this.bank = def.AnimationBank || "rifle";
    this.rounds = def.RoundsPerClip;
    // What the battle sets on a weapon (the AI's settings, its generator):
    // its generator and spread settings are written into the store as they
    // are set (the units' phase reads them there); its sniper scale only
    // the AI reads.
    this.kept = { random: null, spreadScale: 1, yawSpreadOverride: null };
    this.sniperScale = 0;
    this.shownAtWork = 0;
    this.f3RandomDraw = null;
    this.mirrorVersion = -1;
    this.meleeView = def.base === "melee" ? new MeleeView(this) : null;
  }

  // Its mirror (the store's weapon record).
  // WeaponFacade exposes readiness from the consumed native publication.
  // Its parent arsenal can be replaced by reset while this object is retained.
  // The setters below reject an old arsenal before issuing a command.
  // Read-only old weapons retain their last life for presentation observers.
  get w() {
    // AI normally reads only its selected weapons. Decode an observed row
    // once from the retained native publication; this issues no module call.
    const arsenal = this.arsenal, binding = arsenal.binding;
    if (binding && this.mirrorVersion !== binding.weaponVersion) arsenal.phase.readWeapon(this);
    return this.combat.state.arsenal.weapons[this.index];
  }
  // Weapon.def and readiness readers usually need one scalar. Decoding a
  // complete weapon just to obtain its level also made BattleStats.observe
  // refresh every active weapon each tick. Borrow the generated cache row;
  // explicit w observers still get their complete stable mirror above.
  row() {
    // The owning publication resolves this heap base once for the arsenal.
    // Construction and replaced lives retain their ordinary cold mirrors;
    // active getters need only an indexed scalar, not descriptor traversal.
    const base = this.arsenal.nativeWeaponsAt;
    return base < 0 ? -1 : base + this.index * WV.words;
  }
  get random() { return this.kept.random; }
  set random(value) {
    if (this.kept.random === value) return;
    const had = Boolean(this.kept.random); this.kept.random = value;
    if (had !== Boolean(value)) this.keep(W.random, value ? 1 : 0);
    // The callback's identity matters even when its presence stays true.
    // Publish that one stream now instead of polling every arsenal each tick.
    const arsenal = this.arsenal;
    if (this.combat.made && this.combat.arsenal === arsenal && arsenal.phase) {
      arsenal.phase.random.weapon(arsenal.binding, this.index, this);
    }
  }
  get spreadScale() { return this.kept.spreadScale; }
  set spreadScale(value) { if (Object.is(this.kept.spreadScale, value)) return; this.kept.spreadScale = value; this.keep(W.spreadScale, value); }
  get yawSpreadOverride() { return this.kept.yawSpreadOverride; }
  set yawSpreadOverride(value) {
    if (Object.is(this.kept.yawSpreadOverride, value)) return;
    this.kept.yawSpreadOverride = value;
    this.keep(value === null ? W.clearYawSpread : W.yawSpreadOverride, value === null ? 0 : value);
  }
  // Weapon.thrown and repair are resolved with the native weapon level.
  // The mirror carries those flags alongside its selected charge level.
  // Do not repeat class-name tests in every AI or capture getter.
  // That also keeps future content policy changes on the owning side.
  get thrown() {
    const row = this.row();
    return row < 0 ? this.w.thrown : this.bridge.f[row + WV.thrown] !== 0;
  }
  get repair() {
    const row = this.row();
    return row < 0 ? this.w.repair : this.bridge.f[row + WV.repair] !== 0;
  }
  get def() { return this.fixedDef ?? this.levels[this.level]; }
  get level() {
    const row = this.row();
    return row < 0 ? this.w.level : this.bridge.f[row + WV.level];
  }
  get deployed() {
    const row = this.row();
    return row < 0 ? this.w.deployed : this.bridge.f[row + WV.deployed] !== 0;
  }
  get firing() {
    const row = this.row();
    return row < 0 ? this.w.firing : this.bridge.f[row + WV.firing] !== 0;
  }
  get timer() {
    const row = this.row();
    return row < 0 ? this.w.timer : this.bridge.f[row + WV.timer];
  }
  get bottomless() {
    const row = this.row();
    return row < 0 ? this.w.bottomless : this.bridge.f[row + WV.bottomless] !== 0;
  }
  get perShot() {
    const row = this.row();
    return row < 0 ? this.w.perShot : this.bridge.f[row + WV.perShot];
  }
  get clip() {
    const row = this.row();
    return row < 0 ? this.w.clip : this.bridge.f[row + WV.clip];
  }
  set clip(value) { this.set(W.clip, "clip", value); }
  get reserve() {
    const row = this.row();
    return row < 0 ? this.w.reserve : this.bridge.f[row + WV.reserve];
  }
  set reserve(value) { this.set(W.reserve, "reserve", value); }
  get heat() {
    const row = this.row();
    return row < 0 ? this.w.heat : this.bridge.f[row + WV.heat];
  }
  set heat(value) { this.set(W.heat, "heat", value); }
  get overheated() {
    const row = this.row();
    return row < 0 ? this.w.overheated : this.bridge.f[row + WV.overheated] !== 0;
  }
  get floor() {
    const row = this.row();
    return row < 0 ? this.w.floor : this.bridge.f[row + WV.floor];
  }
  set floor(value) { this.set(W.floor, "floor", value); }
  get kick() {
    const row = this.row();
    return row < 0 ? this.w.kick : this.bridge.f[row + WV.kick];
  }
  set kick(value) { this.set(W.kick, "kick", value); }
  get wait() {
    const row = this.row();
    return row < 0 ? this.w.wait : this.bridge.f[row + WV.wait];
  }
  get reloadLeft() {
    const row = this.row();
    return row < 0 ? this.w.reloadLeft : this.bridge.f[row + WV.reloadLeft];
  }
  get throwTime() {
    const row = this.row();
    return row < 0 ? this.w.throwTime : this.bridge.f[row + WV.throwTime];
  }
  get released() {
    const row = this.row();
    return row < 0 ? this.w.released : this.bridge.f[row + WV.released] !== 0;
  }
  get hidden() {
    const row = this.row();
    return row < 0 ? this.w.hidden : this.bridge.f[row + WV.hidden] !== 0;
  }
  get charging() {
    const row = this.row();
    return row < 0 ? this.w.charging : this.bridge.f[row + WV.charging] !== 0;
  }
  get held() {
    const row = this.row();
    return row < 0 ? this.w.held : this.bridge.f[row + WV.held];
  }
  get charge() {
    const row = this.row();
    return row < 0 ? this.w.charge : this.bridge.f[row + WV.charge];
  }
  get limit() {
    const row = this.row();
    return row < 0 ? this.w.limit : this.bridge.f[row + WV.limit];
  }
  get pattern() {
    const row = this.row();
    return row < 0 ? this.w.pattern : this.bridge.f[row + WV.pattern];
  }
  get salvosLeft() {
    const row = this.row();
    return row < 0 ? this.w.salvosLeft : this.bridge.f[row + WV.salvosLeft];
  }
  get salvoWait() {
    const row = this.row();
    return row < 0 ? this.w.salvoWait : this.bridge.f[row + WV.salvoWait];
  }
  get salvoSeed() {
    const row = this.row();
    if (row < 0) return this.w.salvoSeed;
    const f = this.bridge.f, at = row + WV.salvoSeed;
    return f[at] === 0 ? f[at + 1] : null;
  }
  get salvoCount() {
    const row = this.row();
    return row < 0 ? this.w.salvoCount : this.bridge.f[row + WV.salvoCount];
  }
  get lockTargetId() {
    const row = this.row();
    return row < 0 ? this.w.lockTargetId : this.bridge.f[row + WV.lockTargetId];
  }
  get lockGeneration() {
    const row = this.row();
    return row < 0 ? this.w.lockGeneration : this.bridge.f[row + WV.lockGeneration];
  }
  get lockElapsed() {
    const row = this.row();
    return row < 0 ? this.w.lockElapsed : this.bridge.f[row + WV.lockElapsed];
  }
  get lockComplete() {
    const row = this.row();
    return row < 0 ? this.w.lockComplete : this.bridge.f[row + WV.lockComplete] !== 0;
  }
  get melee() { return this.w.melee ? this.meleeView : null; }
  get hasRandom() { return Boolean(this.random); }
  get admits() {
    const row = this.row();
    return row < 0 ? this.w.admits : this.bridge.f[row + WV.admitShot] !== 0;
  }
  get handEmpty() { return this.hidden; }
  get reloading() { return this.reloadLeft > 0; }
  get busy() { return this.throwTime >= 0 || this.firing || this.salvosLeft > 0; }
  get loaded() { return this.bottomless ? Infinity : Math.round(this.clip * this.rounds); }
  get spare() { return this.reserve === Infinity ? Infinity : Math.round(this.reserve * Math.max(this.rounds, 1)); }
  get canShoot() { return this.bottomless || this.clip >= this.perShot - 1e-6; }
  get empty() { return !this.canShoot && this.clip + this.reserve < this.perShot - 1e-6; }

  // Recharge and spread edits are qualified commands after adoption. The
  // passive viewpoint also sets initial policy before its one body import;
  // those construction writes touch only the prepared native combat row.
  // Weapon scalar edits are synchronous, so the caller can read their result.
  // After adoption, the qualified key checks slot generation and gameplay life.
  // Before adoption, settings initialize the already-prepared native combat row.
  // Both paths mutate native state and neither imports a whole Unit.
  set(field,name,value) {
    const c = this.combat, arsenal = this.arsenal;
    if (c.arsenal !== arsenal) return;
    if (arsenal.phase) {
      const p = arsenal.phase;
      p.begin(arsenal.binding,'weaponField',arsenal.key);
      p.change.field = field; p.change.index = this.index; p.change.value = value;
      p.commit();
    } else if (!c.sim.x.cmb_weapon_set(c.owner.id,this.index,field,value)) throw Error('invalid native weapon setting');
    this.w[name] = value;
  }
  keep(field,value) {
    const c = this.combat, arsenal = this.arsenal;
    if (!c.made || c.arsenal !== arsenal) return;
    if (arsenal.phase) {
      const p = arsenal.phase;
      p.begin(arsenal.binding,'weaponField',arsenal.key);
      p.change.field = field; p.change.index = this.index; p.change.value = value;
      p.commit();
    } else if (!c.sim.x.cmb_weapon_set(c.owner.id,this.index,field,value)) throw Error('invalid native weapon setting');
  }
}

// MeleeSwing's sampled state, read without advancing or acquiring a target.
class MeleeView {
  constructor(weapon) { this.weapon = weapon; }
  get state() { return this.weapon.w.melee; }
  get time() { return this.state.time; }
  get index() { return this.state.index; }
  get active() { return this.state.active; }
  captureState() { const s = this.state; return { time: s.time, index: s.index, active: s.active }; }
}

// PlayerCombat.reset replaces the arsenal once per native reset.
class ArsenalFacade {
  constructor(combat, defs) {
    this.combat = combat;
    this.phase = null;
    this.binding = null;
    this.nativeWeaponsAt = -1;
    this.key = {slot:0,generation:0,life:0};
    this.weapons = defs.map((def, i) => new WeaponFacade(this, i, def));
    this.channels = [0, 1].map((c) => this.weapons.filter((w) => w.channel === c));
  }

  // ArsenalFacade uses channel-relative indices from the canonical arsenal.
  // Its cold channel arrays preserve the original weapon object identities.
  // Selection commands retain busy refusal and already-selected early returns.
  // A mirror read never advances a weapon timer to discover its selection.
  get selected() { return this.combat.state.arsenal.selected; }

  current(channel) {
    return this.channels[channel][this.selected[channel]] || null;
  }

  // Reconnect advances this arsenal's key. Reset replaces the arsenal and
  // leaves its old key and weapons detached from the current life.
  bindOwner(phase,binding) {
    this.phase = phase; this.binding = binding;
    this.key.slot = binding.view.key.slot;
    this.key.generation = binding.view.key.generation;
    this.key.life = binding.life;
    binding.arsenalKey = this.key;
  }
  // Arsenal.select keeps the already-selected early return and busy refusal.
  select(weapon) {
    if (this.combat.arsenal !== this || !weapon || this.weapons[weapon.index] !== weapon) return false;
    if (this.current(weapon.channel) === weapon) return true;
    const p = this.phase;
    p.begin(this.binding,'select',this.key); p.change.index = weapon.index; p.commit();
    return p.layout.accepted;
  }
  next(channel,direction = 1) {
    if (this.combat.arsenal !== this) return false;
    const p = this.phase;
    p.begin(this.binding,'next',this.key);
    p.change.index = channel; p.change.value = direction; p.commit();
    return p.layout.accepted;
  }
}

// A stable facade over the combat owned by its Unit row.
export class CombatFacade {
  // PlayerCombat's world facade keeps only foreign policy and read mirrors.
  // The body's Unit row owns health, aim, arsenal and death state in Rust.
  // Drawing references may be absent in a dedicated server or synthetic fixture.
  // They do not choose another implementation of the authoritative phase.
  constructor(sim, {
    soldier, def, actor, animator, collision, camera = null,
    ordnance, lights, owner, player = false,
  }) {
    this.sim = sim;
    this.owner = owner;
    this.phase = null;
    this.aimOverride = null;
    this.onDamaged = () => {};
    this.soldier = soldier;
    this.def = def;
    this.actor = actor;
    this.animator = animator;
    this.collision = collision;
    this.camera = camera;
    this.ordnance = ordnance;
    this.lights = lights;
    this.hud = null;
    this.onRespawn = () => {};
    this.items = null;
    this.live = null;
    // What the battle's JavaScript keeps on a combat that the module reads
    // (the units' phase reads them from the store): written into the store
    // as they are set, once the combat has its record.
    this.kept = {
      mounted: false, gunUnderwater: false, team: 0, player,
      deferMuzzles: false, random: undefined, remotes: null,
    };
    this.glow = null;
    this.effects = null;
    this.firstPerson = false;
    this.view = null;
    this.shown = null;
    this.droideka = def.base === "walkerdroid";
    // PlayerCombat's truthy default is intentional, including a zero class
    // health falling back to 300. Hero health stays a full numeric value.
    this.maxHealthOf = def.MaxHealth || 300;
    this.chunksOf = def.chunks?.length && (def.ChunkFrequency ?? -1) > 0 ? def.ChunkFrequency : 0;
    this.scaleOf = HEALTH_SCALES[(def.HealthType || "person").toLowerCase()] || "PersonScale";
    // The store's record (synced after every command) and the arsenal's
    // facade (new each life).
    this.state = new CombatMirror();
    this.arsenal = null;
    this.made = false;
    // The hurts under way (damage and explode, innermost last): what
    // combat's hooks hand the battle of the caller's own objects.
    this.hurts = [];
  }

  // Its record in the store, made once the unit has its id (the factory
  // calls this when the registry has numbered it).
  // PlayerCombat construction follows registry numbering, before adoption.
  // The native row must exist before cold setters can publish their policy.
  // Adoption later binds the completed body's command key and stable vectors.
  // This is deliberately separate from UnitFactory acquire/release pooling.
  attach() {
    if (this.made) return;
    const sim = this.sim, id = this.owner.id;
    sim.facades[id] = this;
    // (The store names him by his id: the bridge knows his object by it.)
    sim.bridge.refs[KIND_UNIT].slotOf(this.owner);
    const cls = sim.unitClass(this.def);
    this.arsenal = new ArsenalFacade(this, this.def.weapons || []);
    this.made = true;
    this.readConstruction(sim.x.cmb_new(id, cls, this.player ? 1 : 0, this.maxHealthOf, this.chunksOf, sim.bridge.strings.id(this.scaleOf)));
    // What the battle set on the combat before it had its record.
    const k = this.kept;
    for (const [field, value] of [
      [C.mounted,k.mounted], [C.gunUnderwater,k.gunUnderwater], [C.team,k.team], [C.deferMuzzles,k.deferMuzzles],
      [C.random,k.random], [C.remotes,k.remotes],
    ]) sim.setCombat(this,field,Number(field >= C.random ? Boolean(value) : value));
    this.state.mounted = k.mounted; this.state.gunUnderwater = k.gunUnderwater; this.state.team = k.team; this.state.deferMuzzles = k.deferMuzzles;
    this.state.hasRandom = Boolean(k.random); this.state.hasRemotes = Boolean(k.remotes);
  }

  // Foreign settings use scalar commands after the body is adopted.
  get mounted() { return this.kept.mounted; }
  set mounted(value) { this.keep(C.mounted, "mounted", value, Boolean(value)); }
  get gunUnderwater() { return this.kept.gunUnderwater; }
  set gunUnderwater(value) { this.keep(C.gunUnderwater, "gunUnderwater", value, Boolean(value)); }
  get team() { return this.kept.team; }
  set team(value) { this.keep(C.team, "team", value, value); }
  get player() { return this.kept.player; }
  set player(value) { this.keep(C.player, "player", value, Boolean(value)); }
  get deferMuzzles() { return this.kept.deferMuzzles; }
  set deferMuzzles(value) { this.keep(C.deferMuzzles, "deferMuzzles", value, Boolean(value)); }
  get random() { return this.kept.random; }
  set random(value) { this.kept.random = value; if (this.made) { this.sim.setCombat(this, C.random, value ? 1 : 0); this.state.hasRandom = Boolean(value); } }
  get remotes() { return this.kept.remotes; }
  set remotes(value) { this.kept.remotes = value; if (this.made) { this.sim.setCombat(this, C.remotes, value ? 1 : 0); this.state.hasRemotes = Boolean(value); } }

  // A field the battle keeps here, into the store and the mirror at once
  // (`stored`: as the store's record has it).
  keep(field, name, value, stored) {
    if (Object.is(this.kept[name], value)) return;
    this.kept[name] = value;
    if (!this.made) return;
    this.sim.setCombat(this, field, Number(stored));
    this.state[name] = stored;
  }

  // ---- the mirror ----
  get maxHealth() { return this.state.maxHealth; }
  get health() { return this.state.health; }
  set health(value) { this.set(C.health, "health", value); }
  get dead() { return this.state.dead; }
  set dead(value) { this.set(C.dead, "dead", Boolean(value)); }
  get deathTime() { return this.state.deathTime; }
  set deathTime(value) { this.set(C.deathTime, "deathTime", value); }
  get lastAttacker() { return this.state.lastAttacker; }
  get repairTarget() { return this.state.repairTarget; }
  get repairLock() { return this.state.repairLock; }
  get blowApart() { return this.state.blowApart; }
  set blowApart(value) { this.set(C.blowApart, "blowApart", Boolean(value)); }
  get chunks() { return this.state.chunks; }
  get zoomStep() { return this.state.zoomStep; }
  set zoomStep(value) { this.set(C.zoomStep, "zoomStep", value); }
  get kick() { return this.state.kick; }
  get scale() { return this.state.scale; }
  get pendingMuzzles() { return this.state.pendingShots; }
  // PlayerCombat's basis is canonical in the native publication. Camera and
  // effects readers observe only a few bodies; keep their existing vectors
  // and refresh those three words without another module call.
  basis(vector, offset) {
    if (this.phase && this.owner.combat === this) this.phase.readCombatBasis(this,vector,offset);
    return vector;
  }
  get origin() { return this.basis(this.state.origin, V.origin); }
  get target() { return this.basis(this.state.target, V.target); }
  get aimDir() { return this.basis(this.state.aimDir, V.aimDir); }
  get shotDir() { return this.basis(this.state.shotDir, V.shotDir); }
  get left() { return this.basis(this.state.left, V.left); }
  get up() { return this.basis(this.state.up, V.up); }
  get pivot() { return this.basis(this.state.pivot, V.pivot); }
  get zoomed() { return this.zoomStep > 0; }
  set zoomed(on) { if (!on && this.zoomStep !== 0) this.zoomStep = 0; }
  get zoom() {
    const weapon = this.arsenal?.current(0);
    if (!this.zoomStep || !weapon) return 1;
    return PlayerCombat.zooms(weapon.def)[this.zoomStep] || 1;
  }
  get usesEyeView() { return Boolean(this.firstPerson || (this.zoom > 1 && this.arsenal.current(0)?.def.ZoomFirstPerson)); }
  // Cold construction captures the complete initial combat. After adoption
  // Rust retains its notification latches and pending shots; gameplay never
  // reads those lists through this facade.
  get hasRandom() { return Boolean(this.random); }
  get hasRemotes() { return Boolean(this.remotes); }
  get seenList() { return this.state.seenList; }
  get pendingShots() { return this.state.pendingShots; }

  // A field the battle writes: the module's, and the mirror's at once.
  set(field, name, value) {
    this.sim.setCombat(this, field, Number(value));
    this.state[name] = value;
  }

  // PlayerCombat.updateHeld selects the weapon model shown to the page.
  // A thrown secondary remains visible while its native action is busy.
  // The animation bank is presentation policy over published weapon state.
  // This helper neither updates weapons nor emits a gameplay shot.
  updateHeld() {
    if (!this.arsenal) return;
    const thrown = this.arsenal.current(1);
    const weapon = thrown?.busy || thrown?.shownAtWork > 0 ? thrown : this.arsenal.current(0);
    this.shown = this.dead || this.mounted ? null : weapon;
    this.animator.setBank?.(weapon?.bank || "rifle");
  }

  // The initial native record precedes the body's sole adoption. This
  // decoder reads combat construction state without importing a Unit body.
  // PlayerCombat's initial read is a cold, complete native record.
  // Subsequent live publications use the owning phase's retained word deltas.
  // Resolve the heap through the shared generated block header layout.
  // No constructor-provided Unit fields are copied back in this read.
  readConstruction(ptr) {
    const b = this.sim.bridge; b.views();
    const base = ptr / 8, at = base + HEADER, heap = base + b.f[base + BlockHeaderLayout.heap];
    syncCombat(b,b.f,at,heap,this.state);
  }

  // Headless viewpoint and network readers use the native published basis.
  // Camera selection reads those records without a JS eye or aim algorithm.
  clearCharge() { this.glow?.kill(); this.glow = null; }
  firePoint() { return null; }
  static zooms(def) { return PlayerCombat.zooms(def); }
}
