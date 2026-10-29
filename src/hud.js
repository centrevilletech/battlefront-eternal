import * as THREE from "three";
import { InterfaceSurface } from "./interface/surface.js";
import { Batch } from "./interface/batch.js";
import { hasMissileLock, lockTickRadii, spreadTickRadii } from "./weapon-targeting.js";
import { IW, SAFE, SAFE_SCALE } from "./interface/space.js";
import { RadarUnits, HitRing, relationName, radarRelation, unitDefinition, mapHeading, mapPosition, mapIconRotation, killMessage as formatKillMessage } from "./hud-units.js";

// The battle HUD, laid out as the 2004 game lays it out.
//
// It draws in the interface space and with the drawing kit of
// src/interface/ (InterfaceSurface): 1024 units across the screen and
// H = 768 + (h / w - 0.75) x 768 units down it, y down. Each group hangs
// from an anchor on the screen, most inside a 5% safe margin, through its
// own transform, and it is all drawn through one fixed perspective camera
// (60 degrees across, 46.8 up, whatever the window's shape) onto a plane
// 886.81 units away, so the groups turned about their vertical axis show in
// perspective. Panels are cut from hud1 and hud2 by exact texture
// rectangles, text is the game's own bitmap fonts, and every quad is its
// texture times a tint, alpha blended.
//
// - Weapons, top left, turned 30 degrees: a panel per channel with the
//   weapon's icon, rounds in the clip / in reserve, and a bar for a
//   launcher's lock, a charge or a reload's progress (green) or what heat
//   leaves (blue, red overheated). A panel's weapon's name shows when it
//   changes and fades over 2 s.
// - Health, top right, turned -30 degrees: a bar cut from its left end, blue,
//   blinking toward red at half health and below (faster as it falls), and
//   flashing white when hurt. Under it a second, smaller bar shows what the
//   soldier has besides health, with its icon: a jetpack's fuel is blue, red
//   below what lighting it takes, and pulsing red if it gets that low in
//   flight. Under that, red, the health of the enemy his aim is on, fading
//   for 2 s once it is off.
// - Both groups fade to 40% after 3 s with nothing new and come back on a
//   change.
// - Reinforcements, top centre at 1.7x and 80% alpha: each team's bar and
//   count; a team that is bleeding blinks toward white. Below it, the
//   countdown for a team holding no posts.
// - The reticle sits on the screen's vertical centre line at the height of
//   the point 1024 m along the aim, at its texture's size, dimmed while the
//   weapon waits between shots, reloads, or cools.
// - Capture, right edge: hud_flag_timer filled in the colours of the teams
//   taking the post the soldier stands in.
// - The minimap, bottom left: the map at half alpha through a round mask,
//   125 m to its rim (out to 250 m as what he controls speeds up), turned
//   so the soldier (or his seat) faces up, with the posts in relation
//   colours pinned inside the rim. M shows the whole map instead.
// - Messages, lower right: five lines at most, newest at the bottom, 10 s
//   each.
// - Damage direction: a bar at the screen edge on the side an explosion hit
//   from, fading over a second.
// - Outside the battlefield: LEAVING BATTLEFIELD flashing red in the middle
//   of the screen (src/battlefield.js).
// - Stats, left middle: the player's KILLS, DEATHS and CP POINTS, faint at
//   rest, for 3 s after one changes, the changed row flashing yellow or red.
// Dead, the player has no body: until the spawn screen only the
// reinforcements, the messages and the stats show.
// - Zoomed in first person, the weapon's scope overlay: a quarter of it per
//   screen quarter, mirrored about the centre.
// - Seated (a vehicle's seat, a craft's as any other, or a turret), the
//   same displays, read from the seat this page knows he holds each frame:
//   the seat's two channels in the weapons panels, named for the seat
//   (PILOT, MAIN GUNNER...) when he takes it; his own bar still on top and
//   what he sits in on the second panel, with a pip per seat of the hull
//   (his white, anyone else's green); both channels' reticles at one anchor,
//   each in the colour of what its own weapon is on; no capture timer. A
//   craft's pilot gets the port's own takeoff and landing prompts where
//   the original game's tool tips go.
//
// A layer (setLayer) can take the screen over, as the spawn screen
// (src/spawn-screen) and the scoreboard (src/scoreboard) do: the player's
// own displays and the messages hide, the reinforcements count from the
// layer's team (or hide), the map shows the layer's battlefield, and the
// layer draws its own quads and 3D models around the HUD's. An overlay
// (setOverlay), the pause menu (src/pause), goes over all of that: under it
// only the layer's 3D passes still draw (the spawn screen's room and body,
// as the original's pause over the spawn screen shows them).

const rgba = (r, g, b, a = 255) => [r / 255, g / 255, b / 255, a / 255];
const WHITE = rgba(255, 255, 255);
const SHADOW = rgba(0, 0, 0, 207);
const GREEN = rgba(32, 223, 32);
const RED = rgba(223, 32, 32);
const LIGHT_GRAY = rgba(192, 192, 192);
const YELLOW = rgba(223, 223, 32);
const BAR = rgba(0, 152, 255);
const BAR_LOW = rgba(203, 57, 64);
const RETICLE = rgba(67, 167, 242);
const RELATION = { friendly: GREEN, enemy: RED, neutral: LIGHT_GRAY, locals: YELLOW };

// Group faders: woken, in at 8/s and held 3 s, then out at 1/7 per second
// down to 40%.
const FADE_IN = 8;
const FADE_HOLD = 3;
const FADE_OUT = 1 / 7;
const FADE_MIN = 0.4;

// The weapon panels, per channel: where the group sits, its panel, its bar,
// and where its numbers go.
const AMMO = [
  { at: [0, 0], panel: [118, 33, [0.976562, 0.171875, 0.0551181, 0.429688]], bar: [30, 2, 84, 13, [0.828125, 0.0390625, 0.171875, 0.140625]], text: [66, -3] },
  { at: [27, 16], panel: [84, 33, [0.945312, 0.445312, 0.289062, 0.703125]], bar: [32, 2, 48, 12, [0.679688, 0.757812, 0.304688, 0.851562]], text: [48, -3] },
];
const NAME_FADE = 2; // s
const SEAT_PIPS = 5; // seat pips under the hull's panel, at most

// A craft's prompt, where the original game's tool tips go: the middle of the
// band from 80% to 95% down the screen, its text at alpha 204, fading in.
const PROMPT_Y = 0.875;
const PROMPT_ALPHA = 0.8;
const PROMPT_FADE_IN = 0.5; // s

const HEALTH_FLASH = 0.5; // s
const HEALTH_LOW = 0.5;
const BLINK = 0.25; // s of each blink's pulse

const TEAM_ALPHA = 204 / 255;
const TEAM_SCALE = 1.7;

const MINIMAP_SIZE = 143.36; // units
const MINIMAP_CENTER = 71.68;
const MINIMAP_RANGE = 125; // m from the centre to the rim, at up to 10 m/s
const MINIMAP_FAR = 250; // m to the rim at 100 m/s and faster
const MAP_ALPHA = 128 / 255;
const POST_ICON = 6; // units x MapScale
const PLAYER_ICON = 16;
const BIG_MAP = 600; // units across, the whole battlefield
// The spawn screen's map (the original game's layout 2): 328 x W/800 across,
// 128 x W/800 left of the safe area's right middle, through map_mask.
// hud_map_raster's scan lines don't show on the retail screenshots, so
// they aren't drawn.
const SPAWN_MAP_SCALE = IW / 800;
const SPAWN_MAP = 328 * SPAWN_MAP_SCALE;
const SPAWN_MAP_LEFT = 128 * SPAWN_MAP_SCALE;
// The ring around the spawn screen's picked post: its size is the icon's
// plus 3 x its highlight, and its line sits at 0.86 of that (33 units
// across, as the retail screenshots measure).
const RING_SCALE = 0.86;
// The reinforcements over the spawn screen: 10 units left, and turned about
// their vertical axis while a team switch flips them.
const TEAM_SPAWN_SHIFT = -10;

const MESSAGE_LINES = 5;
const MESSAGE_PITCH = 20;
const MESSAGE_LIFE = 10; // s
const MESSAGE_SCROLL = 2 * MESSAGE_PITCH; // units per second

