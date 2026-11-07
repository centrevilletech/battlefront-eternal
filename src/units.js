import * as THREE from "three";
import { JetpackNozzles } from "./jetpack-nozzles.js";
import { buildActor } from "./actor.js";
import { loadClips } from "./anim.js";
import { SoldierAnimation } from "./animator.js";
import { PlayerCombat } from "./combat.js";
import { Droideka, ShieldBubble, goneUp } from "./droideka.js";
import { heatMaterial } from "./destruction-debris.js";
import { ENGINE, Soldier } from "./soldier.js";

import { SoldierPresentation, HeadlessSoldierPresentation } from "./soldier-presentation.js";

// The battle's units: every soldier on the field, the player's included.
//
// A unit is one body of one class: its physics (Soldier, or Droideka), its
// weapons (the same combat code the player's soldier runs), its skinned
// body and animation, and the smoothing that draws it between physics
// steps. Bots and the player share all of it; only what feeds them differs.
// A bot's controller writes an ActorIntent (docs/ai.md) each tick, and the
// unit plays it through the same movement and weapon code a player's keys
// go through. Bots never move by setting their position.
//
// Units are pooled per class: a Character that dies and respawns as the
// same class takes a free body of that class back, its generation bumped so
// that old references to it go stale.

// A body lies 4 s where it fell, then is gone (the soldier's dead state).
const JET_EFFECT_BONE = "bone_ribcage";

// Mines feel a soldier's bounding sphere: its centre above the feet and its
// size (as main.js has them for the player).
const BODY_MIDDLE = 1.048;
const BODY_RADIUS = 1.278;

export function blankIntent() {
  return {
    forward: 0, right: 0, turn: 0, pitch: 0,
    jump: false, crouch: false, prone: false, use: false,
    buttons: [0, 0],
    reload: false, nextPrimary: false, previousPrimary: false, nextSecondary: false, zoom: false,
    aim: null,
    jetDirection: null, // a jump jet's launch direction, as the path asks (Vector3)
    exit: false, // leave the vehicle or turret (edge)
  };
}

// Turns an intent's weapon part into the take/held calls PlayerCombat makes
// of the keyboard. A channel's button counts above zero are held; the first
// held count after a release is a press. (The Classic fire-request consumer
// supplies the counts; see docs/ai-combat.md.)
export class IntentControls {
  constructor(unit) {
    this.unit = unit;
    this.was = [0, 0];
    this.pressed = [false, false];
  }

  // Called once per tick before the combatant reads it.
  latch() {
    const b = this.unit.intent.buttons;
    for (let c = 0; c < 2; c++) {
      this.pressed[c] = b[c] > 0 && !(this.was[c] > 0);
      this.was[c] = b[c];
    }
  }

  take(name) {
    const intent = this.unit.intent;
    switch (name) {
      case "firePrimary": {
        const p = this.pressed[0];
        this.pressed[0] = false;
        return p;
      }
      case "fireSecondary": {
        const p = this.pressed[1];
        this.pressed[1] = false;
        return p;
      }
      case "use":
      case "reload":
      case "nextPrimary":
      case "previousPrimary":
      case "nextSecondary":
      case "zoom": {
        const v = intent[name];
        intent[name] = false;
        return v;
      }
      default:
        return false;
    }
  }

  held(name) {
    const b = this.unit.intent.buttons;
    if (name === "firePrimary") return b[0] > 0;
    if (name === "fireSecondary") return b[1] > 0;
    return false;
  }
}

