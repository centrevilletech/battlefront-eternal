import test from "node:test";
import assert from "node:assert/strict";
import { DEFAULTS, PERFORMANCE_PRESETS, RENDER_SCALE_MIN } from "../src/settings.js";
import { UPSCALERS } from "../src/upscaler.js";
import { OPTIONS, OPTION_KEYS, PROFILE_KEYS, THEME_KEYS, PAGES, optionsFor, optionRow, applies, resetPage, NOTES, enhancedSoundNote, renderResolution } from "../src/menus/options.js";
import { optionsMenu, gamePage, videoPage, audioPage, controlsPage } from "../src/menus/option-pages.js";
import { ScreenStack } from "../src/menus/stack.js";

// A Settings stand-in: values and the order things were set.
function settingsStub(values = {}) {
  const stored = { ...DEFAULTS, ...values };
  return {
    values: stored,
    sets: [],
    get: (key) => stored[key],
    set(key, value) {
      this.sets.push([key, value]);
      stored[key] = value;
    },
  };
}

test("every setting is on exactly one of the four pages (the profile's and the look's on the front end's own picker)", () => {
  const keys = Object.keys(DEFAULTS).filter((key) => !PROFILE_KEYS.has(key) && !THEME_KEYS.has(key));
  for (const key of PROFILE_KEYS) assert.ok(key in DEFAULTS && !OPTION_KEYS.has(key), `${key} is the profile's`);
  for (const key of THEME_KEYS) assert.ok(key in DEFAULTS && !OPTION_KEYS.has(key), `${key} is the look picker's`);
  for (const key of keys) assert.ok(OPTION_KEYS.has(key), `${key} is on a page`);
  assert.equal(OPTIONS.length, keys.length, "one entry each, nothing extra");
  const pages = new Set(PAGES.map((page) => page.id));
  assert.deepEqual([...pages], ["game", "video", "audio", "controls"]);
  for (const entry of OPTIONS) assert.ok(pages.has(entry.page), `${entry.key} on ${entry.page}`);
});

test("when a change applies: live, on the next map, on the next battle, or never", () => {
  assert.equal(applies("textureCompression"), "map");
  for (const key of ["aiMode", "aiDifficulty", "aiHeroes", "battleSize"]) assert.equal(applies(key), "battle", key);
  assert.equal(applies("physicsRate"), "fixed");
  for (const key of ["renderScale", "shadows", "masterVolume", "friendlyFire", "mouseSensitivity", "audioOutput"]) assert.equal(applies(key), "live", key);
  for (const entry of OPTIONS) assert.ok(["live", "map", "battle", "fixed"].includes(entry.apply), `${entry.key} says when it applies`);
  // The rows say so while focused.
  const settings = settingsStub();
  assert.equal(optionRow(OPTIONS.find((e) => e.key === "textureCompression"), settings).note, NOTES.map);
  assert.equal(optionRow(OPTIONS.find((e) => e.key === "aiMode"), settings, { inBattle: true }).note, NOTES.battle);
  assert.equal(optionRow(OPTIONS.find((e) => e.key === "shadows"), settings).note, null);
});

test("the render scale goes down to a quarter, and its row says how big the 3D view is", () => {
  const scale = OPTIONS.find((e) => e.key === "renderScale");
  assert.equal(scale.min, RENDER_SCALE_MIN);
  assert.equal(RENDER_SCALE_MIN, 0.25);
  for (const preset of Object.values(PERFORMANCE_PRESETS)) {
    const value = preset.values.renderScale;
    assert.ok(value >= scale.min && value <= scale.max, `${preset.label} is on the slider`);
  }
  assert.deepEqual(renderResolution(0.5, { innerWidth: 1920, innerHeight: 1080, devicePixelRatio: 1 }), { width: 960, height: 540, windowWidth: 1920, windowHeight: 1080 });
  // The pixels the canvas has: CSS pixels times the display's ratio (at most 2).
  assert.deepEqual(renderResolution(0.25, { innerWidth: 1280, innerHeight: 720, devicePixelRatio: 3 }), { width: 640, height: 360, windowWidth: 2560, windowHeight: 1440 });
  assert.equal(renderResolution(1, null), null);
  const settings = settingsStub({ renderScale: 0.5 });
  const row = optionRow(scale, settings);
  assert.equal(typeof row.note, "function", "the note follows the setting");
  const view = globalThis.window;
  globalThis.window = { innerWidth: 1920, innerHeight: 1080, devicePixelRatio: 1 };
  try {
    assert.equal(row.note(), "3D VIEW 960X540, WINDOW 1920X1080");
    settings.set("renderScale", 0.25);
    assert.equal(row.note(), "3D VIEW 480X270, WINDOW 1920X1080");
  } finally {
    if (view === undefined) delete globalThis.window; else globalThis.window = view;
  }
});

