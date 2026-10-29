import * as THREE from "three";
import { ANGLE, dot32, isCannon, isLauncher, leadPoint, lengthSq32, lookupCos, normalizeOrX } from "./aim.js";

// How a Classic bot uses its weapons, following the 2004 game: what it
// knows about each weapon (ranges, target masks, burst lengths), which weapon
// it picks for a target, and how its controller's two fire requests become
// trigger presses (docs/ai-combat.md).
//
// The weapons themselves are the player's (src/weapons.js): same clips,
// heat, reloads and projectiles. What differs for a bot is decided here and
// in aim.js: the spread rules ({ player: false } keeps no head start below
// zero; SpreadPerShot is multiplied by the side's curve B once per salvo; a
// zoomed single-shot weapon picks up extra yaw from curve S by distance),
// the random draws (the shared ClassicRandom, pitch pair before yaw pair),
// and when the trigger is down.
//
// Fire requests. A controller has two request bytes: A fires directly; B
// asks for validated fire, which goes ahead only while the selected weapon's
// target is the controller's target, the lead point is in the aim cone and
// the target is in range. The request is then shaped by the weapon: a
// charging weapon holds for a drawn time; a single-shot weapon presses on
// alternate pairs of updates (and a zoomed one only every 2-3 s); an
// automatic cannon fires bursts of BarrageMin..BarrageMax rounds with
// BarrageDelay pauses, stopping short of overheating. A request that would
// put a round through a nearby allied or neutral human is dropped. The
// result drives a run-length button: counts above zero are held.
//
// Bots never reload by choice, never zoom and never wait for a lock: the
// ordinary AI path writes only the fire buttons (empty clips reload by
// themselves, as for everyone), and a launcher fires unguided if its shot
// comes before the lock completes.

const f = Math.fround;
const INT_MAX = 2147483647;

// Generator A (world.randomA, else world.random), which every draw here uses.
function rngA(world) {
  return world.randomA ?? world.random;
}
const DEG = 180 / Math.PI;

// ---------------------------------------------------------------------------
// Weapon AI properties. ai.json (tools/zero_ai_classes.py) records each
// weapon's AI keys as the class parser leaves them; attachAIData() hangs
// them on the units.json weapon records as `def.ai`. Without ai.json the
// engine's WeaponClass defaults apply.

export const ENGINE_DEFAULTS = Object.freeze({
  minRange: 0,
  optimalRange: 30,
  maxRange: 60,
  implicitRange: true, // MaxRange not authored: doubles for non-infantry owners
  extremeRange: 0, // the larger of ExtremeRange and LockOnRange as parsed
  relationMask: 1, // TargetEnemy
  categoryMask: 0xffffff9f, // everything but dead and unbuilt buildings
  barrageMin: 3,
  barrageMax: 6,
  barrageDelay: 2,
});

let aiCache = new WeakMap();
const registered = new Map(); // weapon class name -> ai.json weapon record

// A weapon class record's AI view (charge levels are separate records): its
// own `ai` (attachAIData), else the registered ai.json record of its class
// (registerAIData), else the engine defaults and the record's flags.
export function weaponAI(def) {
  if (!def) return null;
  let a = aiCache.get(def);
  if (a) return a;
  const rec = def.ai ?? registeredAI(def);
  const ai = rec || {};
  // Without a record, the flattened keys a class record may carry.
  const raw = rec ? null : def;
  const flags = rec ? null : maskFromFlags(def);
  a = {
    minRange: f(ai.minRange ?? num(raw?.MinRange) ?? ENGINE_DEFAULTS.minRange),
    optimalRange: f(ai.optimalRange ?? num(raw?.OptimalRange) ?? ENGINE_DEFAULTS.optimalRange),
    maxRange: f(ai.maxRange ?? num(raw?.MaxRange) ?? ENGINE_DEFAULTS.maxRange),
    implicitRange: ai.implicitRange ?? (num(raw?.MaxRange) === undefined ? ENGINE_DEFAULTS.implicitRange : false),
    extremeRange: f(ai.extremeRange ?? Math.max(num(def.ExtremeRange) || 0, num(def.LockOnRange) || 0)),
    relationMask: (ai.relationMask ?? flags?.relation ?? ENGINE_DEFAULTS.relationMask) >>> 0,
    categoryMask: (ai.categoryMask ?? flags?.category ?? ENGINE_DEFAULTS.categoryMask) >>> 0,
    barrageMin: ai.barrageMin ?? num(raw?.BarrageMin) ?? ENGINE_DEFAULTS.barrageMin,
    barrageMax: ai.barrageMax ?? num(raw?.BarrageMax) ?? ENGINE_DEFAULTS.barrageMax,
    barrageDelay: f(ai.barrageDelay ?? num(raw?.BarrageDelay) ?? ENGINE_DEFAULTS.barrageDelay),
    // WeaponMelee consumes the button's first update,
    // even though the generic class's authored TriggerSingle is absent.
    // Port adapter: pulse its held fire request through the single-shot
    // consumer so later swings receive a new press.
    triggerSingle: def.base === "melee" || (ai.triggerSingle ?? (def.TriggerSingle > 0)),
    zoomMax: f(ai.zoomMax ?? (def.ZoomMax || 1)),
    action: ai.action ?? actionOf(def.AnimationBank),
  };
  aiCache.set(def, a);
  return a;
}

// A flattened key's number (records keep numbers or authored strings).
function num(value) {
  if (value === undefined || value === null || value === "") return undefined;
  const n = Number.parseFloat(value);
  return Number.isFinite(n) ? n : undefined;
}

