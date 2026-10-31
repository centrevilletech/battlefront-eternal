import { ANTIALIASING, BATTLE_SIZES, DEFAULTS, PERFORMANCE_PRESETS, RENDER_SCALE_MIN, SHADOW_PRESETS, VIEW_DISTANCES, WATER_QUALITIES } from "../settings.js";
import { AUDIO_DYNAMICS, AUDIO_OUTPUTS, AUDIO_QUALITIES } from "../audio/options.js";
import { UPSCALERS } from "../upscaler.js";
import { OptionRow } from "./widgets.js";
import { MAX_TEAM_SIZE } from '../battle-limits.js';

// Every setting of this port (src/settings.js, src/audio/options.js), one
// entry each, laid out on the 2004 game's four option pages: GAME, VIDEO,
// AUDIO and CONTROL. The look is the original's; the content is the rebuilt
// game's.
//
// An entry: key (the setting), page, label (and labelKey, the original's
// string where it had one), kind ("choice", "toggle", "slider", "fixed"),
// its choices or range, and apply: when a change takes effect:
// - "live": at once;
// - "map": when a map next loads (compressed textures);
// - "battle": when a battle starts (the AI's mode, difficulty and heroes
//   are read once); RESTART MISSION brings it in;
// - "fixed": it can't be changed (the 60 Hz world).
// url: the URL key that overrides it for one visit (src/game.js), which
// RESTART MISSION rewrites when the setting is changed.
//
// A new setting needs its entry here: tests/menus-options.test.js checks
// that every one of DEFAULTS is on a page.

export const PAGES = [
  { id: "game", title: "GAME OPTIONS", titleKey: "ifs.GameOpt.title" },
  { id: "video", title: "VIDEO OPTIONS", titleKey: "ifs.VideoOpt.title" },
  { id: "audio", title: "AUDIO OPTIONS", titleKey: "ifs.SoundOpt.title" },
  { id: "controls", title: "CONTROL OPTIONS", titleKey: "ifs.GameOpt.pc_optionstitle" },
];

const upper = (text) => String(text).toUpperCase();
// The 3D view's size at a render scale, in the window as it is now (as
// graphics.js applyScale sizes it): what the RENDER SCALE row's note says.
export function renderResolution(scale, view = typeof window === "undefined" ? null : window) {
  if (!view) return null;
  const ratio = Math.min(view.devicePixelRatio || 1, 2);
  const width = Math.max(1, Math.round(view.innerWidth * ratio)), height = Math.max(1, Math.round(view.innerHeight * ratio));
  const s = Number(scale) || 1;
  return { width: Math.max(1, Math.round(width * s)), height: Math.max(1, Math.round(height * s)), windowWidth: width, windowHeight: height };
}
const scaleNote = (scale) => {
  const size = renderResolution(scale);
  return size ? `3D VIEW ${size.width}X${size.height}, WINDOW ${size.windowWidth}X${size.windowHeight}` : null;
};
// What each upscaler costs, for the row's note: the window-sized passes are
// what weak GPUs can't afford at full screen.
const UPSCALER_NOTES = {
  off: "STRETCHES THE WHOLE PICTURE, HUD INCLUDED: THE CHEAPEST",
  bilinear: "STRETCHES THE 3D VIEW, HUD STAYS SHARP: ALMOST AS CHEAP AS OFF",
  spatial: "SHARPER STRETCH: ONE EXTRA PASS AT WINDOW SIZE",
  temporal: "REBUILDS DETAIL OVER FRAMES: SEVERAL PASSES AT WINDOW SIZE, HEAVY ON OLD GPUS",
};
const CLASSIC = [["classic", "CLASSIC"], ["enhanced", "ENHANCED"]];
const ORIGINAL = [["original", "CLASSIC"], ["enhanced", "ENHANCED"]];
const FPS = (none) => [["0", none], ...[30, 60, 120, 144, 240].map((fps) => [String(fps), `${fps} FPS`])];
const percent = (v) => `${Math.round(Number(v) * 100)}%`;
const SHORT = {
  auto: "AUTOMATIC", headphones: "HEADPHONES", stereo: "STEREO", "5.1": "5.1", "7.1": "7.1", "7.1.4": "7.1.4",
  home: "HOME THEATER", normal: "NORMAL", night: "NIGHT",
};