// The stat popup: three rows 32 apart from the safe area's left middle, each
// drawn at 0.75 from 36 units in: its icon 24 units square from 9 to 33 in,
// its number ending 63 in. At rest it is white at alpha 64.
const STAT_ICONS = ["stats_kills", "stats_deaths", "stats_cpp"];
const STAT_PITCH = 32;
const STAT_SCALE = 0.75;
const STAT_ICON = [9, 33];
const STAT_NUMBER = 63;
const STAT_REST = rgba(255, 255, 255, 64);
const STAT_SHOW = 3; // s on screen after a change, fading over the last
const STAT_FLASH = 1; // s a change's colour lasts, held for the first half

const DAMAGE_FADE = 1; // per second
// Where each damage bar sits and how it turns: right, behind, left, ahead.
const DAMAGE = [
  [1, 0.5, -Math.PI / 2],
  [0.5, 1, 0],
  [0, 0.5, Math.PI / 2],
  [0.5, 0, Math.PI],
];

// Why a craft's takeoff or landing was refused (src/flight/model.js), as
// its pilot is told for the 2 s the craft keeps the reason, and why it
// noses down at the map's ceiling.
const FLIGHT_REFUSALS = {
  "Takeoff path blocked": "TAKEOFF PATH BLOCKED",
  "Landing path blocked": "LANDING PATH BLOCKED",
  "Too fast": "TOO FAST TO LAND",
  "No landing zone": "NO LANDING ZONE",
  "Flight ceiling": "FLIGHT CEILING",
};

// What a craft's pilot is prompted with, from the craft's own replicated
// state, or null: { action, key, text } where action is the flight binding
// whose key goes first (or none) and key the string's localization key
// (text its English). Landed, the original's tool tip for its takeoff
// (game.tooltips.flyertakeoff). The rest are the port's own, for what its
// flight model needs: over ground he can land on, slow enough, the same key
// TO LAND; a refused takeoff or landing, the reason; at the ceiling, that.
export function flightPrompt(v) {
  if (!v?.flying || !v.alive || v.seatIndex !== 0) return null;
  const refused = FLIGHT_REFUSALS[v.flightWarning];
  if (refused) return { action: null, key: null, text: refused };
  if (v.flightState === "Landed") return { action: "takeoff", key: "game.tooltips.flyertakeoff", text: "TO TAKE OFF" };
  if (v.flightState === "Flying" && v.landingReason === "Press SPACE to land") return { action: "takeoff", key: null, text: "TO LAND" };
  return null;
}

// A seat's name, from its section's VehiclePosition (kept by its hash,
// da1b8c0c): the localized string, else its English, else none.
const SEAT_NAMES = {
  "common.vehiclepositions.pilot": "PILOT",
  "common.vehiclepositions.copilot": "CO-PILOT",
  "common.vehiclepositions.gunner": "MAIN GUNNER",
  "common.vehiclepositions.bomber": "BOMBADIER",
  "common.vehiclepositions.leftturret": "LEFT TURRET",
  "common.vehiclepositions.rightturret": "RIGHT TURRET",
  "common.vehiclepositions.passenger": "PASSENGER",
};
export function seatName(v, strings = {}) {
  const key = v?.seats?.[v.seatIndex]?.settings?.da1b8c0c;
  return key ? strings[key] || SEAT_NAMES[key] || "" : "";
}

// The minimap's reach, centre to rim, in metres: 125 at 10 m/s or slower,
// widening with the speed across the ground of what he controls to 250 at
// 100 m/s (the 2004 game's map display).
export function minimapRange(speed = 0) {
  const t = THREE.MathUtils.clamp((speed - 10) / 90, 0, 1);
  return MINIMAP_RANGE + (MINIMAP_FAR - MINIMAP_RANGE) * t;
}

// The HUD's own art, loaded up front.
const ART = ["hud1", "hud2", "hud_health", "hud_vehiclehealth", "hud_flag_timer", "hud_flag_icon", "player_icon", "map_mask", "ambush_icon", "hud_jetpack_icon", "troop_icon", "hit_indicator", "stats_kills", "stats_deaths", "stats_cpp"];

function translate(x, y, z = 0) {
  return new THREE.Matrix4().makeTranslation(x, y, z);
}

// A rectangle w x h aligned by ha / va: 0 from the point, 1 centred on it,
// 2 ending at it.
function aligned(w, h, ha, va) {
  const x0 = ha === 0 ? 0 : ha === 1 ? -w / 2 : -w;
  const y0 = va === 0 ? 0 : va === 1 ? -h / 2 : -h;
  return [x0, y0, x0 + w, y0 + h];
}

function mix(a, b, t) {
  return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t];
}

function alpha(color, a) {
  return [color[0], color[1], color[2], color[3] * a];
}

// 0 -> 1 -> 0 over the last `pulse` seconds of every `period`.
function blink(time, period, pulse = BLINK) {
  const into = (time % period) - (period - pulse);
  if (into <= 0) return 0;
  return 1 - Math.abs((into / pulse) * 2 - 1);
}

class Fader {
  constructor() {
    this.alpha = 0;
    this.hold = 0;
  }

  wake() {
    this.hold = FADE_HOLD;
  }

  update(dt) {
    if (this.hold > 0) {
      this.alpha = Math.min(this.alpha + FADE_IN * dt, 1);
      this.hold -= dt;
    } else {
      this.alpha = Math.max(this.alpha - FADE_OUT * dt, FADE_MIN);
    }
  }
}

export class BattleHud extends InterfaceSurface {
  // renderer: the WebGL renderer; url(name) finds a texture's file; fonts and
  // strings come from common.json; map is the map's manifest.
  constructor(renderer, { url, fonts, strings, map, mapImage }) {
    super(renderer, { url, fonts, strings });
    this.frame = map.minimap;
    this.mapScale = map.commandPost?.mapScale ?? 1;
    this.vehicle = null;
    this.mount = null;
    this.mapImage = mapImage ? this.load(mapImage) : null;
    this.expanded = false;
    this.time = 0;

    this.weaponFader = new Fader();
    this.healthFader = new Fader();
    this.arsenal = null;
    this.health = 1;
    this.shownHealth = null;
    this.healthFlash = 0;
    this.named = [null, null]; // each panel's weapon whose name is showing
    this.nameLeft = [0, 0];
    this.numbers = ["", ""];
    this.reticle = null; // { y, weapon, x, weapons } with y the anchor's height, 0..1
    this.scope = null;
    this.second = null; // { icon, fill, look } for the health group's second bar
    this.hull = null; // { icon, fill, look } of what he sits in, under his own health
    this.shownHull = null;
    this.hullFlash = 0;
    this.prompt = null; // { line, age } of a craft's prompt showing
    this.supply = { health: false, ammo: false }; // a droid serving him now
    this.secondWoke = null; // its fill when it last woke the group
    this.messages = [];
    this.scroll = 0;
    this.damage = DAMAGE.map(() => 0);
    this.banner = null;
    this.leaving = 0; // LEAVING BATTLEFIELD's alpha, 0 hidden
    this.countdown = { second: null, alpha: 1 };
    this.radar = new RadarUnits();
    this.unitSource = null;
    this.unitContext = {};
    this.hitRing = new HitRing();
    this.targetRelation = null;
    this.lockMarker = null;
    this.enemy = null;
    this.enemyLeft = 0;
    this.statValues = null;
    this.statChanges = [0, 0, 0];
    this.statFlash = [0, 0, 0];
    this.statsLeft = 0;
    this.command = null;
    this.commandLeft = 0;
    this.layer = null;
    this.overlay = null;
    this.preload(ART);
  }

  toggleMap() {
    this.expanded = !this.expanded;
  }

  // A screen over the HUD, or null. It may have:
  // - prepare(hud): called first in every render (it can step itself);
  // - teams: { own, foe, flip } for the reinforcements (flip in radians
  //   about their vertical axis), or false to hide them;
  // - battlefield: { team, rings: [{ x, z, size, alpha }] } for the map, in
  //   the spawn layout and as that team sees it;
  // - draw(hud): its quads, after the HUD's own;
  // - renderBefore(renderer, hud) and renderAfter(renderer, hud): its own
  //   passes, under and over the HUD's quads;
  // - drawOver(hud, batch): quads over all of that.
  setLayer(layer) {
    this.layer = layer || null;
  }