// Target masks from the flattened Target* flags units.json keeps, for when
// ai.json is absent: TargetAll first (it sets or clears the whole category
// mask), then each flag its bit. ai.json replays the authored order instead.
const RELATION_FLAGS = [["TargetEnemy", 1], ["TargetNeutral", 2], ["TargetFriendly", 4]];
const CATEGORY_FLAGS = [["TargetPerson", 1], ["TargetAnimal", 2], ["TargetDroid", 4], ["TargetVehicle", 8],
  ["TargetBuilding", 16], ["TargetBuildingDead", 32], ["TargetBuildingUnbuilt", 64]];

function maskFromFlags(def) {
  let relation = ENGINE_DEFAULTS.relationMask;
  let category = ENGINE_DEFAULTS.categoryMask;
  if (def.TargetAll !== undefined) category = Number(def.TargetAll) ? 0xffffffff : 0;
  for (const [key, bit] of RELATION_FLAGS) if (def[key] !== undefined) relation = Number(def[key]) ? relation | bit : relation & ~bit;
  for (const [key, bit] of CATEGORY_FLAGS) if (def[key] !== undefined) category = Number(def[key]) ? category | bit : category & ~bit;
  return { relation: relation >>> 0, category: category >>> 0 };
}

// AnimationBank to the AI's action class (through the bank enum
// the parser builds: rifle 0, bazooka 1, pistol 2, grenade 3, tool 4,
// lightsabre 5, sbdroid 6, bdroid 7, cannon 8, gam 9; anything else 0).
const BANKS = ["rifle", "bazooka", "pistol", "grenade", "tool", "lightsabre", "sbdroid", "bdroid", "cannon", "gam"];
const BANK_ACTION = [1, 3, 0, 2, 6, 4, 1, 1, 3, 4];

export function actionOf(bank) {
  const i = BANKS.indexOf(String(bank ?? "").toLowerCase());
  return BANK_ACTION[i < 0 ? 0 : i];
}

export const ACTION = Object.freeze({ PISTOL: 0, RIFLE: 1, GRENADE: 2, LAUNCHER: 3, MELEE: 4, TOOL: 6 });

// A registered record's AI view for a class record or one of its charge
// levels (a level carries the `at` its units.json charge entry has).
function registeredAI(def) {
  const rec = registered.get(def.class);
  if (!rec) return null;
  if (def.at !== undefined && rec.levels?.length) {
    const at = f(def.at);
    const level = rec.levels.find((l) => f(l.at) === at);
    if (level?.ai) return level.ai;
  }
  return rec.ai ?? null;
}

// Make a side's ai.json weapon records the AI view of every weapon of
// those classes (looked up by class name, charge levels by their `at`; a
// record's own `ai` still wins). Returns the number of records registered.
export function registerAIData(aiJson) {
  if (!aiJson?.weapons) return 0;
  let n = 0;
  for (const [name, rec] of Object.entries(aiJson.weapons)) {
    if (!rec?.ai) continue;
    registered.set(name, rec);
    n++;
  }
  aiCache = new WeakMap();
  return n;
}

// Hang ai.json's weapon records on the units.json classes of a side
// (units[class].weapons[i].ai, and each charge level's), so that bots read
// them through weaponAI(). `units`: units.json, or its `units` map. Returns
// the number of weapons matched.
export function attachAIData(units, aiJson) {
  if (!units || !aiJson?.weapons) return 0;
  if (units.units && typeof units.units === "object") units = units.units;
  let matched = 0;
  for (const unitDef of Object.values(units)) {
    for (const w of unitDef.weapons || []) {
      const rec = aiJson.weapons[w.class];
      if (!rec) continue;
      w.ai = rec.ai;
      (w.charges || []).forEach((level, i) => {
        if (rec.levels?.[i]?.ai) level.ai = rec.levels[i].ai;
      });
      aiCache.delete(w);
      matched++;
    }
    const cls = aiJson.classes?.[unitDef.class];
    if (cls) unitDef.ai = cls.ai;
  }
  return matched;
}

// ---------------------------------------------------------------------------
// Entity facts the weapon code reads.

// Soldier-like owners (EntitySoldier, EntityDroideka, EntityDroid) do not
// double an implicit MaxRange.
function infantryKind(kind) {
  return kind === 0 || kind === 1 || kind === 3;
}

// The nominal forward speed: a soldier's MaxSpeed,
// a vehicle's ForwardSpeed.
export function forwardSpeed(unit) {
  const c = unit?.classDef;
  return f(c?.ForwardSpeed ?? c?.MaxSpeed ?? 0);
}

// The target category the masks test (the health type):
// person 0, animal 1, droid 2, vehicle 3, building 4, dead building 5,
// unbuilt building 6.
const CATEGORIES = { person: 0, animal: 1, droid: 2, vehicle: 3, building: 4 };

export function categoryOf(unit) {
  if (Number.isInteger(unit?.category)) return unit.category;
  const type = String(unit?.classDef?.HealthType ?? "").toLowerCase();
  if (type in CATEGORIES) return CATEGORIES[type];
  const k = unit?.kind;
  if (k === 2 || k === 5 || k === 6) return 3;
  if (k === 8 || k === 9) return 4;
  return 0;
}

// Relationship bit for the masks: enemy 1, neutral 2, friend 4.
export function relationBit(relation) {
  return 1 << (relation + 1);
}

// ---------------------------------------------------------------------------
// Ranges.