export const OPTIONS = [
  // GAME OPTIONS: the original's FRIENDLY FIRE, INSTANT ACTION HEROES and
  // DIFFICULTY, then this port's own.
  { key: "flightFirstPerson", page: "game", label: "STARFIGHTER COCKPIT VIEW", kind: "toggle", apply: "live" },
  { key: "keyBindings", page: "controls", label: "KEY BINDINGS", kind: "fixed", apply: "live" },
  { key: "flightBindings", page: "controls", label: "STARFIGHTER BINDINGS", kind: "fixed", apply: "live" },
  { key: "flightSensitivity", page: "controls", label: "STARFIGHTER SENSITIVITY", kind: "slider", apply: "live", min: .2, max: 3, step: .1 },
  { key: "flightInvertY", page: "controls", label: "STARFIGHTER INVERT Y", kind: "toggle", apply: "live" },
  { key: "flightResponseCurve", page: "controls", label: "STARFIGHTER RESPONSE", kind: "slider", apply: "live", min: 1, max: 3, step: .1 },
  { key: "friendlyFire", page: "game", label: "FRIENDLY FIRE", labelKey: "ifs.gameopt.friendlyfire", kind: "toggle", apply: "live" },
  // The port's own: how the player's vehicle steers and is seen (docs/vehicles.md).
  { key: "aiHeroes", page: "game", label: "INSTANT ACTION HEROES", labelKey: "ifs.gameopt.hero", kind: "toggle", apply: "battle", url: "heroes" },
  { key: "aiDifficulty", page: "game", label: "DIFFICULTY", labelKey: "ifs.gameopt.diff", kind: "choice", apply: "battle", url: "tier",
    choices: [["easy", "EASY", "ifs.difficulty.easy"], ["medium", "NORMAL"], ["hard", "ELITE"]] },
  // Classic: the 2004 game's AI as recovered; Enhanced: the same bots with
  // equal teams, objective focus and fairer aim (docs/ai-enhanced.md).
  { key: "aiMode", page: "game", label: "BATTLE AI", kind: "choice", apply: "battle", url: "ai", choices: [["off", "OFF", "common.off"], ["classic", "CLASSIC"], ["enhanced", "ENHANCED"]] },
  { key: "battleSize", page: "game", label: `BATTLE SIZE (MAX ${MAX_TEAM_SIZE} PER TEAM)`, kind: "choice", apply: "battle", url: "size",
    choices: Object.entries(BATTLE_SIZES).map(([k, size]) => [k, { full: "FULL", large: "REDUCED", small: "SMALL" }[k] || upper(size.label)]) },
  { key: "showStats", page: "game", label: "STATS LINE", kind: "toggle", apply: "live" },
  { key: "physicsRate", page: "game", label: "SIMULATION RATE", kind: "fixed", apply: "fixed", display: "60 HZ" },

  // VIDEO OPTIONS: the preset and pacing first, then the frame, then the
  // effects that have a Classic and an Enhanced look.
  { key: "performancePreset", page: "video", label: "QUALITY PRESET", kind: "choice", apply: "live",
    choices: [["custom", "CUSTOM"], ...Object.entries(PERFORMANCE_PRESETS).map(([k, preset]) => [k, upper(preset.label).replace(/\s*\(.*\)$/, "")])] },
  { key: "frameLimit", page: "video", label: "FRAME LIMIT", kind: "choice", apply: "live", choices: FPS("DISPLAY") },
  { key: "dynamicResolution", page: "video", label: "ADAPTIVE RESOLUTION", kind: "choice", apply: "live", choices: FPS("OFF") },
  { key: "renderScale", page: "video", label: "RENDER SCALE", kind: "slider", apply: "live", url: "scale", min: RENDER_SCALE_MIN, max: 2, step: 0.05,
    format: (v) => `${Number(v).toFixed(2)}X`, note: (settings) => scaleNote(settings.get("renderScale")) },
  { key: "upscaler", page: "video", label: "UPSCALER", kind: "choice", apply: "live",
    choices: Object.keys(UPSCALERS).map((k) => [k, { off: "OFF", bilinear: "BILINEAR", spatial: "SPATIAL", temporal: "TEMPORAL" }[k] || upper(UPSCALERS[k])]),
    note: (settings) => UPSCALER_NOTES[settings.get("upscaler")] ?? null },
  { key: "upscaleSharpness", page: "video", label: "SHARPENING", kind: "slider", apply: "live", min: 0, max: 1, step: 0.05, format: percent },
  { key: "antialiasing", page: "video", label: "ANTIALIASING", labelKey: "ifs.videopt.multisample", kind: "choice", apply: "live",
    choices: Object.keys(ANTIALIASING).map((k) => [k, k === "off" ? "OFF" : k === "fxaa" ? "FXAA" : `${upper(k)} MSAA`]) },
  { key: "shadows", page: "video", label: "SHADOW QUALITY", labelKey: "ifs.videopt.shadows", kind: "choice", apply: "live", url: "shadows",
    choices: Object.keys(SHADOW_PRESETS).map((k) => [k, upper(k)]) },
  { key: "anisotropy", page: "video", label: "TEXTURE FILTERING", kind: "choice", apply: "live",
    choices: [1, 2, 4, 8, 16].map((n) => [n, n === 1 ? "TRILINEAR" : `${n}X ANISOTROPIC`]) },
  { key: "textureCompression", page: "video", label: "COMPRESSED TEXTURES", kind: "toggle", apply: "map" },
  { key: "waterQuality", page: "video", label: "WATER QUALITY", labelKey: "ifs.videoopt.waterquality", kind: "choice", apply: "live",
    choices: Object.keys(WATER_QUALITIES).map((k) => [k, upper(k)]) },
  { key: "viewDistance", page: "video", label: "VIEW DISTANCE", labelKey: "ifs.videopt.viewdistance", kind: "choice", apply: "live",
    choices: Object.entries(VIEW_DISTANCES).map(([k, v]) => [k, upper(v.label)]) },
  { key: "farTerrain", page: "video", label: "FAR TERRAIN", kind: "choice", apply: "live", choices: [["full", "FULL"], ["reduced", "REDUCED"]] },
  { key: "bumpMapping", page: "video", label: "BUMPMAPPING", labelKey: "ifs.videopt.bumpmapping", kind: "toggle", apply: "live" },
  { key: "softFocus", page: "video", label: "SOFT FOCUS", kind: "toggle", apply: "live" },
  { key: "bloom", page: "video", label: "BLOOM AND GLOW", kind: "toggle", apply: "live" },
  { key: "vehicleMotionBlur", page: "video", label: "MOTION BLUR", labelKey: "ifs.videoopt.motionblur", kind: "toggle", apply: "live" },
  { key: "softParticles", page: "video", label: "SOFT PARTICLES", kind: "toggle", apply: "live" },
  { key: "lampLighting", page: "video", label: "LAMP LIGHT ON CHARACTERS", kind: "toggle", apply: "live" },
  { key: "terrainDetail", page: "video", label: "TERRAIN", kind: "choice", apply: "live", choices: ORIGINAL },
  { key: "treeDetail", page: "video", label: "TREES", kind: "choice", apply: "live", choices: ORIGINAL },
  { key: "environmentEffects", page: "video", label: "WATER AND STEAM", kind: "choice", apply: "live", choices: CLASSIC },
  { key: "oceanEffects", page: "video", label: "OCEAN AND SKY", kind: "choice", apply: "live", choices: CLASSIC },
  { key: "rainEffects", page: "video", label: "RAIN", kind: "choice", apply: "live", choices: CLASSIC },
  { key: "boltEffects", page: "video", label: "BLASTER BOLTS", kind: "choice", apply: "live", choices: CLASSIC },
  { key: "arcCasterEffects", page: "video", label: "BOLT CASTER", kind: "choice", apply: "live", choices: CLASSIC },
  { key: "sniperEffects", page: "video", label: "SNIPER SHOTS", kind: "choice", apply: "live", choices: CLASSIC },
  { key: "jetpackEffects", page: "video", label: "JETPACKS", kind: "choice", apply: "live", choices: CLASSIC },
  { key: "droidekaEffects", page: "video", label: "DROIDEKAS", kind: "choice", apply: "live", choices: CLASSIC },
  { key: "destructionEffects", page: "video", label: "DESTRUCTION", kind: "choice", apply: "live", choices: CLASSIC },
  { key: "deathCamera", page: "video", label: "DEATH CAMERA", kind: "choice", apply: "live", choices: CLASSIC },

  // AUDIO OPTIONS: the volumes as the original's sliders, then the rebuilt
  // audio's own.
  { key: "masterVolume", page: "audio", label: "MASTER VOLUME", labelKey: "ifs.soundopt.mastervol", kind: "slider", apply: "live", min: 0, max: 1, step: 0.05, format: percent },
  { key: "musicVolume", page: "audio", label: "MUSIC VOLUME", labelKey: "ifs.soundopt.musicvol", kind: "slider", apply: "live", min: 0, max: 1, step: 0.05, format: percent },
  { key: "effectsVolume", page: "audio", label: "SFX VOLUME", labelKey: "ifs.soundopt.sfxvol", kind: "slider", apply: "live", min: 0, max: 1, step: 0.05, format: percent },
  { key: "voiceVolume", page: "audio", label: "SPEECH VOLUME", labelKey: "ifs.soundopt.speechvol", kind: "slider", apply: "live", min: 0, max: 1, step: 0.05, format: percent },
  { key: "ambienceVolume", page: "audio", label: "AMBIENCE VOLUME", kind: "slider", apply: "live", min: 0, max: 1, step: 0.05, format: percent },
  { key: "interfaceVolume", page: "audio", label: "INTERFACE VOLUME", kind: "slider", apply: "live", min: 0, max: 1, step: 0.05, format: percent },
  { key: "audioQuality", page: "audio", label: "SOUND", kind: "choice", apply: "live", note: () => enhancedSoundNote(),
    choices: AUDIO_QUALITIES.map(([k]) => [k, k === "enhanced" ? "ENHANCED" : "CLASSIC"]) },
  { key: "audioOutput", page: "audio", label: "OUTPUT MODE", labelKey: "ifs.soundopt.outputmode.title", kind: "choice", apply: "live",
    choices: AUDIO_OUTPUTS.map(([k, label]) => [k, SHORT[k] || upper(label)]) },
  { key: "audioDynamics", page: "audio", label: "DYNAMICS", kind: "choice", apply: "live",
    choices: AUDIO_DYNAMICS.map(([k, label]) => [k, SHORT[k] || upper(label)]) },
  { key: "audioMono", page: "audio", label: "MONO", labelKey: "ifs.soundopt.outputmode.mono", kind: "toggle", apply: "live" },
  { key: "audioClarity", page: "audio", label: "CLARITY", kind: "toggle", apply: "live" },

  // CONTROL OPTIONS: the original's two rows over the bindings. Mouse
  // sensitivity moves in tenths, each about 1.4% on the turn rate
  // (Settings.mouseScale), fine enough to tune for online play.
  { key: "mouseSensitivity", page: "controls", label: "MOUSE SENSITIVITY", labelKey: "ifs.GameOpt.pc_mousesens", kind: "slider", apply: "live", url: "sens",
    min: 0.5, max: 10, step: 0.1, format: (v) => Number(v).toFixed(1) },
  { key: "invertMouse", page: "controls", label: "INVERT Y AXIS", labelKey: "ifs.gameopt.pc_invert", kind: "toggle", apply: "live", url: "invert", labels: ["NO", "YES"], labelKeys: ["common.no", "common.yes"] },
];

