// Host binding for the Rust battle HUD. The page publishes game facts, asset
// handles and ordered scalar commands. Rust owns display clocks and geometry.
import { HudArt, HudGlyph as G, HudFrame as H, HudWeapon as W, HudPost as P, HudPostCount as PC, HudUnit as U, HudState as S, HudTeam as T, HudCommand as C, HudMapDraw as M, HudRing as R } from './hud-layout.js';
import { hasMissileLock } from './weapon-targeting.js';
import { unitDefinition } from './targeting-facts.js';
import * as THREE from 'three';

export const MAX_POSTS = 128, MAX_UNITS = 2048, MAX_TEAMS = 32;
export const WEAPONS = H.words, POSTS = WEAPONS + 4 * W.words, POST_COUNTS = POSTS + MAX_POSTS * P.words, UNITS = POST_COUNTS + MAX_POSTS * MAX_TEAMS * PC.words;
export const TEAMS = UNITS + MAX_UNITS * U.words, STATES = TEAMS + MAX_TEAMS * T.words, MAP_DRAW = STATES + (C.kill + 1) * S.words, RINGS = MAP_DRAW + M.words;
const WORDS = RINGS + 128 * R.words;
const ART = Object.keys(HudArt).slice(1, -1);
const FONT_NAMES = ['gamefont_tiny', 'gamefont_small', 'gamefont_medium', 'gamefont_large'];
const EMPTY = Object.freeze([]);
export const relationCode = value => value === 'friendly' || value > 0 ? 1 : value === 'enemy' || value < 0 ? 2 : value === 'locals' ? 3 : value == null ? 4 : 0;

export class HudInput {
  // The staging block is independent of WASM memory. Loading a font page or
  // registering a text handle can grow that memory while setters retain state.
  // Binding the final shared view happens at publication.
  // The same staging allocation is retained for the lifetime of the battle.
  constructor(hud, fonts) {
    this.hud = hud;
    this.core = null; this.fontInput = fonts; this.textValues = [null]; this.attached = false;
    this.f = new Float64Array(WORDS);
    this.assets = [null]; this.assetIds = new Map(); this.textureIds = new Map();
    this.postDefs = new WeakMap(); this.texts = new Map(); this.defs = new WeakMap(); this.weaponIds = new WeakMap(); this.unitIds = new WeakMap();
    this.nextWeapon = 1; this.nextGeneration = 1; this.maxId = 0; this.observedStates = 0;
    this.owners = new Array(MAX_UNITS + 1); this.free = new Uint32Array(MAX_UNITS); this.freeCount = MAX_UNITS;
    this.used = new Uint32Array(MAX_UNITS + 1);
    for (let i = 0; i < MAX_UNITS; i++) this.free[i] = MAX_UNITS - i;
    this.rows = new Array(MAX_UNITS); this.rowCount = 0; this.seen = new Uint32Array(MAX_UNITS + 1); this.stamp = 0;
    this.forward = new THREE.Vector3();
    for (const name of ART) this.asset(name);
    this.texture(hud.solid);
    this.f[H.victory] = this.text(hud.strings['hud.victory_in'] || '');
    this.f[H.defeat] = this.text(hud.strings['hud.defeat_in'] || '');
    this.f[H.leavingText] = this.text(hud.strings['game.tooltips.leaving_battlefield'] || 'LEAVING BATTLEFIELD');
    this.f[H.killTemplates] = this.text(hud.strings['game.infomsg.died'] || '%s DIED', true);
    this.f[H.killTemplates + 1] = this.text(hud.strings['game.infomsg.suicide'] || '%s KILLED HIMSELF', true);
    this.f[H.killTemplates + 2] = this.text(hud.strings['game.infomsg.teamkill'] || '%s KILLED TEAMMATE %s', true);
    this.f[H.killTemplates + 3] = this.text(hud.strings['game.infomsg.killed'] || '%s KILLED %s', true);
    this.f[H.countdown] = NaN;
    // Game construction awaits graphics.rustReady before creating BattleHud.
    // Attach cold resources now so real events never wait for a drawn frame.
    this.attach();
  }