export class Unit {
  constructor({ side, className, def, actor, soldier, animator, combat, presentation, walker = false, human = false }) {
    this.id = 0;
    this.generation = 0;
    this.side = side;
    this.className = className;
    this.classDef = def;
    this.actor = actor;
    this.soldier = soldier;
    this.animator = animator;
    this.combat = combat;
    this.presentation = presentation;
    this.walker = walker;
    this.human = human;
    // EntityDroideka classifies as 3, every other soldier body as 0.
    this.kind = walker ? 3 : 0;
    this.hero = Boolean(def.hero || def.IsHero);
    this.team = 0;
    this.character = null;
    this.controller = null;
    this.seat = null;
    this.alive = false;
    this.dead = false;
    this.deadTime = 0;
    this.removed = true;
    this.intent = blankIntent();
    this.controls = new IntentControls(this);
    this.input = { forward: 0, right: 0, turn: 0, pitch: 0, jump: false, crouch: false, prone: false, use: false, jetDirection: null };
    this.bounds = { center: new THREE.Vector3(), radius: 1 };
    this.rootScale = def.CollisionRootScale ?? 1;
    // Correction mirrors retain these fields; native ownership supplies all geometry.
    this.shapes=[0,1].map(()=>({type:"sphere",part:"body",center:new THREE.Vector3(),radius:0,half:0,yaw:0,halfSize:new THREE.Vector3()}));
    this.hitSet=this.shapes;this.shapeCount=0;
    this.spheresTick = -1;
    this.tickCount = 0;
    this.jetFlame = null;
    this.bubble = null;
    this.poseClock = 0; // s of animation not yet applied
    // The page's: the life whose body has gone up, and how long ago
    // ({ generation, t }); the body's own materials while it dissolves.
    this.gone = null;
    this.ownMaterials = null;
    this.onRemoved = () => {};
    this.onMoved = () => {};
    // Set later: by the factory (navRadius to itemTarget), a life
    // (targetable), water (waterContact), an open seat or a creature's grip
    // (riderHits, held), a bot's driving (steer) or a network player (the
    // rest). They start here as they read before they are set, undefined,
    // so every unit keeps one shape and the code that walks the battle's
    // units stays optimized instead of falling back on every new one.
    this.navRadius = undefined;
    this.crowdSlot = undefined;
    this.enhancedDeath = undefined;
    this.supply = undefined;
    this.itemTarget = undefined;
    this.targetable = undefined;
    this.waterContact = undefined;
    this.riderHits = undefined;
    this.held = undefined;
    this.steer = undefined;
    this.networkHuman = undefined;
    this.networkShot = undefined;
    this.networkShots = undefined;
    this.networkViewTick = undefined;
    this.networkInputLeadTicks = undefined;
    this.networkPlayoutMaxTicks = undefined;
    this.remoteStep = undefined;
  }

  get position() {
    return this.soldier.position;
  }

  get velocity() {
    return this.soldier.velocity;
  }

  get health() {
    return this.combat.health;
  }

  // The broadphase sphere around the hit spheres, for this tick's pose.
  get sphere() {
    // The native owner can publish bounds after observing its lazy shapes;
    // limb arrays need copying only when a reader asks hitShapes explicitly.
    return this.bounds;
  }

  // The point others aim at: the middle of the body.
  aimPoint(out) {
    return out.copy(this.sphere.center);
  }

  // Where the unit looks from: the aim pivot (the gun origin's base).
  origin(out) {
    if (this.seat) return this.seat.origin(out);
    if (this.combat.pivot.lengthSq() > 0) return out.copy(this.combat.pivot);
    const p = this.soldier.position;
    return out.set(p.x, p.y + (this.soldier.posture === "prone" ? 0.6 : this.soldier.posture === "crouch" ? 1.2 : 1.7), p.z);
  }

  // The aim direction, unit length.
  forward(out) {
    if (this.seat) return this.seat.forward(out);
    const s = this.soldier;
    return out.set(Math.sin(s.aimYaw) * Math.cos(s.aimPitch), Math.sin(s.aimPitch), Math.cos(s.aimYaw) * Math.cos(s.aimPitch));
  }

  // Page hit shapes are a retained native publication, refreshed by the
  // registry's coarse observation. Authority installs its own native getter.
  hitShapes(){return this.hitSet;}

  // The posture whose shapes apply: standing in the air, crouched in a
  // dive, lying down prone or dead.
  get hitPosture() {
    const s = this.soldier;
    if (this.dead) return "prone";
    if (s.dive) return "crouch";
    if (s.air) return "stand";
    return s.posture;
  }

  // Into the world at (x, y, z) facing yaw, fresh.
  // Controller and navigation writers use the body's qualified native
  // command port. A SeatUnit exposes the same planner-facing interface.
  bindController(controller) { this.f3.controller(this,controller); }
  consumeContact() { this.f3.consumeContact(this); }
  bindMountedRandom(lease,draw) { this.f3.mountedRandom(lease,draw); }

