// Finds what the desktop app needs on this machine without asking: game data
// already extracted from the player's copy (tools/extract.py's output), and
// the Steam or GOG installation of Star Wars: Battlefront (2004) it comes from.
import { execFile } from "node:child_process";
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";

// Steam's app id for STAR WARS Battlefront (Classic, 2004).
export const BF1_STEAM_APP = "1058020";

// Folders not worth descending into while searching a home folder.
const SKIP = new Set([
  "node_modules", ".git", ".cache", ".npm", ".nvm", ".cargo", ".rustup", "proc", "sys",
  "AppData", "Library", "Applications", "Pictures", "Music", "Movies", "Videos", "snap", ".var", ".wine",
  "go", ".gradle", ".m2", ".venv", "venv", "__pycache__", "dist", "build", "release",
]);

async function exists(file) {
  try {
    await fs.access(file);
    return true;
  } catch {
    return false;
  }
}

async function entries(dir) {
  try {
    return await fs.readdir(dir, { withFileTypes: true });
  } catch {
    return [];
  }
}

// The child of dir named name, matching case-insensitively (Steam on Linux
// keeps the Windows capitalization, but not every copy does).
async function child(dir, name) {
  if (await exists(path.join(dir, name))) return path.join(dir, name);
  const lower = name.toLowerCase();
  const match = (await entries(dir)).find((entry) => entry.name.toLowerCase() === lower);
  return match ? path.join(dir, match.name) : null;
}

async function walk(dir, names) {
  let at = dir;
  for (const name of names) {
    at = at && (await child(at, name));
    if (!at) return null;
  }
  return at;
}

// ---- Extracted game data ----

// A content folder holds common/common.json; more maps rank it higher.
export async function inspectContent(dir) {
  if (!dir || !(await exists(path.join(dir, "common", "common.json")))) return null;
  const maps = (await entries(path.join(dir, "maps"))).filter((entry) => entry.isDirectory()).map((entry) => entry.name);
  return { dir: path.resolve(dir), maps: maps.sort() };
}

// Breadth-first search below root for content folders, bounded by depth,
// folder count and time so a large home folder never stalls startup.
export async function searchContent(root, { depth = 4, budgetMs = 2500, maxDirs = 20000 } = {}) {
  const found = [];
  const deadline = Date.now() + budgetMs;
  let queue = [{ dir: root, level: 0 }];
  let visited = 0;
  while (queue.length && Date.now() < deadline && visited < maxDirs) {
    const next = [];
    for (const { dir, level } of queue) {
      if (Date.now() >= deadline || visited >= maxDirs) break;
      visited++;
      const list = await entries(dir);
      if (list.some((entry) => entry.isDirectory() && entry.name === "common")) {
        const content = await inspectContent(dir);
        if (content) {
          found.push(content);
          continue;
        }
      }
      if (level >= depth) continue;
      for (const entry of list) {
        if (!entry.isDirectory() || entry.isSymbolicLink()) continue;
        if (SKIP.has(entry.name) || entry.name.startsWith(".")) continue;
        next.push({ dir: path.join(dir, entry.name), level: level + 1 });
      }
    }
    queue = next;
  }
  return found;
}

const best = (list) => list.filter(Boolean).sort((a, b) => b.maps.length - a.maps.length)[0] || null;

// The first valid explicit choice wins; otherwise the richest folder among
// the usual places and a search of the home folder.
export async function findContent({ explicit = [], likely = [], home = os.homedir(), search = true } = {}) {
  for (const dir of explicit) {
    const content = await inspectContent(dir);
    if (content) return { ...content, source: "chosen" };
  }
  const known = best(await Promise.all(likely.map(inspectContent)));
  if (known) return { ...known, source: "found" };
  if (!search || !home) return null;
  const searched = best(await searchContent(home));
  return searched ? { ...searched, source: "searched" } : null;
}

// ---- The Steam installation ----

// Windows' reg.exe by its full path: the PATH a game is started with (by
// Steam, a shortcut, an updater hand-off) need not hold System32.
export const regExe = (env = process.env) => path.win32.join(env.SystemRoot || env.windir || "C:\\Windows", "System32", "reg.exe");

// reg.exe query's output, or "" when the key is missing.
function regQuery(args) {
  return new Promise((resolve) => {
    execFile(regExe(), ["query", ...args], { windowsHide: true, timeout: 3000, maxBuffer: 16 << 20 }, (error, stdout) => resolve(error ? "" : stdout));
  });
}