  // A second slot over the layer, or null: the pause menu. It may have
  // prepare(hud), draw(hud, batch) (its quads, over everything) and
  // renderAfter(renderer, hud). While it is up the HUD's own displays hide,
  // and of the layer only its prepare and its 3D passes run; `covered`
  // tells the layer so (the spawn screen hides its logos). An overlay with
  // hidesHud false only draws over the HUD.
  setOverlay(overlay) {
    this.overlay = overlay || null;
  }

  get covered() {
    return Boolean(this.overlay && this.overlay.hidesHud !== false);
  }

  // fn(hud), called first in every render, with a layer or without (the
  // scoreboard watches for the battle's end with it). Returns a function
  // that stops it.
  watch(fn) {
    (this.watchers ??= new Set()).add(fn);
    return () => this.watchers.delete(fn);
  }

  // The spawn screen's map: its centre in interface units and its size.
  spawnMapPlace() {
    return { x: (SAFE + SAFE_SCALE) * IW - SPAWN_MAP_LEFT, y: this.height / 2, size: SPAWN_MAP };
  }

  // A world point (x, z) on a battlefield map `size` units across, from its
  // centre (north up).
  mapPoint(x, z, size) {
    const frame = this.frame;
    const half = size / 2;
    return [((x - frame.center[0]) / frame.half) * half, ((z - frame.center[1]) / frame.half) * half];
  }

  // The minimap's reach this frame (minimapRange).
  get mapRange() {
    return minimapRange(this.player?.speed ?? 0);
  }

  // A command post's icon size on the maps.
  get postIconSize() {
    return POST_ICON * this.mapScale;
  }

  // How a team looks to `viewer`: friendly, enemy, neutral or locals.
  relationOf(team, viewer) {
    if (viewer === undefined || viewer === this.posts?.playerTeam) return this.posts?.relation(team) ?? "neutral";
    return this.posts?.look ? this.posts.look(viewer, team) : radarRelation(viewer, team);
  }

  // A line in the message feed, in a relation colour.
  message(text, relation = null) {
    this.messages.push({ text: String(text).toUpperCase(), color: RELATION[relation] || WHITE, age: 0 });
    if (this.messages.length > MESSAGE_LINES) this.messages.shift();
    this.scroll = MESSAGE_PITCH;
  }

  // A line across the middle of the screen (the battle's end), or null to
  // take it down.
  announce(text, relation) {
    this.banner = text == null ? null : { text: String(text).toUpperCase(), color: RELATION[relation] || WHITE };
  }

  // LEAVING BATTLEFIELD's alpha this frame, 0 to hide it
  // (src/battlefield.js LeavingWarning).
  setLeaving(alpha) {
    this.leaving = alpha > 0 ? Math.min(alpha, 1) : 0;
  }

  // Everything comes back in on a new life.
  wake() {
    this.weaponFader.wake();
    this.healthFader.wake();
    this.shownHealth = null;
    this.shownHull = null;
  }

  // The channels' weapons (his own, or his seat's or turret's) and his own
  // health, 0..1, with its icon (hud_health: the original game never shows the
  // class's own there), seated or not (setHull shows what he sits in). A
  // change of more than 1% wakes the health group; a loss flashes the bar.
  setPlayer(arsenal, health, icon = "hud_health") {
    this.arsenal = arsenal;
    this.healthIcon = icon || "hud_health";
    if (this.shownHealth === null || Math.abs(health - this.shownHealth) > 0.01) {
      if (this.shownHealth !== null && health < this.shownHealth) this.healthFlash = 1;
      this.shownHealth = health;
      this.healthFader.wake();
    }
    this.health = health;
  }

  // The vehicle whose seat he holds (its seat's displays), or null; and
  // what he sits in, that vehicle or a turret, or null on his feet.
  setVehicle(vehicle, mount = vehicle) {
    if (vehicle !== this.vehicle || (mount ?? null) !== this.mount) this.wake();
    this.vehicle = vehicle;
    this.mount = mount ?? null;
  }

  // The reticle's anchor height (0 top, 1 bottom) and the weapon it is for
  // (the one whose lock it shows), or null to hide it; x across, and the
  // weapons whose reticles draw there (a vehicle seat's two channels; by
  // default the one).
  setReticle(y, weapon, x = .5, weapons = null) {
    this.reticle = y === null ? null : { y, weapon, x, weapons };
  }

  // The source and context are live; update() samples them once per frame.
  setUnits(units, context = {}) {
    this.unitSource = units;
    this.unitContext = context;
  }

  noteShot(unit, weapon) { this.radar.shot(unit, weapon?.def || weapon, this.time); }
  hit(relation) { this.hitRing.hit(relation); }

  // What the reticle is on, as a relation (null for nothing), and seated
  // what channel 1's is on (its own weapon targets apart). marker:
  // projected target anchor { x, y, progress: 0..1 } for a lock-on weapon.
  // Progress belongs to the caller's weapon lock state.
  setReticleTarget(relation, marker = null, second = null) {
    this.targetRelation = relation == null ? null : relationName(relation);
    this.secondRelation = second == null ? null : relationName(second);
    this.lockMarker = marker;
  }

  enemyHealth(unit) {
    if (!unit?.alive) return;
    const def = unitDefinition(unit);
    const health = unit.combat?.health ?? unit.health;
    const max = unit.combat?.maxHealth ?? unit.maxHealth ?? def.MaxHealth;
    this.enemy = { fill: THREE.MathUtils.clamp(health / Math.max(max || 1, 1), 0, 1), icon: def.HealthTexture || "hud_health" };
    this.enemyLeft = 2;
    this.healthFader.wake();
  }

  killMessage(event) {
    const line = formatKillMessage(event, this.strings);
    if (line) this.message(line.text, line.relation);
  }

  // The player's Character's numbers, alive or dead: KILLS (kills - 2 x team
  // kills), DEATHS and CP POINTS. A change flashes that row and shows the
  // popup.
  stats({ kills = 0, teamkills = 0, deaths = 0, captures = 0 }) {
    const next = [kills - 2 * teamkills, deaths, captures];
    if (this.statValues && next.some((value, i) => value !== this.statValues[i])) {
      next.forEach((value, i) => {
        if (value === this.statValues[i]) return;
        this.statChanges[i] = Math.sign(value - this.statValues[i]);
        this.statFlash[i] = STAT_FLASH;
      });
      this.statsLeft = STAT_SHOW;
    }
    this.statValues = next;
  }

  squadCommand(kind) {
    const aliases = { moveout: 0, move: 0, hold: 1, follow: 2, atease: 3, dismiss: 3 };
    const index = typeof kind === "number" ? kind : aliases[kind];
    if (!Number.isInteger(index) || index < 0 || index > 3) return;
    this.command = index;
    this.commandLeft = 2;
  }

  // The health group's second bar: an icon texture name, a fill 0..1, and
  // how it looks ("low" solid red, "pulse" pulsing toward red), or null to
  // hide it. A change of more than 1% wakes the group.
  setSecondBar(icon, fill, look = "normal") {
    if (!icon) {
      this.second = null;
      return;
    }
    const f = THREE.MathUtils.clamp(fill, 0, 1);
    if (this.secondWoke === null || Math.abs(f - this.secondWoke) > 0.01) {
      this.healthFader.wake();
      this.secondWoke = f;
    }
    this.second = { icon, fill: f, look };
  }

  // What he sits in, under his own health (the original game's health group
  // adds it for any controlled thing but the soldier: a vehicle, from any
  // of its seats, or a turret): its HealthTexture (hud_vehiclehealth for a
  // class with none) and its health, 0..1, or no icon off a seat. It
  // blinks and flashes as his own bar does.
  setHull(icon, fill = 0) {
    if (!icon) {
      this.hull = null;
      this.shownHull = null;
      return;
    }
    const f = THREE.MathUtils.clamp(fill, 0, 1);
    if (this.shownHull === null || Math.abs(f - this.shownHull) > 0.01) {
      if (this.shownHull !== null && f < this.shownHull) this.hullFlash = 1;
      this.shownHull = f;
      this.healthFader.wake();
    }
    this.hull = { icon, fill: f, look: "health" };
  }

  // A droid serving the soldier: whether it is giving health, and ammo.
  // While it is, the health bar (and the rounds) pulse green and stay shown.
  setSupply(health, ammo) {
    this.supply.health = Boolean(health);
    this.supply.ammo = Boolean(ammo);
    if (this.supply.health) this.healthFader.wake();
    if (this.supply.ammo) this.weaponFader.wake();
  }

