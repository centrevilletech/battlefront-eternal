// Whether the spawn screen runs, and what the world confirms on its own when
// it doesn't. Pure: the page and the world worker both read the same URL
// through this, so they start the same battle (docs/human-spawn.md).
//
// The original opens its spawn screen at map load and four seconds after
// every death. This port skips it whenever the URL already names the
// player's choice (side=, unit= or spawn=): the world then confirms that
// choice at load and after every death, as it always has. A network battle
// (?network=1) always runs the screen and takes no unit or post pick from
// the URL: its server assigns the team and the lives, and the URL's side only
// names the body loaded first. ?spawnscreen=1 or ?spawnscreen=0 overrides
// either way.
//
// The class played first:
// - unit= names it;
// - side= alone picks that side's first non-hero class in the mission;
// - with neither, the screen's default is the first class of the team that
//   fields this port's old default unit (the Republic on Theed), and a
//   bypass without them (spawn= alone, or spawnscreen=0) keeps that old
//   default unit itself.
// The team is the mission team whose roster lists the class (or whose hero
// it is), else team 1.

export const DEFAULT_SIDE = "rep";
export const DEFAULT_UNIT = "rep_inf_jet_trooper";

// URLSearchParams, a query string, or a plain object of strings.
function reader(query) {
  if (query && typeof query.get === "function") return (key) => query.get(key) || "";
  if (typeof query === "string") {
    const params = new URLSearchParams(query);
    return (key) => params.get(key) || "";
  }
  const object = query || {};
  return (key) => (object[key] == null ? "" : String(object[key]));
}

// The mission's teams in number order: [[team, { classes, hero, ... }]].
function teamList(mission) {
  return Object.entries(mission?.teams || {})
    .map(([team, value]) => [Number(team), value || {}])
    .sort((a, b) => a[0] - b[0]);
}

// A team's classes the player may pick: its roster without its hero.
export function playableClasses(team) {
  return (team?.classes || []).map(([className]) => className).filter((className) => className && className !== team.hero);
}

// The mission team whose roster lists the class or whose hero it is, else
// team 1.
export function teamOfClass(mission, className) {
  for (const [team, value] of teamList(mission)) {
    if (value.hero === className || (value.classes || []).some(([name]) => name === className)) return team;
  }
  return 1;
}

// The side a class comes from: the mission side its name starts with.
export function sideOfClass(mission, className, fallback = DEFAULT_SIDE) {
  const prefix = String(className || "").split("_")[0].toLowerCase();
  const sides = mission?.sides || ["rep", "cis", "imp", "all"];
  return sides.includes(prefix) ? prefix : fallback;
}

// { screen, side, className, team, post, auto }: auto is the choice the
// world confirms by itself ({ team, className, post }), or null when the
// screen asks the player.
export function spawnPolicy(query, mission = null) {
  const get = reader(query);
  const network = Boolean(get("network"));
  const side = get("side").toLowerCase();
  const unit = network ? "" : get("unit");
  const post = network ? null : get("spawn") || null;
  const forced = get("spawnscreen");
  const screen = forced === "1" ? true : forced === "0" ? false : network || !(side || unit || post);
  let className = unit;
  if (!className && side) {
    for (const [, team] of teamList(mission)) {
      className = playableClasses(team).find((name) => name.toLowerCase().startsWith(`${side}_`)) || "";
      if (className) break;
    }
  }
  if (!className && screen) {
    const team = mission?.teams?.[teamOfClass(mission, DEFAULT_UNIT)];
    className = playableClasses(team)[0] || "";
  }
  if (!className) className = DEFAULT_UNIT;
  const team = teamOfClass(mission, className);
  // The side loaded first is the chosen class's own. A URL side the mission
  // does not field (a server rotation naming a faction that is not on this
  // map: Hoth is fought only in the Galactic Civil War) stands only for a
  // class of its own, as the old default unit is.
  const sideName = className.toLowerCase().startsWith(`${side}_`) ? side : sideOfClass(mission, className, "") || side || sideOfClass(mission, className);
  return { screen, side: sideName, className, team, post, auto: screen ? null : { team, className, post } };
}