// MaxRange, doubled when implicit for a non-infantry owner, scaled
// by the controller's range scale (2 while sniping from a hint); a scaled
// range above the class's extreme range gives max(extreme, unscaled).
export function effectiveRange(unit, controller, def) {
  const a = weaponAI(def);
  if (!a) return 0;
  let r = a.maxRange;
  if (a.implicitRange && !infantryKind(unit?.kind)) r = f(r + r);
  const k = f(controller?.rangeScale ?? 1);
  const kr = f(k * r);
  if (kr <= a.extremeRange) return kr;
  return a.extremeRange <= r ? r : a.extremeRange;
}

// The range plus five seconds of the owner's nominal speed.
export function expandedRange(unit, controller, def) {
  return f(effectiveRange(unit, controller, def) + f(forwardSpeed(unit) * 5));
}

// The largest expanded range over the inventory and its charge
// levels (the scanner's radius before its 75..350 clamp).
export function maxExpandedRange(unit, controller) {
  let best = 0;
  for (const weapon of unit?.combat?.arsenal?.weapons || []) {
    for (const level of weapon.levels || [weapon.def]) {
      const r = expandedRange(unit, controller, level);
      if (best <= r) best = r;
    }
  }
  return best;
}

// The selected weapon's expanded range: what the acoustic and
// alert responses and the combat agent's give-up distance call the bot's
// sensor range.
export function sensorRange(unit, controller) {
  const weapon = selectedWeapon(unit, controller);
  return weapon ? expandedRange(unit, controller, weapon.def) : 0;
}

// The band a bot keeps from its target (AttackPatterns): lower
// min(Optimal, Min + 10); upper min(max(effective - 10, Optimal), sight range
// of the target - 10).
export function lowerBand(unit, controller) {
  const a = weaponAI(selectedWeapon(unit, controller)?.def);
  if (!a) return 0;
  const lo = f(a.minRange + 10);
  return a.optimalRange <= lo ? a.optimalRange : lo;
}

export function upperBand(unit, controller, sight) {
  const weapon = selectedWeapon(unit, controller);
  const a = weaponAI(weapon?.def);
  if (!a) return 0;
  const e = f(effectiveRange(unit, controller, weapon.def) - 10);
  const hi = e <= a.optimalRange ? a.optimalRange : e;
  const s = f(sight - 10);
  return s <= hi ? s : hi;
}

// ---------------------------------------------------------------------------
// Ammunition.

// Rounds left, clip and reserve; INT_MAX for a bottomless clip or
// unlimited reserve.
export function ammoCount(weapon) {
  if (!weapon) return 0;
  if (weapon.bottomless || weapon.reserve === Infinity) return INT_MAX;
  return Math.trunc(f(f(weapon.clip + weapon.reserve) * weapon.rounds));
}

// The weapon's starting capacity in rounds (clips x RoundsPerClip),
// not what is left: unlimited clips count as FLT_MAX clips.
export function capacity(weapon) {
  const clips = weapon?.def?.ammo > 0 ? weapon.def.ammo : 3.4028234663852886e38;
  const rounds = weapon?.bottomless ? INT_MAX : weapon?.rounds ?? 1;
  return f(clips * rounds);
}

// ---------------------------------------------------------------------------
// Weapon utility: whether a weapon is worth using on a target and
// a comparison score.

const SCALE_KEYS = ["PersonScale", "AnimalScale", "DroidScale", "VehicleScale", "BuildingScale"];

function typeScale(rec, category) {
  const key = SCALE_KEYS[category];
  return f(key ? rec?.[key] ?? 1 : 1);
}

// Falloff at `distance` less half the target's bound radius.
function blastFalloff(distance, inner, outer, radius) {
  const d = radius > 0 ? f(distance - f(radius * 0.5)) : f(distance);
  if (d < inner) return 1;
  if (d < outer) return f(f(outer - d) / f(outer - inner));
  return 0;
}

// An explosion's damage on this target type at 2 units.
function blastDamage(explosion, category, radius) {
  if (!explosion || typeof explosion !== "object") return 0;
  const fall = blastFalloff(2, f(explosion.DamageRadiusInner || 0), f(explosion.DamageRadiusOuter || 0), radius);
  const value = f(fall * f(explosion.Damage || 0));
  return f(typeScale(explosion, category) * value);
}

const GRENADE_ORDNANCE = new Set(["grenade"]);
const STICKY_ORDNANCE = new Set(["sticky", "haywire", "beacon"]);
const FALLBACK_ORDNANCE = new Set(["beam", "missile", "shell"]);

