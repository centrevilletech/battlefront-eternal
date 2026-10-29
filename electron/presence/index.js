// The player's activity for friends (docs/presence.md): Discord Rich
// Presence from what the app knows. Steam shows "playing" by itself when
// the game was started through the player's Steam library (electron/steam).
//
// The main process holds the state, because every move between the front
// end and a battle is a new page: it follows the window's navigation (the
// page's URL names the battle), remembers the servers the player saw or
// hosts, and takes the pages' own small reports (presence.update in the
// preload). The player chooses how much friends see (DETAIL_LEVELS).
import { createHash, randomBytes } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { DETAIL_LEVELS, discordActivity, sceneOf } from "./activity.js";

// A report from a page: only these, checked, so a page cannot put
// arbitrary text in front of the player's friends.
const SCREENS = new Set(["main", "sp", "ia", "mp", "options", "profile", "start"]);
const SIDE = /^[a-z]{3}$/;

export function cleanReport(report) {
  const out = {};
  if (!report || typeof report !== "object") return out;
  if (typeof report.screen === "string" && SCREENS.has(report.screen)) out.screen = report.screen;
  if (typeof report.side === "string" && SIDE.test(report.side)) out.side = report.side;
  if (report.side === null) out.side = null;
  if (Number.isInteger(report.players) && report.players >= 0 && report.players <= 256) out.players = report.players;
  if (report.phase === "playing" || report.phase === "loading" || report.phase === "over") out.phase = report.phase;
  return out;
}

// A server's name as its operator set it, for the player's own profile:
// printable text only (no control characters, and no direction marks or
// overrides that would turn the rest of the line around), one line, at
// most 64 characters.
export function serverName(name) {
  if (typeof name !== "string") return null;
  const text = name.replace(/[\p{Cc}\p{Cf}\u2028\u2029]/gu, "").replace(/\s+/g, " ").trim();
  return text ? [...text].slice(0, 64).join("") : null;
}

// A server as the list, LAN discovery or the hosting dialog describes it.
export function cleanServer(server, hosted = false) {
  if (!server || typeof server !== "object") return null;
  const id = typeof server.id === "string" && /^[a-f0-9]{64}$/.test(server.id) ? server.id : null;
  if (!id) return null;
  const count = (value) => (Number.isInteger(value) && value >= 0 && value <= 256 ? value : null);
  const players = Array.isArray(server.players) ? server.players.length : count(server.players);
  return { id, name: serverName(server.name), players, maxPlayers: count(server.maxPlayers), hosted };
}

// The front end's catalog of maps (shell.json, from the player's own
// prepared data): id -> display name.
export async function readMapNames(contentRoot) {
  try {
    const shell = JSON.parse(await readFile(path.join(contentRoot, "common", "shell", "shell.json"), "utf8"));
    const maps = Array.isArray(shell?.maps) ? shell.maps : Object.entries(shell?.maps ?? {}).map(([id, value]) => ({ id, ...value }));
    return new Map(maps.filter((map) => typeof map?.id === "string" && typeof map.name === "string").map((map) => [map.id.toLowerCase(), map.name.slice(0, 48)]));
  } catch {
    return new Map();
  }
}

// settings(): the desktop settings' `presence` ({ discord: detail level });
// contentRoot(): the game data in use; discord: a DiscordPresence (or any
// { setActivity(activity), stop() }); image: the Discord art asset's key.
export function createPresence({ settings, contentRoot, discord, image = "", now = Date.now, log = () => {} }) {
  const started = now();
  // One party id per run, never the server's own id.
  const partySalt = randomBytes(8).toString("hex");
  const servers = new Map();
  let state = { scene: "loader", loader: null, started };
  let names = null, namesRoot = null;
  let last = null, navigation = 0;

  const detail = () => {
    const value = settings()?.discord;
    return DETAIL_LEVELS.includes(value) ? value : "full";
  };

  async function mapName(map) {
    const root = contentRoot();
    if (!names || namesRoot !== root) {
      namesRoot = root;
      names = await readMapNames(root);
    }
    return names.get(map) ?? null;
  }

  function publish() {
    const activity = discordActivity(state, { detail: detail(), image });
    const key = JSON.stringify(activity);
    if (key === last) return;
    last = key;
    discord?.setActivity(activity);
  }

  return {
    get state() { return state; },

    // The window moved to `url` (did-navigate). A battle's clock runs from
    // the page's start until the battle itself begins (report: playing).
    async navigated(url) {
      const token = ++navigation;
      const scene = sceneOf(url);
      const next = { ...scene, started, loader: state.loader, since: now() };
      if (scene.scene === "battle") {
        const server = scene.network?.server ? servers.get(scene.network.server) : null;
        next.server = server ?? null;
        next.partyId = server ? createHash("sha256").update(partySalt + server.id).digest("hex").slice(0, 32) : null;
        next.mapName = scene.map ? await mapName(scene.map).catch(() => null) : null;
        if (token !== navigation) return;
      }
      state = next;
      publish();
    },

    // A page's own report (cleanReport): the front end's screen, or a
    // battle's start, side and players.
    report(report) {
      const clean = cleanReport(report);
      if (state.scene === "menu") {
        if (!clean.screen || clean.screen === state.screen) return;
        state = { ...state, screen: clean.screen };
      } else if (state.scene === "battle") {
        delete clean.screen;
        if (clean.phase === "playing" && state.phase !== "playing") clean.since = now();
        if (!Object.keys(clean).length) return;
        state = { ...state, ...clean };
      } else return;
      publish();
    },

    // The loader's mode: "update", "preparing" or anything else (starting).
    loader(mode) {
      const loader = mode === "update" || mode === "preparing" ? mode : null;
      if (state.loader === loader) return;
      state = { ...state, loader };
      if (state.scene === "loader") publish();
    },

    // Servers the player can join, from the online list or LAN discovery.
    servers(list) {
      for (const server of Array.isArray(list) ? list.slice(0, 512) : []) {
        const clean = cleanServer(server);
        if (clean) servers.set(clean.id, { ...servers.get(clean.id), ...clean, hosted: servers.get(clean.id)?.hosted ?? false });
      }
    },

    // The server the player hosts from this app (the hosting dialog).
    hosted(server) {
      const clean = cleanServer(server, true);
      if (clean) servers.set(clean.id, clean);
    },

    // The player changed how much friends see.
    refresh() {
      last = null;
      publish();
    },

    stop() {
      discord?.stop();
    },
  };
}
