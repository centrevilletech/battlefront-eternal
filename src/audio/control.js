import * as L from "./control-layout.js";
import { ControlResources } from "./control-resources.js";
import { loadSim } from "../sim/module.js";
import { fnv1a, keyOf } from "./hash.js";
import { attachAudioStream } from "./stream-pcm.js";

const LIMIT = 4096, INPUT_CAPACITY = 1 << 20, SMALL_PAYLOAD = 256, STREAM_PARTS = 96;
const HASH_KEY = /^0x[0-9a-f]{8}$/;
const hash = (name) => typeof name === "string" && HASH_KEY.test(name) ? parseInt(name.slice(2),16) : fnv1a(String(name || ""));
const ROOM_KEYS = ["room","roomHF","decay","hfRatio","reflections","reflDelay","reverb","reverbDelay","diffusion","density","hfReference"];
const CATEGORY = ["effects","weapons","explosions","foley","ambience","music","voice","interface"];
const KEYS = {fire:L.EVENT_KIND.FIRE,actorFire:L.EVENT_KIND.ACTOR_FIRE,hurt:L.EVENT_KIND.HURT,death:L.EVENT_KIND.DEATH,actorHurt:L.EVENT_KIND.ACTOR_HURT,actorDeath:L.EVENT_KIND.ACTOR_DEATH,impact:L.EVENT_KIND.IMPACT,bounce:L.EVENT_KIND.BOUNCE,splash:L.EVENT_KIND.SPLASH,explosion:L.EVENT_KIND.EXPLOSION,sound:L.EVENT_KIND.SOUND,pickup:L.EVENT_KIND.POWERUP,serve:L.EVENT_KIND.POWERUP,turretFire:L.EVENT_KIND.TURRET_FIRE,interface:L.EVENT_KIND.INTERFACE,busFade:L.EVENT_KIND.BUS_FADE,vo:L.EVENT_KIND.CHATTER,post:L.EVENT_KIND.POST,leaving:L.EVENT_KIND.LEAVING,towAttach:L.EVENT_KIND.TOW_ATTACH,towRelease:L.EVENT_KIND.TOW_RELEASE};
const AIR = { jump:1, thrown:2, jet:3 }, POSTURE = {stand:0,crouch:1,prone:2}, FLIGHT = {Landed:1,TakingOff:2,Flying:3,Landing:4};

// Foreign objects have stable identity even when their scalar facts change.
// The mirror retains those facts between frames, so an idle source publishes
// nothing. A disappearance is explicit, including its final coordinates;
// native control can freeze a surviving one-shot and stop retained loops.
// Identity slots are recycled after that removal packet, rather than allowing
// an old source's history to leak into the next object that uses its number.
class Rows {
  constructor(layout, kind, presence) {
    this.layout = layout; this.kind = kind; this.presence = presence; this.count = 0;
    this.ids = new WeakMap(); this.sources = new Array(LIMIT).fill(null); this.metadata = new Array(LIMIT).fill(null); this.free = new Int32Array(LIMIT); this.freeCount = 0; this.stamps = new Uint32Array(LIMIT);
    this.previous = new Float64Array(LIMIT * layout.STRIDE).fill(NaN); this.row = new Float64Array(layout.STRIDE);
    // Registration reuses metadata as well as numeric rows. A new projectile
    // must not construct a new metadata object or grow the free-slot array.
    this.records = Array.from({length:LIMIT},()=>({name:null,class:-1,kind:0,unit:null,owner:-1,def:null,eventStamp:0,unitStamp:0,top:undefined,collision:undefined,shieldDef:null,shieldClass:-1,threshold:0,maxPressed:0,shotDelay:0,launcher:0,strike:0,zoomFirstPerson:0}));
  }
  // Registration is the cold identity edge. Class handles and rig references
  // are resolved here once, alongside the native entity registration, instead
  // of repeatedly looking up class strings while publishing an existing row.
  id(object) {
    if (!object) return -1; let id = this.ids.get(object);
    if (id === undefined) { id = this.freeCount ? this.free[--this.freeCount] : this.count++; if (id >= LIMIT) throw Error("audio: input identity capacity exhausted"); this.ids.set(object,id); this.sources[id] = object; }
    return id;
  }
  // Object.is preserves NaN-as-absent and signed-zero facts on the wire.
  // Comparing before writing also avoids republishing an entire fleet when
  // only one weapon state or emitter pose changed during this frame.
  publish(host,id) {
    const stride = this.layout.STRIDE, at = id * stride; let changed = false;
    this.row[this.layout.ID] = id; this.stamps[id] = host.stamp;
    for (let i=0;i<stride;i++) if (!Object.is(this.row[i],this.previous[at+i])) { changed = true; break; }
    if (changed) { host.packet(this.kind,this.row); for (let i=0;i<stride;i++) this.previous[at+i] = this.row[i]; }
  }
  missing(host) {
    const stride = this.layout.STRIDE;
    for (let id=0;id<this.count;id++) if (this.sources[id] && this.stamps[id] !== host.stamp) {
      const at = id * stride; for (let i=0;i<stride;i++) this.row[i] = this.previous[at+i]; this.row[this.presence] = this.kind === L.INPUT.WEAPON ? -1 : 0;
      this.publish(host,id); this.ids.delete(this.sources[id]); this.sources[id] = null; this.metadata[id] = null; this.free[this.freeCount++] = id;
    }
  }
}