  // Game creation awaits the renderer before BattleHud construction.
  // Attach at load, preserve the locally assigned
  // text IDs, and upload cold font rows before taking a frame memory view.
  attach() {
    if (this.attached) return;
    this.core = this.hud.graphics.rust;
    if (!this.core) throw new Error('Rust HUD renderer is not ready');
    if (this.core.rust.hud_model_input_len() !== WORDS) throw new Error('HUD input layout mismatch');
    this.core.addView('hudModelInput', Float64Array, () => this.core.rust.hud_model_input_ptr(), () => this.core.rust.hud_model_input_len());
    for (let index = 0; index < FONT_NAMES.length; index++) {
      const font = this.fontInput?.[FONT_NAMES[index]];
      if (!font) continue;
      const rows = new Float64Array(font.glyphs.length * G.words);
      let at = 0;
      for (const row of font.glyphs) {
        if (row.length !== G.words) throw new Error('HUD glyph layout mismatch');
        for (let k = 0; k < G.words; k++) rows[at + k] = row[k];
        rows[at + G.page] = this.asset(font.pages[row[G.page]]);
        at += G.words;
      }
      this.core.rust.hud_model_font(index, font.lineHeight, rows);
    }
    for (let id = 1; id < this.textValues.length; id++) {
      if (this.core.rust.hud_model_text(this.textValues[id], false) !== id) throw new Error('HUD text handle mismatch');
    }
    this.core.views();
    this.core.hudModelInput.set(this.f);
    this.attached = true;
  }

  // Text and class facts are registered on first observation, then retained by
  // numeric handles. Frame rows never construct glyph objects or key strings.
  text(value, uppercase = false) {
    if (!value) return 0;
    const text = String(value), key = uppercase ? text.toUpperCase() : text;
    let id = this.texts.get(key);
    if (id === undefined) { id = this.textValues.length; this.textValues.push(key); this.texts.set(key, id); if (this.attached && this.core.rust.hud_model_text(key, false) !== id) throw new Error('HUD text handle mismatch'); }
    return id;
  }
  // Reserve absent art too: fixed battle-art slots must keep their identities
  // when a map has no URL for one of them. Zero remains the explicit no-art
  // handle for optional weapon, scope and objective descriptors.
  asset(name) {
    if (!name) return 0;
    let id = this.assetIds.get(name);
    if (id === undefined) { id = this.texture(this.hud.texture(name), true); this.assetIds.set(name, id); }
    return id;
  }
  // Retain texture identity across raster atlas upgrades. The descriptor stays
  // the same when immutable GPU storage is replaced, so model rows never need
  // to be rebuilt solely because an atlas acquired larger pixels.
  texture(texture, unique = false) {
    if (!texture && !unique) return 0;
    let id = !unique && this.textureIds.get(texture);
    if (!id) {
      id = this.assets.length;
      // Missing art clips just as the old batch skipped unavailable textures.
      if (id >= 4096) return 0;
      this.assets.push({ texture, version: -1, handle: -2, width: -1, height: -1, ready: false });
      if (texture) this.textureIds.set(texture, id);
    }
    return id;
  }
  // Definition objects are cold authored policy, not changing weapon state.
  // Resolve their art and label once; frame serialization reads those numeric
  // handles instead of allocating labels or looking up glyph pages per weapon.
  definition(def) {
    let row = this.defs.get(def);
    if (!row) {
      row = { icon: this.asset(def.IconTexture), label: this.text(def.label || ''), reticle: this.asset(def.ReticuleTexture), map: this.asset(def.MapTexture), health: this.asset(def.HealthTexture), lock: hasMissileLock(def), hasMap: def.MapTexture != null, scale: def.MapScale == null ? NaN : Number(def.MapScale), scan: Number(def.ScanningRange) || 0, transmit: Number(def.TransmitRange) || 0 };
      this.defs.set(def, row);
    }
    return row;
  }
  // Names fade when the weapon object changes, even between equal definitions.
  // A stable numeric identity preserves that rule without exposing page objects
  // to the native model or restarting a timer on every serialized row.
  weaponId(weapon) {
    if (!weapon) return 0;
    let id = this.weaponIds.get(weapon);
    if (!id) { id = this.nextWeapon++; this.weaponIds.set(weapon, id); }
    return id;
  }
  // Radar slots name current facades, not every entity ever created. A slot
  // absent for a complete update is reusable; a lease generation prevents its
  // previous owner's discovery and shot memory leaking into the next occupant.
  unitId(unit) {
    if (!unit) return 0;
    let row = this.unitIds.get(unit);
    if (!row) {
      row = { unit, id: 0, generation: 0, sourceGeneration: NaN, source: null, definition: null };
      this.unitIds.set(unit, row);
    }
    if (!row.id || this.owners[row.id] !== unit) {
      if (!this.freeCount) return 0;
      row.id = this.free[--this.freeCount]; this.owners[row.id] = unit; this.maxId = Math.max(this.maxId, row.id);
      row.generation = this.nextGeneration++;
    }
    if (row.sourceGeneration !== (unit.generation ?? 0)) {
      row.sourceGeneration = unit.generation ?? 0; row.generation = this.nextGeneration++;
    }
    const source = unit.classDef || unit.def || null;
    if (!row.definition || row.source !== source) {
      row.source = source; row.definition = this.definition(unitDefinition(unit));
    }
    this.used[row.id] = this.stamp;
    return row.id;
  }
  unitGeneration(unit) { return this.unitIds.get(unit)?.generation || 0; }
  // State setters overwrite retained scalar rows. Thousands of preview calls
  // during loading still publish one value per property, with no command queue.
  state(kind, a = 0, b = 0, c = 0, d = 0) {
    const o = STATES + kind * S.words, f = this.f;
    // Health/hull losses and stat direction are real transition feedback: a
    // preview can change twice before the same final value is drawn. Observe
    // changed samples in Rust without queuing them; bars still publish only
    // their final state. Secondary bars need only their latest fill.
    if ((kind === C.health || kind === C.hull || kind === C.stats) &&
        (!(this.observedStates & (1 << kind)) || f[o] !== a || f[o + 1] !== b || f[o + 2] !== c || f[o + 3] !== d)) {
      this.core.rust.hud_model_observe(kind, a, b, c, d);
      this.observedStates |= 1 << kind;
    }
    f[o + S.values] = a; f[o + S.values + 1] = b; f[o + S.values + 2] = c; f[o + S.values + 3] = d;
    f[H.states] |= 1 << kind;
  }
  // Real events enter the native model immediately at its current visual time.
  // Rust retains the five-line feed, overlapping hits and shot windows, so a
  // stalled render or a long load cannot accumulate an unbounded host backlog.
  event(kind, a = 0, b = 0, c = 0, d = 0) {
    this.core.rust.hud_model_event(kind, a, b, c, d);
  }