// Returns { ok, score }. owner/target: units.
export function weaponUtility(owner, weapon, target, out = { ok: false, score: 0 }) {
  out.ok = false;
  out.score = 0;
  const def = weapon?.def;
  if (!def || !target) return out;
  const ordnance = def.ordnance || null;
  const ownerSoldier = owner.kind === 0;
  const targetKind = target.kind;
  // A no-ordnance melee weapon against a soldier or droideka: 1000.
  if (!ordnance && def.base === "melee" && (targetKind === 0 || targetKind === 3)) return accept(out, 1000);
  // A hero soldier with a cannon: 1000.
  if (ownerSoldier && owner.hero && isCannon(def)) return accept(out, 1000);
  if (target.trap) return out;
  // Soldier on soldier: the weapon's category mask decides (the only pair
  // the 2004 game tests it for here). Elsewhere a mask that leaves the
  // target out only keeps the weapon's own target (SetTarget's weapon target,
  // what a launcher's lock follows) clear; the fire requests go by the
  // controllable's target (validateFire), so the weapon still fires. The
  // Hoth turrets' cannons clear every category bit: tested for every target
  // (as the port once did) their gunners never chose a weapon, so never
  // took a target, and sat on through every attack.
  if (ownerSoldier && targetKind === 0 && !(weaponAI(def).categoryMask & (1 << categoryOf(target)))) return out;
  if (!ordnance) return out;
  const category = categoryOf(target);
  const radius = target.sphere?.radius ?? 0;
  let damage = f(f(ordnance.Damage || 0) * typeScale(ordnance, category));
  damage = f(blastDamage(ordnance.ExplosionImpact, category, radius) + damage);
  damage = f(blastDamage(ordnance.ExplosionExpire, category, radius) + damage);
  const cap = capacity(weapon);
  const value = f((cap <= 50 ? cap : 50) * damage);
  if (f(maxHealthOf(target) * 0.5) < value) return accept(out, damage);
  // Vehicle-borne weapons accept any positive damage.
  const ok = owner.kind;
  if ((ok === 6 || ok === 2 || ok === 8 || ok === 7) && damage > 0) return accept(out, damage);
  // Otherwise only hovers, walkers and live buildings stay candidates, and
  // only for ordnance that can hurt them.
  if (targetKind !== 6 && targetKind !== 2) {
    if (!finiteHealth(target) || !(targetKind === 9 || targetKind === 8)) return out;
  }
  if (!(damage > 0)) return out;
  const base = ordnance.base;
  if (GRENADE_ORDNANCE.has(base) || STICKY_ORDNANCE.has(base)) {
    const v = target.velocity;
    if (v && lengthSq32(v.x, v.y, v.z) < 144) return accept(out, damage);
    return out;
  }
  if (FALLBACK_ORDNANCE.has(base)) return accept(out, damage);
  return out;
}

function accept(out, score) {
  out.ok = true;
  out.score = f(score);
  return out;
}

function maxHealthOf(unit) {
  return f(unit.combat?.maxHealth ?? unit.classDef?.MaxHealth ?? unit.maxHealth ?? 0);
}

function finiteHealth(unit) {
  return maxHealthOf(unit) < 3.4028234663852886e38;
}

// ---------------------------------------------------------------------------
// Weapon selection and the periodic grenade priority.

const aimAt = new THREE.Vector3();
const originAt = new THREE.Vector3();
const utility = { ok: false, score: 0 };

// Picks a weapon for `target`. Returns { ok, index, weapon, action, inBand }
// (index: the inventory index, -1 when none). switchAllowed: switch the
// controller's selection to it when it differs.
export function selectWeapon(controller, unit, target, switchAllowed, world, out = {}) {
  out.ok = false;
  out.index = -1;
  out.weapon = null;
  out.action = 6;
  out.inBand = false;
  const weapons = unit?.combat?.arsenal?.weapons || [];
  if (!target || !weapons.length) return out;
  const now = world?.time ?? 0;
  unit.origin(originAt);
  target.aimPoint(aimAt);
  const d2 = lengthSq32(f(aimAt.x - originAt.x), f(aimAt.y - originAt.y), f(aimAt.z - originAt.z));
  const ownerSoldier = unit.kind === 0;
  let best = -1;
  let bestScore = -1;
  let bestBig = false; // capacity above 20
  let bestInBand = false;
  for (let i = 0; i < weapons.length; i++) {
    const weapon = weapons[i];
    if (!(ammoCount(weapon) > 0)) continue;
    if (!weaponUtility(unit, weapon, target, utility).ok) continue;
    const score = utility.score;
    const cap = capacity(weapon);
    const big = cap > 20;
    const better = bestScore < score;
    const a = weaponAI(weapon.def);
    const range = effectiveRange(unit, controller, weapon.def);
    if (d2 <= f(a.minRange * a.minRange) || f(range * range) <= d2) {
      // Out of the band: a fallback while nothing in band has been found.
      if (bestInBand) continue;
      const far = expandedRange(unit, controller, weapon.def);
      if (!(d2 < f(far * far))) continue;
      if (bestBig ? big && better : big || better) {
        bestScore = score;
        best = i;
        bestBig = big;
      }
      continue;
    }
    // In the band: beats any fallback; among in-band picks, capacity then
    // utility.
    let take = !bestInBand || (bestBig ? big && better : big || better);
    if (grenadePriority(controller, unit, target, weapon.def, ownerSoldier, now)) take = true;
    if (take) {
      bestBig = big;
      bestInBand = true;
      bestScore = score;
      best = i;
    }
  }
  if (best < 0) return out;
  out.ok = true;
  out.index = best;
  out.weapon = weapons[best];
  out.action = weaponAI(out.weapon.def).action;
  out.inBand = bestInBand;
  if (switchAllowed && selectedIndex(unit, controller) !== best) switchWeapon(controller, unit, best);
  return out;
}

// A soldier prefers a grenade in band when its target was recently seen in a
// cluster (target controller flag within 5 s) or on its own three-second
// slot: (controller roll mod 10) == (trunc(time) / 3 mod 10).
function grenadePriority(controller, unit, target, def, ownerSoldier, now) {
  if (!ownerSoldier || !target || def.base !== "grenade") return false;
  const cluster = clusterState(target);
  if (cluster && f(now) < f(cluster.flaggedAt + 5)) return true;
  const id = controller?.roll ?? controller?.id ?? 0;
  return id % 10 === Math.trunc(Math.trunc(now) / 3) % 10;
}