// This host publishes facts and copies native outputs. It never chooses a
// sample, ranks a voice, calculates attenuation or schedules a segment.
export class SoundControl {
  constructor(audio, x, handle) {
    this.audio = audio; this.engine = audio.engine; this.renderer = this.engine.renderer; this.x = x; this.h = handle;
    this.room = Object.fromEntries(ROOM_KEYS.map((key)=>[key,undefined])); this.renderer.roomParams = this.room;
    this.at = 0; this.stamp = 0; this.eventCount = 0; this.events = new Float64Array(LIMIT * L.EVENT.STRIDE); this.empty = [];
    this.emitters = new Rows(L.EMITTER,L.INPUT.EMITTER,L.EMITTER.KIND); this.weapons = new Rows(L.WEAPON,L.INPUT.WEAPON,L.WEAPON.OWNER); this.posts = new Rows(L.POST,L.INPUT.POST,L.POST.PRESENT); this.items = new Rows(L.ITEM,L.INPUT.ITEM,L.ITEM.PRESENT); this.turrets = new Rows(L.TURRET,L.INPUT.TURRET,L.TURRET.PRESENT); this.feet = new Rows(L.FOOT,L.INPUT.FOOT,L.FOOT.PRESENT);
    this.frame = new Float64Array(L.FRAME.STRIDE); this.bodyFlags = new Int8Array(65536).fill(-1); this.body = new Float64Array(L.BODY.STRIDE); this.countRow = new Float64Array(L.COUNT.STRIDE);
    this.buffers = Array.from({length:4},()=>new Float64Array(L.INFO.STRIDE + 65536 + 8192));
    // A whole frame uses a large buffer, but each ready stream part emits
    // only a short start packet. All part completions may share one microtask
    // turn, before the page can receive any returned MessagePort credits.
    // Reserve those short packets separately rather than multiplying the
    // large frame arena by the number of simultaneously loading parts.
    this.smallBuffers = Array.from({length:STREAM_PARTS},()=>new Float64Array(L.INFO.STRIDE + SMALL_PAYLOAD)); this.message = {type:"controls",data:null}; this.transfer = [null];
    this.renderer.onControlBuffer = (data) => { if (data.length === L.INFO.STRIDE + SMALL_PAYLOAD) this.smallBuffers.push(data); else this.buffers.push(data); };
    this.releaseMessage = {type:"release-samples",ids:new Uint32Array(3)};
    this.preparedHulls = new WeakSet(); this.assets = Array.from({length:96},()=>({generation:0,asset:null}));
    this.inputAt = x.sound_input(handle); this.infoAt = x.sound_info(handle); this.commandsAt = x.sound_commands(handle); this.placementsAt = x.sound_placements(handle); this.requestsAt = x.sound_requests(handle); this.followsAt = x.sound_follows(handle);
    this.refresh(); this.offline = false;
  }
  // The controller borrows the page collision instance. Synthetic fixtures
  // create the same required native phase with analytic map resources; no
  // JavaScript ray callback or alternate decision implementation is involved.
  // Production seeds are chosen once, while recorded scenes supply their seed
  // explicitly so every probability, sample and deviation draw is auditable.
  static async prepare(audio, seed) {
    const x = audio.game.collision?.rays?.x || (await loadSim()).exports;
    if (typeof x.sound_step !== "function") throw Error("audio: required Rust sound-control module is missing");
    if (seed === undefined) { const value = new Uint32Array(1); globalThis.crypto.getRandomValues(value); seed = value[0]; }
    const host = new SoundControl(audio,x,x.sound_new(seed));
    host.resources = new ControlResources(audio.data,host.renderer,audio.game,audio.engine.enhanced).prepare(); host.resources.upload(host); host.refresh();
    host.prepareVehicles();
    const posts = audio.game.posts?.posts || host.empty;
    for (let i=0;i<posts.length;i++) host.post(posts[i]);
    if (audio.engine.enhanced) {
      audio.engine.enhanced.onSample = host.sampleCallback = (id,buffer)=>host.sampleReady(id,buffer);
      for (const [id,buffer] of audio.engine.enhanced.buffers) host.sampleReady(id,buffer);
    }
    // Bank PCM uploads precede this barrier on the output port. Native
    // control must not outrun a cold upload backlog before its first frame.
    await host.renderer.sync();
    return host;
  }
  refresh() {
    this.memory = this.x.memory.buffer;
    this.input = new Float64Array(this.memory,this.inputAt,INPUT_CAPACITY); this.info = new Float64Array(this.memory,this.infoAt,L.INFO.STRIDE);
    this.commands = new Float64Array(this.memory,this.commandsAt,65536); this.placements = new Float64Array(this.memory,this.placementsAt,8192); this.requests = new Float64Array(this.memory,this.requestsAt,128*L.REQUEST.STRIDE);
    this.follows = new Uint32Array(this.memory,this.followsAt,LIMIT);
  }
  sampleReady(id,buffer) {
    const index = this.resources.sampleIds.get(id); if (index === undefined) return;
    const r = new Float64Array(L.ENHANCED_SAMPLE.STRIDE); r[L.ENHANCED_SAMPLE.ID] = index; r[L.ENHANCED_SAMPLE.RENDER_ID] = this.renderer.engineSample(buffer);
    r[L.ENHANCED_SAMPLE.DURATION] = buffer.duration / (buffer.rateScale || 1); r[L.ENHANCED_SAMPLE.RATE] = buffer.sampleRate; r[L.ENHANCED_SAMPLE.SCALE] = buffer.rateScale || 1; r[L.ENHANCED_SAMPLE.AVAILABLE] = 1;
    this.import(L.IMPORT.ENHANCED_SAMPLE,r);
  }
  // Rig assets are prepared at attachment, never while publishing a frame.
  // Presentation matrices below are read as foreign facts without posing.
  prepareVehicles() {
    const hulls = this.audio.game.vehicles?.vehicles || this.empty;
    for (let i=0;i<hulls.length;i++) {
      const v = hulls[i]; if (this.preparedHulls.has(v)) continue; this.preparedHulls.add(v); this.resources.prepareFeet(v,this);
      const feet = v.def?.fx?.feet || this.empty;
      for (let j=0;j<feet.length;j++) { const foot = feet[j], identity = {hull:v,index:j}, id = this.feet.id(identity); this.feet.metadata[id] = {hull:v,index:j,node:(foot.bone && v.byName?.[foot.bone]) || v.root}; }
    }
  }
  import(kind, values) { if (this.memory !== this.x.memory.buffer) this.refresh(); this.input.set(values); this.x.sound_import(this.h,kind,values.length); if (this.memory !== this.x.memory.buffer) this.refresh(); }
  // Each packet uses generated type/count fields and a generated row stride.
  // The native parser retains unchanged rows, then consumes all queued scalar
  // events in their original order during the single sound_step below.
  packet(kind, values) {
    const end = this.at + L.PACKET.STRIDE + values.length; if (end > this.input.length) throw Error("audio: frame input capacity exhausted");
    this.input[this.at + L.PACKET.KIND] = kind; this.input[this.at + L.PACKET.COUNT] = 1; this.at += L.PACKET.STRIDE;
    for (let i=0;i<values.length;i++) this.input[this.at+i] = values[i]; this.at = end;
  }
  actor(combat, className, unit = null) {
    if (!combat?.soldier) return -1; const id = this.emitters.id(combat);
    let meta = this.emitters.metadata[id];
    if (!meta || meta.name !== className) { this.emitters.metadata[id] = meta = this.emitters.records[id]; meta.name = className; meta.class = this.resources.class(className); meta.kind = combat.soldier.rolled !== undefined ? L.EMITTER_KIND.DROIDEKA : L.EMITTER_KIND.SOLDIER; meta.unit = unit; meta.unitStamp = 0; }
    else if (unit) meta.unit = unit;
    meta.eventStamp = this.stamp + 1;
    return id;
  }
  weapon(weapon, owner) {
    if (!weapon?.def) return -1; const id = this.weapons.id(weapon); let meta = this.weapons.metadata[id];
    // A weapon's definition facts are read once here. Parsed definitions
    // are large dictionaries, too slow to read for every weapon every frame.
    if (!meta) {
      const d = weapon.def; this.weapons.metadata[id] = meta = this.weapons.records[id]; meta.class = this.resources.class(d.class); meta.def = d;
      meta.threshold = d.HeatThreshold ?? 1; meta.maxPressed = weapon.levels?.[0]?.MaxPressedTime || 1; meta.shotDelay = d.ShotDelay || 0;
      meta.launcher = d.base === "launcher" ? 1:0; meta.strike = d.strike ? 1:0; meta.zoomFirstPerson = d.ZoomFirstPerson ? 1:0;
    } meta.owner = owner;
    return id;
  }
  entity(object,kind,name) {
    const id = this.emitters.id(object); let meta = this.emitters.metadata[id];
    if (!meta) { this.emitters.metadata[id] = meta = this.emitters.records[id]; meta.name = name; meta.class = this.resources.class(name); meta.kind = kind; meta.unit = null; meta.top = undefined; meta.collision = undefined; } return id;
  }
  post(object) {
    const id = this.posts.id(object); if (!this.posts.metadata[id]) {
      const data = this.resources.data.world.posts?.[object.desc?.name] || this.resources.data.world.posts?.[object.name] || {};
      const className = data.class || "com_bldg_controlzone", classId = this.resources.class(className), props = this.resources.data.classSounds(className);
      const meta = {class:classId,sounds:new Float64Array(L.POST.STRIDE).fill(-1),counts:new Float64Array(16).fill(NaN),scratch:new Float64Array(16)};
      meta.count = (value,team)=> { if (team < 0 || team >= 16) throw Error("audio: post team capacity exhausted"); meta.scratch[team] = value; };
      for (const team of [1,2]) { const side = this.resources.sides[team]; const entry = [].concat(props?.AmbientSound || []).find((value)=>String(value).split(/\s+/)[0].toLowerCase() === side?.toLowerCase()); meta.sounds[L.POST["AMBIENT"+team]] = this.resources.sound(entry ? String(entry).split(/\s+/).slice(1).join(" ") : null);
        for (const actor of [1,2]) { const listener = this.resources.sides[team], owner = this.resources.sides[actor]; meta.sounds[L.POST["INFO"+team+actor]] = this.resources.sound(data.vo?.[`VO_${listener}_${owner}Info`]); meta.sounds[L.POST["CAPTURE"+team+actor]] = this.resources.sound(data.vo?.[`VO_${listener}_${owner}Capture`]); meta.sounds[L.POST["LOST"+team+actor]] = this.resources.sound(data.vo?.[`VO_${listener}_${owner}Lost`]); }
      } this.posts.metadata[id] = meta;
    } return id;
  }
  enqueue(kind, owner=-1, weapon=-1, cls=-1, sound=0, position=null, amount=0, health=0, value=0, seconds=0, positional=1) {
    if (this.eventCount >= LIMIT) throw Error("audio: sound event capacity exhausted"); const o = this.eventCount++ * L.EVENT.STRIDE, r = this.events;
    r[o+L.EVENT.KIND] = kind; r[o+L.EVENT.OWNER] = owner; r[o+L.EVENT.WEAPON] = weapon; r[o+L.EVENT.CLASS] = cls; r[o+L.EVENT.SOUND] = sound;
    r[o+L.EVENT.X] = position?.x ?? 0; r[o+L.EVENT.Y] = position?.y ?? 0; r[o+L.EVENT.Z] = position?.z ?? 0; r[o+L.EVENT.PLACED] = position ? 1 : 0;
    r[o+L.EVENT.AMOUNT] = amount; r[o+L.EVENT.HEALTH] = health; r[o+L.EVENT.VALUE] = value; r[o+L.EVENT.SECONDS] = seconds; r[o+L.EVENT.POSITIONAL] = positional;
  }
  // Ports GameAudio.on's observation boundary. Gameplay hooks arrive before
  // the next frame, so this journal records identity and event facts now.
  // It never starts a voice or advances a gate: native event dispatch runs
  // before listener measurement, preserving that timing for shots and hurts.
  event(type,a,b,c) {
    const kind = KEYS[type]; if (!kind) return; const g = this.audio.game;
    if (type === "fire" || type === "actorFire") { const owner = this.actor(a,type === "fire" ? g.unit : a?.owner?.className,a?.owner); this.enqueue(kind,owner,this.weapon(b?.weapon,owner),-1,0,null,0,0,b?.repair ? 1:0); }
    else if (type === "hurt" || type === "death" || type === "actorHurt" || type === "actorDeath") { const name = type === "hurt" || type === "death" ? g.unit : a?.owner?.className; const owner = this.actor(a,name,a?.owner); this.enqueue(kind,owner,-1,this.resources.class(name),0,null,b,a?.health); }
    else if (type === "interface") this.enqueue(kind,-1,-1,-1,hash(a));
    else if (type === "busFade") this.enqueue(kind,-1,-1,-1,hash(a),null,0,0,b,c);
    else if (type === "sound") this.enqueue(kind,-1,-1,-1,hash(a),b);
    else if (type === "leaving") this.enqueue(kind);
    else if (type === "post") { const post = this.post(a.post); this.enqueue(kind,post,-1,-1,0,null,0,0,a.kind === "captured" ? a.by : a.from,a.kind === "captured" ? 0 : a.kind === "neutralized" ? 1 : a.kind === "lost" ? 2 : 3); }
    else if (type === "vo") {
      const unit = a?.source?.unit, name = unit?.occupant?.className || unit?.className; if (!name || !a.property) return;
      const p = a.position; const o = this.eventCount * L.EVENT.STRIDE; this.enqueue(kind,-1,-1,this.resources.class(name),hash(a.property),null,0,0,0,0,a.positional === false ? 0 : 1);
      if (p) { this.events[o+L.EVENT.PLACED] = 1; this.events[o+L.EVENT.X] = Array.isArray(p) ? p[0] : p.x; this.events[o+L.EVENT.Y] = Array.isArray(p) ? p[1] : p.y; this.events[o+L.EVENT.Z] = Array.isArray(p) ? p[2] : p.z; }
    } else if (type === "turretFire") { const weapon = b?.weapon || a.weapon; if (weapon) this.enqueue(kind,-1,-1,this.resources.class(weapon.def.class),0,a.position); }
    else if (type === "serve") this.enqueue(kind,-1,-1,this.resources.class(a.desc?.class),0,a.position);
    else if (type === "towAttach" || type === "towRelease") this.enqueue(kind,-1,-1,this.resources.class(b?.class),0,a.position);
    else this.enqueue(kind,-1,-1,this.resources.class(a?.class),0,b);
  }
  // Every field of the row is written, so no clear is needed. Native
  // control reads heat only while overheated, the held time only while
  // charging, and the wait and empty clip only for the player's own weapons
  // (weapons.js::updateWeapon, PlayerSounds.empty). Holding them at zero
  // otherwise skips those reads and keeps a firing weapon's row unchanged.
  captureWeapon(weapon,owner,primary,secondary,ordinal,own) {
    const id = this.weapon(weapon,owner); if (id < 0) return;
    const r = this.weapons.row, m = this.weapons.metadata[id], hot = weapon.overheated, charging = weapon.charging;
    r[L.WEAPON.ORDINAL] = ordinal; r[L.WEAPON.OWNER] = owner; r[L.WEAPON.CLASS] = m.class; r[L.WEAPON.SELECTED] = (weapon === primary ? 1:0) | (weapon === secondary ? 2:0);
    r[L.WEAPON.RELOADING] = weapon.reloading ? 1:0; r[L.WEAPON.OVERHEATED] = hot ? 1:0; r[L.WEAPON.CHARGING] = charging ? 1:0; r[L.WEAPON.FIRING] = weapon.firing ? 1:0;
    r[L.WEAPON.HEAT] = hot ? weapon.heat ?? 0 : 0; r[L.WEAPON.THRESHOLD] = m.threshold; r[L.WEAPON.HELD] = charging ? weapon.held ?? 0 : 0; r[L.WEAPON.MAX_PRESSED] = m.maxPressed;
    r[L.WEAPON.WAIT] = own ? weapon.wait ?? 0 : 0; r[L.WEAPON.SHOT_DELAY] = m.shotDelay; r[L.WEAPON.EMPTY] = own && weapon.empty ? 1:0; r[L.WEAPON.LAUNCHER] = m.launcher; r[L.WEAPON.STRIKE] = m.strike; r[L.WEAPON.ZOOM_FIRST_PERSON] = m.zoomFirstPerson;
    this.weapons.publish(this,id);
  }
  // PlayerSounds and SoldierSounds used these same gameplay observations.
  // The host reads the already published pose and bone matrices; native
  // player.rs owns plant, stride, posture, scope and engine state transitions.
  // Sources retained only by a finishing one-shot still publish their pose,
  // without rejoining the live-soldier weapon or charge detectors.
  // Another soldier's row carries only what native control reads of him:
  // kind, class, place, life and removal (SoldierSounds, SceneSounds). The
  // body, view and control facts are the player's alone (PlayerSounds), so
  // other rows skip those getters, several of which search the arsenal.
  captureActor(id,own) {
    const source = this.emitters.sources[id], meta = this.emitters.metadata[id], soldier = source.soldier, unit = meta.unit, r = this.emitters.row, p = soldier.position;
    r.fill(0); r[L.EMITTER.KIND] = own || meta.unitStamp === this.stamp ? meta.kind : L.EMITTER_KIND.EVENT_ACTOR; r[L.EMITTER.CLASS] = meta.class;
    r[L.EMITTER.X] = p.x; r[L.EMITTER.Y] = p.y; r[L.EMITTER.Z] = p.z; r[L.EMITTER.ALIVE] = source.dead ? 0:1; r[L.EMITTER.REMOVED] = unit?.removed ? 1:0;
    const arsenal = source.arsenal;
    if (own) {
      const v = soldier.velocity; r[L.EMITTER.VX] = v?.x ?? 0; r[L.EMITTER.VY] = v?.y || 0; r[L.EMITTER.VZ] = v?.z ?? 0;
      r[L.EMITTER.HEALTH] = source.health ?? 0; r[L.EMITTER.MAX_HEALTH] = source.maxHealth || 1;
      r[L.EMITTER.MODE] = soldier.mode ?? 0; r[L.EMITTER.AIR] = AIR[soldier.air] || 0; r[L.EMITTER.POSTURE] = POSTURE[soldier.posture] || 0; r[L.EMITTER.DIVE] = soldier.dive ? 1:0; r[L.EMITTER.GROUNDED] = soldier.grounded === false ? 0:1;
      r[L.EMITTER.FOOT0] = NaN; r[L.EMITTER.FOOT1] = NaN; r[L.EMITTER.FOOT2] = NaN;
      const actor = this.audio.game.actor;
      if (meta.kind === L.EMITTER_KIND.DROIDEKA) { r[L.EMITTER.FOOT0] = actor?.byName?.bone_l_leg2?.matrixWorld?.elements[13] ?? NaN; r[L.EMITTER.FOOT1] = actor?.byName?.bone_r_leg2?.matrixWorld?.elements[13] ?? NaN; r[L.EMITTER.FOOT2] = actor?.byName?.bone_b_leg2?.matrixWorld?.elements[13] ?? NaN; }
      else { r[L.EMITTER.FOOT0] = actor?.byName?.bone_l_foot?.matrixWorld?.elements[13] ?? NaN; r[L.EMITTER.FOOT1] = actor?.byName?.bone_r_foot?.matrixWorld?.elements[13] ?? NaN; }
      if (meta.shieldDef !== soldier.shield?.def) { meta.shieldDef = soldier.shield?.def; meta.shieldClass = this.resources.class(meta.shieldDef?.class); }
      r[L.EMITTER.SHIELD] = soldier.shield?.on ? meta.shieldClass : -1;
      r[L.EMITTER.SELECTED0] = arsenal?.selected?.[0] ?? -1; r[L.EMITTER.SELECTED1] = arsenal?.selected?.[1] ?? -1;
      r[L.EMITTER.ZOOM] = source.zoom ?? 1; r[L.EMITTER.ZOOM_STEP] = source.zoomStep ?? 0; r[L.EMITTER.FIRST_PERSON] = source.firstPerson ? 1:0; r[L.EMITTER.MOUNTED] = source.mounted ? 1:0;
      const controls = this.audio.game.controls; r[L.EMITTER.FIRING_PRIMARY] = controls?.held("firePrimary") ? 1:0; r[L.EMITTER.FIRING_SECONDARY] = controls?.held("fireSecondary") ? 1:0;
    }
    this.emitters.publish(this,id);
    const primary = arsenal?.current(0), secondary = arsenal?.current(1), weapons = arsenal?.weapons || this.empty;
    for (let i=0;i<weapons.length;i++) this.captureWeapon(weapons[i],id,primary,secondary,i,own);
  }
  captureEntity(object,kind,name) {
    const id = this.entity(object,kind,name), r = this.emitters.row, m = this.emitters.metadata[id]; r.fill(0);
    const p = kind === L.EMITTER_KIND.HULL ? object.root?.position ?? object.position : object.position, v = object.velocity;
    r[L.EMITTER.KIND] = kind; r[L.EMITTER.CLASS] = m.class; r[L.EMITTER.X] = p.x; r[L.EMITTER.Y] = p.y; r[L.EMITTER.Z] = p.z;
    r[L.EMITTER.VX] = v?.x ?? 0; r[L.EMITTER.VY] = v?.y ?? 0; r[L.EMITTER.VZ] = v?.z ?? 0;
    r[L.EMITTER.ALIVE] = kind === L.EMITTER_KIND.DROID ? !object.dead : object.alive === false ? 0:1; r[L.EMITTER.REMOVED] = object.gone ? 1:0; r[L.EMITTER.HEALTH] = object.health ?? 0; r[L.EMITTER.MAX_HEALTH] = object.maxHealth ?? 1;
    r[L.EMITTER.LIFE] = object.life ?? Infinity; r[L.EMITTER.GENERATION] = object.aiTarget?.generation ?? 0; r[L.EMITTER.FLIGHT] = FLIGHT[object.flightState] || 0; r[L.EMITTER.FLYING] = object.flying ? 1:0;
    r[L.EMITTER.PILOTED] = kind === L.EMITTER_KIND.HULL && (object.held ? object.held(0) : object.occupied) ? 1:0;
    r[L.EMITTER.SPEED_LIMIT] = m.top ?? 0; r[L.EMITTER.COLLISION_DROP] = m.collision ?? 5; r[L.EMITTER.COUNTDOWN] = object.countdown ?? -1; r[L.EMITTER.SALVO] = object.salvo ? 1:0;
    r[L.EMITTER.WALK_PHASE] = object.shownWalkPhase ?? object.walkPhase ?? 0; r[L.EMITTER.WALK_BLEND] = object.shownWalkBlend ?? object.walkBlend ?? 0;
    if (kind === L.EMITTER_KIND.DROID) {
      const weapons = object.arsenal?.weapons || this.empty;
      for (let i=0;i<weapons.length;i++) this.captureWeapon(weapons[i],id,null,null,i,false);
    }
    this.emitters.publish(this,id); return id;
  }
  captureTurret(object,owner,cls,held,alive,yaw,pitch,yawTop,pitchTop,node,position) {
    const id = this.turrets.id(object), r = this.turrets.row, matrix = node?.matrixWorld?.elements;
    r.fill(0); r[L.TURRET.CLASS] = cls; r[L.TURRET.OWNER] = owner; r[L.TURRET.X] = matrix ? matrix[12] : position.x; r[L.TURRET.Y] = matrix ? matrix[13] : position.y; r[L.TURRET.Z] = matrix ? matrix[14] : position.z;
    r[L.TURRET.HELD] = held ? 1:0; r[L.TURRET.ALIVE] = alive ? 1:0; r[L.TURRET.YAW] = yaw; r[L.TURRET.PITCH] = pitch; r[L.TURRET.YAW_TOP] = yawTop; r[L.TURRET.PITCH_TOP] = pitchTop; r[L.TURRET.PRESENT] = 1; this.turrets.publish(this,id);
  }
  // Occupancy remains raw team/count data. A reused callback avoids a Map
  // iterator allocation, and Rust decides whether those counts charge,
  // dispute or discharge the post. Counts are sent only when they change.
  capturePosts(posts) {
    for (let i=0;i<posts.length;i++) {
      const post = posts[i], id = this.post(post), r = this.posts.row, m = this.posts.metadata[id]; r.set(m.sounds); r[L.POST.CLASS] = m.class; r[L.POST.X] = post.position.x; r[L.POST.Y] = post.position.y; r[L.POST.Z] = post.position.z;
      r[L.POST.ACTIVE] = post.active === false ? 0:1; r[L.POST.TEAM] = post.team || 0; r[L.POST.CONTESTED] = post.contested ? 1:0; r[L.POST.PROGRESS] = post.progress || 0; r[L.POST.NEUTRALIZE] = post.neutralize || 0; r[L.POST.BUILDING_ALIVE] = post.building && !post.building.alive ? 0:1; r[L.POST.PRESENT] = 1;
      m.scratch.fill(0); post.counts?.forEach(m.count);
      for (let team=0;team<16;team++) if (!Object.is(m.counts[team],m.scratch[team])) { m.counts[team] = m.scratch[team]; const c = this.countRow; c[L.COUNT.POST] = id; c[L.COUNT.TEAM] = team; c[L.COUNT.VALUE] = m.scratch[team]; this.packet(L.INPUT.COUNT,c); }
      this.posts.publish(this,id);
    }
  }
  captureItems(list,landingSource) {
    for (let i=0;i<list.length;i++) {
      const item = list[i], id = this.items.id(item), r = this.items.row; let meta = this.items.metadata[id];
      if (!meta) { this.items.metadata[id] = meta = this.items.records[id]; meta.class = this.resources.class(item.def?.class); meta.def = item.def; } r.fill(0);
      r[L.ITEM.CLASS] = meta.class; r[L.ITEM.X] = item.position.x; r[L.ITEM.Y] = item.position.y; r[L.ITEM.Z] = item.position.z; r[L.ITEM.MINE] = item.mine ? 1:0; r[L.ITEM.MINE_BASE] = meta.def?.base === "mine" ? 1:0; r[L.ITEM.LANDING_SOURCE] = landingSource;
      r[L.ITEM.LANDED] = item.landed ? 1:0; r[L.ITEM.GONE] = item.gone ? 1:0; r[L.ITEM.LIFE] = Number.isFinite(item.life) ? item.life : NaN; r[L.ITEM.SPAN] = meta.def?.LifeSpan ?? 0; r[L.ITEM.PRESENT] = 1; this.items.publish(this,id);
    }
  }
  // Presentation registries supply the foreign objects, including pooled
  // hulls and rounds that are created after the battle loads. Their native
  // histories use identity, never their current array index in a registry.
  // Matrix publication reads presentation results without updating a rig.
  // Dynamic collision flags accompany the frame so native room and source
  // rays see the same enabled bodies as other queries of the page world.
  capture(game) {
    const own = this.actor(game.combat,game.unit,game.combat?.owner), units = game.units?.all() || this.empty;
    for (let i=0;i<units.length;i++) { const unit = units[i]; if (!unit.combat?.arsenal || !unit.combat.soldier) continue; const id = this.actor(unit.combat,unit.className,unit); this.emitters.metadata[id].unitStamp = this.stamp; }
    for (let id=0;id<this.emitters.count;id++) {
      const m = this.emitters.metadata[id]; if (!m || m.kind !== L.EMITTER_KIND.SOLDIER && m.kind !== L.EMITTER_KIND.DROIDEKA) continue;
      if (id === own || m.unitStamp === this.stamp || m.eventStamp === this.stamp || this.follows[id]) this.captureActor(id,id === own);
    }
    const hulls = game.vehicles?.vehicles || this.empty;
    for (let i=0;i<hulls.length;i++) {
      const v = hulls[i], id = this.entity(v,L.EMITTER_KIND.HULL,v.name), meta = this.emitters.metadata[id];
      if (meta.top === undefined) { meta.top = Number(v.def?.settings?.MaxSpeed) || Number(v.def?.settings?.ForwardSpeed) || 0; meta.collision = Number(v.def?.settings?.CollisionThreshold) || 5; }
      this.captureEntity(v,L.EMITTER_KIND.HULL,v.name); const seats = v.seats || this.empty;
      for (let j=1;j<seats.length;j++) { const seat = seats[j], settings = seat.settings; this.captureTurret(seat,id,meta.class,v.held ? v.held(j) : seat.lease,v.alive,seat.aimYaw,seat.pitch,seat.view?.turnRate || Number(settings?.TurnRate) || 2,Number(settings?.PitchRate) || Number(v.def?.settings?.PitchRate) || 1,seat.view?.turret,v.root?.position ?? v.position); }
    }
    const turrets = game.live?.turrets || this.empty;
    for (let i=0;i<turrets.length;i++) { const t = turrets[i]; let meta = this.turrets.metadata[this.turrets.id(t)]; if (!meta) { const id = this.turrets.ids.get(t); this.turrets.metadata[id] = meta = this.turrets.records[id]; meta.class = this.resources.class(t.desc?.class); } this.captureTurret(t,-1,meta.class,t.lease,t.alive,t.yaw,t.pitch,t.turnRate || 2,t.turnRate || 2,t.drum,t.position); }
    const ordnance = game.ordnance;
    if (ordnance) { this.captureRounds(ordnance.bolts); this.captureRounds(ordnance.rounds); this.captureRounds(ordnance.grenades); }
    const buildings = game.live?.buildings || this.empty;
    for (let i=0;i<buildings.length;i++) { const b = buildings[i]; if (b.serve !== undefined) this.captureEntity(b,L.EMITTER_KIND.STATION,b.desc?.class); }
    const droids = game.remotes?.droids || this.empty;
    for (let i=0;i<droids.length;i++) this.captureEntity(droids[i],L.EMITTER_KIND.DROID,droids[i].def?.class);
    for (let id=0;id<this.feet.count;id++) {
      const meta = this.feet.metadata[id]; if (!meta) continue;
      const matrix = meta.node?.matrixWorld?.elements; if (!matrix) continue;
      const r = this.feet.row; r[L.FOOT.OWNER] = this.emitters.ids.get(meta.hull); r[L.FOOT.INDEX] = meta.index; r[L.FOOT.PRESENT] = 1;
      for (let j=0;j<16;j++) r[L.FOOT.M0+j] = matrix[j]; this.feet.publish(this,id);
    }
    this.capturePosts(game.posts?.posts || this.empty); this.captureItems(game.items?.items || this.empty,1); this.captureItems(game.items?.views || this.empty,0);
    this.emitters.missing(this); this.weapons.missing(this); this.posts.missing(this); this.items.missing(this); this.turrets.missing(this); this.feet.missing(this);
    const collision = game.collision;
    if (collision?.rays) { collision.rayBodies(); const bodies = collision.bodies; for (let i=0;i<bodies.length;i++) { const body = bodies[i], enabled = body.enabled ? 1:0, id = body.raySlot; if (this.bodyFlags[id] !== enabled) { this.bodyFlags[id] = enabled; this.body[L.BODY.ID] = id; this.body[L.BODY.ENABLED] = enabled; this.packet(L.INPUT.BODY,this.body); } } }
    return own;
  }
  // A round in flight is heard only by its class, place, removal and life
  // (SceneSounds.updateRounds). Bolts, shells and grenades are the busiest
  // sources, so they skip the hull, droid and station facts above.
  captureRounds(list) {
    if (!list) return; const r = this.emitters.row;
    for (let i=0;i<list.length;i++) {
      const round = list[i], id = this.entity(round,L.EMITTER_KIND.ROUND,round.def.class), p = round.position; r.fill(0);
      r[L.EMITTER.KIND] = L.EMITTER_KIND.ROUND; r[L.EMITTER.CLASS] = this.emitters.metadata[id].class; r[L.EMITTER.X] = p.x; r[L.EMITTER.Y] = p.y; r[L.EMITTER.Z] = p.z;
      r[L.EMITTER.ALIVE] = 1; r[L.EMITTER.REMOVED] = round.gone ? 1:0; r[L.EMITTER.LIFE] = round.life ?? Infinity;
      this.emitters.publish(this,id);
    }
  }
  // GameAudio.update reaches native control once per presented frame. The
  // packet carries clocks as separate facts: pause stops gameplay timers,
  // while source fades, segment deadlines and device playback keep running.
  // Memory views are recreated only after cold resource growth detaches them.
  // No async continuation can interrupt this synchronous capture/step/drain.
  update(dt) {
    if (this.memory !== this.x.memory.buffer) this.refresh(); this.at = 0; this.stamp++;
    const game = this.audio.game, own = this.capture(game), r = this.renderer, f = this.frame, m = game.camera.matrixWorld.elements;
    f.fill(0); f[L.FRAME.WALL_NOW] = this.offline ? this.engine.ctx.currentTime : performance.now()/1000; f[L.FRAME.CONFIGURATION] = r.configuration; f[L.FRAME.CHANNELS] = r.channels; f[L.FRAME.DT] = dt; f[L.FRAME.NOW] = this.engine.ctx.currentTime; f[L.FRAME.RATE] = this.engine.ctx.sampleRate; f[L.FRAME.ENHANCED] = r.enhanced ? 1:0; f[L.FRAME.LIGHT] = r.light ? 1:0; f[L.FRAME.HRTF] = r.hrtf ? 1:0; f[L.FRAME.BINAURAL] = r.binaural ? 1:0; f[L.FRAME.LFE] = r.lfe ? 1:0; f[L.FRAME.RUNNING] = this.offline || this.engine.ctx.state === "running" ? 1:0;
    f[L.FRAME.MAX_VOICES] = this.engine.maxVoices; f[L.FRAME.VIRTUAL_VOICES] = this.engine.virtualVoices; f[L.FRAME.PX] = m[12]; f[L.FRAME.PY] = m[13]; f[L.FRAME.PZ] = m[14]; f[L.FRAME.FX] = -m[8]; f[L.FRAME.FY] = -m[9]; f[L.FRAME.FZ] = -m[10]; f[L.FRAME.UX] = m[4]; f[L.FRAME.UY] = m[5]; f[L.FRAME.UZ] = m[6];
    f[L.FRAME.OWN] = own; f[L.FRAME.TEAM] = Number(game.team); f[L.FRAME.OVER] = game.battle?.over ? 1:0; f[L.FRAME.TICKETS1] = game.battle?.reinforcements?.[1] ?? 0; f[L.FRAME.TICKETS2] = game.battle?.reinforcements?.[2] ?? 0; f[L.FRAME.BLEED1] = game.battle?.bleed?.[1] ?? 0; f[L.FRAME.BLEED2] = game.battle?.bleed?.[2] ?? 0; f[L.FRAME.HUMAN] = game.battle?.human?.unit ? 1:0;
    const settings = this.audio.settings;
    f[L.FRAME.EFFECTS_VOLUME] = Number(settings?.get("effectsVolume") ?? 1); f[L.FRAME.AMBIENCE_VOLUME] = Number(settings?.get("ambienceVolume") ?? 1); f[L.FRAME.MUSIC_VOLUME] = Number(settings?.get("musicVolume") ?? 1); f[L.FRAME.VOICE_VOLUME] = Number(settings?.get("voiceVolume") ?? 1); f[L.FRAME.INTERFACE_VOLUME] = Number(settings?.get("interfaceVolume") ?? 1);
    f[L.FRAME.WATER_LEVEL] = game.water?.level ?? -Infinity; f[L.FRAME.WORLD] = game.collision?.rayWorld ?? -1; f[L.FRAME.COLLISION] = game.collision?.rays || game.soundPlanes?.length ? 1:0;
    this.packet(L.INPUT.FRAME,f);
    if (this.eventCount) { this.input[this.at+L.PACKET.KIND] = L.INPUT.EVENT; this.input[this.at+L.PACKET.COUNT] = this.eventCount; this.at += L.PACKET.STRIDE; const count = this.eventCount * L.EVENT.STRIDE; for (let i=0;i<count;i++) this.input[this.at+i] = this.events[i]; this.at += count; this.eventCount = 0; }
    this.x.sound_step(this.h,this.at); this.drain();
  }
  // The output host returns these transfer buffers after consuming their
  // used prefixes. Reusing them avoids allocating a new command array or
  // MessagePort envelope every frame. A stalled transport is an audio error,
  // rather than a reason to construct another buffer or another controller.
  // Cached room facts are retained for a later device/layout reconfigure;
  // this copy performs no room classification or parameter calculation.
  drain() {
    const i = this.info, stats = this.engine.stats; stats.real = i[L.INFO.REAL]; stats.virtual = i[L.INFO.VIRTUAL]; stats.started = i[L.INFO.STARTED]; stats.stolen = i[L.INFO.STOLEN]; stats.dropped = i[L.INFO.DROPPED]; stats.culled = i[L.INFO.CULLED];
    const commands = i[L.INFO.COMMANDS], placements = i[L.INFO.PLACEMENTS], mask = i[L.INFO.ROOM_CHANGED];
    for (let n=0;n<ROOM_KEYS.length;n++) if (mask & (1 << n)) this.room[ROOM_KEYS[n]] = i[L.INFO.ROOM+n];
    if (commands || placements || i[L.INFO.ROOM_CHANGED]) {
      const data = commands + placements <= SMALL_PAYLOAD ? this.smallBuffers.pop() : this.buffers.pop(); if (!data) throw Error("audio: control transport stalled");
      for (let n=0;n<L.INFO.STRIDE;n++) data[n] = i[n]; for (let n=0;n<commands;n++) data[L.INFO.STRIDE+n] = this.commands[n]; for (let n=0;n<placements;n++) data[L.INFO.STRIDE+commands+n] = this.placements[n];
      this.message.data = data; this.transfer[0] = data.buffer; this.renderer.mixer.port.postMessage(this.message,this.transfer);
    }
    const count = i[L.INFO.REQUESTS];
    for (let n=0;n<count;n++) { const at = n * L.REQUEST.STRIDE, voice = this.requests[at+L.REQUEST.VOICE], generation = this.requests[at+L.REQUEST.GENERATION], sample = this.requests[at+L.REQUEST.SAMPLE];
      const operation = this.requests[at+L.REQUEST.OPERATION], segment = this.requests[at+L.REQUEST.SEGMENT];
      if (operation === L.REQUEST_KIND.RELEASE) this.release(voice,generation,sample);
      else if (operation === L.REQUEST_KIND.EVICT) this.audio.streams.evict(segment);
      else this.load(voice,generation,segment,this.requests[at+L.REQUEST.PART],sample,this.requests[at+L.REQUEST.VARIANT]);
    }
  }
  release(voice,generation,sample) {
    for (let p=0;p<3;p++) { const record = this.assets[voice*3+p]; if (record.generation === generation) { record.generation = 0; record.asset?.close(); record.asset = null; } this.releaseMessage.ids[p] = sample+p; }
    this.renderer.mixer.port.postMessage(this.releaseMessage);
  }
  // Only native requests initiate an asset load. A generation belongs to
  // one stream lifetime, not to a reusable physical part/sample number.
  // Late completions close their producer and cannot revive a stopped line.
  // Stream workers prime their bounded PCM window before readiness is sent;
  // a metadata-only completion would start a source before its PCM exists.
  async load(voice,generation,segment,part,sample,variant) {
    const record = this.assets[voice*3+part], descriptor = this.resources.segments[segment]; record.asset?.close(); record.asset = null; record.generation = generation;
    let asset;
    try {
      asset = await this.audio.streams.segment(descriptor.index,descriptor.key,segment,variant < 0 ? null : this.resources.variants[variant]);
      if (record.generation !== generation || this.disposed) { asset?.close?.(); return; }
      if (!asset) { this.ready(voice,generation,part,sample,0,0,1); return; }
      if (asset.attach) {
        record.asset = asset;
        await attachAudioStream(asset,this.renderer.mixer,sample,this.renderer.streamWaits,(error)=>this.renderer.fail(error));
        if (record.generation !== generation || this.disposed) { asset.close(); return; }
      } else this.renderer.uploadSample(asset,sample);
      if (!this.ready(voice,generation,part,sample,asset.duration,asset.numberOfChannels,0)) asset.close?.();
    } catch (error) { if (record.generation === generation && !this.disposed && this.ready(voice,generation,part,sample,0,0,2)) this.audio.reportError(error); }
  }
  // Ports StreamVoice.next's I/O continuation. Readiness uses current device
  // time, independently of the next presented frame, and cannot change the
  // listener, replay gameplay events or advance a battle sound timer.
  ready(voice,generation,part,sample,duration,channels,failed) {
    if (this.memory !== this.x.memory.buffer) this.refresh(); const r = this.input; r[L.READY.VOICE] = voice; r[L.READY.GENERATION] = generation; r[L.READY.PART] = part; r[L.READY.SAMPLE] = sample; r[L.READY.DURATION] = duration; r[L.READY.CHANNELS] = channels; r[L.READY.TIME] = this.engine.ctx.currentTime; r[L.READY.WALL_NOW] = this.offline ? this.engine.ctx.currentTime : performance.now()/1000; r[L.READY.FAILED] = failed;
    this.x.sound_ready(this.h); const accepted = this.info[L.INFO.ACCEPTED] !== 0; this.drain(); return accepted;
  }
  // The panel asks for diagnostic rows on its own slow UI timer. Game
  // frames never construct names, objects or lists for this optional view.
  diagnostic() {
    const at = this.x.sound_debug(this.h), count = this.info[L.INFO.DEBUG_COUNT], rows = new Float64Array(this.x.memory.buffer,at,count*L.DEBUG_VOICE.STRIDE), result = [];
    for (let i=0;i<count;i++) { const o = i*L.DEBUG_VOICE.STRIDE; result.push({name:this.resources.sounds[rows[o+L.DEBUG_VOICE.SOUND]]?.Name,category:CATEGORY[rows[o+L.DEBUG_VOICE.CATEGORY]],mode3D:rows[o+L.DEBUG_VOICE.MODE3D],distance:rows[o+L.DEBUG_VOICE.DISTANCE],audible:rows[o+L.DEBUG_VOICE.AUDIBLE],score:rows[o+L.DEBUG_VOICE.SCORE],real:rows[o+L.DEBUG_VOICE.REAL],occluded:rows[o+L.DEBUG_VOICE.OCCLUDED]}); }
    return result;
  }
  dispose() { if (this.disposed) return; this.disposed = true; if (this.engine.enhanced?.onSample === this.sampleCallback) this.engine.enhanced.onSample = null; for (let i=0;i<this.assets.length;i++) { this.assets[i].generation = 0; this.assets[i].asset?.close(); } this.x.sound_free(this.h); }
}