export const OPTION_KEYS = new Set(OPTIONS.map((entry) => entry.key));
// Settings PROFILE MANAGEMENT keeps, not the OPTIONS pages.
export const PROFILE_KEYS = new Set(["playerName"]);
// Settings the front end's look picker sets (src/menus/themes/page).
export const THEME_KEYS = new Set(["menuTheme"]);

// The entries of a page, in order.
export function optionsFor(page) {
  return OPTIONS.filter((entry) => entry.page === page);
}

export function optionEntry(key) {
  return OPTIONS.find((entry) => entry.key === key) || null;
}

// When a change to a setting takes effect: "live", "map", "battle" or
// "fixed".
export function applies(key) {
  return optionEntry(key)?.apply ?? "live";
}

// The notes shown under a page while a row is focused, for the changes
// that don't take effect at once.
export const NOTES = {
  map: "TAKES EFFECT WHEN A MAP NEXT LOADS",
  battle: "TAKES EFFECT WHEN A BATTLE STARTS (RESTART MISSION)",
  battleShell: "TAKES EFFECT WHEN A BATTLE STARTS",
  fixed: "THE BATTLE ALWAYS RUNS AT 60 HZ",
  enhancedSound: "ENHANCED SOUND ISN'T PREPARED: GAME MENU (ALT) > GAME DATA PREPARATION ADDS IT",
};

