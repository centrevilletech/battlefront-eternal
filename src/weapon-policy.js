// Shared weapon class facts for target previews, prediction and cold native
// definition loading. Bot decisions and fire timing remain in the AI owner.
// The cache and registration order are shared by every reader of a class.
const f = Math.fround;

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

