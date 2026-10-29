import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, test } from "node:test";
import {
  BF1_STEAM_APP,
  driveLibraries,
  findContent,
  findGameInstall,
  findGogInstall,
  findSteamInstall,
  gogFolders,
  gogRecords,
  inspectInstall,
  installedAddOns,
  parseGogRegistry,
  parseHeroicInstalled,
  parseInstallDir,
  parseLibraryFolders,
  searchContent,
} from "../electron/discover.js";

const temp = mkdtempSync(path.join(tmpdir(), "bfe-discover-"));
after(() => rmSync(temp, { recursive: true, force: true }));

function touch(file, text = "") {
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, text);
}

function content(dir, maps = []) {
  touch(path.join(dir, "common", "common.json"), "{}");
  for (const map of maps) mkdirSync(path.join(dir, "maps", map), { recursive: true });
}

function install(dir, { lvl = "_LVL_PC" } = {}) {
  touch(path.join(dir, "GameData", "Data", lvl, "Common", "ENG", "common.lvl"));
  mkdirSync(path.join(dir, "GameData", "Data", lvl, "SIDE"), { recursive: true });
}

test("libraryfolders.vdf and appmanifest parsing", () => {
  const vdf = `"libraryfolders"\n{\n\t"0"\n\t{\n\t\t"path"\t\t"C:\\\\Program Files (x86)\\\\Steam"\n\t}\n\t"1"\n\t{\n\t\t"path"\t\t"/mnt/games/SteamLibrary"\n\t\t"apps"\t{ "1058020" "123" }\n\t}\n}`;
  assert.deepEqual(parseLibraryFolders(vdf), ["C:\\Program Files (x86)\\Steam", "/mnt/games/SteamLibrary"]);
  assert.equal(parseInstallDir(`"AppState"\n{\n\t"appid"\t"1058020"\n\t"installdir"\t"Star Wars Battlefront"\n}`), "Star Wars Battlefront");
  assert.equal(parseInstallDir("{}"), null);
});

test("an installation needs _LVL_PC with SIDE and a per-language common.lvl", async () => {
  const good = path.join(temp, "installs", "good");
  install(good);
  assert.equal((await inspectInstall(good)).dir, good);
  const lower = path.join(temp, "installs", "lower");
  install(lower, { lvl: "_lvl_pc" });
  assert.equal((await inspectInstall(lower)).dir, lower);
  const partial = path.join(temp, "installs", "partial");
  mkdirSync(path.join(partial, "GameData", "Data", "_LVL_PC", "SIDE"), { recursive: true });
  assert.equal(await inspectInstall(partial), null);
});

test("finds the game through a secondary Steam library's app manifest", async () => {
  const steam = path.join(temp, "steam");
  const library = path.join(temp, "library");
  touch(path.join(steam, "steamapps", "libraryfolders.vdf"), `"libraryfolders" { "0" { "path" "${steam}" } "1" { "path" "${library}" } }`);
  touch(path.join(library, "steamapps", "appmanifest_1058020.acf"), `"AppState" { "installdir" "Star Wars Battlefront" }`);
  install(path.join(library, "steamapps", "common", "Star Wars Battlefront"));
  const found = await findSteamInstall({ roots: [steam] });
  assert.equal(found.dir, path.join(library, "steamapps", "common", "Star Wars Battlefront"));
  assert.equal(found.source, "steam");
  assert.equal(await findSteamInstall({ roots: [path.join(temp, "nowhere")] }), null);
});

test("a library only config/libraryfolders.vdf lists is searched too", async () => {
  const steam = path.join(temp, "steam-config");
  const library = path.join(temp, "library-config");
  touch(path.join(steam, "config", "libraryfolders.vdf"), `"libraryfolders" { "1" { "path" "${library}" } }`);
  install(path.join(library, "steamapps", "common", "Star Wars Battlefront"));
  assert.equal((await findSteamInstall({ roots: [steam] })).dir, path.join(library, "steamapps", "common", "Star Wars Battlefront"));
});

