import { UI } from "./rust-render/frame-layout.js";
import * as THREE from 'three';
import { InterfaceSurface } from './interface/surface.js';
import { Batch } from './interface/batch.js';
import { HudInput, relationCode, MAP_DRAW, RINGS } from './hud-input.js';
import { HudArt, HudFrame as H, HudMapDraw as M, HudRing as R, HudRenderFrame as L, HudDraw as D, HudCommand as C } from './hud-layout.js';
import { radarRelation, unitDefinition } from './targeting-facts.js';
import { humanName } from './bot-names.js';

// Battle page host. Rust owns its view model, radar, text and quad generation;
// the page retains shell layer/overlay surfaces and resource IO until those
// surfaces acquire their own Rust owners. There is one required battle path.
export class BattleHud extends InterfaceSurface {
  constructor(renderer, { url, fonts, strings, map, mapImage, graphics }) {
    super(renderer, { url, fonts, strings });
    this.graphics = graphics; this.frame = map.minimap; this.mapScale = map.commandPost?.mapScale ?? 1;
    this.mapImage = mapImage ? this.load(mapImage) : null;
    this.vehicle = null; this.mount = null; this.arsenal = null; this.reticle = null; this.lockMarker = null;
    this.unitSource = null; this.unitContext = {}; this.layer = null; this.overlay = null; this.time = 0;
    this.reticleRecord = { y: 0, weapon: null, x: .5, weapons: null };
    this.input = new HudInput(this, fonts);
    // Retain submission wrappers so the render phase allocates no HUD objects.
    this.nativeDraw = { batch: this.batch, scene: this.scene, camera: this.camera, nativeMode: 0 };
    this.bannerDraw = { batch: this.batch, scene: this.scene, camera: this.camera, nativeMode: 1 };
  }
  toggleMap() { this.input.event(C.toggleMap); }
  setLayer(layer) { this.layer = layer || null; }
  setOverlay(overlay) { this.overlay = overlay || null; }
  get covered() { return Boolean(this.overlay && this.overlay.hidesHud !== false); }
  watch(fn) { (this.watchers ??= new Set()).add(fn); return () => this.watchers.delete(fn); }
  spawnMapPlace() { return { x: .95 * 1024 - 128 * (1024 / 800), y: this.height / 2, size: 328 * (1024 / 800) }; }
  mapPoint(x, z, size) { return [((x - this.frame.center[0]) / this.frame.half) * (size / 2), ((z - this.frame.center[1]) / this.frame.half) * (size / 2)]; }
  get postIconSize() { return 6 * this.mapScale; }
  relationOf(team, viewer) {
    if (viewer === undefined || viewer === this.posts?.playerTeam) return this.posts?.relation(team) ?? 'neutral';
    return this.posts?.look ? this.posts.look(viewer, team) : radarRelation(viewer, team);
  }
  message(text, relation = null) { this.input.event(C.message, this.input.text(text, true), relationCode(relation)); }
  announce(text, relation) { this.input.state(C.banner, text == null ? 0 : this.input.text(text, true), relationCode(relation)); }
  setLeaving(alpha) { this.input.f[H.leaving] = alpha; }
  wake() { this.input.event(C.wake); }
  setPlayer(arsenal, health, icon = 'hud_health') {
    this.arsenal = arsenal;
    this.input.f[H.health] = health;
    const name = icon || 'hud_health';
    if (name !== this.healthIconName) { this.healthIconName = name; this.input.f[H.healthIcon] = this.input.asset(name); }
    this.input.state(C.health, health);
  }
  setVehicle(vehicle, mount = vehicle) {
    if (vehicle !== this.vehicle || (mount ?? null) !== this.mount) this.wake();
    this.vehicle = vehicle; this.mount = mount ?? null;
  }
  setReticle(y, weapon, x = .5, weapons = null) {
    const r = this.reticleRecord; r.y = y; r.weapon = weapon; r.x = x; r.weapons = weapons;
    this.reticle = y === null ? null : r;
  }
  setUnits(units, context) { this.unitSource = units; this.unitContext = context; }
  noteShot(unit, weapon) { const id = this.input.unitId(unit); if (id) this.input.event(C.shot, id, this.input.unitGeneration(unit), (weapon?.def || weapon)?.ShotDelay || 0); }
  hit(relation) { this.input.event(C.hit, relationCode(relation)); }
  setReticleTarget(relation, marker = null, second = null) {
    this.input.f[H.targetRelation] = relationCode(relation); this.input.f[H.secondRelation] = relationCode(second); this.lockMarker = marker;
  }
  enemyHealth(unit) {
    if (!unit?.alive) return;
    const def = unitDefinition(unit), health = unit.combat?.health ?? unit.health;
    const max = unit.combat?.maxHealth ?? unit.maxHealth ?? def.MaxHealth;
    this.input.state(C.enemy, health / Math.max(max || 1, 1), this.input.definition(def).health || HudArt.hud_health);
  }
  killMessage({ killer = null, victim, relationToViewer = 'neutral' }) {
    if (!victim || !(victim.human || victim.character?.human || killer?.human || killer?.character?.human || killer?.hero)) return;
    this.input.event(C.kill, !killer ? 0 : killer === victim ? 1 : killer.team === victim.team ? 2 : 3,
      this.input.text(unitName(killer), true), this.input.text(unitName(victim), true), relationCode(relationToViewer));
  }
  stats(values) { this.input.state(C.stats, values.kills ?? 0, values.teamkills ?? 0, values.deaths ?? 0, values.captures ?? 0); }
  squadCommand(kind) {
    const index = typeof kind === 'number' ? kind : COMMANDS[kind];
    if (Number.isInteger(index) && index >= 0 && index <= 3) this.input.state(C.squad, index);
  }
  setSecondBar(icon, fill, look = 'normal') {
    if (icon !== this.secondIconName) { this.secondIconName = icon; this.input.f[H.secondIcon] = this.input.asset(icon); }
    const id = this.input.f[H.secondIcon]; this.input.f[H.secondFill] = fill || 0;
    this.input.f[H.secondLook] = look === 'low' ? 1 : look === 'pulse' ? 2 : 0;
    this.input.state(C.second, id, fill || 0);
  }
  setHull(icon, fill = 0) {
    if (icon !== this.hullIconName) { this.hullIconName = icon; this.input.f[H.hullIcon] = this.input.asset(icon); }
    this.input.f[H.hullFill] = fill; this.input.state(C.hull, this.input.f[H.hullIcon], fill);
  }
  setSupply(health, ammo) { this.input.f[H.supplyHealth] = Boolean(health); this.input.f[H.supplyAmmo] = Boolean(ammo); this.input.state(C.supply, Number(Boolean(health)), Number(Boolean(ammo))); }
  setScope(name) { if (name !== this.scopeName) { this.scopeName = name; this.input.f[H.scope] = this.input.asset(name); } }
  damageFrom(angle) { this.input.event(C.damage, angle); }
  update(dt, posts, player, battle) {
    this.posts = posts; this.player = player; this.battle = battle;
    this.input.f[H.dt] = dt; this.input.facts(true); this.input.publish();
    this.time = this.graphics.rust.rust.hud_model_update();
  }