// The per-target-controller cluster timestamps (last scan, flagged), kept on
// the target's controller for bots and on the unit for humans (the base
// Controller holds them in the 2004 game; provisional for humans, see
// docs/ai-combat.md).
const clusters = new WeakMap();

export function clusterState(target, create = false) {
  const key = target?.controller ?? target;
  if (!key) return null;
  let state = clusters.get(key);
  if (!state && create) {
    state = { scanAt: -Infinity, flaggedAt: -Infinity, generation: target.generation };
    clusters.set(key, state);
  }
  if (state && !target.controller && state.generation !== target.generation) {
    state.scanAt = -Infinity;
    state.flaggedAt = -Infinity;
    state.generation = target.generation;
  }
  return state;
}

// ---------------------------------------------------------------------------
// The controller's weapon state: selected channel and the trigger shaping.

export class FireControl {
  constructor() {
    this.channel = 0; // the channel the AI fires (its selected weapon's)
    this.buttons = [{ count: 0, code: 0 }, { count: 0, code: 0 }];
    this.lastRelease = -1; // charge: last release time
    this.charging = false;
    this.hold = 0; // drawn hold time
    this.sniperActive = false;
    this.sniperDeadline = 0;
    this.burst = { state: 0, heatLimit: 0, lastEnd: -100, savedAmmo: 0, count: 0 };
    this.weaponTarget = null; // the selected weapon's target ref (SetTarget, masks applied)
    this.protected = []; // protected humans (refs), at most 8
  }

  reset() {
    this.channel = 0;
    for (const b of this.buttons) {
      b.count = 0;
      b.code = 0;
    }
    this.lastRelease = -1;
    this.charging = false;
    this.hold = 0;
    this.sniperActive = false;
    this.sniperDeadline = 0;
    this.burst.state = 0;
    this.weaponTarget = null;
    this.protected.length = 0;
  }
}

function fireOf(controller) {
  return controller?.perception?.fire ?? null;
}

// The selected weapon: the current weapon of the AI's channel.
export function selectedWeapon(unit, controller = unit?.controller) {
  const arsenal = unit?.combat?.arsenal;
  if (!arsenal) return null;
  const channel = fireOf(controller)?.channel ?? 0;
  return arsenal.current(channel) || null;
}

function selectedIndex(unit, controller) {
  const weapon = selectedWeapon(unit, controller);
  return weapon ? unit.combat.arsenal.weapons.indexOf(weapon) : -1;
}

// Switch to inventory index i; resets the charge release time,
// the sniper gate and the burst.
export function switchWeapon(controller, unit, index) {
  const fire = fireOf(controller);
  const arsenal = unit?.combat?.arsenal;
  const weapon = arsenal?.weapons[index];
  if (!fire || !weapon) return false;
  fire.sniperActive = false;
  fire.lastRelease = -1;
  fire.charging = false;
  fire.burst.state = 0;
  fire.channel = weapon.channel;
  return arsenal.select ? arsenal.select(weapon) !== false : selectInChannel(arsenal, weapon);
}

// For an Arsenal without select(): cycle its channel to the weapon.
function selectInChannel(arsenal, weapon) {
  const list = arsenal.channels[weapon.channel];
  const i = list.indexOf(weapon);
  if (i < 0) return false;
  if (arsenal.current(weapon.channel) === weapon) return true;
  const old = arsenal.current(weapon.channel);
  if (old?.busy) return false;
  old?.deselect();
  arsenal.selected[weapon.channel] = i;
  return true;
}

// SetTarget: the controller's target is set as given; the selected
// weapon's own target only when its relationship and category masks allow
// (a failed check clears it).
export function setTarget(controller, unit, target, world) {
  controller.target = target ? { unit: target, generation: target.generation } : null;
  const fire = fireOf(controller);
  if (!fire) return;
  const weapon = selectedWeapon(unit, controller);
  if (!weapon) return;
  let kept = target;
  if (target) {
    const a = weaponAI(weapon.def);
    const rel = world?.units?.relation ? world.units.relation(unit.team, target.team) : -1;
    if (!(a.relationMask & relationBit(rel)) || !(a.categoryMask & (1 << categoryOf(target)))) kept = null;
  }
  fire.weaponTarget = kept ? { unit: kept, generation: kept.generation } : null;
}

// ---------------------------------------------------------------------------
// The run-length button: count in [-100, 100], > 0 held. A press
// from zero starts at 2, from a release at 1; a release from zero starts at
// -2, from a hold at -1. code: the edge/hold byte (0 held < 70, 3 held
// longer; on release, whether the hold was shorter than 70; a press from
// release turns code 1 into 2).

export function pressButton(button, down) {
  const c = button.count;
  if (!down) {
    if (c === 0) button.count = -2;
    else if (c > 0) {
      button.code = c < 70 ? 1 : 0;
      button.count = -1;
    } else button.count = Math.max(c - 1, -100);
    return button;
  }
  if (c === 0) button.count = 2;
  else if (c < 0) {
    if (button.code === 1) button.code = 2;
    button.count = 1;
  } else {
    button.code = c < 70 ? 0 : 3;
    button.count = Math.min(c + 1, 100);
  }
  return button;
}

// The weapon trigger a channel's counts make: held while above zero, pressed
// on the first held count after a release (src/units.js IntentControls does
// the same for bots' intents).
export function triggerFromCounts(previous, current, out = { held: false, pressed: false }) {
  out.held = current > 0;
  out.pressed = current > 0 && !(previous > 0);
  return out;
}