  // Register sampler handles before acquiring WASM views. A late font page or
  // atlas upgrade can grow memory; the staging block remains valid throughout.
  samplers() {
    const core = this.core;
    for (let id = 1; id < this.assets.length; id++) {
      const asset = this.assets[id], texture = asset.texture;
      const image = texture?.image;
      let handle = -1;
      if (texture) {
        const properties = core.renderer.properties.get(texture);
        if (texture.version > 0 && properties.__version !== texture.version && image) core.renderer.initTexture(texture);
        handle = core.texture(texture);
      }
      const width = texture?.userData.nativeWidth || image?.width || 0;
      const height = texture?.userData.nativeHeight || image?.height || 0;
      const ready = Boolean(image);
      if (asset.handle !== handle || asset.width !== width || asset.height !== height || asset.ready !== ready) {
        core.rust.hud_model_asset(id, handle, width, height, ready);
        asset.handle = handle; asset.width = width; asset.height = height; asset.ready = ready;
      }
    }
  }

  // Two rows describe panel weapons and two describe reticle weapons. These
  // can differ during mounted/remote previews. Copy current scalar state only;
  // Rust chooses progress priority, ammo layout, reticle dimming and geometry.
  weapon(weapon, row) {
    const f = this.f, o = WEAPONS + row * W.words;
    if (!weapon) { f.fill(0, o, o + W.words); return; }
    const def = weapon.def, asset = this.definition(def);
    f[o + W.id] = this.weaponId(weapon); f[o + W.icon] = asset.icon; f[o + W.label] = asset.label; f[o + W.reticle] = asset.reticle;
    f[o + W.lock] = asset.lock; f[o + W.lockTarget] = weapon.lockTargetId ?? -1; f[o + W.lockElapsed] = weapon.lockElapsed || 0; f[o + W.lockTime] = def.LockTime || 0;
    f[o + W.charging] = Boolean(weapon.charging); f[o + W.held] = weapon.held || 0; f[o + W.maxPressed] = weapon.levels?.[0].MaxPressedTime || 0;
    f[o + W.reloading] = Boolean(weapon.reloading); f[o + W.reloadLeft] = weapon.reloadLeft || 0; f[o + W.reloadTime] = def.ReloadTime || 0;
    f[o + W.bottomless] = Boolean(weapon.bottomless); f[o + W.heatPerShot] = def.HeatPerShot || 0; f[o + W.heat] = weapon.heat || 0; f[o + W.overheated] = Boolean(weapon.overheated);
    f[o + W.loaded] = weapon.loaded; f[o + W.spare] = weapon.spare; f[o + W.reserve] = weapon.reserve; f[o + W.roundsPerClip] = def.RoundsPerClip || 0;
    f[o + W.wait] = weapon.wait || 0; f[o + W.shotDelay] = def.ShotDelay || 0; f[o + W.lockOffAngle] = def.LockOffAngle || 0;
    // Spread facts are the authored stance minima used with zero stick and
    // standing posture. Weapon.spread's native prediction owner remains shared.
    f[o + W.kick] = weapon.kick || 0; f[o + W.stillSpread] = def.StandStillSpread ?? 0;
    f[o + W.yawSpread] = weapon.yawSpreadOverride ?? def.YawSpread ?? 0; f[o + W.pitchSpread] = def.PitchSpread || 0;
  }