  // The shell's map entry remains callable by its holomap and Classic view.
  // It delegates geometry to the same native implementation as the minimap.
  // Copying into a shell-owned Batch is a resource dependency of that surface.
  drawBattlefield(m, size, { team, mask = null, labels = true, player = null, rings = [], spawn = false } = {}) {
    const input = this.input, f = input.f;
    input.facts(); input.posts(team);
    for (let i = 0; i < 16; i++) f[MAP_DRAW + M.matrix + i] = m.elements[i];
    f[MAP_DRAW + M.size] = size; f[MAP_DRAW + M.labels] = labels; f[MAP_DRAW + M.spawn] = spawn; f[MAP_DRAW + M.mask] = input.asset(mask);
    f[MAP_DRAW + M.player] = Boolean(player); f[MAP_DRAW + M.playerX] = player?.position.x || 0; f[MAP_DRAW + M.playerZ] = player?.position.z || 0; f[MAP_DRAW + M.playerTurn] = player?.turn || 0;
    this.mapRings(rings); input.publish();
    this.appendMap();
  }
  mapRings(rings) {
    const f = this.input.f, count = Math.min(128, rings?.length || 0); f[MAP_DRAW + M.rings] = count;
    for (let i = 0; i < count; i++) { const ring = rings[i], o = RINGS + i * R.words; f[o + R.x] = ring.x; f[o + R.z] = ring.z; f[o + R.size] = ring.size ?? this.postIconSize; f[o + R.alpha] = ring.alpha; }
  }
  appendMap() {
    const core = this.graphics.rust, batch = this.batch;
    // Initialize the shared pass's persistent mirrors before the holomap's
    // prepare runs ahead of the first battle HUD draw.
    core.byName.hud.ensure(this);
    const count = core.rust.hud_model_build(2, true); core.views();
    const first = batch.count, last = Math.min(2048, first + count);
    for (let i = 0; i < (last - first) * 12; i++) batch.position[first * 12 + i] = core.hudPosition[i];
    for (let i = 0; i < (last - first) * 8; i++) { batch.uv[first * 8 + i] = core.hudUv[i]; batch.maskUv[first * 8 + i] = core.hudMaskUv[i]; }
    for (let i = 0; i < (last - first) * 16; i++) batch.tint[first * 16 + i] = core.hudTint[i];
    batch.dirty.fill(1, first, last); batch.count = last;
    const input = core.hudInput;
    for (let i = 0; i < input[L.groups]; i++) {
      const o = L.words + i * D.words, texture = this.input.imagesByHandle.get(input[o + D.texture]);
      const mask = input[o + D.masked] ? this.input.imagesByHandle.get(input[o + D.mask]) : null;
      const material = batch.material(texture, mask); let materialIndex = batch.looks.indexOf(material);
      if (materialIndex < 0) materialIndex = batch.looks.push(material) - 1;
      const start=first*6+input[o+D.first],count=Math.min(input[o+D.count],last*6-start);
      if(count<=0||batch.drawGroups.length>=batch.groups.length)continue;
      const group=batch.groups[batch.drawGroups.length];group.start=start;group.count=count;group.materialIndex=materialIndex;batch.drawGroups.push(group);
    }
    // A shell copy displaced the persistent GPU mirrors. The next owner must
    // upload fully, including when it is the native battle batch again.
    core.byName.hud.full = true;
  }
  drawSpawnMap() {} // The main layer map is included in the native batch.
  postColor(post, team, spawn = false) {
    const color = COLORS[this.relationOf(post.team, team)];
    let taking = null;
    if (post.team && post.neutralize > 0) { for (const t of post.counts.keys()) if (t !== post.team) { taking = t; break; } }
    else if (!post.team && post.progress > 0) taking = post.toward;
    const lit = spawn ? this.time % 1 < .25 : Math.floor(this.time * 4) % 2;
    return taking && lit ? COLORS[this.relationOf(taking, team)] : color;
  }

