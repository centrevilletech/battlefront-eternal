import { KIND_UNIT, KIND_CHARACTER, F3ColdStageMirror, F3ColdStageLayout, putF3ColdStage,
  F3PlayerBindingMirror, F3PlayerBindingLayout, putF3PlayerBinding,
  F3PlayerSpawnMirror, F3PlayerSpawnLayout, putF3PlayerSpawn } from './sim/gen/index.js';

// UnitFactory.build prepares bodies outside ticking. Their complete native
// rows and rig/query storage move at birth; inactive stock never appears in
// Units.list, so it cannot alter a raycast tie or a death explosion's order.
export class F3Factory {
  constructor(phase) {
    this.phase = phase;
    this.ready = [];
    this.retired = false;
    this.stage = new F3ColdStageMirror();
    this.stageArgs = {words:F3ColdStageLayout.words,put:putF3ColdStage};
    this.player = new F3PlayerBindingMirror();
    this.playerArgs = {words:F3PlayerBindingLayout.words,put:putF3PlayerBinding};
    this.spawn = new F3PlayerSpawnMirror();
    this.spawnArgs = {words:F3PlayerSpawnLayout.words,put:putF3PlayerSpawn};
  }

  // `limit` is the future mixed registry id bound, including vehicle births.
  // Preparing admission capacity must finish before a native factory runs.
  reserve(limit) {
    const p = this.phase;
    if (p.active) throw Error('unit preparation inside the owned phase');
    p.x.f3_factory_reserve(limit);this.limit=Math.max(this.limit??0,limit);
    p.readLayout();
    p.counters.wasmCalls++;p.counters.lifecycleCalls++;p.counters.inputFields++;
  }

  // The temporary source slot is reused after staging takes its entire row.
  // Neither the JS factory list nor its free stack observes this construction.
  prepare(className, count = 1) {
    const p = this.phase, refs = p.bridge.refs[KIND_UNIT];
    if (p.active) throw Error('unit preparation inside the owned phase');
    for (let i = 0; i < count; i++) {
      const unit = p.factory.build(className,{prepared:true});
      const source = unit.id, binding = p.bindings[source];
      // onBuilt binds gameplay's RNG after adoption. Complete that binding
      // now rather than asking JavaScript to supply it during native birth.
      p.random.refresh(binding);
      const cookie = this.ready.length;
      this.ready.push(binding);
      binding.detached = unit;
      this.stage.cookie = cookie;this.stage.adoption = binding.adoption;
      p.bridge.callArgs(this.stageArgs,this.stage);p.x.f3_factory_stage();
      p.counters.wasmCalls++;p.counters.lifecycleCalls++;p.counters.inputFields += this.stageArgs.words;
      p.bindings[source] = undefined;
      refs.gens[source] = 0;
      refs.objects[source] = undefined;
      p.combatSim.facades[source] = undefined;
    }
  }

  // PlayerBodies initially owns the viewpoint outside UnitFactory. Keep a
  // cold facade for that class so swapping away can retain its body vectors,
  // native rig and arsenal without exposing it through another class pool.
  resident(binding) {
    const p = this.phase, cookie = this.ready.length;
    binding.detached = Object.assign(Object.create(Object.getPrototypeOf(binding.unit)),binding.unit);
    this.ready.push(null);
    this.stage.cookie = cookie;this.stage.adoption = binding.adoption;
    p.bridge.callArgs(this.stageArgs,this.stage);p.x.f3_factory_resident();
    p.counters.wasmCalls++;p.counters.lifecycleCalls++;p.counters.inputFields += this.stageArgs.words;
  }

  // Admission resolves streams without creating their lazy RandomStreams
  // entries. The native player factory later reuses these numeric bindings.
  bindPlayer(character, body, {local = false, combatRandom = null, weaponRandom = null,
    reconKey = null, reconSeed = 0} = {}) {
    const p = this.phase, row = this.player;
    if (p.active) throw Error('player admission inside the owned phase');
    row.character = character;row.body = body;row.local = local;
    row.combatRandom = p.random.register(combatRandom);
    row.weaponRandom = p.random.register(weaponRandom);
    row.reconKey = reconKey;row.reconSeed = reconSeed;
    p.one[0] = character;p.bridge.writeSome(KIND_CHARACTER,p.one);
    p.bridge.callArgs(this.playerArgs,row);p.x.f3_factory_player_bind();
    p.counters.wasmCalls++;p.counters.lifecycleCalls++;p.counters.inputFields += this.playerArgs.words;
  }

