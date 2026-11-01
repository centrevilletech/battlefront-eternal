import test from "node:test";
import assert from "node:assert/strict";

// localStorage for the settings, in memory.
const store = new Map();
globalThis.localStorage = {
  getItem: (key) => (store.has(key) ? store.get(key) : null),
  setItem: (key, value) => store.set(key, String(value)),
  removeItem: (key) => store.delete(key),
};
const { Settings, DEFAULTS } = await import("../src/settings.js");
const KEY = "battlefront-eternal.settings";
const saved = () => JSON.parse(store.get(KEY) || "{}");

test("the Eternal rename migrates old preferences without saving URL overrides", () => {
  store.clear();
  const legacy = JSON.stringify({ masterVolume: 0.25, aiMode: "off", mouseSensitivity: 7 });
  store.set("battlefront-classic.settings", legacy);
  const settings = new Settings({ aiMode: "classic" });
  assert.equal(settings.get("masterVolume"), 0.25);
  assert.equal(settings.get("mouseSensitivity"), 7);
  assert.equal(settings.get("aiMode"), "classic");
  assert.equal(store.get(KEY), legacy);
  settings.set("masterVolume", 0.4);
  assert.equal(saved().masterVolume, 0.4);
  assert.equal(saved().aiMode, "off");
  assert.equal(store.get("battlefront-classic.settings"), legacy);
});

test("Eternal preferences take precedence over the old settings", () => {
  store.clear();
  store.set("battlefront-classic.settings", JSON.stringify({ masterVolume: 0.25 }));
  store.set(KEY, JSON.stringify({ masterVolume: 0.75 }));
  assert.equal(new Settings().get("masterVolume"), 0.75);
});

test("migration keeps preferences usable when storage cannot be written", () => {
  store.clear();
  store.set("battlefront-classic.settings", JSON.stringify({ masterVolume: 0.25 }));
  const setItem = localStorage.setItem;
  localStorage.setItem = () => { throw new Error("storage full"); };
  try {
    assert.equal(new Settings().get("masterVolume"), 0.25);
    assert.equal(store.has(KEY), false);
  } finally {
    localStorage.setItem = setItem;
  }
});

test("URL overrides apply for the visit but aren't saved when a setting changes", () => {
  store.clear();
  store.set(KEY, JSON.stringify({ aiMode: "off", masterVolume: 0.5 }));
  const settings = new Settings({ aiMode: "classic", aiDifficulty: "hard", renderScale: 0.75 });
  assert.equal(settings.get("aiMode"), "classic");
  assert.equal(settings.get("aiDifficulty"), "hard");
  settings.set("masterVolume", 0.6);
  const written = saved();
  assert.equal(written.masterVolume, 0.6);
  assert.equal(written.aiMode, "off", "?ai=classic stays out of the saved settings");
  assert.equal(written.aiDifficulty, DEFAULTS.aiDifficulty);
  assert.equal(written.renderScale, DEFAULTS.renderScale);
  // The next visit without the URL's options gets the saved ones.
  const next = new Settings();
  assert.equal(next.get("aiMode"), "off");
  assert.equal(next.get("masterVolume"), 0.6);
});

test("setting an overridden setting by hand saves it, even to the override's value", () => {
  store.clear();
  const settings = new Settings({ aiMode: "classic" });
  const heard = [];
  settings.onChange((key, value) => heard.push([key, value]));
  settings.set("aiMode", "classic");
  assert.equal(saved().aiMode, "classic", "the player's own choice");
  assert.deepEqual(heard, [], "no change to apply");
  settings.set("aiMode", "off");
  assert.equal(saved().aiMode, "off");
  assert.deepEqual(heard, [["aiMode", "off"]]);
});

test("a preset sets its values and saves them; the overrides still aren't saved", () => {
  store.clear();
  const settings = new Settings({ shadows: "ultra" });
  settings.set("performancePreset", "performance");
  const written = saved();
  assert.equal(written.performancePreset, "performance");
  assert.equal(written.shadows, "off", "the preset's own value, set by it");
  assert.equal(written.renderScale, 0.75);
  settings.set("bloom", true);
  assert.equal(saved().performancePreset, "custom");
});

test("the simulation rate stays 60 Hz", () => {
  store.clear();
  const settings = new Settings({ physicsRate: "30" });
  assert.equal(settings.get("physicsRate"), "60");
  settings.set("physicsRate", "120");
  assert.equal(settings.get("physicsRate"), "60");
});