async function registry(key, value) {
  const match = new RegExp(`${value}\\s+REG_\\w+\\s+(.+)`, "i").exec(await regQuery([key, "/v", value]));
  return match ? match[1].trim() : null;
}

// Where Steam itself may live on this platform.
export async function steamRoots({ platform = process.platform, home = os.homedir(), env = process.env } = {}) {
  const roots = [];
  if (platform === "win32") {
    roots.push(await registry("HKCU\\Software\\Valve\\Steam", "SteamPath"));
    roots.push(await registry("HKLM\\SOFTWARE\\WOW6432Node\\Valve\\Steam", "InstallPath"));
    roots.push(await registry("HKLM\\SOFTWARE\\Valve\\Steam", "InstallPath"));
    roots.push(path.join(env["ProgramFiles(x86)"] || "C:\\Program Files (x86)", "Steam"));
    roots.push(path.join(env.ProgramFiles || "C:\\Program Files", "Steam"));
  } else if (platform === "darwin") {
    roots.push(path.join(home, "Library", "Application Support", "Steam"));
  } else {
    roots.push(
      path.join(home, ".steam", "steam"),
      path.join(home, ".steam", "root"),
      path.join(home, ".local", "share", "Steam"),
      path.join(home, ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam"),
      path.join(home, "snap", "steam", "common", ".local", "share", "Steam"),
    );
  }
  const unique = new Map();
  for (const root of roots.filter(Boolean)) {
    let real = path.resolve(root);
    try {
      real = await fs.realpath(real);
    } catch {
      continue;
    }
    unique.set(real.toLowerCase(), real);
  }
  return [...unique.values()];
}

// The "path" entries of steamapps/libraryfolders.vdf.
export function parseLibraryFolders(text) {
  return [...text.matchAll(/"path"\s+"((?:[^"\\]|\\.)*)"/gi)].map((found) => found[1].replace(/\\\\/g, "\\"));
}

// The "installdir" of an appmanifest_<id>.acf.
export function parseInstallDir(text) {
  return /"installdir"\s+"((?:[^"\\]|\\.)*)"/i.exec(text)?.[1].replace(/\\\\/g, "\\") || null;
}

// Every library the Steam installations list (steamapps/libraryfolders.vdf,
// and config/libraryfolders.vdf, which older clients keep), each once.
async function libraries(roots) {
  const found = new Map();
  for (const root of roots) {
    found.set(root.toLowerCase(), root);
    for (const names of [["steamapps", "libraryfolders.vdf"], ["config", "libraryfolders.vdf"]]) {
      const vdf = await walk(root, names);
      if (!vdf) continue;
      for (const library of parseLibraryFolders(await fs.readFile(vdf, "utf8").catch(() => ""))) {
        found.set(library.toLowerCase(), library);
      }
    }
  }
  return [...found.values()];
}

// Where a library may sit on a Windows drive when Steam cannot say (no
// registry entry, no libraryfolders.vdf naming it): the name Steam gives a
// new library, Steam's own folder, and the usual hand-made ones.
export const DRIVE_LIBRARIES = ["SteamLibrary", "Steam", "Program Files (x86)\\Steam", "Program Files\\Steam", "Games\\Steam", "Games\\SteamLibrary", "SteamGames"];

// The library folders (those holding steamapps) at those places on each
// drive: C: to Z: on Windows, none elsewhere (tests pass folders standing in
// for drives). Each look gives up after timeoutMs, so a network drive that
// does not answer cannot stall startup.
export async function driveLibraries({ platform = process.platform, drives = null, timeoutMs = 1500 } = {}) {
  const within = bounded(timeoutMs);
  const found = await Promise.all((await presentDrives({ platform, drives, within })).flatMap((root) => DRIVE_LIBRARIES.map(async (name) => {
    const dir = await within(walk(root, name.split("\\")));
    return dir && (await within(child(dir, "steamapps"))) ? dir : null;
  })));
  return unique(found);
}

// A look at the disk that gives up (false) after timeoutMs.
const bounded = (timeoutMs) => (check) => Promise.race([check, new Promise((resolve) => setTimeout(resolve, timeoutMs, false).unref?.())]);

// The drives that answer, C: to Z:, or the folders tests pass for them.
async function presentDrives({ platform, drives, within }) {
  if (!drives && platform !== "win32") return [];
  const roots = drives ?? [..."CDEFGHIJKLMNOPQRSTUVWXYZ"].map((letter) => `${letter}:\\`);
  return (await Promise.all(roots.map(async (root) => ((await within(exists(root))) ? root : null)))).filter(Boolean);
}