test("each upscaler's row says what it costs", () => {
  const entry = OPTIONS.find((e) => e.key === "upscaler");
  assert.deepEqual(entry.choices.map(([value]) => value), Object.keys(UPSCALERS));
  assert.equal(entry.choices.find(([value]) => value === "bilinear")[1], "BILINEAR");
  const settings = settingsStub();
  const row = optionRow(entry, settings);
  for (const mode of Object.keys(UPSCALERS)) {
    settings.set("upscaler", mode);
    assert.match(row.note(), /^[A-Z0-9 ,:]+$/, `${mode}: the 2004 font's letters`);
    assert.ok(row.note().length <= 80, `${mode}: one line in the classic menu`);
  }
});

test("each entry's choices hold its default, and a slider's range does", () => {
  for (const entry of OPTIONS) {
    const value = DEFAULTS[entry.key];
    if (entry.kind === "choice") assert.ok(entry.choices.some(([v]) => String(v) === String(value)), `${entry.key}'s default ${value} is a choice`);
    else if (entry.kind === "toggle") assert.equal(typeof value, "boolean", entry.key);
    else if (entry.kind === "slider") assert.ok(value >= entry.min && value <= entry.max, entry.key);
  }
});

test("a row reads and writes its setting, in the original's words where it had them", () => {
  const settings = settingsStub();
  const strings = { "ifs.soundopt.mastervol": "MASTER VOLUME", "common.on": "ON", "common.off": "OFF" };
  const master = optionRow(OPTIONS.find((e) => e.key === "masterVolume"), settings, { strings });
  assert.equal(master.label, "MASTER VOLUME");
  assert.equal(master.text(), "80%");
  master.stepBy(1);
  assert.deepEqual(settings.sets.at(-1), ["masterVolume", 0.85]);
  const shadows = optionRow(OPTIONS.find((e) => e.key === "shadows"), settings);
  assert.equal(shadows.text(), "HIGH");
  shadows.stepBy(1);
  assert.deepEqual(settings.sets.at(-1), ["shadows", "ultra"]);
  const aniso = optionRow(OPTIONS.find((e) => e.key === "anisotropy"), settings);
  assert.equal(aniso.text(), "16X ANISOTROPIC");
  aniso.stepBy(1);
  assert.deepEqual(settings.sets.at(-1), ["anisotropy", 1], "numeric choices stay numbers");
  const invert = optionRow(OPTIONS.find((e) => e.key === "invertMouse"), settings);
  assert.equal(invert.text(), "NO");
});

test("mouse sensitivity moves in tenths, by key or along its track", () => {
  const settings = settingsStub();
  const sens = optionRow(OPTIONS.find((e) => e.key === "mouseSensitivity"), settings);
  assert.equal(sens.text(), "5.0");
  sens.stepBy(1);
  assert.deepEqual(settings.sets.at(-1), ["mouseSensitivity", 5.1]);
  assert.equal(sens.text(), "5.1");
  sens.stepBy(-1);
  sens.stepBy(-1);
  assert.deepEqual(settings.sets.at(-1), ["mouseSensitivity", 4.9]);
  sens.setFraction((7.34 - 0.5) / 9.5);
  assert.deepEqual(settings.sets.at(-1), ["mouseSensitivity", 7.3], "a drag snaps to the nearest tenth");
  // A value saved under the old half steps still reads and steps.
  settings.set("mouseSensitivity", 5.5);
  sens.stepBy(1);
  assert.deepEqual(settings.sets.at(-1), ["mouseSensitivity", 5.6]);
});

test("RESET TO DEFAULTS resets that page's settings only", () => {
  const settings = settingsStub({ masterVolume: 0.2, shadows: "off" });
  resetPage("audio", settings);
  assert.equal(settings.get("masterVolume"), DEFAULTS.masterVolume);
  assert.equal(settings.get("shadows"), "off");
  assert.ok(!settings.sets.some(([key]) => key === "physicsRate"));
});