test("a library on another drive is found where libraries usually are when Steam's records do not name it", async () => {
  // Drives as folders: F: has the game in F:\STEAM (as one player's PC does),
  // G: an empty Games\SteamLibrary, H: nothing of Steam's.
  const f = path.join(temp, "drive-f"), g = path.join(temp, "drive-g"), h = path.join(temp, "drive-h");
  const game = path.join(f, "STEAM", "steamapps", "common", "Star Wars Battlefront (Classic 2004)");
  touch(path.join(f, "STEAM", "steamapps", `appmanifest_${BF1_STEAM_APP}.acf`), `"AppState" { "installdir" "Star Wars Battlefront (Classic 2004)" }`);
  install(game);
  mkdirSync(path.join(g, "Games", "SteamLibrary", "steamapps"), { recursive: true });
  mkdirSync(h, { recursive: true });
  const missing = path.join(temp, "drive-z");
  assert.deepEqual(await driveLibraries({ drives: [f, g, h, missing] }), [path.join(f, "STEAM"), path.join(g, "Games", "SteamLibrary")]);
  assert.deepEqual(await driveLibraries({ platform: "linux" }), [], "only Windows has drive letters to look on");
  const nowhere = [path.join(temp, "no-steam")];
  assert.deepEqual(await findSteamInstall({ roots: nowhere, drives: { drives: [g, f] } }), { dir: game, source: "steam" });
  assert.equal(await findSteamInstall({ roots: nowhere, drives: false }), null, "drives: false looks only where Steam says");
  assert.equal(await findSteamInstall({ roots: nowhere, drives: { drives: [g, h] } }), null);
});

test("add-ons are the AddOn folders with an addme.script and data, as tools/prepare.py records them", async () => {
  const game = path.join(temp, "installs", "addons");
  install(game);
  assert.deepEqual(await installedAddOns(game), []);
  touch(path.join(game, "GameData", "AddOn", "Tat3", "addme.script"));
  touch(path.join(game, "GameData", "AddOn", "Tat3", "Data", "_lvl_pc", "TAT", "tat3.lvl"));
  touch(path.join(game, "GameData", "AddOn", "Abc", "addme.script"));
  touch(path.join(game, "GameData", "AddOn", "Abc", "Data", "_LVL_PC", "mission.lvl"));
  touch(path.join(game, "GameData", "AddOn", "Notes", "readme.txt"));
  touch(path.join(game, "GameData", "AddOn", "NoData", "addme.script"));
  assert.deepEqual(await installedAddOns(game), ["abc", "tat3"]);
  assert.deepEqual(await installedAddOns(path.join(temp, "nowhere")), []);
});

test("finds a matching folder in steamapps/common without a manifest", async () => {
  const steam = path.join(temp, "steam-bare");
  install(path.join(steam, "steamapps", "common", "SWBF"));
  assert.equal((await findSteamInstall({ roots: [steam] })).dir, path.join(steam, "steamapps", "common", "SWBF"));
});

test("GOG's registry and Heroic's records give each game's folder", () => {
  // reg query HKLM\SOFTWARE\WOW6432Node\GOG.com\Games /s, as reg.exe prints it.
  const reg = [
    "",
    "HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\GOG.com\\Games\\1000000001",
    "    gameName    REG_SZ    Some Other Game",
    "    path    REG_SZ    E:\\Games\\Some Other Game",
    "    launchParam    REG_SZ    ",
    "",
    // GOG's BF1 as GOG Galaxy installed it into a library on E: (2026-10-22).
    "HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\GOG.com\\Games\\1668107107",
    "    gameName    REG_SZ    STAR WARS Battlefront",
    "    gameID    REG_SZ    1668107107",
    "    PATH    REG_SZ    E:\\Games\\Star Wars - Battlefront",
    "    ver    REG_SZ    1.3.7.4",
    "    DLC    REG_SZ",
    "",
    "HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\GOG.com\\Games\\1668107107\\Settings",
    "",
  ].join("\r\n");
  assert.deepEqual(parseGogRegistry(reg), [
    { name: "Some Other Game", path: "E:\\Games\\Some Other Game" },
    { name: "STAR WARS Battlefront", path: "E:\\Games\\Star Wars - Battlefront" },
  ]);
  assert.deepEqual(parseGogRegistry(""), []);
  const heroic = { installed: [{ appName: "1668107107", install_path: "/home/p/Games/Heroic/Star Wars - Battlefront", platform: "windows" }, { appName: "3" }] };
  assert.deepEqual(parseHeroicInstalled(JSON.stringify(heroic)), [{ name: "", path: "/home/p/Games/Heroic/Star Wars - Battlefront" }]);
  assert.deepEqual(parseHeroicInstalled("not json"), []);
  assert.deepEqual(parseHeroicInstalled("null"), []);
});