  render() {
    if (this.watchers) for (const watcher of this.watchers) watcher(this);
    const layer = this.layer, overlay = this.overlay;
    layer?.prepare?.(this); overlay?.prepare?.(this);
    const covered = this.covered, renderer = this.renderer, core = this.graphics.rust;
    if (layer && !covered) { this.batch.begin(); layer.draw?.(this); this.batch.end(); }
    this.input.facts(); this.mapRings(layer?.battlefield?.rings); this.input.publish();
    const autoClear = renderer.autoClear; renderer.autoClear = false;
    core.frame.uiStage(UI.BEFORE);
    if (layer?.renderBefore) { renderer.setRenderTarget(null); layer.renderBefore(renderer, this); }
    if (!core.contextLost) {
      core.frame.uiStage(UI.NATIVE);core.coverHud(renderer, this.nativeDraw);
      if (layer && !covered) { core.frame.uiStage(UI.LAYER);core.coverHud(renderer, this);core.frame.uiStage(UI.BANNER);core.coverHud(renderer, this.bannerDraw); }
    }
    core.frame.uiStage(UI.AFTER);
    if (layer?.renderAfter) { renderer.setRenderTarget(null); layer.renderAfter(renderer, this); }
    const layerOver = !covered && layer?.drawOver;
    if (layerOver || overlay?.draw) {
      if (!this.overBatch) {
        this.overBatch = new Batch(); this.overScene = new THREE.Scene(); this.overScene.add(this.overBatch.mesh);
        this.overDraw = { batch: this.overBatch, scene: this.overScene, camera: this.camera };
      }
      this.overBatch.begin(); if (layerOver) layer.drawOver(this, this.overBatch); overlay?.draw?.(this, this.overBatch); this.overBatch.end();
      core.frame.uiStage(UI.OVER);renderer.setRenderTarget(null); if (!core.contextLost) core.coverHud(renderer, this.overDraw);
    }
    core.frame.uiStage(UI.OVERLAY);
    if (overlay?.renderAfter) { renderer.setRenderTarget(null); overlay.renderAfter(renderer, this); }
    renderer.autoClear = autoClear;
  }
}
const COMMANDS = { moveout: 0, move: 0, hold: 1, follow: 2, atease: 3, dismiss: 3 };
const COLORS = { friendly: [32/255,223/255,32/255,1], enemy: [223/255,32/255,32/255,1], neutral: [192/255,192/255,192/255,1], locals: [223/255,223/255,32/255,1] };

function unitName(unit) { return unit?.character?.name || (unit?.human ? humanName() : null) || unit?.name || unit?.classDef?.Label || 'Soldier'; }