// The whole weapon input an Arsenal.update() takes, from two ticks of
// intent buttons, plus the edge fields (weapon cycling and zoom, which the
// ordinary AI never sets; src/units.js IntentControls is the in-game
// equivalent). previous: last tick's intent.buttons.
export function arsenalInput(previous, intent, out = null) {
  out ||= {
    primary: { held: false, pressed: false },
    secondary: { held: false, pressed: false },
    reload: false,
    nextPrimary: false,
    nextSecondary: false,
    zoom: false,
  };
  triggerFromCounts(previous[0], intent.buttons[0], out.primary);
  triggerFromCounts(previous[1], intent.buttons[1], out.secondary);
  out.reload = Boolean(intent.reload);
  out.nextPrimary = Boolean(intent.nextPrimary);
  out.nextSecondary = Boolean(intent.nextSecondary);
  out.zoom = Boolean(intent.zoom);
  return out;
}

// ---------------------------------------------------------------------------
// The fire-request consumer, run by AI input execution every
// update for every AI unit. Writes intent.buttons (the AI channel's button,
// the other released) and returns the shaped request.

export function consumeFire(controller, unit, world) {
  const fire = fireOf(controller);
  if (!fire || !unit) return false;
  const now = world?.time ?? 0;
  const weapon = selectedWeapon(unit, controller);
  let request = Boolean(controller.fireA);
  if (weapon) {
    if (!controller.fireA && controller.fireB) {
      const target = resolve(controller.target);
      if (target) request = validateFire(controller, unit, target, world);
    }
    if (chargeWeapon(weapon)) request = chargeRequest(fire, weapon, unit, request, now, world);
    else if (!weaponAI(weapon.def).triggerSingle) {
      if (isCannon(weapon.def)) request = burstRequest(fire, weapon, request, now, world);
    } else request = semiautoRequest(controller, fire, weapon, request, now, world);
    if (request && protectedVeto(controller, unit, weapon)) request = false;
    prepareSpread(controller, unit, weapon);
  }
  const channel = fire.channel;
  pressButton(fire.buttons[channel], request);
  pressButton(fire.buttons[1 - channel], false);
  const buttons = unit.intent?.buttons;
  if (buttons) {
    buttons[0] = fire.buttons[0].count;
    buttons[1] = fire.buttons[1].count;
  }
  return request;
}

function resolve(ref) {
  const u = ref?.unit;
  return u && u.alive && u.generation === ref.generation ? u : null;
}

// Charge handling for a cannon or grenade with MaxPressedTime > 0,
// never for a launcher.
export function chargeWeapon(weapon) {
  const def = weapon.levels?.[0] ?? weapon.def;
  if (isLauncher(def)) return false;
  if (!(def.MaxPressedTime > 0)) return false;
  return isCannon(def) || def.base === "grenade";
}

// A new charge with a falling projectile waits 4 s after the last
// release; a soldier (other than with a repair tool) draws its hold from
// 0.25..1 of the weapon timer's limit as it stands when the charge starts
// (Weapon.limit: ShotDelay after a shot, so a bowcaster in steady fire
// holds 0.09..0.35 s and fires single bolts; ReloadTime after a reload; 1 s
// new); once started the charge runs to that hold whatever the requests do,
// then lets go.
function chargeRequest(fire, weapon, unit, request, now, world) {
  if (request) {
    const gravity = weaponGravity(weapon);
    if (gravity === 0 || f(fire.lastRelease + 4) <= f(now)) {
      if (!fire.charging) {
        fire.charging = true;
        const most = f(weapon.limit ?? weapon.levels?.[0]?.MaxPressedTime ?? weapon.def.MaxPressedTime);
        fire.hold = most;
        if (unit.kind === 0 && weapon.def.base !== "repair") fire.hold = rngA(world).range(f(most * 0.25), most);
      }
    } else request = false;
  }
  if (!fire.charging) return false;
  if (weapon.charging && fire.hold <= f(weapon.held)) {
    fire.lastRelease = now;
    fire.charging = false;
    return false;
  }
  return true;
}

function weaponGravity(weapon) {
  const o = weapon.def?.ordnance;
  if (!o) return 0;
  return o.base === "shell" || o.base === "grenade" || o.base === "sticky" || o.base === "haywire" || o.base === "beacon" ? -1 : 0;
}

// The semi-automatic request, with the sniper gate first. The request
// holds only while the last count was exactly +1 (one held update), so a
// standing request presses two updates and releases two. A dispenser's or
// shield's just-released request clears both request bytes.
function semiautoRequest(controller, fire, weapon, request, now, world) {
  if (sniperWeapon(weapon)) request = sniperGate(fire, weapon, request, now, world);
  if (!request) return false;
  const button = fire.buttons[fire.channel];
  if (button.count > 0) return button.count === 1;
  if (button.count === -1) {
    const base = weapon.def.base;
    if (base === "dispenser" || base === "remote" || base === "detonator" || base === "shield") {
      controller.fireA = false;
      controller.fireB = false;
    }
    return false;
  }
  return true;
}

// TriggerSingle with ZoomMax >= 6.
export function sniperWeapon(weapon) {
  const a = weaponAI(weapon?.def);
  return Boolean(a?.triggerSingle && a.zoomMax >= 6);
}

