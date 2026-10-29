import { onlineNotice } from './online-notice.js';
import { MenuScreen, Rows, List, Button, Text, HorizontalButtons, optionsMenu, confirmPopup, okPopup } from "../menus/index.js";
import { launchUrl, MAX_PLAYLIST } from "../launch.js";
import { directConnect } from "./direct-connect.js";
import { hostDialog } from "./host-dialog.js";
import { profileDialog, cleanPlayerName, DEFAULT_PLAYER_NAME } from "./profile-dialog.js";
import { quad, WHITE } from "../menus/themes/classic/draw.js";
import { integrationsPage, offerSteam } from "./integrations.js";
import { versionText } from "./version.js";
import { battleParams } from "../runtime/battle-query.js";
import { serverBrowser } from "./server-browser.js";

// Screen construction is independent of the renderer and navigation, so the
// playlist and all menu actions can be exercised without loading a battle.
// browser: MULTIPLAYER is the server browser (server-browser.js, the page
// theme's) rather than the original's HOST / LAN / ONLINE rows.
export function createScreens({ data, settings, media, query = "", navigate = (url) => location.assign(url), close = () => window.close(), browser = false, askProfile = profileDialog }) {
  const strings = data.strings;
  const say = (key, fallback) => strings[key] || fallback;
  const back = () => new Button({ id: "back", text: say("common.back", "BACK") });
  const title = (text) => new Text({ id: "title", look: "title", text });
  const push = (build) => (screen) => screen.stack.push(build());
  const keepDiagnostics = target => {
    const current = new URLSearchParams(query);
    for (const key of ['traceInput', 'profileRuntime']) if (current.has(key)) target.set(key, current.get(key));
  };
  const notice = (text) => (screen) => screen.stack.push(okPopup({ message: text, strings }));
  const rowScreen = (id, text, rows, layout = {}) => new MenuScreen({
    id, focus: "keep", widgets: [new Rows({ id: "rows", title: text, rows,
      layout: { at: [0.5, 0.5], y: 20, width: 350, rotY: 35, ...layout } }), back()],
  });
  const exit = (screen) => screen.stack.push(confirmPopup({ message: "EXIT TO WINDOWS?", strings, onYes: close }));
  // One profile: the player's name, kept in the settings (playerName) and
  // sent to a server on joining. CREATE PROFILE renames it; DELETE PROFILE
  // returns to the original's first profile, PLAYER 1.
  const playerName = () => cleanPlayerName(settings?.get?.("playerName"));
  const profile = () => rowScreen("profile", "PROFILE MANAGEMENT", [
    { id: "player", label: () => playerName(), disabled: true },
    { id: "new", label: "CREATE PROFILE", action: async () => { const name = await profileDialog(playerName()); if (name) settings?.set?.("playerName", name); } },
    { id: "delete", label: "DELETE PROFILE", action: () => settings?.set?.("playerName", DEFAULT_PLAYER_NAME) },
  ]);
  // Multiplayer needs a profile: a name of the player's own, not the
  // default every new install has (a tester's suggestion, so servers
  // don't fill with PLAYER 1s). Asks for one when there is none; false
  // when the player cancels, and nothing launches.
  const hasProfile = () => playerName() !== DEFAULT_PLAYER_NAME;
  const requireProfile = async () => {
    if (hasProfile()) return true;
    const name = await askProfile("", { message: "Create a profile before playing multiplayer. Other players will see this name." });
    if (!name || name === DEFAULT_PLAYER_NAME) return false;
    settings?.set?.("playerName", name);
    return hasProfile();
  };
  // Into a server's battle: kind "LAN" (an address of its own) or
  // "ONLINE".
  const joinServer = async (server, kind) => {
    if (!(await requireProfile())) return;
    const mission = server.rotation?.[0] || "swbf1/nab2:rep:conquest";
    const match = mission.match(/^swbf1\/([^:]+):([^:]+)/);
    // The server's own battle where its listing (or /info) names it, the
    // battle's keys only; else one made from its rotation.
    const battle = battleParams(server.query), own = battle.has("map");
    const target = own ? battle : new URLSearchParams(query);
    keepDiagnostics(target);
    // The server's mission decides the map and side; a pick or playlist
    // left in the menu's own URL from an earlier battle must not survive.
    for (const name of ["shell", "screen", "address", "port", "relay", "unit", "spawn", "spawnscreen", "rotation", "round", "order", "mission"]) target.delete(name);
    if (!own) { target.set("map", match?.[1] || "nab2"); target.set("side", match?.[2] || "rep"); target.set("era", ["rep", "cis", "cw"].includes(match?.[2]) ? "cw" : "gcw"); target.set("ai", "classic"); target.set("seed", "1"); }
    target.set("network", "1"); target.set("server", server.id);
    if (kind === "LAN") { target.set("address", server.address); target.set("port", String(server.port || 18451)); }
    if (server.relayPolicy === "always" && kind !== "LAN") target.set("relay", "always");
    navigate(`/?${target}`);
  };
  const network = (kind) => {
    const bridge = globalThis.bfeDesktop?.network;
    const rows = [{ id: "status", label: "LOADING SERVERS...", disabled: true }];
    const screen = rowScreen(`mp-${kind.toLowerCase()}`, kind, rows, { width: 650 });
    const launch = (server) => joinServer(server, kind);
    const refresh = async () => {
      try {
        if (!bridge) throw new Error("ONLINE PLAY REQUIRES THE DESKTOP CLIENT.");
        const consent = await bridge.request("notice");
        if (kind !== "LAN" && !consent.accepted) {
          rows.splice(0, rows.length, { id: "notice", label: "ACCEPT ONLINE PRIVACY NOTICE", action: async () => {
            if (await onlineNotice(bridge, consent)) await refresh();
          } });
          return;
        }
        const result = kind === "LAN" ? { servers: await bridge.request("discover") } : await bridge.request("servers");
        rows.splice(0, rows.length, ...result.servers.map(server => ({ id: server.id, label: `${server.name}  ${server.players}/${server.maxPlayers}`, action: () => {
          launch(server);
        } })));
        if (!rows.length) rows.push({ id: "empty", label: "NO SERVERS AVAILABLE", disabled: true });
        rows.push({ id: "refresh", label: "REFRESH", action: refresh });
        if (kind === "LAN") rows.push({ id: "direct", label: "DIRECT CONNECTION", action: async () => { const server = await directConnect(bridge); if (server) launch(server); } });
      } catch (error) { rows.splice(0, rows.length, { id: "error", label: error.message, disabled: true }, { id: "refresh", label: "RETRY", action: refresh }); }
    };
    screen.onEnter = refresh;
    return screen;
  };
  const host = async screen => {
    const desktop = globalThis.bfeDesktop;
    if (!desktop?.host) { notice("HOSTING REQUIRES THE DESKTOP CLIENT.")(screen); return; }
    if (!(await requireProfile())) return;
    const server = await hostDialog(desktop.host, desktop.network, data.maps);
    if (!server) return;
    const target = new URLSearchParams(server.query);
    keepDiagnostics(target);
    for (const [key, value] of Object.entries({ network: '1', server: server.id, address: server.address, port: server.port })) target.set(key, String(value));
    navigate('/?' + target);
  };
  // The server browser's list: the master's (once the privacy notice is
  // accepted) or the LAN's.
  const loadServers = async (source) => {
    const bridge = globalThis.bfeDesktop?.network;
    if (!bridge) return { state: "desktop", message: "ONLINE PLAY REQUIRES THE DESKTOP CLIENT." };
    if (source === "lan") return { servers: await bridge.request("discover") };
    const consent = await bridge.request("notice");
    if (!consent.accepted) return { state: "notice", consent };
    return { servers: (await bridge.request("servers")).servers };
  };
  const serverList = () => serverBrowser({
    load: loadServers,
    join: (server, source) => joinServer(server, source === "lan" ? "LAN" : "ONLINE"),
    host,
    direct: async (screen) => {
      const bridge = globalThis.bfeDesktop?.network;
      if (!bridge) { notice("ONLINE PLAY REQUIRES THE DESKTOP CLIENT.")(screen); return; }
      const server = await directConnect(bridge);
      if (server) joinServer(server, "LAN");
    },
    acceptNotice: (consent) => onlineNotice(globalThis.bfeDesktop?.network, consent),
    strings,
  });
  const multiplayer = browser ? serverList : () => rowScreen("mp", "MULTIPLAYER", [
    { id: "host", label: "HOST", action: host },
    { id: "lan", label: "LAN", action: push(() => network("LAN")) },
    { id: "online", label: "ONLINE", action: push(() => network("ONLINE")) },
  ]);
  const integrations = globalThis.bfeDesktop?.integrations;
  const options = () => optionsMenu({ settings, strings,
    online: push(browser ? serverList : () => network("ONLINE")),
    integrations: integrations ? push(() => integrationsPage({ bridge: integrations, strings })) : null,
    credits: notice("BATTLEFRONT ETERNAL\nA fan-made mod for the Steam versions of\nStar Wars: Battlefront (2004) and Battlefront II (2005).\nBattlefront II support is planned."),
  });
  const singleplayer = () => rowScreen("sp", say("ifs.main.sp", "SINGLEPLAYER"), [
    { id: "campaign", label: "HISTORICAL CAMPAIGN", disabled: true },
    { id: "conquest", label: "GALACTIC CONQUEST", disabled: true },
    { id: "instant", label: "INSTANT ACTION", action: push(instantAction) },
  ], { font: "gamefont_large", width: 650 });

  function instantAction() {
    const entries = [];
    let random = false;
    const maps = data.maps;
    // The eras the import holds a battle for (shell.json marks each one).
    const eras = (map) => Object.keys(map.eras || {}).filter((era) => ["cw", "gcw"].includes(era) && map.eras[era]?.extracted !== false);
    const name = (id) => maps.find((map) => map.id === id)?.name || id;
    const eraName = (era) => era === "cw" ? "CLONE WARS" : "GALACTIC CIVIL WAR";
    const look = (side) => (surface) => ({ at: [0.5, 0.5],
      x: side * 227, rotY: -side * 35,
      box: { x: 0, y: -10, w: 442, h: Math.min(398, surface.height - 210) },
      font: "gamefont_tiny", pitch: 26,
    });
    const add = (screen) => {
      const map = maps[available.selected];
      if (!map?.extracted || !eras(map).length) { screen.sound("error"); return; }
      if (entries.length >= MAX_PLAYLIST) { notice("SELECTED MAPLIST IS FULL.")(screen); return; }
      const append = (era) => { entries.push({ map: map.id, era }); selected.selected = entries.length - 1; selected.reveal(); };
      const choices = eras(map);
      if (choices.length === 1) append(choices[0]);
      else screen.stack.push(rowScreen("ia-era", map.name, choices.map((era) => ({
        id: era, label: eraName(era), action: (picker) => { append(era); picker.stack.pop(); },
      })), { at: [0.25, 0.5], width: 350 }));
    };
    const remove = () => {
      entries.splice(selected.selected, 1);
      selected.selected = Math.max(0, Math.min(selected.selected, entries.length - 1));
      selected.reveal();
    };
    const available = new List({ id: "available", look: look(-1),
      rows: maps.map((map) => { const row = [map.name]; row.disabled = !map.extracted; return row; }),
      onSelect: (index) => media.preview(maps[index]?.id), onAccept: (_, screen) => add(screen),
    });
    const selected = new List({ id: "selected", look: look(1),
      rows: () => entries.map((entry) => {
        const row = [name(entry.map)];
        row.icons = entry.era === "cw" ? ["rep_icon", "cis_icon"] : ["imp_icon", "all_icon"];
        return row;
      }),
      onAccept: remove,
    });
    return new MenuScreen({ id: "ia", focus: "keep", entries,
      onEnter: () => media.preview(maps[available.selected]?.id),
      onKey: (code) => { if (code !== "Delete") return false; remove(); return true; },
      draw(ctx, phase) {
        if (phase === "under" && ctx.surface.texture("his_brief_bg")) {
          quad(ctx, ctx.surface.anchor(0, 0, false), "his_brief_bg", 0, 0, 1024, ctx.surface.height, [0, 0, 1, 1], WHITE);
        }
      },
      widgets: [title("INSTANT ACTION"),
        new Text({ id: "available-title", look: "label", text: "AVAILABLE MAPS:", at: [0.25, 0], y: 45, align: 2 }),
        new Text({ id: "selected-title", look: "label", text: "SELECTED MAPS:", at: [0.75, 0], y: 45, align: 2 }),
        available, selected,
        new HorizontalButtons({ id: "edit", y: 220, buttons: [
          { id: "add", label: "ADD", action: add }, { id: "remove", label: "REMOVE", action: remove },
        ] }),
        back(),
        new Button({ id: "order", place: "middle", text: () => random ? "RANDOM ORDER" : "PLAY IN ORDER", action: () => { random = !random; } }),
        new Button({ id: "launch", place: "accept", text: "LAUNCH", disabled: () => !entries.length,
          action: () => { if (entries.length) navigate(launchUrl(entries, { random }, query)); } }),
      ],
    });
  }
  const transferContent = action => async screen => {
    const desktop = globalThis.bfeDesktop;
    if (!desktop?.contentTransfer) { notice("CONTENT TRANSFER REQUIRES THE DESKTOP CLIENT.")(screen); return; }
    try { await desktop.contentTransfer(action); }
    catch (error) { notice(error.message)(screen); }
  };
  const version = versionText();
  const main = () => {
    const screen = rowScreen("main", say("ifs.main.title", "MAIN MENU"), [
      { id: "sp", label: "SINGLEPLAYER", action: push(singleplayer) },
      { id: "mp", label: "MULTIPLAYER", action: push(multiplayer) },
      { id: "options", label: "OPTIONS", action: push(options) },
      { id: "profile", label: "PROFILE MANAGEMENT", action: push(profile) },
      { id: "export-content", label: "EXPORT CONTENT", action: transferContent("export") },
      { id: "import-content", label: "IMPORT CONTENT", action: transferContent("import") },
      { id: "quit", label: "EXIT TO WINDOWS", action: exit },
    ], { at: [0.5, 0.65], y: 30, x: 0, width: 250, rotY: -30, rotX: -20, rotZ: 1, order: "XYZ" });
    screen.widget("rows").listCount = 7;
    screen.widgets.pop();
    // This build, its simulation and its game data's generations (version.js).
    screen.widgets.push(new Text({ id: "version", look: "label", text: version, at: [1, 1], x: -8, y: -18, align: 1, alpha: 0.7 }));
    screen.backAction = push(profile);
    // Once: put the game in the player's Steam library (integrations.js).
    if (integrations) screen.onEnter = () => setTimeout(() => offerSteam(integrations), 1500);
    return screen;
  };
  return { main, sp: singleplayer, ia: instantAction, mp: multiplayer, profile, options };
}