  // 0 -> 1 -> 0 at 1.5 Hz: the supply indicator's pulse.
  get supplyPulse() {
    return 0.5 - 0.5 * Math.cos(this.time * 3 * Math.PI);
  }

  // The scope overlay's texture name while zoomed in, or null.
  setScope(name) {
    this.scope = name || null;
  }

  // An explosion hurt the soldier from `angle` radians off his facing
  // (positive to his right).
  damageFrom(angle) {
    const a = Math.atan2(Math.sin(angle), Math.cos(angle));
    const side = Math.abs(a) <= Math.PI / 4 ? 3 : Math.abs(a) >= (3 * Math.PI) / 4 ? 1 : a > 0 ? 0 : 2;
    this.damage[side] = 1;
  }

  // posts: the CommandPosts; player: { position, yaw, team, dead, speed }
  // (speed: across the ground, m/s, of what he controls); battle:
  // the reinforcement counts, starting counts, bleed rates, and countdown.
  update(dt, posts, player, battle) {
    this.time += dt;
    this.posts = posts;
    this.player = player;
    this.battle = battle;
    this.weaponFader.update(dt);
    this.healthFader.update(dt);
    this.healthFlash = Math.max(this.healthFlash - dt / HEALTH_FLASH, 0);
    this.hullFlash = Math.max(this.hullFlash - dt / HEALTH_FLASH, 0);
    // How long the craft's prompt has shown, from when it first applied.
    const line = this.promptLine();
    if (line !== this.prompt?.line) this.prompt = line ? { line, age: 0 } : null;
    else if (this.prompt) this.prompt.age += dt;
    for (let i = 0; i < this.nameLeft.length; i++) this.nameLeft[i] = Math.max(this.nameLeft[i] - dt, 0);
    this.scroll = Math.max(this.scroll - MESSAGE_SCROLL * dt, 0);
    for (const line of this.messages) line.age += dt;
    this.messages = this.messages.filter((line) => line.age < MESSAGE_LIFE);
    this.damage = this.damage.map((a) => Math.max(a - DAMAGE_FADE * dt, 0));
    this.countdown.alpha = Math.max(this.countdown.alpha - 2 * dt, 0.5);
    this.hitRing.update(dt);
    this.enemyLeft = Math.max(0, this.enemyLeft - dt);
    this.statsLeft = Math.max(0, this.statsLeft - dt);
    this.statFlash = this.statFlash.map((left) => Math.max(0, left - dt));
    this.commandLeft = Math.max(0, this.commandLeft - dt);
    // Over a layer's battlefield the radar is that team's, with no soldier
    // of its own to see from.
    const view = this.layer?.battlefield;
    const context = view ? { ...this.unitContext, viewer: null, controllable: null, viewerTeam: view.team } : this.unitContext;
    if (this.unitSource) this.radar.update(dt, this.time, this.unitSource, context);
  }

  render() {
    const batch = this.batch;
    if (this.watchers) for (const watcher of this.watchers) watcher(this);
    const layer = this.layer;
    const overlay = this.overlay;
    layer?.prepare?.(this);
    overlay?.prepare?.(this);
    const covered = this.covered;
    batch.begin();
    if (covered) {
      // Paused: the HUD hides, and so do the layer's own quads.
    } else if (layer) {
      // A screen over the battle: the reinforcements (unless it hides
      // them), its map, and its own.
      this.drawSpawnMap(layer.battlefield);
      if (layer.teams !== false) this.drawTeams(layer.teams);
      layer.draw?.(this);
    } else if (this.player?.dead) {
      // Dead, he has no body to show the displays of: the original keeps
      // only the reinforcements, the messages and the stat popup (which
      // his death has just shown) until the spawn screen.
      this.drawTeams();
      this.drawMessages();
      this.drawStats();
    } else {
      this.drawScope();
      this.drawMap();
      this.drawTeams();
      this.drawWeapons();
      this.drawHealth();
      this.drawFlightPrompt();
      this.drawCapture();
      this.drawMessages();
      this.drawDamage();
      this.drawLeaving();
      this.drawReticle();
      this.drawHitRing();
      this.drawLockDiamond();
      this.drawStats();
      this.drawSquadCommand();
    }
    if (!covered) this.drawBanner();
    batch.end();
    const renderer = this.renderer;
    renderer.setRenderTarget(null);
    const autoClear = renderer.autoClear;
    renderer.autoClear = false;
    layer?.renderBefore?.(renderer, this);
    renderer.render(this.scene, this.camera);
    layer?.renderAfter?.(renderer, this);
    // Quads over everything, the layer's models included: the layer's own
    // (its pointer) unless it is covered, then the overlay's.
    const layerOver = !covered && layer?.drawOver;
    if (layerOver || overlay?.draw) {
      if (!this.overBatch) {
        this.overBatch = new Batch();
        this.overScene = new THREE.Scene();
        this.overScene.add(this.overBatch.mesh);
      }
      this.overBatch.begin();
      if (layerOver) layer.drawOver(this, this.overBatch);
      overlay?.draw?.(this, this.overBatch);
      this.overBatch.end();
      renderer.render(this.overScene, this.camera);
    }
    overlay?.renderAfter?.(renderer, this);
    renderer.autoClear = autoClear;
  }

  drawWeapons() {
    const arsenal = this.arsenal;
    if (!arsenal) return;
    const batch = this.batch;
    const group = this.anchor(0, 0).multiply(translate(0, 21, 40)).multiply(new THREE.Matrix4().makeRotationY(Math.PI / 6));
    const hud1 = this.texture("hud1");
    const fade = this.weaponFader.alpha;
    const tint = alpha(WHITE, fade);
    // The rounds, pulsing green while a droid tops them up.
    const count = this.supply.ammo ? alpha(mix(WHITE, GREEN, 0.75 * this.supplyPulse), fade) : tint;
    const groups = [null, null];
    AMMO.forEach((layout, channel) => {
      const weapon = arsenal.current(channel);
      if (!weapon) return;
      const def = weapon.def;
      const m = group.clone().multiply(translate(layout.at[0], layout.at[1]));
      groups[channel] = m;
      const [pw, ph, puv] = layout.panel;
      batch.quad(m, 0, 0, pw, ph, hud1, puv, tint);
      // A weapon with no IconTexture (the TIE fighter's and the Jedi
      // starfighter's cannons) leaves its panel without one.
      const icon = this.texture(def.IconTexture);
      if (icon) batch.quad(m.clone().multiply(translate(6, 3)), 24, 24, 2, 2, icon, [1, 1, 0, 0], tint);

      // The bar: in the 2004 game's weapon state 4, a launcher's lock under
      // way or a held charge (the arc caster's, the mortar's), its timer
      // against its limit in green; else a reload's progress, or
      // what heat leaves.
      const [bx, by, bw, bh, buv] = layout.bar;
      const heats = weapon.bottomless;
      let fill = null;
      let color = null;
      if (hasMissileLock(def) && weapon.lockTargetId >= 0) {
        fill = def.LockTime > 0 ? Math.min(1, weapon.lockElapsed / def.LockTime) : 1;
        color = GREEN;
        this.weaponFader.wake();
      } else if (weapon.charging) {
        fill = Math.min(1, weapon.held / Math.max(weapon.levels?.[0].MaxPressedTime || 0, 1e-3));
        color = GREEN;
        this.weaponFader.wake();
      } else if (weapon.reloading) {
        fill = 1 - weapon.reloadLeft / Math.max(def.ReloadTime, 1e-3);
        color = GREEN;
        this.weaponFader.wake();
      } else if (heats && def.HeatPerShot > 0) {
        fill = 1 - Math.min(weapon.heat, 1);
        color = weapon.overheated ? RED : BAR;
        this.weaponFader.wake();
      }
      if (fill !== null && fill > 0) {
        batch.quad(m, bx, by, bx + bw * fill, by + bh, hud1, [buv[0], buv[1], buv[0] + (buv[2] - buv[0]) * fill, buv[3]], alpha(color, fade));
      }

      // Rounds: clip / reserve; one total for single-round clips or a
      // reserve of a clip or less; the clip alone with an endless reserve.
      if (heats) return;
      const [tx, ty] = layout.text;
      const clip = weapon.loaded;
      const reserve = weapon.spare;
      const at = m.clone().multiply(translate(tx, ty));
      let shown;
      if (reserve === Infinity) {
        shown = String(clip);
        this.fonts.draw(batch, at, 18, 0, shown, "gamefont_tiny", count, 1);
      } else if ((def.RoundsPerClip || 0) < 2 || weapon.reserve <= 1) {
        shown = String(clip + reserve);
        this.fonts.draw(batch, at, 0, 0, shown, "gamefont_tiny", count, 0);
      } else {
        shown = `${clip}/${reserve}`;
        this.fonts.draw(batch, at, 0, 0, String(clip), "gamefont_tiny", count, 1);
        this.fonts.draw(batch, at, 3, 0, "/", "gamefont_tiny", count, 2);
        this.fonts.draw(batch, at, 6, 0, String(reserve), "gamefont_tiny", count, 0);
      }
      if (shown !== this.numbers[channel]) {
        this.numbers[channel] = shown;
        this.weaponFader.wake();
      }
    });

    // Each panel's name, for 2 s after its weapon changes, over its shadow
    // (taking a seat changes both). Seated, the seat's name in its place
    // (its VehiclePosition: PILOT, MAIN GUNNER...), or none for a seat
    // with none, as the original game labels a seat's panels.
    AMMO.forEach((layout, channel) => {
      const weapon = arsenal.current(channel);
      if (weapon !== this.named[channel]) {
        if (this.named[channel] !== null || weapon) this.nameLeft[channel] = NAME_FADE;
        this.named[channel] = weapon;
        this.weaponFader.wake();
      }
      if (!groups[channel] || !weapon || this.nameLeft[channel] <= 0) return;
      const k = (this.nameLeft[channel] / NAME_FADE) * fade;
      const label = this.vehicle ? seatName(this.vehicle, this.strings) : weapon.def.label || "";
      const at = groups[channel].clone().multiply(translate(123, 1));
      this.fonts.draw(batch, at, 0, 0, label, "gamefont_tiny", alpha(SHADOW, k), 0, 1, 1, 3);
      this.fonts.draw(batch, at, 0, 0, label, "gamefont_tiny", alpha(WHITE, k));
    });
  }