  // Keep all objectives, including inactive/unmapped ones, as raw game facts.
  // Rust chooses map eligibility, capture fills and blink colours. The page's
  // posts.at query supplies only which objective currently contains the player;
  // it does not compute a HUD bar or colour transition.
  posts(team) {
    const hud = this.hud, f = this.f, posts = hud.posts?.posts || EMPTY;
    const capture = hud.player && !hud.mount ? hud.posts?.at(hud.player.position) : null;
    f[H.capturePost] = -1;
    let count = 0;
    for (const post of posts) {
      if (count >= MAX_POSTS) break;
      const index = count++, o = POSTS + index * P.words;
      if (post === capture) f[H.capturePost] = index;
      f[o + P.x] = post.position.x; f[o + P.z] = post.position.z;
      f[o + P.mapped] = post.mapped !== false; f[o + P.active] = post.active !== false;
      f[o + P.team] = post.team || 0; f[o + P.relation] = relationCode(hud.relationOf(post.team, team));
      f[o + P.neutralize] = post.neutralize || 0; f[o + P.progress] = post.progress || 0; f[o + P.toward] = post.toward || 0;
      f[o + P.towardRelation] = relationCode(hud.relationOf(post.toward, team)); f[o + P.contested] = Boolean(post.contested);
      // Preserve Map insertion order as scalar rows. The original selects the
      // first other team, and distinguishes any present key for neutralization
      // from a positive count for a contested neutral capture.
      const start = POST_COUNTS + index * MAX_TEAMS * PC.words;
      f[o + P.counts] = start;
      let n = 0;
      for (const t of post.counts.keys()) {
        if (n >= MAX_TEAMS) break;
        const row = start + n++ * PC.words;
        f[row + PC.team] = t; f[row + PC.count] = post.counts.get(t); f[row + PC.relation] = relationCode(hud.relationOf(t, team));
      }
      f[o + P.countsLength] = n;
      let cached = this.postDefs.get(post);
      if (!cached) { cached = { texture: this.asset(post.desc?.mapTexture), label: this.text(post.label, true) }; this.postDefs.set(post, cached); }
      f[o + P.texture] = cached.texture; f[o + P.scale] = post.desc?.mapScale ?? 2.5; f[o + P.label] = cached.label;
    }
    f[H.posts] = count;
  }
  // The page selects the current presentation pose, owned by the actor/vehicle
  // presentation path. Rust turns hull quaternions into headings and retains a
  // previous heading at a vertical nose. A foreign forward reader only supplies
  // a direction for game objects that have no scalar yaw or hull quaternion.
  heading(unit, o) {
    const f = this.f, vehicle = unit.vehicle;
    f[o + U.yaw] = unit.presentation?.state?.aimYaw ?? unit.soldier?.aimYaw ?? vehicle?.yaw ?? unit.yaw ?? unit.turret?.yaw ?? NaN;
    const q = vehicle?.root?.quaternion ?? (vehicle?.flying ? vehicle.quaternion : null);
    f[o + U.hasQuaternion] = Boolean(q);
    if (q) { f[o + U.quaternion] = q.x; f[o + U.quaternion + 1] = q.y; f[o + U.quaternion + 2] = q.z; f[o + U.quaternion + 3] = q.w; f[o + U.forward] = 1; return; }
    else if (!vehicle && !unit.soldier && typeof unit.forward === 'function') unit.forward(this.forward.set(0, 0, 0));
    else { f[o + U.forward] = 0; return; }
    f[o + U.forward] = 1; f[o + U.forwardX] = this.forward.x; f[o + U.forwardZ] = this.forward.z;
  }
  // Update samples raw simulation facts and freezes their row identities for
  // radar eligibility. Drawing refreshes only the presentation position/heading
  // facts on those same rows. A new visual pose cannot cause a second sensor
  // query or add a marker before the next HUD update.
  units(update) {
    const hud = this.hud, f = this.f, context = hud.unitContext, view = hud.layer?.battlefield;
    const viewer = view ? null : context.viewer, team = view?.team ?? context.viewerTeam ?? viewer?.team;
    const controllable = view ? null : context.controllable ?? viewer;
    if (update) {
      for (let id = 1; id <= this.maxId; id++) {
        if (this.owners[id] && (this.used[id] !== this.stamp || this.owners[id].removed)) {
          this.owners[id] = null; this.free[this.freeCount++] = id;
        }
      }
      this.rowCount = 0; this.stamp = (this.stamp + 1) >>> 0; if (!this.stamp) { this.seen.fill(0); this.stamp = 1; }
      this.teams(team);
      const units = hud.unitSource?.all ? hud.unitSource.all() : hud.unitSource || EMPTY;
      for (const unit of units) this.addUnit(unit);
      const objects = context.objects?.all ? context.objects.all() : context.objects || EMPTY;
      for (const unit of objects) this.addUnit(unit);
    }
    for (let i = 0; i < this.rowCount; i++) {
      const record = this.rows[i], unit = record.unit, o = UNITS + i * U.words;
      // Removed, seated, dead and hero are independent game facts. Their HUD
      // meaning, soldier defaults, discovery and living counts belong to Rust;
      // this binding does not keep a second visibility model.
      if (update) {
        const def = record.definition;
        const position = unit.position;
        f[o + U.id] = record.id; f[o + U.generation] = record.generation;
        f[o + U.x] = position?.x || 0; f[o + U.y] = position?.y || 0; f[o + U.z] = position?.z || 0;
        f[o + U.removed] = Boolean(unit.removed); f[o + U.seated] = Boolean(unit.seat); f[o + U.dead] = Boolean(unit.dead);
        f[o + U.hasPosition] = Boolean(position); f[o + U.kind] = unit.kind ?? NaN; f[o + U.soldier] = Boolean(unit.soldier); f[o + U.hero] = Boolean(unit.hero);
        f[o + U.alive] = Boolean(unit.alive); f[o + U.team] = unit.team || 0;
        f[o + U.texture] = def.map; f[o + U.hasMapTexture] = def.hasMap;
        f[o + U.scale] = def.scale; f[o + U.scan] = def.scan; f[o + U.transmit] = def.transmit;
      }
      const position = unit.vehicle?.root?.position ?? unit.vehicle?.visual ?? unit.presentation?.position ?? unit.position ?? unit.vehicle?.position;
      f[o + U.visualX] = position?.x || 0; f[o + U.visualZ] = position?.z || 0;
      this.heading(unit, o);
    }
    f[H.units] = this.rowCount; f[H.viewer] = this.unitId(viewer); f[H.controllable] = this.unitId(controllable);
    f[H.hasViewer] = Boolean(viewer); f[H.easy] = Boolean(context.easy); f[H.viewerTeam] = team || 0;
    f[H.hasControl] = Boolean(controllable?.position);
    f[H.controlX] = controllable?.position?.x || 0; f[H.controlY] = controllable?.position?.y || 0; f[H.controlZ] = controllable?.position?.z || 0;
    f[H.controlScan] = controllable && f[H.controllable] ? this.unitIds.get(controllable).definition.scan : 0;
  }
  // The registry and optional object list can name the same facade. Numeric
  // stamps deduplicate them in source order without allocating a Set or scanning
  // the entire growing list for every row.
  addUnit(unit) {
    const id = this.unitId(unit);
    if (!id || this.seen[id] === this.stamp) return;
    this.seen[id] = this.stamp;
    if (this.rowCount >= MAX_UNITS) return;
    this.rows[this.rowCount++] = this.unitIds.get(unit);
  }