  // HumanSpawn's outside-phase caller uses the same operation as F4's
  // native factory. Class selection and reset never call a JS constructor.
  spawnPlayer(character, previous, className, position, yaw, local = false) {
    const p = this.phase, row = this.spawn;
    if (p.active) throw Error('external player spawn inside the owned phase');
    row.character = character;row.previous = previous;row.className = className;
    row.position.copy(position);row.yaw = yaw;row.local = local;
    p.bridge.callArgs(this.spawnArgs,row);
    const slot = p.x.f3_factory_player_spawn();
    p.counters.wasmCalls++;p.counters.lifecycleCalls++;p.counters.inputFields += this.spawnArgs.words;
    p.sync();
    return slot < 0 ? null : p.bindings[slot].unit;
  }

  // This structural notice runs before generated readers decode Unit refs.
  // It binds prepared facades only: no module call or gameplay callback can
  // reenter the owner while it is between moving and firing another body.
  created(event) {
    const p = this.phase, key = event.key, binding = this.ready[event.cookie];
    if (!binding) return; // The ordinary event cursor sees this notice again.
    this.ready[event.cookie] = null;
    let unit = binding.unit;
    if (event.replacement) {
      this.retired = true;
      const outgoing = p.bindings[key.slot], old = outgoing.unit, combat = old.combat;
      if (event.retiredCookie !== null) {
        p.fleet?.riders.alias(old,outgoing.detached);
        outgoing.retarget(outgoing.detached);
        this.ready[event.retiredCookie] = outgoing;
      }
      // PlayerBodies.adopt keeps the viewpoint's identity but changes its
      // class body. Copy prepared resources, preserving existing observers.
      old.side = unit.side;old.className = unit.className;old.classDef = unit.classDef;
      old.actor = unit.actor;old.soldier = unit.soldier;old.animator = unit.animator;
      old.combat = unit.combat;old.presentation = unit.presentation;old.walker = unit.walker;
      old.kind = unit.kind;old.rootScale = unit.rootScale;old.navRadius = unit.navRadius;
      old.supply = unit.supply;old.itemTarget = unit.itemTarget;old.riderHits = unit.riderHits;
      old.bounds = unit.bounds;old.shapes = unit.shapes;old.hitSet = unit.hitSet;
      old.shapeCount = unit.shapeCount;old.spheresTick = unit.spheresTick;
      old.spawn = unit.spawn;old.remove = unit.remove;old.letGo = unit.letGo;
      old.updateSpheres = unit.updateSpheres;old.hitShapes = unit.hitShapes;
      old.combat.onPresentation = combat.onPresentation;
      old.combat.onFired = combat.onFired;old.combat.onDamaged = combat.onDamaged;
      old.combat.onRespawn = combat.onRespawn;
      p.fleet?.riders.alias(unit,old);
      unit.canonicalTarget = old;
      binding.retarget(old);
      unit = old;
    } else {
      const units = p.registry.units;
      if (event.order !== units.list.length + 1) throw Error('unit registry birth order');
      units.list.push(unit);units.byIdMap.set(key.slot,unit);units.nextId = key.slot + 1;
      p.factory.built.push(unit);
    }
    unit.id = key.slot;unit.combat.owner = unit;
    const view = binding.view.key;
    view.slot = key.slot;view.generation = key.generation;view.life = key.life;
    binding.shapes.key = view;
    binding.arsenalKey.slot = key.slot;binding.arsenalKey.generation = key.generation;
    binding.arsenalKey.life = key.life;
    p.bindings[key.slot] = binding;
    const refs = p.bridge.refs[KIND_UNIT];
    refs.gens[key.slot] = key.generation;refs.slotOf(unit);
    p.combatSim.facades[key.slot] = unit.combat;
    p.items.schedule(binding);
  }

  // Native reset/rebind publishes callbacks as numeric ids. Set retained
  // mirror bindings directly so the next refresh cannot restore the stock's
  // gameplay stream or issue a setter from inside the consuming phase.
  streams(event) {
    const p = this.phase, binding = p.bindings[event.key.slot];
    if (binding.unit.generation !== event.key.life) return;
    const unit = binding.unit;
    unit.human = event.metadata.human;unit.networkHuman = event.metadata.networkHuman;
    unit.hero = event.metadata.hero;
    const combat = unit.combat;combat.kept.player=event.metadata.human;
    const draw = p.random.callback(event.combat);
    combat.kept.random = draw;binding.randomDraw = draw;
    const weapons = combat.arsenal.weapons, weaponDraw = p.random.callback(event.weapon);
    for (let i = 0; i < weapons.length; i++) {
      weapons[i].kept.random = weaponDraw;weapons[i].f3RandomDraw = weaponDraw;
    }
  }

  // Discarded native class rows survive all nested readers until this outer
  // return. The same retained retirement buffer serves later class changes.
  finish() {
    if (this.phase.active || !this.retired) return;
    this.retired = false;
    this.phase.x.f3_factory_finish();
    this.phase.counters.wasmCalls++;this.phase.counters.lifecycleCalls++;
  }
}
