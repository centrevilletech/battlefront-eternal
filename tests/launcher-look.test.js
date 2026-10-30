// The desktop launcher (electron/ui/prepare.html) in the front end's looks:
// it picks the player's look as the front end reads its settings, and the
// looks, the logo and the font are the page theme's, defined once.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const read = (file) => readFileSync(new URL(`../${file}`, import.meta.url), "utf8");
const LOOKS = read("electron/ui/looks.css");
const PICK = read("electron/ui/theme.js");
const LAUNCHER = read("electron/ui/prepare.html");
const LAUNCHER_STYLE = read("electron/ui/prepare.css");
const PAGE_THEME = read("src/menus/themes/page/index.js");
const PAGE_STYLE = read("src/menus/themes/page/style.js");

// The page theme's looks and default, from its source (its styles are a
// Vite import, which plain Node can't load).
const THEMES = [...PAGE_THEME.match(/export const THEMES = \[([\s\S]*?)\];/)[1].matchAll(/id: "(\w+)"/g)].map((match) => match[1]);
const DEFAULT_THEME = /export const DEFAULT_THEME = "(\w+)"/.exec(PAGE_THEME)[1];

function memoryStorage(entries) {
  const map = new Map(Object.entries(entries));
  return {
    getItem: (key) => (map.has(key) ? map.get(key) : null),
    setItem: (key, value) => map.set(key, String(value)),
    removeItem: (key) => map.delete(key),
  };
}

// localStorage's entries for a case: the current key's record, the old
// name's, or text as it is.
const entries = ({ local, legacy, raw }) => ({
  ...(local ? { "battlefront-eternal.settings": JSON.stringify(local) } : {}),
  ...(legacy ? { "battlefront-classic.settings": JSON.stringify(legacy) } : {}),
  ...(raw ? { "battlefront-eternal.settings": raw } : {}),
});

// The desktop's settings store (electron/preload.cjs settingsStore) returning
// `read`, throwing it when it is an Error; none when undefined.
const storeOf = (read) => (read === undefined ? null : {
  read: () => {
    if (read instanceof Error) throw read;
    return read;
  },
  write: () => assert.fail("the launcher never writes the settings"),
});

// The look the launcher's theme.js sets on <html>.
function launcherLook(testCase) {
  const html = { dataset: {} };
  const store = storeOf(testCase.read);
  vm.runInNewContext(PICK, { window: store ? { bfeDesktop: { settingsStore: store } } : {}, document: { documentElement: html }, localStorage: memoryStorage(entries(testCase)) });
  return html.dataset.bfeTheme;
}

// The look the front end shows for the same settings (src/settings.js, then
// the page theme's themeOf).
async function frontEndLook(testCase) {
  globalThis.localStorage = memoryStorage(entries(testCase));
  const { Settings } = await import("../src/settings.js");
  const store = storeOf(testCase.read);
  const value = new Settings({}, { store: store && { read: store.read, write() {} } }).get("menuTheme");
  return THEMES.includes(value) ? value : DEFAULT_THEME;
}

const file = (values, complete = true) => ({ record: { complete, values }, shadowed: !complete });
const CASES = [
  { name: "no settings yet", read: { record: null, shadowed: false }, expect: DEFAULT_THEME },
  { name: "no desktop store, nothing stored", expect: DEFAULT_THEME },
  ...THEMES.map((id) => ({ name: `the settings file's ${id}`, read: file({ menuTheme: id }), local: { menuTheme: "unknown" }, expect: id })),
  { name: "a complete file without a look", read: file({ playerName: "REBEL" }), local: { menuTheme: "bf2" }, expect: DEFAULT_THEME },
  { name: "an incomplete file over localStorage", read: file({ menuTheme: "bf2" }, false), local: { menuTheme: "bf1" }, expect: "bf2" },
  { name: "an incomplete file, localStorage's look", read: file({ playerName: "REBEL" }, false), local: { menuTheme: "bf1" }, expect: "bf1" },
  { name: "localStorage alone", read: { record: null, shadowed: false }, local: { menuTheme: "bf2" }, expect: "bf2" },
  { name: "the old name's settings", read: { record: null, shadowed: false }, legacy: { menuTheme: "bf1" }, expect: "bf1" },
  { name: "a store that throws", read: new Error("closing"), local: { menuTheme: "bf2" }, expect: "bf2" },
  { name: "no desktop store", local: { menuTheme: "bf1" }, expect: "bf1" },
  { name: "a look that doesn't exist", read: file({ menuTheme: "classic" }), expect: DEFAULT_THEME },
  { name: "unreadable localStorage", read: { record: null, shadowed: false }, raw: "{not json", expect: DEFAULT_THEME },
];