  // The health group, in the original game's order: seated, the hull's seat
  // pips; his own bar; the second panel (what he sits in, or on foot what
  // he has besides health); the enemy panel.
  drawHealth() {
    if (!this.arsenal) return;
    const batch = this.batch;
    const m = this.anchor(1, 0).multiply(translate(0, 25, 40)).multiply(new THREE.Matrix4().makeRotationY(-Math.PI / 6));
    const hud1 = this.texture("hud1");
    const hud2 = this.texture("hud2");
    const fade = this.healthFader.alpha;
    const tint = alpha(WHITE, fade);
    // Right-aligned: each piece ends at its x.
    const piece = (x, y, w, h, texture, uv, color) => batch.quad(m, x - w, y, x, y + h, texture, uv, color);

    // A pip per seat of the hull, the first five: its slot, and
    // over a held seat the fill, white for him, green for anyone else with
    // a Character (a bot included), yellow for anyone without.
    const seats = this.vehicle?.seats;
    for (let i = 0; i < Math.min(seats?.length ?? 0, SEAT_PIPS); i++) {
      const x = -89 + 7 * i;
      piece(x, 35, 11, 10, hud2, [0.015625, 0.351562, 0.101562, 0.429688], tint);
      const lease = seats[i].lease;
      if (!lease) continue;
      const color = lease.unit && lease.unit === this.unitContext.viewer ? WHITE : lease.unit?.character ? GREEN : YELLOW;
      piece(x, 35, 11, 10, hud2, [0.015625, 0.5, 0.101562, 0.578125], alpha(color, fade));
    }

    piece(0, 0, 118, 33, hud1, [0.0546875, 0.171875, 0.984252, 0.429688], tint);
    const f = THREE.MathUtils.clamp(this.health, 0, 1);
    let color = BAR;
    if (f <= HEALTH_LOW) color = mix(BAR, BAR_LOW, blink(this.time, 4 * f * f + BLINK));
    if (this.supply.health) color = mix(color, GREEN, 0.75 * this.supplyPulse);
    color = mix(color, WHITE, this.healthFlash);
    if (f > 0) {
      const [u0, v0, u1, v1] = [0.171875, 0.0390625, 0.828125, 0.140625];
      piece(-30, 2, 84 * f, 13, hud1, [u1 + (u0 - u1) * f, v0, u1, v1], alpha(color, fade));
    }
    piece(-6, 4, 24, 24, this.texture(this.healthIcon) || this.texture('hud_health'), [0, 0, 1, 1], tint);

    // The second panel, cut from its left end like the first: what he sits
    // in, blinking and flashing as his own bar does; on foot, what he has
    // besides health.
    const hull = this.hull;
    const second = hull ?? this.second;
    if (second) {
      piece(-27, 16, 84, 33, hud1, [0.289062, 0.445312, 0.953125, 0.710938], tint);
      if (second.fill > 0) {
        const [u0, v0, u1, v1] = [0.304688, 0.757812, 0.679688, 0.851562];
        const f = second.fill;
        let color = BAR;
        if (second.look === "low") color = BAR_LOW;
        else if (second.look === "pulse" || (second.look === "health" && f <= HEALTH_LOW)) color = mix(BAR, BAR_LOW, blink(this.time, 4 * f * f + BLINK));
        if (hull) color = mix(color, WHITE, this.hullFlash);
        piece(-59, 18, 48 * f, 12, hud1, [u1 + (u0 - u1) * f, v0, u1, v1], alpha(color, fade));
      }
      piece(-35, 20, 24, 24, this.texture(second.icon) || (hull ? this.texture("hud_vehiclehealth") : null), [0, 0, 1, 1], tint);
    }

    // The enemy his aim is on, fading for 2 s once it is off:
    // hud2's panel, its bar cut from the left, and its HealthTexture, all
    // in the HUD's red, where the hexagons interlock under the rest.
    if (this.enemy && this.enemyLeft > 0) {
      const red = alpha(RED, fade * this.enemyLeft / 2);
      piece(0, 32, 96, 33, hud2, [0.132812, 0.359375, 0.889764, 0.617188], red);
      const f = this.enemy.fill;
      if (f > 0) piece(-31, 49, 62 * f, 13, hud2, [0.734375 - (0.734375 - 0.25) * f, 0.765625, 0.734375, 0.867188], red);
      piece(-6, 36, 24, 24, this.texture(this.enemy.icon), [0, 0, 1, 1], red);
    }
  }

  // view: a layer's { own, foe, flip }, counted from its team and moved left
  // (the spawn screen's), or null for the player's.
  drawTeams(view = null) {
    const battle = this.battle;
    if (!battle) return;
    const batch = this.batch;
    const m = this.anchor(0.5, 0).multiply(translate(0, -3));
    if (view) m.multiply(translate(TEAM_SPAWN_SHIFT, 0)).multiply(new THREE.Matrix4().makeRotationY(view.flip || 0));
    m.multiply(new THREE.Matrix4().makeScale(TEAM_SCALE, TEAM_SCALE, TEAM_SCALE));
    const hud2 = this.texture("hud2");
    const [x0, y0, x1, y1] = aligned(118, 19, 1, 0);
    batch.quad(m, x0, y0, x1, y1, hud2, [0.0390625, 0.0390625, 0.96484375, 0.1875], alpha(WHITE, TEAM_ALPHA));
    const side = (team, color) => {
      const count = Math.max(Math.ceil(battle.reinforcements[team] ?? 0), 0);
      const start = battle.start?.[team] || count || 1;
      const rate = battle.bleed?.[team] || 0;
      const k = rate > 0 ? blink(this.time, 2 / rate) : 0;
      return { count, f: THREE.MathUtils.clamp(count / start, 0, 1), color: alpha(mix(color, WHITE, k), TEAM_ALPHA) };
    };
    const own = side(view?.own ?? battle.own, GREEN);
    const foe = side(view?.foe ?? battle.foe, RED);
    if (own.f > 0) batch.quad(m, -1 - 55 * own.f, 1, -1, 5, hud2, [0.078125, 0.6875, 0.078125 + 0.4140625 * own.f, 0.703125], own.color);
    if (foe.f > 0) batch.quad(m, 1, 1, 1 + 55 * foe.f, 5, hud2, [0.921875 - 0.4140625 * foe.f, 0.6875, 0.921875, 0.703125], foe.color);
    this.fonts.draw(batch, m, -4, 3, String(own.count), "gamefont_tiny", own.color, 1, 0.6);
    this.fonts.draw(batch, m, 4, 3, String(foe.count), "gamefont_tiny", foe.color, 0, 0.6);

    // The countdown for a team that holds no posts: the last 20 s large and
    // growing, under VICTORY IN or DEFEAT IN; each new second flares.
    if (battle.countdown === null || battle.countdown === undefined) {
      this.countdown.second = null;
      return;
    }
    const seconds = Math.max(Math.floor(battle.countdown), 0);
    if (seconds !== this.countdown.second) {
      this.countdown.second = seconds;
      this.countdown.alpha = 1;
    }
    const timer = m.clone().multiply(translate(0, 30));
    const color = alpha(battle.losing ? RED : GREEN, this.countdown.alpha);
    if (seconds <= 20) {
      this.fonts.draw(batch, timer, 0, 0, this.text(battle.losing ? "hud.defeat_in" : "hud.victory_in"), "gamefont_tiny", color, 2);
      this.fonts.draw(batch, timer, 0, 15, String(seconds), "gamefont_large", color, 2, 2 - seconds / 20);
    } else if (seconds < 60) {
      this.fonts.draw(batch, timer, 0, 0, String(seconds), "gamefont_medium", color, 2);
    } else {
      this.fonts.draw(batch, timer, 0, 0, `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`, "gamefont_tiny", color, 2);
    }
  }