  spawn(x, y, z, yaw) {
    this.seat = null;
    this.letGo();
    this.targetable = true;
    this.combat.mounted = false;
    this.generation += 1;
    this.soldier.dead = false;
    this.soldier.spawn(x, y, z, yaw);
    this.combat.reset();
    this.presentation.reset();
    this.animator.reset(yaw);
    this.actor.root.visible = true;
    this.actor.root.position.copy(this.presentation.position);
    this.alive = true;
    this.dead = false;
    this.deadTime = 0;
    this.removed = false;
    this.intent = blankIntent();
    this.controls.was = [0, 0];
    this.tickCount += 1;
  }

  // Another body for the same unit (the player's, between lives): its class,
  // physics, weapons and model, and everything that follows from the class.
  // The unit keeps its id, generation, team and Character.
  setBody({ side, className, def, actor, soldier, animator, combat, presentation, walker = false }) {
    this.restoreLook();
    this.gone = null;
    this.side = side;
    this.className = className;
    this.classDef = def;
    this.actor = actor;
    this.soldier = soldier;
    this.animator = animator;
    this.combat = combat;
    this.presentation = presentation;
    this.walker = walker;
    this.kind = walker ? 3 : 0;
    this.hero = Boolean(def.hero || def.IsHero);
    this.rootScale = def.CollisionRootScale ?? 1;
    // Correction mirrors retain these fields; native ownership supplies all geometry.
    this.shapes=[0,1].map(()=>({type:"sphere",part:"body",center:new THREE.Vector3(),radius:0,half:0,yaw:0,halfSize:new THREE.Vector3()}));
    this.hitSet=this.shapes;this.shapeCount=0;
    this.spheresTick = -1;
    this.poseClock = 0;
    this.shadowNear = undefined;
    this.jetFlame?.stop();
    this.jetFlame = null;
  }

  // Off the field (after a death, or between lives). Its Character lets go
  // of it: the body goes back to the pool for anyone's next spawn.
  remove() {
    this.alive = false;
    this.removed = true;
    this.letGo();
    if (this.character?.unit === this) this.character.unit = null;
    this.character = null;
    this.undraw();
    this.onRemoved(this);
  }

  // Out of a creature's hold (src/props.js release): upright again, in his
  // own clips, his weapons his own.
  letGo() {
    if (!this.held) return;
    this.held = null;
    if (this.combat) this.combat.mounted = false;
    this.animator?.gunner?.(null);
    this.actor?.root?.rotation.set(0, this.actor.root.rotation.y, 0);
  }

  // Nothing of it drawn: its body, a droideka's shield, a jet flame, a
  // weapon's charge glow.
  undraw() {
    this.actor.root.visible = false;
    this.restoreLook();
    this.bubble?.hide();
    this.jetFlame?.stop();
    this.jetFlame = null;
    this.combat?.clearCharge?.();
  }

  // Its body went up (the page's): nothing more of this life is drawn, but
  // for a droideka's body dissolving away first.
  goUp() {
    if (this.gone?.generation !== this.generation) this.gone = { generation: this.generation, t: 0 };
  }

  // The body dissolving by `share` (0 to 1) with heatMaterial's noise, lit
  // as its own materials are this frame: charred and glowing as it burns
  // away in Enhanced (enhanced), plain in Classic.
  dissolve(share, enhanced) {
    const body = this.actor.body;
    if (!this.ownMaterials) {
      this.ownMaterials = body.material;
      body.material = Array.isArray(body.material) ? body.material.map((material) => heatMaterial(material)) : heatMaterial(body.material);
      body.userData.heated = { heat: 0, tear: new THREE.Vector4(0, 0.9, 0, 0.6), base: 0, scale: 0.6, soot: 0, dissolve: 0, sun: 1, brightness: 1, darken: 0, lighten: 0 };
    }
    const look = body.userData.heated;
    const lit = Array.isArray(this.ownMaterials)?this.ownMaterials[0]:this.ownMaterials;
    if (lit?.actorAppearance) {
      look.sun = lit.values[0];
      look.brightness = lit.values[3];
      look.darken = lit.values[1];
      look.lighten = lit.values[2];
    }
    look.dissolve = share * 1.02;
    look.heat = enhanced ? 0.9 * (1 - share * 0.5) : 0;
    look.base = enhanced ? 0.3 : 0;
    look.soot = enhanced ? 1 : 0;
  }

  // Its own materials back on the body (after a dissolve).
  restoreLook() {
    if (!this.ownMaterials) return;
    this.actor.body.material = this.ownMaterials;
    delete this.actor.body.userData.heated;
    this.ownMaterials = null;
  }