test("finds a GOG copy where Heroic's records say", async () => {
  const home = path.join(temp, "gog-heroic-home");
  const game = path.join(temp, "gog-library", "Star Wars - Battlefront");
  const other = path.join(temp, "gog-library", "Other Game");
  install(game);
  install(other);
  // Flatpak Heroic, the way Omarchy and the Steam Deck usually have it.
  const records = { installed: [{ install_path: other }, { install_path: game }] };
  touch(path.join(home, ".var", "app", "com.heroicgameslauncher.hgl", "config", "heroic", "gog_store", "installed.json"), JSON.stringify(records));
  const options = { platform: "linux", home, env: {} };
  assert.deepEqual(await gogRecords(options), [game], "only folders that say Battlefront");
  assert.deepEqual(await findGogInstall(options), { dir: game, source: "gog" });
  assert.equal(await findGogInstall({ platform: "linux", home: path.join(temp, "nowhere"), env: {} }), null);
});

test("finds a GOG copy where GOG games usually go when nothing records it", async () => {
  // A drive (a folder standing in for D:) with the offline installer's GOG Games.
  const drive = path.join(temp, "gog-drive-d");
  const game = path.join(drive, "GOG Games", "Star Wars - Battlefront");
  install(game);
  mkdirSync(path.join(drive, "GOG Games", "Star Wars Battlefront Notes"), { recursive: true });
  const empty = path.join(temp, "gog-empty-home");
  mkdirSync(empty, { recursive: true });
  assert.deepEqual(await gogFolders({ platform: "linux", home: empty, drives: [drive] }), [path.join(drive, "GOG Games")]);
  assert.deepEqual(await findGogInstall({ platform: "linux", home: empty, env: {}, drives: [drive] }), { dir: game, source: "gog" });

  // Lutris's Wine prefix, one per game in ~/Games.
  const lutris = path.join(temp, "gog-lutris-home");
  const inPrefix = path.join(lutris, "Games", "star-wars-battlefront", "drive_c", "GOG Games", "Star Wars - Battlefront");
  install(inPrefix);
  assert.deepEqual(await findGogInstall({ platform: "linux", home: lutris, env: {} }), { dir: inPrefix, source: "gog" });

  // A CrossOver bottle on macOS, with GOG Galaxy's own folder.
  const mac = path.join(temp, "gog-mac-home");
  const bottle = path.join(mac, "Library", "Application Support", "CrossOver", "Bottles", "Battlefront", "drive_c", "Program Files (x86)", "GOG Galaxy", "Games", "Star Wars - Battlefront");
  install(bottle);
  assert.deepEqual(await findGogInstall({ platform: "darwin", home: mac, env: {} }), { dir: bottle, source: "gog" });
});