  // A craft's pilot's prompt (flightPrompt), from the craft he holds now,
  // as the original game lays out its tool tips (ToolTipsDisplay, which the
  // PC game never shows): white gamefont_small at 80%, centred across the
  // screen in the band from 80% to 95% of its height, fading in over half a
  // second; the key bound to the flight action first, as the controls
  // screen names it (keyName(action), set by the game).
  drawFlightPrompt() {
    const line = this.promptLine();
    if (!line) return;
    const age = line === this.prompt?.line ? this.prompt.age : 0;
    const font = "gamefont_small";
    const a = PROMPT_ALPHA * Math.min(1, age / PROMPT_FADE_IN);
    this.fonts.draw(this.batch, this.anchor(0.5, PROMPT_Y, false), 0, -this.fonts.capsMiddle(font), line, font, alpha(WHITE, a), 2);
  }

  // The prompt's line for the craft he holds now, or null.
  promptLine() {
    const prompt = flightPrompt(this.vehicle);
    if (!prompt) return null;
    const text = (prompt.key && this.strings[prompt.key]) || prompt.text;
    const key = prompt.action ? this.keyName?.(prompt.action) : null;
    return key ? `${key} ${text}` : text;
  }

  // The reticle: each weapon's own at the one anchor (seated, both
  // channels', as the 2004 game's reticle display reads a vehicle seat;
  // on foot channel 0's), each dimmed by its own weapon's state and in the
  // colour of what that weapon is on.
  drawReticle() {
    const reticle = this.reticle;
    if (!reticle) return;
    const at = this.anchor(reticle.x ?? .5, reticle.y, false);
    const weapons = reticle.weapons;
    if (!weapons) {
      this.drawWeaponReticle(at, reticle.weapon, this.reticleColor(0));
      return;
    }
    // A weapon both channels hold draws once.
    for (let channel = 0; channel < weapons.length; channel++) {
      const weapon = weapons[channel];
      if (weapon && weapons.indexOf(weapon) === channel) this.drawWeaponReticle(at, weapon, this.reticleColor(channel));
    }
  }

  // A channel's reticle colour: green on a friend, red on an enemy or the
  // locals, blue on nothing.
  reticleColor(channel = 0) {
    const relation = channel ? this.secondRelation : this.targetRelation;
    return relation === "friendly" ? GREEN : relation === "enemy" || relation === "locals" ? RED : RETICLE;
  }

  // A line of quads from (ax, ay) to (bx, by) under m, one unit wide.
  drawLine(m, ax, ay, bx, by, tint) {
    const length = Math.hypot(bx - ax, by - ay);
    const at = m.clone().multiply(translate(ax, ay)).multiply(new THREE.Matrix4().makeRotationZ(Math.atan2(by - ay, bx - ax)));
    this.batch.quad(at, 0, -.5, length, .5, this.solid, [0, 0, 1, 1], tint);
  }

  // Dim between shots (63 rising to 127), reloading or overheated (63).
  reticleAlpha(weapon) {
    if (!weapon) return 1;
    if (weapon.reloading || weapon.overheated) return 63 / 255;
    if (weapon.wait > 0 && weapon.def.ShotDelay > 0) return (63 + 64 * (1 - weapon.wait / weapon.def.ShotDelay)) / 255;
    return 1;
  }

  drawWeaponReticle(at, weapon, color) {
    const a = this.reticleAlpha(weapon);
    const texture = this.texture(weapon?.def.ReticuleTexture);
    if (!texture?.image) {
      // A seat's weapon with no reticle art: four ticks, 5 units long, as
      // far out as the weapon's spread reaches now; no lock cursor.
      if (!this.vehicle || !weapon) return;
      const [rx, ry] = spreadTickRadii(weapon, this.aimCamera, this.height);
      const tint = alpha(color, a);
      this.drawLine(at, rx, 0, rx + 5, 0, tint);
      this.drawLine(at, -rx, 0, -rx - 5, 0, tint);
      this.drawLine(at, 0, ry, 0, ry + 5, tint);
      this.drawLine(at, 0, -ry, 0, -ry - 5, tint);
      return;
    }
    const image = texture.image;
    const w = texture.userData?.nativeWidth || image?.width || 64;
    const h = texture.userData?.nativeHeight || image?.height || 64;
    const [x0, y0, x1, y1] = aligned(w, h, 1, 1);
    this.batch.quad(at, x0, y0, x1, y1, texture, [0, 0, 1, 1], alpha(color, a));
    // LockOffAngle draws eight radial ticks at the AIM anchor,
    // even without a target. It is not a circle attached to the target.
    if (!(weapon?.def.LockOffAngle > 0)) return;
    const [rx, ry] = lockTickRadii(weapon.def, this.aimCamera, this.height);
    for (let i = 0; i < 8; i++) {
      const angle = i * Math.PI / 4, x = Math.cos(angle), y = Math.sin(angle);
      this.drawLine(at, rx * x, ry * y, (rx + 5) * x, (ry + 5) * y, alpha(color, a));
    }
  }

  // Only the launcher's acquisition state draws the target diamond, over
  // the reticles and the hit ring, in its channel's colour.
  drawLockDiamond() {
    const reticle = this.reticle, weapon = reticle?.weapon;
    if (!this.lockMarker || !(weapon?.def.LockOffAngle > 0) || !this.texture(weapon.def.ReticuleTexture)?.image) return;
    const channel = reticle.weapons ? Math.max(0, reticle.weapons.indexOf(weapon)) : 0;
    const { x, y, progress = 0 } = this.lockMarker;
    const p = THREE.MathUtils.clamp(progress, 0, 1), size = 32 - 24 * p;
    const at = this.anchor(x, y, false);
    const tint = alpha(this.reticleColor(channel), Math.floor(15 + 240 * p * p) / 255);
    this.drawLine(at, 0, -size, size, 0, tint);
    this.drawLine(at, size, 0, 0, size, tint);
    this.drawLine(at, 0, size, -size, 0, tint);
    this.drawLine(at, -size, 0, 0, -size, tint);
  }

  drawHitRing() {
    if (!this.reticle) return;
    const color = this.hitRing.color();
    if (!color[3]) return;
    this.batch.quad(this.anchor(this.reticle.x ?? .5, this.reticle.y, false), -32, -32, 32, 32, this.texture("hit_indicator"), [0, 0, 1, 1], color);
  }