// Each folder once, whatever its capitalization.
const unique = (dirs) => [...new Map(dirs.filter(Boolean).map((dir) => [dir.toLowerCase(), dir])).values()];

// A BF1 installation: GameData/Data/_LVL_PC with the per-language
// Common/<language>/common.lvl and the SIDE archives.
export async function inspectInstall(dir) {
  if (!dir) return null;
  const lvl = await walk(dir, ["GameData", "Data", "_LVL_PC"]);
  if (!lvl || !(await child(lvl, "SIDE"))) return null;
  const common = await child(lvl, "Common");
  if (!common) return null;
  for (const entry of await entries(common)) {
    if (entry.isDirectory() && (await child(path.join(common, entry.name), "common.lvl"))) return { dir: path.resolve(dir) };
  }
  return null;
}

// The add-ons installed beside the game, GameData/AddOn/<name> with an
// addme.script and Data/_LVL_PC (the Steam copy installs Jabba's Palace as
// AddOn/Tat3): their folder names in lower case, sorted, as
// tools/prepare.py records them in .prepare.json's addOns.
export async function installedAddOns(dir) {
  const folder = dir && (await walk(dir, ["GameData", "AddOn"]));
  const names = [];
  for (const entry of folder ? await entries(folder) : []) {
    // A folder or a link to one, as the tools read it.
    const at = path.join(folder, entry.name);
    if ((await child(at, "addme.script")) && (await walk(at, ["Data", "_LVL_PC"]))) names.push(entry.name.toLowerCase());
  }
  return names.sort();
}

// The Steam copy of Battlefront (2004): its app manifest in any Steam
// library first, then any steamapps/common folder laid out like it. The
// libraries are those Steam lists, then (drives: driveLibraries' options)
// those found where libraries usually are, on any drive: a library on F:
// is found though Steam's records of it are missing.
export async function findSteamInstall({ roots, extra = [], drives = {} } = {}) {
  for (const dir of extra) {
    const install = await inspectInstall(dir);
    if (install) return { ...install, source: "local" };
  }
  const listed = await libraries(roots ?? (await steamRoots()));
  const install = await searchLibraries(listed);
  if (install || !drives) return install;
  const known = new Set(listed.map((library) => path.resolve(library).toLowerCase()));
  return searchLibraries((await driveLibraries(drives)).filter((library) => !known.has(path.resolve(library).toLowerCase())));
}

async function searchLibraries(list) {
  for (const library of list) {
    const steamapps = await child(library, "steamapps");
    if (!steamapps) continue;
    const common = await child(steamapps, "common");
    const manifest = await child(steamapps, `appmanifest_${BF1_STEAM_APP}.acf`);
    const installdir = manifest ? parseInstallDir(await fs.readFile(manifest, "utf8").catch(() => "")) : null;
    if (common && installdir) {
      const install = await inspectInstall(await child(common, installdir));
      if (install) return { ...install, source: "steam" };
    }
    for (const entry of common ? await entries(common) : []) {
      if (!entry.isDirectory()) continue;
      const install = await inspectInstall(path.join(common, entry.name));
      if (install) return { ...install, source: "steam" };
    }
  }
  return null;
}

// ---- The GOG installation ----

// GOG's store page for STAR WARS Battlefront (Classic, 2004).
export const BF1_GOG_PAGE = "https://www.gog.com/game/star_wars_battlefront";

// Where GOG's installers and GOG Galaxy record each game they install: a key
// per game with its gameName and path (BF1: 1668107107, "STAR WARS
// Battlefront").
const GOG_REGISTRY = ["HKLM\\SOFTWARE\\WOW6432Node\\GOG.com\\Games", "HKLM\\SOFTWARE\\GOG.com\\Games"];

// `reg query <GOG_REGISTRY> /s`: each game's name and folder.
export function parseGogRegistry(text) {
  const games = [];
  let game = null;
  for (const line of text.split(/\r?\n/)) {
    if (/^HKEY_/i.test(line)) games.push((game = {}));
    else {
      const match = game && /^\s+(\S+)\s+REG_\w+\s+(.*)$/.exec(line);
      if (match) game[match[1].toLowerCase()] = match[2].trim();
    }
  }
  return games.filter((entry) => entry.path).map((entry) => ({ name: entry.gamename || "", path: entry.path }));
}