test("OPTIONS opens the four pages, with each page's settings; online and credits only where given", () => {
  const settings = settingsStub();
  const stack = new ScreenStack({ theme: { draw() {}, hit: () => null }, sound: () => {} });
  const menu = optionsMenu({ settings });
  stack.push(menu);
  const rows = menu.widget("options");
  assert.deepEqual(menu.ring().filter((t) => t.widget === rows).map((t) => rows.rows[t.item].id), ["general", "video", "sound", "controls"]);
  const opened = [];
  for (let i = 0; i < 4; i++) {
    menu.focused = { widget: rows, item: i };
    stack.key("Enter");
    opened.push(stack.top.id);
    assert.equal(stack.top.dim, true, "the pages dim what is behind them");
    stack.key("Escape");
    assert.equal(stack.top, menu);
  }
  assert.deepEqual(opened, ["options-game", "options-video", "options-audio", "options-controls"]);
  const shell = optionsMenu({ settings, online: () => {}, credits: () => {} });
  const shellRows = shell.widget("options");
  assert.equal(shellRows.visibleRows().length, 6);
});

test("each page lists its schema entries in order", () => {
  const settings = settingsStub();
  const ids = (screen) => screen.widget("rows").rows.map((row) => row.id);
  const viewpoint = { first: false, get() { return this.first; }, set(v) { this.first = v; } };
  assert.deepEqual(ids(gamePage({ settings, strings: {}, viewpoint })), ["viewpoint", ...optionsFor("game").map((e) => e.key), "reset"]);
  assert.deepEqual(ids(gamePage({ settings, strings: {} })), [...optionsFor("game").map((e) => e.key), "reset"], "no VIEWPOINT outside a battle");
  assert.deepEqual(ids(videoPage({ settings, strings: {} })), optionsFor("video").map((e) => e.key));
  assert.deepEqual(ids(audioPage({ settings, strings: {}, audioPanel: () => {} })), [...optionsFor("audio").map((e) => e.key), "panel"]);
  assert.deepEqual(ids(controlsPage({ settings, strings: {} })), ["mouseSensitivity", "invertMouse"]);
  // VIEWPOINT drives the battle's camera.
  const row = gamePage({ settings, strings: {}, viewpoint }).widget("rows").rows[0];
  row.stepBy(1);
  assert.equal(viewpoint.first, true);
});

test("the controls page: the two rows, then the bindings list; Left and Right change the tab", () => {
  const settings = settingsStub();
  const stack = new ScreenStack({ theme: { draw() {}, hit: () => null }, sound: () => {} });
  const page = controlsPage({ settings, strings: {} });
  stack.push(page);
  const list = page.widget("bindings");
  const tabs = page.widget("tabs");
  assert.equal(list.rowsNow()[0][0], "PRIMARY FIRE:");
  assert.equal(list.rowsNow()[0][1], "MOUSE BUTTON 1");
  stack.key("ArrowDown");
  stack.key("ArrowDown");
  assert.equal(page.focused.widget, list);
  stack.key("ArrowRight");
  assert.equal(tabs.active, 1);
  assert.ok(list.rowsNow().some(([action]) => action === "BRAKE:"), "the VEHICLE tab");
  stack.key("ArrowLeft");
  stack.key("ArrowLeft");
  assert.equal(tabs.active, 2);
  assert.ok(list.rowsNow().some(([action, keys]) => action === "TAKE OFF / LAND:" && keys === "SPACE"), "the STARFIGHTER tab");
});

test("SOUND says when the desktop prepared the game data without the Enhanced sound, and where to add it", async () => {
  const desktop = (enhancedSound) => ({ asked: 0, about() { this.asked++; return Promise.resolve({ prepared: { maps: 16, enhancedSound } }); } });
  const without = desktop(false);
  assert.equal(enhancedSoundNote(without), null, "not known yet");
  await Promise.resolve();
  assert.equal(enhancedSoundNote(without), NOTES.enhancedSound);
  assert.equal(enhancedSoundNote(without), NOTES.enhancedSound);
  assert.equal(without.asked, 1, "asked once");
  const prepared = desktop(true);
  enhancedSoundNote(prepared);
  await Promise.resolve();
  assert.equal(enhancedSoundNote(prepared), null);
  assert.equal(enhancedSoundNote(null), null, "a browser has no desktop to ask");
  assert.equal(enhancedSoundNote({ about: () => Promise.reject(new Error("gone")) }), null);
  // The row asks the page's desktop; the other rows keep their notes.
  const row = optionRow(OPTIONS.find((entry) => entry.key === "audioQuality"), settingsStub());
  assert.equal(typeof row.note, "function");
  assert.equal(row.note(), null, "no desktop in the tests");
  assert.equal(optionRow(OPTIONS.find((entry) => entry.apply === "map"), settingsStub()).note, NOTES.map);
});
