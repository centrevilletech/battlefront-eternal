import assert from "node:assert/strict";
import { test } from "node:test";
import { field, parseShortcuts, parseTextVdf, serializeShortcuts, TYPE, writeMap } from "../electron/steam/vdf.js";
import { describeShortcut, findShortcut, removeShortcut, rungameId, shortcutAppId, STEAM_ARG, upsertShortcut } from "../electron/steam/shortcut.js";

// A shortcut as the Steam client writes it (field order and types of a
// 2025-2026 client), built byte by byte rather than with writeMap.
function steamEntry({ appid, name, exe, dir, launch = "", tags = ["Favorites"], lastPlay = 1760000000, extra = [] }) {
  const parts = [];
  const str = (key, value) => parts.push(Buffer.from([1]), Buffer.from(`${key}\0${value}\0`, "utf8"));
  const int = (key, value) => { const b = Buffer.alloc(4); b.writeInt32LE(value); parts.push(Buffer.from([2]), Buffer.from(`${key}\0`), b); };
  int("appid", appid);
  str("AppName", name);
  str("Exe", exe);
  str("StartDir", dir);
  str("icon", "");
  str("ShortcutPath", "");
  str("LaunchOptions", launch);
  int("IsHidden", 0);
  int("AllowDesktopConfig", 1);
  int("AllowOverlay", 1);
  int("OpenVR", 0);
  int("Devkit", 0);
  str("DevkitGameID", "");
  int("DevkitOverrideAppID", 0);
  int("LastPlayTime", lastPlay);
  str("FlatpakAppID", "");
  for (const [type, key, bytes] of extra) parts.push(Buffer.from([type]), Buffer.from(`${key}\0`), bytes);
  parts.push(Buffer.from([0]), Buffer.from("tags\0"));
  tags.forEach((tag, i) => str(String(i), tag));
  parts.push(Buffer.from([8, 8]));
  return Buffer.concat(parts);
}

function steamFile(entries) {
  return Buffer.concat([
    Buffer.from("\0shortcuts\0", "utf8"),
    ...entries.flatMap((entry, i) => [Buffer.from(`\0${i}\0`, "utf8"), entry]),
    Buffer.from([8, 8]),
  ]);
}

test("a Steam-written shortcuts.vdf parses and is written back byte for byte", () => {
  const uint64 = Buffer.alloc(8);
  uint64.writeBigUInt64LE(0xfedcba9876543210n);
  const file = steamFile([
    steamEntry({ appid: -1234567890, name: "Café Ünïcode ゲーム", exe: '"C:\\Games\\Some Game\\game.exe"', dir: '"C:\\Games\\Some Game\\"', launch: "-windowed" }),
    steamEntry({ appid: -42, name: "Emulator", exe: '"/usr/bin/retroarch"', dir: '"/usr/bin/"', tags: [], extra: [[TYPE.UINT64, "Future", uint64]] }),
  ]);
  const parsed = parseShortcuts(file);
  assert.equal(parsed.entries.length, 2);
  assert.equal(field(parsed.entries[0].fields, "appname"), "Café Ünïcode ゲーム");
  assert.equal(field(parsed.entries[0].fields, "AppId"), -1234567890);
  assert.deepEqual(field(parsed.entries[0].fields, "tags"), [{ type: TYPE.STRING, key: "0", value: "Favorites" }]);
  assert.equal(field(parsed.entries[1].fields, "Future"), 0xfedcba9876543210n);
  assert.ok(serializeShortcuts(parsed).equals(file));
});

test("re-serializing a parsed entry's fields gives Steam's own bytes", () => {
  const entry = steamEntry({ appid: -7, name: "Battlefront Eternal", exe: '"/opt/Battlefront Eternal/battlefront-eternal"', dir: '"/opt/Battlefront Eternal/"' });
  const parsed = parseShortcuts(steamFile([entry]));
  assert.ok(writeMap(parsed.entries[0].fields).equals(entry));
});

test("an empty or missing file is an empty list, and an empty list is what Steam writes", () => {
  assert.deepEqual(parseShortcuts(Buffer.alloc(0)).entries, []);
  assert.deepEqual(parseShortcuts(null).entries, []);
  const empty = serializeShortcuts({ entries: [] });
  assert.ok(empty.equals(Buffer.from("\0shortcuts\0\b\b", "latin1")));
  assert.deepEqual(parseShortcuts(empty).entries, []);
});