  // Team facts are published once, rather than asking four callbacks for
  // every body. The game supplies its current relation rows and count tables;
  // native radar owns living counts when the game has no override table.
  teams(viewer) {
    const f = this.f, context = this.hud.unitContext;
    for (let team = 0; team < MAX_TEAMS; team++) {
      const o = TEAMS + team * T.words;
      f[o + T.relation] = context.relations?.[viewer * MAX_TEAMS + team] ?? (!viewer || !team ? 0 : viewer === team ? 1 : -1);
      f[o + T.locals] = context.locals?.[team] ?? team > 2;
      f[o + T.tickets] = context.tickets?.[team] ?? Infinity;
      f[o + T.living] = context.living?.[team] ?? NaN;
    }
  }

  // Update does not draw panels. Its input phase needs radar and prompt facts
  // only; draw-time publication supplies current camera, weapons, objectives and
  // layer facts after actor poses and combat previews have completed.
  facts(update = false) {
    const hud = this.hud, f = this.f, player = hud.player, battle = hud.battle, view = hud.layer?.teams;
    if (update) { this.units(true); this.prompt(); return; }
    f[H.height] = hud.height; f[H.covered] = hud.covered; f[H.layer] = Boolean(hud.layer); f[H.dead] = Boolean(player?.dead);
    f[H.arsenal] = Boolean(hud.arsenal); f[H.vehicle] = Boolean(hud.vehicle); f[H.mount] = Boolean(hud.mount);
    f[H.player] = Boolean(player); f[H.playerX] = player?.position.x || 0; f[H.playerZ] = player?.position.z || 0; f[H.playerYaw] = player?.yaw || 0; f[H.speed] = player?.speed || 0;
    f[H.map] = Boolean(hud.frame); f[H.mapX] = hud.frame?.center[0] || 0; f[H.mapZ] = hud.frame?.center[1] || 0; f[H.mapHalf] = hud.frame?.half || 1; f[H.mapScale] = hud.mapScale; f[H.mapImage] = this.texture(hud.mapImage);
    f[H.battle] = Boolean(battle);
    const own = view?.own ?? battle?.own, foe = view?.foe ?? battle?.foe;
    f[H.own] = battle?.reinforcements[own] ?? 0; f[H.ownStart] = battle?.start?.[own] || 0; f[H.ownBleed] = battle?.bleed?.[own] || 0;
    f[H.foe] = battle?.reinforcements[foe] ?? 0; f[H.foeStart] = battle?.start?.[foe] || 0; f[H.foeBleed] = battle?.bleed?.[foe] || 0;
    f[H.countdown] = battle?.countdown ?? NaN; f[H.losing] = Boolean(battle?.losing); f[H.gameOver] = Boolean(battle?.gameOver);
    f[H.viewTeams] = Boolean(view); f[H.hideTeams] = view === false; f[H.flip] = view?.flip || 0;
    f[H.spawnMap] = Boolean(hud.layer?.battlefield) && !Object.hasOwn(hud, 'drawSpawnMap');
    // Reticle anchors are projections supplied by the current camera owner.
    // Lock progress is simulation state; Rust uses it only to draw the diamond.
    // Channel identities let Rust deduplicate a shared weapon's reticle.
    const reticle = hud.reticle, weapons = reticle?.weapons;
    f[H.reticle] = Boolean(reticle); f[H.reticleX] = reticle?.x ?? .5; f[H.reticleY] = reticle?.y ?? .5;
    const marker = hud.lockMarker; f[H.marker] = Boolean(marker); f[H.markerX] = marker?.x || 0; f[H.markerY] = marker?.y || 0; f[H.markerProgress] = marker?.progress || 0;
    f[H.lockChannel] = weapons ? Math.max(0, weapons.indexOf(reticle.weapon)) : 0;
    f[H.fov] = hud.aimCamera?.fov ?? 46.826; f[H.aspect] = hud.aimCamera?.aspect ?? 4 / 3;
    this.weapon(hud.arsenal?.current(0), 0); this.weapon(hud.arsenal?.current(1), 1);
    this.weapon(weapons ? weapons[0] : reticle?.weapon, 2); this.weapon(weapons?.[1], 3);
    const seats = hud.vehicle?.seats;
    for (let i = 0; i < 5; i++) { const seat = seats?.[i]; const lease = seat?.lease; f[H.seats + i] = !seat ? -1 : !lease ? 0 : lease.unit && lease.unit === hud.unitContext.viewer ? 1 : lease.unit?.character ? 2 : 3; }
    f[H.playerTeam] = player?.team || 0;
    this.posts(hud.layer?.battlefield?.team); this.units(update);
    this.prompt();
  }
  // Flight state and refusal codes are replicated game facts. Rust picks the
  // prompt and runs its age/fade clock. The host resolves localized text and a
  // changed key binding once, before those numeric text handles are published.
  prompt() {
    const hud = this.hud, v = hud.vehicle, f = this.f;
    // Prompt strings change only when a control binding or locale changes.
    const key = hud.keyName?.('takeoff') || '';
    if (this.promptKey !== key) {
      this.promptKey = key;
      const takeoff = hud.strings['game.tooltips.flyertakeoff'] || 'TO TAKE OFF';
      f[H.promptTexts] = this.text(key ? `${key} ${takeoff}` : takeoff);
      f[H.promptTexts + 1] = this.text(key ? `${key} TO LAND` : 'TO LAND');
      f[H.promptTexts + 2] = this.text('TAKEOFF PATH BLOCKED');
      f[H.promptTexts + 3] = this.text('LANDING PATH BLOCKED');
      f[H.promptTexts + 4] = this.text('TOO FAST TO LAND');
      f[H.promptTexts + 5] = this.text('NO LANDING ZONE');
      f[H.promptTexts + 6] = this.text('FLIGHT CEILING');
    }
    f[H.flightPilot] = Boolean(v?.flying && v.alive && v.seatIndex === 0);
    f[H.flightWarning] = FLIGHT_WARNINGS[v?.flightWarning] || 0;
    f[H.flightState] = v?.flightState === 'Landed' ? 1 : v?.flightState === 'Flying' ? 2 : 0;
    f[H.flightCanLand] = v?.landingReason === 'Press SPACE to land';
    const name = v?.seats?.[v.seatIndex]?.settings?.da1b8c0c;
    if (name !== this.seatKey) { this.seatKey = name; f[H.seatName] = this.text(name ? hud.strings[name] || SEAT_NAMES[name] || '' : ''); }
  }
  // Only active regions are inspected. Comparing before storing avoids
  // republishing unchanged scalars into native state. Dirty state bits identify
  // the latest setter values. No subarray or copied frame
  // object is constructed on this boundary.
  publish() {
    this.attach();
    const core = this.core, f = this.f;
    this.samplers(); core.views();
    const target = core.hudModelInput;
    this.copy(target, 0, POSTS + f[H.posts] * P.words);
    for (let i = 0; i < f[H.posts]; i++) {
      const start = POST_COUNTS + i * MAX_TEAMS * PC.words;
      this.copy(target, start, start + f[POSTS + i * P.words + P.countsLength] * PC.words);
    }
    this.copy(target, UNITS, UNITS + f[H.units] * U.words);
    this.copy(target, TEAMS, MAP_DRAW);
    this.copy(target, MAP_DRAW, RINGS + Math.min(128, f[MAP_DRAW + M.rings]) * R.words);
    f[H.states] = 0;
  }
  // Object.is preserves both NaN sentinels and signed zero. A NaN countdown
  // means no timer; rewriting it every frame would make an unchanged block
  // look dirty even though the model has no countdown to display.
  copy(target, start, end) { for (let i = start; i < end; i++) if (!Object.is(target[i], this.f[i])) target[i] = this.f[i]; }
}
const SEAT_NAMES = { 'common.vehiclepositions.pilot': 'PILOT', 'common.vehiclepositions.copilot': 'CO-PILOT', 'common.vehiclepositions.gunner': 'MAIN GUNNER', 'common.vehiclepositions.bomber': 'BOMBADIER', 'common.vehiclepositions.leftturret': 'LEFT TURRET', 'common.vehiclepositions.rightturret': 'RIGHT TURRET', 'common.vehiclepositions.passenger': 'PASSENGER' };

const FLIGHT_WARNINGS = { "Takeoff path blocked": 1, "Landing path blocked": 2, "Too fast": 3, "No landing zone": 4, "Flight ceiling": 5 };
