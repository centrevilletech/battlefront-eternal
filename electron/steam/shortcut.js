// Battlefront Eternal's entry among the player's non-Steam games
// (shortcuts.vdf, vdf.js). Steam starts the game from it, so friends see
// "playing Battlefront Eternal" while it runs.
//
// The entry's app id is Steam's own scheme for shortcuts, as Steam ROM
// Manager and BoilR compute it: the CRC-32 of the Exe and the name, with
// the top bit set. It is chosen once and then kept, so a moved executable
// keeps its library entry, play time and artwork. The 64-bit id that
// steam://rungameid/ takes is (app id << 32) | 0x02000000.
import { crc32 } from "node:zlib";
import { field, parseShortcuts, serializeShortcuts, TYPE } from "./vdf.js";

export const SHORTCUT_NAME = "Battlefront Eternal";
// In the entry's launch options: this start came from Steam. Steam's own
// environment (SteamGameId) says so too, but not through a Flatpak Steam's
// host spawn, which passes arguments and no environment.
export const STEAM_ARG = "--steam";

export const shortcutAppId = (exe, name = SHORTCUT_NAME) => (crc32(`${exe}${name}`) | 0x80000000) >>> 0;
export const rungameId = (appid) => ((BigInt(appid >>> 0) << 32n) | 0x02000000n).toString();
export const quoted = (file) => `"${file}"`;

// The fields of a new entry, in the order and with the types the Steam
// client writes them. The overlay starts off: Chromium's GPU process and
// the Steam overlay's hooks get along badly (docs/presence.md), and the
// player can turn it on in the shortcut's properties.
export function newShortcut({ appid, exe, startDir, icon = "", launchOptions = "", name = SHORTCUT_NAME }) {
  return [
    { type: TYPE.INT32, key: "appid", value: appid | 0 },
    { type: TYPE.STRING, key: "AppName", value: name },
    { type: TYPE.STRING, key: "Exe", value: exe },
    { type: TYPE.STRING, key: "StartDir", value: startDir },
    { type: TYPE.STRING, key: "icon", value: icon },
    { type: TYPE.STRING, key: "ShortcutPath", value: "" },
    { type: TYPE.STRING, key: "LaunchOptions", value: launchOptions },
    { type: TYPE.INT32, key: "IsHidden", value: 0 },
    { type: TYPE.INT32, key: "AllowDesktopConfig", value: 1 },
    { type: TYPE.INT32, key: "AllowOverlay", value: 0 },
    { type: TYPE.INT32, key: "OpenVR", value: 0 },
    { type: TYPE.INT32, key: "Devkit", value: 0 },
    { type: TYPE.STRING, key: "DevkitGameID", value: "" },
    { type: TYPE.INT32, key: "DevkitOverrideAppID", value: 0 },
    { type: TYPE.INT32, key: "LastPlayTime", value: 0 },
    { type: TYPE.STRING, key: "FlatpakAppID", value: "" },
    { type: TYPE.MAP, key: "tags", value: [] },
  ];
}

const appidOf = (entry) => Number(field(entry.fields, "appid")) >>> 0;

// Ours: the app id this app chose, or (when that was lost) an entry that
// starts Battlefront Eternal with the Steam argument.
export function findShortcut(entries, { appid = null } = {}) {
  if (appid !== null) {
    const index = entries.findIndex((entry) => appidOf(entry) === appid >>> 0);
    if (index >= 0) return index;
  }
  return entries.findIndex((entry) => {
    const launch = String(field(entry.fields, "LaunchOptions") ?? ""), exe = String(field(entry.fields, "Exe") ?? "");
    return launch.split(/\s+/).includes(STEAM_ARG) && /battlefront[ -]eternal/i.test(`${exe} ${launch}`);
  });
}

// What the entry is: { appid, name, exe, launchOptions, startDir, overlay }.
export function describeShortcut(entry) {
  if (!entry) return null;
  return {
    appid: appidOf(entry),
    name: String(field(entry.fields, "AppName") ?? ""),
    exe: String(field(entry.fields, "Exe") ?? ""),
    startDir: String(field(entry.fields, "StartDir") ?? ""),
    launchOptions: String(field(entry.fields, "LaunchOptions") ?? ""),
    overlay: Number(field(entry.fields, "AllowOverlay") ?? 1) !== 0,
  };
}

// Sets one field of an entry, keeping its place (or adding it last).
function setField(fields, key, type, value) {
  const lower = key.toLowerCase();
  const found = fields.find((item) => item.key.toLowerCase() === lower);
  if (found) {
    if (found.type === type && found.value === value) return false;
    found.type = type;
    found.value = value;
  } else fields.push({ type, key, value });
  return true;
}

// Whether an entry already starts the game as wanted: the same program and
// folder, and launch options that still name ours, wherever the player put
// their own around them (`gamemoderun %command% --steam`).
export function shortcutCurrent(entry, { exe, startDir, launchOptions }) {
  const options = String(field(entry.fields, "LaunchOptions") ?? "");
  return field(entry.fields, "Exe") === exe && field(entry.fields, "StartDir") === startDir && (options === launchOptions || options.includes(launchOptions));
}

// shortcuts.vdf's bytes with the entry added, or updated where it already
// is. Only how it starts (Exe, StartDir, the launch options and an
// empty icon) is changed in an existing entry; the player's name, artwork,
// overlay choice, collections and play time stay. Returns { buffer,
// appid, changed, added }; throws when the file cannot be read as a list
// of shortcuts, so nothing is ever written over a file not understood.
export function upsertShortcut(buffer, { appid = null, exe, startDir, icon = "", launchOptions }) {
  const list = parseShortcuts(buffer);
  const index = findShortcut(list.entries, { appid });
  if (index < 0) {
    const id = appid ?? shortcutAppId(exe);
    const fields = newShortcut({ appid: id, exe, startDir, icon, launchOptions });
    list.entries.push({ key: String(list.entries.length), fields, raw: null });
    return { buffer: serializeShortcuts(list), appid: id >>> 0, changed: true, added: true };
  }
  const entry = list.entries[index];
  let changed = false;
  if (!shortcutCurrent(entry, { exe, startDir, launchOptions })) {
    setField(entry.fields, "Exe", TYPE.STRING, exe);
    setField(entry.fields, "StartDir", TYPE.STRING, startDir);
    setField(entry.fields, "LaunchOptions", TYPE.STRING, launchOptions);
    changed = true;
  }
  if (icon && !field(entry.fields, "icon")) changed = setField(entry.fields, "icon", TYPE.STRING, icon) || changed;
  if (changed) entry.raw = null;
  return { buffer: changed ? serializeShortcuts(list) : buffer, appid: appidOf(entry), changed, added: false };
}

// shortcuts.vdf's bytes without the entry: { buffer, removed }.
export function removeShortcut(buffer, { appid = null } = {}) {
  const list = parseShortcuts(buffer);
  const index = findShortcut(list.entries, { appid });
  if (index < 0) return { buffer, removed: false };
  list.entries.splice(index, 1);
  return { buffer: serializeShortcuts(list), removed: true };
}