test("entries are renumbered in order when one is removed", () => {
  const a = steamEntry({ appid: -1, name: "A", exe: '"a"', dir: '""' });
  const b = steamEntry({ appid: -2, name: "B", exe: '"b"', dir: '""' });
  const c = steamEntry({ appid: -3, name: "C", exe: '"c"', dir: '""' });
  const parsed = parseShortcuts(steamFile([a, b, c]));
  parsed.entries.splice(1, 1);
  assert.ok(serializeShortcuts(parsed).equals(steamFile([a, c])));
});

test("a file that is not a shortcut list, or is cut short, is refused", () => {
  const good = steamFile([steamEntry({ appid: -1, name: "A", exe: '"a"', dir: '""' })]);
  assert.throws(() => parseShortcuts(good.subarray(0, good.length - 5)), /ends early|unterminated/);
  assert.throws(() => parseShortcuts(Buffer.concat([good, Buffer.from("junk")])), /after/);
  assert.throws(() => parseShortcuts(Buffer.from("\0config\0\b\b", "latin1")), /starts with "config"/);
  assert.throws(() => parseShortcuts(Buffer.from("\0shortcuts\0\0" + "0\0" + "\x09bad\0\b\b\b", "latin1")), /unknown field type/);
  // Nested maps beyond any real file.
  const deep = Buffer.concat([Buffer.from("\0shortcuts\0\0" + "0\0", "latin1"), Buffer.from("\0x\0".repeat(64), "latin1"), Buffer.from("\b".repeat(67), "latin1")]);
  assert.throws(() => parseShortcuts(deep), /nests too deeply/);
});

test("text KeyValues: loginusers.vdf and registry.vdf as Steam writes them", () => {
  const loginusers = `"users"
{
\t"76561198000000001"
\t{
\t\t"AccountName"\t\t"john_doe"
\t\t"PersonaName"\t\t"John \\"JD\\" Doe"
\t\t"RememberPassword"\t\t"1"
\t\t"MostRecent"\t\t"1"
\t\t"Timestamp"\t\t"1760000000"
\t}
\t"76561198000000002"
\t{
\t\t"AccountName"\t\t"other"
\t\t"MostRecent"\t\t"0"
\t}
}
`;
  const users = parseTextVdf(loginusers).users;
  assert.equal(users["76561198000000001"].PersonaName, 'John "JD" Doe');
  assert.equal(users["76561198000000002"].MostRecent, "0");
  const registry = `"Registry"\n{\n\t"HKCU"\n\t{\n\t\t"Software"\n\t\t{\n\t\t\t"Valve"\n\t\t\t{\n\t\t\t\t"Steam"\n\t\t\t\t{\n\t\t\t\t\t"ActiveProcess"\n\t\t\t\t\t{\n\t\t\t\t\t\t"pid"\t\t"4242"\n\t\t\t\t\t\t"ActiveUser"\t\t"39734273"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n`;
  assert.equal(parseTextVdf(registry).Registry.HKCU.Software.Valve.Steam.ActiveProcess.ActiveUser, "39734273");
});

// ---- The library entry ----

test("the entry's app id is Steam's own for shortcuts, and rungameid's id follows from it", () => {
  // Steam ROM Manager's scheme: crc32(exe + name) | 0x80000000.
  const exe = '"/home/john/.local/bin/battlefront-eternal"';
  const appid = shortcutAppId(exe, "Battlefront Eternal");
  assert.ok(appid >= 0x80000000 && appid <= 0xffffffff);
  assert.equal(appid, shortcutAppId(exe));
  assert.notEqual(appid, shortcutAppId('"/opt/Battlefront Eternal/battlefront-eternal"'));
  // A known value: crc32("ab") is 0x9e83486d; with the top bit, 0x9e83486d.
  assert.equal(shortcutAppId("a", "b"), 0x9e83486d);
  assert.equal(shortcutAppId("", ""), 0x80000000);
  assert.equal(rungameId(0x9e83486d), String((0x9e83486dn << 32n) | 0x02000000n));
});