  // The stat popup (the original game's StatIconDisplay): a row each for
  // KILLS, DEATHS and CP POINTS, centred on its line. A changed row's icon
  // and number turn yellow if it rose, red if it fell, hold for half a
  // second and blend back over the next. Gone once the game is over.
  drawStats() {
    if (!this.statValues || this.statsLeft <= 0 || this.battle?.gameOver) return;
    const m = this.anchor(0, .5);
    const fade = Math.min(1, this.statsLeft);
    const fh = this.fonts.lineHeight("gamefont_small") * STAT_SCALE;
    const half = (STAT_ICON[1] - STAT_ICON[0]) / 2;
    STAT_ICONS.forEach((name, i) => {
      const y = i * STAT_PITCH;
      const flash = this.statFlash[i];
      const lit = this.statChanges[i] > 0 ? YELLOW : RED;
      const color = alpha(flash >= STAT_FLASH / 2 ? lit : mix(STAT_REST, lit, (2 * flash) / STAT_FLASH), fade);
      const texture = this.texture(name);
      if (texture) this.batch.quad(m, STAT_ICON[0], y - half, STAT_ICON[1], y + half, texture, [0, 0, 1, 1], color);
      else this.fonts.draw(this.batch, m, STAT_ICON[1], y - fh / 2, ["K", "D", "CP"][i], "gamefont_small", color, 1, STAT_SCALE);
      this.fonts.draw(this.batch, m, STAT_NUMBER, y - fh / 2, String(this.statValues[i]), "gamefont_small", color, 1, STAT_SCALE);
    });
  }

  drawSquadCommand() {
    if (this.commandLeft <= 0) return;
    const m = this.anchor(1, 1);
    ["MOVE OUT", "HOLD POSITION", "FOLLOW ME", "AT EASE"].forEach((label, i) => {
      this.fonts.draw(this.batch, m, 0, -80 + i * 18, label, "gamefont_tiny", alpha(WHITE, this.commandLeft / 2 * (i === this.command ? 1 : .5)), 1);
    });
  }

  // The post the soldier stands in, if his team doesn't hold it: the timer
  // fills from the bottom in the capturing team's colour, over grey (or the
  // other team's colour, every other half second, while contested).
  drawCapture() {
    const posts = this.posts;
    const player = this.player;
    // Seated he captures nothing (src/game.js updateBattle), and the
    // original game shows the timer only for what captures.
    if (!posts || !player || this.mount) return;
    const post = posts.at(player.position);
    if (!post || post.team === player.team) return;
    let p;
    let bottom;
    let top = LIGHT_GRAY;
    if (post.team) {
      p = post.neutralize;
      bottom = RELATION[posts.relation(post.team)];
    } else {
      p = 1 - post.progress;
      bottom = post.toward ? RELATION[posts.relation(post.toward)] : LIGHT_GRAY;
      const other = [...post.counts.keys()].find((team) => team !== post.toward && post.counts.get(team) > 0);
      if (post.contested && other && Math.floor(this.time * 2) % 2) top = RELATION[posts.relation(other)];
    }
    const m = this.anchor(1, 0.5).multiply(translate(0, -30));
    const texture = this.texture("hud_flag_timer");
    p = THREE.MathUtils.clamp(p, 0, 1);
    if (p > 0) this.batch.quad(m, -64, 0, 0, 64 * p, texture, [0, 0, 1, p], top);
    if (p < 1) this.batch.quad(m, -64, 64 * p, 0, 64, texture, [0, p, 1, 1], bottom);
  }

  drawMap() {
    const frame = this.frame;
    const player = this.player;
    if (!frame || !player || !this.mapImage) return;
    const batch = this.batch;
    const icon = this.texture("hud_flag_icon");
    const arrow = this.texture("player_icon");
    // Facing (sin yaw, cos yaw) on the map's x and z; turned to point up.
    const turn = mapIconRotation(player.yaw);
    const cos = Math.cos(turn);
    const sin = Math.sin(turn);
    const iconSize = POST_ICON * this.mapScale;

    if (this.expanded) {
      // The whole battlefield, fixed, in the middle of the screen.
      this.drawBattlefield(this.anchor(0.5, 0.5, false), BIG_MAP, { player: { position: player.position, turn } });
      return;
    }

    const m = this.anchor(0, 1).multiply(translate(MINIMAP_CENTER, -MINIMAP_CENTER));
    const half = MINIMAP_SIZE / 2;
    // The map's texture coordinates at the minimap's corners: the soldier at
    // the centre, mapRange to the rim, turned.
    const cu = 0.5 + ((player.position.x - frame.center[0]) * 0.5) / frame.half;
    const cv = 0.5 + ((player.position.z - frame.center[1]) * 0.5) / frame.half;
    const range = this.mapRange;
    const r = (range * 0.5) / frame.half;
    const corner = (s, t) => [cu + (cos * s - sin * t) * r, cv + (sin * s + cos * t) * r];
    batch.quadCorners(m, -half, -half, half, half, this.mapImage, [corner(-1, -1), corner(-1, 1), corner(1, -1), corner(1, 1)], alpha(WHITE, MAP_ALPHA), this.texture("map_mask"));

    // The posts, pinned inside the rim.
    const k = half / range;
    for (const post of this.posts?.posts || []) {
      if (post.mapped === false || post.active === false) continue;
      const dx = post.position.x - player.position.x;
      const dz = post.position.z - player.position.z;
      let x = (cos * dx + sin * dz) * k;
      let y = (-sin * dx + cos * dz) * k;
      const limit = half - iconSize / 2;
      const d = Math.hypot(x, y);
      if (d > limit) {
        x *= limit / d;
        y *= limit / d;
      }
      const [art, size] = this.postIcon(post, icon, iconSize);
      batch.quad(m, x - size / 2, y - size / 2, x + size / 2, y + size / 2, art, [0, 0, 1, 1], RELATION[this.posts.relation(post.team)]);
    }
    for (const v of this.unitSource ? [] : this.vehicles?.vehicles || []) {
      if (!v.alive || v.driver) continue;
      const position = v.root?.position ?? v.visual ?? v.position;
      const dx = position.x - player.position.x, dz = position.z - player.position.z;
      const x = (cos * dx + sin * dz) * k, y = (-sin * dx + cos * dz) * k;
      if (Math.hypot(x, y) > half - iconSize) continue;
      const texture = this.texture(v.def.settings.MapTexture) || arrow;
      this.drawMapMarker(m, x, y, iconSize, texture, RELATION[this.posts.relation(v.team)], mapHeading({ vehicle: v }), -turn);
    }
    this.drawMapUnits(m, (x, z) => {
      const dx = x - player.position.x, dz = z - player.position.z;
      if (dx * dx + dz * dz > range * range) return null;
      return [(cos * dx + sin * dz) * k, (-sin * dx + cos * dz) * k];
    }, -turn);
    this.drawMapPostDetails(m, (post) => {
      const dx = post.position.x - player.position.x, dz = post.position.z - player.position.z;
      const x = (cos * dx + sin * dz) * k, y = (-sin * dx + cos * dz) * k;
      const scale = Math.min(1, (half - iconSize / 2) / Math.max(Math.hypot(x, y), .001));
      return [x * scale, y * scale];
    }, iconSize);
    batch.quad(m, -PLAYER_ICON / 2, -PLAYER_ICON / 2, PLAYER_ICON / 2, PLAYER_ICON / 2, arrow, [0, 0, 1, 1], WHITE);
  }

  // The whole battlefield, north up, `size` units across and centred under
  // m: the map at half alpha, the posts in relation colours (blinking toward
  // a team taking them), the vehicles, and the radar's icons, as `team`
  // sees them (the player's team by default). The M key's map adds the
  // posts' names and the player's arrow ({ position, turn }). The spawn
  // screen's draws through a mask, with rings ({ x, z, size, alpha }: a
  // world point, the icon's size, and its highlight) around the post
  // picked.
  drawBattlefield(m, size, { team, mask = null, labels = true, player = null, rings = [], spawn = false } = {}) {
    if (!this.frame || !this.mapImage) return;
    const batch = this.batch;
    const half = size / 2;
    const icon = this.texture("hud_flag_icon");
    const arrow = this.texture("player_icon");
    const iconSize = POST_ICON * this.mapScale;
    const maskTexture = mask ? this.texture(mask) : null;
    const relation = (t) => RELATION[this.relationOf(t, team)];
    const at = (x, z) => this.mapPoint(x, z, size);
    batch.quad(m, -half, -half, half, half, this.mapImage, [0, 0, 1, 1], alpha(WHITE, MAP_ALPHA), 0, maskTexture);
    for (const post of this.posts?.posts || []) {
      if (post.mapped === false || post.active === false) continue;
      const [x, y] = at(post.position.x, post.position.z);
      const [art, postSize] = this.postIcon(post, icon, iconSize);
      batch.quad(m, x - postSize / 2, y - postSize / 2, x + postSize / 2, y + postSize / 2, art, [0, 0, 1, 1], relation(post.team));
    }
    for (const v of this.unitSource ? [] : this.vehicles?.vehicles || []) {
      if (!v.alive) continue;
      const position = v.root?.position ?? v.visual ?? v.position;
      const [x, y] = at(position.x, position.z);
      const texture = this.texture(v.def.settings.MapTexture) || arrow;
      const vehicleSize = iconSize * 1.2;
      this.drawMapMarker(m, x, y, vehicleSize, texture, relation(v.team), mapHeading({ vehicle: v }));
    }
    this.drawMapUnits(m, at, 0);
    this.drawMapPostDetails(m, (post) => at(post.position.x, post.position.z), iconSize, { team, labels, spawn });
    for (const ring of rings) {
      if (!(ring.alpha > 0)) continue;
      const [x, y] = at(ring.x, ring.z);
      this.drawRing(m, x, y, RING_SCALE * ((ring.size ?? iconSize) + 3 * ring.alpha), alpha(GREEN, ring.alpha), 1.5);
    }
    if (player) {
      const [px, py] = at(player.position.x, player.position.z);
      const face = m.clone().multiply(translate(px, py)).multiply(new THREE.Matrix4().makeRotationZ(player.turn));
      batch.quad(face, -PLAYER_ICON / 2, -PLAYER_ICON / 2, PLAYER_ICON / 2, PLAYER_ICON / 2, arrow, [0, 0, 1, 1], WHITE);
    }
  }

