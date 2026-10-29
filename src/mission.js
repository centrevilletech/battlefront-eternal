// The battle a map plays. An import holds every battle fought in a map's
// world (map.json `missions`: each era's, and the historical campaign's
// versions of them), the world itself once. A battle is one of them, picked
// here and only here: the page, the world worker, a dedicated server and the
// desktop host each load a map through selectMission with the battle's URL
// (or a server's query), so they play the same mission, and what follows
// from it (the network content fingerprint, src/runtime/content-hash.js; the
// entity registry; the AI's mission data, selectAIMission) follows it too.
// Pure: no fetches, no globals.
//
// The pick, from the query:
// - mission=<name> names one (a historical version is only ever played this
//   way, for a campaign);
// - era=cw or era=gcw: that era's battle (Instant Action's era picker);
// - side=: the era its army fights in (rep and cis the Clone Wars, all and
//   imp the Galactic Civil War: a server's swbf1/nab2:rep:conquest), or for
//   a local side the battle that fields it;
// - else, or where the map has no battle of that era: the map's Clone Wars
//   battle, else its first.
// A battle fought in a world of its own (the campaign's Theed, NAB/nab2_h)
// is recorded but never played on this map's.
//
// The picked battle's parts (its mission, spawnScreen and vehicles) take the
// places they had in map.json when an import held one mission, so the code
// that reads world.mission, world.spawnScreen and world.vehicles needs
// nothing of its own.

export const ERAS = ["cw", "gcw"];
// The armies of each era; any other side is a local one.
export const ERA_SIDES = { cw: ["rep", "cis"], gcw: ["all", "imp"] };
// The era played when nothing names one, where the map has it: the port's
// default unit is a clone trooper.
export const DEFAULT_ERA = "cw";

// URLSearchParams, a query string, or a plain object of strings.
function reader(query) {
  if (query && typeof query.get === "function") return (key) => String(query.get(key) || "").toLowerCase();
  if (typeof query === "string") {
    const params = new URLSearchParams(query);
    return (key) => String(params.get(key) || "").toLowerCase();
  }
  const object = query || {};
  return (key) => (object[key] == null ? "" : String(object[key]).toLowerCase());
}

// The era an army fights in, or null for a local side.
export function eraOfSide(side) {
  return ERAS.find((era) => ERA_SIDES[era].includes(side)) || null;
}

// The battles map.json holds, in name order: [{ name, era, historical,
// playable }]. playable: fought in this map's own world.
export function battlesOf(world) {
  return Object.entries(world?.missions || {})
    .map(([name, battle]) => {
      const mission = battle?.mission || {};
      return { name, era: mission.era || null, historical: Boolean(mission.historical), sides: mission.sides || [], playable: !mission.world };
    })
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}

// The name of the mission a query plays on this map, or null when it holds
// none to play. An import from before every battle was kept has the one.
export function pickMission(world, query = "") {
  if (!world?.missions) return world?.mission?.name ?? null;
  const get = reader(query);
  const playable = battlesOf(world).filter((battle) => battle.playable);
  const named = playable.find((battle) => battle.name === get("mission"));
  if (named) return named.name;
  const regular = playable.filter((battle) => !battle.historical);
  // The default era's first, so a side two eras field has one answer.
  const ordered = [...regular.filter((battle) => battle.era === DEFAULT_ERA), ...regular.filter((battle) => battle.era !== DEFAULT_ERA)];
  const ofEra = (era) => ordered.find((battle) => battle.era === era)?.name;
  const era = get("era");
  if (ERAS.includes(era) && ofEra(era)) return ofEra(era);
  const side = get("side");
  const sideEra = eraOfSide(side);
  const bySide = sideEra ? ofEra(sideEra) : ordered.find((battle) => battle.sides.includes(side))?.name;
  return bySide || ordered[0]?.name || null;
}

// map.json as the battle plays it: the world, with the picked mission's
// mission, spawnScreen and vehicles in their places (null where it has
// none). A new object; the import's own is left as it was.
export function selectMission(world, query = "") {
  if (!world?.missions) return world;
  const name = pickMission(world, query);
  const battle = (name && world.missions[name]) || {};
  return { ...world, mission: battle.mission || null, spawnScreen: battle.spawnScreen || null, vehicles: battle.vehicles || null };
}

// ai-mission.json for the battle: it holds every script of the map, and its
// `mission` is the one the battle plays, the selected mission's (null when
// the file has no script of that name: the AI then reads map.json's mission
// block, never another battle's script). variants are that mission's other
// versions (its historical one).
export function selectAIMission(data, mission) {
  if (!data || typeof data !== "object") return data;
  const name = typeof mission === "string" ? mission : mission?.name;
  const held = Boolean(name) && Object.hasOwn(data.scripts || {}, name);
  return {
    ...data,
    mission: held ? name : null,
    variants: held ? Object.keys(data.scripts).filter((other) => other !== name && other.startsWith(name)).sort() : [],
  };
}