test("adding writes Steam's fields once; again changes nothing; the player's other games are untouched", () => {
  const other = steamEntry({ appid: -5, name: "RetroArch", exe: '"/usr/bin/retroarch"', dir: '"/usr/bin/"' });
  const file = steamFile([other]);
  const launch = { exe: '"/home/john/.local/bin/battlefront-eternal"', startDir: '"/home/john"', launchOptions: STEAM_ARG };
  const added = upsertShortcut(file, { ...launch, icon: "/grid/1_icon.png" });
  assert.equal(added.added, true);
  const list = parseShortcuts(added.buffer);
  assert.equal(list.entries.length, 2);
  assert.ok(list.entries[0].raw.equals(other), "the other entry's bytes");
  const ours = describeShortcut(list.entries[1]);
  assert.deepEqual(ours, { appid: added.appid, name: "Battlefront Eternal", exe: launch.exe, startDir: launch.startDir, launchOptions: STEAM_ARG, overlay: false });
  assert.equal(field(list.entries[1].fields, "icon"), "/grid/1_icon.png");
  assert.equal(field(list.entries[1].fields, "AllowDesktopConfig"), 1);
  const again = upsertShortcut(added.buffer, { appid: added.appid, ...launch });
  assert.equal(again.changed, false);
  assert.ok(again.buffer.equals(added.buffer));
});

test("a moved launcher updates only how the entry starts; the player's name, overlay, options and tags stay", () => {
  const launch = { exe: '"/old/battlefront-eternal"', startDir: '"/old/"', launchOptions: STEAM_ARG };
  const first = upsertShortcut(null, launch);
  // The player renames it, turns the overlay on and adds an option in Steam.
  const list = parseShortcuts(first.buffer);
  const fields = list.entries[0].fields;
  fields.find((f) => f.key === "AppName").value = "BF Eternal";
  fields.find((f) => f.key === "AllowOverlay").value = 1;
  fields.find((f) => f.key === "LaunchOptions").value = `${STEAM_ARG} --enable-logging`;
  fields.find((f) => f.key === "tags").value = [{ type: TYPE.STRING, key: "0", value: "Shooters" }];
  list.entries[0].raw = null;
  const edited = serializeShortcuts(list);
  // Its own options after ours: still current.
  assert.equal(upsertShortcut(edited, { appid: first.appid, ...launch }).changed, false);
  const moved = upsertShortcut(edited, { appid: first.appid, exe: '"/new/battlefront-eternal"', startDir: '"/new/"', launchOptions: STEAM_ARG });
  assert.equal(moved.changed, true);
  assert.equal(moved.appid, first.appid);
  const after = parseShortcuts(moved.buffer).entries[0];
  assert.deepEqual(describeShortcut(after), { appid: first.appid, name: "BF Eternal", exe: '"/new/battlefront-eternal"', startDir: '"/new/"', launchOptions: STEAM_ARG, overlay: true });
  assert.deepEqual(field(after.fields, "tags"), [{ type: TYPE.STRING, key: "0", value: "Shooters" }]);
});

test("an entry is ours by its app id, or by starting Battlefront Eternal from Steam; removing takes only it", () => {
  const theirs = steamEntry({ appid: -9, name: "Battlefront Eternal (manual)", exe: '"/opt/Battlefront Eternal/battlefront-eternal"', dir: '"/opt/Battlefront Eternal/"' });
  const ours = steamEntry({ appid: -1234, name: "Battlefront Eternal", exe: '"/home/j/.local/bin/battlefront-eternal"', dir: '"/home/j"', launch: STEAM_ARG });
  const file = steamFile([theirs, ours]);
  const { entries } = parseShortcuts(file);
  assert.equal(findShortcut(entries, { appid: -1234 >>> 0 }), 1);
  assert.equal(findShortcut(entries, {}), 1, "found without the app id");
  assert.equal(findShortcut(entries, { appid: 7 }), 1, "an unknown id falls back to the launch options");
  const { buffer, removed } = removeShortcut(file, { appid: -1234 >>> 0 });
  assert.equal(removed, true);
  assert.ok(buffer.equals(steamFile([theirs])));
  assert.equal(removeShortcut(buffer, {}).removed, false);
});