test("the chosen folder wins, then the Steam copy, then the GOG copy", async () => {
  const steam = path.join(temp, "both-steam");
  const steamGame = path.join(steam, "steamapps", "common", "Star Wars Battlefront (Classic 2004)");
  install(steamGame);
  const home = path.join(temp, "both-home");
  const gogGame = path.join(home, "Games", "Heroic", "Star Wars - Battlefront");
  install(gogGame);
  const chosen = path.join(temp, "both-chosen");
  install(chosen);
  const gog = { platform: "linux", home, env: {} };
  const nowhere = { roots: [path.join(temp, "no-steam")], drives: false };
  assert.deepEqual(await findGameInstall({ steam: { roots: [steam], drives: false }, gog }), { dir: steamGame, source: "steam" });
  assert.deepEqual(await findGameInstall({ steam: nowhere, gog }), { dir: gogGame, source: "gog" });
  assert.deepEqual(await findGameInstall({ extra: [chosen], steam: { roots: [steam], drives: false }, gog }), { dir: chosen, source: "local" });
  assert.equal(await findGameInstall({ steam: nowhere, gog: { platform: "linux", home: path.join(temp, "nowhere"), env: {} } }), null);
});

test("searches a home folder for extracted data, skipping noise", async () => {
  const home = path.join(temp, "home");
  content(path.join(home, "code", "battlefront-eternal", "public"), ["nab2", "bes2"]);
  content(path.join(home, "node_modules", "pkg", "public"), ["x", "y", "z"]);
  content(path.join(home, ".hidden", "public"), ["x", "y", "z"]);
  const found = await searchContent(home);
  assert.deepEqual(found.map((entry) => entry.dir), [path.join(home, "code", "battlefront-eternal", "public")]);
  assert.deepEqual(found[0].maps, ["bes2", "nab2"]);
  assert.deepEqual(await searchContent(home, { depth: 2 }), []);
});

test("explicit choices win, then the richest likely folder, then the search", async () => {
  const a = path.join(temp, "content-a");
  const b = path.join(temp, "content-b");
  content(a, ["nab2"]);
  content(b, ["nab2", "kam1"]);
  const missing = path.join(temp, "missing");
  assert.equal((await findContent({ explicit: [a], likely: [b], search: false })).source, "chosen");
  const likely = await findContent({ explicit: [missing], likely: [a, b, missing], search: false });
  assert.equal(likely.dir, b);
  assert.equal(likely.source, "found");
  const searched = await findContent({ likely: [missing], home: path.join(temp, "home") });
  assert.equal(searched.source, "searched");
  assert.equal(await findContent({ likely: [missing], search: false }), null);
});

test("Linux desktop integration writes the launcher entry and command once", async () => {
  const { integrateLinuxDesktop, desktopEntry, DISPLAY_NAME } = await import("../electron/linux-integration.js");
  const home = path.join(temp, "linux-home");
  const icon = path.join(temp, "icon.png");
  touch(icon, "png");
  const env = {};
  const first = await integrateLinuxDesktop({ executable: "/opt/Apps/Battlefront Eternal.AppImage", icon, home, env });
  assert.deepEqual(first, { icon: true, entry: true, command: true });
  const { readFileSync, statSync } = await import("node:fs");
  const entry = readFileSync(path.join(home, ".local/share/applications/battlefront-eternal.desktop"), "utf8");
  assert.match(entry, new RegExp(`Name=${DISPLAY_NAME}`));
  assert.match(entry, /Exec="\/opt\/Apps\/Battlefront Eternal.AppImage" %U/);
  const command = path.join(home, ".local/bin/battlefront");
  assert.match(readFileSync(command, "utf8"), /exec '\/opt\/Apps\/Battlefront Eternal.AppImage' "\$@"/);
  assert.ok(statSync(command).mode & 0o100);
  assert.deepEqual(await integrateLinuxDesktop({ executable: "/opt/Apps/Battlefront Eternal.AppImage", icon, home, env }), { icon: false, entry: false, command: false });
  writeFileSync(command, "#!/bin/sh\necho mine\n");
  assert.equal((await integrateLinuxDesktop({ executable: "/new/place.AppImage", icon, home, env })).command, "foreign");
  assert.equal(desktopEntry('/a "b"/c').split("\n").find((line) => line.startsWith("Exec=")), 'Exec="/a \\"b\\"/c" %U');
  assert.deepEqual(await integrateLinuxDesktop({ executable: "/x", icon, home, env: { BFE_NO_DESKTOP_INTEGRATION: "1" } }), { skipped: true });
});