  castShadows(near) {
    this.shadowNear = near;
    if (this.actor.shadow) this.actor.shadow.visible = near;
    for (const held of this.combat?.held?.values?.() ?? []) {
      held.group?.traverse((mesh) => {
        if (!mesh.userData.actorAsset) return;
        if (!near) { mesh.userData.castShadowNear ??= mesh.castShadow; mesh.castShadow = false; }
        else if (mesh.userData.castShadowNear !== undefined) { mesh.castShadow = mesh.userData.castShadowNear; delete mesh.userData.castShadowNear; }
      });
    }
  }

  updateJetFlame(effects) {
    const name = this.classDef.JetEffect;
    const bone = this.actor.byName[JET_EFFECT_BONE];
    const active = this.alive && this.soldier.air === "jet" && name && bone;
    if (active) {
      bone.updateWorldMatrix(true, false);
      const at = (this.flameAt ||= new THREE.Vector3());
      const turn = (this.flameTurn ||= new THREE.Quaternion());
      const scale = (this.flameScale ||= new THREE.Vector3());
      bone.matrixWorld.decompose(at, turn, scale);
      if (!this.jetFlame) this.jetFlame = effects?.play(name, at, null, this.soldier.velocity, turn) || null;
      else this.jetFlame.move(at, turn, this.soldier.velocity);
      this.jetFlame?.attachNozzles?.(this.actor.jetpackNozzles?.update());
    } else if (this.jetFlame) {
      this.jetFlame.stop();
      this.jetFlame = null;
    }
  }
}

// A class's OverrideTexture takes the place of the texture most of its body
// uses, and OverrideTexture2 of the next: { body texture: override }.
export function textureOverrides(def, bodyDef) {
  const textureUse = new Map();
  for (const part of bodyDef.parts) textureUse.set(part.texture, (textureUse.get(part.texture) || 0) + part.vertexCount);
  const byUse = [...textureUse.keys()].sort((a, b) => textureUse.get(b) - textureUse.get(a));
  const overrides = {};
  ["OverrideTexture", "OverrideTexture2"].forEach((key, i) => {
    if (def[key] && byUse[i]) overrides[byUse[i]] = def[key];
  });
  return overrides;
}

// One body of a class, built the same way for the player and the bots: the
// skinned model (OverrideTexture re-skins it in place of the texture most of
// it uses, OverrideTexture2 the next), its physics (a Soldier, or a Droideka
// walking by its own clips) and its animator. Nothing is added to a scene.
// data: the body's model ({ def, buf, indexBuf }); walkerData: a droideka's
// clips ({ meta, buf }); clips: the shared clips for the body's bones.
export function buildBody({ def, data, walkerData = null, texture, graphics, collision, clips, animMeta, maxFlyHeight = 200, headless = false }) {
  const walker = def.base === "walkerdroid";
  const actor = buildActor(data.def, data.buf, data.indexBuf, texture, graphics, textureOverrides(def, data.def));
  if (def.JetEffect) actor.jetpackNozzles = new JetpackNozzles(actor, data.def);
  let soldier;
  let animator;
  if (walker) {
    soldier = new Droideka(collision, def, loadClips(walkerData.meta, walkerData.buf, data.def.bones.map((bone) => bone.name)), actor);
    animator = soldier.animator;
  } else {
    soldier = new Soldier(collision, def);
    animator = new SoldierAnimation(actor, clips, animMeta.sets, headless);
  }
  // Anyone in the air is pushed back down near the mission's ceiling.
  soldier.maxFlyHeight = maxFlyHeight;
  // Getting up from prone holds the soldier still for the get-up clip.
  soldier.getupTime = clips.get("human_rifle_prone_getup_stand")?.duration ?? 0;
  // A side dive lasts three quarters of the dive clip.
  soldier.diveTime = ENGINE.diveLength * (clips.get("human_rifle_diveforward")?.duration ?? 0);
  return { actor, soldier, animator, walker, clips };
}

// Builds units for any class of the mission's sides, and keeps their
// bodies for reuse. Loading is asynchronous (prepare() each class first);
// building and acquiring are not, so a spawn never waits on the network.
export class UnitFactory {
  // sides: name -> { dir, units, items, texture(name), firePoints }
  // context: { graphics, scene, collision, animMeta, animBuf, json, bin,
  //   ordnance, lights, weaponModels, flashModel, anyTexture, effects,
  //   settings, maxFlyHeight, items, live, units (registry), destruction
  //   (Enhanced droid deaths, or null) }
  constructor(sides, context) {
    this.sides = sides;
    this.context = context;
    this.loading = new Map(); // key -> Promise
    this.loaded = new Map(); // key -> resolved data
    this.clipSets = new Map(); // bone names -> clips
    this.built = [];
  }