// Heroic Games Launcher's gog_store/installed.json (how GOG games are
// usually played on Linux): each game's folder. Heroic records no name.
export function parseHeroicInstalled(text) {
  let json;
  try {
    json = JSON.parse(text);
  } catch {
    return [];
  }
  const list = Object.values(json && typeof json === "object" ? json : {}).flat();
  return list.filter((entry) => typeof entry?.install_path === "string").map((entry) => ({ name: "", path: entry.install_path }));
}

function heroicFiles({ platform, home, env }) {
  const file = (dir) => path.join(dir, "heroic", "gog_store", "installed.json");
  if (platform === "win32") return env.APPDATA ? [file(env.APPDATA)] : [];
  if (platform === "darwin") return [file(path.join(home, "Library", "Application Support"))];
  return [file(env.XDG_CONFIG_HOME || path.join(home, ".config")), file(path.join(home, ".var", "app", "com.heroicgameslauncher.hgl", "config"))];
}

// The game folders GOG's records name, those that say Battlefront: the
// registry on Windows, Heroic's records everywhere it runs.
export async function gogRecords({ platform = process.platform, home = os.homedir(), env = process.env } = {}) {
  const games = [];
  if (platform === "win32") {
    for (const key of GOG_REGISTRY) games.push(...parseGogRegistry(await regQuery([key, "/s"])));
  }
  for (const file of heroicFiles({ platform, home, env })) {
    games.push(...parseHeroicInstalled(await fs.readFile(file, "utf8").catch(() => "")));
  }
  return unique(games.filter((game) => /battlefront/i.test(`${game.name} ${game.path}`)).map((game) => game.path));
}

// Where GOG games go when nothing records them, as the folders holding the
// game's own ("Star Wars - Battlefront"): the offline installers' GOG Games
// and GOG Galaxy's Games on each Windows drive; elsewhere Heroic's ~/Games/
// Heroic, and the same Windows folders inside Wine prefixes (the default
// ~/.wine, Lutris's one per game in ~/Games, CrossOver's bottles).
export const DRIVE_GOG_FOLDERS = ["GOG Games", "GOG Galaxy\\Games", "Program Files (x86)\\GOG Galaxy\\Games", "Program Files\\GOG Galaxy\\Games", "Games"];
const PREFIX_GOG_FOLDERS = [["drive_c", "GOG Games"], ["drive_c", "Program Files (x86)", "GOG Galaxy", "Games"], ["drive_c", "Program Files", "GOG Galaxy", "Games"]];

export async function gogFolders({ platform = process.platform, home = os.homedir(), drives = null, timeoutMs = 1500 } = {}) {
  const within = bounded(timeoutMs);
  const found = [];
  for (const root of await presentDrives({ platform, drives, within })) {
    found.push(...(await Promise.all(DRIVE_GOG_FOLDERS.map((name) => within(walk(root, name.split("\\")))))));
  }
  if (platform !== "win32" && home) {
    const games = await child(home, "Games");
    found.push(games, games && (await child(games, "Heroic")), await child(home, "GOG Games"));
    const prefixes = [await child(home, ".wine")];
    for (const parent of [games, await walk(home, ["Library", "Application Support", "CrossOver", "Bottles"])]) {
      for (const entry of parent ? await entries(parent) : []) if (entry.isDirectory()) prefixes.push(path.join(parent, entry.name));
    }
    for (const prefix of prefixes.filter(Boolean)) {
      for (const names of PREFIX_GOG_FOLDERS) found.push(await walk(prefix, names));
    }
  }
  return unique(found);
}

// The GOG copy of Battlefront (2004): where GOG's records say, then a
// Battlefront folder where GOG games usually go. Its layout is the Steam
// copy's (GameData/Data/_LVL_PC). options: gogRecords' and gogFolders'.
export async function findGogInstall(options = {}) {
  for (const dir of await gogRecords(options)) {
    const install = await inspectInstall(dir);
    if (install) return { ...install, source: "gog" };
  }
  for (const parent of await gogFolders(options)) {
    for (const entry of await entries(parent)) {
      if (!entry.isDirectory() || !/battlefront/i.test(entry.name)) continue;
      const install = await inspectInstall(path.join(parent, entry.name));
      if (install) return { ...install, source: "gog" };
    }
  }
  return null;
}

// The player's copy: a folder they chose (extra), then Steam's, the
// authenticity reference, then GOG's.
export async function findGameInstall({ extra = [], steam = {}, gog = {} } = {}) {
  return (await findSteamInstall({ ...steam, extra })) || (await findGogInstall(gog));
}