  // The spawn screen's map (view: the layer's battlefield).
  drawSpawnMap(view) {
    if (!view) return;
    const place = this.spawnMapPlace();
    const m = this.anchor(0, 0, false).multiply(translate(place.x, place.y));
    this.drawBattlefield(m, place.size, { team: view.team, mask: "map_mask", labels: false, rings: view.rings, spawn: true });
  }

  // A thin circle of `radius` units around (x, y) under m.
  drawRing(m, x, y, radius, color, width = 2) {
    const steps = 32;
    for (let i = 0; i < steps; i++) {
      const a0 = (i / steps) * 2 * Math.PI;
      const a1 = ((i + 1) / steps) * 2 * Math.PI;
      const ax = x + radius * Math.cos(a0), ay = y + radius * Math.sin(a0);
      const bx = x + radius * Math.cos(a1), by = y + radius * Math.sin(a1);
      const segment = m.clone().multiply(translate(ax, ay)).multiply(new THREE.Matrix4().makeRotationZ(Math.atan2(by - ay, bx - ax)));
      this.batch.quad(segment, 0, -width / 2, Math.hypot(bx - ax, by - ay) + 0.5, width / 2, this.solid, [0, 0, 1, 1], color);
    }
  }

  drawMapUnits(m, at, turn) {
    for (const icon of this.radar.icons) {
      // HUD eligibility runs before bodies are posed; sample the latest drawn
      // frame here so both map views track the same smooth pose as the world.
      const position = mapPosition(icon.unit);
      const point = at(position.x, position.z);
      if (!point) continue;
      icon.yaw = mapHeading(icon.unit, icon.yaw);
      this.drawMapMarker(m, point[0], point[1], icon.size, this.texture(icon.texture), icon.color, icon.yaw, turn);
    }
  }

  drawMapMarker(m, x, y, size, texture, color, yaw, turn = 0) {
    const half = size / 2;
    const face = m.clone().multiply(translate(x, y)).multiply(new THREE.Matrix4().makeRotationZ(mapIconRotation(yaw, turn)));
    this.batch.quad(face, -half, -half, half, half, texture, [0, 0, 1, 1], color);
  }

  // A post's map icon: its class's MapTexture (a destructible objective's
  // hud_dest_icon), sized by its MapScale against the capture posts' 2.5,
  // else the flag. A destroyed objective is off the maps.
  postIcon(post, flag, size) {
    const art = (post.desc?.mapTexture && this.texture(post.desc.mapTexture)) || flag;
    return [art, art === flag ? size : size * (post.desc?.mapScale ?? 2.5) / 2.5];
  }

  // team: whose view (the player's by default); labels: the posts' names;
  // spawn: the spawn screen's blink.
  drawMapPostDetails(m, at, size, { team, labels = true, spawn = false } = {}) {
    const posts = this.posts?.posts || [];
    for (const post of posts) {
      if (post.mapped === false || post.active === false) continue;
      const color = this.postColor(post, team, spawn);
      if (color === RELATION[this.relationOf(post.team, team)]) continue;
      const [x, y] = at(post);
      this.batch.quad(m, x - size / 2, y - size / 2, x + size / 2, y + size / 2, this.texture("hud_flag_icon"), [0, 0, 1, 1], color);
    }
    if (!labels) return;
    for (const post of posts) {
      if (!post.label || post.mapped === false || post.active === false) continue;
      const [x, y] = at(post);
      this.fonts.draw(this.batch, m, x, y + size / 2, String(post.label).toUpperCase(), "gamefont_tiny", this.postColor(post, team, spawn), 2, .5);
    }
  }

  // A post in its owner's relation colour (as `team` sees it), blinking
  // toward the colour of a team taking it: every other quarter second, or
  // on the spawn screen the first quarter of each second, as the original's
  // spawn map does.
  postColor(post, team, spawn = false) {
    const color = RELATION[this.relationOf(post.team, team)];
    const taking = post.team ? (post.neutralize > 0 ? [...post.counts.keys()].find((t) => t !== post.team) : null) : post.progress > 0 ? post.toward : null;
    if (!taking) return color;
    const lit = spawn ? this.time % 1 < 0.25 : Math.floor(this.time * 4) % 2;
    return lit ? RELATION[this.relationOf(taking, team)] : color;
  }

  drawMessages() {
    const lh = this.fonts.lineHeight("gamefont_tiny");
    // Keep the command list legible while both bottom-right overlays live.
    const m = this.anchor(1, 0.9).multiply(translate(0, this.commandLeft > 0 ? -76 : 0));
    const lines = this.messages;
    lines.forEach((line, i) => {
      const fromBottom = lines.length - 1 - i;
      const y = -lh - fromBottom * MESSAGE_PITCH + this.scroll;
      const a = Math.min((MESSAGE_LIFE - line.age) / 0.5, 1);
      this.fonts.draw(this.batch, m, 0, y, line.text, "gamefont_tiny", alpha(line.color, a), 1);
    });
  }

  drawDamage() {
    const texture = this.texture("ambush_icon");
    DAMAGE.forEach(([ax, ay, angle], i) => {
      const a = this.damage[i];
      if (a <= 0) return;
      const m = this.anchor(ax, ay).multiply(new THREE.Matrix4().makeRotationZ(angle));
      const [x0, y0, x1, y1] = aligned(128, 16, 1, 2);
      this.batch.quad(m, x0, y0, x1, y1, texture, [0, 0, 1, 1], alpha(WHITE, a));
    });
  }

  // A quarter of the overlay per screen quarter, its (1, 1) corner at the
  // centre.
  drawScope() {
    const texture = this.texture(this.scope);
    if (!texture) return;
    const m = this.anchor(0.5, 0.5, false);
    const w = IW / 2;
    const h = this.height / 2;
    this.batch.quad(m, -w, -h, 0, 0, texture, [0, 0, 1, 1], WHITE);
    this.batch.quad(m, w, -h, 0, 0, texture, [0, 0, 1, 1], WHITE);
    this.batch.quad(m, -w, h, 0, 0, texture, [0, 0, 1, 1], WHITE);
    this.batch.quad(m, w, h, 0, 0, texture, [0, 0, 1, 1], WHITE);
  }

  // Outside the battlefield: core.lvl's LEAVING BATTLEFIELD, centred on the
  // screen in gamefont_large, flashing in the HUD's red.
  drawLeaving() {
    if (!(this.leaving > 0)) return;
    const font = "gamefont_large";
    const text = this.strings["game.tooltips.leaving_battlefield"] || "LEAVING BATTLEFIELD";
    this.fonts.draw(this.batch, this.anchor(0.5, 0.5), 0, -this.fonts.lineHeight(font) / 2, text, font, alpha(RED, this.leaving), 2);
  }

  drawBanner() {
    if (!this.banner) return;
    this.fonts.draw(this.batch, this.anchor(0.5, 0.35), 0, 0, this.banner.text, "gamefont_large", this.banner.color, 2, 1.5);
  }
}