// The desktop app's file (electron/player-settings.js), round-tripped through
// its own module as the main process stores it. shadowed: an update's older
// version still holds localStorage, which reads empty (a tester's report: updates
// reset the profile to PLAYER 1).
async function desktopFile() {
  const { mkdtempSync, rmSync } = await import("node:fs");
  const os = await import("node:os"), path = await import("node:path");
  const { readPlayerSettings, writePlayerSettings } = await import("../electron/player-settings.js");
  const dir = mkdtempSync(path.join(os.tmpdir(), "bfe-settings-"));
  let shadowed = false;
  return {
    store: { read: () => ({ record: readPlayerSettings(dir), shadowed }), write: (record) => writePlayerSettings(dir, record) },
    record: () => readPlayerSettings(dir),
    shadow(value) { shadowed = value; },
    dispose: () => rmSync(dir, { recursive: true, force: true }),
  };
}

test("the desktop app keeps the profile through an update that hides localStorage", async () => {
  const file = await desktopFile();
  try {
    store.clear();
    store.set(KEY, JSON.stringify({ playerName: "KEVIN", masterVolume: 0.3 }));
    // The first session with the file moves localStorage's settings in.
    new Settings({}, { store: file.store });
    assert.equal(file.record().complete, true);
    assert.equal(file.record().values.playerName, "KEVIN");
    // An update starts the new version beside the old one: localStorage empty.
    store.clear(); file.shadow(true);
    const updated = new Settings({}, { store: file.store });
    assert.equal(updated.get("playerName"), "KEVIN");
    updated.set("masterVolume", 0.6);
    file.shadow(false);
    const next = new Settings({}, { store: file.store });
    assert.deepEqual([next.get("playerName"), next.get("masterVolume")], ["KEVIN", 0.6], "and what that session changed stays");
  } finally { file.dispose(); }
});

test("the update that brings the file keeps localStorage's settings and that session's changes", async () => {
  const file = await desktopFile();
  try {
    store.clear();
    const before = JSON.stringify({ playerName: "KEVIN", masterVolume: 0.3 });
    // No file yet, started by an update: localStorage reads empty.
    file.shadow(true);
    const updated = new Settings({}, { store: file.store });
    assert.equal(updated.get("playerName"), DEFAULTS.playerName);
    updated.set("mouseSensitivity", 4);
    assert.deepEqual(file.record(), { complete: false, values: { mouseSensitivity: 4 } }, "only the player's own change, never defaults over his settings");
    // The next launch sees localStorage again: his settings, that change on top.
    store.set(KEY, before); file.shadow(false);
    const next = new Settings({}, { store: file.store });
    assert.deepEqual([next.get("playerName"), next.get("masterVolume"), next.get("mouseSensitivity")], ["KEVIN", 0.3, 4]);
    assert.equal(file.record().complete, true);
  } finally { file.dispose(); }
});

test("a browser has no file and keeps localStorage alone", () => {
  store.clear();
  const settings = new Settings({}, { store: null });
  settings.set("playerName", "KEVIN");
  assert.equal(saved().playerName, "KEVIN");
});

test("a first session on an old GPU starts on the Minimum preset, once", async () => {
  const { PERFORMANCE_PRESETS } = await import("../src/settings.js");
  const hd2000 = "ANGLE (Intel, Mesa Intel(R) HD Graphics 2000 (SNB GT1), OpenGL 3.3 (Core Profile) Mesa 22.3.6)";
  store.clear();
  const first = new Settings();
  assert.equal(first.fresh, true);
  assert.equal(first.adoptGpuPreset(hd2000), true);
  assert.equal(first.get("performancePreset"), "minimum");
  for (const [key, value] of Object.entries(PERFORMANCE_PRESETS.minimum.values)) assert.equal(first.get(key), value, key);
  assert.equal(saved().performancePreset, "minimum");
  // Only the first time it asks.
  first.set("renderScale", 1);
  assert.equal(first.adoptGpuPreset(hd2000), false);
  assert.equal(first.get("renderScale"), 1);
  // Settings stored: the player's, whatever the GPU.
  const later = new Settings();
  assert.equal(later.fresh, false);
  assert.equal(later.adoptGpuPreset(hd2000), false);
  assert.equal(later.get("renderScale"), 1);
  // A current GPU keeps the defaults.
  store.clear();
  const modern = new Settings();
  assert.equal(modern.adoptGpuPreset("ANGLE (NVIDIA, NVIDIA GeForce GTX 1060 Direct3D11 vs_5_0 ps_5_0, D3D11)"), false);
  assert.equal(modern.get("renderScale"), DEFAULTS.renderScale);
  assert.equal(modern.get("upscaler"), DEFAULTS.upscaler);
});