// The SOUND row's note when the desktop client prepared the game data
// without the Enhanced sound (the player's choice there): ENHANCED keeps the
// 2026 sound stage over the original recordings, and the loader's ADD
// ENHANCED SOUND prepares them. Asked of the desktop once; null meanwhile.
const preparedOf = new WeakMap();
export function enhancedSoundNote(desktop = globalThis.bfeDesktop) {
  if (typeof desktop?.about !== "function") return null;
  if (!preparedOf.has(desktop)) {
    preparedOf.set(desktop, null);
    desktop.about().then((about) => preparedOf.set(desktop, about?.prepared ?? null), () => {});
  }
  return preparedOf.get(desktop)?.enhancedSound === false ? NOTES.enhancedSound : null;
}

// An entry as an option row over a Settings: its label and choices in the
// strings given (common.json's), else English. inBattle: the notes say so.
export function optionRow(entry, settings, { strings = {}, inBattle = false } = {}) {
  const say = (key, fallback) => (key && strings[key]) || fallback;
  const note = typeof entry.note === "function" ? () => entry.note(settings)
    : entry.note ?? (entry.apply === "map" ? NOTES.map : entry.apply === "battle" ? (inBattle ? NOTES.battle : NOTES.battleShell) : entry.apply === "fixed" ? NOTES.fixed : null);
  return new OptionRow({
    id: entry.key,
    key: entry.key,
    kind: entry.kind,
    apply: entry.apply,
    label: say(entry.labelKey, entry.label),
    choices: (entry.choices || []).map(([value, label, key]) => [value, say(key, label)]),
    labels: entry.labels ? entry.labels.map((label, i) => say(entry.labelKeys?.[i], label)) : [say("common.off", "OFF"), say("common.on", "ON")],
    min: entry.min,
    max: entry.max,
    step: entry.step,
    format: entry.format,
    display: entry.display,
    note,
    get: () => settings.get(entry.key),
    set: (value) => settings.set(entry.key, value),
  });
}

// Every setting's default, for RESET TO DEFAULTS on one page.
export function resetPage(page, settings) {
  for (const entry of optionsFor(page)) {
    if (entry.apply === "fixed") continue;
    settings.set(entry.key, DEFAULTS[entry.key]);
  }
}