  // The side a class belongs to.
  sideOf(className) {
    for (const [name, side] of Object.entries(this.sides)) if (side.units[className]) return name;
    return null;
  }

  load(key, fetcher) {
    if (!this.loading.has(key)) {
      this.loading.set(key, fetcher().then((value) => {
        this.loaded.set(key, value);
        return value;
      }));
    }
    return this.loading.get(key);
  }

  clipsFor(bones) {
    const key = bones.map((bone) => bone.name).join(",");
    if (!this.clipSets.has(key)) {
      const { animMeta, animBuf } = this.context;
      this.clipSets.set(key, loadClips(animMeta, animBuf, bones.map((bone) => bone.name)));
    }
    return this.clipSets.get(key);
  }

  // Load what a class needs before its first spawn.
  // Side packages also contain non-infantry definitions, such as Hoth's
  // probe droid. Only soldier bodies belong in this factory's prerequisites.
  async prepareSoldiers() {
    const classes = Object.values(this.sides).flatMap(side => Object.entries(side.units)
      .filter(([, def]) => def.base === 'soldier' || def.base === 'walkerdroid')
      .map(([name]) => name));
    await Promise.all([...new Set(classes)].map(name => this.prepare(name)));
  }

  async prepare(className) {
    const sideName = this.sideOf(className);
    if (!sideName) throw new Error(`no side has the class ${className}`);
    const { json, bin } = this.context;
    const def = this.sides[sideName].units[className];
    // Soldiers and droidekas have bodies here; Hoth's probe droid (a droid,
    // its mission's team 4, which never deploys) has none.
    if (def.base !== "soldier" && def.base !== "walkerdroid") throw new Error(`${className} is a ${def.base}, not a soldier`);
    const dir = `${this.sides[sideName].dir}/actors`;
    await this.load(`body:${sideName}/${def.model}`, () =>
      Promise.all([json(`${dir}/${def.model}.json`), bin(`${dir}/${def.model}.bin`), bin(`${dir}/${def.model}_index.bin`)])
        .then(([meta, buf, indexBuf]) => ({ def: meta, buf, indexBuf })));
    if (def.base === "walkerdroid") {
      await this.load(`clips:${sideName}/${def.clips}`, () =>
        Promise.all([json(`${dir}/${def.clips}.json`), bin(`${dir}/${def.clips}.bin`)]).then(([meta, buf]) => ({ meta, buf })));
    }
    return def;
  }

  prepared(className) {
    const sideName = this.sideOf(className);
    const def = sideName && this.sides[sideName].units[className];
    return Boolean(def && this.loaded.has(`body:${sideName}/${def.model}`));
  }

  // A prepared class's model ({ def, buf, indexBuf }), or null.
  bodyData(className) {
    const sideName = this.sideOf(className);
    const def = sideName && this.sides[sideName].units[className];
    return (def && this.loaded.get(`body:${sideName}/${def.model}`)) || null;
  }

  // A prepared droideka's own clips ({ meta, buf }), or null for a soldier.
  walkerData(className) {
    const sideName = this.sideOf(className);
    const def = sideName && this.sides[sideName].units[className];
    return def?.base === "walkerdroid" ? this.loaded.get(`clips:${sideName}/${def.clips}`) || null : null;
  }

  // A free unit of the class, built if none is free. The class must have
  // been prepared.
  acquire(className) {
    return this.context.unitPhase.acquire(className);
  }

  release(unit) {
    return this.context.unitPhase.release(unit);
  }

