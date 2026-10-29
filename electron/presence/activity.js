// What friends see the player doing, built from what the app knows:
// the page (a battle's URL, or the front end and its screen), the map
// catalog's names, the server the player joined or hosts (as the server
// list or the hosting dialog described it) and the battle's own reports
// (the player's side, how many players are in). Pure, so every case is
// tested without Discord.
//
// Public text: no em dashes, nothing the player typed but their server's
// name, nothing private (no addresses, identities or chat).
import { isBattle } from "../../src/launch.js";

export const ERAS = { cw: "Clone Wars", gcw: "Galactic Civil War" };
export const SIDES = { rep: "Republic", cis: "CIS", imp: "Empire", all: "Rebel Alliance" };
// What the front end's screens say (src/shell/index.js mediaId).
const SCREENS = { ia: "Choosing Instant Action maps", sp: "Choosing Instant Action maps", mp: "Looking for a server" };
// How much friends see: "full" (map, era, side, server), "game" (only
// that the game is running) or "off".
export const DETAIL_LEVELS = ["full", "game", "off"];

// Discord shows text of 2 to 128 characters.
export function fit(text, max = 128) {
  const value = String(text ?? "").replace(/\s+/g, " ").trim();
  if (value.length < 2) return undefined;
  return value.length <= max ? value : `${value.slice(0, max - 1).trimEnd()}…`;
}

// "NABOO: THEED" as the catalog may spell it, as a sentence would.
export function titleCase(name) {
  const text = String(name ?? "").trim();
  if (!text || text !== text.toUpperCase()) return text;
  return text.toLowerCase().replace(/(^|[\s:(/-])(\p{L})/gu, (_, lead, letter) => lead + letter.toUpperCase());
}

// The page's URL as a scene: the front end, or a battle with its map,
// era, side and connection.
export function sceneOf(url) {
  let parsed;
  try { parsed = new URL(url); } catch { return { scene: "unknown" }; }
  if (parsed.pathname.includes("/__desktop/")) return { scene: "loader" };
  if (!isBattle(parsed.search)) return { scene: "menu" };
  const params = parsed.searchParams;
  const word = (key) => (/^[a-z0-9_]{1,16}$/i.test(params.get(key) ?? "") ? params.get(key).toLowerCase() : null);
  const server = /^[a-f0-9]{64}$/.test(params.get("server") ?? "") ? params.get("server") : null;
  const network = params.get("network") === "1";
  return {
    scene: "battle",
    map: word("map"),
    era: word("era"),
    side: word("side"),
    network: network ? { server, lan: !!params.get("address") && !!params.get("port") } : null,
  };
}

// The Discord activity for a state (presence/index.js keeps it), or null
// for none.
//
// state: { scene, map, mapName, era, side, network, server: {name,
// players, maxPlayers, hosted}, screen, loader, players, since, started }.
// image: the application's art asset to show beside it, if any.
export function discordActivity(state, { detail = "full", image = "" } = {}) {
  if (detail === "off" || !state || state.scene === "unknown") return null;
  const activity = { timestamps: { start: Math.floor(state.started ?? Date.now()) } };
  if (image) activity.assets = { large_image: image, large_text: "Battlefront Eternal" };
  if (detail !== "full") return activity;
  const mapName = titleCase(state.mapName) || (state.map ? state.map.toUpperCase() : null);
  if (state.scene === "loader") {
    activity.details = { update: "Updating the game", preparing: "Preparing game data" }[state.loader] ?? "Starting up";
  } else if (state.scene === "menu") {
    activity.details = "In the menus";
    activity.state = fit(SCREENS[state.screen]);
  } else if (state.scene === "battle") {
    const server = state.server;
    const kind = !state.network ? "Instant Action" : server?.hosted ? "Hosting" : state.network.lan ? "LAN" : "Online";
    activity.details = fit(mapName ? `${kind}: ${mapName}` : kind);
    if (state.network) {
      activity.state = fit(server?.name) ?? fit([ERAS[state.era], SIDES[state.side]].filter(Boolean).join(", "));
      const players = Number(state.players ?? server?.players), most = Number(server?.maxPlayers);
      if (Number.isInteger(players) && Number.isInteger(most) && players >= 1 && most >= players) {
        activity.party = { id: state.partyId, size: [players, most] };
      }
    } else {
      activity.state = fit([ERAS[state.era], SIDES[state.side]].filter(Boolean).join(", "));
    }
    activity.timestamps.start = Math.floor(state.since ?? state.started ?? Date.now());
  }
  for (const key of ["details", "state"]) if (activity[key] === undefined) delete activity[key];
  if (activity.party && !activity.party.id) delete activity.party;
  return activity;
}