// One shot request per 2..3 s, and only with the weapon idle.
function sniperGate(fire, weapon, request, now, world) {
  if (fire.sniperActive && f(fire.sniperDeadline) < f(now)) fire.sniperActive = false;
  if (!fire.sniperActive && request && weaponIdle(weapon)) {
    fire.sniperActive = true;
    fire.sniperDeadline = f(f(now) + rngA(world).range(2, 3));
    return true;
  }
  return false;
}

// The weapon is ready: not reloading, charging, throwing, or in its shot
// delay.
function weaponIdle(weapon) {
  return !weapon.reloading && !weapon.charging && !weapon.busy && !(weapon.wait > 1e-6) && !weapon.overheated;
}

// Bursts for an automatic cannon. Draw N in [BarrageMin,
// BarrageMax]; after BarrageDelay since the last burst, start only if
// ceil(N / ShotsPerSalvo) salvos of heat fit under 1 - heat; fire while the
// heat is under its limit and fewer than N rounds have gone.
function burstRequest(fire, weapon, request, now, world) {
  const b = fire.burst;
  if (!request) {
    b.state = 0;
    return false;
  }
  const heat = f(weapon.heat);
  const ammo = ammoCount(weapon);
  const a = weaponAI(weapon.def);
  const single = a.triggerSingle;
  if (b.state === 0) {
    b.heatLimit = 0;
    b.lastEnd = -100;
    b.savedAmmo = 0;
    b.count = 0;
    b.state = 1;
  }
  if (b.state === 1) {
    b.count = single ? 1 : rngA(world).int(a.barrageMin, a.barrageMax);
    b.state = 2;
  }
  if (b.state === 2 && f(f(a.barrageDelay) + f(b.lastEnd)) < f(now)) {
    const shots = Math.max(weapon.def.ShotsPerSalvo || 1, 1);
    const salvoHeat = f(f(shots) * f(weapon.def.HeatPerShot || 0));
    // The 2004 game multiplies and compares in single precision: needed
    // above 1 becomes 0.99, and the burst starts only while needed < 1 -
    // heat. A burst that exactly fills the gauge (4 rounds of the Hoth turret
    // cannon's 0.25, 5 of a speeder bike's 0.2, which rounds to 1) is
    // neither: it never starts, and with heat cooled to exactly 0 (it is
    // clamped there) and its target in view, the request never drops to
    // redraw the count, so the gunner aims and never fires again. Port: a
    // burst that fills the gauge counts as one above it (0.99), so it starts
    // once the heat is all but gone.
    let needed = f(salvoHeat * Math.ceil(f(b.count / f(shots))));
    if (needed >= 1) needed = f(0.99);
    if (needed < f(1 - heat)) {
      b.savedAmmo = ammo;
      b.state = 3;
      b.heatLimit = f(f(1 - (salvoHeat >= 1 ? f(0.99) : salvoHeat)) - f(0.0001));
    }
  }
  if (b.state === 3) {
    if (heat < b.heatLimit && b.savedAmmo - ammo < b.count) return true;
    b.lastEnd = now;
    b.state = 1;
  }
  return false;
}

// ---------------------------------------------------------------------------
// Fire validation for request B.

const leadAt = new THREE.Vector3();
const pointAt = new THREE.Vector3();
const n3 = { x: 0, y: 0, z: 0 };
const fwd3 = new THREE.Vector3();

export function validateFire(controller, unit, target, world) {
  const fire = fireOf(controller);
  const weapon = selectedWeapon(unit, controller);
  const aimer = controller.perception?.aimer;
  if (!fire || !weapon || !aimer) return false;
  // The target is compared with the controllable's own, which SetTarget
  // writes whatever the weapon's masks say, not with the weapon's masked
  // target.
  if (resolve(controller.target) !== target) return false;
  leadPoint(0, unit, target, weapon, leadAt);
  target.aimPoint(pointAt);
  return facingAndRange(controller, unit, leadAt, pointAt);
}

// The lead point in the firing cone and the target's aim point
// inside the selected weapon's effective range.
export function facingAndRange(controller, unit, lead, point) {
  if (!firingCone(controller, unit, lead)) return false;
  const weapon = selectedWeapon(unit, controller);
  const range = Math.min(effectiveRange(unit, controller, weapon.def), harpoonReach(weapon.def));
  unit.origin(originAt);
  const d2 = lengthSq32(f(point.x - originAt.x), f(point.y - originAt.y), f(point.z - originAt.z));
  return d2 < f(range * range);
}

// How far the snowspeeder's harpoon reaches: the port casts it LockOnRange
// along the gun, at least 20 m (Vehicles.launchTow), where the 2004 game's
// flies as a round (Velocity 250 for LifeSpan 2.5). The AI's range for the
// tow gun is its MaxRange, 300 m, so a bot fired harpoons at walkers it
// could not reach, each one costing the gun its 3 s ShotDelay. Port: a bot
// fires one only inside this reach. Infinity for any other weapon.
export function harpoonReach(def) {
  return def?.ordnance?.base === "towcable" ? Math.max(20, def.LockOnRange || 0) : Infinity;
}

let fireCos = null;