test("the launcher picks the look the front end shows, Eternal without settings", async () => {
  assert.equal(DEFAULT_THEME, "eternal");
  assert.ok(THEMES.length >= 3);
  for (const testCase of CASES) {
    assert.equal(launcherLook(testCase), testCase.expect, testCase.name);
    assert.equal(await frontEndLook(testCase), testCase.expect, `${testCase.name} (front end)`);
  }
});

test("the look is set before the launcher's first frame", () => {
  const head = LAUNCHER.slice(0, LAUNCHER.indexOf("</head>"));
  const pick = head.indexOf('<script src="theme.js"></script>');
  assert.ok(pick > 0, "theme.js is a plain, blocking script in the head");
  for (const later of ['<link rel="stylesheet"', '<script type="module"']) assert.ok(head.indexOf(later) > pick, `${later} comes after theme.js`);
});

test("the looks are defined once, in electron/ui/looks.css", () => {
  const blocks = THEMES.map((id) => {
    const block = new RegExp(`html\\[data-bfe-theme="${id}"\\] \\{([^}]*)\\}`).exec(LOOKS);
    assert.ok(block, `looks.css defines the ${id} look`);
    return [...block[1].matchAll(/(--bfe-[\w-]+):/g)].map((match) => match[1]).sort();
  });
  for (const names of blocks.slice(1)) assert.deepEqual(names, blocks[0], "every look sets the same properties");
  assert.ok(blocks[0].includes("--bfe-accent") && blocks[0].includes("--bfe-logo-fill"));
  // The page theme takes them from there, with no copy of its own.
  assert.match(PAGE_STYLE, /^import \w+ from "\.\.\/\.\.\/\.\.\/\.\.\/electron\/ui\/looks\.css\?inline";$/m);
  assert.doesNotMatch(PAGE_STYLE, /html\[data-bfe-theme="\w+"\] \{/);
  assert.doesNotMatch(PAGE_STYLE, /@font-face|^\.bfe-logo \{|^\.bfe-btn \{|^\.bfe-text-title \{/m);
  // The launcher only uses them.
  assert.doesNotMatch(LAUNCHER_STYLE, /--bfe-[\w-]+\s*:/);
  assert.match(LAUNCHER, /<link rel="stylesheet" href="looks\.css" \/>/);
});

test("the launcher draws the front end's logo", () => {
  // As the page theme's logo() builds it.
  assert.match(PAGE_THEME, /make\("div", "bfe-logo"\)[\s\S]*?"aria-label", "Battlefront Eternal"[\s\S]*?make\("span", "word", "Battlefront"\)[\s\S]*?dataset\.text = "Battlefront"[\s\S]*?make\("span", "tag", "Eternal"\)/);
  assert.ok(LAUNCHER.includes('<div class="bfe-logo" role="img" aria-label="Battlefront Eternal"><span class="word" data-text="Battlefront">Battlefront</span><span class="tag">Eternal</span></div>'));
});

test("the looks' font ships beside them, with its license", () => {
  const font = /@font-face \{ font-family: "BFE Saira"; src: url\("([^"]+)"\)/.exec(LOOKS)?.[1];
  assert.equal(font, "fonts/saira-latin.woff2");
  assert.ok(existsSync(new URL(`../electron/ui/${font}`, import.meta.url)));
  assert.match(read("electron/ui/fonts/OFL.txt"), /SIL OPEN FONT LICENSE Version 1\.1/);
  assert.match(read("electron-builder.yml"), /- from: electron\/ui\/fonts\n\s+to: legal\n\s+filter: \["OFL\.txt"\]/);
});