  // `page`: a body a page draws and predicts its own player with (a tool's
  // stand-in for one): its combat is the JavaScript's even where the
  // world's combats are on the simulation's module.
  build(className, { page = false, prepared = false } = {}) {
    const c = this.context;
    const sideName = this.sideOf(className);
    if (!sideName) throw new Error(`no side has the class ${className}`);
    const side = this.sides[sideName];
    const def = side.units[className];
    const body = this.loaded.get(`body:${sideName}/${def.model}`);
    if (!body) throw new Error(`${className} wasn't prepared`);
    const texture = (name) => side.texture(name) || c.anyTexture(name);
    const { actor, soldier, animator, walker } = buildBody({
      def, data: body, walkerData: this.walkerData(className), texture, graphics: c.graphics, collision: c.collision,
      clips: this.clipsFor(body.def.bones), animMeta: c.animMeta, headless:c.headless, maxFlyHeight: c.maxFlyHeight ?? 200,
    });
    actor.root.visible = false;
    // The body's bind-pose box, for the path planner's clearance: a quarter
    // of its horizontal diagonal (docs/ai-nav.md).
    let box=actor.body.userData.bindBounds;
    if(!box){actor.body.geometry.computeBoundingBox();box=actor.body.geometry.boundingBox;}
    c.scene.add(actor.root);
    const unit = new Unit({ side: sideName, className, def, actor, soldier, animator, walker,
      combat: null, presentation: new (c.headless ? HeadlessSoldierPresentation : SoldierPresentation)(soldier) });
    unit.navRadius = Math.fround(0.25 * Math.hypot(box.max.x - box.min.x, box.max.z - box.min.z));
    // Worlds construct native combat facades; pages keep drawn weapon state.
    const combat = (c.headless && !page ? c.combatSim.make.bind(c.combatSim) : (options) => new PlayerCombat(options))({
      soldier, def, actor, animator,
      collision: c.collision,
      camera: null,
      ordnance: c.ordnance,
      lights: c.lights,
      weaponModels: c.weaponModels,
      firePoints: side.firePoints || {},
      flashModel: c.flashModel,
      texture,
      hud: null,
      caster: (object) => c.graphics.caster(object),
      owner: unit,
      player: false,
    });
    combat.items = c.items;
    combat.live = c.live;
    combat.effects = c.effects;
    combat.deferMuzzles = walker;
    combat.onRespawn = () => {};
    unit.combat = combat;
    // Its shadow, and its guns', through the crowd's one mesh (crowd-shadows.js).
    unit.crowdSlot = c.graphics.crowdShadows?.add(unit) ?? null;
    if (walker) {
      soldier.arm(def.weapons || []);
      const shieldWeapon = (def.weapons || []).find((w) => w.base === "shield");
      const look = c.effects.shields[shieldWeapon?.ShieldEffect];
      unit.bubble = !c.headless && look ? new ShieldBubble(c.scene, look, c.anyTexture, c.settings) : null;
      // The game's hook flares the bubble (or records it, in a world worker).
      soldier.onShieldHit = (amount, source, kind) => (c.shieldHit ? c.shieldHit(unit, amount, source, kind) : unit.bubble?.hit(source, amount, soldier, kind));
      // How its body dissolves as it goes up (Unit.dissolve).
      unit.enhancedDeath = () => (c.settings?.get?.("destructionEffects") ?? "enhanced") === "enhanced";
    }
    unit.onRemoved = (u) => this.release(u);
    // What recharge droids serve, and what mines and power-ups see.
    const healthType = (def.HealthType || "person").toLowerCase();
    unit.supply = {
      position: soldier.position,
      healthType,
      get health() { return combat.health; },
      set health(value) { combat.health = value; },
      get maxHealth() { return combat.maxHealth; },
      get weapons() { return combat.arsenal.weapons; },
    };
    const center = new THREE.Vector3();
    unit.itemTarget = {
      position: soldier.position,
      get center() { return center.set(soldier.position.x, soldier.position.y + BODY_MIDDLE, soldier.position.z); },
      radius: BODY_RADIUS,
      velocity: soldier.velocity,
      relation: (team) => c.units.relation((unit.canonicalTarget ?? unit).team, team),
    };
    // A correction tool can construct an independent page body beside its
    // headless authority. It predicts and draws that body, never ticks it as
    // another world unit or lends its ordinary arsenal to native ownership.
    if (page && c.headless) return unit;
    // Cold stock has no registry identity yet. Its temporary slab row is
    // transferred out before another body uses this same construction slot.
    if (prepared) unit.id = c.units.nextId;
    else c.units.add(unit);
    // (A combat on the module is the store's from now on: it has his id.)
    combat.attach?.();
    if (c.unitPhase) c.unitPhase.adopt(unit);
    if (!prepared) this.built.push(unit);
    c.onBuilt?.(unit);
    return unit;
  }
}