// The body's forward within 22.5 deg of the point (45 deg with a
// melee weapon), the Aimer's direction within as much of the forward
// (skipped for melee), and beyond 50 units the point within 20 units of the
// forward line. A soldier's zoomed single-shot weapon instead needs the point
// within 0.25 units of the forward line.
export function firingCone(controller, unit, point) {
  const weapon = selectedWeapon(unit, controller);
  const aimer = controller.perception?.aimer;
  if (!weapon || !aimer) return false;
  fireCos ||= { normal: lookupCos(ANGLE.FIRE), melee: lookupCos(ANGLE.MELEE) };
  unit.origin(originAt);
  unit.forward(fwd3);
  const rx = f(point.x - originAt.x), ry = f(point.y - originAt.y), rz = f(point.z - originAt.z);
  if (sniperWeapon(weapon) && unit.kind === 0) return perpSq(rx, ry, rz, fwd3) < 0.0625;
  normalizeOrX(rx, ry, rz, n3);
  const melee = weapon.def.base === "melee";
  const limit = melee ? fireCos.melee : fireCos.normal;
  if (dot32(fwd3.x, fwd3.y, fwd3.z, n3.x, n3.y, n3.z) <= limit) return false;
  const q = aimer.direction;
  normalizeOrX(q.x, q.y, q.z, n3);
  if (dot32(n3.x, n3.y, n3.z, fwd3.x, fwd3.y, fwd3.z) <= limit && !melee) return false;
  if (lengthSq32(rx, ry, rz) < 2500) return true;
  return perpSq(rx, ry, rz, fwd3) < 400;
}

// |r x d|^2 in float32.
function perpSq(rx, ry, rz, d) {
  const cx = f(f(ry * f(d.z)) - f(rz * f(d.y)));
  const cy = f(f(rz * f(d.x)) - f(rx * f(d.z)));
  const cz = f(f(rx * f(d.y)) - f(ry * f(d.x)));
  return lengthSq32(cx, cy, cz);
}

// ---------------------------------------------------------------------------
// Friendly-human protection.

// Once per scheduler cycle while there are fewer than 9 humans:
// every AI soldier within 50 (sphere-expanded) of an allied or neutral
// human, and every AI seat on its vehicle, keeps that human (up to 8).
const near = [];

export function rebuildProtectedHumans(world) {
  const units = world.units;
  const humans = [];
  for (const u of units.living()) if (u.human) humans.push(u);
  if (humans.length >= 9) return;
  for (const u of units.living()) {
    const fire = fireOf(u.controller);
    if (fire) fire.protected.length = 0;
  }
  for (const human of humans) {
    units.query(human.position, 50, near, (u) => u !== human && units.relation(human.team, u.team) >= 0);
    for (let i = 0, n = Math.min(near.length, 20); i < n; i++) {
      const fire = fireOf(near[i].controller);
      if (fire && fire.protected.length < 8) fire.protected.push({ unit: human, generation: human.generation });
    }
  }
}

// Drop the request when the body's forward ray (from the aim
// origin) passes through a protected human's bound sphere ahead of it.
// Repair tools and dispensers skip the check.
const cross = { x: 0, y: 0, z: 0 };

export function protectedVeto(controller, unit, weapon) {
  const base = weapon?.def?.base;
  if (base === "repair" || base === "dispenser" || base === "remote" || base === "detonator") return false;
  const fire = fireOf(controller);
  if (!fire?.protected.length) return false;
  unit.origin(originAt);
  unit.forward(fwd3);
  for (const ref of fire.protected) {
    const human = resolve(ref);
    if (!human) continue;
    const s = human.sphere;
    const rx = f(s.center.x - originAt.x), ry = f(s.center.y - originAt.y), rz = f(s.center.z - originAt.z);
    cross.x = f(f(f(fwd3.y) * rz) - f(f(fwd3.z) * ry));
    cross.y = f(f(f(fwd3.z) * rx) - f(f(fwd3.x) * rz));
    cross.z = f(f(f(fwd3.x) * ry) - f(f(fwd3.y) * rx));
    const r = f(s.radius);
    if (lengthSq32(cross.x, cross.y, cross.z) < f(r * r) && dot32(fwd3.x, fwd3.y, fwd3.z, rx, ry, rz) > 0) return true;
  }
  return false;
}

// ---------------------------------------------------------------------------
// Spread: the zoomed single-shot weapon's extra yaw for a bot (the yaw
// getter's AI branch). Applies only with no authored YawSpread, a
// non-player owner, TriggerSingle, ZoomMax >= 6 and a current target: the
// yaw spread becomes S x a radian step by the distance between the owner's
// and target's positions. Set here, in degrees as the port's spreads are,
// for the weapon's next update.

const SNIPER_STEPS = [[400, 0.0997], [1600, 0.0499], [3600, 0.0333], [6400, 0.025], [10000, 0.02], [14400, 0.0167]].map(([d, y]) => [f(d), f(y)]);
const SNIPER_FAR = f(0.0143);

export function sniperYaw(distanceSq) {
  for (const [limit, yaw] of SNIPER_STEPS) if (distanceSq < limit) return yaw;
  return SNIPER_FAR;
}

export function prepareSpread(controller, unit, weapon) {
  if (!weapon) return;
  weapon.yawSpreadOverride = null;
  if (unit.human || (weapon.def.YawSpread || 0) !== 0 || !sniperWeapon(weapon)) return;
  const target = resolve(controller.target);
  if (!target) return;
  const p = unit.position, q = target.position;
  const d2 = lengthSq32(f(p.x - q.x), f(p.y - q.y), f(p.z - q.z));
  weapon.yawSpreadOverride = f(f(weapon.sniperScale ?? 0) * sniperYaw(d2)) * DEG;
}

// Give an AI-owned arsenal the shared generator for its spread draws (the
// 2004 game draws them from the same generator as AI decisions). The
// player's weapons keep Math.random.
export function useClassicRandom(arsenal, random) {
  const draw = () => random.uniform();
  for (const weapon of arsenal?.weapons || []) weapon.random = draw;
}
